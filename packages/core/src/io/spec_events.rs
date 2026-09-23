//! `spec_events` — a gravação e a leitura do `spec.ndjson`.
//!
//! Só o binário escreve no arquivo. Cada gravação pega a trava do próprio
//! arquivo, lê o maior número, soma 1 e acrescenta a linha inteira de uma vez;
//! outra sessão que grava ao mesmo tempo espera a trava e grava com o número
//! seguinte. A leitura pega a trava compartilhada, então nunca vê uma linha
//! pela metade de uma gravação em curso.
//!
//! Uma linha pela metade que ficou de uma queda é pulada na leitura, com
//! aviso, e a gravação seguinte começa numa linha nova. O arquivo nunca é
//! descartado. O próximo número parte do maior que existe, inclusive depois
//! de uma edição à mão.
//!
//! Quem depende do arquivo como ficou (a conta das ondas depois da aprovação)
//! recebe o conteúdo recém-gravado ainda com a trava presa: a gravação
//! seguinte só entra depois. Numa pasta de spec do projeto, a linha da spec no
//! índice (`io::spec_index`) é refeita do mesmo jeito: todo gravador passa por
//! aqui, então todo evento gravado atualiza o índice.
//!
//! Num worktree, a spec continua sendo a do checkout principal: o arquivo mora
//! fora do git, na pasta do Mustard do checkout principal, e sobrevive à troca
//! de branch.
//!
//! Os tipos, as conferências e a leitura por bloco moram em
//! `domain::spec_events`; aqui ficam o disco, a trava e o relógio.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::domain::citation::{self, Finding};
use crate::domain::spec_events::{self as model, Refusal, SpecLog};
use crate::io::citation::DiskWorld;
use crate::io::claude_paths::ClaudePaths;
use crate::io::fs::lock::{read_shared, LockedFile};
use crate::io::workspace;
use crate::platform::error::Error;

/// A raiz do projeto em que as specs moram, vista de `start`: o checkout
/// principal quando `start` está num worktree, e a âncora do Mustard a partir
/// daí. Sem âncora, o próprio ponto de partida.
#[must_use]
pub fn spec_root(start: &Path) -> PathBuf {
    // Um `.` só sobe pelas pastas de cima depois de virar caminho absoluto.
    let start = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
    let base = workspace::linked_worktree_main(&start).unwrap_or_else(|| start.clone());
    workspace::workspace_root_or_self(&base)
}

/// O `spec.ndjson` da spec `name` no projeto `root`.
pub fn spec_file(root: &Path, name: &str) -> Result<PathBuf, Refusal> {
    let paths = ClaudePaths::for_project(root).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    paths
        .for_spec(name.trim())
        .map(|spec| spec.spec_ndjson_path())
        .map_err(|_| Refusal::BadSpecName { spec: name.to_string() })
}

/// O que uma gravação deixou no arquivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// O número do evento gravado.
    pub id: u64,
    /// O código do item, `MSTD-<sigla>-<NNNN>`.
    pub code: Option<String>,
    /// Os números que um `remove` tirou da leitura.
    pub removed: Vec<u64>,
    /// Os números cujo texto um `purge` tirou do arquivo.
    pub purged: Vec<u64>,
    /// Por que a linha da spec no índice não foi refeita, quando não foi. O
    /// evento já está gravado; o `index` refaz o índice.
    pub index_warning: Option<Refusal>,
    /// Os avisos da conferência dos nomes citados nos fatos de um ponto, com o
    /// número do fato: o nome que o mapa acha em outro arquivo, o que ele não
    /// conhece e, uma vez só, a falta do mapa. O ponto já está gravado.
    pub citation_warnings: Vec<(usize, Finding)>,
}

/// Grava um evento com a hora de agora. Veja [`write_at_then`].
pub fn write(
    path: &Path,
    event_type: &str,
    draft: Map<String, Value>,
    cite_roots: &[PathBuf],
) -> Result<Written, Refusal> {
    write_at_then(path, event_type, draft, cite_roots, &now(), |_| {})
}

/// Grava um evento com a hora de agora e entrega a `then` o arquivo como
/// ficou, ainda com a trava presa. Veja [`write_at_then`].
pub fn write_then(
    path: &Path,
    event_type: &str,
    draft: Map<String, Value>,
    cite_roots: &[PathBuf],
    then: impl FnOnce(&SpecLog),
) -> Result<Written, Refusal> {
    write_at_then(path, event_type, draft, cite_roots, &now(), then)
}

/// Grava um evento com a hora `at`. Veja [`write_at_then`].
pub fn write_at(
    path: &Path,
    event_type: &str,
    draft: Map<String, Value>,
    cite_roots: &[PathBuf],
    at: &str,
) -> Result<Written, Refusal> {
    write_at_then(path, event_type, draft, cite_roots, at, |_| {})
}

/// Grava um evento do tipo `event_type` com os campos de `draft` e a hora
/// `at`, e entrega a `then` o arquivo como ficou.
///
/// O binário grava o número, a hora e o código do item (veja
/// [`model::code_after`]). Um código em `replaces` ou nos alvos de `remove` e
/// `purge` vira o número do evento que ele nomeia antes da gravação.
///
/// Recusa, sem tocar no arquivo: tipo desconhecido, campo obrigatório vazio,
/// código mandado por quem grava, fato de ponto sem fonte, arquivo citado que
/// não existe em nenhuma de `cite_roots`, número ou código apontado que não
/// existe, versão nova de outro tipo, filtro de remoção que não pega nada e
/// expurgo cujo trecho não aparece no alvo. O expurgo reescreve o arquivo com
/// o trecho dos alvos trocado por "…"; as outras gravações só acrescentam uma
/// linha. Aqui o expurgo só usa o trecho que o pedido indica em `excerpt`;
/// [`write_guarded`] recebe também a procura de segredo. Um nome de código citado num fato que
/// o mapa do projeto (o da última de `cite_roots`) não confirma só avisa, em
/// [`Written::citation_warnings`]. O `last` de uma gravação `copy` que aponta
/// além do último item do arquivo é trocado pelo último item, sem recusa.
///
/// Numa pasta de spec do projeto (`<raiz>/.claude/spec/<nome>/spec.ndjson`),
/// a linha da spec no índice é refeita logo depois da escrita, com a trava do
/// arquivo de eventos ainda presa; se ela não puder ser refeita, o evento
/// continua gravado, e [`Written::index_warning`] diz por quê.
///
/// `then` roda depois da escrita e antes de a trava soltar, com o conteúdo
/// que acabou de ser gravado, e não lê o disco: é onde se confere o arquivo
/// como ele ficou, sem que outra gravação entre no meio. Numa recusa, nem o
/// índice nem `then` são tocados.
pub fn write_at_then(
    path: &Path,
    event_type: &str,
    draft: Map<String, Value>,
    cite_roots: &[PathBuf],
    at: &str,
    then: impl FnOnce(&SpecLog),
) -> Result<Written, Refusal> {
    write_inner(path, event_type, draft, cite_roots, at, &|_| Vec::new(), |_, _| Ok(()), then)
}

/// Grava um evento com a hora de agora, depois de `guard` aceitar o arquivo
/// como ele ficaria, e entrega a `then` o arquivo como ficou. `guard` recebe o
/// arquivo antes e depois da gravação, com a trava presa, e a recusa dele
/// deixa o arquivo como estava. Num expurgo sem `excerpt`, os trechos de cada
/// alvo são os que `find` acha nos campos de texto dele. Veja
/// [`write_at_then`].
pub fn write_guarded(
    path: &Path,
    event_type: &str,
    draft: Map<String, Value>,
    cite_roots: &[PathBuf],
    find: &dyn Fn(&str) -> Vec<String>,
    guard: impl FnOnce(&SpecLog, &SpecLog) -> Result<(), Refusal>,
    then: impl FnOnce(&SpecLog),
) -> Result<Written, Refusal> {
    write_inner(path, event_type, draft, cite_roots, &now(), find, guard, then)
}

