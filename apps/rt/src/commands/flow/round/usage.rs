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

/// O consumo do agente que recebeu o pedido da onda `wave`, no arquivo dele
/// dentro da pasta da sessão (`session`): o agente cujo pedido abre com a
/// primeira linha do texto do envio que despachou a onda e que começou
/// depois dele. `None` quando o arquivo não é achado ou não se lê.
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
