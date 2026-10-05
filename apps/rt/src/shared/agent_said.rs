//! `agent_said` — a última fala do agente antes de uma chamada, lida do fim
//! do arquivo da conversa que o Claude Code guarda.
//!
//! Cada linha do arquivo é uma mensagem em JSON. A leitura vai de trás para a
//! frente, em pedaços, e para na primeira resposta:
//!
//! - a mensagem do assistente com um bloco de texto dá a fala (o bloco mais
//!   novo da mensagem); os blocos de uso de ferramenta e de raciocínio ficam
//!   para trás;
//! - a mensagem de papel `user` só com resultado de ferramenta fica para
//!   trás;
//! - a mensagem de papel `user` com texto é de gente: a leitura para ali, sem
//!   fala. Texto de gente nunca é devolvido.
//!
//! Só o fim do arquivo é lido, até [`SCAN_LIMIT`] bytes. Nunca falha: o
//! arquivo que falta, que não abre ou que não traz fala dá o texto vazio.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// O tamanho de cada pedaço lido de trás para a frente.
const CHUNK: u64 = 64 * 1024;

/// O quanto do fim do arquivo se lê, no máximo: passado isso, sem fala.
const SCAN_LIMIT: u64 = 4 * 1024 * 1024;

/// O arquivo da conversa do subagente `name` (`agent-<id>.jsonl`): o Claude
/// Code o guarda em `<pasta da conversa principal>/*/subagents/`, e a pasta do
/// meio nem sempre é a que leva o nome da conversa principal. `None` quando
/// o nome não é um nome de arquivo simples ou quando nenhuma pasta o tem.
pub(crate) fn subagent_transcript(main: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || Path::new(name).file_name() != Some(std::ffi::OsStr::new(name)) {
        return None;
    }
    let folder = main.parent()?;
    std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("subagents").join(name))
        .find(|candidate| candidate.is_file())
}

/// A última fala do agente no arquivo `transcript`, como está nele; vazia
/// quando não há.
pub(crate) fn last_said(transcript: &Path) -> String {
    scan_back(transcript).unwrap_or_default()
}

/// A leitura de trás para a frente: cada linha completa, da última para a
/// primeira, até uma resposta. `None` sem fala, sem arquivo ou passado o
/// limite.
fn scan_back(transcript: &Path) -> Option<String> {
    let mut file = File::open(transcript).ok()?;
    let length = file.metadata().ok()?.len();
    let floor = length.saturating_sub(SCAN_LIMIT);
    let mut position = length;
    // O começo da linha que o pedaço seguinte ainda completa.
    let mut carry: Vec<u8> = Vec::new();
    while position > floor {
        let from = position.saturating_sub(CHUNK).max(floor);
        let mut chunk = vec![0u8; usize::try_from(position - from).ok()?];
        file.seek(SeekFrom::Start(from)).ok()?;
        file.read_exact(&mut chunk).ok()?;
        chunk.extend_from_slice(&carry);
        position = from;
        let mut lines = chunk.split(|byte| *byte == b'\n');
        // Antes do começo do arquivo, a primeira linha do pedaço pode estar
        // partida; ela espera o pedaço de trás.
        let first = lines.next()?;
        let whole: Vec<&[u8]> = lines.collect();
        for line in whole.into_iter().rev() {
            if let Some(ending) = read_line(line) {
                return ending.said();
            }
        }
        if position == 0 {
            return read_line(first).and_then(Ending::said);
        }
        carry = first.to_vec();
    }
    None
}

/// Como a leitura termina numa linha da conversa.
enum Ending {
    /// Mensagem de gente: o fim, sem fala.
    Person,
    /// Texto do assistente: a fala.
    Said(String),
}

impl Ending {
    /// A fala, quando o fim a traz.
    fn said(self) -> Option<String> {
        match self {
            Self::Person => None,
            Self::Said(text) => Some(text),
        }
    }
}

/// Se a linha da conversa termina a leitura, e como; `None` segue para a
/// linha de trás.
fn read_line(line: &[u8]) -> Option<Ending> {
    let Ok(entry) = serde_json::from_slice::<Value>(line) else { return None };
    let message = entry.get("message")?;
    let content = message.get("content");
    match message.get("role").and_then(Value::as_str)? {
        "assistant" => content
            .and_then(Value::as_array)?
            .iter()
            .rev()
            .find_map(text_of)
            .map(|text| Ending::Said(text.to_string())),
        "user" => match content {
            Some(Value::String(text)) => (!text.trim().is_empty()).then_some(Ending::Person),
            Some(Value::Array(blocks)) => blocks.iter().any(|block| text_of(block).is_some()).then_some(Ending::Person),
            _ => None,
        },
        _ => None,
    }
}

