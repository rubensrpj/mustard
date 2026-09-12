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
//! Num worktree, a spec continua sendo a do checkout principal: o arquivo mora
//! fora do git, na pasta do Mustard do checkout principal, e sobrevive à troca
//! de branch.
//!
//! Os tipos, as conferências e a leitura por bloco moram em
//! `domain::spec_events`; aqui ficam o disco, a trava e o relógio.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::domain::spec_events::{self as model, Refusal, SpecLog};
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
    /// Os números que um `remove` tirou da leitura.
    pub removed: Vec<u64>,
    /// Os números cujo texto um `purge` tirou do arquivo.
    pub purged: Vec<u64>,
}

/// Grava um evento com a hora de agora. Veja [`write_at`].
pub fn write(
    path: &Path,
    event_type: &str,
    draft: Map<String, Value>,
    cite_roots: &[PathBuf],
) -> Result<Written, Refusal> {
    write_at(path, event_type, draft, cite_roots, &now())
}

/// Grava um evento do tipo `event_type` com os campos de `draft` e a hora
/// `at`.
///
/// Recusa, sem tocar no arquivo: tipo desconhecido, campo obrigatório vazio,
/// fato de ponto sem fonte, arquivo citado que não existe em nenhuma de
/// `cite_roots`, número apontado que não existe, versão nova de outro tipo e
/// filtro de remoção que não pega nada. O expurgo reescreve o arquivo com o
/// texto dos alvos tirado; as outras gravações só acrescentam uma linha.
pub fn write_at(
    path: &Path,
    event_type: &str,
    draft: Map<String, Value>,
    cite_roots: &[PathBuf],
    at: &str,
) -> Result<Written, Refusal> {
    let event = model::normalize(draft, event_type);
    model::validate(&event)?;
    check_citations(cite_roots, &event)?;

    let mut file = LockedFile::exclusive(path).map_err(io_refusal)?;
    let content = file.read_to_string().map_err(io_refusal)?;
    let log = model::parse_log(&content);
    let id = log.max_id().saturating_add(1);
    let effects = model::check_against(&log, &event, id)?;
    let line = model::render_line(&model::stamp(event, id, at));

    let wrote = if effects.purged.is_empty() {
        // Uma última linha pela metade fica sozinha na linha dela, e a
        // gravação começa numa linha nova.
        let clean = content.is_empty() || content.ends_with('\n');
        file.append_line(&if clean { line } else { format!("\n{line}") })
    } else {
        let mut body = model::purge_lines(&content, &effects.purged, id);
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&line);
        body.push('\n');
        file.replace(body.as_bytes())
    };
    wrote.map_err(io_refusal)?;
    Ok(Written { id, removed: effects.removed, purged: effects.purged })
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

/// O que está errado numa citação de arquivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CitationProblem {
    /// O arquivo não existe em nenhuma das raízes.
    MissingFile { path: String },
    /// O arquivo existe, e a linha citada passa do fim dele.
    MissingLine { path: String, line: u64, lines: u64 },
}

/// Confere uma fonte que cita arquivo e linha. O caminho é procurado em cada
/// raiz, em ordem, e a primeira em que ele existe decide a linha. `None`
/// quando a citação confere ou quando a fonte não cita arquivo.
#[must_use]
pub fn citation_problem(roots: &[PathBuf], source: &str) -> Option<CitationProblem> {
    let (path, line) = model::file_citation(source)?;
    let Some(bytes) = roots.iter().find_map(|root| crate::io::fs::read(root.join(&path)).ok()) else {
        return Some(CitationProblem::MissingFile { path });
    };
    let lines = count_lines(&bytes);
    if line == 0 || line > lines {
        Some(CitationProblem::MissingLine { path, line, lines })
    } else {
        None
    }
}

/// As raízes em que uma citação é procurada, a partir de onde o comando roda:
/// a própria pasta, cada pasta acima dela e, por último, a raiz das specs.
/// Assim a citação confere de uma subpasta, de um submódulo e de um worktree.
#[must_use]
pub fn citation_roots(start: &Path, spec_root: &Path) -> Vec<PathBuf> {
    let start = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
    let mut roots: Vec<PathBuf> = start.ancestors().map(Path::to_path_buf).collect();
    roots.push(spec_root.to_path_buf());
    roots
}

