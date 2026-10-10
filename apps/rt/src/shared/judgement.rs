//! Provider-independent judgement port. Cache keys describe the actual
//! evidence/questions and provider revision; changing agents is not an input.
//! Per-key OS locks coordinate independent runtime processes and release on
//! process death. Failed/invalid answers are never negative cache entries.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mustard_core::domain::map_filter::FilterError;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::sha256::Sha256;
use serde_json::{Value, json};

use crate::shared::dag::Judgement;
use mustard_core::domain::map_filter::FilterUsage;
use mustard_core::domain::spec_events::SpecEvent;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) trait WaveJudge: Send + Sync + std::fmt::Debug {
    fn judge_backlog(&self, board: &Board) -> Result<Judged, FilterError>;
    fn judge_items(&self, board: &ItemsBoard) -> Result<ItemsJudged, FilterError>;
    fn held_by_budget(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoardTask {
    /// O número da tarefa na spec, que identifica as respostas dela.
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) text: String,
    /// A parte da tarefa que é do agente.
    pub(crate) agent: String,
    pub(crate) files: Vec<String>,
    /// Os títulos das tarefas de que ela depende.
    pub(crate) depends_on: Vec<String>,
    /// Explicit read locations and acceptance criteria are local affinity
    /// evidence. Shared reading alone is never a concurrent-write conflict.
    pub(crate) reads: Vec<String>,
    pub(crate) criteria: BTreeSet<u64>,
}

impl BoardTask {
    /// A tarefa `task` como o Jev a vê: o que ela diz de si, os arquivos que
    /// declara — cada item de `files`, como texto solto ou como `{"path": …}`,
    /// com barras normais — e os títulos das tarefas de que depende
    /// (`depends_on`).
    pub(crate) fn of(task: &SpecEvent, depends_on: Vec<String>) -> Self {
        let files: BTreeSet<String> = task
            .fields
            .get("files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|file| file.as_str().or_else(|| file.get("path").and_then(Value::as_str)))
            .map(|path| path.trim().replace('\\', "/"))
            .filter(|path| !path.is_empty())
            .collect();
        Self {
            id: task.id,
            title: task.str_field("title").unwrap_or_default().to_string(),
            text: task.str_field("text").unwrap_or_default().to_string(),
            agent: task.str_field("agent").unwrap_or_default().to_string(),
            files: files.into_iter().collect(),
            depends_on,
            reads: task.fields.get("must_read").and_then(Value::as_array).into_iter().flatten()
                .filter_map(Value::as_str).map(|path| path.split('#').next().unwrap_or(path).trim().replace('\\', "/"))
                .filter(|path| !path.is_empty()).collect::<BTreeSet<_>>().into_iter().collect(),
            criteria: task.ints("covers").into_iter().collect(),
        }
    }

    pub(crate) fn relation(&self, other: &Self) -> TaskRelation {
        let left_files: BTreeSet<String> = self.files.iter().cloned().collect();
        let right_files: BTreeSet<String> = other.files.iter().cloned().collect();
        let left_reads: BTreeSet<String> = self.reads.iter().cloned().collect();
        let right_reads: BTreeSet<String> = other.reads.iter().cloned().collect();
        TaskRelation {
            dependency: self.depends_on.contains(&other.title) || other.depends_on.contains(&self.title),
            flow: self.depends_on.contains(&other.title) || other.depends_on.contains(&self.title)
                || !self.criteria.is_disjoint(&other.criteria),
            shared_read: !left_reads.is_disjoint(&right_reads),
            read_write: crate::shared::dag::sets_cross(&left_files, &right_reads)
                || crate::shared::dag::sets_cross(&right_files, &left_reads),
            write_overlap: crate::shared::dag::sets_cross(&left_files, &right_files),
            neighborhood: self.files.iter().any(|left| other.files.iter().any(|right|
                Path::new(left).parent().filter(|p| !p.as_os_str().is_empty())
                    .is_some_and(|parent| Path::new(right).parent() == Some(parent)))),
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub(crate) struct TaskRelation {
    pub(crate) dependency: bool,
    pub(crate) flow: bool,
    pub(crate) shared_read: bool,
    pub(crate) read_write: bool,
    pub(crate) write_overlap: bool,
    pub(crate) neighborhood: bool,
}

impl TaskRelation {
    pub(crate) fn semantic_interference(self) -> bool {
        // Write overlap is already an exact reservation. Do not ask Jev to
        // rediscover it, and do not confuse common read context with a race.
        !self.write_overlap && (self.dependency || self.read_write || self.neighborhood)
    }
}

/// Uma onda em andamento, com as tarefas dela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoardWave {
    pub(crate) n: u64,
    pub(crate) tasks: Vec<BoardTask>,
}

/// O quadro de uma montagem: as ondas em andamento e o backlog pronto, na
/// ordem em que a rodada o lê.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Board {
    pub(crate) running: Vec<BoardWave>,
    pub(crate) backlog: Vec<BoardTask>,
}

/// O que o Jev julgou do backlog de um quadro, com o que a chamada gastou.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Judged {
    /// O julgamento de cada tarefa do backlog, pelo número dela.
    pub(crate) tasks: BTreeMap<u64, Judgement>,
    pub(crate) usage: FilterUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoardItem {
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) text: String,
}

impl Board {
    /// Inverted indices generate only explicit criterion/read/dependency pairs.
    /// This is local evidence, not a semantic judgement of every ready pair.
    pub(crate) fn affinities(&self) -> crate::shared::dag::Affinities<u64> {
        let mut criteria: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        let mut reads: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        let mut pairs = BTreeMap::new();
        let titles: BTreeMap<&str, u64> = self.backlog.iter().map(|t| (t.title.as_str(), t.id)).collect();
        for task in &self.backlog {
            for criterion in &task.criteria { criteria.entry(*criterion).or_default().push(task.id); }
            for read in &task.reads { reads.entry(read).or_default().push(task.id); }
            for dependency in &task.depends_on {
                if let Some(&other) = titles.get(dependency.as_str()) { pairs.entry((task.id.min(other), task.id.max(other))).or_insert((false, false)).0 = true; }
            }
        }
        for (buckets, flow) in [(criteria.into_values().collect::<Vec<_>>(), true), (reads.into_values().collect(), false)] {
            for ids in buckets {
                for (at, &left) in ids.iter().enumerate() {
                    for &right in ids.iter().skip(at + 1) {
                        let flags = pairs.entry((left.min(right), left.max(right))).or_insert((false, false));
                        if flow { flags.0 = true; } else { flags.1 = true; }
                    }
                }
            }
        }
        pairs
    }
}

/// O quadro dos itens de uma onda: as tarefas dela e os itens a julgar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemsBoard {
    pub(crate) tasks: Vec<BoardTask>,
    pub(crate) items: Vec<BoardItem>,
}

/// O que o Jev julgou dos itens de uma onda, com o que a chamada gastou.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ItemsJudged {
    /// A chance de sim que o Jev deu a cada item, pelo número dele: o item
    /// governa algo que as tarefas da onda mudam ou testam.
    pub(crate) chances: BTreeMap<u64, f64>,
    pub(crate) usage: FilterUsage,
}

