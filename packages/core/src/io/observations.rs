//! Immutable receipts of a host tool result. A receipt proves what was
//! observed, not that the model's interpretation of it is true.
use crate::domain::text::{CutAt, cut};
use crate::io::fs::lock::LockedFile;
use crate::io::sha256::Sha256;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u8,
    pub spec: String,
    pub dispatch: u64,
    pub wave: u64,
    pub host: String,
    pub actor: String,
    pub call: String,
    pub tool: String,
    pub source: String,
    pub input_sha256: String,
    pub output_sha256: String,
    pub exit_code: Option<i64>,
    /// Como a chamada acabou. `returned`, `failed` ou `interrupted` vêm do
    /// host. Sem o código de saída, o resumo do executor de teste ou de
    /// compilação decide: `passed`, `failed`, ou `summary-hidden` quando o
    /// pipe tirou o resumo da saída. A leitura de um arquivo dá `succeeded`.
    pub result_status: String,
    pub observed_at: String,
}

pub fn digest(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.hex_digest()
}

/// Quanto a leitura de um recibo espera a vez de outra conexão antes de
/// desistir. Várias ondas gravam no mesmo banco ao mesmo tempo, e o disco
/// ocupado pelas compilações delas segura uma gravação por bem mais que
/// alguns milissegundos. Sem disputa, a leitura não espera nada.
const WAIT: Duration = Duration::from_secs(5);

/// Quanto a gravação de um recibo espera a trava do banco: a mesma espera
/// curta do que a busca aprende. O gancho grava depois de cada ferramenta e
/// não segura o agente esperando outra gravação: com o banco ocupado, o
/// recibo vai para a fila ([`queue_path`]), e a próxima gravação ou leitura
/// que pegar a trava o leva ao banco.
const WRITE_WAIT: Duration = crate::io::knowledge::observations::LEARNING_WAIT;

/// A fila dos recibos que acharam o banco ocupado: uma linha por recibo, ao
/// lado do banco. Quem lê os recibos junta a fila ao banco, então o recibo na
/// fila já é prova.
fn queue_path(root: &Path) -> std::path::PathBuf {
    root.join(".claude/mustard/observations-queue.ndjson")
}

/// Recusa `path` fora do projeto em `root`: a pasta que existe no caminho
/// leva a outro lugar, ou o próprio arquivo é um atalho.
fn inside(root: &Path, path: &Path) -> Result<(), String> {
    let mut ancestor = path.to_path_buf();
    while !ancestor.exists() {
        if !ancestor.pop() {
            return Err("observation-outside-project".into());
        }
    }
    if !ancestor
        .canonicalize()
        .ok()
        .zip(root.canonicalize().ok())
        .is_some_and(|(p, r)| p.starts_with(r))
        || path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("observation-outside-project".into());
    }
    Ok(())
}

fn open(root: &Path, write: bool, wait: Duration) -> Result<Connection, String> {
    let path = root.join(".claude/mustard/observations.db");
    inside(root, &path)?;
    if write {
        std::fs::create_dir_all(path.parent().ok_or("observation-missing-directory")?)
            .map_err(|e| e.to_string())?;
    }
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection =
        Connection::open_with_flags(path, flags).map_err(|_| "observation-store-unavailable")?;
    // A espera vem antes de tudo: até trocar o diário pede a trava.
    connection
        .busy_timeout(wait)
        .map_err(|e| e.to_string())?;
    if write {
        // No `WAL`, quem lê não espera quem grava, e cada gravação confirma o
        // disco uma vez só. O modo fica gravado no arquivo, e o banco de antes
        // passa a ele na primeira gravação; o disco que não guarda o `WAL`
        // deixa o banco no diário que já tinha, que também vale. A troca no
        // banco recém-criado espera a vez de outra gravação que a faz junto.
        crate::io::map_db::journal_to_wal(&connection, wait).map_err(|e| e.to_string())?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY, spec TEXT NOT NULL, dispatch INTEGER NOT NULL, observed_at TEXT NOT NULL, payload TEXT NOT NULL); CREATE INDEX IF NOT EXISTS receipts_scope ON receipts(spec,dispatch,observed_at);").map_err(|e|e.to_string())?;
    }
    Ok(connection)
}

