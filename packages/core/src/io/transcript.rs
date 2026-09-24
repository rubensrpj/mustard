//! A leitura dos arquivos de conversa que a plataforma grava em
//! `<pasta de configuração>/projects/<pasta do projeto>/`: cada conversa em
//! `<sessão>.jsonl`, e cada agente dela em `<sessão>/subagents/agent-<id>.jsonl`,
//! com um `.meta.json` ao lado.
//!
//! Cada linha é um objeto JSON. Uma resposta do modelo traz `message.usage`, e a
//! plataforma grava a mesma resposta em várias linhas — uma por bloco do
//! conteúdo —, repetindo `message.id` em todas; o uso que vale é o da última.
//!
//! A conta é a da régua de custo por arquivo tocado: tokens é tudo o que o
//! modelo leu e escreveu em cada resposta — entrada, criação de cache, leitura
//! de cache e saída —, contando cada resposta uma vez; passos é o número de usos
//! de ferramenta.
//!
//! A parte pura, [`usage_of`], não toca o disco; as outras acham os arquivos e
//! entregam as linhas a ela. Linha que não é JSON, ou sem `usage`, é pulada: o
//! arquivo é da plataforma, e uma linha que este leitor não entende não pode
//! derrubar a medida inteira.

use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// O consumo medido num conjunto de linhas de conversa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    /// O modelo da última resposta contada; `None` quando não houve resposta.
    pub model: Option<String>,
    /// Quantos usos de ferramenta as respostas pediram.
    pub steps: u64,
    /// A soma, por resposta, de entrada, criação de cache, leitura de cache e
    /// saída, cada resposta contada uma vez, pela última linha dela.
    pub tokens: u64,
}

/// O texto que toda linha com uso carrega. Uma linha sem ele não tem o que
/// somar, e é pulada sem ser lida como JSON: as linhas grandes da conversa são
/// as das respostas das ferramentas, que não têm uso.
const USAGE_KEY: &str = "\"usage\"";

/// Uma linha do arquivo, só com o que este leitor usa.
#[derive(Deserialize)]
struct Line {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: Option<bool>,
    #[serde(default, rename = "gitBranch")]
    git_branch: Option<String>,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<Tokens>,
    #[serde(default)]
    content: Option<Content>,
}

/// Os quatro números que somam o que o modelo leu e escreveu numa resposta.
#[derive(Deserialize)]
struct Tokens {
    #[serde(default, rename = "input_tokens")]
    input: Option<u64>,
    #[serde(default, rename = "cache_creation_input_tokens")]
    cache_creation: Option<u64>,
    #[serde(default, rename = "cache_read_input_tokens")]
    cache_read: Option<u64>,
    #[serde(default, rename = "output_tokens")]
    output: Option<u64>,
}

impl Tokens {
    fn total(&self) -> u64 {
        [self.input, self.cache_creation, self.cache_read, self.output]
        .into_iter()
        .flatten()
        .fold(0, u64::saturating_add)
    }
}

/// O conteúdo de uma mensagem: texto corrido, ou a lista de blocos.
#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

#[derive(Deserialize)]
struct Block {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

impl Content {
    /// O texto da mensagem: o corrido, ou o do primeiro bloco de texto.
    fn text(&self) -> Option<&str> {
        match self {
            Content::Text(text) => Some(text),
            Content::Blocks(blocks) => blocks
                .iter()
                .find(|block| block.kind.as_deref() == Some("text"))
                .and_then(|block| block.text.as_deref()),
        }
    }
}

/// A soma em andamento. Cada resposta é guardada pelo `message.id`, e a linha
/// seguinte da mesma resposta troca o valor em vez de somar; cada uso de
/// ferramenta, pelo próprio id, porque a plataforma grava cada bloco numa linha.
#[derive(Default)]
struct Tally {
    responses: HashMap<String, u64>,
    unnamed_tokens: u64,
    tools: HashSet<String>,
    unnamed_tools: u64,
    model: Option<String>,
}

impl Tally {
    fn add(&mut self, line: Line) {
        let Some(message) = line.message else { return };
        let Some(tokens) = message.usage else { return };
        let total = tokens.total();
        match message.id {
            Some(id) => {
                self.responses.insert(id, total);
            }
            None => self.unnamed_tokens = self.unnamed_tokens.saturating_add(total),
        }
        // A plataforma grava como `<synthetic>` a resposta que ela mesma
        // escreveu, sem modelo nenhum por trás; o nome entre sinais não é modelo.
        if let Some(model) = message.model.filter(|model| !model.starts_with('<')) {
            self.model = Some(model);
        }
        if let Some(Content::Blocks(blocks)) = message.content {
            for block in blocks.into_iter().filter(|block| block.kind.as_deref() == Some("tool_use")) {
                match block.id {
                    Some(id) => {
                        self.tools.insert(id);
                    }
                    None => self.unnamed_tools = self.unnamed_tools.saturating_add(1),
                }
            }
        }
    }

