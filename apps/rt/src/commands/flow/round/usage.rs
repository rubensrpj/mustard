//! O consumo de cada onda que a rodada assume, medido nos arquivos de
//! conversa que a plataforma grava, e não num número que alguém digita: o do
//! agente que recebeu o pedido da onda, no arquivo dele, e o da conversa
//! principal — a do orquestrador — no ramo da spec. A leitura dos arquivos é
//! a de [`mustard_core::io::transcript`]; aqui fica só o que a rodada pergunta
//! a ela e o que grava no envio da onda.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::io::transcript;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

/// Quem chama a rodada ou o fechamento: a sessão da conversa principal e a
/// pasta de configuração da plataforma, onde ela grava o arquivo de conversa
/// dessa sessão e os dos agentes que a sessão abriu. As duas são resolvidas
/// na entrada do comando, e o teste passa as dele sem mexer no ambiente. Sem
/// a pasta, só os recibos explícitos de transcript podem medir a onda.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Caller<'a> {
    pub session: Option<&'a str>,
    pub config_dir: Option<&'a Path>,
}

/// O consumo de uma onda, que a rodada mede ([`measure_usage`]): o modelo
/// que o agente usou de verdade, os passos que deu e os tokens que gastou,
/// lidos do arquivo dele, e o consumo da conversa principal — a do
/// orquestrador, não a da onda — no ramo da spec, do começo dela até esta
/// rodada. O campo sem valor é o que não foi achado.
#[derive(Debug, Clone, Default)]
pub(crate) struct Usage {
    pub model_used: Option<String>,
    pub steps: Option<u64>,
    pub tokens: Option<u64>,
    pub caller_steps: Option<u64>,
    pub caller_tokens: Option<u64>,
    pub breakdown: Option<transcript::TokenBreakdown>,
    pub caller_breakdown: Option<transcript::TokenBreakdown>,
}

impl Usage {
    /// Os campos medidos, com os nomes que o envio grava; vazio sem nenhum.
    pub(super) fn fields(&self) -> Map<String, Value> {
        let mut out = Map::new();
        for (key,value) in [("usage_breakdown",&self.breakdown),("caller_usage_breakdown",&self.caller_breakdown)] {
            if let Some(value)=value {out.insert(key.into(),json!(value));}
        }
        if let Some(model) = &self.model_used {
            out.insert("model_used".into(), json!(model));
        }
        for (key, value) in [("steps", self.steps), ("tokens", self.tokens), ("caller_steps", self.caller_steps), ("caller_tokens", self.caller_tokens)] {
            if let Some(value) = value {
                out.insert(key.into(), json!(value));
            }
        }
        out
    }
}

/// Mede, nos arquivos de conversa da plataforma de `caller`, o consumo de
/// cada onda de `waves` — as que a rodada assume agora e as cuja linha
/// `USAGE` chegou depois de assumidas —, trocando o de cada uma: o do agente
/// da onda, no arquivo dele, e o da conversa principal, o mesmo para todas,
/// no ramo da spec desde o começo dela. Sem a pasta da sessão, nada é medido.
pub(super) fn measure_usage<'u>(root: &Path, log: &SpecLog, caller: Caller<'_>, waves: impl IntoIterator<Item = (u64, &'u mut Usage)>) {
    let session = (crate::shared::host_observation::caller_host(root,caller.session)!="codex").then_some(caller.config_dir.zip(caller.session)).flatten().and_then(|(dir, id)| transcript::session_dir(dir, id));
    let main = session.as_deref().and_then(|session|main_usage(log,session));
    for (wave, usage) in waves {
        let spec=log.last_by_wave("send").get(&wave).and_then(|id|log.get(*id)).and_then(|e|e.str_field("text"))
            .and_then(|text|mustard_core::domain::wave_prompt::ReadDispatch::of(text).map(|r|r.spec)
                .or_else(||text.lines().next().and_then(|heading|mustard_core::domain::wave_prompt::wave_of_title(heading,Locale::PtBr).or_else(||mustard_core::domain::wave_prompt::wave_of_title(heading,Locale::EnUs))).map(|r|r.0)));
        let native=spec.as_deref().and_then(|spec|crate::shared::host_observation::wave_usage(root,spec,log,wave));
        let own = native.as_ref().map(|measured|measured.usage.clone())
            .or_else(||session.as_deref().and_then(|session|wave_usage(root, log, session, wave)));
        *usage = Usage {
            model_used: own.as_ref().and_then(|own| own.model.clone()),
            steps: native.as_ref().map_or_else(||own.as_ref().map(|own|own.steps),|measured|measured.steps),
            tokens: own.as_ref().map(|own| own.tokens),
            caller_steps: main.as_ref().map(|main| main.steps),
            caller_tokens: main.as_ref().map(|main| main.tokens),
            breakdown: own.as_ref().map(|own|own.breakdown.clone()),
            caller_breakdown: main.as_ref().map(|main|main.breakdown.clone()),
        };
    }
}

