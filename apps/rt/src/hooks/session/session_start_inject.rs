//! `session_start_inject` — o início da sessão, como uma lista de avisos.
//!
//! O `SessionStart` roda na abertura, depois de `/clear` e depois da
//! compactação. Cada aviso é um item de [`NOTICES`], com a sua condição e o seu
//! texto. Na ordem em que a janela os lê:
//!
//! 1. **Os textos declarados** — as entradas `on: sessionStart` de
//!    `mustard.json#inject`, onde mora o mapa do início da sessão; depois de
//!    `/clear` e da compactação, a linha curta do caminho dele.
//! 2. **A retomada** — a spec atual, a fase, o último passo e o próximo item,
//!    a mesma linha que o `resume` devolve; depois da compactação, o bloco de
//!    retomada. Quando a conversa da sessão não está ligada à spec — a que
//!    voltou pelo mesmo id ou depois do `/clear` —, a linha leva o aviso que
//!    manda rodar o `resume`.
//! 3. **As pendências** — uma linha só, com a contagem, uma vez por
//!    contagem: o projeto lembra a última, e o aviso só volta quando ela muda.
//!
//! O resto age sem falar. Na abertura e na retomada da conversa, o mapa
//! volta ao commit atual e o pull request feito por outra pessoa segue o
//! caminho do merge do Mustard (a spec entregue, a base atualizada, a branch
//! apagada). Os processos que um agente deixou presos são encerrados. No
//! código-fonte do Mustard, a compilação que falta é solta em segundo plano.
//! O disco, a versão e as branches mergeadas moram no `doctor`.
//!
//! Antes dos avisos, num projeto com `mustard.json`, a linha de versão do
//! programa que roda vira a marca da versão no projeto, quando difere da
//! última: o ponto de partida da medição do uso.
//!
//! ## Até 2.560 bytes
//!
//! Tudo junto cabe em [`MAX_BYTES`]. Quando o todo passa do teto, a contagem
//! das pendências cede primeiro, e só então os textos declarados. A retomada
//! nunca cede. O bloco de retomada da compactação encolhe as listas
//! ([`current_block_within`](crate::commands::flow::resume::current_block_within))
//! até caber ao lado do texto declarado.
//!
//! Nunca barra: devolve `Inject` com os avisos, ou `Allow` sem nenhum.

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::{Locale, translate};
use std::path::Path;

use crate::commands::event::pending::PendingCount;
use crate::commands::review::pr_door::{NOT_DELIVERED, merged_elsewhere, merged_outside};
use crate::hooks::session::injectables;
use crate::shared::development_build::{gap, start_in_background};

/// O teto do texto que o início da sessão coloca: o mapa do início da sessão
/// e a linha de retomada cabem nele.
pub(crate) const MAX_BYTES: usize = 2_560;

/// Os bytes que separam dois avisos: a linha em branco entre eles.
const SEPARATOR_BYTES: usize = 2;

/// O início da sessão.
pub struct SessionStartInject;

/// O que os avisos leem.
struct Probe<'a> {
    root: &'a Path,
    session: Option<&'a str>,
    lang: Locale,
    /// A janela foi renovada pela compactação: a retomada vem em bloco.
    compacted: bool,
    /// A conversa foi limpa (`/clear`): a retomada lista as ondas cujo agente
    /// a limpeza parou.
    cleared: bool,
    /// Os textos declarados, lidos uma vez antes dos avisos: o bloco de
    /// retomada precisa saber quanto lugar eles tomam.
    declared: Option<&'a str>,
    /// A linha das pendências, lida uma vez.
    pending: &'a PendingCount,
}

/// Um aviso do início da sessão: o nome, o texto — que só existe quando a
/// condição vale — e a vez dele de ceder o lugar quando o todo passa do teto:
/// o menor número sai primeiro, e `None` nunca sai.
struct Notice {
    name: &'static str,
    text: fn(&Probe<'_>) -> Option<String>,
    cedes: Option<u8>,
}

/// Os avisos, na ordem em que a janela os lê.
const NOTICES: &[Notice] = &[
    Notice { name: "declared", text: declared_notice, cedes: Some(1) },
    Notice { name: "resume", text: resume_notice, cedes: None },
    Notice { name: "pending", text: pending_notice_of, cedes: Some(0) },
];

impl Check for SessionStartInject {
    /// No código-fonte do Mustard, solta a compilação que falta, o único
    /// gesto que lê a máquina, e entrega o resto a [`session_start_core`].
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger == Some(Trigger::SessionStart) {
            start_development_build(Path::new(&ctx.project_dir_or_cwd(input)));
        }
        session_start_core(input, ctx, Some(env!("MUSTARD_VERSION_FULL")))
    }
}

