//! `copy_witness` — a testemunha da cópia da página da spec.
//!
//! Depois de cada gravação que prepara a cópia, a conversa manda as escritas
//! de cada lote ao banco da página da spec com a ferramenta `ArtifactData`,
//! ação `batch`. O resultado de cada lote chega pelo harness no
//! `PostToolUse`, em `tool_response`, que o modelo não escreve: é por ele que
//! o Mustard sabe que o lote entrou e com qual versão cada documento ficou.
//!
//! ## O que ela grava
//!
//! A preparação da cópia deixa na pasta da cópia o registro da cópia da
//! página da spec, com os lotes dela (`pages::copy::SPEC_RECORD`). A
//! testemunha age só no lote mandado ao endereço da página da spec atual,
//! cujas escritas casam com um dos lotes desse registro, e só quando o
//! resultado diz `committed` e traz a versão de cada documento escrito, uma
//! linha por escrita, como `- set "ranges"/"1100" (version 6)`. As versões
//! de cada lote ficam guardadas na pasta da cópia. Quando todo lote voltou,
//! ela grava o registro `copy` com o `last` guardado e as versões de todos
//! os lotes, pela mesma gravação do `run write`, e apaga o registro: o mesmo
//! resultado lido duas vezes não grava duas cópias. Quem toma o registro
//! para gravar o renomeia antes, e só um consegue: dois lotes que voltam ao
//! mesmo tempo gravam uma cópia só.
//!
//! ## Nunca barra
//!
//! O lote que falhou, o de outra página, outra ação da ferramenta e a spec
//! sem registro passam calados, sem gravar nada: a ordem da cópia continua
//! com a gravação à mão (`run write copy`) para quando a testemunha não
//! avisar. Gravada a cópia, ela devolve `Inject` com a linha que diz ao
//! assistente para não gravar de novo.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::translate;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use crate::commands::spec_events::pages::copy::{spec_page_url, write_mark, FOLDER, SPEC_RECORD};
use crate::hooks::write::write_gate::say;
use crate::shared::spec_state::DiskSpecState;

/// A testemunha da cópia, no `PostToolUse` da ferramenta do banco das
/// páginas.
pub struct CopyWitness;

/// O registro tomado por quem vai gravar a cópia, na pasta da cópia: sai
/// dali gravada a cópia, e volta a ser o registro quando a gravação falha.
const TAKEN: &str = "record.taken";

/// O arquivo com as versões que o lote `n` (a partir de 1) devolveu, na
/// pasta da cópia `folder`.
fn returned_file(folder: &Path, n: usize) -> PathBuf {
    folder.join(format!("returned-{n}.json"))
}

impl Check for CopyWitness {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PostToolUse)
            || input.tool_input.get("action").and_then(Value::as_str) != Some("batch")
        {
            return Ok(Verdict::Allow);
        }
        let root = ctx.project_dir_or_cwd(input);
        if !witness(Path::new(&root), input.session_id.as_deref(), input) {
            return Ok(Verdict::Allow);
        }
        let lang = ctx.config.language().text_or_default();
        let context = say("page.copy.recorded", lang, &[("{page}", translate("page.name.spec", lang))]);
        Ok(Verdict::Inject { context })
    }
}