/// A gravação de [`write_at_then`], com a conferência de [`write_guarded`]
/// antes de escrever.
#[allow(clippy::too_many_arguments)]
fn write_inner(
    path: &Path,
    event_type: &str,
    draft: Map<String, Value>,
    cite_roots: &[PathBuf],
    at: &str,
    find: &dyn Fn(&str) -> Vec<String>,
    guard: impl FnOnce(&SpecLog, &SpecLog) -> Result<(), Refusal>,
    then: impl FnOnce(&SpecLog),
) -> Result<Written, Refusal> {
    let Prepared { mut event, asked, citation_warnings } = prepare(event_type, draft, cite_roots)?;

    let mut file = LockedFile::exclusive(path).map_err(io_refusal)?;
    let content = file.read_to_string().map_err(io_refusal)?;
    let log = model::parse_log(&content);
    // O `last` de uma cópia da página da spec nunca aponta além do que o
    // arquivo tem: um número maior, de uma pasta de cópia velha ou de um
    // pedido errado, é trocado pelo último item do arquivo, sem recusa —
    // senão a cópia seguinte pularia os itens até esse número para sempre.
    if event_type == "copy"
        && let Some(last) = event.get("last").and_then(Value::as_u64)
        && last > log.max_id()
    {
        event.insert("last".to_string(), Value::from(log.max_id()));
    }
    // O arquivo como ficaria, conferido antes de qualquer escrita.
    let Staged { next, appended, after, id, code, effects } = stage(&content, &log, event, asked, at, find)?;
    guard(&log, &after)?;
    match appended {
        Some(added) => file.append_line(&added).map_err(io_refusal)?,
        None => file.replace(next.as_bytes()).map_err(io_refusal)?,
    }
    let log = after;
    // Primeiro a trava da spec, depois a do índice: sempre nessa ordem. A
    // publicação da página do projeto leva o endereço para a linha do
    // projeto: ela é, por ter acabado de ser gravada, a última. A marca do
    // template vai junto, e a publicação sem ela é a da página antiga.
    let project_url = log.events.iter().rev().find(|e| e.id == id).and_then(|e| {
        crate::domain::spec_index::published_to(e, crate::domain::spec_index::PROJECT_PAGE)
            .map(|url| (url, crate::domain::spec_index::is_template(e)))
    });
    let index_warning = crate::io::spec_index::index_for(path).and_then(|(index, name)| {
        crate::io::spec_index::refresh_line(&index, &name, &log)
            .and_then(|()| {
                project_url.map_or(Ok(()), |(url, template)| {
                    crate::io::spec_index::set_project_url(&index, url, template)
                })
            })
            .err()
    });
    then(&log);
    drop(file);
    Ok(Written { id, code, removed: effects.removed, purged: effects.purged, index_warning, citation_warnings })
}

/// O evento conferido sozinho, antes de a trava ser pega.
struct Prepared {
    event: Map<String, Value>,
    /// O trecho que o pedido de expurgo indica.
    asked: Option<String>,
    citation_warnings: Vec<(usize, Finding)>,
}

/// Confere o evento sozinho: a forma dele e os arquivos que um ponto cita.
fn prepare(event_type: &str, draft: Map<String, Value>, cite_roots: &[PathBuf]) -> Result<Prepared, Refusal> {
    let mut event = model::normalize(draft, event_type);
    model::validate(&event)?;
    // O trecho que o pedido indica serve ao expurgo e nunca vai para o
    // arquivo: gravá-lo seria gravar de novo o que se quer tirar.
    let asked = event.remove("excerpt").and_then(|v| v.as_str().map(str::to_string));
    let citation_warnings = check_citations(cite_roots, &event)?;
    Ok(Prepared { event, asked, citation_warnings })
}

/// O arquivo como ficaria com o evento, ainda sem nada escrito.
struct Staged {
    /// O conteúdo inteiro depois da gravação.
    next: String,
    /// A linha acrescentada, quando a gravação só acrescenta; `None` no
    /// expurgo, que reescreve o arquivo.
    appended: Option<String>,
    after: SpecLog,
    id: u64,
    code: Option<String>,
    effects: model::Effects,
}

/// Monta, a partir do arquivo `content` já lido como `log`, o arquivo como
/// ficaria com o evento: o número, o código, os alvos conferidos e, num
/// expurgo, os trechos trocados.
fn stage(
    content: &str,
    log: &SpecLog,
    mut event: Map<String, Value>,
    asked: Option<String>,
    at: &str,
    find: &dyn Fn(&str) -> Vec<String>,
) -> Result<Staged, Refusal> {
    let id = log.max_id().saturating_add(1);
    model::resolve_codes(log, &mut event)?;
    model::carry_closed_identity(log, &mut event)?;
    let effects = model::check_against(log, &event, id)?;
    let code = model::code_after(log, &event);
    let line = model::render_line(&model::stamp(event, id, code.as_deref(), at));

    let redactions = if effects.purged.is_empty() {
        std::collections::BTreeMap::new()
    } else {
        model::purge_excerpts(log, &effects.purged, asked.as_deref(), find)?
    };
    let (next, appended) = if effects.purged.is_empty() {
        // Uma última linha pela metade fica sozinha na linha dela, e a
        // gravação começa numa linha nova.
        let clean = content.is_empty() || content.ends_with('\n');
        let added = if clean { line } else { format!("\n{line}") };
        (format!("{content}{added}\n"), Some(added))
    } else {
        let mut body = model::purge_lines(content, &redactions);
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&line);
        body.push('\n');
        (body, None)
    };
    let after = model::parse_log(&next);
    Ok(Staged { next, appended, after, id, code, effects })
}

/// Uma sequência de gravações conferida sem gravar nada: cada uma passa pela
/// mesma conferência de [`write_guarded`], sobre o arquivo como as anteriores
/// o deixariam. É como quem precisa fazer algo que não se desfaz antes de
/// gravar — o commit da rodada — sabe que a gravação depois dele não será
/// recusada.
pub struct DryRun<'a> {
    content: String,
    log: SpecLog,
    cite_roots: Vec<PathBuf>,
    find: &'a dyn Fn(&str) -> Vec<String>,
}

impl<'a> DryRun<'a> {
    /// Lê o arquivo `path` como ele está, com a trava compartilhada. A spec
    /// sem arquivo começa vazia, como a gravação a começaria.
    ///
    /// # Errors
    ///
    /// [`Refusal::Io`] quando o arquivo existe e não pode ser lido.
    pub fn open(
        path: &Path,
        cite_roots: Vec<PathBuf>,
        find: &'a dyn Fn(&str) -> Vec<String>,
    ) -> Result<Self, Refusal> {
        let content = match read_shared(path) {
            Ok(content) => content,
            Err(Error::NotFound(_)) => String::new(),
            Err(e) => return Err(io_refusal(e)),
        };
        let log = model::parse_log(&content);
        Ok(Self { content, log, cite_roots, find })
    }

    /// O arquivo como as gravações conferidas até aqui o deixariam.
    #[must_use]
    pub fn log(&self) -> &SpecLog {
        &self.log
    }

    /// Confere a gravação de um evento do tipo `event_type` com os campos de
    /// `draft`, como [`write_guarded`] a conferiria, e passa a contar com ela.
    ///
    /// # Errors
    ///
    /// A recusa que a gravação daria; nesse caso, nada muda.
    pub fn write(
        &mut self,
        event_type: &str,
        draft: Map<String, Value>,
        guard: impl FnOnce(&SpecLog, &SpecLog) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let Prepared { event, asked, .. } = prepare(event_type, draft, &self.cite_roots)?;
        let staged = stage(&self.content, &self.log, event, asked, &now(), self.find)?;
        guard(&self.log, &staged.after)?;
        self.content = staged.next;
        self.log = staged.after;
        Ok(())
    }
}

/// Lê o arquivo inteiro, com a trava compartilhada. `Ok(None)` quando a spec
/// ainda não tem arquivo.
pub fn read(path: &Path) -> Result<Option<SpecLog>, Refusal> {
    match read_shared(path) {
        Ok(content) => Ok(Some(model::parse_log(&content))),
        Err(Error::NotFound(_)) => Ok(None),
        Err(e) => Err(io_refusal(e)),
    }
}