/// Quantas linhas o arquivo tem: a última conta mesmo sem `\n` no fim.
fn count_lines(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        return 0;
    }
    let pieces = bytes.split(|b| *b == b'\n').count() as u64;
    if bytes.ends_with(b"\n") { pieces - 1 } else { pieces }
}

fn check_citations(roots: &[PathBuf], event: &Map<String, Value>) -> Result<(), Refusal> {
    if event.get("type").and_then(Value::as_str) != Some("point") {
        return Ok(());
    }
    let facts = event.get("facts").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for (i, fact) in facts.iter().enumerate() {
        let Some(source) = fact.get("source").and_then(Value::as_str) else { continue };
        match citation_problem(roots, source) {
            Some(CitationProblem::MissingFile { path }) => {
                return Err(Refusal::CitedFileMissing { fact: i + 1, path });
            }
            Some(CitationProblem::MissingLine { path, line, lines }) => {
                return Err(Refusal::CitedLineMissing { fact: i + 1, path, line, lines });
            }
            None => {}
        }
    }
    Ok(())
}

fn io_refusal(error: Error) -> Refusal {
    Refusal::Io { detail: error.to_string() }
}

/// Agora, na hora local com o fuso: `2026-09-11T21:03:12-03:00`.
fn now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{Block, BlockQuery, Hidden, SkipReason, Step, TYPES};
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

    /// Uma spec de teste com os 33 tipos, em três ondas, com uma remoção por
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
        add("decision", "decision", "08:45", json!({"text": "A página sai só nos marcos.", "why": "Cada publicação gasta.", "keys": ["página"], "origin": msg}));
        add("out_of_scope", "out_of_scope", "08:46", json!({"text": "Supabase.", "keys": ["servidor"], "origin": msg}));
        add("edge_case", "edge_case", "08:47", json!({"text": "Duas sessões gravam juntas.", "expected": "A segunda espera a trava.", "keys": ["trava"], "origin": msg}));
        let rule = add("rule", "rule", "08:48", json!({"text": "A trava confere o programa.", "example": "rm -rf pasta é barrado.", "keys": ["trava", "apagar"], "origin": msg}));
        add("contract", "contract", "08:49", json!({"text": "A barra tem duas linhas.", "example": "dev · teste", "keys": ["barra"], "origin": msg}));
        add("error", "error", "08:50", json!({"text": "Título longo.", "message": "O título passa de 60.", "keys": ["título"], "origin": msg}));
        add("point", "point", "08:51", json!({"block": "limits", "gap": "tamanho do pedido", "from": "gap", "status": "open", "facts": [{"text": "Não há teto.", "source": "src/render.rs:2"}], "origin": msg}));
        let old_limit = add("limit_old", "limit", "08:52", json!({"text": "Tamanho do pedido.", "value": "400 linhas", "keys": ["pedido"], "origin": msg}));
        let c1 = add("criterion_1", "criterion", "08:53", json!({"when": "a", "then": "b", "proof": "cargo test a", "origin": msg}));
        let c2 = add("criterion_2", "criterion", "08:54", json!({"when": "c", "then": "d", "proof": "cargo test c", "origin": msg}));
        add("wave_1", "wave", "08:55", json!({"n": 1, "text": "Preparo.", "criteria": [c1], "done_when": "A suíte passa.", "origin": msg}));
        add("task_1", "task", "08:56", json!({"wave": 1, "text": "Juntar o texto.", "files": [{"path": "src/render.rs"}], "origin": msg}));
        add("delivered_1", "delivered", "08:57", json!({"author": "wave", "wave": 1, "text": "Texto junto.", "files": ["src/render.rs"]}));
        add("wave_2", "wave", "08:58", json!({"n": 2, "text": "A aprovação lê o estado.", "criteria": [c2], "done_when": "A trava passa.", "depends_on": [1], "origin": msg}));
        add("task_2", "task", "08:59", json!({"wave": 2, "text": "O portão lê a aprovação.", "files": [{"path": "src/gate.rs", "new": true}], "skill": "add-hook-rule", "covers": [rule], "origin": msg}));
        add("skill", "skill", "09:00", json!({"name": "add-hook-rule", "action": "create", "text": "Passos da regra.", "sha": "3f9a1c2e", "examples": [{"path": "src/render.rs", "why": "mesma pasta"}, {"path": "src/gate.rs", "why": "com teste"}], "origin": msg}));
        add("send", "send", "09:01", json!({"author": "binary", "wave": 2, "role": "wave", "lines": 312, "chars": 21480, "items": [rule], "mustard": "0.2.0"}));
        add("delivered_2", "delivered", "09:02", json!({"author": "wave", "wave": 2, "text": "O portão lê a aprovação.", "files": ["src/gate.rs"]}));
        add("verdict", "verdict", "09:03", json!({"author": "review", "wave": 2, "result": "approved", "text": "Sem achados.", "criteria": [{"criterion": c2, "tests_rule": true}]}));
        add("commit", "commit", "09:04", json!({"author": "binary", "sha": "5e0c7a91", "title": "fix: a aprovação sai do estado", "waves": [2], "files": ["src/gate.rs"], "repo": "."}));
        add("criterion_run", "criterion_run", "09:05", json!({"author": "binary", "criterion": c2, "result": "pass", "exit": 0, "ms": 5990}));
        add("pr_summary", "pr_summary", "09:06", json!({"text": "O portão lê o estado.", "origin": msg}));
        add("request", "request", "09:07", json!({"text": "Incluir o Windows.", "keys": ["windows"], "effect": "adjust_waves", "origin": msg}));
        add("deferred", "deferred", "09:08", json!({"text": "Medir o antivírus.", "keys": ["antivírus"], "pending": 3, "origin": msg}));
        add("note", "note", "09:09", json!({"text": "O Clippy foi corrigido.", "keys": ["clippy"], "origin": msg}));
        add("injection", "injection", "09:10", json!({"author": "hook", "hook": "session_start", "chars": 2870, "text": "Spec teste, fase execução."}));
        add("publish", "publish", "09:11", json!({"page": "spec", "milestone": "approval", "ok": true, "url": "https://example.com/p"}));
        add("call", "call", "09:12", json!({"author": "binary", "command": "round", "ms": 41, "result": "ok"}));
        add("hook", "hook", "09:13", json!({"author": "hook", "hook": "command_guard", "action": "block", "tool": "Bash", "reason": "rm -rf apaga trabalho."}));
        add("response", "response", "09:14", json!({"text": "Tirei a atualização dos projetos da Suzano.", "reply_to": msg}));
        add("approved", "state", "09:15", json!({"author": "binary", "phase": "approved", "witness": {"question": "Aprovar esta spec?", "answer": "Aprovar"}}));
        let secret = add("secret", "message", "21:04", json!({"author": "user", "text": "a senha é hunter2-segredo"}));
        add("pasted", "message", "21:08", json!({"author": "user", "text": "colado por engano"}));
        add("later", "message", "21:12", json!({"author": "user", "text": "depois do intervalo"}));
        add("limit", "limit", "21:13", json!({"text": "Tamanho do pedido.", "value": "500 linhas", "keys": ["pedido"], "replaces": old_limit, "origin": msg}));
        add("remove", "remove", "21:14", json!({"filter": {"type": "message", "from": "2026-09-11T21:03", "to": "2026-09-11T21:10"}, "reason": "Coladas por engano.", "origin": msg}));
        add("purge", "purge", "21:15", json!({"targets": [secret], "reason": "secret", "origin": msg}));
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
            spec.ids(&["wave_1", "task_1", "delivered_1"])
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
        assert!(rule.matches(&model::search_terms("apagando")), "the stem of the key matches");
        assert!(!rule.shown().contains("search"));
    }

    #[test]
    fn each_step_reads_only_what_it_needs() {
        let spec = every_type();
        let log = spec.log();
        let got = |step: Step| ids_of(&log.step(&step));

        assert_eq!(got(Step::Resume), spec.ids(&["state", "publish", "approved"]));
        assert_eq!(
            got(Step::Close),
            spec.ids(&["state", "criterion_1", "criterion_2", "criterion_run", "publish", "approved"])
        );
        assert_eq!(
            got(Step::Review { wave: 2 }),
            spec.ids(&["criterion_2", "wave_2", "task_2", "skill", "send", "delivered_2"])
        );
        // The dispatch brings the wave, its criteria, the specification with the
        // current limit, the agreed item its task covers and what the wave it
        // depends on delivered — and never a line of the conversation.
        assert_eq!(
            got(Step::Dispatch { wave: 2 }),
            spec.ids(&[
                "context",
                "concern",
                "rule",
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
        for event in log.step(&Step::Dispatch { wave: 2 }) {
            assert_ne!(event.block(), Some(Block::Conversation), "{}", event.shown());
        }
        assert_eq!(got(Step::Question { term: "Suzano".into() }), spec.ids(&["response"]));
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
        put(&path, &[], "note", &at("10:00"), json!({"text": "t", "keys": ["k"], "origin": 1}));
        let before = std::fs::read(&path).unwrap();

        let unknown = write_at(&path, "lesson", obj(json!({"text": "x"})), &[], &at("10:01"));
        assert_eq!(unknown.unwrap_err(), Refusal::UnknownType { found: "lesson".into() });
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
        let point = |source: Value| {
            let mut fact = json!({"text": "o pedido não tem teto"});
            if !source.is_null() {
                fact["source"] = source;
            }
            obj(json!({"block": "limits", "gap": "tamanho", "from": "gap", "status": "open", "facts": [fact], "origin": 1}))
        };
        let write = |draft| write_at(&path, "point", draft, &roots, &at("10:00"));

        assert_eq!(write(point(Value::Null)).unwrap_err(), Refusal::FactWithoutSource { fact: 1 });
        assert_eq!(
            write(point(json!("src/nao-existe.rs:10"))).unwrap_err(),
            Refusal::CitedFileMissing { fact: 1, path: "src/nao-existe.rs".into() }
        );
        assert_eq!(
            write(point(json!("src/real.rs:9"))).unwrap_err(),
            Refusal::CitedLineMissing { fact: 1, path: "src/real.rs".into(), line: 9, lines: 3 }
        );
        assert!(!path.exists(), "the refusals wrote nothing");
        assert_eq!(write(point(json!("src/real.rs:3"))).unwrap().id, 1);
        assert_eq!(write(point(json!("cargo test -p x → 12 passed"))).unwrap().id, 2);
    }

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
        // The purge.
        let purge = put(&path, &[], "purge", &at("21:22"), json!({"targets": [secret], "reason": "secret", "origin": 1}));
        assert_eq!(purge.purged, [secret]);

        let log = read(&path).unwrap().unwrap();
        let shown: BTreeSet<u64> = log.visible().iter().map(|e| e.id).collect();
        for gone in [12, 13, first, second, secret] {
            assert!(!shown.contains(&gone), "{gone} still shows");
        }
        assert!(shown.contains(&outside), "the message after the range stays");
        assert!(log.block(BlockQuery::Block(Block::Notes)).is_empty());
        assert_eq!(log.hidden()[&12], Hidden::Removed { by: by_number.id });
        assert_eq!(log.hidden()[&first], Hidden::Removed { by: by_time.id });
        assert_eq!(log.hidden()[&secret], Hidden::Purged { by: purge.id });

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("Nota doze") && raw.contains("colada 2"), "removed lines stay in the file");
        assert!(raw.contains("Itens errados.") && raw.contains("Coladas por engano."), "with the reason");
        assert!(!raw.contains("hunter2"), "the purge takes the text out of the file");
        let purged_line = raw.lines().find(|l| l.contains(&format!("\"id\":{secret},"))).unwrap();
        assert!(purged_line.contains(&format!("\"purged\":{}", purge.id)), "{purged_line}");
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
        assert_eq!(unknown.unwrap_err(), Refusal::UnknownTarget { id: 999 });
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_replaced_item_shows_only_its_new_version_and_keeps_its_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.ndjson");
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
        assert_eq!(missing.unwrap_err(), Refusal::UnknownTarget { id: 99 });
    }

    #[test]
    fn a_spec_without_file_reads_as_none_and_a_bad_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = spec_file(dir.path(), "nada").unwrap();
        assert!(read(&path).unwrap().is_none());
        assert_eq!(spec_file(dir.path(), "../fora").unwrap_err(), Refusal::BadSpecName { spec: "../fora".into() });
    }
}