#[derive(Clone, Copy, Debug)]
pub enum Purpose {
    Search,
    WavePlanning,
    Context,
}

impl Purpose {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::WavePlanning => "wave-planning",
            Self::Context => "context",
        }
    }
}

pub struct JudgementRequest<'a> {
    pub purpose: Purpose,
    pub payload: &'a str,
}

pub trait JudgementProvider: Send + Sync {
    /// Includes endpoint, pinned model and adapter/rubric revision. No key.
    fn identity(&self) -> String;
    fn records_physical_attempts(&self) -> bool {
        false
    }
    fn evaluate(&self, request: &JudgementRequest<'_>, deadline: Instant) -> Result<Value, FilterError>;
}

pub struct JudgementService<'a> {
    provider: &'a dyn JudgementProvider,
    cache: Option<PathBuf>,
}

impl<'a> JudgementService<'a> {
    pub fn new(provider: &'a dyn JudgementProvider, cache: Option<&Path>) -> Self {
        Self { provider, cache: cache.map(Path::to_path_buf) }
    }

    pub(crate) fn key(&self, request: &JudgementRequest<'_>) -> String {
        let mut digest = Sha256::new();
        let (payload, _) = profile_aliases(request.payload);
        for part in ["judgement-v3", &self.provider.identity(), request.purpose.key(), &payload] {
            digest.update(part.as_bytes());
            digest.update(&[0]);
        }
        digest.hex_digest()
    }

