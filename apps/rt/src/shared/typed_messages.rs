//! `typed_messages` — o que o usuário digitou numa conversa, lido do arquivo
//! que o Claude Code guarda para a sessão.
//!
//! Cada linha do arquivo é uma mensagem em JSON. A leitura vai do começo ao
//! fim e fica só com a fala de gente: o resultado de ferramenta, a linha de
//! sistema, o aviso do harness e o bloco que o próprio Claude Code põe na fala
//! do usuário ficam de fora. A abertura de uma spec no meio da conversa grava
//! essas mensagens nela, para o pedido dito antes ficar nas palavras dele.
//!
//! Nunca falha: a sessão sem arquivo, ou o arquivo que não abre, dá nada.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::shared::prompt::is_harness_notice;

/// O arquivo da conversa principal da sessão `session` no projeto de `root`,
/// guardado pelo Claude Code em
/// `<pasta de configuração>/projects/<pasta do projeto>/<sessão>.jsonl`, pela
/// mesma procura que a medição do consumo usa
/// ([`mustard_core::io::transcript::session_dir`]). `None` quando a sessão não
/// é um nome de arquivo simples, quando a conversa dela é de outro projeto ou
/// quando nenhuma pasta a tem, como numa sessão do Codex.
pub(crate) fn session_transcript(root: &Path, session: &str) -> Option<PathBuf> {
    let session = session.trim();
    let config = mustard_core::platform::harness::claude_config_dir()?;
    let folder = mustard_core::io::transcript::session_dir(&config, root, session)?;
    Some(folder.parent()?.join(format!("{session}.jsonl")))
}

/// Os blocos que o próprio Claude Code põe na fala do usuário: a saída de um
/// comando local, o comando do modo bash e a saída dele, e o lembrete do
/// sistema. Só o começo do bloco conta.
const HARNESS_BLOCKS: &[&str] =
    &["<local-command-", "<bash-input>", "<bash-stdout>", "<bash-stderr>", "<system-reminder>"];

/// O que o usuário digitou na conversa `transcript`, em ordem, como ele
/// escreveu. Fica de fora o que não é dele: o resultado de ferramenta, a
/// linha de sistema (`isMeta`, o resumo da compactação, a fala de um
/// subagente), o aviso do harness e o bloco do Claude Code. O comando de barra
/// entra como foi digitado, `/nome argumentos`, e só quando traz argumentos.
/// Vazio quando o arquivo falta ou não abre.
pub(crate) fn typed_messages(transcript: &Path) -> Vec<String> {
    let Ok(file) = File::open(transcript) else { return Vec::new() };
    BufReader::new(file)
        .split(b'\n')
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_slice::<Value>(&line).ok())
        .filter_map(|entry| typed_text(&entry))
        .collect()
}

/// O texto de um bloco, quando é um bloco de texto com algo escrito.
pub(crate) fn text_of(block: &Value) -> Option<&str> {
    if block.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    block.get("text").and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())
}

/// O texto digitado de uma linha da conversa, quando ela é uma mensagem do
/// usuário escrita por gente.
fn typed_text(entry: &Value) -> Option<String> {
    let flag = |key: &str| entry.get(key).and_then(Value::as_bool) == Some(true);
    if ["isMeta", "isSidechain", "isCompactSummary", "isVisibleInTranscriptOnly"].into_iter().any(flag) {
        return None;
    }
    // O Claude Code marca quem pôs a linha: só a de gente segue. A linha
    // antiga, sem a marca, passa pelos outros filtros.
    if entry.pointer("/origin/kind").and_then(Value::as_str).is_some_and(|kind| kind != "human") {
        return None;
    }
    let message = entry.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let blocks: Vec<&str> = match message.get("content")? {
        Value::String(text) => vec![text.as_str()],
        Value::Array(blocks) => {
            if blocks.iter().any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result")) {
                return None;
            }
            blocks.iter().filter_map(text_of).collect()
        }
        _ => return None,
    };
    let text = blocks
        .into_iter()
        .map(str::trim)
        .filter(|block| !block.is_empty() && !HARNESS_BLOCKS.iter().any(|tag| block.starts_with(tag)))
        .map(typed_back)
        .collect::<Option<Vec<String>>>()?
        .join("\n\n");
    (!text.is_empty() && !is_harness_notice(&text)).then_some(text)
}

/// O texto que um bloco leva à mensagem: o próprio bloco quando ele não é um
/// comando de barra; o comando que o Claude Code guarda em marcas, de volta
/// como foi digitado (`/nome argumentos`); e `None` quando o comando não traz
/// argumentos.
fn typed_back(block: &str) -> Option<String> {
    let command = block.starts_with("<command-message>") || block.starts_with("<command-name>");
    let inside = |tag: &str| {
        let open = format!("<{tag}>");
        let start = block.find(&open)? + open.len();
        let end = block[start..].find(&format!("</{tag}>"))? + start;
        Some(block[start..end].trim())
    };
    let Some(name) = inside("command-name").filter(|_| command) else {
        return Some(block.to_string());
    };
    let args = inside("command-args").unwrap_or_default();
    (!args.is_empty()).then(|| format!("{name} {args}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user_plain(text: &str) -> String {
        json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
    }

    #[test]
    fn a_slash_command_is_typed_back_with_its_arguments_and_harness_blocks_are_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.jsonl");
        let lines = [
            user_plain("<command-message>mustard:feature</command-message>\n<command-name>/mustard:feature</command-name>\n<command-args>travar o merge</command-args>"),
            user_plain("<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>"),
            user_plain("<local-command-stdout>Compacted</local-command-stdout>"),
            json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "text", "text": "e o painel também"},
                {"type": "text", "text": "<system-reminder>lembrete</system-reminder>"}
            ]}})
            .to_string(),
            json!({"type": "user", "isCompactSummary": true, "message": {"role": "user", "content": "This session is being continued"}}).to_string(),
        ];
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        assert_eq!(typed_messages(&file), vec!["/mustard:feature travar o merge".to_string(), "e o painel também".to_string()]);
    }
}