    fn usage(self) -> Usage {
        let named = self.responses.values().fold(0u64, |sum, tokens| sum.saturating_add(*tokens));
        Usage {
            model: self.model,
            steps: u64::try_from(self.tools.len())
                .unwrap_or(u64::MAX)
                .saturating_add(self.unnamed_tools),
            tokens: named.saturating_add(self.unnamed_tokens),
        }
    }
}

/// A linha lida, quando ela carrega uso e é JSON; `None` para todo o resto.
fn usage_line(text: &str) -> Option<Line> {
    if !text.contains(USAGE_KEY) {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// O instante de um carimbo, em UTC. O arquivo da plataforma grava em UTC e a
/// spec grava com o deslocamento local: os dois só se comparam no mesmo fuso.
fn utc(at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at).ok().map(|at| at.with_timezone(&Utc))
}

/// O consumo das `lines`: o modelo, os passos e os tokens.
///
/// Cada resposta conta uma vez, pela última linha com o `message.id` dela;
/// cada uso de ferramenta conta uma vez, em qualquer linha da resposta.
#[must_use]
pub fn usage_of<'a, I>(lines: I) -> Usage
where
    I: IntoIterator<Item = &'a str>,
{
    let mut tally = Tally::default();
    for line in lines.into_iter().filter_map(usage_line) {
        tally.add(line);
    }
    tally.usage()
}

/// A pasta da sessão `session`, procurada em `<config_dir>/projects/*/`: a pasta
/// que tem `<session>.jsonl` devolve `<ela>/<session>`, onde moram os agentes. A
/// pasta acima da devolvida é a do projeto, a que [`orchestrator_usage`] lê.
/// `None` quando nenhuma pasta tem a conversa.
#[must_use]
pub fn session_dir(config_dir: &Path, session: &str) -> Option<PathBuf> {
    // A sessão é um nome de arquivo; separador ou `..` sairiam da pasta.
    if session.is_empty() || Path::new(session).file_name() != Some(std::ffi::OsStr::new(session)) {
        return None;
    }
    let file = format!("{session}.jsonl");
    let mut projects: Vec<PathBuf> = std::fs::read_dir(config_dir.join("projects"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|project| project.join(&file).is_file())
        .collect();
    projects.sort();
    projects.into_iter().next().map(|project| project.join(session))
}

/// O arquivo do agente que recebeu o pedido da onda: em
/// `<session_dir>/subagents/`, o `agent-*.jsonl` cuja primeira mensagem abre
/// com `title` — a primeira linha do pedido, `# ` e o título da onda — e cujo
/// primeiro carimbo não vem antes de `sent`, o `at` do envio.
///
/// Quando mais de um agente serve, vale o que começou mais perto do envio.
/// `None` quando nenhum serve, quando a pasta não existe ou quando `sent` não
/// é um instante.
#[must_use]
pub fn wave_agent_file(session_dir: &Path, title: &str, sent: &str) -> Option<PathBuf> {
    let sent = utc(sent)?;
    let title = title.trim_end();
    std::fs::read_dir(session_dir.join("subagents"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_agent_file(path))
        .filter_map(|path| {
            let (heading, started) = opening(&path)?;
            (heading == title && started >= sent).then_some((started, path))
        })
        .min_by(|(a, pa), (b, pb)| a.cmp(b).then_with(|| pa.cmp(pb)))
        .map(|(_, path)| path)
}

/// `agent-<id>.jsonl`: o `.meta.json` ao lado não é conversa.
fn is_agent_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("agent-"))
        && path.is_file()
}

/// A primeira linha da primeira mensagem do agente e o primeiro carimbo do
/// arquivo. Lê só o começo: as duas coisas vêm nas primeiras linhas.
fn opening(path: &Path) -> Option<(String, DateTime<Utc>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut heading: Option<String> = None;
    let mut started: Option<DateTime<Utc>> = None;
    for text in std::io::BufReader::new(file).lines() {
        let Ok(text) = text else { break };
        let Ok(line) = serde_json::from_str::<Line>(&text) else { continue };
        if started.is_none() {
            started = line.timestamp.as_deref().and_then(utc);
        }
        if heading.is_none()
            && let Some(message) = &line.message
        {
            let first = message.content.as_ref().and_then(Content::text).unwrap_or_default();
            heading = Some(first.lines().next().unwrap_or_default().trim_end().to_string());
        }
        if let (Some(heading), Some(started)) = (&heading, started) {
            return Some((heading.clone(), started));
        }
    }
    None
}

/// O consumo da conversa principal no ramo `branch` desde `since`: soma as
/// linhas fora de agente (`isSidechain` falso), com `gitBranch` igual ao ramo e
/// carimbo desde `since`, de todas as conversas em `project_dir` — cada `/clear`
/// abre uma conversa nova, e a obra atravessa várias.
///
/// A mesma resposta gravada em duas conversas conta uma vez. `None` quando a
/// pasta não se lê ou `since` não é um instante.
#[must_use]
pub fn orchestrator_usage(project_dir: &Path, branch: &str, since: &str) -> Option<Usage> {
    let since = utc(since)?;
    let floor = SystemTime::from(since);
    let mut files: Vec<PathBuf> = std::fs::read_dir(project_dir)
        .ok()?
        .filter_map(Result::ok)
        // Um arquivo que não mudou desde `since` não tem linha depois dele.
        .filter(|entry| entry.metadata().and_then(|meta| meta.modified()).ok().is_none_or(|at| at >= floor))
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl") && path.is_file())
        .collect();
    files.sort();
    let mut tally = Tally::default();
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        for line in String::from_utf8_lossy(&bytes).lines().filter_map(usage_line) {
            let main = line.is_sidechain == Some(false);
            let on_branch = line.git_branch.as_deref() == Some(branch);
            let in_time = line.timestamp.as_deref().and_then(utc).is_some_and(|at| at >= since);
            if main && on_branch && in_time {
                tally.add(line);
            }
        }
    }
    Some(tally.usage())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MODEL: &str = "claude-opus-5-5";

    /// Uma linha de resposta do modelo, como a plataforma grava: `blocks` é o
    /// conteúdo daquela linha, e `usage` os quatro números, na ordem entrada,
    /// criação de cache, leitura de cache e saída.
    fn response(id: &str, at: &str, branch: &str, sidechain: bool, usage: [u64; 4], blocks: &serde_json::Value) -> String {
        json!({
            "type": "assistant",
            "isSidechain": sidechain,
            "gitBranch": branch,
            "timestamp": at,
            "message": {
                "id": id,
                "model": MODEL,
                "role": "assistant",
                "content": blocks,
                "usage": {
                    "input_tokens": usage[0],
                    "cache_creation_input_tokens": usage[1],
                    "cache_read_input_tokens": usage[2],
                    "output_tokens": usage[3],
                    "cache_creation": { "ephemeral_5m_input_tokens": usage[1] },
                    "service_tier": "standard"
                }
            }
        })
        .to_string()
    }

    fn tool(id: &str) -> serde_json::Value {
        json!([{ "type": "tool_use", "id": id, "name": "Bash", "input": { "command": "ls" } }])
    }

    fn thinking() -> serde_json::Value {
        json!([{ "type": "thinking", "thinking": "..." }])
    }

    /// A primeira linha de um agente: o pedido, com o título na primeira linha.
    fn request(at: &str, text: &str) -> String {
        json!({
            "parentUuid": null,
            "isSidechain": true,
            "type": "user",
            "timestamp": at,
            "gitBranch": "feat/obra",
            "message": { "role": "user", "content": text }
        })
        .to_string()
    }

    fn write(path: &Path, lines: &[String]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    #[test]
    fn the_transcript_reader_counts_each_response_once_and_finds_the_wave_agent_by_its_request_title() {
        let config = tempfile::tempdir().unwrap();
        let project = config.path().join("projects").join("-home-alguem-obra");
        // Outro projeto, sem a sessão: a busca passa por ele e não para.
        std::fs::create_dir_all(config.path().join("projects").join("-home-alguem-outra")).unwrap();

        // O envio da onda 3, no relógio local de quem grava a spec: 21:54:28 em UTC.
        let sent = "2026-01-10T18:54:28-03:00";
        let since = "2026-01-10T18:00:00-03:00";
        let branch = "feat/obra";

        // A conversa principal. Entram só as linhas fora de agente, no ramo da
        // obra, desde o começo dela; a resposta `m1` aparece em duas linhas.
        write(
            &project.join("sessao.jsonl"),
            &[
                json!({
                    "type": "user", "isSidechain": false, "gitBranch": branch,
                    "timestamp": "2026-01-10T21:00:00.000Z",
                    "message": { "role": "user", "content": "comece a obra" }
                })
                .to_string(),
                "isto não é json".to_string(),
                response("m1", "2026-01-10T21:00:01.000Z", branch, false, [10, 20, 30, 40], &tool("u1")),
                response("m1", "2026-01-10T21:00:01.100Z", branch, false, [10, 20, 30, 50], &tool("u2")),
                response("m2", "2026-01-10T21:00:02.000Z", "main", false, [1000, 0, 0, 0], &tool("u3")),
                response("m3", "2026-01-10T21:00:03.000Z", branch, true, [2000, 0, 0, 0], &tool("u4")),
                response("m4", "2026-01-10T20:59:59.999Z", branch, false, [4000, 0, 0, 0], &tool("u5")),
            ],
        );
        // Depois de um `/clear`, a conversa segue noutro arquivo.
        write(
            &project.join("depois-do-clear.jsonl"),
            &[response("m5", "2026-01-10T22:00:00.000Z", branch, false, [1, 2, 3, 4], &thinking())],
        );

        let agents = project.join("sessao").join("subagents");
        let body = "\n\nModelo desta onda: Opus.\n";
        // Mesmo título, mas começou antes do envio: é o de um envio anterior.
        write(
            &agents.join("agent-antes.jsonl"),
            &[
                request("2026-01-10T21:54:27.999Z", &format!("# obra — onda 3{body}")),
                response("a0", "2026-01-10T21:54:29.000Z", branch, true, [9000, 0, 0, 0], &thinking()),
            ],
        );
        // Começou no instante do envio, antes do da onda 3, mas é a onda 30: o
        // título da 3 é só o começo do dela.
        write(
            &agents.join("agent-trinta.jsonl"),
            &[
                request("2026-01-10T21:54:28.000Z", &format!("# obra — onda 30{body}")),
                response("a30", "2026-01-10T21:54:31.000Z", branch, true, [8000, 0, 0, 0], &thinking()),
            ],
        );
        // O agente da onda 3: a resposta `r1` vem em três linhas — o raciocínio
        // e dois usos de ferramenta —, e só a última traz a saída inteira.
        write(
            &agents.join("agent-onda3.jsonl"),
            &[
                request("2026-01-10T21:54:28.200Z", &format!("# obra — onda 3{body}")),
                response("r1", "2026-01-10T21:54:29.000Z", branch, true, [2, 100, 1000, 5], &thinking()),
                response("r1", "2026-01-10T21:54:29.100Z", branch, true, [2, 100, 1000, 5], &tool("t1")),
                response("r1", "2026-01-10T21:54:29.200Z", branch, true, [2, 100, 1000, 40], &tool("t2")),
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\"}]}}".to_string(),
                response("r2", "2026-01-10T21:54:31.000Z", branch, true, [3, 0, 1142, 7], &json!([{"type": "text", "text": "pronto"}])),
            ],
        );
        std::fs::write(agents.join("agent-onda3.meta.json"), "{\"agentType\":\"wave\"}").unwrap();

        let session = session_dir(config.path(), "sessao").expect("a sessão é achada pelo arquivo dela");
        assert_eq!(session, project.join("sessao"));
        assert_eq!(session_dir(config.path(), "nenhuma"), None);

        let found = wave_agent_file(&session, "# obra — onda 3", sent);
        assert_eq!(found, Some(agents.join("agent-onda3.jsonl")), "o agente da onda 3, e não o anterior nem o da 30");
        // O agente que começa no instante do envio serve; com o envio um
        // milésimo depois do começo do agente da onda 3, ele já não serve.
        assert_eq!(wave_agent_file(&session, "# obra — onda 30", sent), Some(agents.join("agent-trinta.jsonl")));
        assert_eq!(wave_agent_file(&session, "# obra — onda 3", "2026-01-10T18:54:28.201-03:00"), None);

        let text = std::fs::read_to_string(found.unwrap()).unwrap();
        let wave = usage_of(text.lines());
        // r1 conta uma vez, pela última linha: 2 + 100 + 1000 + 40 = 1142;
        // r2: 3 + 0 + 1142 + 7 = 1152.
        assert_eq!(wave.tokens, 1142 + 1152, "{wave:?}");
        assert_eq!(wave.steps, 2, "dois usos de ferramenta, em linhas diferentes da mesma resposta");
        assert_eq!(wave.model.as_deref(), Some(MODEL));

        // O orquestrador: m1 pela última linha (10 + 20 + 30 + 50 = 110) e m5
        // (1 + 2 + 3 + 4 = 10); os passos são u1 e u2.
        let main = orchestrator_usage(&project, branch, since).expect("a pasta do projeto se lê");
        assert_eq!(main.tokens, 110 + 10, "{main:?}");
        assert_eq!(main.steps, 2, "{main:?}");
        assert_eq!(main.model.as_deref(), Some(MODEL));
    }

    /// Nenhuma linha que se entenda não é consumo zero de um modelo qualquer:
    /// é consumo nenhum, sem modelo.
    #[test]
    fn lines_without_usage_or_json_add_nothing() {
        let usage = usage_of(["", "{", "{\"message\":{\"id\":\"x\"}}", "[\"usage\"]"]);
        assert_eq!(usage, Usage::default());
    }
}