/// Decide com o carimbo do programa que roda: `None` não grava a marca da
/// versão.
fn session_start_core(input: &HookInput, ctx: &Ctx, version: Option<&str>) -> Result<Verdict, Error> {
    if ctx.trigger != Some(Trigger::SessionStart) {
        return Ok(Verdict::Allow);
    }
    let cwd = ctx.project_dir_or_cwd(input);
    let root = Path::new(&cwd);
    // A marca da versão: a primeira sessão de cada compilação no projeto é o
    // ponto de partida da medição do uso. Falha ao gravar não muda a sessão.
    if let Some(version) = version.filter(|_| mustard_core::ProjectConfig::exists(root)) {
        let _ = mustard_core::io::measure::record(root, version);
    }
    let session = session_of(input);
    let source = input.raw.get("source").and_then(|v| v.as_str()).unwrap_or_default();
    let compacted = source.eq_ignore_ascii_case("compact");
    let refreshed = compacted || source.eq_ignore_ascii_case("clear");
    if refreshed {
        super::conversation_size::reset_conductor_notice(input, root);
    } else {
        // Depois de `/clear` e da compactação, a sessão é a mesma e o código
        // não saiu do lugar: o scan e a pergunta ao provedor ficam para a
        // abertura e a retomada da conversa. O mapa volta ao commit atual, e
        // o merge feito por outra pessoa age antes da retomada, que já lê o
        // estado novo.
        crate::commands::flow::round::refresh_map_if_stale(root, &|root, out| mustard_core::Scan::locate().scan_then_read_history(root, out));
        settle_merged_elsewhere(root, session.as_deref());
    }
    sweep_stuck_processes(root);
    let declared = injectables::collect(&root.to_string_lossy(), session.as_deref(), refreshed);
    let lang = crate::shared::context::config::project_config_cached(root).language().text_or_default();
    let pending = pending_notice(root, lang);
    let probe = Probe {
        root,
        session: session.as_deref(),
        lang,
        compacted,
        cleared: source.eq_ignore_ascii_case("clear"),
        declared: declared.as_deref(),
        pending: &pending,
    };
    // O aviso que conta só sai quando a contagem difere da última que ele deu
    // no projeto, em qualquer sessão; sem nada a contar, a contagem lembrada
    // é esquecida, e a próxima volta a ser novidade.
    let shown = within_cap(NOTICES.iter().filter_map(|notice| {
        let Some(text) = (notice.text)(&probe) else {
            if nothing_to_count(&probe, notice.name) {
                injectables::forget_in_project(root, notice.name);
            }
            return None;
        };
        let told = counts(notice.name) && injectables::told_in_project(root, notice.name, &text);
        (!told).then_some((notice, text))
    }).collect());
    // Só o aviso que coube no teto fica lembrado: o que cedeu o lugar volta.
    let texts: Vec<String> = shown.into_iter().map(|(notice, text)| {
        if counts(notice.name) {
            injectables::remember_in_project(root, notice.name, &text);
        }
        text
    }).collect();
    Ok(if texts.is_empty() { Verdict::Allow } else { Verdict::Inject { context: texts.join("\n\n") } })
}

/// Se o aviso `name` conta e o projeto lembra a contagem: só a linha das
/// pendências, que é feita só das contagens.
fn counts(name: &str) -> bool {
    name == "pending"
}

/// O aviso `name`, que conta, ficou sem nada a contar: nenhuma pendência
/// aberta. A leitura que falhou não diz isso, e a contagem lembrada fica.
fn nothing_to_count(probe: &Probe<'_>, name: &str) -> bool {
    counts(name) && *probe.pending == PendingCount::NoneOpen
}

/// Os avisos que cabem no teto, com os textos, na ordem da lista: enquanto o
/// todo passa de [`MAX_BYTES`], sai o aviso cuja vez de ceder vem primeiro,
/// com uma linha no stderr.
fn within_cap(mut shown: Vec<(&Notice, String)>) -> Vec<(&Notice, String)> {
    let size = |shown: &[(&Notice, String)]| shown.iter().map(|(_, text)| text.len()).sum::<usize>() + SEPARATOR_BYTES * shown.len().saturating_sub(1);
    while size(&shown) > MAX_BYTES {
        let next = shown.iter().enumerate().filter_map(|(at, (notice, _))| notice.cedes.map(|turn| (turn, at))).min();
        let Some((_, at)) = next else {
            break;
        };
        let (notice, text) = shown.remove(at);
        eprintln!("mustard: o aviso `{}` do início da sessão ({} bytes) saiu para o todo caber em {MAX_BYTES} bytes", notice.name, text.len());
    }
    shown
}