/// Pega a trava exclusiva do arquivo, lê pelo mesmo manipulador e entrega o
/// arquivo lido a `f`, soltando a trava só depois. É como a cópia para o
/// banco da página, e a página do comando de página, saem sem gravar evento:
/// nenhuma gravação entra no meio, então nenhuma das duas fica atrás do
/// arquivo. `Ok(None)` quando a spec ainda não tem arquivo; nada é criado.
pub fn with_locked_log<R>(path: &Path, f: impl FnOnce(&SpecLog) -> R) -> Result<Option<R>, Refusal> {
    let mut file = match LockedFile::existing(path) {
        Ok(file) => file,
        Err(Error::NotFound(_)) => return Ok(None),
        Err(e) => return Err(io_refusal(e)),
    };
    let content = file.read_to_string().map_err(io_refusal)?;
    let out = f(&model::parse_log(&content));
    drop(file);
    Ok(Some(out))
}

/// As raízes das citações moram na conferência das citações.
pub use crate::io::citation::citation_roots;

/// Confere as fontes dos fatos de um ponto pela conferência única das
/// citações, a mesma que o plano chama: o arquivo citado é procurado em
/// `roots`, e os nomes, no mapa do projeto da última delas. O arquivo ou a
/// linha que não existe recusa o ponto. Os achados dos nomes voltam como
/// avisos, com o número do fato, e a falta do mapa, uma vez só.
fn check_citations(roots: &[PathBuf], event: &Map<String, Value>) -> Result<Vec<(usize, Finding)>, Refusal> {
    let mut warnings: Vec<(usize, Finding)> = Vec::new();
    if event.get("type").and_then(Value::as_str) != Some("point") {
        return Ok(warnings);
    }
    let world = DiskWorld::new(roots.to_vec(), roots.last().map(PathBuf::as_path));
    let facts = event.get("facts").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for (i, fact) in facts.iter().enumerate() {
        let Some(source) = fact.get("source").and_then(Value::as_str) else { continue };
        let text = fact.get("text").and_then(Value::as_str).unwrap_or_default();
        for finding in citation::check(&world, source, text) {
            if let Some(refusal) = finding.refusal(i + 1) {
                return Err(refusal);
            }
            if finding == Finding::NoMap && warnings.iter().any(|(_, seen)| *seen == Finding::NoMap) {
                continue;
            }
            warnings.push((i + 1, finding));
        }
    }
    Ok(warnings)
}

fn io_refusal(error: Error) -> Refusal {
    Refusal::Io { detail: error.to_string() }
}

