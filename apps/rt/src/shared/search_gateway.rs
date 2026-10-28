//! Shared gateway for CLI and host adapters; never evaluates shell request text.
use mustard_core::domain::code_search::{Invocation, Request};
use mustard_core::domain::model::contract::{Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::scan::{NativePass, Scan, ScanReport};
use serde_json::{Value, json};
use mustard_core::io::map_db::Later;
use std::path::Path;
use std::time::{Duration, Instant};

/// A variável em que o gancho do Bash entrega ao comando nativo a identidade
/// do agente que o pediu; a sessão segue nas variáveis da sessão
/// ([`super::spec_state::shell_export`]).
const AGENT_VAR: &str = "MUSTARD_AGENT_ID";

pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Entrega ao comando nativo a identidade do subagente cujo gancho o liberou.
pub(crate) fn agent_export(agent: &str) -> String {
    format!("export {AGENT_VAR}={}\n", quote(agent))
}

/// O subagente que o gancho nomeou para este processo, por [`agent_export`].
pub(crate) fn agent_from_env() -> Option<String> {
    std::env::var(AGENT_VAR).ok().map(|id| id.trim().to_string()).filter(|id| !id.is_empty())
}

/// O prazo de cada busca para tudo o que não é a resposta em si: a passada
/// do mapa, a espera da trava para gravar o que ela aprendeu, os vetores de
/// código e o Jev. Medido de 14 a 18/10: dentro do processo, a busca que só
/// localiza levava 0,3 s, e a com finalidade, 15,7 s na mediana e até 43 s,
/// mesmo sem o Jev. O tempo ia em esperas: a passada do mapa (até 5 s, duas
/// vezes por busca), a trava do mapa que as cópias das ondas dividem (até 5 s
/// por gravação, quatro gravações por busca) e o Jev (até 10 s por chamada).
/// A passada que relê um arquivo leva de 18 a 47 s neste repositório: esperar
/// por ela não paga. Com 3 s para tudo isso, projeto pequeno ainda atualiza
/// dentro do prazo; passado ele, a busca responde com o mapa de antes e diz
/// isso, a passada segue sozinha, e a próxima busca a aproveita.
const SEARCH_BUDGET: Duration = Duration::from_secs(3);

pub(crate) fn answer(
    cwd: &Path,
    request: &Request,
) -> Result<mustard_core::io::code_search::Answer, String> {
    answer_with(cwd, request, &Scan::locate(), SEARCH_BUDGET)
}

/// O que sobra do prazo da busca que acaba em `deadline`.
fn left(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

/// Quanto cada etapa da busca levou, para o relato e o registro de operações.
#[derive(Default)]
struct Timings {
    scan_wait: Duration,
    lock_wait: Duration,
    vectors: Duration,
    investigate: Duration,
    jev: Duration,
}

impl Timings {
    fn report(&self) -> Value {
        json!({"scan_wait_ms":self.scan_wait.as_millis(),"lock_wait_ms":self.lock_wait.as_millis(),"vectors_ms":self.vectors.as_millis(),
            "investigate_ms":self.investigate.as_millis(),"jev_ms":self.jev.as_millis()})
    }
}

/// A passada nativa do mapa de `tree`, esperada até `wait`: o relato quando
/// ela acabou, ou o estado que a busca registra no lugar dele. O tempo da
/// espera soma em `timings`.
fn refresh_map(scan: &Scan, tree: &Path, wait: Duration, timings: &mut Timings) -> Result<ScanReport, Value> {
    let started = Instant::now();
    let pass = scan.scan_native_within(tree, &mustard_core::io::project_map::model_path(tree), wait);
    timings.scan_wait += started.elapsed();
    match pass {
        Ok(NativePass::Done(report)) => Ok(report),
        Ok(NativePass::Running) => Err(json!({"status":"in-progress","waited_ms":wait.as_millis(),"local_model_calls":0,"remote_model_calls":0})),
        Err(error) => Err(json!({"status":"pending","reason":error.to_string()})),
    }
}

/// Diz a quem buscou, na saída de erro, que o mapa segue em atualização: a
/// saída da busca continua a mesma, e o aviso chega também a quem a lê crua.
fn tell_map_running(root: &Path, answer: &mut mustard_core::io::code_search::Answer) {
    if answer.report["learning"]["scan"]["status"] == "in-progress" {
        let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
        answer.stderr.extend_from_slice(super::say::say("search.gateway.scan_running", lang, &[]).as_bytes());
    }
}

/// O fim de toda busca: grava de uma vez o que ela aprendeu, esperando a
/// trava pouco e só dentro do prazo, e põe no relato quanto cada etapa levou.
fn finish(root: &Path, answer: &mut mustard_core::io::code_search::Answer, later: Later, deadline: Instant, mut timings: Timings) {
    let kept = later.keep(mustard_core::io::knowledge::observations::LEARNING_WAIT.min(left(deadline)));
    timings.lock_wait = kept.lock_wait;
    mustard_core::io::code_search::settle(answer, &kept);
    answer.report["timings"] = timings.report();
    tell_map_running(root, answer);
}

/// A busca de `request` em `cwd`. Tudo o que não é a resposta cabe em
/// `budget`: a busca pede no máximo uma passada do mapa e investiga uma vez
/// só, no mapa que houver quando o prazo acaba.
fn answer_with(
    cwd: &Path,
    request: &Request,
    scan: &Scan,
    budget: Duration,
) -> Result<mustard_core::io::code_search::Answer, String> {
    let deadline = Instant::now() + budget;
    let mut timings = Timings::default();
    request.validate()?;
    let cwd = cwd.canonicalize().map_err(|e| e.to_string())?;
    let root = mustard_core::io::spec_events::spec_root(&cwd);
    let git = mustard_core::platform::git::run(&cwd, &["rev-parse", "--show-toplevel"]);
    let tree = if git.ok {
        std::path::PathBuf::from(git.stdout.trim())
    } else {
        root.clone()
    };
    let tree = tree.canonicalize().map_err(|e| e.to_string())?;
    guard(&root, &cwd, request)?;
    let mut later = Later::default();
    if mustard_core::io::code_search::operations::requested(request) {
        let index_root = if mustard_core::io::project_map::model_path(&tree).is_file() {
            &tree
        } else {
            &root
        };
        use mustard_core::io::code_search::operations;
        let asked = Instant::now();
        let mut answer = operations::execute(index_root, &tree, &cwd, request, &mut later)?;
        timings.investigate += asked.elapsed();
        let learning = answer.report["learning"].clone();
        if learning["needs_scan"] == true {
            match refresh_map(scan, &tree, left(deadline), &mut timings) {
                Ok(report) => {
                    let saved = mustard_core::io::knowledge::observations::acknowledge(
                        index_root, &tree, &learning, &mut later,
                    );
                    // A pergunta que o bloco velho deixou sem resposta se
                    // repete no mapa que a passada pôs em dia; a das outras
                    // operações só ganha os donos.
                    let asked = Instant::now();
                    if operations::awaits_pass(&answer) {
                        answer = operations::execute(index_root, &tree, &cwd, request, &mut later)?;
                    } else {
                        operations::recross(&tree, &tree, &mut answer)?;
                    }
                    timings.investigate += asked.elapsed();
                    answer.report["learning"]["scan"] = json!({"status":"refreshed-native","files_read":report.read.len(),"acknowledged":saved.is_ok(),"local_model_calls":0,"remote_model_calls":0});
                    if saved.is_ok() {
                        answer.report["learning"]["needs_scan"] = json!(false);
                    }
                }
                Err(status) => answer.report["learning"]["scan"] = status,
            }
        }
        answer.report["choice_gate"]=json!("native-operation");
        finish(&root, &mut answer, later, deadline, timings);
        return Ok(answer);
    }
    let selector = if request.choose && !request.intent.trim().is_empty()
        && request.tool!="Read" && request.purpose!=mustard_core::domain::knowledge::investigation::Purpose::Locate {
        super::knowledge_selection::Budgeted::configured(&root, deadline)
    } else {
        None
    };
    let mut answer = mustard_core::io::code_search::execute_native(&cwd, request)?;
    mustard_core::io::code_search::observe(&root,&tree,&cwd,request,&mut answer,&mut later);
    let learning = answer.report["learning"].clone();
    let mut index_root = root.clone();
    if learning["needs_scan"] == true {
        // A linked checkout gets its own structural snapshot. Its observations
        // remain keyed by checkout in the anchor; main's scan is never replaced.
        match refresh_map(scan, &tree, left(deadline), &mut timings) {
            Ok(report) => {
                index_root.clone_from(&tree);
                let saved =
                    mustard_core::io::knowledge::observations::acknowledge(&root, &tree, &learning, &mut later);
                answer.report["learning"]["scan"] = json!({"status":"refreshed-native","files_read":report.read.len(),"acknowledged":saved.is_ok(),"local_model_calls":0,"remote_model_calls":0});
                if saved.is_ok() {
                    answer.report["learning"]["needs_scan"] = json!(false);
                    answer.report["learning"]["scanned_paths"] = learning["pending_paths"].clone();
                    answer.report["learning"]["pending_paths"] = json!([]);
                }
            }
            Err(status) => answer.report["learning"]["scan"] = status,
        }
    } else if tree != root && mustard_core::io::project_map::model_path(&tree).is_file() {
        index_root.clone_from(&tree);
    }
    // Uma investigação só, no mapa que houver agora: o arquivo que ela achar
    // fora do pedido e o mapa ainda não conhece fica para a passada que ela
    // pedir, e a próxima busca o aproveita.
    let asked = Instant::now();
    mustard_core::io::code_search::enrich(
            &index_root,
            &tree,
            &cwd,
            request,
            &mut answer,
            selector
                .as_ref()
                .map(|s| s as &dyn mustard_core::domain::knowledge::selection::SymbolSelector),
            &mut later,
        );
    leave_choice_to_agent(&root, &mut answer, selector.is_some());
    timings.jev = selector.as_ref().map_or(Duration::ZERO, super::knowledge_selection::Budgeted::spent);
    timings.investigate += asked.elapsed().saturating_sub(timings.jev);
    let asked = Instant::now();
    if let Some(refresh)=refresh_code_vectors(&index_root,request,deadline,&mut later) {
        let computed=refresh["computed_vectors"].as_u64().unwrap_or(0);
        answer.report["local_index_refresh"]=json!({"attempts":[refresh],"computed_vectors":computed,"remote_model_calls":0});
    }
    timings.vectors = asked.elapsed();
    answer.report["choice_gate"]=json!(choice_gate(&root,request,selector.is_some()));
    finish(&root, &mut answer, later, deadline, timings);
    Ok(answer)
}

/// A busca que pediu a escolha ao Jev ligado (`asked_jev`) e ficou sem ela —
/// o Jev falhou, ou o prazo único da busca venceu — diz ao agente que buscou,
/// numa linha do catálogo no idioma do projeto `root`, que a escolha entre os
/// candidatos que a resposta lista é dele. A busca não tem reserva no modelo
/// pequeno: quem buscou já é um agente e decide.
fn leave_choice_to_agent(root: &Path, answer: &mut mustard_core::io::code_search::Answer, asked_jev: bool) {
    if !asked_jev {
        return;
    }
    // A escolha da investigação da tarefa e a do cruzamento das ocorrências.
    for part in ["task_context", "evidence"] {
        let missed = matches!(answer.report[part]["selection"]["status"].as_str(), Some("native-fallback" | "native; search-budget-spent"));
        if missed {
            let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
            answer.report[part]["selection"]["choice_left"] = json!(mustard_core::platform::i18n::translate("map.search_choice_left", lang));
        }
    }
}

fn choice_gate(root:&Path,request:&Request,provider:bool)->&'static str {
    use mustard_core::domain::knowledge::investigation::Purpose;
    if request.tool=="Read" {return "exact-read";}
    if request.purpose==Purpose::Locate {return "locate";}
    if !request.choose {return "not-requested";}
    if request.intent.trim().is_empty() {return "missing-intent";}
    if provider {return "eligible";}
    // O ajuste que falta sai da mesma leitura que liga o Jev. A chave não é
    // lida aqui: com o Jev e a finalidade ligados, o filtro que não montou
    // fica indisponível.
    let config=mustard_core::ProjectConfig::load(root);
    match mustard_core::io::jev_gate::purpose_key(&config,"search",||Ok(())) {
        Ok(())=>"provider-unavailable",
        Err(missing)=>missing.reason(),
    }
}

/// Os vetores de código do que mudou no mapa de `root`, calculados até o fim
/// do prazo da busca e gravados com o resto do que ela aprendeu: valem para a
/// próxima. A passada do scan que segue sozinha os calcula depois de gravar o
/// mapa: com ela calculando, a busca não espera nem calcula de novo.
fn refresh_code_vectors(root:&Path,request:&Request,deadline:Instant,later:&mut Later)->Option<serde_json::Value> {
    use mustard_core::domain::knowledge::investigation::Purpose;
    if request.purpose==Purpose::Locate || request.tool=="Read" || request.intent.trim().is_empty()
        || !mustard_core::ProjectConfig::load(root).ai_vectors_enabled() {return None;}
    let path=mustard_core::io::project_map::model_path(root);
    if !path.is_file() {return None;}
    if left(deadline).is_zero() {
        return Some(json!({"status":"search-budget-spent; left to the next search or the scan","computed_vectors":0,"remote_model_calls":0}));
    }
    Some(match mustard_core::io::code_vectors::fill_later(&path,root,deadline,later) {
        Ok(Some((count,cut)))=>json!({"status":"current-local-code-index","computed_vectors":count,"budget_cut":cut,"remote_model_calls":0}),
        Ok(None)=>json!({"status":"computed-by-running-scan","computed_vectors":0,"remote_model_calls":0}),
        Err(reason)=>json!({"status":"unavailable; lexical fallback preserved","reason":reason.to_string(),"remote_model_calls":0}),
    })
}

fn guard(root: &Path, cwd: &Path, request: &Request) -> Result<(), String> {
    // A conferência vale para quem pediu a busca: o agente de onda que busca
    // na própria cópia não é o condutor preso nela, e a cópia de outra onda
    // continua barrada para ele.
    let session_id = super::spec_state::session_from_env();
    let agent_id = agent_from_env();
    let ctx = Ctx {
        project_dir: root.to_string_lossy().into_owned(),
        trigger: Some(Trigger::PreToolUse),
        config: mustard_core::ProjectConfig::load(root),
        ..Ctx::default()
    };
    let file_operation =
        request.tool == "Read" || mustard_core::io::code_search::operations::requested(request);
    let input = if file_operation {
        if request.input["file_path"].as_str().is_none() {
            return Err("search-missing-file_path".into());
        }
        HookInput {
            tool_name: Some("Read".into()),
            tool_input: {
                let mut value = request.input.clone();
                if let Some(file) = value["file_path"].as_str() {
                    value["file_path"] = json!(cwd.join(file));
                }
                value
            },
            cwd: Some(cwd.to_string_lossy().into_owned()),
            session_id,
            agent_id,
            ..HookInput::default()
        }
    } else {
        let (tool, args) = mustard_core::io::code_search::arguments(request)?;
        let command = std::iter::once(tool)
            .chain(args)
            .map(|arg| quote(&arg))
            .collect::<Vec<_>>()
            .join(" ");
        HookInput {
            tool_name: Some("Bash".into()),
            tool_input: json!({"command":command}),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            session_id,
            agent_id,
            ..HookInput::default()
        }
    };
    let verdict = if file_operation {
        crate::hooks::write::write_gate::file_verdict(
            root.to_str().unwrap_or_default(),
            &input,
            &ctx,
        )
    } else {
        // A busca roda aqui, sem shell: a cópia como raiz não prende ninguém.
        crate::hooks::bash::command_guard::CommandGuard::evaluate_in_process(&input, &ctx)
            .map_err(|e| e.to_string())?
    };
    match verdict {
        Verdict::Deny { reason } => Err(reason),
        Verdict::Rewrite { tool_input, note } => Err(format!(
            "{} Retry with input: {tool_input}",
            note.unwrap_or_default()
        )),
        _ => Ok(()),
    }
}

/// Rewrite literal file-search segments without moving shell operators.
/// Stream filters and ambiguous syntax retain the original permission path.
pub(crate) fn before_bash(input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !ctx.config.search_answer() {
        return None;
    }
    let command = input.tool_input.get("command")?.as_str()?;
    let ranges = crate::hooks::bash::lex::command_ranges(command);
    let compound = ranges.len() > 1;
    let mut replacement = command.to_string();
    let mut changed = false;
    for (at,range) in ranges.iter().enumerate().rev() {
        if at>0 {
            let separator=command[ranges[at-1].end..range.start].trim_start();
            if separator.starts_with('|') && !separator.starts_with("||") {continue;}
        }
        let part = &command[range.clone()];
        let trimmed = part.trim();
        // Only literal outer commands. Never evaluate substitutions or move
        // commands across &&, ||, pipes, redirects, loops or shell functions.
        if !literal_shell(trimmed) { continue; }
        let segments = crate::hooks::bash::lex::segments(trimmed);
        let Some(segment) = segments.first().filter(|_| segments.len() == 1) else {continue};
        if !matches!(segment.program.raw.as_str(), "rg" | "grep" | "git")
            || !segment.leading.is_empty() || !segment.redirects.is_empty()
            || !trimmed.strip_prefix(&segment.program.raw).is_some_and(|rest| rest.starts_with(char::is_whitespace))
            || segment.args.iter().any(|arg| arg.raw == arg.text && arg.raw.contains(['*', '?', '[', '{', '~'])) {
            continue;
        }
        let argv: Vec<_> = std::iter::once(segment.program.text.clone())
            .chain(segment.args.iter().map(|arg| arg.text.clone())).collect();
        let Ok(mut request) = Request::native(&argv) else {continue};
        if mustard_core::io::code_search::arguments(&request).is_err() {continue;}
        request.describe(input.tool_description().unwrap_or_default());
        let Ok(envelope) = serde_json::to_string(&Invocation::new(request)) else {continue};
        // `.` is evaluated when this segment runs, after any preceding cd and
        // after the hook leads a wave agent's command into its copy.
        // Pipeline consumers receive exactly native bytes, without cards.
        let routed = format!("mustard-rt run search --root '.' --request {} {}",
            quote(&envelope), if compound {"--raw"} else {"--shell-output"});
        let begin = range.start + part.len() - part.trim_start().len();
        replacement.replace_range(begin..begin + trimmed.len(), &routed);
        changed = true;
    }
    if !changed {return None;}
    let mut tool_input = input.tool_input.clone();
    tool_input["command"] = json!(replacement);
    Some(Verdict::Rewrite {tool_input, note: None})
}

pub(crate) fn literal_shell(command: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    for c in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            }
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if matches!(c, '$' | '`') {
            return false;
        }
        if quote == Some('"') {
            if c == '"' {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        if matches!(
            c,
            '\n' | '\r' | ';' | '|' | '&' | '<' | '>' | '(' | ')' | '#'
        ) {
            return false;
        }
    }
    quote.is_none() && !escaped
}

/// A classic hook cannot change a tool's name. Hand off discovery to the
/// intent-aware gateway. Exact Read calls stay on the host permission path;
/// their already executed excerpts are crossed in PostToolUse instead.
pub(crate) fn before_file(input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    if !ctx.config.search_answer() {
        return None;
    }
    let tool = input.tool_name.as_deref()?;
    if !matches!(tool, "Grep" | "Glob") {
        return None;
    }
    mustard_core::platform::process::program_location("rg")?;
    let cwd = input.cwd.as_deref().unwrap_or(&ctx.project_dir);
    let given = input
        .tool_input
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(".");
    let path = super::code_route::project_path(&ctx.project_dir, cwd, given)?;
    let canonical = path.abs.canonicalize().ok()?;
    if !canonical.starts_with(path.tree.canonicalize().ok()?) {
        return None;
    }
    let mut request = Request {
        tool: tool.into(),
        input: input.tool_input.clone(),
        intent: String::new(),
        purpose: Default::default(),
        choose: false,
    };
    request.describe(input.tool_description().unwrap_or_default());
    mustard_core::io::code_search::arguments(&request).ok()?;
    let command = format!(
        "mustard-rt run search --root {} --request {} --shell-output",
        quote(cwd),
        quote(&serde_json::to_string(&Invocation::new(request)).ok()?)
    );
    let reason = super::say::say(
        "search.gateway.route",
        ctx.config.language().text_or_default(),
        &[("{command}", &command)],
    );
    Some(Verdict::Deny { reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choice_diagnostics_distinguish_exact_native_and_configured_investigations() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        let dir=tempfile::tempdir().unwrap();
        let mut request=Request{tool:"Read".into(),input:json!({"file_path":"x.rs"}),
            intent:"Understand behavior".into(),purpose:Purpose::Understand,choose:true};
        assert_eq!(choice_gate(dir.path(),&request,true),"exact-read");
        request.tool="Grep".into();request.purpose=Purpose::Locate;
        assert_eq!(choice_gate(dir.path(),&request,true),"locate");
        request.purpose=Purpose::Understand;request.choose=false;
        assert_eq!(choice_gate(dir.path(),&request,true),"not-requested");
        request.choose=true;
        assert_eq!(choice_gate(dir.path(),&request,true),"eligible");
        assert_eq!(choice_gate(dir.path(),&request,false),"ai-fallback-disabled");
        std::fs::write(dir.path().join("mustard.json"),r#"{"ai":{"fallback":true}}"#).unwrap();
        assert_eq!(choice_gate(dir.path(),&request,false),"purpose-not-configured");
        std::fs::write(dir.path().join("mustard.json"),r#"{"ai":{"fallback":true},"judgement":{"search":{"filter":"jev"}}}"#).unwrap();
        assert_eq!(choice_gate(dir.path(),&request,false),"provider-unavailable");
    }

    /// A busca que pediu a escolha ao Jev ligado, quando ele falha ou o prazo
    /// único da busca vence, diz ao agente que buscou, numa linha do
    /// catálogo, que a escolha entre os candidatos listados é dele; não há
    /// reserva ao modelo pequeno. Com a escolha do Jev, ou sem Jev, a linha
    /// não aparece.
    #[test]
    fn a_search_whose_jev_fails_tells_the_agent_the_choice_is_his() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
        let said = mustard_core::platform::i18n::translate("map.search_choice_left", lang);
        let card = |id: &str, name: &str, line: u64| json!({ "id": id, "name": name, "initial_source_excerpt": true, "parse_complete": true,
            "source": { "file": "src/quartz.rs", "line": line, "end_line": line, "sha256": "0" },
            "source_excerpt": { "text": format!("{line}:pub fn {name}() {{}}"), "truncated": false } });
        let request = Request { tool: "rg".into(), input: json!({ "args": ["-n", "quartz", "."] }), intent: "which quartz entry opens the gate".into(),
            purpose: Purpose::Implement, choose: true };
        let answered = |status: &str| {
            let mut answer = mustard_core::io::code_search::Answer { report: json!({ "task_context": { "status": "current-task-evidence",
                "cards": [card("c1", "quartz_entry", 1), card("c2", "quartz_gate", 3)], "selection": { "status": status } } }),
                stdout: Vec::new(), stderr: Vec::new(), exit_code: 0 };
            leave_choice_to_agent(root, &mut answer, true);
            String::from_utf8(mustard_core::io::code_search::presentation::agent(&answer, &request, root).stdout).unwrap()
        };
        for status in ["native-fallback", "native; search-budget-spent"] {
            let view = answered(status);
            assert!(view.contains(said) && view.contains("quartz_entry") && view.contains("quartz_gate"), "{status}: {view}");
        }
        assert!(!answered("jev-choice").contains(said), "the Jev chose");
        let mut silent = mustard_core::io::code_search::Answer { report: json!({ "task_context": { "selection": { "status": "native-fallback" } } }),
            stdout: Vec::new(), stderr: Vec::new(), exit_code: 0 };
        leave_choice_to_agent(root, &mut silent, false);
        assert!(silent.report["task_context"]["selection"].get("choice_left").is_none(), "without the Jev nothing was asked");
    }

    fn call(root: &Path, tool: &str, value: serde_json::Value) -> (HookInput, Ctx) {
        let root = root.to_string_lossy().to_string();
        (
            HookInput {
                tool_name: Some(tool.into()),
                tool_input: value,
                cwd: Some(root.clone()),
                ..HookInput::default()
            },
            Ctx {
                project_dir: root,
                trigger: Some(Trigger::PreToolUse),
                ..Ctx::default()
            },
        )
    }
    #[test]
    fn simple_native_searches_rewrite_once_with_every_argument_and_description() {
        let dir = tempfile::tempdir().unwrap();
        for command in [
            "rg -n -i --glob '*.rs' 'restore quartz' src",
            "rg -n 'restore|quartz.*' src",
            "grep -Rnw 'restore' src",
            "git grep -n -F quartz -- src",
        ] {
            let (mut input, ctx) = call(
                dir.path(),
                "Bash",
                json!({"command":command,"description":"Understand persistence","timeout":5000}),
            );
            let original = input.tool_input.clone();
            let Some(Verdict::Rewrite { tool_input, .. }) = before_bash(&input, &ctx) else {
                panic!("{command}")
            };
            assert_eq!(tool_input["timeout"], 5000);
            assert!(!tool_input["command"].as_str().unwrap().ends_with(" --raw"),"a quoted regex alternation is not a pipeline");
            let parts = crate::hooks::bash::lex::segments(tool_input["command"].as_str().unwrap());
            let args = &parts[0].args;
            let at = args.iter().position(|arg| arg.text == "--request").unwrap();
            let request = Request::from_json(&args[at + 1].text).unwrap();
            let original_parts = crate::hooks::bash::lex::segments(command);
            assert_eq!(
                request.input["args"],
                json!(
                    original_parts[0]
                        .args
                        .iter()
                        .map(|arg| &arg.text)
                        .collect::<Vec<_>>()
                )
            );
            assert_eq!(request.intent, "Understand persistence");
            assert!(!request.choose);
            input.tool_input = tool_input;
            assert!(before_bash(&input, &ctx).is_none(), "no rewrite loop");
            assert_eq!(original["command"], command, "input is immutable");
        }
    }
    #[test]
    fn compound_searches_preserve_conditions_cwd_and_native_pipe_output() {
        let dir = tempfile::tempdir().unwrap();
        for command in ["cd src && grep -n save store.ts | head -5", "git status --short; rg -n save src || echo missing"] {
            let (input, ctx) = call(dir.path(), "Bash", json!({"command":command,"timeout":4000}));
            let Some(Verdict::Rewrite {tool_input,..}) = before_bash(&input,&ctx) else {panic!("{command}")};
            let routed = tool_input["command"].as_str().unwrap();
            assert!(routed.contains("--root '.'"), "{routed}");
            assert!(routed.contains(" --raw") && !routed.contains("--shell-output"), "{routed}");
            assert_eq!(tool_input["timeout"],4000);
            if command.starts_with("cd") {assert!(routed.starts_with("cd src && "));assert!(routed.ends_with(" | head -5"));}
            else {assert!(routed.starts_with("git status --short; "));assert!(routed.ends_with(" || echo missing"));}
            let mut again=input.clone();again.tool_input=tool_input;
            assert!(before_bash(&again,&ctx).is_none(), "no rewrite loop");
        }
    }

    #[test]
    fn unknown_shell_effects_keep_the_original_execution_path() {
        let dir = tempfile::tempdir().unwrap();
        for command in [
            "rg x src > result",
            "rg \"$TERM\" src",
            "FOO=x rg x src",
            "rtk rg x src",
            "rg x src/*.rs",
            "rg --pre helper x src",
            "git grep -O x",
            "git config user.name",
        ] {
            let (input, ctx) = call(dir.path(), "Bash", json!({"command":command}));
            assert!(before_bash(&input, &ctx).is_none(), "{command}");
        }
    }
    #[test]
    fn structured_host_description_preserves_investigation_purpose_in_versioned_contract() {
        let dir = tempfile::tempdir().unwrap();
        let (input, ctx) = call(dir.path(), "Bash", json!({
            "command":"rg -n 'current_copy' src",
            "description":"mustard:spec: Verify which operation exports the edited copy"
        }));
        let Some(Verdict::Rewrite { tool_input, .. }) = before_bash(&input, &ctx) else {
            panic!("annotated search should be routed");
        };
        let parts = crate::hooks::bash::lex::segments(tool_input["command"].as_str().unwrap());
        let args = &parts[0].args;
        let at = args.iter().position(|arg| arg.text == "--request").unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&args[at + 1].text).unwrap();
        assert_eq!(envelope["schema_version"], 1);
        let request = Request::from_json(&args[at + 1].text).unwrap();
        assert_eq!(request.purpose, mustard_core::domain::knowledge::investigation::Purpose::Spec);
        assert_eq!(request.intent, "Verify which operation exports the edited copy");
        assert_eq!(request.input["args"], json!(["-n","current_copy","src"]));
        assert!(!request.choose);
    }
    #[test]
    fn typed_search_handoff_keeps_the_complete_request_and_unknown_fields_pass() {
        let dir = tempfile::tempdir().unwrap();
        for tool in ["Grep", "Glob"] {
            let value = if tool == "Grep" {
                json!({"pattern":"quartz.*","-i":true,"output_mode":"count","offset":2})
            } else {
                json!({"pattern":"**/*quartz*"})
            };
            let (input, ctx) = call(dir.path(), tool, value.clone());
            let Some(Verdict::Deny { reason }) = before_file(&input, &ctx) else {
                panic!("{tool}")
            };
            assert!(reason.contains("mustard-rt run search"));
            assert!(reason.contains(&serde_json::to_string(&value).unwrap()));
            let (input, ctx) = call(
                dir.path(),
                tool,
                json!({"pattern":"quartz","future_option":true}),
            );
            assert!(before_file(&input, &ctx).is_none());
        }
    }
    /// Um scan de mentira em `dir`: cada passada anota o pedido no registro e
    /// só acaba quando o teste cria o arquivo de soltura, ou no limite de
    /// segurança das esperas dos testes.
    #[cfg(unix)]
    fn held_scan(dir: &Path) -> (Scan, std::path::PathBuf, std::path::PathBuf) {
        let tools = dir.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let (log, release) = (dir.join("log"), dir.join("release"));
        let script = format!(
            "#!/bin/sh\necho \"$1\" >> '{log}'\n{held}\n\
             echo finished >> '{log}'\necho '{{\"ok\":true,\"read\":[]}}'\n",
            log = log.display(),
            held = crate::held_command::held_until_released(&release)
        );
        let written = std::process::Command::new("/bin/sh")
            .args(["-c", "printf '%s' \"$2\" > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(tools.join("scan"))
            .arg(script)
            .status()
            .unwrap();
        assert!(written.success());
        (Scan::located_from(Some(&tools.join("mustard-rt"))), log, release)
    }

    /// O registro do scan de mentira de [`held_scan`] depois que a passada
    /// pedida anotou o começo. A busca que responde no prazo pode voltar antes
    /// de o processo do scan escrever a primeira linha: na máquina carregada,
    /// ele demora a começar.
    #[cfg(unix)]
    fn log_once_started(log: &Path) -> String {
        crate::wait_for::wait_for("o começo da passada pedida ao scan", || Some(std::fs::read_to_string(log).unwrap_or_default()).filter(|calls| calls.starts_with("scan\n")))
    }

    /// A primeira busca numa pasta sem mapa não espera a leitura do projeto
    /// inteiro: responde no prazo, com o resultado da busca, e diz na saída de
    /// erro que o mapa segue em atualização; a passada continua depois dela.
    #[cfg(unix)]
    #[test]
    fn a_search_in_a_folder_without_a_map_answers_at_the_wait_and_says_the_refresh_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("mustard.json"), "{}").unwrap();
        std::fs::write(project.join("src/a.rs"), "pub fn quartz_marker() {}\n").unwrap();
        let git = std::process::Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap();
        assert!(git.success());
        let (scan, log, release) = held_scan(dir.path());
        let request = Request::native(&["rg".into(), "-n".into(), "quartz_marker".into(), ".".into()]).unwrap();

        let answer = answer_with(&project, &request, &scan, Duration::from_millis(300)).expect("the search answers");
        let calls = log_once_started(&log);
        std::fs::write(&release, "").unwrap();

        assert!(!calls.contains("finished"), "the answer came while the refresh was still running: {calls}");
        assert!(calls.starts_with("scan\n"), "the search asked for the refresh: {calls}");
        assert_eq!(answer.report["learning"]["scan"]["status"], "in-progress", "{}", answer.report["learning"]);
        assert!(String::from_utf8_lossy(&answer.stdout).contains("quartz_marker"), "the search result is kept");
        let lang = mustard_core::ProjectConfig::load(&project).language().text_or_default();
        let notice = super::super::say::say("search.gateway.scan_running", lang, &[]);
        assert!(String::from_utf8_lossy(&answer.stderr).contains(&notice), "{}", String::from_utf8_lossy(&answer.stderr));
    }

    /// O mapa herdado de outra versão do programa, com o índice de
    /// conhecimento velho, é refeito pela passada do scan, que segura a trava
    /// de gravação enquanto o refaz. A busca que pediu a passada e a que chega
    /// no meio dela não abrem o mapa para refazer o mesmo bloco nem esperam a
    /// trava: cada uma responde antes de uma espera de trava inteira (5 s),
    /// com o resultado nativo, e diz que o mapa segue em atualização.
    #[cfg(unix)]
    #[test]
    fn searches_over_a_map_with_an_outdated_block_answer_without_waiting_for_the_pass_that_rebuilds_it() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        use mustard_core::io::map_db::MapDb;
        let real = Scan::locate();
        assert!(
            real.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("mustard.json"), r#"{"ai":{"vectors":false}}"#).unwrap();
        std::fs::write(project.join("src/entry.ts"), "export function quartzEntry(): number { return 1; }\n").unwrap();
        let git = std::process::Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap();
        assert!(git.success());
        let model = mustard_core::io::project_map::model_path(&project);
        real.scan(&project, &model).expect("the real scan writes the map");
        let version = || {
            MapDb::open(&model, &project, &[]).unwrap().conn().query_row("SELECT version FROM blocks WHERE name = 'knowledge_index'", [], |row| row.get::<_, u32>(0)).unwrap()
        };
        let current = version();
        MapDb::open(&model, &project, &[]).unwrap().write(|tx| Ok(tx.execute_batch("UPDATE blocks SET version = version - 1 WHERE name = 'knowledge_index'")?)).unwrap();
        let (scan, log, release) = held_scan(dir.path());
        // A passada que refaz o bloco: segura a trava de gravação do mapa
        // desde que começa até o teste a soltar.
        let (locked, hold) = (dir.path().join("locked"), std::sync::mpsc::channel::<()>());
        let (marker, watched, holder_model, holder_root) = (locked.clone(), log.clone(), model.clone(), project.clone());
        let holder = std::thread::spawn(move || {
            crate::wait_for::wait_for("o começo da passada do scan", || watched.exists().then_some(()));
            MapDb::open(&holder_model, &holder_root, &[]).unwrap().write(|_| {
                std::fs::write(&marker, "").unwrap();
                let _ = crate::wait_for::wait_for_message("a soltura da trava do mapa", &hold.1);
                Ok(())
            }).unwrap();
        });
        let requests = [
            Request { tool: "rg".into(), input: json!({"args":["-n","quartzEntry","."]}), intent: "where the quartz entry is defined".into(), purpose: Purpose::Implement, choose: false },
            Request { tool: "Glob".into(), input: json!({"pattern":"src/**/*.ts"}), intent: "find the quartz entry module".into(), purpose: Purpose::Implement, choose: false },
        ];
        let lang = mustard_core::ProjectConfig::load(&project).language().text_or_default();
        let notice = super::super::say::say("search.gateway.scan_running", lang, &[]);
        let mut answers = Vec::new();
        for request in &requests {
            let answer = answer_with(&project, request, &scan, Duration::from_secs(1)).expect("the search answers");
            answers.push((request.tool.clone(), locked.exists(), answer));
        }
        let still = version();
        std::fs::write(&release, "").unwrap();
        hold.0.send(()).unwrap();
        holder.join().unwrap();

        for (tool, held, answer) in &answers {
            let stderr = String::from_utf8_lossy(&answer.stderr);
            assert!(held, "{tool}: the pass held the map lock while the search answered");
            assert_eq!(answer.report["learning"]["scan"]["status"], "in-progress", "{tool}: {}", answer.report["learning"]);
            assert_eq!(answer.report["task_context"]["status"], "native-fallback", "{tool}: {}", answer.report["task_context"]);
            assert!(stderr.contains(&notice), "{tool}: {stderr}");
        }
        assert!(String::from_utf8_lossy(&answers[0].2.stdout).contains("quartzEntry"), "the search result is kept");
        assert_eq!(answers[0].2.report["crossing_status"], "index-unavailable-or-stale");
        assert_eq!(still, current - 1, "the search leaves the outdated block to the pass");
        assert!(std::fs::read_to_string(&log).unwrap().starts_with("scan\n"), "the search asked for the pass");
    }

    /// As perguntas explícitas por símbolo e por estrutura, num mapa herdado
    /// com o índice de conhecimento velho, não refazem o bloco antes de pedir
    /// a passada nem esperam a trava da passada que o refaz: a primeira pede a
    /// passada, as que chegam no meio dela respondem antes de uma espera de
    /// trava inteira (5 s), e todas dizem que o mapa segue em atualização.
    /// Quando a passada acaba no prazo, a pergunta por símbolo se repete no
    /// mapa posto em dia e traz o símbolo.
    #[cfg(unix)]
    #[test]
    fn explicit_searches_over_a_map_with_an_outdated_block_answer_without_rebuilding_it_or_waiting_for_the_pass() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        use mustard_core::io::code_search::operations::MAP_OUTDATED;
        use mustard_core::io::map_db::MapDb;
        let real = Scan::locate();
        assert!(
            real.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("mustard.json"), r#"{"ai":{"vectors":false}}"#).unwrap();
        std::fs::write(project.join("src/entry.ts"), "export function quartzEntry(): number { return 1; }\n").unwrap();
        let git = std::process::Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap();
        assert!(git.success());
        let model = mustard_core::io::project_map::model_path(&project);
        real.scan(&project, &model).expect("the real scan writes the map");
        let version = || {
            MapDb::open(&model, &project, &[]).unwrap().conn().query_row("SELECT version FROM blocks WHERE name = 'knowledge_index'", [], |row| row.get::<_, u32>(0)).unwrap()
        };
        let current = version();
        MapDb::open(&model, &project, &[]).unwrap().write(|tx| Ok(tx.execute_batch("UPDATE blocks SET version = version - 1 WHERE name = 'knowledge_index'")?)).unwrap();
        let (scan, log, release) = held_scan(dir.path());
        let (locked, hold) = (dir.path().join("locked"), std::sync::mpsc::channel::<()>());
        let (marker, watched, holder_model, holder_root) = (locked.clone(), log.clone(), model.clone(), project.clone());
        let holder = std::thread::spawn(move || {
            crate::wait_for::wait_for("o começo da passada do scan", || watched.exists().then_some(()));
            MapDb::open(&holder_model, &holder_root, &[]).unwrap().write(|_| {
                std::fs::write(&marker, "").unwrap();
                let _ = crate::wait_for::wait_for_message("a soltura da trava do mapa", &hold.1);
                Ok(())
            }).unwrap();
        });
        let ask = |tool: &str, input: serde_json::Value| Request { tool: tool.into(), input, intent: "what the quartz entry returns".into(), purpose: Purpose::Understand, choose: false };
        let symbol = ask("Symbol", json!({"file_path":"src/entry.ts","symbol":"src/entry.ts:1:quartzEntry"}));
        let structure = ask("Structure", json!({"file_path":"src/entry.ts","query":"(function_declaration name: (identifier) @name)"}));
        let lang = mustard_core::ProjectConfig::load(&project).language().text_or_default();
        let notice = super::super::say::say("search.gateway.scan_running", lang, &[]);
        let mut answers = Vec::new();
        for request in [&symbol, &structure, &symbol] {
            let answer = answer_with(&project, request, &scan, Duration::from_secs(1));
            answers.push((request.tool.clone(), locked.exists(), answer));
        }
        let still = version();
        std::fs::write(&release, "").unwrap();
        hold.0.send(()).unwrap();
        holder.join().unwrap();

        for (at, (tool, held, answer)) in answers.into_iter().enumerate() {
            let answer = answer.unwrap_or_else(|error| panic!("{tool} {at} refused: {error}"));
            assert!(held, "{tool} {at}: the pass held the map lock while the search answered; block version {still} of {current}; {}", answer.report["learning"]);
            assert_eq!(answer.report["learning"]["scan"]["status"], "in-progress", "{tool} {at}: {}", answer.report["learning"]);
            assert!(String::from_utf8_lossy(&answer.stderr).contains(&notice), "{tool} {at}: {}", String::from_utf8_lossy(&answer.stderr));
            let result = &answer.report["result"];
            if tool == "Symbol" {
                assert_eq!(result["status"], MAP_OUTDATED, "{tool} {at}: {result}");
            } else {
                assert!(result["matches"].as_array().is_some_and(|found| !found.is_empty()), "{tool} {at}: {result}");
                assert_eq!(result["owners"]["status"], MAP_OUTDATED, "{tool} {at}: {result}");
            }
        }
        assert_eq!(still, current - 1, "the search leaves the outdated block to the pass");
        assert!(std::fs::read_to_string(&log).unwrap().starts_with("scan\n"), "the search asked for the pass");

        let refreshed = answer_with(&project, &symbol, &real, Duration::from_secs(60)).expect("the search answers after the pass");
        assert_eq!(refreshed.report["learning"]["scan"]["status"], "refreshed-native", "{}", refreshed.report["learning"]);
        assert!(refreshed.report["result"]["cards"].as_array().is_some_and(|cards| cards.iter().any(|card| card["id"] == "src/entry.ts:1:quartzEntry")), "{}", refreshed.report["result"]);
        assert_eq!(version(), current, "the pass rebuilt the block");
    }

    /// Um projeto com o mapa gravado pelo scan de verdade: a função
    /// `quartzEntry` em `src/entry.ts`, chamada em `src/use.ts`.
    #[cfg(unix)]
    fn scanned_project(dir: &Path) -> std::path::PathBuf {
        let real = Scan::locate();
        assert!(
            real.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("mustard.json"), r#"{"ai":{"vectors":false}}"#).unwrap();
        std::fs::write(project.join("src/entry.ts"), "export function quartzEntry(): number { return 1; }\n").unwrap();
        std::fs::write(project.join("src/use.ts"), "import { quartzEntry } from \"./entry\";\nexport const quartzTotal = quartzEntry() + 1;\n").unwrap();
        let git = std::process::Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap();
        assert!(git.success());
        real.scan(&project, &mustard_core::io::project_map::model_path(&project)).expect("the real scan writes the map");
        project
    }

    /// Quantas passadas do mapa o scan de mentira de [`held_scan`] recebeu.
    #[cfg(unix)]
    fn passes(log: &Path) -> usize {
        std::fs::read_to_string(log).unwrap_or_default().lines().filter(|line| *line == "scan").count()
    }

    /// Outra gravação segura a trava do mapa em dia até a busca voltar. A
    /// busca com finalidade não espera por ela: responde com a trava ainda
    /// presa, com o resultado nativo, sem pedir passada, e deixa o que
    /// aprendeu para a próxima busca. Diz quanto cada etapa levou.
    #[cfg(unix)]
    #[test]
    fn a_purposed_search_answers_without_waiting_for_another_writer_of_the_map() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        use mustard_core::io::map_db::MapDb;
        let dir = tempfile::tempdir().unwrap();
        let project = scanned_project(dir.path());
        let model = mustard_core::io::project_map::model_path(&project);
        let (scan, log, release) = held_scan(dir.path());
        let (locked, hold) = (std::sync::mpsc::channel::<()>(), std::sync::mpsc::channel::<()>());
        let (holder_model, holder_root) = (model.clone(), project.clone());
        let holder = std::thread::spawn(move || {
            MapDb::open(&holder_model, &holder_root, &[]).unwrap().write(|_| {
                locked.0.send(()).unwrap();
                let _ = crate::wait_for::wait_for_message("a soltura da trava do mapa", &hold.1);
                Ok(())
            }).unwrap();
        });
        crate::wait_for::wait_for_message("a trava do mapa com a outra gravação", &locked.1).expect("the other writer holds the map lock");
        let request = Request {
            tool: "rg".into(),
            input: json!({"args":["-n","quartzEntry","src"]}),
            intent: "what the quartz entry returns".into(),
            purpose: Purpose::Understand,
            choose: false,
        };

        let answer = answer_with(&project, &request, &scan, Duration::from_secs(1)).expect("the search answers");
        std::fs::write(&release, "").unwrap();
        hold.0.send(()).unwrap();
        holder.join().unwrap();

        assert!(String::from_utf8_lossy(&answer.stdout).contains("quartzEntry"), "the search result is kept");
        assert!(passes(&log) <= 1, "{}", std::fs::read_to_string(&log).unwrap_or_default());
        assert_eq!(answer.report["learning_write"]["status"], "left-for-next-search", "{}", answer.report["learning_write"]);
        for stage in ["scan_wait_ms", "lock_wait_ms", "vectors_ms", "investigate_ms", "jev_ms"] {
            assert!(answer.report["timings"][stage].is_u64(), "{stage}: {}", answer.report["timings"]);
        }
    }

    /// A passada que não acaba no prazo da busca segue sozinha: a busca pede
    /// uma passada só, não investiga duas vezes, responde com o mapa de antes
    /// e diz que ele segue em atualização.
    #[cfg(unix)]
    #[test]
    fn a_search_whose_pass_outlasts_the_budget_asks_for_one_pass_and_answers_with_the_map_before() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        let dir = tempfile::tempdir().unwrap();
        let project = scanned_project(dir.path());
        std::fs::write(project.join("src/fresh.ts"), "import { quartzEntry } from \"./entry\";\nexport const quartzFresh = quartzEntry() * 2;\n").unwrap();
        let (scan, log, release) = held_scan(dir.path());
        let request = Request {
            tool: "rg".into(),
            input: json!({"args":["-n","quartzEntry","src"]}),
            intent: "who calls the quartz entry".into(),
            purpose: Purpose::Understand,
            choose: false,
        };

        let answer = answer_with(&project, &request, &scan, Duration::from_millis(300)).expect("the search answers");
        let calls = log_once_started(&log);
        std::fs::write(&release, "").unwrap();

        assert_eq!(passes(&log), 1, "{calls}");
        assert_eq!(answer.report["learning"]["scan"]["status"], "in-progress", "{}", answer.report["learning"]);
        assert!(answer.report["task_context"]["learning"].get("scan").is_none(), "{}", answer.report["task_context"]["learning"]);
        assert!(String::from_utf8_lossy(&answer.stdout).contains("quartzFresh"), "the search result is kept");
        assert_eq!(answer.report["crossing_status"], "enriched", "{}", answer.report);
        let lang = mustard_core::ProjectConfig::load(&project).language().text_or_default();
        let notice = super::super::say::say("search.gateway.scan_running", lang, &[]);
        assert!(String::from_utf8_lossy(&answer.stderr).contains(&notice), "{}", String::from_utf8_lossy(&answer.stderr));
    }

    /// A busca de implementação de um padrão traz primeiro todas as linhas
    /// que casam, mesmo além de 64, e nada de arquivo onde o padrão não casa:
    /// nem a função chamada noutro arquivo, nem a de palavras parecidas.
    #[test]
    fn an_implementation_search_answers_with_every_pattern_hit_first_and_no_file_without_hits() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        let real = Scan::locate();
        assert!(
            real.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("mustard.json"), r#"{"ai":{"vectors":false}}"#).unwrap();
        std::fs::write(project.join("src/entry.ts"), "import { recoverSnapshot } from \"./store\";\nexport function quartzEntry(): number {\n  return recoverSnapshot();\n}\n").unwrap();
        std::fs::write(project.join("src/store.ts"), "/** Recover the quartz snapshot. */\nexport function recoverSnapshot(): number {\n  const storeBodySentinel = 2;\n  return storeBodySentinel;\n}\n").unwrap();
        std::fs::write(project.join("src/noise.ts"), "/** Recover the quartz snapshot from the archive. */\nexport function archiveRecover(): number {\n  const noiseBodySentinel = 4;\n  return noiseBodySentinel;\n}\n").unwrap();
        let rows = (0..150).fold(String::new(), |mut rows, at| {
            use std::fmt::Write as _;
            let _ = writeln!(rows, "  \"quartzEntry {at}\",");
            rows
        });
        std::fs::write(project.join("src/table.ts"), format!("export const quartzTable = [\n{rows}];\n")).unwrap();
        let git = std::process::Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap();
        assert!(git.success());
        real.scan(&project, &mustard_core::io::project_map::model_path(&project)).expect("the real scan writes the map");
        let request = Request {
            tool: "rg".into(),
            input: json!({"args":["-n","quartzEntry","src"]}),
            intent: "recover the quartz snapshot".into(),
            purpose: Purpose::Implement,
            choose: false,
        };
        let answer = answer(&project, &request).expect("the search answers");
        let view = mustard_core::io::code_search::presentation::agent(&answer, &request, &project);
        let text = String::from_utf8(view.stdout).unwrap();
        assert_eq!(view.representation, "current-task-evidence", "{text}");
        let first_reference = text
            .match_indices('\n')
            .map(|(at, _)| at + 1)
            .find(|at| ["# References", "# Native follow-up", "# static relation", "# Deferred"].iter().any(|mark| text[*at..].starts_with(mark)))
            .unwrap_or(text.len());
        let native = String::from_utf8_lossy(&answer.stdout).into_owned();
        let hits: Vec<_> = native.lines().filter_map(|row| row.split_once(':')).filter_map(|(_, rest)| rest.split_once(':')).collect();
        assert_eq!(hits.len(), 151, "{native}");
        for (line, body) in hits {
            let at = text.find(&format!("\n{line} | {body}\n")).unwrap_or_else(|| panic!("hit {line} left out: {text}"));
            assert!(at < first_reference, "hit {line} after a reference: {text}");
        }
        for absent in ["src/store.ts", "storeBodySentinel", "src/noise.ts", "noiseBodySentinel"] {
            assert!(!text.contains(absent), "{absent}: {text}");
        }
    }

    /// A busca com finalidade que nomeia o arquivo traz toda linha casada
    /// dele antes do resumo por símbolo, e nada do teste que não tem a
    /// palavra.
    #[test]
    fn a_purposed_search_of_a_named_file_answers_with_every_matched_line_before_the_symbols() {
        use mustard_core::domain::knowledge::investigation::Purpose;
        let real = Scan::locate();
        assert!(
            real.is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("mustard.json"), r#"{"ai":{"vectors":false}}"#).unwrap();
        std::fs::write(project.join("src/limits.ts"), "export const TAIL_BYTES = 4096; // threshold of the tail\n\
            export function overLimit(size: number): boolean {\n  const threshold = TAIL_BYTES * 2;\n  return size > threshold;\n}\n\
            export class Gauge {\n  constructor(private readonly threshold: number) {}\n  exceeds(value: number): boolean {\n    return value > this.threshold;\n  }\n}\n\
            /** Explains the threshold rule. */\nexport function describe(): string {\n  return \"threshold reached\";\n}\n").unwrap();
        std::fs::write(project.join("src/limits.test.ts"), "import { overLimit, Gauge } from \"./limits\";\n\
            test(\"big sizes pass\", () => {\n  const unrelatedSentinel = new Gauge(1);\n  expect(overLimit(10000) && unrelatedSentinel.exceeds(2)).toBe(true);\n});\n").unwrap();
        let git = std::process::Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap();
        assert!(git.success());
        real.scan(&project, &mustard_core::io::project_map::model_path(&project)).expect("the real scan writes the map");
        let request = Request {
            tool: "rg".into(),
            input: json!({"args":["-n","-H","threshold","src/limits.ts"]}),
            intent: "check how the gauge compares against its threshold".into(),
            purpose: Purpose::Validate,
            choose: false,
        };

        let answer = answer(&project, &request).expect("the search answers");
        let view = mustard_core::io::code_search::presentation::agent(&answer, &request, &project);

        let text = String::from_utf8(view.stdout).unwrap();
        assert_eq!(view.representation, "current-task-evidence", "{text}");
        let native = String::from_utf8_lossy(&answer.stdout).into_owned();
        let hits: Vec<_> = native.lines().filter_map(|row| row.split_once(':')).filter_map(|(_, rest)| rest.split_once(':')).collect();
        assert_eq!(hits.len(), 7, "{native}");
        let first_reference = text
            .match_indices('\n')
            .map(|(at, _)| at + 1)
            .find(|at| ["# References", "# Native follow-up", "# static relation", "# Deferred"].iter().any(|mark| text[*at..].starts_with(mark)))
            .unwrap_or(text.len());
        for (line, body) in hits {
            let at = text.find(&format!("\n{line} | {body}\n")).unwrap_or_else(|| panic!("hit {line} left out: {text}"));
            assert!(at < first_reference, "hit {line} after a reference: {text}");
        }
        for absent in ["@ src/limits.test.ts", "unrelatedSentinel"] {
            assert!(!text.contains(absent), "{absent}: {text}");
        }
    }

    #[test]
    fn gateway_reads_apply_the_existing_secret_and_config_guards() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        std::fs::write(dir.path().join("private.pem"), "hidden-sentinel").unwrap();
        let req = Request {
            tool: "Read".into(),
            input: json!({"file_path":"private.pem","offset":1,"limit":1}),
            intent: String::new(),
            purpose: Default::default(),
            choose: false,
        };
        let error = answer(dir.path(), &req)
            .err()
            .expect("secret refused before reading");
        assert!(!error.contains("hidden-sentinel"));
        std::fs::create_dir(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join(".claude/settings.local.json"),r#"{"env":{"TYPESAFE_API_KEY":"private-env-sentinel"}}"#).unwrap();
        let mut req=req;
        req.input["file_path"]=json!(".claude/settings.local.json");
        let error=answer(dir.path(),&req).err().expect("private Jev key refused before reading");
        assert!(!error.contains("private-env-sentinel"));
        assert!(error.contains("***"));
    }
}