/// O aviso de que o arquivo de conversa do agente da onda `wave` não foi
/// achado: o envio dela fica sem o consumo, e a resposta nomeia a onda.
pub(super) fn usage_missing(wave: u64, lang: Locale) -> Value {
    json!({
        "reason": "usage-missing",
        "wave": wave,
        "hint": translate("round.usage_missing", lang).replace("{wave}", &wave.to_string()),
    })
}

/// Read-only live measurement. The panel caches this projection separately
/// from the event log; observing a running agent never changes its dispatch.
pub(crate) fn snapshot(root: &Path, log: &SpecLog, session: Option<&str>, config: Option<&Path>) -> Value {
    // Meter actual sends, including work later removed from the plan.
    // A retired task must not erase its already incurred usage.
    let mut waves: Vec<_> = log.last_by_wave("send").keys().map(|wave|(*wave,Usage::default())).collect();
    measure_usage(root, log, Caller {session,config_dir:config}, waves.iter_mut().map(|(n,u)|(*n,u)));
    json!({"measured_at":chrono::Utc::now().to_rfc3339(),"waves":waves.into_iter().filter_map(|(wave,usage)| {
        let send = log.visible().into_iter().rev().find(|event|event.event_type=="send" && event.wave()==Some(wave))?;
        Some(json!({"wave":wave,"dispatch":log.dispatch_position(send.id),"usage":usage.fields()}))
    }).collect::<Vec<_>>()})
}

/// O consumo do agente que recebeu o pedido da onda `wave`, no arquivo dele:
/// o agente cujo pedido abre com a primeira linha do texto do envio que
/// despachou a onda e que começou depois dele, procurado na pasta da sessão
/// (`session`) e, quando ela não o tem — a onda saiu antes de um `/clear` —,
/// nas das outras sessões do mesmo projeto. A conversa do agente que
/// atravessou um `/clear` soma todos os pedaços dela
/// ([`transcript::agent_pieces`]); a linha repetida entre dois pedaços conta
/// uma vez, como toda resposta e todo uso de ferramenta.
///
/// O envio que outro envio da onda substituiu sem que nenhum agente o
/// recebesse — nenhum arquivo, em nenhuma sessão do projeto, entre ele e o
/// envio seguinte — não gastou nada e não conta: a rodada reenvia a onda
/// órfã ou pausada mesmo quando nenhum agente chegou a nascer do envio
/// anterior. Já o último envio é o do agente que entregou: sem o arquivo
/// dele, o consumo da onda seria só o das tentativas anteriores, e a medição
/// fica de fora. `None` quando o arquivo do último envio não é achado ou
/// nenhum pedaço se lê.
fn wave_usage(root: &Path, log: &SpecLog, session: &Path, wave: u64) -> Option<transcript::Usage> {
    let sends: Vec<_> = log.events.iter().filter(|e| e.event_type == "send" && e.wave() == Some(wave) && log.dispatch_position(e.id) == e.id).collect();
    let mut pieces = std::collections::BTreeSet::new();
    for (at, sent) in sends.iter().enumerate() {
        let title = sent.str_field("text")?.lines().next()?;
        let spec = mustard_core::domain::wave_prompt::wave_of_title(title, Locale::PtBr)
            .or_else(|| mustard_core::domain::wave_prompt::wave_of_title(title, Locale::EnUs))?.0;
        let read = mustard_core::domain::wave_prompt::ReadDispatch { spec, wave, root: mustard_core::io::wave_prompt::shown(root) };
        let until = sends.get(at + 1).map(|s| s.at());
        let files = transcript::wave_agent_files(session, title, sent.at(), until, &read);
        if files.is_empty() && until.is_none() {
            return None;
        }
        for file in files {
            pieces.extend(transcript::agent_pieces(&file));
        }
    }
    let texts: Vec<String> = pieces.into_iter().map(std::fs::read_to_string).collect::<Result<_, _>>().ok()?;
    let usage=transcript::usage_of(texts.iter().flat_map(|t| t.lines()));
    (usage.model.is_some() || usage.tokens>0).then_some(usage)
}