/// A chave do recibo: o host, o agente e a chamada. A mesma chamada dá
/// sempre a mesma chave.
fn receipt_id(receipt: &Receipt) -> Result<String, String> {
    Ok(digest(
        &serde_json::to_vec(&(&receipt.host, &receipt.actor, &receipt.call))
            .map_err(|e| e.to_string())?,
    ))
}

/// `prior`, o recibo já guardado da mesma chamada, quando ele só difere de
/// `receipt` na hora; o conflito quando difere em mais.
fn same_call(receipt: &Receipt, prior: Receipt) -> Result<Receipt, String> {
    let mut expected = receipt.clone();
    expected.observed_at.clone_from(&prior.observed_at);
    if serde_json::to_value(&expected).ok() != serde_json::to_value(&prior).ok() {
        return Err("observation-conflicting-tool-result".into());
    }
    Ok(prior)
}

/// Retrying the same native call never renews its observation time. A changed
/// payload for the same call is a conflict, not another observation. With the
/// store held past the short write wait, the receipt goes to the queue instead
/// of being lost; the queue reaches the store with the next write that gets it.
pub fn record(root: &Path, receipt: &Receipt) -> Result<(String, Receipt), String> {
    let id = receipt_id(receipt)?;
    match store(root, &id, receipt) {
        Ok(prior) => same_call(receipt, prior).map(|prior| (id, prior)),
        Err(_) => queue(root, &id, receipt).map(|prior| (id, prior)),
    }
}

/// Grava `receipt` no banco, com a fila junto, e devolve o recibo que ficou
/// guardado com a chave `id`.
fn store(root: &Path, id: &str, receipt: &Receipt) -> Result<Receipt, String> {
    let old: String = with_queue(root, |tx| {
        insert(tx, id, receipt)?;
        tx.query_row("SELECT payload FROM receipts WHERE id=?1", [id], |row| row.get(0))
            .map_err(|e| e.to_string())
    })?;
    serde_json::from_str(&old).map_err(|_| "observation-corrupt-receipt".into())
}

/// Grava no banco, numa transação só, os recibos da fila e o que `then`
/// grava. A fila só se esvazia depois de o banco confirmar: a fila que sobra
/// de uma queda no meio entra de novo, e a chave repetida não duplica. Com a
/// fila presa por outra gravação além da espera, ela fica para a próxima.
fn with_queue<T>(root: &Path, then: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
    let mut connection = open(root, true, WRITE_WAIT)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let path = queue_path(root);
    let mut queue = if path.metadata().is_ok_and(|m| m.is_file() && m.len() > 0) && inside(root, &path).is_ok() {
        LockedFile::exclusive_within(&path, WRITE_WAIT).map_err(|e| e.to_string())?
    } else {
        None
    };
    if let Some(held) = queue.as_mut() {
        for (id, queued) in parsed(&held.read_to_string().map_err(|e| e.to_string())?) {
            insert(&tx, &id, &queued)?;
        }
    }
    let done = then(&tx)?;
    tx.commit().map_err(|e| e.to_string())?;
    if let Some(held) = queue.as_mut() {
        let _ = held.replace(b"");
    }
    Ok(done)
}

/// Põe `receipt` no banco com a chave `id`, quando a chave ainda não está lá.
fn insert(tx: &Connection, id: &str, receipt: &Receipt) -> Result<(), String> {
    let payload = serde_json::to_string(receipt).map_err(|e| e.to_string())?;
    let dispatch = i64::try_from(receipt.dispatch).map_err(|_| "observation-invalid-dispatch")?;
    tx.execute(
        "INSERT OR IGNORE INTO receipts VALUES(?1,?2,?3,?4,?5)",
        params![id, receipt.spec, dispatch, receipt.observed_at, payload],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Os recibos de um texto da fila, com a chave de cada um. A linha que não
/// se entende fica de fora.
fn parsed(text: &str) -> Vec<(String, Receipt)> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Receipt>(line).ok())
        .filter_map(|receipt| Some((receipt_id(&receipt).ok()?, receipt)))
        .collect()
}