/// Lê o resultado do lote de `input` na spec atual do projeto `root` e grava
/// a cópia quando ele é o último a voltar; `true` quando gravou.
fn witness(root: &Path, session: Option<&str>, input: &HookInput) -> bool {
    let state = DiskSpecState::new(root);
    let Some(spec) = state.active(session) else { return false };
    let Some(url) = state.log(&spec).as_ref().and_then(spec_page_url) else { return false };
    if input.tool_input.get("url").and_then(Value::as_str).map(str::trim) != Some(url.trim()) {
        return false;
    }
    let Some(folder) = copy_folder(root, &spec) else { return false };
    let Some(saved) = read_json(&folder.join(SPEC_RECORD)) else { return false };
    let Some(batches) = saved["batches"].as_array() else { return false };
    let writes = sent_writes(&input.tool_input);
    let Some(n) = batch_of(&writes, batches) else { return false };
    let Some(versions) = input.raw.get("tool_response").and_then(|response| returned(response, &writes)) else {
        return false;
    };
    if write_json(&returned_file(&folder, n), &json!({ "versions": versions })).is_err() {
        return false;
    }
    if !(1..=batches.len()).all(|batch| returned_file(&folder, batch).is_file()) {
        return false;
    }
    // Só quem renomeia o registro grava: o outro lote que voltou junto, ou o
    // mesmo resultado lido de novo, acha o registro já tomado.
    let (record, taken) = (folder.join(SPEC_RECORD), folder.join(TAKEN));
    if std::fs::rename(&record, &taken).is_err() {
        return false;
    }
    let mut draft = saved["record"].as_object().cloned().unwrap_or_default();
    let mut all = Map::new();
    for batch in 1..=batches.len() {
        if let Some(Value::Object(versions)) = read_json(&returned_file(&folder, batch)).map(|v| v["versions"].clone()) {
            all.extend(versions);
        }
    }
    draft.insert("versions".into(), Value::Object(all));
    draft.insert("author".into(), json!("hook"));
    if crate::commands::spec_events::write::record(root, &spec, "copy", draft, PhaseWriter::Binary).is_err() {
        let _ = std::fs::rename(&taken, &record);
        return false;
    }
    let _ = std::fs::remove_file(&taken);
    for batch in 1..=batches.len() {
        let _ = std::fs::remove_file(returned_file(&folder, batch));
    }
    true
}