    pub fn cached(&self, request: &JudgementRequest<'_>) -> Option<Value> {
        let key = self.key(request);
        let file = self.cache.as_ref()?.join(format!("{key}.json"));
        let mut doc: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
        let (_, aliases) = profile_aliases(request.payload);
        remap_answers(&mut doc, &aliases, true);
        validate(request, &doc).ok()?;
        doc["_mustard"] = json!({"cached":true, "request_id":key, "original_usage":doc.get("usage")});
        // Consumption belongs to the physical request. The reference above
        // preserves its origin; a second consumer never bills it again.
        doc["usage"] = json!({"input_tokens":0, "output_tokens":0});
        Some(doc)
    }

    pub fn evaluate(&self, request: &JudgementRequest<'_>, deadline: Instant) -> Result<Value, FilterError> {
        if let Some(doc) = self.cached(request) {
            return Ok(doc);
        }
        let key = self.key(request);
        let held = if let Some(cache) = &self.cache {
            std::fs::create_dir_all(cache).map_err(|_| FilterError::Unreadable("judgement cache unavailable".into()))?;
            let path = cache.join(format!("{key}.lock"));
            loop {
                if let Some(lock) = LockedFile::exclusive_if_free(&path).map_err(|_| FilterError::Unreadable("judgement lock unavailable".into()))? {
                    break Some(lock);
                }
                if let Some(doc) = self.cached(request) {
                    return Ok(doc);
                }
                if Instant::now() >= deadline {
                    return Err(FilterError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())));
            }
        } else {
            None
        };
        // Another process can finish between the first lookup and the lock.
        if let Some(doc) = self.cached(request) {
            return Ok(doc);
        }
        let started = Instant::now();
        let evaluated = self.provider.evaluate(request, deadline);
        self.record(request, &key, &evaluated, started.elapsed());
        let mut doc = evaluated?;
        validate(request, &doc)?;
        // A response cannot inject answers to a different profile into a
        // later combined board, even if the provider included surplus keys.
        let asked: Value = serde_json::from_str(request.payload).map_err(|_| FilterError::Unreadable("invalid request".into()))?;
        if let Some(answers) = doc.get_mut("answers").and_then(Value::as_object_mut) {
            answers.retain(|key, _| asked["questions"].get(key).is_some());
        }
        if let Some(cache) = &self.cache {
            let file = cache.join(format!("{key}.json"));
            // The lock protects writers/readers. Store only typed answers and
            // usage, never the original state or authentication material.
            let mut stored = json!({"answers":doc.get("answers"), "usage":doc.get("usage"), "model":doc.get("model")});
            let (_, aliases) = profile_aliases(request.payload);
            remap_answers(&mut stored, &aliases, false);
            if let Ok(bytes) = serde_json::to_vec(&stored) {
                let temp = cache.join(format!("{key}.tmp"));
                if std::fs::write(&temp, bytes).is_ok() {
                    let _ = std::fs::rename(temp, file);
                }
            }
        }
        doc["_mustard"] = json!({"cached":false, "request_id":key});
        drop(held);
        Ok(doc)
    }

    fn record(&self, request: &JudgementRequest<'_>, key: &str, result: &Result<Value, FilterError>, elapsed: Duration) {
        let Some(cache) = &self.cache else { return };
        let value = result.as_ref().ok();
        let valid = value.map(|doc| validate(request, doc));
        let invalid = valid.as_ref().is_some_and(Result::is_err);
        let entry = json!({"request_id":key,"purpose":request.purpose.key(),"physical_tracking":self.provider.records_physical_attempts(),
            "at":chrono::Utc::now().to_rfc3339(),"ms":elapsed.as_millis(),
            "result":if invalid {"invalid"}else if result.is_ok(){"received"}else{"failed"},
            "error":result.as_ref().err().or_else(||valid.as_ref().and_then(|v|v.as_ref().err())).map(FilterError::reason),
            "input_tokens":value.and_then(|v|v.pointer("/usage/input_tokens")).and_then(Value::as_u64),
            "output_tokens":value.and_then(|v|v.pointer("/usage/output_tokens")).and_then(Value::as_u64),
            "attempts":value.and_then(|v|v.get("_attempts")).and_then(Value::as_u64),
            "model":value.and_then(|v|v.get("model"))});
        if let Ok(mut log) = LockedFile::exclusive(&cache.join("requests.ndjson")) {
            let _ = log.append_line(&entry.to_string());
        }
    }
}