/// Põe `receipt` na fila, com a mesma regra do banco: a chamada repetida
/// guarda a hora da primeira, e a de resultado diferente é conflito.
fn queue(root: &Path, id: &str, receipt: &Receipt) -> Result<Receipt, String> {
    let path = queue_path(root);
    inside(root, &path)?;
    let mut held = LockedFile::exclusive_within(&path, WAIT)
        .map_err(|e| e.to_string())?
        .ok_or("observation-store-unavailable")?;
    let text = held.read_to_string().map_err(|e| e.to_string())?;
    if let Some((_, prior)) = parsed(&text).into_iter().find(|(queued, _)| queued == id) {
        return same_call(receipt, prior);
    }
    let line = serde_json::to_string(receipt).map_err(|e| e.to_string())?;
    held.append_line(&line).map_err(|e| e.to_string())?;
    Ok(receipt.clone())
}

/// Os recibos ainda na fila, depois de tentar levá-los ao banco com a espera
/// curta da gravação. Quem lê os recibos lê a fila antes do banco: o recibo
/// que sai da fila no meio da leitura já está no banco.
fn waiting(root: &Path) -> Vec<(String, Receipt)> {
    let path = queue_path(root);
    if !path.metadata().is_ok_and(|m| m.is_file() && m.len() > 0) || inside(root, &path).is_err() {
        return Vec::new();
    }
    let _ = with_queue(root, |_| Ok(()));
    crate::io::fs::lock::read_shared(&path).map(|text| parsed(&text)).unwrap_or_default()
}

pub fn read(root: &Path, id: &str) -> Result<Receipt, String> {
    if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("observation-invalid-receipt-id".into());
    }
    let queued = waiting(root);
    // Só a falta da linha diz que o recibo não existe. O banco preso por outra
    // conexão além da espera, ou que não se deixa ler, diz que o banco está
    // indisponível: quem cita o recibo grava de novo, em vez de achar que ele
    // sumiu. O recibo na fila vale como o do banco.
    let stored = open(root, false, WAIT).and_then(|connection| {
        connection
            .query_row(
                "SELECT payload FROM receipts WHERE id=?1 AND length(payload)<=8192",
                [id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => "observation-receipt-not-found".into(),
                _ => "observation-store-unavailable".into(),
            })
    });
    match stored {
        Ok(payload) => serde_json::from_str(&payload).map_err(|_| "observation-corrupt-receipt".into()),
        Err(reason) => queued.into_iter().find(|(queued, _)| queued == id).map(|(_, receipt)| receipt).ok_or(reason),
    }
}

/// The receipt `id` of this spec and dispatch. A receipt of another wave or
/// attempt is refused, so a page and its detail never mix dispatches.
fn scoped(root: &Path, spec: &str, dispatch: u64, id: &str) -> Result<Receipt, String> {
    let receipt = read(root, id)?;
    if receipt.spec != spec || receipt.dispatch != dispatch {
        return Err("observation-belongs-to-another-dispatch".into());
    }
    Ok(receipt)
}

/// Quantos caracteres do comando a página mostra de cada recibo, antes das
/// reticências.
const COMMAND_LINE_CHARS: usize = 80;

/// A primeira linha do comando gravado, com no máximo [`COMMAND_LINE_CHARS`]
/// caracteres e as reticências, pelo corte do núcleo ([`cut`]). A troca de
/// pasta do começo, o grupo que um gancho abre depois dela e a identidade que
/// um gancho põe à frente de um comando nativo ficam de fora: repetem em toda
/// chamada da onda e encheriam a linha. Uma linha ou um prefixo deixados de
/// fora contam como texto que passa do teto e também terminam a linha em `…`;
/// o comando inteiro fica no recibo.
fn command_line(source: &str) -> String {
    let whole = source.trim();
    let rest = &whole[command_start(whole)..];
    let mut lines = rest.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or_default();
    let left_out = lines.next().is_some() || rest.len() < whole.len();
    let line = if left_out { format!("{first}…") } else { first.to_string() };
    cut(&line, COMMAND_LINE_CHARS + 1, CutAt::Char)
}