/// O texto de um bloco, quando é um bloco de texto com algo escrito.
fn text_of(block: &Value) -> Option<&str> {
    if block.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    block.get("text").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assistant(blocks: Value) -> String {
        json!({"type": "assistant", "message": {"role": "assistant", "content": blocks}}).to_string()
    }

    fn user_text(text: &str) -> String {
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}}).to_string()
    }

    fn user_plain(text: &str) -> String {
        json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
    }

    fn tool_result(text: &str) -> String {
        json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": text}
        ]}})
        .to_string()
    }

    fn say(text: &str) -> String {
        assistant(json!([{"type": "text", "text": text}]))
    }

    fn tool_use() -> String {
        assistant(json!([{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "grep x"}}]))
    }

    fn thinking() -> String {
        assistant(json!([{"type": "thinking", "thinking": "hmm"}]))
    }

    fn said_in(lines: &[String]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.jsonl");
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        last_said(&file)
    }

    #[test]
    fn the_last_text_of_the_assistant_is_the_said_and_tool_calls_and_thinking_are_skipped() {
        let lines = [
            user_plain("a pergunta do usuário"),
            say("Vou procurar o cálculo do frete."),
            tool_use(),
            tool_result("resultado"),
            thinking(),
            tool_use(),
        ];
        assert_eq!(said_in(&lines), "Vou procurar o cálculo do frete.");
    }

    #[test]
    fn the_newest_text_wins_over_an_older_one() {
        let lines = [say("primeira"), tool_use(), tool_result("x"), say("segunda"), tool_use()];
        assert_eq!(said_in(&lines), "segunda");
    }

    #[test]
    fn the_newest_text_block_of_one_message_is_the_said() {
        let both = assistant(json!([
            {"type": "text", "text": "antes"},
            {"type": "tool_use", "id": "t", "name": "Bash", "input": {}},
            {"type": "text", "text": "depois"},
        ]));
        assert_eq!(said_in(&[both]), "depois");
    }

    #[test]
    fn a_message_of_the_user_after_the_last_text_of_the_assistant_gives_no_said() {
        let after_list = [say("fala antiga"), user_text("agora faça outra coisa"), tool_use()];
        assert_eq!(said_in(&after_list), "", "text of a person stops the reading");
        let after_plain = [say("fala antiga"), user_plain("agora faça outra coisa"), tool_use()];
        assert_eq!(said_in(&after_plain), "");
    }

    #[test]
    fn the_text_of_the_user_is_never_the_said() {
        let only_user = [user_plain("segredo do usuário"), tool_use()];
        assert_eq!(said_in(&only_user), "");
        let secret = "chave do usuário";
        let lines = [say("do agente"), user_text(secret), say(""), tool_use()];
        assert!(!said_in(&lines).contains(secret));
    }

    #[test]
    fn a_result_of_a_tool_does_not_stop_the_reading() {
        let lines = [say("a fala"), tool_use(), tool_result("saída longa"), tool_use(), tool_result("outra")];
        assert_eq!(said_in(&lines), "a fala");
    }

    #[test]
    fn lines_that_are_not_messages_or_not_json_are_skipped() {
        let lines = [
            say("a fala"),
            r#"{"type":"summary","summary":"x"}"#.to_string(),
            "isto não é json".to_string(),
            r#"{"type":"system","content":"aviso"}"#.to_string(),
            tool_use(),
        ];
        assert_eq!(said_in(&lines), "a fala");
    }

    #[test]
    fn a_line_ends_the_reading_with_the_said_of_the_assistant_or_with_a_person_and_otherwise_goes_on() {
        let ends = |line: String| read_line(line.as_bytes());
        assert!(matches!(ends(say("a fala")), Some(Ending::Said(text)) if text == "a fala"));
        assert!(matches!(ends(user_text("outra coisa")), Some(Ending::Person)));
        assert!(matches!(ends(user_plain("outra coisa")), Some(Ending::Person)));
        assert!(ends(user_plain("   ")).is_none(), "a blank message of the user does not stop the reading");
        assert!(ends(tool_result("saída")).is_none());
        assert!(ends(tool_use()).is_none());
        assert!(ends(thinking()).is_none());
        assert!(ends("isto não é json".to_string()).is_none());
        assert_eq!(Ending::Said("fala".to_string()).said().as_deref(), Some("fala"));
        assert_eq!(Ending::Person.said(), None);
    }

    #[test]
    fn a_missing_or_empty_file_gives_no_said() {
        assert_eq!(last_said(Path::new("/nonexistent/session.jsonl")), "");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("empty.jsonl");
        std::fs::write(&file, "").unwrap();
        assert_eq!(last_said(&file), "");
    }

    #[test]
    fn a_file_without_the_final_newline_still_reads_its_last_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.jsonl");
        std::fs::write(&file, [tool_use(), say("sem quebra no fim")].join("\n")).unwrap();
        assert_eq!(last_said(&file), "sem quebra no fim");
    }

    #[test]
    fn only_the_end_of_a_big_file_is_read_and_a_line_cut_by_a_chunk_is_whole() {
        // O arquivo passa de vários pedaços; o texto está bem antes do fim,
        // sob muitas linhas grandes de resultado de ferramenta.
        let big = "x".repeat(40_000);
        let mut lines = vec![user_plain("pergunta"), say("fala no meio"), tool_use()];
        for _ in 0..6 {
            lines.push(tool_result(&big));
            lines.push(tool_use());
        }
        assert_eq!(said_in(&lines), "fala no meio");
    }

    #[test]
    fn a_said_deeper_than_the_scan_limit_gives_no_said() {
        let big = "x".repeat(1_000_000);
        let mut lines = vec![say("fala longe demais")];
        for _ in 0..5 {
            lines.push(tool_result(&big));
        }
        assert_eq!(said_in(&lines), "", "five megabytes of results are past the limit");
    }

    /// As linhas `before`, a linha `target` e, depois dela, duas linhas que
    /// somam mil bytes a menos que um pedaço de leitura: a borda entre o
    /// último pedaço e o de trás cai dentro de `target`.
    fn lines_with_the_border_inside(before: Vec<String>, target: String) -> Vec<String> {
        let base = tool_result("").len() + 1 + tool_use().len() + 1;
        let filler = "x".repeat(CHUNK as usize - 1000 - base);
        let tail = vec![tool_result(&filler), tool_use()];
        let tail_bytes: usize = tail.iter().map(|line| line.len() + 1).sum();
        assert!(tail_bytes < CHUNK as usize, "the last chunk starts inside the target line");
        assert!(tail_bytes + target.len() + 1 > CHUNK as usize, "the target line starts before the border");
        let mut lines = before;
        lines.push(target);
        lines.extend(tail);
        lines
    }

    #[test]
    fn a_speech_line_that_crosses_the_border_between_two_chunks_is_read_whole() {
        let text = format!("{} fala de agente cruzando a borda", "z".repeat(1_000));
        let lines = lines_with_the_border_inside(vec![say("fala antiga")], say(&text));
        assert_eq!(said_in(&lines), text);
    }

    #[test]
    fn a_line_of_the_user_that_crosses_the_border_between_two_chunks_still_stops_the_reading() {
        let text = format!("{} agora faça outra coisa", "y".repeat(1_000));
        let lines = lines_with_the_border_inside(vec![say("fala antiga")], user_plain(&text));
        assert_eq!(said_in(&lines), "", "the broken half of the line must not let the old speech through");
    }

    #[test]
    fn the_subagent_transcript_is_found_in_any_folder_next_to_the_main_one() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("860fee3d.jsonl");
        std::fs::write(&main, "").unwrap();
        let other = dir.path().join("6435bf6e").join("subagents");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("agent-a3d2.jsonl"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("0000").join("subagents")).unwrap();
        assert_eq!(subagent_transcript(&main, "agent-a3d2.jsonl"), Some(other.join("agent-a3d2.jsonl")));
        assert_eq!(subagent_transcript(&main, "agent-zzzz.jsonl"), None);
        for unsafe_name in ["", "../860fee3d.jsonl", "a/b.jsonl", ".."] {
            assert_eq!(subagent_transcript(&main, unsafe_name), None, "{unsafe_name:?}");
        }
    }

    #[test]
    fn a_long_unicode_text_is_returned_whole() {
        let text = "ação ".repeat(30_000);
        let lines = [say(&text), tool_use()];
        assert_eq!(said_in(&lines), text.trim());
    }
}