/// O consumo da conversa principal: as linhas dela no ramo da spec, desde o
/// primeiro evento da spec, em todas as conversas da pasta do projeto que
/// guarda a sessão (`session`). `None` sem ramo gravado ou sem pasta que se
/// leia.
fn main_usage(log: &SpecLog, session: &Path) -> Option<transcript::Usage> {
    let branch = State::from_log(log).branch?;
    let since = log.events.first()?.at();
    transcript::orchestrator_usage(session.parent()?, &branch, since)
}

/// Os arquivos de conversa da plataforma, montados como ela os grava: os
/// ajudantes daqui servem também aos testes da volta, em `report`.
#[cfg(test)]
pub(super) mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::Caller;
    use crate::commands::flow::round::tests::{approved, line, request_at, returned, round};
    use crate::commands::flow::round::{RoundOpts, round_in};

    /// O modelo que a plataforma grava nas respostas de um agente.
    pub(crate) const MODEL: &str = "claude-opus-5-5";

    /// O instante `at` da spec deslocado de `millis`, em UTC, preservando a
    /// precisão original: deslocamento zero precisa continuar na fronteira.
    pub(crate) fn instant(at: &str, millis: i64) -> String {
        let at = chrono::DateTime::parse_from_rfc3339(at).unwrap().with_timezone(&chrono::Utc);
        (at + chrono::Duration::milliseconds(millis)).to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
    }

    /// Um arquivo de conversa em `<config>/projects/<project>/<relative>`, no
    /// lugar em que a plataforma o grava: `<sessão>.jsonl` para a conversa
    /// principal e `<sessão>/subagents/agent-<nome>.jsonl` para um agente
    /// dela.
    pub(crate) fn platform_file(config: &Path, project: &str, relative: &str, lines: &[Value]) {
        let path = config.join("projects").join(project).join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        std::fs::write(path, text.join("\n") + "\n").unwrap();
    }

    /// A primeira linha do arquivo de um agente: o pedido `request` que ele
    /// recebeu em `at`.
    pub(crate) fn request_line(at: &str, request: &str) -> Value {
        json!({"type": "user", "isSidechain": true, "gitBranch": "feature/x", "timestamp": at,
            "message": {"role": "user", "content": request}})
    }

    /// Uma linha de resposta do modelo como a plataforma grava: o carimbo, o
    /// ramo, se é de agente, o id da resposta, os quatro números — entrada,
    /// criação de cache, leitura de cache e saída — e os usos de ferramenta;
    /// sem nenhum, a resposta é só texto.
    pub(crate) fn answer_line(at: &str, branch: &str, sidechain: bool, id: &str, usage: [u64; 4], tools: &[&str]) -> Value {
        let content: Vec<Value> = if tools.is_empty() {
            vec![json!({"type": "text", "text": "pronto"})]
        } else {
            tools.iter().map(|tool| json!({"type": "tool_use", "id": tool, "name": "Bash", "input": {}})).collect()
        };
        json!({"type": "assistant", "isSidechain": sidechain, "gitBranch": branch, "timestamp": at,
            "message": {"id": id, "model": MODEL, "role": "assistant", "content": content,
                "usage": {"input_tokens": usage[0], "cache_creation_input_tokens": usage[1],
                    "cache_read_input_tokens": usage[2], "output_tokens": usage[3]}}})
    }

    /// A conversa principal de uma sessão, só com a mensagem de quem a abriu.
    fn session(config: &Path, project: &str, name: &str, at: &str) {
        let opened = json!({"type": "user", "isSidechain": false, "gitBranch": "feature/x", "timestamp": at,
            "message": {"role": "user", "content": "siga a obra"}});
        platform_file(config, project, &format!("{name}.jsonl"), &[opened]);
    }

    /// O arquivo de um agente: o pedido `request` que ele recebeu em `start` e
    /// uma resposta `id`, no mesmo instante, com um uso de ferramenta e os
    /// quatro números.
    fn agent(config: &Path, project: &str, relative: &str, start: &str, request: &str, id: &str, usage: [u64; 4]) {
        let tool = format!("{id}-uso");
        let answered = answer_line(start, "feature/x", true, id, usage, &[&tool]);
        platform_file(config, project, relative, &[request_line(start, request), answered]);
    }

    /// A onda saiu na sessão antiga, e um `/clear` abriu a sessão nova, de
    /// onde a rodada assume a volta: a pasta da sessão nova não tem o agente
    /// da onda, só o de outra, e a rodada o acha na pasta da sessão antiga do
    /// mesmo projeto, pelo título do pedido e pela hora do envio. Lá, o mesmo
    /// pedido começado um milésimo antes do envio é de um envio anterior, e o
    /// começado um milésimo depois do agente da onda perde para ele, o mais
    /// perto do envio. O mesmo pedido na pasta de outro projeto, começado
    /// ainda mais perto do envio, não conta.
    #[test]
    fn the_round_finds_the_wave_agent_dispatched_before_a_clear() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let out = round(root, "x", None);
        let prompt = request_at(&out, 0);
        assert!(prompt.starts_with("# "), "o pedido abre com o título da onda: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let send = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap().clone();
        let sent = send.at();

        let platform = tempdir().unwrap();
        let config = platform.path();
        session(config, "-tmp-obra", "antiga", &instant(sent, -1_000));
        let read = format!("Idiomas deste projeto.\n\nLeia o seu pedido com: mustard-rt run read request-1 --root {} --spec x\nConclua e entregue.",mustard_core::io::wave_prompt::shown(root));
        agent(config, "-tmp-obra", "antiga/subagents/agent-onda.jsonl", &instant(sent, 200), &read, "r1", [2, 100, 1_000, 40]);
        agent(config, "-tmp-obra", "antiga/subagents/agent-outro-root.jsonl", &instant(sent, 100), &read.replace(&mustard_core::io::wave_prompt::shown(root),"/another/project"), "foreign", [90_000,0,0,0]);
        agent(config, "-tmp-obra", "antiga/subagents/agent-antes.jsonl", &instant(sent, -1), &prompt, "a1", [9_000, 0, 0, 0]);
        agent(config, "-tmp-obra", "antiga/subagents/agent-depois.jsonl", &instant(sent, 201), &prompt, "d1", [7_000, 0, 0, 0]);
        session(config, "-tmp-obra", "nova", &instant(sent, 50));
        let other_wave = "# x — onda 2\n\nOutro pedido.\n";
        agent(config, "-tmp-obra", "nova/subagents/agent-outra.jsonl", &instant(sent, 100), other_wave, "b1", [8_000, 0, 0, 0]);
        session(config, "-tmp-outro", "alheia", &instant(sent, -1_000));
        agent(config, "-tmp-outro", "alheia/subagents/agent-onda.jsonl", &instant(sent, 100), &prompt, "o1", [6_000, 0, 0, 0]);

        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let delivery = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, delivery)["ok"], json!(true));
        let opts = RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: Some(line("USAGE", json!({"wave": 1}))) };
        super::super::review::test_conductor_review(root, "x");
        let out = round_in(&opts, Caller { session: Some("nova"), config_dir: Some(config) });
        assert_eq!(out["ok"], json!(true), "{out}");
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.iter().all(|w| w["reason"] != json!("usage-missing")), "o arquivo foi achado: {out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let revised = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert_eq!(revised.replaced(), vec![send.id], "a versão nova aponta o envio original");
        // 2 + 100 + 1000 + 40: o agente da onda na sessão antiga, e nenhum outro.
        assert_eq!(revised.int("tokens"), Some(1_142), "{revised:?}");
        assert_eq!(revised.int("steps"), Some(1), "{revised:?}");
        assert_eq!(revised.str_field("model_used"), Some(MODEL), "{revised:?}");
    }

    /// A onda atravessou dois `/clear` de quem conduz, e a conversa do agente
    /// ficou em três pedaços, um na pasta de cada sessão: o pedido e a
    /// resposta `r1` na sessão antiga, `r2` na do meio e, na nova, `r2` de
    /// novo — a linha que a plataforma repete na troca — e `r3`. A rodada
    /// grava a conversa inteira, cada resposta e cada uso de ferramenta uma
    /// vez.
    #[test]
    fn the_round_sums_every_piece_of_the_wave_agent_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let prompt = request_at(&round(root, "x", None), 0);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap().at().to_string();

        let platform = tempdir().unwrap();
        let config = platform.path();
        let at = |millis| instant(&sent, millis);
        let answer = |millis, id: &str, usage, tools: &[&str]| answer_line(&at(millis), "feature/x", true, id, usage, tools);
        let second = answer(60_000, "r2", [3, 0, 2_000, 50], &["t2", "t3"]);
        for (name, start) in [("antiga", -1_000), ("meio", 50_000), ("nova", 100_000)] {
            session(config, "-tmp-obra", name, &at(start));
        }
        let piece = |name: &str| format!("{name}/subagents/agent-onda.jsonl");
        let first = answer(300, "r1", [2, 100, 1_000, 40], &["t1"]);
        platform_file(config, "-tmp-obra", &piece("antiga"), &[request_line(&at(200), &prompt), first]);
        platform_file(config, "-tmp-obra", &piece("meio"), std::slice::from_ref(&second));
        platform_file(config, "-tmp-obra", &piece("nova"), &[second, answer(120_000, "r3", [4, 0, 3_000, 60], &["t4"])]);

        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let delivery = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, delivery)["ok"], json!(true));
        let opts = RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: Some(line("USAGE", json!({"wave": 1}))) };
        super::super::review::test_conductor_review(root, "x");
        assert_eq!(round_in(&opts, Caller { session: Some("nova"), config_dir: Some(config) })["ok"], json!(true));

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let revised = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        // r1 = 1.142, r2 = 2.053 e r3 = 3.064, cada uma uma vez; t1 a t4.
        assert_eq!(revised.int("tokens"), Some(1_142 + 2_053 + 3_064), "{revised:?}");
        assert_eq!(revised.int("steps"), Some(4), "{revised:?}");
    }

    /// A onda 1 da obra `x` sai e, antes de qualquer agente nascer dela, a
    /// rodada seguinte a reenvia (a onda pausada sai de novo). Devolve a hora
    /// do primeiro envio e a do reenvio, e o texto curto com que o condutor
    /// despacha a onda: o título e o comando que lê o pedido.
    fn resent_wave(root: &Path) -> (String, String, String) {
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let resent = round(root, "x", Some(&line("PAUSED", json!({"wave": 1}))));
        assert_eq!(resent["dispatch"][0]["wave"], json!(1), "a onda pausada sai de novo: {resent}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sends: Vec<String> = log.visible().into_iter().filter(|e| e.event_type == "send" && e.wave() == Some(1)).map(|e| e.at().to_string()).collect();
        assert_eq!(sends.len(), 2, "o envio e o reenvio: {sends:?}");
        let dispatch = super::super::answer::wave_dispatch(root, "x", 1, mustard_core::platform::i18n::Locale::PtBr);
        (sends[0].clone(), sends[1].clone(), dispatch)
    }

    /// A volta da onda 1 da obra `x`, assumida pela rodada com a linha `USAGE`
    /// só com o número da onda, na sessão `session` da pasta da plataforma
    /// `config`. Devolve a resposta e o envio que a rodada gravou com o
    /// consumo.
    fn measured_return(root: &Path, config: &Path, session: &str) -> (Value, mustard_core::domain::spec_events::SpecEvent) {
        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let delivery = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, delivery)["ok"], json!(true));
        let opts = RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: Some(line("USAGE", json!({"wave": 1}))) };
        super::super::review::test_conductor_review(root, "x");
        let out = round_in(&opts, Caller { session: Some(session), config_dir: Some(config) });
        assert_eq!(out["ok"], json!(true), "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let revised = log.get(log.last_by_wave("send")[&1]).unwrap().clone();
        (out, revised)
    }

    /// Se a resposta `out` avisa que o consumo da onda 1 ficou de fora.
    fn usage_missing(out: &Value) -> bool {
        out["warnings"].as_array().into_iter().flatten().any(|w| w["reason"] == json!("usage-missing") && w["wave"] == json!(1))
    }

    /// O aplicativo de desktop grava o agente da onda em
    /// `<sessão>/subagents/agent-<id>.jsonl`, com um `agent-<id>.meta.json`
    /// ao lado, e o agente recebe o texto curto do despacho. A onda saiu e foi
    /// reenviada antes de qualquer agente nascer do primeiro envio: só o
    /// reenvio tem agente. A rodada acha o arquivo dele e grava o consumo; o
    /// envio sem agente não deixa a onda sem medição.
    #[test]
    fn the_desktop_agent_transcript_is_found_under_the_session_folder() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, resent, dispatch) = resent_wave(root);
        let platform = tempdir().unwrap();
        let config = platform.path();
        session(config, "-tmp-obra", "desktop", &instant(&resent, -60_000));
        agent(config, "-tmp-obra", "desktop/subagents/agent-x.jsonl", &instant(&resent, 200), &dispatch, "r1", [2, 100, 1_000, 40]);
        platform_file(config, "-tmp-obra", "desktop/subagents/agent-x.meta.json", &[json!({"agentType": "mustard-wave", "description": "Onda 1 x"})]);

        let (out, revised) = measured_return(root, config, "desktop");
        assert!(!usage_missing(&out), "o arquivo do agente do reenvio foi achado: {out}");
        // 2 + 100 + 1000 + 40: o agente do reenvio, com um uso de ferramenta.
        assert_eq!(revised.int("tokens"), Some(1_142), "{revised:?}");
        assert_eq!(revised.int("steps"), Some(1), "{revised:?}");
        assert_eq!(revised.str_field("model_used"), Some(MODEL), "{revised:?}");
    }

    /// O agente do primeiro envio trabalhou e sumiu, e o do reenvio — o que
    /// entregou — não tem arquivo em sessão nenhuma do projeto: o consumo das
    /// tentativas anteriores não passa pelo da onda. A resposta avisa que o
    /// consumo ficou de fora, e o envio não ganha tokens.
    #[test]
    fn the_wave_stays_unmeasured_without_the_agent_of_its_last_send() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (first, _, dispatch) = resent_wave(root);
        let platform = tempdir().unwrap();
        let config = platform.path();
        session(config, "-tmp-obra", "desktop", &instant(&first, -60_000));
        agent(config, "-tmp-obra", "desktop/subagents/agent-antes.jsonl", &instant(&first, 1), &dispatch, "a1", [9_000, 0, 0, 0]);

        let (out, revised) = measured_return(root, config, "desktop");
        assert!(usage_missing(&out), "sem o agente que entregou, a medição fica de fora: {out}");
        assert_eq!((revised.int("tokens"), revised.int("steps")), (None, None), "{revised:?}");
    }
}