/// Where the command the agent wrote starts in `source`, in bytes: past the
/// leading changes of directory, the groups a hook opens after them and the
/// identity a hook puts ahead of a native command.
#[must_use]
pub fn command_start(source: &str) -> usize {
    let mut rest = source.trim_start();
    while let Some(next) = after_leading_cd(rest).or_else(|| opened_group(rest)).or_else(|| after_identity(rest)) {
        rest = next;
    }
    source.len() - rest.len()
}

/// What follows a leading `cd <dir>` ended by `&&`, `;` or a line break, with
/// quotes respected. `None` when no other command follows it.
fn after_leading_cd(text: &str) -> Option<&str> {
    if !text.strip_prefix("cd")?.starts_with([' ', '\t']) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut quote = None;
    for (at, byte) in bytes.iter().enumerate() {
        let end = match (quote, byte) {
            (None, b'\'' | b'"') => {
                quote = Some(*byte);
                continue;
            }
            (Some(open), _) if open == *byte => {
                quote = None;
                continue;
            }
            (None, b';' | b'\n') => at + 1,
            (None, b'&') if bytes.get(at + 1) == Some(&b'&') => at + 2,
            _ => continue,
        };
        return Some(text[end..].trim_start()).filter(|rest| !rest.is_empty());
    }
    None
}

/// What follows a `(` alone on the first line, the group a hook wraps the
/// original command in.
fn opened_group(text: &str) -> Option<&str> {
    let (line, rest) = text.split_once('\n')?;
    (line.trim() == "(").then(|| rest.trim_start()).filter(|rest| !rest.is_empty())
}

/// What follows a first line `export MUSTARD_<NAME>=<value>`, the session or
/// the agent a hook hands to a native command. An `export` of any other
/// variable is part of the agent's command and stays.
fn after_identity(text: &str) -> Option<&str> {
    let (line, rest) = text.split_once('\n')?;
    let (name, _) = line.strip_prefix("export MUSTARD_")?.split_once('=')?;
    (!name.is_empty() && name.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'_'))
        .then(|| rest.trim_start())
        .filter(|rest| !rest.is_empty())
}