/// A pasta da cópia da spec `spec` do projeto `root`.
fn copy_folder(root: &Path, spec: &str) -> Option<PathBuf> {
    let paths = ClaudePaths::for_project(root).ok()?;
    Some(paths.for_spec(spec).ok()?.dir().join(FOLDER))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write_json(path: &Path, value: &Value) -> Result<(), Error> {
    mustard_core::io::fs::write_atomic(path, value.to_string().as_bytes())
}

/// As escritas que a chamada mandou, em `writes`: a lista, ou o texto dela,
/// quando chega como texto.
fn sent_writes(tool_input: &Value) -> Vec<Value> {
    match tool_input.get("writes") {
        Some(Value::Array(writes)) => writes.clone(),
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// O número (a partir de 1) do lote do registro, em `batches`, cujas
/// escritas são as de `writes`: as mesmas, uma a uma, pela marca de cada
/// ([`write_mark`]). O `file_path` mandado é absoluto, e o do registro é
/// relativo ao projeto: um casa com o outro quando termina nele.
fn batch_of(writes: &[Value], batches: &[Value]) -> Option<usize> {
    let sent: Vec<String> = writes.iter().map(write_mark).collect();
    if sent.is_empty() {
        return None;
    }
    let same = |sent: &str, recorded: &str| sent == recorded || sent.ends_with(&format!("/{recorded}"));
    batches.iter().position(|batch| {
        let recorded: Vec<&str> = batch.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        recorded.len() == sent.len()
            && recorded.iter().all(|r| sent.iter().any(|s| same(s, r)))
            && sent.iter().all(|s| recorded.iter().any(|r| same(s, r)))
    })
    .map(|index| index + 1)
}

/// A versão que o resultado `response` do lote devolveu a cada documento
/// escrito, pelo nome `coleção/doc_id`. `None` quando o lote não deu certo:
/// o resultado não diz `committed`, ou falta a versão de algum documento que
/// o lote grava (`set`).
fn returned(response: &Value, writes: &[Value]) -> Option<Map<String, Value>> {
    let text = response_text(response);
    if !text.to_lowercase().contains("committed") {
        return None;
    }
    let versions: Map<String, Value> = text.lines().filter_map(version_line).collect();
    let complete = writes.iter().filter(|w| w["op"] == json!("set")).all(|w| {
        let doc = format!("{}/{}", w["collection"].as_str().unwrap_or_default(), w["doc_id"].as_str().unwrap_or_default());
        versions.contains_key(&doc)
    });
    complete.then_some(versions)
}

/// O texto do resultado de uma ferramenta: o próprio texto, ou os blocos de
/// texto de uma lista, ou o conteúdo de um objeto, juntos, uma linha cada.
fn response_text(response: &Value) -> String {
    match response {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().map(response_text).collect::<Vec<_>>().join("\n"),
        Value::Object(map) => ["text", "content", "result", "output"]
            .into_iter()
            .find_map(|key| map.get(key))
            .map(response_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// A versão de uma linha do resultado do lote, como
/// `- set "ranges"/"1100" (version 6)`: o nome `ranges/1100` e o número 6.
fn version_line(line: &str) -> Option<(String, Value)> {
    let line = line.trim().trim_start_matches(['-', '*']).trim();
    let (_op, rest) = line.split_once(char::is_whitespace)?;
    let (collection, rest) = quoted(rest.trim_start())?;
    let (doc_id, rest) = quoted(rest.trim_start().strip_prefix('/')?.trim_start())?;
    let after = rest.split_once("(version")?.1.trim_start();
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    let version: u64 = digits.parse().ok()?;
    Some((format!("{collection}/{doc_id}"), json!(version)))
}

/// O texto entre aspas no começo de `text` e o que vem depois dele.
fn quoted(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some((&rest[..end], &rest[end + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linha de cada escrita do resultado dá o nome do documento e a
    /// versão; a linha sem versão, ou fora do formato, não dá nada.
    #[test]
    fn each_result_line_gives_the_document_and_its_version() {
        assert_eq!(version_line(r#"- set "ranges"/"1100" (version 6)"#), Some(("ranges/1100".into(), json!(6))));
        assert_eq!(
            version_line(r#"  - set "computed"/"current" (version 12)"#),
            Some(("computed/current".into(), json!(12)))
        );
        assert_eq!(version_line(r#"- delete "ranges"/"1200""#), None);
        assert_eq!(version_line("Batch committed: 2 writes"), None);
    }

    /// O resultado chega como texto, como lista de blocos de texto ou como
    /// objeto com o conteúdo: os três dão o mesmo texto.
    #[test]
    fn the_result_reads_as_text_or_as_blocks() {
        let text = "Batch committed\n- set \"ranges\"/\"0\" (version 2)";
        let blocks = json!([{"type": "text", "text": "Batch committed"}, {"type": "text", "text": "- set \"ranges\"/\"0\" (version 2)"}]);
        let object = json!({"content": [{"type": "text", "text": text}]});
        for response in [json!(text), blocks, object] {
            let writes = [json!({"op": "set", "collection": "ranges", "doc_id": "0"})];
            let versions = returned(&response, &writes).unwrap_or_else(|| panic!("{response}"));
            assert_eq!(versions.get("ranges/0"), Some(&json!(2)), "{response}");
        }
    }

    /// O lote que não diz `committed`, ou que não traz a versão de um
    /// documento que ele grava, não deu certo.
    #[test]
    fn a_failed_batch_gives_no_versions() {
        let writes = [
            json!({"op": "set", "collection": "ranges", "doc_id": "0"}),
            json!({"op": "set", "collection": "computed", "doc_id": "current"}),
        ];
        assert_eq!(returned(&json!("Error: version conflict on \"ranges\"/\"0\""), &writes), None);
        assert_eq!(returned(&json!("Batch committed\n- set \"ranges\"/\"0\" (version 2)"), &writes), None);
    }

    /// O lote casa pelo arquivo de cada escrita, mandado com o caminho
    /// absoluto, e pelo nome do documento na escrita que o tira; um lote com
    /// uma escrita a mais ou a menos não casa.
    #[test]
    fn the_batch_matches_by_the_written_files() {
        let batches = json!([
            [".claude/spec/x/copy/ranges/0.json", "ranges/100"],
            [".claude/spec/x/copy/computed/current.json"]
        ]);
        let batches = batches.as_array().unwrap();
        let first = [
            json!({"op": "delete", "collection": "ranges", "doc_id": "100"}),
            json!({"op": "set", "collection": "ranges", "doc_id": "0", "file_path": "/p/.claude/spec/x/copy/ranges/0.json"}),
        ];
        assert_eq!(batch_of(&first, batches), Some(1));
        let second = [json!({"op": "set", "collection": "computed", "doc_id": "current",
            "file_path": "C:\\p\\.claude\\spec\\x\\copy\\computed\\current.json"})];
        assert_eq!(batch_of(&second, batches), Some(2));
        assert_eq!(batch_of(&first[1..], batches), None, "one write short");
        assert_eq!(batch_of(&[], batches), None);
    }
}