/// Agora, na hora local com o fuso: `2026-09-11T21:03:12-03:00`.
pub(crate) fn now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{Block, BlockQuery, EventRef, Hidden, SkipReason, Step, TYPES};
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};

    fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    fn at(hm: &str) -> String {
        format!("2026-09-11T{hm}:00-03:00")
    }

    fn put(path: &Path, roots: &[PathBuf], event_type: &str, time: &str, draft: Value) -> Written {
        write_at(path, event_type, obj(draft), roots, time)
            .unwrap_or_else(|r| panic!("{event_type} was refused: {r:?}"))
    }

    fn ids_of(events: &[&model::SpecEvent]) -> Vec<u64> {
        events.iter().map(|e| e.id).collect()
    }

    /// A mensagem do usuário que abre o arquivo, o evento número 1: tudo o
    /// que o assistente grava aponta de onde veio, e a origem tem de ser um
    /// evento que já está no arquivo.
    fn seed_message(path: &Path) {
        put(path, &[], "message", &at("09:00"), json!({"author": "user", "text": "o pedido"}));
    }

    /// Uma spec de teste com os 36 tipos, em três ondas, com uma remoção por
    /// horário, um expurgo e um limite revisto.
    struct Spec {
        _dir: tempfile::TempDir,
        path: PathBuf,
        ids: BTreeMap<&'static str, u64>,
    }

    impl Spec {
        fn ids(&self, names: &[&str]) -> Vec<u64> {
            let mut out: Vec<u64> = names.iter().map(|n| self.ids[n]).collect();
            out.sort_unstable();
            out
        }

        fn log(&self) -> SpecLog {
            read(&self.path).unwrap().unwrap()
        }
    }

    fn every_type() -> Spec {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/render.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
        let path = root.join(".claude/spec/teste/spec.ndjson");
        let roots = vec![root];
        let mut ids = BTreeMap::new();
        let mut add = |name: &'static str, event_type: &str, hm: &str, draft: Value| {
            let id = put(&path, &roots, event_type, &at(hm), draft).id;
            ids.insert(name, id);
            id
        };
        add("state", "state", "08:40", json!({"author": "binary", "phase": "survey", "branch": "feature/teste", "base": "dev"}));
        let msg = add("message", "message", "08:41", json!({"author": "user", "text": "Absolutamente tudo precisa ser revisto"}));
        add("work_type", "work_type", "08:42", json!({"kinds": ["refactor"], "origin": msg}));
        add("context", "context", "08:43", json!({"text": "O Rust roda rápido.", "origin": msg}));
        add("concern", "concern", "08:44", json!({"text": "Testes prendem frases.", "origin": msg}));
        add("decision", "decision", "08:45", json!({"text": "A página sai só nos marcos.", "why": "Cada publicação gasta.", "keys": ["página"], "applies_to": {"files": ["**"]}, "origin": msg}));
        add("out_of_scope", "out_of_scope", "08:46", json!({"text": "Supabase.", "keys": ["servidor"], "applies_to": {"files": ["**"]}, "origin": msg}));
        add("edge_case", "edge_case", "08:47", json!({"text": "Duas sessões gravam juntas.", "expected": "A segunda espera a trava.", "keys": ["trava"], "waves": [1], "origin": msg}));
        let rule = add("rule", "rule", "08:48", json!({"text": "A trava confere o programa.", "example": "rm -rf pasta é barrado.", "keys": ["trava", "apagar"], "origin": msg}));
        add("contract", "contract", "08:49", json!({"text": "A barra tem duas linhas.", "example": "dev · teste", "keys": ["barra"], "waves": [2], "origin": msg}));
        add("error", "error", "08:50", json!({"text": "Título longo.", "message": "O título passa de 60.", "keys": ["título"], "origin": msg}));
        add("point", "point", "08:51", json!({"block": "limits", "gap": "tamanho do pedido", "from": "gap", "status": "open", "facts": [{"text": "Não há teto.", "source": "src/render.rs:2"}], "origin": msg}));
        let old_limit = add("limit_old", "limit", "08:52", json!({"text": "Tamanho do pedido.", "value": "400 linhas", "keys": ["pedido"], "origin": msg}));
        let c1 = add(
            "criterion_1",
            "criterion",
            "08:53",
            json!({"when": "a", "then": "b", "proof": "cargo test a", "form": "ubiquitous", "origin": msg}),
        );
        let c2 = add(
            "criterion_2",
            "criterion",
            "08:54",
            json!({"when": "c", "then": "d", "proof": "cargo test c", "form": "ubiquitous", "origin": msg}),
        );
        add("wave_1", "wave", "08:55", json!({"n": 1, "text": "Preparo.", "criteria": [c1], "done_when": "A suíte passa.", "origin": msg}));
        let task1 = add("task_1", "task", "08:56", json!({"wave": 1, "text": "Juntar o texto.", "files": [{"path": "src/render.rs"}], "origin": msg}));
        add("step", "step", "08:56", json!({"wave": 1, "item": task1, "text": "A tarefa 1 ficou pronta."}));
        add("delivered_1", "delivered", "08:57", json!({"author": "wave", "wave": 1, "text": "Texto junto.", "files": ["src/render.rs"]}));
        add("wave_2", "wave", "08:58", json!({"n": 2, "text": "A aprovação lê o estado.", "criteria": [c2], "done_when": "A trava passa.", "depends_on": [1], "origin": msg}));
        add("task_2", "task", "08:59", json!({"wave": 2, "text": "O portão lê a aprovação.", "files": [{"path": "src/gate.rs", "new": true}], "skill": "add-hook-rule", "covers": [rule], "origin": msg}));
        add("skill", "skill", "09:00", json!({"name": "add-hook-rule", "action": "create", "text": "Passos da regra.", "sha": "3f9a1c2e", "examples": [{"path": "src/render.rs", "why": "mesma pasta"}, {"path": "src/gate.rs", "why": "com teste"}], "origin": msg}));
        add("send", "send", "09:01", json!({"author": "binary", "wave": 2, "role": "wave", "text": "# teste — onda 2", "lines": 312, "chars": 21480, "items": [rule], "mustard": "0.2.0"}));
        add("delivered_2", "delivered", "09:02", json!({"author": "wave", "wave": 2, "text": "O portão lê a aprovação.", "files": ["src/gate.rs"]}));
        add("verdict", "verdict", "09:03", json!({"author": "review", "wave": 2, "result": "approved", "text": "Sem achados.", "criteria": [{"criterion": c2, "tests_rule": true}]}));
        add("tracking", "tracking", "09:03", json!({"author": "binary", "items": [{"item": rule, "verification": "A trava confere o programa.", "file": "src/gate.rs", "met": true}]}));
        add("commit", "commit", "09:04", json!({"author": "binary", "sha": "5e0c7a91", "title": "fix: a aprovação sai do estado", "waves": [2], "files": ["src/gate.rs"], "repo": "."}));
        add("criterion_run", "criterion_run", "09:05", json!({"author": "binary", "criterion": c2, "result": "pass", "exit": 0, "ms": 5990}));
        add("pr_summary", "pr_summary", "09:06", json!({"text": "O portão lê o estado.", "origin": msg}));
        add("request", "request", "09:07", json!({"text": "Incluir o Windows.", "keys": ["windows"], "effect": "adjust_waves", "origin": msg}));
        add("deferred", "deferred", "09:08", json!({"text": "Medir o antivírus.", "keys": ["antivírus"], "pending": 3, "origin": msg}));
        add("note", "note", "09:09", json!({"text": "O Clippy foi corrigido.", "keys": ["clippy"], "origin": msg}));
        add("injection", "injection", "09:10", json!({"author": "hook", "hook": "session_start", "chars": 2870, "text": "Spec teste, fase execução."}));
        add("publish", "publish", "09:11", json!({"page": "spec", "milestone": "approval", "ok": true, "url": "https://example.com/p"}));
        add("copy", "copy", "09:11", json!({"page": "spec", "last": 32}));
        add("call", "call", "09:12", json!({"author": "binary", "command": "round", "ms": 41, "result": "ok"}));
        add("hook", "hook", "09:13", json!({"author": "hook", "hook": "command_guard", "action": "block", "tool": "Bash", "reason": "rm -rf apaga trabalho."}));
        add("response", "response", "09:14", json!({"text": "Tirei a atualização dos projetos da Contoso.", "reply_to": msg}));
        add("approved", "state", "09:15", json!({"author": "binary", "phase": "approved", "witness": {"question": "Aprovar esta spec?", "answer": "Aprovar"}}));
        let secret = add("secret", "message", "21:04", json!({"author": "user", "text": "a senha é hunter2-segredo"}));
        add("pasted", "message", "21:08", json!({"author": "user", "text": "colado por engano"}));
        add("later", "message", "21:12", json!({"author": "user", "text": "depois do intervalo"}));
        add("limit", "limit", "21:13", json!({"text": "Tamanho do pedido.", "value": "500 linhas", "keys": ["pedido"], "replaces": old_limit, "waves": [2], "origin": msg}));
        add("remove", "remove", "21:14", json!({"filter": {"type": "message", "from": "2026-09-11T21:03", "to": "2026-09-11T21:10"}, "reason": "Coladas por engano.", "origin": msg}));
        add("purge", "purge", "21:15", json!({"targets": [secret], "reason": "secret", "excerpt": "hunter2-segredo", "origin": msg}));
        Spec { _dir: dir, path, ids }
    }

    #[test]
    fn a_spec_with_every_type_is_written_and_read_block_by_block() {
        let spec = every_type();
        let log = spec.log();
        assert!(log.skipped.is_empty(), "{:?}", log.skipped);
        let written: BTreeSet<&str> = log.events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(written.len(), TYPES.len(), "every type is in the file: {written:?}");

        // Each event the reading shows sits in the block of its type and in no other.
        let visible: BTreeSet<u64> = log.visible().iter().map(|e| e.id).collect();
        let mut placed = BTreeSet::new();
        for block in Block::ALL.into_iter().filter(|b| *b != Block::Metrics) {
            for event in log.block(BlockQuery::Block(block)) {
                assert_eq!(event.block(), Some(block), "{}", event.shown());
                assert!(placed.insert(event.id), "event {} shows in two blocks", event.id);
            }
        }
        assert_eq!(placed, visible);
        for event in log.block(BlockQuery::Block(Block::Metrics)) {
            assert!(model::METRIC_TYPES.contains(&event.event_type.as_str()));
        }

        // A single wave brings only that wave.
        assert_eq!(
            ids_of(&log.block(BlockQuery::Wave(2))),
            spec.ids(&["wave_2", "task_2", "skill", "send", "delivered_2"])
        );
        assert_eq!(
            ids_of(&log.block(BlockQuery::Wave(1))),
            spec.ids(&["wave_1", "task_1", "step", "delivered_1"])
        );
        assert!(log.block(BlockQuery::Wave(3)).is_empty());
    }

    #[test]
    fn the_search_field_is_written_and_never_shown() {
        let spec = every_type();
        let raw = std::fs::read_to_string(&spec.path).unwrap();
        let rule_line = raw.lines().find(|l| l.contains("A trava confere o programa.")).unwrap();
        assert!(rule_line.contains(r#""search":"#), "{rule_line}");
        let log = spec.log();
        let rule = log.get(spec.ids["rule"]).unwrap();
        assert!(rule.matches(&model::search_terms("apagando"), None), "the stem of the key matches");
        assert!(!rule.shown().contains("search"));
    }

    #[test]
    fn each_step_reads_only_what_it_needs() {
        let spec = every_type();
        let log = spec.log();
        let got = |step: Step| ids_of(&log.step(&step));

        assert_eq!(got(Step::Resume), spec.ids(&["state", "publish", "copy", "approved"]));
        assert_eq!(
            got(Step::Close),
            spec.ids(&["state", "criterion_1", "criterion_2", "criterion_run", "publish", "copy", "approved"])
        );
        assert_eq!(
            got(Step::Review { wave: 2 }),
            spec.ids(&["criterion_2", "wave_2", "task_2", "skill", "send", "delivered_2"])
        );
        // O despacho traz a onda, os critérios dela, a especificação, os itens
        // combinados de que ela ou o projeto são donos e o que a onda de que
        // ela depende entregou — e nunca uma linha da conversa. Os itens são o
        // que a tarefa cobre, os que dizem a onda e os do projeto todo; o caso
        // de borda, que é da OUTRA onda, fica fora, e o erro, que não tem
        // dono, não vai para onda nenhuma.
        assert_eq!(
            got(Step::Dispatch { wave: 2 }),
            spec.ids(&[
                "context",
                "concern",
                "decision",
                "out_of_scope",
                "rule",
                "contract",
                "criterion_2",
                "delivered_1",
                "wave_2",
                "task_2",
                "skill",
                "send",
                "delivered_2",
                "limit",
            ])
        );
        assert!(!got(Step::Dispatch { wave: 2 }).contains(&spec.ids["edge_case"]));
        assert!(got(Step::Dispatch { wave: 1 }).contains(&spec.ids["edge_case"]));
        for wave in [1, 2] {
            assert!(!got(Step::Dispatch { wave }).contains(&spec.ids["error"]), "onda {wave}");
        }
        for event in log.step(&Step::Dispatch { wave: 2 }) {
            assert_ne!(event.block(), Some(Block::Conversation), "{}", event.shown());
        }
        assert_eq!(got(Step::Question { term: "Contoso".into() }), spec.ids(&["response"]));
    }

    #[test]
    fn two_writes_at_the_same_time_get_consecutive_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude").join("spec").join("s").join("spec.ndjson");
        let each = 20;
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let writers: Vec<_> = (0..2)
            .map(|w| {
                let path = path.clone();
                let start = std::sync::Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    (0..each)
                        .map(|i| {
                            let draft = obj(json!({"author": "user", "text": format!("gravação {w}-{i}")}));
                            write_at(&path, "message", draft, &[], &at("10:00")).unwrap().id
                        })
                        .collect::<Vec<u64>>()
                })
            })
            .collect();
        let mut ids: Vec<u64> = writers.into_iter().flat_map(|h| h.join().unwrap()).collect();
        ids.sort_unstable();
        assert_eq!(ids, (1..=2 * each).collect::<Vec<u64>>(), "no number repeats and none is skipped");

        let log = read(&path).unwrap().unwrap();
        assert!(log.skipped.is_empty(), "no line was torn: {:?}", log.skipped);
        assert_eq!(log.events.len(), 2 * each as usize);
        assert!(log.events.windows(2).all(|w| w[0].id + 1 == w[1].id), "the file is in number order");
    }

    #[test]
    fn a_broken_line_is_skipped_with_a_warning_and_the_next_write_starts_a_new_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        put(&path, &[], "message", &at("10:00"), json!({"author": "user", "text": "antes"}));
        // The machine went down in the middle of the second write.
        let mut torn = std::fs::read_to_string(&path).unwrap();
        torn.push_str(r#"{"v":1,"id":2,"at":"2026-09-11T10:01"#);
        std::fs::write(&path, &torn).unwrap();

        let after = put(&path, &[], "message", &at("10:02"), json!({"author": "user", "text": "depois"}));
        assert_eq!(after.id, 3, "the torn line's number is not reused");

        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw.lines().count(), 3, "the new event has a line of its own: {raw}");
        let log = read(&path).unwrap().unwrap();
        assert_eq!(log.events.iter().map(|e| e.id).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(log.skipped.len(), 1);
        assert_eq!((log.skipped[0].line, log.skipped[0].reason), (2, SkipReason::Unreadable));
        let warning = log.skipped[0].message(crate::platform::i18n::Locale::PtBr);
        assert!(warning.contains("linha 2") && warning.contains("pulada"), "{warning}");
    }

    #[test]
    fn unknown_type_and_empty_required_field_are_refused_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        seed_message(&path);
        put(&path, &[], "note", &at("10:00"), json!({"text": "t", "keys": ["k"], "origin": 1}));
        let before = std::fs::read(&path).unwrap();

        let unknown = write_at(&path, "licao", obj(json!({"text": "x"})), &[], &at("10:01"));
        assert_eq!(unknown.unwrap_err(), Refusal::UnknownType { found: "licao".into() });
        let empty = write_at(
            &path,
            "rule",
            obj(json!({"text": "t", "keys": ["k"], "example": "", "origin": 1})),
            &[],
            &at("10:02"),
        );
        assert_eq!(
            empty.unwrap_err(),
            Refusal::MissingField { event_type: "rule".into(), field: "example".into() }
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "a refusal never touches the file");
    }

    #[test]
    fn a_line_from_an_older_writer_is_read_and_numbering_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        // No version number and no keys: the writer of today would refuse it.
        std::fs::write(&path, "{\"id\":1,\"at\":\"2026-09-01T10:00:00-03:00\",\"type\":\"rule\",\"author\":\"assistant\",\"text\":\"antiga\"}\n").unwrap();
        let log = read(&path).unwrap().unwrap();
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.block(BlockQuery::Block(Block::Agreed)).len(), 1);
        assert_eq!(put(&path, &[], "message", &at("10:00"), json!({"author": "user", "text": "nova"})).id, 2);
    }

    #[test]
    fn after_a_hand_edit_the_next_number_follows_the_largest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        put(&path, &[], "message", &at("10:00"), json!({"author": "user", "text": "um"}));
        put(&path, &[], "message", &at("10:01"), json!({"author": "user", "text": "dois"}));
        let mut edited = std::fs::read_to_string(&path).unwrap();
        edited.push_str("{\"v\":1,\"id\":40,\"at\":\"2026-09-11T10:02:00-03:00\",\"type\":\"message\",\"author\":\"user\",\"text\":\"à mão\"}\n");
        std::fs::write(&path, edited).unwrap();
        assert_eq!(put(&path, &[], "message", &at("10:03"), json!({"author": "user", "text": "três"})).id, 41);
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn canon(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap()
    }

    #[test]
    fn a_write_from_a_linked_worktree_lands_in_the_main_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "seed"]);
        // The Mustard stays out of git, so the worktree gets none of it.
        std::fs::write(main.join("mustard.json"), "{}").unwrap();
        std::fs::create_dir_all(main.join(".claude")).unwrap();
        let worktree = dir.path().join("wt");
        git(&main, &["worktree", "add", "-q", "-b", "work", &worktree.to_string_lossy()]);
        assert!(!worktree.join("mustard.json").exists());

        let root = spec_root(&worktree);
        assert_eq!(canon(&root), canon(&main));
        let path = spec_file(&root, "s").unwrap();
        put(&path, &[], "message", &at("10:00"), json!({"author": "user", "text": "do worktree"}));
        assert!(main.join(".claude").join("spec").join("s").join("spec.ndjson").is_file());
        assert!(!worktree.join(".claude").exists(), "nothing is written in the worktree");
        assert_eq!(canon(&spec_root(&main)), canon(&main));
    }

    #[test]
    fn a_point_citing_a_missing_file_or_line_is_refused_and_a_real_citation_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/real.rs"), "a\nb\nc").unwrap();
        let path = root.join("spec.ndjson");
        let roots = vec![root];
        seed_message(&path);
        let before = std::fs::read(&path).unwrap();
        let point = |gap: &str, source: Value| {
            let mut fact = json!({"text": "o pedido não tem teto"});
            if !source.is_null() {
                fact["source"] = source;
            }
            obj(json!({"block": "limits", "gap": gap, "from": "gap", "status": "open", "facts": [fact], "origin": 1}))
        };
        let write = |draft| write_at(&path, "point", draft, &roots, &at("10:00"));

        assert_eq!(write(point("tamanho", Value::Null)).unwrap_err(), Refusal::FactWithoutSource { fact: 1 });
        assert_eq!(
            write(point("tamanho", json!("src/nao-existe.rs:10"))).unwrap_err(),
            Refusal::CitedFileMissing { fact: 1, path: "src/nao-existe.rs".into() }
        );
        assert_eq!(
            write(point("tamanho", json!("src/real.rs:9"))).unwrap_err(),
            Refusal::CitedLineMissing { fact: 1, path: "src/real.rs".into(), line: 9, lines: 3 }
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "the refusals wrote nothing");
        assert_eq!(write(point("tamanho", json!("src/real.rs:3"))).unwrap().id, 2);
        assert_eq!(write(point("prazo", json!("cargo test -p x → 12 passed"))).unwrap().id, 3);
    }

    /// Um ponto de um fato só, na lacuna `gap`, com a fonte e o texto dados.
    fn one_fact_point(gap: &str, source: Option<&str>, text: &str) -> Map<String, Value> {
        let mut fact = json!({"text": text});
        if let Some(source) = source {
            fact["source"] = json!(source);
        }
        obj(json!({"block": "limits", "gap": gap, "from": "gap", "status": "open", "facts": [fact], "origin": 1}))
    }

    /// Um fato sem fonte e outro que cita um arquivo que não existe são
    /// recusados, com a mensagem do que falta; o que cita um arquivo real com
    /// a linha entra. A conferência que o plano chama acha o mesmo nas mesmas
    /// fontes.
    #[test]
    fn a_point_without_source_or_citing_a_missing_file_is_refused_and_a_real_line_is_written_through_the_shared_check() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/real.rs"), "fn a() {}\nfn ler_linha() {}\nfn c() {}\n").unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            crate::io::project_map::model_path(&root),
            r#"{"modules":[{"path":"src/real.rs","declarations":[{"kind":"function","name":"ler_linha","line":2}]}]}"#,
        )
        .unwrap();
        let path = root.join("spec.ndjson");
        let roots = vec![root.clone()];
        seed_message(&path);
        let before = std::fs::read(&path).unwrap();
        let write =
            |source: Option<&str>, text: &str| write_at(&path, "point", one_fact_point("tamanho", source, text), &roots, &at("10:00"));
        let plan = |source: &str, text: &str| crate::io::citation::check_at(&roots, &root, source, text);

        let without = write(None, "o pedido não tem teto").unwrap_err();
        assert_eq!(without, Refusal::FactWithoutSource { fact: 1 });
        assert!(without.message(crate::platform::i18n::Locale::PtBr).contains("não tem fonte"));

        let missing = write(Some("src/nao-existe.rs:10"), "o pedido não tem teto").unwrap_err();
        assert_eq!(missing, Refusal::CitedFileMissing { fact: 1, path: "src/nao-existe.rs".into() });
        assert!(missing.message(crate::platform::i18n::Locale::PtBr).contains("src/nao-existe.rs"));
        assert_eq!(plan("src/nao-existe.rs:10", ""), vec![Finding::MissingFile { path: "src/nao-existe.rs".into() }]);
        assert_eq!(std::fs::read(&path).unwrap(), before, "the refusals wrote nothing");

        let real = write(Some("src/real.rs:2"), "quem lê é `ler_linha`").unwrap();
        assert_eq!(real.id, 2);
        assert_eq!(real.citation_warnings, Vec::new());
        assert_eq!(plan("src/real.rs:2", "quem lê é `ler_linha`"), Vec::new());
    }

    /// A gravação de um ponto recusa cada fonte pelo problema que a conferência
    /// única das citações acha nela, e passa a fonte em que ela não acha nada.
    #[test]
    fn the_point_write_refuses_what_the_shared_citation_check_finds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/real.rs"), "a\nb\nc\n").unwrap();
        let path = root.join("spec.ndjson");
        let roots = vec![root.clone()];
        let sources = [
            "src/real.rs:2",
            "src/nao-existe.rs:10",
            "src/real.rs:3-9",
            "src/real.rs:0",
            "src\\real.rs:3",
            "cargo test -p x → 12 passed",
            "354",
            "https://example.com:8080/src/real.rs:3",
            "",
        ];
        seed_message(&path);
        // Cada fonte entra na própria lacuna: dois pontos abertos com a mesma
        // lacuna são recusados, e aqui o assunto é a fonte.
        for (i, source) in sources.into_iter().enumerate() {
            let gap = format!("lacuna {i}");
            let shared: Vec<Finding> =
                crate::io::citation::check_at(&roots, &root, source, "").into_iter().filter(Finding::is_refusal).collect();
            let door = match write_at(&path, "point", one_fact_point(&gap, Some(source), "t"), &roots, &at("10:00")) {
                Ok(_) => Vec::new(),
                Err(Refusal::CitedFileMissing { path, .. }) => vec![Finding::MissingFile { path }],
                Err(Refusal::CitedLineMissing { path, line, lines, .. }) => vec![Finding::MissingLine { path, line, lines }],
                Err(Refusal::FactWithoutSource { .. }) if source.is_empty() => Vec::new(),
                Err(other) => panic!("{source:?} was refused for another reason: {other:?}"),
            };
            assert_eq!(door, shared, "the point write and the shared check differ on {source:?}");
        }
    }

    /// Sem o mapa do projeto, os nomes citados não são conferidos, o ponto
    /// entra, e um aviso só diz isso, por mais fatos que citem nomes.
    #[test]
    fn without_a_map_names_are_not_checked_and_one_warning_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/real.rs"), "a\nb\n").unwrap();
        let path = root.join("spec.ndjson");
        let roots = vec![root];
        let facts = json!([
            {"text": "o `SpecLog` guarda", "source": "src/real.rs:1"},
            {"text": "e o `check_citations` confere", "source": "cargo test → ok"},
            {"text": "sem nome nenhum", "source": "12"}
        ]);
        seed_message(&path);
        let draft = obj(json!({"block": "limits", "gap": "g", "from": "gap", "status": "open", "facts": facts, "origin": 1}));
        let written = write_at(&path, "point", draft, &roots, &at("10:00")).unwrap();
        assert_eq!(written.citation_warnings, vec![(1, Finding::NoMap)]);
        let plain =
            write_at(&path, "point", one_fact_point("tamanho", Some("src/real.rs:2"), "sem nome"), &roots, &at("10:01"));
        assert_eq!(plain.unwrap().citation_warnings, Vec::new(), "a point without names gets no warning");
    }

    /// Os itens removidos somem da leitura e continuam no arquivo com o
    /// motivo; o expurgo troca por "…", no arquivo, só o trecho que o pedido
    /// indica, e o item continua na leitura com o resto do texto e o código.
    /// O trecho nunca é gravado no próprio expurgo, e o expurgo cujo trecho
    /// não aparece no item é recusado sem tocar no arquivo.
    #[test]
    fn removed_items_leave_the_reading_and_stay_in_the_file_with_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        for i in 1..=11 {
            put(&path, &[], "message", &at("20:00"), json!({"author": "user", "text": format!("mensagem {i}")}));
        }
        let twelve = put(&path, &[], "note", &at("20:10"), json!({"text": "Nota doze", "keys": ["doze"], "origin": 1})).id;
        let thirteen = put(&path, &[], "note", &at("20:11"), json!({"text": "Nota treze", "keys": ["treze"], "origin": 1})).id;
        assert_eq!((twelve, thirteen), (12, 13));
        let secret = put(&path, &[], "message", &at("20:30"), json!({"author": "user", "text": "a senha é hunter2-segredo"})).id;
        let first = put(&path, &[], "message", "2026-09-11T21:03:05-03:00", json!({"author": "user", "text": "colada 1"})).id;
        let second = put(&path, &[], "message", "2026-09-11T21:10:45-03:00", json!({"author": "user", "text": "colada 2"})).id;
        let outside = put(&path, &[], "message", "2026-09-11T21:11:00-03:00", json!({"author": "user", "text": "fica"})).id;

        // By number.
        let by_number = put(&path, &[], "remove", &at("21:20"), json!({"targets": [12, 13], "reason": "Itens errados.", "origin": 1}));
        assert_eq!(by_number.removed, [12, 13]);
        // By type and time.
        let by_time = put(
            &path,
            &[],
            "remove",
            &at("21:21"),
            json!({"filter": {"type": "message", "from": "2026-09-11T21:03", "to": "2026-09-11T21:10"}, "reason": "Coladas por engano.", "origin": 1}),
        );
        assert_eq!(by_time.removed, [first, second]);
        // The purge: an excerpt that is not in the item is refused, and the
        // file stays byte for byte.
        let untouched = std::fs::read(&path).unwrap();
        let missing = write_at(
            &path,
            "purge",
            obj(json!({"targets": [secret], "reason": "secret", "excerpt": "outra-senha", "origin": 1})),
            &[],
            &at("21:22"),
        );
        assert_eq!(missing.unwrap_err(), Refusal::PurgeExcerptNotFound { code: "MSTD-MSG-0012".into() });
        let unnamed = write_at(&path, "purge", obj(json!({"targets": [secret], "reason": "secret"})), &[], &at("21:22"));
        assert_eq!(unnamed.unwrap_err().reason(), "purge-excerpt-not-found", "no finder, no excerpt");
        assert_eq!(std::fs::read(&path).unwrap(), untouched);
        let purge = put(&path, &[], "purge", &at("21:22"),
            json!({"targets": [secret], "reason": "secret", "excerpt": "hunter2-segredo", "origin": 1}));
        assert_eq!(purge.purged, [secret]);

        let log = read(&path).unwrap().unwrap();
        let shown: BTreeSet<u64> = log.visible().iter().map(|e| e.id).collect();
        for gone in [12, 13, first, second] {
            assert!(!shown.contains(&gone), "{gone} still shows");
        }
        assert!(shown.contains(&outside), "the message after the range stays");
        assert!(shown.contains(&secret), "the purged item stays in the reading");
        assert_eq!(log.get(secret).unwrap().str_field("text"), Some("a senha é …"));
        assert!(log.block(BlockQuery::Block(Block::Notes)).is_empty());
        assert_eq!(log.hidden()[&12], Hidden::Removed { by: by_number.id });
        assert_eq!(log.hidden()[&first], Hidden::Removed { by: by_time.id });
        assert!(!log.hidden().contains_key(&secret));

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("Nota doze") && raw.contains("colada 2"), "removed lines stay in the file");
        assert!(raw.contains("Itens errados.") && raw.contains("Coladas por engano."), "with the reason");
        assert!(!raw.contains("hunter2"), "the purge takes the excerpt out of the file, and never writes it itself");
        let purged_line = raw.lines().find(|l| l.contains(&format!("\"id\":{secret},"))).unwrap();
        assert!(purged_line.contains("\"code\":\"MSTD-MSG-0012\""), "the purge keeps the code: {purged_line}");
        assert!(purged_line.contains("a senha é …"), "{purged_line}");
        assert!(!purged_line.contains("\"purged\""), "{purged_line}");
        assert!(log.skipped.is_empty(), "the rewrite leaves no broken line");

        // A filter that catches nothing and an unknown number write nothing.
        let before = std::fs::read(&path).unwrap();
        let none = write_at(
            &path,
            "remove",
            obj(json!({"filter": {"type": "message", "from": "2026-09-10T08:00", "to": "2026-09-10T09:00"}, "reason": "r"})),
            &[],
            &at("21:30"),
        );
        assert!(matches!(none.unwrap_err(), Refusal::FilterMatchesNothing { .. }));
        let unknown = write_at(&path, "remove", obj(json!({"targets": [999], "reason": "r"})), &[], &at("21:31"));
        assert_eq!(unknown.unwrap_err(), Refusal::UnknownTarget { target: EventRef::Id(999) });
        let unknown_code =
            write_at(&path, "remove", obj(json!({"targets": ["MSTD-NOTE-0009"], "reason": "r"})), &[], &at("21:32"));
        assert_eq!(unknown_code.unwrap_err(), Refusal::UnknownTarget { target: EventRef::Code("MSTD-NOTE-0009".into()) });
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_replaced_item_shows_only_its_new_version_and_keeps_its_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        seed_message(&path);
        let old = put(&path, &[], "decision", &at("10:00"), json!({"text": "Publicar sempre.", "why": "w", "keys": ["página"], "origin": 1})).id;
        let new = put(&path, &[], "decision", &at("10:01"), json!({"text": "Publicar só nos marcos.", "why": "w", "keys": ["página"], "replaces": old, "origin": 1})).id;
        let log = read(&path).unwrap().unwrap();
        assert_eq!(ids_of(&log.block(BlockQuery::Block(Block::Agreed))), [new]);
        assert_eq!(log.current(old).map(|e| e.id), Some(new));
        assert_eq!(log.hidden()[&old], Hidden::Replaced { by: new });

        let other = write_at(&path, "note", obj(json!({"text": "t", "keys": ["k"], "replaces": new, "origin": 1})), &[], &at("10:02"));
        assert_eq!(
            other.unwrap_err(),
            Refusal::ReplacesOtherType { id: new, found: "decision".into(), event_type: "note".into() }
        );
        let missing = write_at(&path, "decision", obj(json!({"text": "t", "why": "w", "keys": ["k"], "replaces": 99, "origin": 1})), &[], &at("10:03"));
        assert_eq!(missing.unwrap_err(), Refusal::UnknownTarget { target: EventRef::Id(99) });
    }

    fn rule(text: &str) -> Value {
        json!({"text": text, "example": "e", "keys": ["k"], "origin": 1})
    }

    fn line_of(path: &Path, id: u64) -> String {
        let raw = std::fs::read_to_string(path).unwrap();
        raw.lines().find(|l| l.contains(&format!("\"id\":{id},"))).unwrap_or_default().to_string()
    }

    /// O código mora na linha: apagar à mão uma linha do meio não muda o
    /// código de nenhuma outra, e o número que saiu nunca volta. A versão nova
    /// grava o código da antiga. Uma linha sem código, posta à mão, recebe o
    /// próximo número livre e não o perde quando outra linha é gravada.
    #[test]
    fn the_code_is_written_in_the_line_and_a_hand_deleted_line_moves_no_other() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        seed_message(&path);
        let written: Vec<Written> =
            (1..=4).map(|i| put(&path, &[], "rule", &at("10:00"), rule(&format!("regra {i}")))).collect();
        for (i, w) in written.iter().enumerate() {
            let code = format!("MSTD-RULE-000{}", i + 1);
            assert_eq!(w.code.as_deref(), Some(code.as_str()));
            assert!(line_of(&path, w.id).contains(&format!("\"code\":\"{code}\"")), "{}", line_of(&path, w.id));
        }

        let raw = std::fs::read_to_string(&path).unwrap();
        let kept: String = raw.lines().filter(|l| !l.contains("regra 2")).flat_map(|l| [l, "\n"]).collect();
        std::fs::write(&path, kept).unwrap();
        let codes = read(&path).unwrap().unwrap().codes();
        let shown: Vec<&str> = [2, 4, 5].iter().map(|id| codes[id].as_str()).collect();
        assert_eq!(shown, ["MSTD-RULE-0001", "MSTD-RULE-0003", "MSTD-RULE-0004"]);

        let fifth = put(&path, &[], "rule", &at("10:01"), rule("regra 5"));
        assert_eq!(fifth.code.as_deref(), Some("MSTD-RULE-0005"), "the number that left never returns");

        let mut revised = rule("regra 3, revista");
        revised["replaces"] = json!(4);
        let revised = put(&path, &[], "rule", &at("10:02"), revised);
        assert_eq!(revised.code.as_deref(), Some("MSTD-RULE-0003"));
        assert!(line_of(&path, revised.id).contains("\"code\":\"MSTD-RULE-0003\""));

        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str("{\"v\":1,\"id\":50,\"at\":\"2026-09-11T10:03:00-03:00\",\"type\":\"rule\",\"author\":\"assistant\",\"text\":\"à mão\"}\n");
        std::fs::write(&path, raw).unwrap();
        assert_eq!(read(&path).unwrap().unwrap().codes()[&50], "MSTD-RULE-0006");
        let after = put(&path, &[], "rule", &at("10:04"), rule("regra 7"));
        assert_eq!(after.code.as_deref(), Some("MSTD-RULE-0007"));
        let codes = read(&path).unwrap().unwrap().codes();
        assert_eq!(codes[&50], "MSTD-RULE-0006", "the hand line keeps its number");
        assert_eq!(codes[&5], "MSTD-RULE-0004");
    }

    /// Um item se aponta pelo código que a página mostra: a remoção tira da
    /// leitura, a linha guarda o número do evento e o motivo fica no arquivo.
    #[test]
    fn an_item_is_removed_by_its_code() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        seed_message(&path);
        let ids: Vec<u64> = (1..=3).map(|i| put(&path, &[], "rule", &at("10:00"), rule(&format!("regra {i}"))).id).collect();
        let removal = put(&path, &[], "remove", &at("10:01"), json!({"targets": ["MSTD-RULE-0002"], "reason": "repetida"}));
        assert_eq!(removal.removed, [ids[1]]);
        assert!(line_of(&path, removal.id).contains(&format!("\"targets\":[{}]", ids[1])));
        let log = read(&path).unwrap().unwrap();
        assert_eq!(ids_of(&log.block(BlockQuery::Block(Block::Agreed))), [ids[0], ids[2]]);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("regra 2") && raw.contains("repetida"));
    }

    /// Quem recebe o arquivo depois da gravação recebe o que acabou de ser
    /// gravado, com o evento novo; numa recusa, não recebe nada.
    #[test]
    fn what_comes_after_a_write_sees_the_line_just_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        seed_message(&path);
        let mut seen = Vec::new();
        let written = write_at_then(&path, "rule", obj(rule("regra")), &[], &at("10:00"), |log| {
            seen = log.events.iter().map(|e| e.id).collect();
        })
        .unwrap();
        assert_eq!(seen, [1, written.id], "the line just written is there");
        let mut called = false;
        let refused =
            write_at_then(&path, "remove", obj(json!({"targets": [9], "reason": "r"})), &[], &at("10:01"), |_| {
                called = true;
            });
        assert!(refused.is_err() && !called);
        let locked = with_locked_log(&path, |log| log.events.len()).unwrap();
        assert_eq!(locked, Some(2));
        assert_eq!(with_locked_log(&dir.path().join("nada.ndjson"), |_| ()).unwrap(), None);
        assert!(!dir.path().join("nada.ndjson").exists());
    }

    #[test]
    fn a_spec_without_file_reads_as_none_and_a_bad_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = spec_file(dir.path(), "nada").unwrap();
        assert!(read(&path).unwrap().is_none());
        assert_eq!(spec_file(dir.path(), "../fora").unwrap_err(), Refusal::BadSpecName { spec: "../fora".into() });
    }

    /// O pedido de uma onda ou da revisão final, como a rodada e o fechamento
    /// o gravam: sem ele, a volta do agente não teria a que responder.
    fn put_send(path: &Path, wave: Option<u64>, time: &str) -> u64 {
        let mut draft = json!({"author": "binary", "role": "review", "text": "# pedido", "lines": 1, "chars": 8, "mustard": "0.2.3"});
        if let Some(n) = wave {
            draft["wave"] = json!(n);
            draft["role"] = json!("wave");
        }
        put(path, &[], "send", &at(time), draft).id
    }

    /// A entrega que a própria onda grava, com todos os campos da volta.
    fn wave_return(wave: u64, text: &str) -> Value {
        json!({
            "author": "wave",
            "wave": wave,
            "text": text,
            "files": ["src/a.rs"],
            "commit": "a onda saiu",
            "proofs": [{"criterion": "MSTD-CRIT-0001", "proof": "cargo test a"}],
            "fixes": [1],
            "leftovers": [{"title": "Comentário velho", "detail": "src/b.rs:3 cita um comando que saiu."}],
            "returned": true,
        })
    }

    /// A volta que a onda grava fica fora de toda leitura de entrega — as
    /// ondas entregues, a última entrega de cada onda, o bloco das ondas, o
    /// da onda, o painel e a versão vigente — até a rodada gravar a versão
    /// oficial com `replaces` para ela. O leitor de voltas devolve só a última
    /// de cada onda; depois da versão oficial, nenhuma volta anterior da onda
    /// espera, nem a que ela não apontou, e a gravada depois volta a esperar.
    /// A sobra sem título ou sem detalhe é recusada sem gravar nada, e a que
    /// traz os dois passa.
    #[test]
    fn a_volta_da_onda_fica_fora_da_leitura_ate_a_rodada_assumir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        seed_message(&path);
        let send = put_send(&path, Some(2), "10:00");

        let before = std::fs::read(&path).unwrap();
        let mut missing = wave_return(2, "Sobra pela metade.");
        missing["leftovers"] = json!([{"title": "  ", "detail": "d"}, {"title": "Outra sobra"}]);
        let refused = write_at(&path, "delivered", obj(missing), &[], &at("10:01")).unwrap_err();
        assert_eq!(
            refused,
            Refusal::MissingField { event_type: "delivered".into(), field: "leftovers[1].title, leftovers[2].detail".into() }
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "nada foi gravado");

        let first = put(&path, &[], "delivered", &at("10:02"), wave_return(2, "Primeira volta.")).id;
        assert!(line_of(&path, first).contains("\"returned\":true"), "a volta fica no arquivo");
        let log = read(&path).unwrap().unwrap();
        assert!(log.delivered_waves().is_empty(), "a volta não conta como onda entregue");
        assert!(log.last_by_wave("delivered").is_empty());
        assert!(log.block(BlockQuery::Block(Block::Waves)).iter().all(|e| e.id != first));
        assert_eq!(ids_of(&log.block(BlockQuery::Wave(2))), [send]);
        assert_eq!(ids_of(&log.block(BlockQuery::Block(Block::Metrics))), [send]);
        assert_eq!(log.current(first), None);
        assert_eq!(log.hidden().get(&first), Some(&Hidden::Returned));
        assert_eq!(ids_of(&log.unassumed_returns()), [first]);

        let second = put(&path, &[], "delivered", &at("10:03"), wave_return(2, "Segunda volta.")).id;
        let other = put(&path, &[], "delivered", &at("10:04"), wave_return(3, "Volta da onda 3.")).id;
        let log = read(&path).unwrap().unwrap();
        assert_eq!(ids_of(&log.unassumed_returns()), [second, other], "vale a última volta de cada onda");
        assert!(log.delivered_waves().is_empty());

        // A rodada assume a onda 2: a versão oficial, sem `returned`, aponta
        // a última volta.
        let official = put(
            &path,
            &[],
            "delivered",
            &at("10:05"),
            json!({"author": "binary", "wave": 2, "text": "Segunda volta.", "files": ["src/a.rs"], "commit": "a onda saiu", "replaces": second}),
        )
        .id;
        let log = read(&path).unwrap().unwrap();
        assert_eq!(log.delivered_waves(), BTreeSet::from([2]));
        assert_eq!(log.last_by_wave("delivered"), BTreeMap::from([(2, official)]));
        let shown: Vec<u64> =
            log.block(BlockQuery::Block(Block::Waves)).iter().filter(|e| e.event_type == "delivered").map(|e| e.id).collect();
        assert_eq!(shown, [official], "só a versão oficial aparece");
        assert_eq!(log.hidden()[&second], Hidden::Replaced { by: official });
        assert_eq!(log.hidden()[&first], Hidden::Returned, "a volta velha nunca volta à leitura");
        assert_eq!(ids_of(&log.unassumed_returns()), [other], "nenhuma volta da onda 2 espera mais");

        // O conserto da onda 2 volta depois da versão oficial e espera de novo.
        let fix = put(&path, &[], "delivered", &at("10:06"), wave_return(2, "Conserto.")).id;
        let log = read(&path).unwrap().unwrap();
        assert_eq!(ids_of(&log.unassumed_returns()), [other, fix]);
        assert_eq!(log.last_by_wave("delivered"), BTreeMap::from([(2, official)]), "a oficial segue a vigente");
    }

    /// O veredito final, que o revisor grava sem onda como volta, fica fora de
    /// toda leitura de veredito — o bloco da revisão, os vereditos por onda e
    /// a aprovação final da obra —, e o leitor de voltas o devolve ao lado da
    /// volta de uma onda, só o último. Depois que o fechamento grava o
    /// veredito oficial com `replaces` para ele, ele não espera mais, e a
    /// aprovação final é a oficial.
    #[test]
    fn o_veredito_final_sem_onda_e_lido_como_volta() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
        seed_message(&path);
        put_send(&path, Some(1), "10:00");
        put_send(&path, None, "10:01");
        let verdict = |text: &str| {
            json!({"author": "review", "final": true, "result": "approved", "text": text, "agreed": [{"item": 1, "met": true}], "returned": true})
        };
        let first = put(&path, &[], "verdict", &at("10:02"), verdict("Primeira leitura.")).id;
        let delivery = put(&path, &[], "delivered", &at("10:03"), wave_return(1, "A onda 1 voltou.")).id;
        let last = put(&path, &[], "verdict", &at("10:04"), verdict("Sem achados.")).id;

        let log = read(&path).unwrap().unwrap();
        assert_eq!(log.get(last).and_then(model::SpecEvent::wave), None, "o veredito final vem sem onda");
        assert!(log.block(BlockQuery::Block(Block::Review)).is_empty());
        assert!(log.verdicts_by_wave().is_empty());
        assert!(log.last_rejected().is_empty());
        assert!(crate::domain::spec_state::final_approval(&log).is_none(), "a volta não aprova a obra");
        assert_eq!(log.hidden().get(&last), Some(&Hidden::Returned));
        assert_eq!(ids_of(&log.unassumed_returns()), [delivery, last]);

        // O fechamento assume o veredito final.
        let official = put(
            &path,
            &[],
            "verdict",
            &at("10:05"),
            json!({"author": "review", "final": true, "result": "approved", "text": "Sem achados.", "agreed": [{"item": 1, "met": true}], "replaces": last}),
        )
        .id;
        let log = read(&path).unwrap().unwrap();
        assert_eq!(ids_of(&log.block(BlockQuery::Block(Block::Review))), [official]);
        assert_eq!(crate::domain::spec_state::final_approval(&log).map(|e| e.id), Some(official));
        assert_eq!(log.hidden()[&first], Hidden::Returned);
        assert_eq!(ids_of(&log.unassumed_returns()), [delivery], "a entrega da onda 1 segue à espera");
    }
}