/// One page of the dispatch's receipts, newest first: the receipt, its time,
/// its result and one short line of the command. [`detail`] gives the rest.
pub fn page(root: &Path, spec: &str, dispatch: u64, before: Option<&str>) -> Result<Value, String> {
    let cursor = before
        .map(|id| scoped(root, spec, dispatch, id).map(|receipt| (receipt.observed_at, id)))
        .transpose()?;
    let before_cursor = |observed_at: &str, id: &str| {
        cursor.as_ref().is_none_or(|(at, cut)| observed_at < at.as_str() || (observed_at == at.as_str() && id < *cut))
    };
    // A fila se lê antes do banco, e o recibo nos dois vale pelo do banco.
    let mut rows: Vec<(String, Receipt)> = waiting(root)
        .into_iter()
        .filter(|(id, receipt)| receipt.spec == spec && receipt.dispatch == dispatch && before_cursor(&receipt.observed_at, id))
        .collect();
    let dispatch = i64::try_from(dispatch).map_err(|_| "observation-invalid-dispatch")?;
    match open(root, false, WAIT) {
        Ok(connection) => {
            let mut statement=connection.prepare("SELECT id,payload FROM receipts WHERE spec=?1 AND dispatch=?2 AND (?3 IS NULL OR observed_at<?3 OR (observed_at=?3 AND id<?4)) ORDER BY observed_at DESC,id DESC LIMIT 33").map_err(|e|e.to_string())?;
            let stored = statement
                .query_map(
                    params![
                        spec,
                        dispatch,
                        cursor.as_ref().map(|c| &c.0),
                        cursor.as_ref().map(|c| c.1)
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(|e| e.to_string())?;
            for row in stored {
                let (id, payload) = row.map_err(|e| e.to_string())?;
                let receipt: Receipt =
                    serde_json::from_str(&payload).map_err(|_| "observation-corrupt-receipt")?;
                rows.retain(|(queued, _)| *queued != id);
                rows.push((id, receipt));
            }
        }
        Err(reason) if rows.is_empty() => return Err(reason),
        Err(_) => {}
    }
    rows.sort_by(|(a, first), (b, second)| second.observed_at.cmp(&first.observed_at).then_with(|| b.cmp(a)));
    let mut output: Vec<Value> = rows
        .into_iter()
        .take(33)
        .map(|(id, receipt)| json!({"receipt":id,"observed_at":receipt.observed_at,"tool":receipt.tool,
            "source":command_line(&receipt.source),"exit_code":receipt.exit_code,"result_status":receipt.result_status}))
        .collect();
    let next_before = (output.len() > 32).then(|| output[31]["receipt"].clone());
    output.truncate(32);
    Ok(json!({"receipts":output,"next_before":next_before}))
}

/// One receipt of the dispatch, whole: the command as recorded and the
/// digests of its input and output.
pub fn detail(root: &Path, spec: &str, dispatch: u64, id: &str) -> Result<Value, String> {
    let receipt = scoped(root, spec, dispatch, id)?;
    Ok(json!({"receipt":id,"observed_at":receipt.observed_at,"host":receipt.host,"tool":receipt.tool,
        "source":receipt.source,"exit_code":receipt.exit_code,"result_status":receipt.result_status,
        "input_sha256":receipt.input_sha256,"output_sha256":receipt.output_sha256}))
}

pub fn list(root: &Path, spec: &str, dispatch: u64) -> Result<Vec<Value>, String> {
    Ok(page(root, spec, dispatch, None)?["receipts"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// A native read must have delivered the exact version sent to this attempt.
/// Existence, selection, a partial read or an agent's claim are insufficient.
pub fn read_receipt(root:&Path,spec:&str,dispatch:u64,source:&str,sha:&str)->Option<Value> {
    let delivered=|id:String,observed_at:String|json!({"receipt":id,"observed_at":observed_at,"body_delivered":true,"understanding_verified":false});
    // A leitura ainda na fila prova como a do banco.
    let queued=waiting(root).into_iter()
        .filter(|(_,r)|r.spec==spec && r.dispatch==dispatch && r.tool=="Read" && r.source==source && r.output_sha256==sha && r.result_status=="succeeded")
        .max_by(|(a,first),(b,second)|first.observed_at.cmp(&second.observed_at).then_with(||a.cmp(b)));
    let stored=open(root,false,WAIT).ok().and_then(|connection|connection.query_row("SELECT id,observed_at FROM receipts WHERE spec=?1 AND dispatch=?2 AND json_extract(payload,'$.tool')='Read' AND json_extract(payload,'$.source')=?3 AND json_extract(payload,'$.output_sha256')=?4 AND json_extract(payload,'$.result_status')='succeeded' ORDER BY observed_at DESC LIMIT 1",
        params![spec,i64::try_from(dispatch).ok()?,source,sha],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).ok());
    stored.or_else(||queued.map(|(id,receipt)|(id,receipt.observed_at))).map(|(id,observed_at)|delivered(id,observed_at))
}

/// The only accepted assertion of freshness is an immutable native receipt
/// from this exact dispatch. Historical references retain the original date.
pub fn resolve(
    root: &Path,
    spec: &str,
    dispatch: u64,
    sent_at: &str,
    entries: &Value,
) -> Result<Value, String> {
    let entries = entries
        .as_array()
        .filter(|e| e.len() <= 16)
        .ok_or("observations-expect-at-most-16-references")?;
    let since = chrono::DateTime::parse_from_rfc3339(sent_at)
        .map_err(|_| "observation-unknown-dispatch-time")?;
    let mut output = Vec::new();
    for entry in entries {
        let object = entry.as_object().ok_or("observation-invalid-reference")?;
        if object
            .keys()
            .any(|key| !["receipt", "mode", "text"].contains(&key.as_str()))
        {
            return Err("observation-reference-accepts-only-receipt-mode-text".into());
        }
        let id = entry["receipt"]
            .as_str()
            .ok_or("observation-missing-receipt")?;
        let mode = entry["mode"]
            .as_str()
            .filter(|m| ["current", "historical"].contains(m))
            .ok_or("observation-mode-must-be-current-or-historical")?;
        let text = entry["text"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 2000)
            .ok_or("observation-needs-short-reported-claim")?;
        let receipt = read(root, id)?;
        let observed = chrono::DateTime::parse_from_rfc3339(&receipt.observed_at)
            .map_err(|_| "observation-invalid-time")?;
        if receipt.version != 1 || receipt.spec != spec {
            return Err("observation-belongs-to-another-spec".into());
        }
        if mode == "current"
            && (receipt.dispatch != dispatch || observed < since || observed > chrono::Utc::now())
        {
            return Err("observation-historical-source-cannot-be-current; recheck with a tool or use historical".into());
        }
        output.push(json!({"receipt":id,"mode":mode,"text":text,"observed_at":receipt.observed_at,
            "dispatch":receipt.dispatch,"wave":receipt.wave,"host":receipt.host,"tool":receipt.tool,"source":receipt.source,
            "exit_code":receipt.exit_code,"result_status":receipt.result_status,"input_sha256":receipt.input_sha256,"output_sha256":receipt.output_sha256,
            "claim_status":"reported-interpretation","tool_result_observed":true,"claim_verified":false}));
    }
    Ok(json!(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt() -> Receipt {
        Receipt {
            version: 1,
            spec: "demo".into(),
            dispatch: 10,
            wave: 1,
            host: "claude".into(),
            actor: "hashed-actor".into(),
            call: "hashed-call".into(),
            tool: "Bash".into(),
            source: "read-only probe".into(),
            input_sha256: digest(b"probe"),
            output_sha256: digest(b"denied"),
            exit_code: Some(1),
            result_status: "failed".into(),
            observed_at: "2026-01-01T01:00:00Z".into(),
        }
    }
    #[test]
    fn reading_evidence_requires_this_attempt_this_body_and_success() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        let mut read=receipt();read.tool="Read".into();read.source=".claude/skills/store/SKILL.md".into();
        read.output_sha256=digest(b"full procedure");read.result_status="succeeded".into();read.exit_code=None;
        record(root,&read).unwrap();
        let observed=read_receipt(root,"demo",10,&read.source,&read.output_sha256).unwrap();
        assert_eq!(observed["body_delivered"],true);assert_eq!(observed["understanding_verified"],false);
        assert!(read_receipt(root,"demo",11,&read.source,&read.output_sha256).is_none());
        assert!(read_receipt(root,"demo",10,&read.source,&digest(b"partial procedure")).is_none());
        assert!(read_receipt(root,"other",10,&read.source,&read.output_sha256).is_none());
    }
    #[test]
    fn a_replayed_call_keeps_its_date_and_conflicting_output_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = receipt();
        let (id, saved) = record(dir.path(), &first).unwrap();
        first.observed_at = "2026-01-02T01:00:00Z".into();
        assert_eq!(
            record(dir.path(), &first).unwrap().1.observed_at,
            saved.observed_at
        );
        first.output_sha256 = digest(b"allowed");
        assert!(
            record(dir.path(), &first)
                .unwrap_err()
                .contains("conflicting")
        );
        first.output_sha256 = digest(b"denied");
        first.dispatch = 11;
        assert!(
            record(dir.path(), &first)
                .unwrap_err()
                .contains("conflicting")
        );
        assert_eq!(read(dir.path(), &id).unwrap().exit_code, Some(1));
    }
    #[test]
    fn an_old_dispatch_is_historical_and_cannot_be_dated_as_current() {
        let dir = tempfile::tempdir().unwrap();
        let (id, _) = record(dir.path(), &receipt()).unwrap();
        let entry =
            |mode| json!([{"receipt":id,"mode":mode,"text":"The bucket was inaccessible."}]);
        assert!(
            resolve(
                dir.path(),
                "demo",
                20,
                "2026-01-02T00:00:00Z",
                &entry("current")
            )
            .is_err()
        );
        let history = resolve(
            dir.path(),
            "demo",
            20,
            "2026-01-02T00:00:00Z",
            &entry("historical"),
        )
        .unwrap();
        assert_eq!(history[0]["observed_at"], "2026-01-01T01:00:00Z");
        assert_eq!(history[0]["claim_verified"], false);
        assert!(
            resolve(
                dir.path(),
                "other",
                10,
                "2026-01-01T00:00:00Z",
                &entry("current")
            )
            .is_err()
        );
        let forged =
            json!([{"receipt":id,"mode":"current","text":"Accessible","observed_at":"2026-01-03"}]);
        assert!(resolve(dir.path(), "demo", 10, "2026-01-01T00:00:00Z", &forged).is_err());
    }
    /// O caminho do banco, para outra conexão abri-lo como outra onda o abre.
    fn store(root: &Path) -> std::path::PathBuf {
        root.join(".claude/mustard/observations.db")
    }
    #[test]
    fn a_receipt_waits_for_another_write_instead_of_being_lost() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        record(&root, &receipt()).unwrap();
        let (held, wait_for_hold) = std::sync::mpsc::channel();
        let path = store(&root);
        let holder = std::thread::spawn(move || {
            let other = Connection::open(path).unwrap();
            other.execute_batch("BEGIN EXCLUSIVE").unwrap();
            held.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(200));
            other.execute_batch("COMMIT").unwrap();
        });
        wait_for_hold.recv().unwrap();
        let mut during = receipt();
        during.call = "call-during-another-write".into();
        let written = record(&root, &during);
        holder.join().unwrap();
        let (id, _) = written.unwrap();
        assert_eq!(read(&root, &id).unwrap().call, "call-during-another-write");
    }
    /// Os dois primeiros recibos de um projeto, gravados ao mesmo tempo,
    /// entram os dois, rodada após rodada: a troca do diário no banco
    /// recém-criado espera a vez da outra gravação.
    #[test]
    fn the_first_two_receipts_recorded_at_the_same_time_are_both_kept() {
        for round in 0..12 {
            let dir = tempfile::tempdir().unwrap();
            let start = std::sync::Barrier::new(2);
            let written: Vec<Result<String, String>> = std::thread::scope(|scope| {
                let writers: Vec<_> = ["first-call", "second-call"]
                    .into_iter()
                    .map(|call| {
                        let (root, start) = (dir.path(), &start);
                        scope.spawn(move || {
                            let mut each = receipt();
                            each.call = call.into();
                            start.wait();
                            record(root, &each).map(|(id, _)| id)
                        })
                    })
                    .collect();
                writers.into_iter().map(|writer| writer.join().unwrap()).collect()
            });
            for id in written {
                let id = id.unwrap_or_else(|err| panic!("round {round}: both receipts are recorded: {err}"));
                assert!(read(dir.path(), &id).is_ok(), "round {round}");
            }
        }
    }
    #[test]
    fn a_receipt_reads_while_another_write_holds_a_store_from_the_rollback_journal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (id, _) = record(&root, &receipt()).unwrap();
        // Um banco gravado antes fica no diário de desfazer até a próxima gravação.
        let old = Connection::open(store(&root)).unwrap();
        let mode: String = old
            .pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
        drop(old);
        let mut next = receipt();
        next.call = "call-after-the-old-journal".into();
        record(&root, &next).unwrap();
        let (held, wait_for_hold) = std::sync::mpsc::channel();
        let (finished, wait_for_reader) = std::sync::mpsc::channel::<()>();
        let path = store(&root);
        let holder = std::thread::spawn(move || {
            let other = Connection::open(path).unwrap();
            other
                .execute_batch("BEGIN EXCLUSIVE; INSERT INTO receipts VALUES('uncommitted','demo',10,'2026-01-01T02:00:00Z','{}')")
                .unwrap();
            held.send(()).unwrap();
            wait_for_reader.recv().unwrap();
            other.execute_batch("ROLLBACK").unwrap();
        });
        wait_for_hold.recv().unwrap();
        let seen = read(&root, &id);
        let listed = list(&root, "demo", 10);
        finished.send(()).unwrap();
        holder.join().unwrap();
        assert_eq!(seen.unwrap().exit_code, Some(1));
        assert_eq!(listed.unwrap().len(), 2, "the write still open is not seen");
    }
    #[test]
    fn a_store_held_past_the_wait_is_unavailable_and_only_a_missing_row_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (id, _) = record(&root, &receipt()).unwrap();
        // No diário de desfazer, quem grava com trava exclusiva barra também
        // quem lê, e a leitura espera a vez até desistir.
        let old = Connection::open(store(&root)).unwrap();
        let mode: String = old
            .pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
        drop(old);
        let (held, wait_for_hold) = std::sync::mpsc::channel();
        let (finished, wait_for_reader) = std::sync::mpsc::channel::<()>();
        let path = store(&root);
        let holder = std::thread::spawn(move || {
            let other = Connection::open(path).unwrap();
            other.execute_batch("BEGIN EXCLUSIVE").unwrap();
            held.send(()).unwrap();
            wait_for_reader.recv().unwrap();
            other.execute_batch("ROLLBACK").unwrap();
        });
        wait_for_hold.recv().unwrap();
        let started = std::time::Instant::now();
        let busy = read(&root, &id);
        let waited = started.elapsed();
        finished.send(()).unwrap();
        holder.join().unwrap();
        assert_eq!(busy.unwrap_err(), "observation-store-unavailable");
        assert!(waited >= WAIT, "the read gave up after {waited:?}, before the wait");
        assert_eq!(
            read(&root, &digest(b"a call never recorded")).unwrap_err(),
            "observation-receipt-not-found"
        );
        assert_eq!(read(&root, &id).unwrap().exit_code, Some(1));
    }
    /// Com o banco preso por outra gravação, o recibo não espera por ela: vai
    /// para a fila e já aparece na leitura, a da conferência da entrega
    /// inclusive. A gravação seguinte, com o banco livre, leva a fila ao
    /// banco uma vez só.
    #[test]
    fn a_receipt_is_queued_while_the_store_is_held_and_stored_once_after() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        record(root, &receipt()).unwrap();
        let holder = Connection::open(store(root)).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let mut during = receipt();
        during.call = "call-during-another-write".into();
        let mut skill = receipt();
        skill.call = "skill-read-during-another-write".into();
        skill.tool = "Read".into();
        skill.source = ".claude/skills/store/SKILL.md".into();
        skill.output_sha256 = digest(b"full procedure");
        skill.result_status = "succeeded".into();
        skill.exit_code = None;
        let started = std::time::Instant::now();
        let queued = record(root, &during);
        let waited = started.elapsed();
        let skill_queued = record(root, &skill);
        let found = queued.as_ref().ok().map(|(id, _)| read(root, id).map(|seen| seen.call));
        let listed = list(root, "demo", 10).map(|rows| rows.len());
        let skill_seen = read_receipt(root, "demo", 10, &skill.source, &skill.output_sha256).is_some();
        holder.execute_batch("COMMIT").unwrap();
        // 3 s and not less: opening the store on a slow CI runner takes over a
        // second; the old wait on a held store was 5 s.
        assert!(waited < Duration::from_secs(3), "the receipt waited {waited:?} for the held store");
        queued.unwrap();
        skill_queued.unwrap();
        assert_eq!(found, Some(Ok("call-during-another-write".to_string())));
        assert_eq!(listed, Ok(3));
        assert!(skill_seen, "the delivery check sees the queued skill read");
        let mut after = receipt();
        after.call = "call-after-the-store-is-free".into();
        record(root, &after).unwrap();
        let rows: i64 = holder.query_row("SELECT count(*) FROM receipts", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 4, "the queue went into the store, each receipt once");
        assert_eq!(list(root, "demo", 10).unwrap().len(), 4);
        let left = std::fs::read_to_string(root.join(".claude/mustard/observations-queue.ndjson")).unwrap_or_default();
        assert_eq!(left, "", "the queue is empty once stored");
    }
    #[test]
    fn every_receipt_remains_accessible_without_loading_the_whole_history() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..35 {
            let mut row = receipt();
            row.call = format!("call-{n}");
            record(dir.path(), &row).unwrap();
        }
        let first = page(dir.path(), "demo", 10, None).unwrap();
        assert_eq!(first["receipts"].as_array().unwrap().len(), 32);
        let cursor = first["next_before"].as_str().unwrap();
        let last = page(dir.path(), "demo", 10, Some(cursor)).unwrap();
        assert_eq!(last["receipts"].as_array().unwrap().len(), 3);
        assert!(last["next_before"].is_null());
        let ids: std::collections::BTreeSet<_> = first["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .chain(last["receipts"].as_array().unwrap())
            .map(|r| r["receipt"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 35);
        assert!(page(dir.path(), "other", 10, Some(cursor)).is_err());
        assert!(page(dir.path(), "demo", 11, Some(cursor)).is_err());
    }
}