/// Only an intrinsic single-task profile has stable aliases. Running-wave
/// relations keep their actual IDs and complete changing state in the key.
fn profile_aliases(payload: &str) -> (String, BTreeMap<String, String>) {
    let Ok(mut value) = serde_json::from_str::<Value>(payload) else {
        return (payload.into(), BTreeMap::new());
    };
    let Some(backlog) = value.pointer("/state/backlog").and_then(Value::as_object) else {
        return (payload.into(), BTreeMap::new());
    };
    if backlog.len() != 1 || !value.pointer("/state/running").and_then(Value::as_object).is_some_and(|running| running.is_empty()) {
        return (payload.into(), BTreeMap::new());
    }
    let Some((old, task)) = backlog.iter().next().map(|(key, value)| (key.clone(), value.clone())) else {
        return (payload.into(), BTreeMap::new());
    };
    let names = [(format!("tipo_{old}"), "tipo_t0".to_string()), (format!("tam_{old}"), "tam_t0".to_string())];
    let Some(questions) = value["questions"].as_object() else {
        return (payload.into(), BTreeMap::new());
    };
    if questions.len() != 2
        || questions.get(&names[0].0).is_none_or(|q| q["type"] != "choice")
        || questions.get(&names[1].0).is_none_or(|q| q["type"] != "score")
    {
        return (payload.into(), BTreeMap::new());
    }
    let mut canonical = serde_json::Map::new();
    for (from, to) in &names {
        let mut question = questions[from].clone();
        if let Some(text) = question.pointer_mut("/instructions/question") {
            if let Some(original) = text.as_str() {
                *text = Value::String(original.replace(&format!("backlog.{old}"), "backlog.t0"));
            }
        } else if let Some(text) = question.get_mut("instructions")
            && let Some(original) = text.as_str() {
                *text = Value::String(original.replace(&format!("backlog.{old}"), "backlog.t0"));
            }
        canonical.insert(to.clone(), question);
    }
    value["questions"] = Value::Object(canonical);
    value["state"]["backlog"] = json!({"t0":task});
    (value.to_string(), names.into_iter().collect())
}
fn remap_answers(doc: &mut Value, aliases: &BTreeMap<String, String>, restore: bool) {
    let Some(answers) = doc.get_mut("answers").and_then(Value::as_object_mut) else {
        return;
    };
    for (original, canonical) in aliases {
        let (from, to) = if restore { (canonical, original) } else { (original, canonical) };
        if let Some(value) = answers.remove(from) {
            answers.insert(to.clone(), value);
        }
    }
}

