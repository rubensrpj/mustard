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
use serde_json::{json, Map, Value};

use super::report::dispatched_at;

/// Quem chama a rodada ou o fechamento: a sessão da conversa principal e a
/// pasta de configuração da plataforma, onde ela grava o arquivo de conversa
/// dessa sessão e os dos agentes que a sessão abriu. As duas são resolvidas
/// na entrada do comando, e o teste passa as dele sem mexer no ambiente. Sem
/// a pasta ou sem a sessão, o consumo não é medido.
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
}

impl Usage {
    /// Os campos medidos, com os nomes que o envio grava; vazio sem nenhum.
    pub(super) fn fields(&self) -> Map<String, Value> {
        let mut out = Map::new();
        if let Some(model) = &self.model_used {
            out.insert("model_used".into(), json!(model));
        }
        for (key, value) in [
            ("steps", self.steps),
            ("tokens", self.tokens),
            ("caller_steps", self.caller_steps),
            ("caller_tokens", self.caller_tokens),
        ] {
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
pub(super) fn measure_usage<'u>(
    log: &SpecLog,
    caller: Caller<'_>,
    waves: impl IntoIterator<Item = (u64, &'u mut Usage)>,
) {
    let Some(session) = caller.config_dir.zip(caller.session).and_then(|(dir, id)| transcript::session_dir(dir, id))
    else {
        return;
    };
    let main = main_usage(log, &session);
    for (wave, usage) in waves {
        let own = wave_usage(log, &session, wave);
        *usage = Usage {
            model_used: own.as_ref().and_then(|own| own.model.clone()),
            steps: own.as_ref().map(|own| own.steps),
            tokens: own.as_ref().map(|own| own.tokens),
            caller_steps: main.as_ref().map(|main| main.steps),
            caller_tokens: main.as_ref().map(|main| main.tokens),
        };
    }
}

/// O consumo do agente que recebeu o pedido da onda `wave`, no arquivo dele:
/// o agente cujo pedido abre com a primeira linha do texto do envio que
/// despachou a onda e que começou depois dele, procurado na pasta da sessão
/// (`session`) e, quando ela não o tem — a onda saiu antes de um `/clear` —,
/// nas das outras sessões do mesmo projeto. `None` quando o arquivo não é
/// achado ou não se lê.
fn wave_usage(log: &SpecLog, session: &Path, wave: u64) -> Option<transcript::Usage> {
    let sent = log.get(dispatched_at(log, wave)?)?;
    let title = sent.str_field("text")?.lines().next()?;
    let file = transcript::wave_agent_file(session, title, sent.at())?;
    let bytes = std::fs::read(file).ok()?;
    Some(transcript::usage_of(String::from_utf8_lossy(&bytes).lines()))
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::Caller;
    use crate::commands::flow::round::tests::{approved, line, returned, round};
    use crate::commands::flow::round::{round_in, RoundOpts};

    /// O modelo que a plataforma grava nas respostas de um agente.
    const MODEL: &str = "claude-opus-5-5";

    /// O instante `at` da spec deslocado de `millis`, como a plataforma grava
    /// o carimbo: em UTC, com os milésimos.
    fn instant(at: &str, millis: i64) -> String {
        let at = chrono::DateTime::parse_from_rfc3339(at).unwrap().with_timezone(&chrono::Utc);
        (at + chrono::Duration::milliseconds(millis)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    /// Um arquivo de conversa em `<config>/projects/<project>/<relative>`, no
    /// lugar em que a plataforma o grava.
    fn platform_file(config: &Path, project: &str, relative: &str, lines: &[Value]) {
        let path = config.join("projects").join(project).join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        std::fs::write(path, text.join("\n") + "\n").unwrap();
    }

    /// A conversa principal de uma sessão, só com a mensagem de quem a abriu.
    fn session(config: &Path, project: &str, name: &str, at: &str) {
        let opened = json!({"type": "user", "isSidechain": false, "gitBranch": "feature/x", "timestamp": at,
            "message": {"role": "user", "content": "siga a obra"}});
        platform_file(config, project, &format!("{name}.jsonl"), &[opened]);
    }

    /// O arquivo de um agente: o pedido `request` que ele recebeu em `start` e
    /// uma resposta `id`, com um uso de ferramenta e os quatro números —
    /// entrada, criação de cache, leitura de cache e saída.
    fn agent(config: &Path, project: &str, relative: &str, start: &str, request: &str, id: &str, usage: [u64; 4]) {
        let asked = json!({"type": "user", "isSidechain": true, "gitBranch": "feature/x", "timestamp": start,
            "message": {"role": "user", "content": request}});
        let answered = json!({"type": "assistant", "isSidechain": true, "gitBranch": "feature/x", "timestamp": start,
            "message": {"id": id, "model": MODEL, "role": "assistant",
                "content": [{"type": "tool_use", "id": format!("{id}-uso"), "name": "Bash", "input": {}}],
                "usage": {"input_tokens": usage[0], "cache_creation_input_tokens": usage[1],
                    "cache_read_input_tokens": usage[2], "output_tokens": usage[3]}}});
        platform_file(config, project, relative, &[asked, answered]);
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
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default().to_string();
        assert!(prompt.starts_with("# "), "o pedido abre com o título da onda: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let send = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap().clone();
        let sent = send.at();

        let platform = tempdir().unwrap();
        let config = platform.path();
        session(config, "-tmp-obra", "antiga", &instant(sent, -1_000));
        agent(config, "-tmp-obra", "antiga/subagents/agent-onda.jsonl", &instant(sent, 200), &prompt, "r1", [2, 100, 1_000, 40]);
        agent(config, "-tmp-obra", "antiga/subagents/agent-antes.jsonl", &instant(sent, -1), &prompt, "a1", [9_000, 0, 0, 0]);
        agent(config, "-tmp-obra", "antiga/subagents/agent-depois.jsonl", &instant(sent, 201), &prompt, "d1", [7_000, 0, 0, 0]);
        session(config, "-tmp-obra", "nova", &instant(sent, 50));
        let other_wave = "# x — onda 2\n\nOutro pedido.\n";
        agent(config, "-tmp-obra", "nova/subagents/agent-outra.jsonl", &instant(sent, 100), other_wave, "b1", [8_000, 0, 0, 0]);
        session(config, "-tmp-outro", "alheia", &instant(sent, -1_000));
        agent(config, "-tmp-outro", "alheia/subagents/agent-onda.jsonl", &instant(sent, 100), &prompt, "o1", [6_000, 0, 0, 0]);

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let delivery = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, delivery)["ok"], json!(true));
        let opts = RoundOpts {
            root: root.to_path_buf(),
            spec: Some("x".to_string()),
            report: Some(line("USAGE", json!({"wave": 1}))),
        };
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
}