/// O id da sessão, quando o harness o mandou.
fn session_of(input: &HookInput) -> Option<String> {
    input.session_id.clone().or_else(|| input.raw.get("sessionId").and_then(|v| v.as_str()).map(str::to_string)).filter(|s| !s.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Os avisos
// ---------------------------------------------------------------------------

/// Os textos declarados para o início da sessão; com a janela renovada, eles
/// voltam mesmo com a marca de entregue.
fn declared_notice(probe: &Probe<'_>) -> Option<String> {
    probe.declared.map(str::to_string)
}

/// A retomada da spec atual: depois da compactação, o bloco de retomada; nas
/// outras aberturas, a linha, e o aviso de que a conversa não está ligada à
/// spec quando a abertura, a retomada ou o `/clear` deixou a sessão sem
/// ligação ([`unbound_notice`]).
///
/// O bloco nunca cede o lugar, e o texto declarado sairia no lugar dele
/// quando os dois passam do teto. As listas do bloco encolhem, e cada uma diz
/// quantos itens ficaram de fora: o bloco fica com o que cabe ao lado do texto
/// declarado. Quando nem o bloco mínimo cabe ao lado dele, o texto declarado
/// sai pelo teto de qualquer jeito, e o bloco fica com as listas.
fn resume_notice(probe: &Probe<'_>) -> Option<String> {
    use crate::commands::flow::resume::{current_block, current_block_within, current_line, lost_by_clear};
    if !probe.compacted {
        let line = current_line(probe.root, probe.session)?;
        let line = match probe.cleared.then(|| lost_by_clear(probe.root, probe.session)).flatten() {
            Some(lost) => format!("{line}\n{lost}"),
            None => line,
        };
        return Some(match unbound_notice(probe) {
            Some(unbound) => format!("{line}\n{unbound}"),
            None => line,
        });
    }
    let room = block_room(probe.declared);
    let block = current_block_within(probe.root, probe.session, room)?;
    if block.len() <= room {
        return Some(block);
    }
    current_block(probe.root, probe.session)
}

/// O aviso de que a conversa da sessão não está ligada a nenhuma spec, com o
/// comando que a liga. A sessão que terminou perde a ligação, e a que volta
/// pelo mesmo id (`claude --resume`) ou depois do `/clear` começa sem ela: sem
/// o `resume`, a fala e a resposta dela não entrariam na spec. `None` sem
/// sessão, que nada liga, e com a conversa ligada. Depois da compactação a
/// sessão é a mesma e a ligação segue, por isso o bloco não leva o aviso.
fn unbound_notice(probe: &Probe<'_>) -> Option<String> {
    let session = probe.session?;
    let unbound = crate::commands::spec_events::session_spec::bound(probe.root, Some(session)).is_none();
    unbound.then(|| translate("resume.unbound", probe.lang).to_string())
}

/// Quantos bytes do teto sobram ao bloco de retomada ao lado do texto
/// declarado e da linha em branco que os separa; o teto inteiro sem texto
/// declarado.
fn block_room(declared: Option<&str>) -> usize {
    MAX_BYTES.saturating_sub(declared.map_or(0, |text| text.len() + SEPARATOR_BYTES))
}

/// A contagem das pendências abertas ([`Probe::pending`]).
fn pending_notice_of(probe: &Probe<'_>) -> Option<String> {
    match probe.pending {
        PendingCount::Line(line) => Some(line.clone()),
        PendingCount::Unread | PendingCount::NoneOpen => None,
    }
}

/// Uma linha com a contagem de pendências abertas e o comando que mostra a
/// lista ([`count_line`](crate::commands::event::pending::count_line)), como
/// [`Probe::pending`] a guarda. Num projeto sem `mustard.json`, a lista não se
/// lê.
pub(crate) fn pending_notice(root: &Path, lang: Locale) -> PendingCount {
    if !mustard_core::ProjectConfig::exists(root) {
        return PendingCount::Unread;
    }
    crate::commands::event::pending::count_line(root, lang)
}

// ---------------------------------------------------------------------------
// O que age sem falar
// ---------------------------------------------------------------------------

/// O merge feito fora do Mustard, levado ao caminho do merge dele: a spec
/// atual, pelo que o provedor responde sobre o pull request dela
/// ([`merged_elsewhere`]), e cada spec do índice em execução, fechada ou com
/// o pull request aberto, só pelo git ([`merged_outside`]); a atual entra só
/// em execução ou fechada, porque com o pull request aberto quem responde por
/// ela é o provedor. Nada num projeto sem `mustard.json`, e a spec de antes
/// desta versão do Mustard fica como está.
fn settle_merged_elsewhere(root: &Path, session: Option<&str>) {
    if !mustard_core::ProjectConfig::exists(root) {
        return;
    }
    let legacy = mustard_core::io::legacy_specs::LegacySpecs::read(&mustard_core::io::spec_events::spec_root(root));
    let active = crate::shared::spec_state::DiskSpecState::new(root).active(session).filter(|spec| !legacy.contains(spec));
    if let Some(spec) = &active {
        let _ = merged_elsewhere(root, spec, session);
    }
    for line in mustard_core::io::spec_index::read(root).into_iter().filter(|line| !legacy.contains(&line.name)) {
        if line.phase.as_deref().is_some_and(|phase| NOT_DELIVERED.contains(&phase)) {
            let phases: &[&str] = if Some(&line.name) == active.as_ref() { &["running", "closed"] } else { &NOT_DELIVERED };
            let _ = merged_outside(root, &line.name, session, phases);
        }
    }
}

/// Encerra os processos que um agente deixou rodando — um laço de espera, ou
/// um comando na cópia de uma onda já apagada —, também aqui, não só a cada
/// rodada e no fechamento. A busca roda com a trava das cópias presa, e só
/// num projeto com o Mustard, onde a pasta da trava é dele. O início da
/// sessão não espera a trava: presa por uma rodada, a busca fica para ela.
/// Devolve os processos encerrados.
fn sweep_stuck_processes(root: &Path) -> Vec<crate::commands::flow::stuck::Detected> {
    if !mustard_core::ProjectConfig::exists(root) {
        return Vec::new();
    }
    crate::commands::git_settle::copies_lock_if_free(root)
        .map(|held| crate::commands::flow::stuck::find_stuck_processes(root, &held))
        .unwrap_or_default()
}

/// No código-fonte do Mustard, com o programa compilado da branch faltando
/// ou atrás do commit atual, solta a compilação em segundo plano — sem
/// esperar, e só uma de cada vez.
fn start_development_build(root: &Path) {
    if let Some(main) = mustard_core::mustard_checkout(root)
        && gap(&main).is_some()
    {
        let _ = start_in_background(&main);
    }
}

#[cfg(test)]
pub(crate) fn started_after_clear(root: &Path, session: &str) -> String {
    let input = HookInput {
        hook_event_name: Some("SessionStart".to_string()),
        session_id: Some(session.to_string()),
        raw: serde_json::json!({ "source": "clear" }),
        ..HookInput::default()
    };
    let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::SessionStart));
    match session_start_core(&input, &ctx, None) {
        Ok(Verdict::Inject { context }) => context,
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// A linha de versão que um teste entrega: nenhuma, e a marca da versão
    /// não se grava.
    const NO_VERSION: Option<&str> = None;

    fn ctx(dir: &Path) -> Ctx {
        Ctx::for_test(dir.to_string_lossy().into_owned(), Some(Trigger::SessionStart))
    }

    fn session_input(session_id: &str, source: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some(session_id.to_string()),
            raw: json!({ "source": source }),
            ..HookInput::default()
        }
    }

    fn context_of(root: &Path, input: &HookInput) -> String {
        match session_start_core(input, &ctx(root), NO_VERSION).unwrap() {
            Verdict::Inject { context } => context,
            _ => String::new(),
        }
    }

    /// Um projeto com o Mustard na versão que roda: o aviso de versão cala.
    fn installed_project(root: &Path) {
        std::fs::write(root.join("mustard.json"), format!(r#"{{"version":"{}"}}"#, mustard_core::harness_version())).unwrap();
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").args(args).current_dir(dir).output().expect("git on PATH");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Os avisos são uma lista, na ordem em que a janela os lê: o mapa, a
    /// retomada e a contagem das pendências. A contagem cede primeiro, o mapa
    /// depois, e a retomada nunca.
    #[test]
    fn the_notices_are_a_typed_list_in_reading_order() {
        let names: Vec<&str> = NOTICES.iter().map(|n| n.name).collect();
        assert_eq!(names, ["declared", "resume", "pending"]);
        let mut ceding: Vec<(u8, &str)> = NOTICES.iter().filter_map(|n| n.cedes.map(|turn| (turn, n.name))).collect();
        ceding.sort_unstable();
        let order: Vec<&str> = ceding.into_iter().map(|(_, name)| name).collect();
        assert_eq!(order, ["pending", "declared"]);
        let kept: Vec<&str> = NOTICES.iter().filter(|n| n.cedes.is_none()).map(|n| n.name).collect();
        assert_eq!(kept, ["resume"]);
    }

    /// O início da sessão de um projeto com o mapa gravado não leva o resumo
    /// do mapa: nenhuma linha de subprojeto e nenhuma das palavras que
    /// abriam o aviso, nos dois idiomas. O texto declarado do projeto segue
    /// chegando, para a conferência não passar só porque a saída ficou vazia.
    #[test]
    fn the_session_start_of_a_project_with_a_map_carries_no_terrain() {
        const MAP: &str = r#"{
          "projects": [
            {"name": "rt", "dir": "apps/rt", "kind": "cargo", "code_files": 232},
            {"name": "web", "dir": "apps/web", "kind": "npm", "code_files": 40}
          ],
          "skeleton": [
            {"dir": "apps/rt", "role": "L1"},
            {"dir": "apps/web", "role": "L0"}
          ]
        }"#;
        for lang in ["pt-BR", "en-US"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::write(
                root.join("mustard.json"),
                format!(
                    r#"{{"version":"{}","language":{{"text":"{lang}"}},"inject":[{{"on":"sessionStart","file":".claude/mustard/regras.md","once":true}}]}}"#,
                    mustard_core::harness_version()
                ),
            )
            .unwrap();
            std::fs::create_dir_all(root.join(".claude/mustard")).unwrap();
            std::fs::write(root.join(".claude/mustard/regras.md"), "TEXTO-DECLARADO\n").unwrap();
            mustard_core::io::project_map::write_text(root, MAP).unwrap();

            let context = context_of(root, &session_input("s1", "startup"));
            assert!(context.contains("TEXTO-DECLARADO"), "{lang}: the declared text still comes: {context}");
            for gone in ["Terreno", "Terrain", "subprojeto", "subproject", "apps/rt", "cargo ·", "npm ·"] {
                assert!(!context.contains(gone), "{lang}: `{gone}` is back in the session start: {context}");
            }
        }
    }

    /// Fora do início da sessão, nada; sem aviso nenhum, `Allow`.
    #[test]
    fn nothing_to_say_is_allow() {
        let dir = tempdir().unwrap();
        let other = Ctx::for_test(dir.path().to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        assert_eq!(SessionStartInject.evaluate(&session_input("s", "startup"), &other).unwrap(), Verdict::Allow);
        let verdict = session_start_core(&session_input("s", "startup"), &ctx(dir.path()), NO_VERSION);
        assert_eq!(verdict.unwrap(), Verdict::Allow);
    }

    /// Nenhuma fonte de abertura inicia publicação ou sincronização externa.
    #[test]
    fn session_start_never_requests_external_pages_or_spend_synchronization() {
        for language in ["pt-BR", "en-US"] {
            let project = tempdir().unwrap();
            let root = project.path();
            std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{language}"}}}}"#)).unwrap();
            for source in ["startup", "resume", "clear", "compact"] {
                let verdict = session_start_core(&session_input("local-only", source), &ctx(root), NO_VERSION).unwrap();
                if let Verdict::Inject { context } = verdict {
                    assert!(!context.contains("run spend") && !context.contains("ArtifactData") && !context.contains("write publish"), "{context}");
                }
                assert!(!root.join(".claude/mustard/pages").exists());
                assert!(!root.join(".claude/mustard/publications").exists());
            }
        }
    }

    /// A marca da versão nasce na primeira sessão de um projeto com
    /// `mustard.json`, não se repete na mesma linha de versão, venha a sessão
    /// de onde vier, e ganha uma linha nova a cada troca de versão, também na
    /// volta a uma anterior. Sem `mustard.json` nada se grava, e a falha ao
    /// gravar deixa a sessão igual.
    #[test]
    fn the_first_session_of_each_build_marks_the_version_in_the_project() {
        use mustard_core::io::measure::{marks, marks_path};
        let start = |root: &Path, source: &str, version: Option<&str>| {
            session_start_core(&session_input("s-marca", source), &ctx(root), version).unwrap()
        };
        let project = tempdir().unwrap();
        let root = project.path();
        installed_project(root);
        let (old, new) = ("1.0.0 (abc1234 2026-10-01)", "1.0.0 (def5678 2026-10-02)");
        let cases: [(&str, &str, &[&str]); 5] =
            [("startup", old, &[old]), ("startup", old, &[old]), ("clear", old, &[old]), ("resume", new, &[old, new]), ("startup", old, &[old, new, old])];
        for (source, version, expected) in cases {
            start(root, source, Some(version));
            let found = marks(root);
            let versions: Vec<&str> = found.iter().map(|mark| mark.version.as_str()).collect();
            assert_eq!(versions, expected, "{source} with {version}");
            assert!(found.iter().all(|mark| mustard_core::domain::spend::day_of_stamp(&mark.at).is_some()), "{found:?}");
        }

        let loose = tempdir().unwrap();
        start(loose.path(), "startup", Some(new));
        assert!(!marks_path(loose.path()).exists(), "no config, no mark");

        let blocked = tempdir().unwrap();
        installed_project(blocked.path());
        std::fs::create_dir_all(blocked.path().join(".claude")).unwrap();
        std::fs::write(blocked.path().join(".claude/mustard"), "").unwrap();
        assert_eq!(start(blocked.path(), "startup", Some(new)), start(blocked.path(), "startup", NO_VERSION));
    }

    /// O início da sessão de um projeto recém-semeado, nos dois idiomas, diz
    /// o que o gancho faz com o agente: o pedido e a tarefa quem grava é o
    /// condutor. A frase velha, que mandava todo agente gravar na spec, não
    /// volta, e cada gravação que o texto reserva ao condutor é mesmo recusada
    /// pela trava do terminal quando vem de um agente.
    #[test]
    fn the_session_start_says_the_conductor_records_what_the_hook_refuses_to_an_agent() {
        use crate::hooks::bash::command_guard::CommandGuard;
        for (lang, old, hook, conductor) in [
            (Locale::PtBr, "todo agente grava na spec", "o gancho", "o condutor grava"),
            (Locale::EnUs, "every agent records in the spec", "the hook", "the conductor records"),
        ] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let config = json!({
                "version": mustard_core::harness_version(),
                "language": {"text": lang.as_str()},
                "inject": mustard_core::platform::project_seed::default_inject_entries(),
            });
            std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
            mustard_core::platform::project_seed::seed_harness_texts(&root.join(".claude"), lang, Default::default()).unwrap();

            let context = context_of(root, &session_input("s-condutor", "startup"));
            assert!(!context.contains(old), "{lang:?}: the session start still tells every agent to record in the spec: {context}");
            let line = context
                .lines()
                .find(|line| line.contains(conductor))
                .unwrap_or_else(|| panic!("{lang:?}: the session start does not say the conductor records: {context}"));
            let refused = line
                .find(hook)
                .and_then(|from| line[from..].find(conductor).map(|to| &line[from..from + to]))
                .unwrap_or_else(|| panic!("{lang:?}: the line does not say what the hook refuses before the conductor records: {line}"));
            for kind in ["request", "task"] {
                assert!(refused.contains(kind), "{lang:?}: the hook's refusal does not name `run write {kind}`: {line}");
                let input = HookInput {
                    tool_name: Some("Bash".to_string()),
                    tool_input: json!({ "command": format!("mustard-rt run write {kind} --spec uma-spec --json '{{}}'") }),
                    hook_event_name: Some("PreToolUse".to_string()),
                    agent_id: Some("wave-agent".to_string()),
                    ..HookInput::default()
                };
                let guard = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
                let verdict = CommandGuard.evaluate(&input, &guard).unwrap();
                assert!(matches!(verdict, Verdict::Deny { .. }), "{lang:?}: the hook lets an agent run `run write {kind}`: {verdict:?}");
            }
        }
    }

    /// O que o gancho recusa ao agente não passa por outra forma do comando:
    /// o lote, cujas linhas levam tarefa, pedido ou onda na entrada padrão, e
    /// o tipo escrito depois das opções são recusados. O passo do agente
    /// segue livre, e o lote da conversa principal também.
    #[test]
    fn the_hook_refuses_an_agent_the_plan_through_a_batch_or_after_the_options() {
        use crate::hooks::bash::command_guard::CommandGuard;
        let dir = tempdir().unwrap();
        let ctx = Ctx::for_test(dir.path().to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        let verdict = |command: &str, agent: Option<&str>| {
            let input = HookInput {
                tool_name: Some("Bash".to_string()),
                tool_input: json!({ "command": command }),
                hook_event_name: Some("PreToolUse".to_string()),
                agent_id: agent.map(str::to_string),
                ..HookInput::default()
            };
            CommandGuard.evaluate(&input, &ctx).unwrap()
        };
        let task_batch = "mustard-rt run write --batch --spec uma-spec <<'EOF'\n{\"type\":\"task\",\"title\":\"Uma tarefa\"}\nEOF";
        for refused in [
            task_batch,
            "mustard-rt run write --spec uma-spec --batch < lote.ndjson",
            "mustard-rt run write --spec uma-spec --json '{}' task",
            "mustard-rt run write --root . wave --spec uma-spec --json '{}'",
        ] {
            let verdict = verdict(refused, Some("wave-agent"));
            assert!(matches!(verdict, Verdict::Deny { .. }), "the hook lets an agent run {refused:?}: {verdict:?}");
        }
        for (allowed, agent) in [
            ("mustard-rt run write step --spec uma-spec --json '{}'", Some("wave-agent")),
            ("mustard-rt run write --spec uma-spec --json '{}' delivered", Some("wave-agent")),
            (task_batch, None),
        ] {
            let verdict = verdict(allowed, agent);
            assert!(!matches!(verdict, Verdict::Deny { .. }), "the hook refuses {allowed:?} to {agent:?}: {verdict:?}");
        }
    }

    /// A abertura de um projeto recém-semeado, nos dois idiomas, com spec
    /// aberta, pendências, versão velha e branches mergeadas, é o mapa do
    /// início da sessão inteiro, a linha de retomada e a contagem, abaixo de
    /// 2.560 bytes. A versão velha e as branches mergeadas não falam na
    /// abertura: moram no `doctor`.
    #[test]
    fn the_opening_is_the_map_and_the_resume_line_under_the_cap() {
        use mustard_core::platform::i18n::Locale;
        assert_eq!(MAX_BYTES, 2_560);
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = tempdir().unwrap();
            let project = dir.path().join("projeto");
            let root = project.as_path();
            std::fs::create_dir_all(root).unwrap();
            git(root, &["init", "-q", "."]);
            git(root, &["config", "user.email", "t@t"]);
            git(root, &["config", "user.name", "t"]);
            git(root, &["config", "commit.gpgsign", "false"]);
            git(root, &["checkout", "-q", "-b", "dev"]);
            std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\n").unwrap();
            let config = json!({
                "version": "0.0.1-velha",
                "language": {"text": lang.as_str()},
                "git": {"flow": {"*": "dev"}},
                "inject": mustard_core::platform::project_seed::default_inject_entries(),
            });
            std::fs::write(root.join("mustard.json"), config.to_string()).unwrap();
            mustard_core::platform::project_seed::seed_harness_texts(&root.join(".claude"), lang, Default::default()).unwrap();
            std::fs::write(root.join("README.md"), "loja\n").unwrap();
            git(root, &["add", "README.md"]);
            git(root, &["commit", "-q", "-m", "seed"]);
            let landed = "feature/uma-entrega-ja-mergeada-pelo-colega";
            git(root, &["checkout", "-q", "-b", landed]);
            git(root, &["commit", "-q", "--allow-empty", "-m", "work"]);
            git(root, &["checkout", "-q", "dev"]);
            git(root, &["merge", "-q", "--no-ff", "-m", "merge", landed]);
            for title in ["Humanize", "HTML padrão da spec"] {
                let out = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
                    root: root.to_path_buf(),
                    add: true,
                    title: Some(title.to_string()),
                    detail: Some("combinado na conversa".to_string()),
                    ..crate::commands::event::pending::PendingOpts::default()
                });
                assert_eq!(out["ok"], json!(true), "seed: {out}");
            }
            let spec = "relatorios-dos-agentes-aceitos-como-vem";
            let branch = format!("feature/{spec}");
            git(root, &["checkout", "-q", "-b", &branch]);
            assert_eq!(crate::commands::spec_events::write::record_open(root, spec, &branch, "dev"), Ok(true));

            let map = mustard_core::session_map(lang).trim().to_string();
            let context = context_of(root, &session_input("s-mapa", "startup"));
            assert!(context.contains(&map), "{lang:?}: the whole map arrives: {context}");
            let resume = crate::commands::flow::resume::current_line(root, Some("s-mapa")).expect("an open spec");
            assert!(context.contains(&resume), "{lang:?}: the resume stays: {context}");
            assert!(context.len() < MAX_BYTES, "{lang:?}: {} bytes", context.len());
            assert!(!context.contains("0.0.1-velha") && !context.contains(landed), "{lang:?}: the version and the branches stay out: {context}");
        }
    }

    /// O texto declarado sai uma vez por sessão e volta depois de `/clear` e
    /// da compactação.
    #[test]
    fn the_declared_text_comes_once_and_again_after_clear_or_compact() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"version":"{}","inject":[{{"on":"sessionStart","file":".claude/mustard/mapa.md","once":true}}]}}"#, mustard_core::harness_version()),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".claude/mustard")).unwrap();
        std::fs::write(root.join(".claude/mustard/mapa.md"), "MAPA-DO-INICIO\n").unwrap();

        assert!(context_of(root, &session_input("s1", "startup")).contains("MAPA-DO-INICIO"));
        assert!(!context_of(root, &session_input("s1", "resume")).contains("MAPA-DO-INICIO"));
        for source in ["clear", "compact"] {
            assert!(context_of(root, &session_input("s1", source)).contains("MAPA-DO-INICIO"), "{source} brings it back");
        }
    }

    /// As pendências abertas viram uma linha só, com a contagem e sem título.
    #[test]
    fn the_pending_notice_is_one_line_with_the_count() {
        let lang = Locale::PtBr;
        let bare = tempdir().unwrap();
        assert_eq!(pending_notice(bare.path(), lang), PendingCount::Unread, "not installed");

        let dir = tempdir().unwrap();
        let root = dir.path();
        installed_project(root);
        assert_eq!(pending_notice(root, lang), PendingCount::NoneOpen, "nothing open");
        for title in ["HTML padrao da spec", "Humanize"] {
            let out = crate::commands::event::pending::pending_at(&crate::commands::event::pending::PendingOpts {
                root: root.to_path_buf(),
                add: true,
                title: Some(title.to_string()),
                detail: Some("combinado na conversa".to_string()),
                ..crate::commands::event::pending::PendingOpts::default()
            });
            assert_eq!(out["ok"], json!(true), "seed: {out}");
        }
        let context = context_of(root, &session_input("s-pend", "startup"));
        let line = translate("pending.count.many", lang).replace("{count}", "2");
        assert!(context.contains(&line), "the count line: {context}");
        assert!(!context.contains("Humanize") && !context.contains("P-1"), "no item is listed: {context}");
        for source in ["clear", "compact"] {
            let again = context_of(root, &session_input("s-pend", source));
            assert!(!again.contains(&line), "unchanged pending count must not repeat: {again}");
        }
    }

    /// O mapa do projeto acompanha o commit atual desde o início da sessão,
    /// pela mesma conferência da rodada: um mapa que ficou para trás do
    /// commit do checkout nunca trava a sessão, mesmo sem a ferramenta do
    /// scan instalada no ambiente do teste — o início da sessão continua
    /// respondendo normalmente, com o resto dos avisos.
    #[test]
    fn the_session_start_never_breaks_on_a_stale_map() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        installed_project(root);
        std::fs::write(root.join("README.md"), "loja\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "seed"]);

        mustard_core::io::project_map::write_text(root, &json!({"modules": [], "state": {"head": "0000000000000000000000000000000000000000"}}).to_string())
            .unwrap();

        let ctx = ctx(root);
        let input = session_input("s-stale-map", "startup");
        assert!(session_start_core(&input, &ctx, NO_VERSION).is_ok(), "a sessão não trava com o mapa velho");
    }

    /// O início da sessão procura processo preso só com a trava das cópias
    /// livre, e nunca espera por ela: presa por uma rodada, nada é encerrado
    /// e o comando esquecido numa vaga sem onda fica; solta a trava, a mesma
    /// busca o encerra, sem texto na abertura. Num projeto sem o Mustard,
    /// nada é procurado e a pasta da trava não nasce.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_session_start_sweeps_only_with_the_copies_lock_free_and_never_waits_for_it() {
        use crate::commands::git_settle::{copies_lock, waiting_for_lock};
        use std::os::unix::fs::MetadataExt;
        use std::process::{Command, Stdio};

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        crate::commands::flow::round::copies_leave_with_the_test(root);
        let slot = mustard_core::io::wave_prompt::slot_path(root, "x", 0);
        std::fs::create_dir_all(slot.join("src")).unwrap();
        std::fs::write(slot.join(".git"), "gitdir: /nowhere\n").unwrap();
        let mut idle = Command::new("sleep")
            .arg("30")
            .current_dir(slot.join("src"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn a command left in a slot with no wave");
        crate::commands::flow::stuck::wait_until_spawned(idle.id(), "sleep");

        let held = copies_lock(root).unwrap();
        let inode = std::fs::metadata(root.join(".claude").join("spec").join("round-copies.lock")).unwrap().ino();
        let (waited, while_held) = std::thread::scope(|scope| {
            let asked = scope.spawn(|| sweep_stuck_processes(root));
            while !asked.is_finished() && !waiting_for_lock(inode) {
                std::thread::yield_now();
            }
            let waited = waiting_for_lock(inode);
            drop(held);
            (waited, asked.join().unwrap())
        });
        let kept = idle.try_wait().ok().flatten().is_none();
        let freed = sweep_stuck_processes(root);
        let _ = idle.kill();
        let _ = idle.wait();
        assert!(!waited, "the session start never waits for the copies lock");
        assert!(while_held.is_empty(), "with the lock held by a round, nothing is swept");
        assert!(kept, "the command in the slot waits for the round to sweep");
        assert_eq!(freed.len(), 1, "with the lock free, the command left in the slot is ended");
        assert!(context_of(root, &session_input("s-preso", "startup")).is_empty(), "the sweep says nothing in the opening");

        let outside = tempdir().unwrap();
        assert!(sweep_stuck_processes(outside.path()).is_empty());
        assert!(!outside.path().join(".claude").exists(), "no lock folder in a project without the Mustard");
    }

    /// Um projeto na sessão `s-room`, com o texto declarado `declared` no
    /// início da sessão e a spec `x` aprovada, com `count` decisões gravadas
    /// depois da última rodada: o bloco de retomada cresce com elas.
    fn compacted_project(root: &Path, lang: Locale, declared: &str, count: usize) {
        std::fs::write(
            root.join("mustard.json"),
            json!({
                "version": mustard_core::harness_version(),
                "language": {"text": lang.as_str()},
                "inject": [{"on": "sessionStart", "file": ".claude/mustard/session-map.md", "once": false}],
            })
            .to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".claude/mustard")).unwrap();
        std::fs::write(root.join(".claude/mustard/session-map.md"), declared).unwrap();
        crate::shared::spec_state::stand_on_spec_branch(root, "x");
        crate::commands::spec_events::write::record_open(root, "x", "feature/x", "dev").expect("open");
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join("x"));
        let said = crate::shared::spec_state::seed_event(root, "x", "message", json!({"author": "user", "text": "muitas decisões"}));
        crate::shared::spec_state::seed_event(root, "x", "call", json!({"command": "round", "ms": 3, "result": "ok", "author": "binary"}));
        for at in 0..count {
            crate::shared::spec_state::seed_event(
                root,
                "x",
                "decision",
                json!({"text": format!("Decisão {at}."), "keys": ["retomada"], "why": "o usuário pediu", "origin": said}),
            );
        }
    }

    /// O início da sessão depois da compactação, na sessão `s-room`.
    fn after_compaction(root: &Path) -> String {
        context_of(root, &session_input("s-room", "compact"))
    }

    /// Depois da compactação, o mapa do início da sessão não cai a cada
    /// recomeço: com tantos códigos gravados que o bloco de retomada inteiro
    /// não cabe ao lado dele, o bloco encolhe as listas e diz quantos itens
    /// ficaram de fora, o mapa chega inteiro e o todo cabe no teto. Nos dois
    /// idiomas.
    #[test]
    fn the_session_map_survives_a_long_resume_block_after_compaction() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let map = mustard_core::session_map(lang).trim().to_string();
            compacted_project(root, lang, &map, 60);
            let full = crate::commands::flow::resume::current_block(root, Some("s-room")).expect("the block");
            let context = after_compaction(root);
            assert!(context.contains("mcp__mustard__search") && context.contains(".claude/mustard/session-map.md"));
            assert!(context.len() <= MAX_BYTES, "{lang:?}: {} bytes", context.len());
            assert!(context.contains(&full), "the compact guide makes room for the full recorded state");
            assert!(context.contains("mustard-rt run round --dispatch --spec x"));

        }
    }

    /// Depois da compactação, a janela renovada leva a linha curta do mapa;
    /// quando o bloco de retomada inteiro cabe ao lado dela, nada encolhe: os
    /// dois chegam inteiros.
    #[test]
    fn the_resume_block_keeps_every_list_when_it_fits_beside_the_map() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let map = mustard_core::session_map(Locale::PtBr).trim().to_string();
        compacted_project(root, Locale::PtBr, &map, 12);
        let full = crate::commands::flow::resume::current_block(root, Some("s-room")).expect("the block");
        let guide = translate("session.guide", Locale::PtBr);
        assert!(full.len() <= block_room(Some(guide)), "the whole block fits beside the guide: {} bytes", full.len());

        let context = after_compaction(root);
        assert!(context.contains(".claude/mustard/session-map.md") && context.contains(&full), "routing and state arrive: {context}");
    }

    /// Quando nem o bloco de retomada mínimo cabe ao lado do texto declarado,
    /// o bloco não encolhe à toa: o texto declarado sai pelo teto, como
    /// sempre, e o bloco chega com as listas que cabem no teto sozinho.
    #[test]
    fn a_declared_text_that_cannot_stay_beside_the_smallest_block_goes_and_the_block_keeps_its_lists() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let huge = "d".repeat(MAX_BYTES - 100);
        compacted_project(root, Locale::PtBr, &huge, 30);
        let full = crate::commands::flow::resume::current_block(root, Some("s-room")).expect("the block");
        let smallest = crate::commands::flow::resume::current_block_within(root, Some("s-room"), 0).expect("the block");
        assert!(smallest.len() > block_room(Some(&huge)), "not even the smallest block fits beside the text");

        let context = after_compaction(root);
        assert_eq!(context, full, "the block stays whole and the declared text went: {context}");
    }
}