fn validate(request: &JudgementRequest<'_>, doc: &Value) -> Result<(), FilterError> {
    let bad = || FilterError::Unreadable("missing or invalid typed judgement".into());
    let payload: Value = serde_json::from_str(request.payload).map_err(|_| bad())?;
    let questions = payload.get("questions").and_then(Value::as_object).ok_or_else(bad)?;
    let answers = doc.get("answers").and_then(Value::as_object).ok_or_else(bad)?;
    let probability = |value: &Value| value.as_f64().is_some_and(|n| n.is_finite() && (0.0..=1.0).contains(&n));
    for (id, question) in questions {
        let answer = answers.get(id).ok_or_else(bad)?;
        if answer.get("type").is_some_and(|kind| Some(kind) != question.get("type")) {
            return Err(bad());
        }
        match question.get("type").and_then(Value::as_str) {
            Some("noul") if answer.get("noul").is_some_and(probability) => {}
            Some("choice" | "score") => {
                let probabilities = answer.get("probabilities").and_then(Value::as_object).ok_or_else(bad)?;
                if probabilities.is_empty() || !probabilities.values().all(probability) {
                    return Err(bad());
                }
                if let Some(criteria) = question.get("criteria").and_then(Value::as_object)
                    && !criteria.keys().all(|key| probabilities.contains_key(key)) {
                        return Err(bad());
                    }
                // Confidence may be unavailable. Preserve that absence;
                // consumers requiring it take their explicit local fallback.
                if answer.get("confidence").is_some_and(|value| !probability(value)) {
                    return Err(bad());
                }
                if question["type"] == "choice" && !answer["choice"].as_str().is_some_and(|key| probabilities.contains_key(key)) {
                    return Err(bad());
                }
                if question["type"] == "score" {
                    let score = answer["score"].as_f64().filter(|score| score.is_finite()).ok_or_else(bad)?;
                    let mut scale = probabilities.keys().map(|key| key.parse::<f64>().map_err(|_| bad())).collect::<Result<Vec<_>, _>>()?;
                    if !scale.iter().all(|value| value.is_finite()) {
                        return Err(bad());
                    }
                    scale.sort_by(f64::total_cmp);
                    if scale.first().is_none_or(|min| score < *min) || scale.last().is_none_or(|max| score > *max) {
                        return Err(bad());
                    }
                }
            }
            _ => return Err(bad()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        calls: AtomicUsize,
    }
    impl JudgementProvider for Fake {
        fn identity(&self) -> String {
            "fake-v1".into()
        }
        fn evaluate(&self, _: &JudgementRequest<'_>, _: Instant) -> Result<Value, FilterError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(20));
            Ok(json!({"answers":{"q":{"noul":0.8}},"usage":{"input_tokens":120},"model":"fake"}))
        }
    }

    #[test]
    fn simultaneous_services_share_a_physical_request_and_changed_evidence_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake { calls: AtomicUsize::new(0) };
        let payload = r#"{"state":"old","questions":{"q":{"type":"noul","instructions":"Is the state relevant?"}}}"#;
        std::thread::scope(|scope| {
            let running: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        JudgementService::new(&fake, Some(dir.path()))
                            .evaluate(&JudgementRequest { purpose: Purpose::Search, payload }, Instant::now() + Duration::from_secs(3))
                            .unwrap()
                    })
                })
                .collect();
            let docs: Vec<_> = running.into_iter().map(|h| h.join().unwrap()).collect();
            assert_eq!(docs.iter().filter(|d| d["_mustard"]["cached"] == false).count(), 1);
            assert_eq!(docs.iter().filter_map(|d| d.pointer("/usage/input_tokens").and_then(Value::as_u64)).sum::<u64>(), 120);
        });
        let changed = payload.replace("old", "new");
        JudgementService::new(&fake, Some(dir.path()))
            .evaluate(&JudgementRequest { purpose: Purpose::Search, payload: &changed }, Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn invalid_answers_are_not_negative_cache_entries() {
        let payload = r#"{"questions":{"q":{"type":"noul"}}}"#;
        let request = JudgementRequest { purpose: Purpose::Search, payload };
        assert!(validate(&request, &json!({"answers":{"q":{"noul":2.0}}})).is_err());
        assert!(validate(&request, &json!({"answers":{}})).is_err());
    }

    struct ProcessFake {
        directory: PathBuf,
    }
    impl JudgementProvider for ProcessFake {
        fn identity(&self) -> String {
            "process-fake-v1".into()
        }
        fn evaluate(&self, _: &JudgementRequest<'_>, _: Instant) -> Result<Value, FilterError> {
            let mut count = LockedFile::exclusive(&self.directory.join("physical-count")).unwrap();
            count.append_line("called").unwrap();
            Ok(json!({"answers":{"q":{"noul":0.8},"unsolicited":{"noul":0.2}},"usage":{"input_tokens":120}}))
        }
    }

    #[test]
    fn cache_process_helper() {
        let Some(directory) = std::env::var_os("MUSTARD_JUDGEMENT_TEST_CHILD") else {
            return;
        };
        let directory = PathBuf::from(directory);
        let fake = ProcessFake { directory: directory.clone() };
        let request = JudgementRequest { purpose: Purpose::Search, payload: r#"{"state":"fixture","questions":{"q":{"type":"noul"}}}"# };
        let answer = JudgementService::new(&fake, Some(&directory)).evaluate(&request, Instant::now() + Duration::from_secs(10)).unwrap();
        assert!(answer["answers"].get("unsolicited").is_none());
    }

    #[test]
    fn independent_processes_reuse_one_physical_request() {
        let directory = tempfile::tempdir().unwrap();
        let mut children: Vec<_> = (0..4)
            .map(|_| {
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "shared::judgement::tests::cache_process_helper"])
                    .env("MUSTARD_JUDGEMENT_TEST_CHILD", directory.path())
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for child in &mut children {
            assert!(child.wait().unwrap().success());
        }
        assert_eq!(std::fs::read_to_string(directory.path().join("physical-count")).unwrap().lines().count(), 1);
        assert_eq!(std::fs::read_to_string(directory.path().join("requests.ndjson")).unwrap().lines().count(), 1);
    }
}
