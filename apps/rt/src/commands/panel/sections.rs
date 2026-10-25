//! The panel screens: one bounded answer per screen, built from the selected
//! spec's events and the round's own counts ([`super::progress`]). No other
//! spec's history is parsed, no wave delivery text is copied (the project
//! summary's own short lines are, cut), and nothing is written, measured or
//! requested over the network.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::domain::spec_state::State;
use mustard_core::domain::text::cut_line;
use mustard_core::view::document::{WaveState, WaveStates};
use serde_json::{Value, json};

use super::progress;

/// The screens this module answers.
pub(super) const SCREENS: &[&str] = &["overview", "spec", "items", "item", "usage", "waves"];

/// Item kinds the list screen pages through.
pub(super) const KINDS: &[&str] = &["task", "criterion", "decision", "rule", "question"];

/// Filters of the list screen.
pub(super) const FILTERS: &[&str] = &["all", "attention", "committed", "uncommitted"];

/// Rows per page when the caller does not say.
pub(super) const PAGE: usize = 8;

/// What the list and item screens were asked for.
#[derive(Debug, Default, Clone)]
pub(crate) struct Params {
    pub(crate) kind: Option<String>,
    pub(crate) query: Option<String>,
    pub(crate) filter: Option<String>,
    pub(crate) offset: Option<usize>,
    pub(crate) limit: Option<usize>,
    pub(crate) item: Option<String>,
}

/// One spec row of the project index, without parsing that spec's events.
pub(super) struct IndexRow {
    pub(super) name: String,
    pub(super) phase: Option<String>,
    pub(super) created: Option<String>,
    pub(super) updated: Option<String>,
    pub(super) branch: Option<String>,
}

/// The index rows, plus every spec folder the index does not know yet, plus
/// how many specs were discarded. A spec of before this version of Mustard is
/// in none of them.
pub(super) fn index_rows(root: &Path) -> (Vec<IndexRow>, usize) {
    let folder = root.join(".claude/spec");
    let legacy = mustard_core::io::legacy_specs::LegacySpecs::read(root);
    let index = std::fs::read_to_string(folder.join("index.ndjson")).unwrap_or_default();
    let text = |row: &Value, key: &str| row[key].as_str().map(str::to_string);
    let mut rows: Vec<IndexRow> = index.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(|row| row["type"] == "spec")
        .filter_map(|row| Some(IndexRow { name: text(&row, "name")?, phase: text(&row, "phase"), created: text(&row, "created"), updated: text(&row, "updated"), branch: text(&row, "branch") }))
        .collect();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') && entry.path().join("spec.ndjson").is_file() && !rows.iter().any(|row| row.name == name) {
                rows.push(IndexRow { name, phase: None, created: None, updated: None, branch: None });
            }
        }
    }
    rows.retain(|row| !legacy.contains(&row.name));
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    let discarded = std::fs::read_dir(folder.join(mustard_core::io::spec_index::DISCARDED_DIR))
        .map(|entries| entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| !legacy.contains(&entry.file_name().to_string_lossy())).count()).unwrap_or(0);
    (rows, discarded)
}

/// A merge of a spec branch into another branch, read from the local git
/// history: the pull request number when the subject names one, and the
/// commit date. No provider is asked.
#[derive(Clone)]
pub(super) struct Merge {
    pub(super) pr: Option<u64>,
    pub(super) at: String,
}

/// Every merge commit of the local history, by the branch it brought in.
/// The newest merge of each branch wins.
pub(super) fn merges(start: &Path) -> BTreeMap<String, Merge> {
    let out = mustard_core::platform::git::run(start, &["log", "--merges", "--all", "--format=%cI%x09%s"]).out().unwrap_or_default();
    let mut found = BTreeMap::new();
    for line in out.lines() {
        let Some((at, subject)) = line.split_once('\t') else { continue };
        let (pr, branch) = if let Some(rest) = subject.strip_prefix("Merge pull request #") {
            let (number, from) = rest.split_once(" from ").unwrap_or((rest, ""));
            let branch = from.split_whitespace().next().unwrap_or_default();
            // The head is `owner/branch`; the branch itself may hold slashes.
            (number.trim().parse::<u64>().ok(), branch.split_once('/').map_or(branch, |(_, branch)| branch).to_string())
        } else if let Some(rest) = subject.strip_prefix("Merge branch '") {
            (None, rest.split('\'').next().unwrap_or_default().to_string())
        } else {
            continue;
        };
        if !branch.is_empty() {
            found.entry(branch).or_insert(Merge { pr, at: at.to_string() });
        }
    }
    found
}

/// The merge of `branch` made after the spec opened at `created`.
fn merge_of(merges: &BTreeMap<String, Merge>, branch: Option<&str>, created: Option<&str>) -> Option<Merge> {
    let merge = merges.get(branch?)?;
    let after = created.is_none_or(|created| instant(&merge.at).zip(instant(created)).is_none_or(|(merged, opened)| merged >= opened));
    after.then(|| merge.clone())
}

fn instant(at: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(at.trim()).ok()
}

/// The phases in which a merged pull request means the spec was left behind.
const OPEN_PHASES: &[&str] = &["approved", "running", "closed", "pr_open"];

/// The pull request number recorded by the newest "pull request open".
fn recorded_pr(log: &SpecLog) -> Option<u64> {
    log.visible().into_iter().filter(|e| e.event_type == "state" && e.str_field("phase") == Some("pr_open"))
        .filter_map(|e| e.fields.get("pr").and_then(|pr| pr.get("number")).and_then(Value::as_u64)).next_back()
}

/// The spec's pull request: the recorded number, or the one the merge names,
/// and when it reached the base.
fn pull_request(log: &SpecLog, state: &State, merges: &BTreeMap<String, Merge>) -> Value {
    let merge = merge_of(merges, state.branch.as_deref(), log.events.first().map(SpecEvent::at));
    let number = recorded_pr(log).or_else(|| merge.as_ref().and_then(|merge| merge.pr));
    if number.is_none() && merge.is_none() {
        return Value::Null;
    }
    json!({"number":number,"merged_at":merge.map(|merge|merge.at),"source":"local-git"})
}

/// The overview: the selected spec in three numbers, what needs action, the
/// next step in the project's language `lang`, the waves in the air, other
/// specs that need attention and the project's measured usage. Bounded by
/// construction.
pub(super) fn overview(start: &Path, root: &Path, name: Option<&str>, log: Option<&SpecLog>, session: Option<&str>, lang: mustard_core::platform::i18n::Locale) -> Value {
    let merges = merges(start);
    let (rows, discarded) = index_rows(root);
    let discarded_listed = rows.iter().filter(|row| row.phase.as_deref() == Some("discarded")).count();
    let mut warnings = Vec::new();
    let spec = name.zip(log).map(|(name, log)| {
        let states = progress::wave_states(log);
        let state = State::from_log(log);
        let phase = state.phase.unwrap_or("survey");
        let waves = progress::waves(&states);
        if waves["no_agent"].as_array().is_some_and(|list| !list.is_empty()) {
            warnings.push(json!({"kind":"wave-no-agent","waves":waves["no_agent"]}));
        }
        let pr = pull_request(log, &state, &merges);
        if OPEN_PHASES.contains(&phase) && !pr["merged_at"].is_null() {
            warnings.push(json!({"kind":"pr-merged","pr":pr["number"],"at":pr["merged_at"],"phase":phase}));
        }
        let review = progress::review(log);
        if !review["reopened_at"].is_null() {
            warnings.push(json!({"kind":"review-reopened","approved_at":review["approved_at"],"reopened_at":review["reopened_at"]}));
        }
        // The slots and the queue as the round counts them, without the
        // queue's per-task reasons, which the overview does not show.
        let (capacity, free) = progress::slots(root, name, log);
        json!({"name":name,"goal":mustard_core::domain::spec_index::goal_of(log),"phase":phase,
            "tasks":progress::tasks(log,&states),"criteria":progress::criteria(log),
            "waves":{"finished":waves["finished"],"planned":waves["planned"]},
            "next":crate::commands::flow::resume::next_step(root,phase,name,&state,log,lang),
            "air":{"running":waves["running"],"no_agent":waves["no_agent"],"free_slots":free,
                "capacity":capacity,"backlog":crate::commands::flow::round::backlog_left(log).len()}})
    });
    let mut others: Vec<Value> = rows.iter().filter(|row| Some(row.name.as_str()) != name)
        .filter(|row| row.phase.as_deref().is_some_and(|phase| OPEN_PHASES.contains(&phase)))
        .filter_map(|row| merge_of(&merges, row.branch.as_deref(), row.created.as_deref())
            .map(|merge| json!({"name":row.name,"phase":row.phase,"updated":row.updated,"warning":{"kind":"pr-merged","pr":merge.pr,"at":merge.at}})))
        .collect();
    others.sort_by(|a, b| b["updated"].as_str().cmp(&a["updated"].as_str()));
    let attention = others.len();
    others.truncate(3);
    let costs = super::consumption::costs(root);
    let history = super::consumption::history(root, mustard_core::io::spend::machine_dir().as_deref());
    let live = super::live_usage::snapshot(root, session);
    let agents = live["agents"].as_object().map(|agents| agents.values().filter(|agent| agent["status"] == "running").count());
    json!({"spec":spec,"warnings":warnings,
        "others":{"total":rows.len()+discarded.saturating_sub(discarded_listed),"attention":attention,"shown":others},
        "deliveries":deliveries(root),
        "usage":{"cost_micro_usd":costs["known_micro_usd"],"sessions":costs["observed_sessions"],"tokens":history["project_tokens"],
            "measured_through":history["counted_through"],"session_agents":agents}})
}

/// How many deliveries of the project summary the overview shows.
const DELIVERIES_SHOWN: usize = 5;

/// How many characters of each delivery's text the overview carries.
const DELIVERY_CHARS: usize = 200;

/// The project summary for the overview: the newest deliveries first, a few,
/// each text cut, and how many the summary holds. Read from the summary file
/// alone; no spec's events are parsed.
fn deliveries(root: &Path) -> Value {
    use mustard_core::domain::project_summary::shown;
    let entries = mustard_core::io::project_summary::read(root);
    let rows: Vec<Value> = entries
        .iter()
        .take(DELIVERIES_SHOWN)
        .map(|entry| {
            let mut row = shown(entry);
            row["delivered"] = json!(cut_line(&entry.delivered, DELIVERY_CHARS));
            row["rules"] = json!(entry.rules.iter().map(|file| file.rules.len()).sum::<usize>());
            row
        })
        .collect();
    json!({"shown":rows,"total":entries.len()})
}

/// The spec screen: where it lives, what was agreed in six counters, waves by
/// state with the stopped ones named, the final review and the pull request,
/// and the other specs to switch to.
pub(super) fn spec(start: &Path, root: &Path, name: &str, log: &SpecLog) -> Value {
    let merges = merges(start);
    let states = progress::wave_states(log);
    let state = State::from_log(log);
    let visible = log.visible();
    let count = |kind: &str| visible.iter().filter(|e| e.event_type == kind).count();
    let commits: Vec<&&SpecEvent> = visible.iter().filter(|e| e.event_type == "commit").collect();
    let questions = visible.iter().filter(|e| e.event_type == "point").count();
    let open_questions = visible.iter().filter(|e| e.event_type == "point" && e.str_field("status") == Some("open")).count();
    let (rows, discarded) = index_rows(root);
    let specs: Vec<Value> = rows.iter().map(|row| json!({"name":row.name,"phase":row.phase,"created":row.created,"updated":row.updated})).collect();
    json!({"spec":{"name":name,"goal":mustard_core::domain::spec_index::goal_of(log),"phase":state.phase,
        "branch":state.branch,"base":state.base,"opened_at":log.events.first().map(SpecEvent::at),
        "last_commit":commits.last().map(|e|json!({"at":e.at(),"sha":e.str_field("sha")})),"commits":commits.len(),
        "counts":{"tasks":progress::tasks(log,&states),"criteria":progress::criteria(log),
            "questions":{"total":questions,"open":open_questions},"decisions":count("decision"),"rules":count("rule"),"waves":states.len()},
        "waves":progress::waves(&states),"review":progress::review(log),"pr":pull_request(log,&state,&merges)},
        "specs":specs,"discarded":discarded})
}

/// What one wave's tasks show in a list row.
#[derive(Default)]
struct WaveRow {
    state: Option<WaveState>,
    at: Option<String>,
    files: Option<usize>,
    unmet: usize,
    resends: usize,
    tokens: Option<u64>,
    /// The wave's own commit: the first one recorded for it. A repair commit
    /// recorded later for the wave does not take its place.
    commit: Option<String>,
    /// When the wave first went out, and when its accepted delivery came.
    first_sent: Option<String>,
    delivered: Option<String>,
}

/// One pass over the events for every wave's row: the commit or the return
/// that closed it, its files, the unmet agreed items, the resends, and when
/// it first went out and came back.
fn wave_rows(log: &SpecLog, states: &WaveStates) -> BTreeMap<u64, WaveRow> {
    let dispatches = log.last_dispatch_by_wave();
    let belongs = |n: u64, id: u64| dispatches.get(&n).is_none_or(|start| id > *start);
    let mut rows: BTreeMap<u64, WaveRow> = states.iter().map(|(n, state)| (*n, WaveRow { state: Some(*state), ..WaveRow::default() })).collect();
    let mut attempts: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for event in &log.events {
        if event.event_type == "send" && event.str_field("role").is_none_or(|role| role == "wave") && let Some(n) = event.wave() {
            attempts.entry(n).or_default().insert(log.dispatch_position(event.id));
            if let Some(row) = rows.get_mut(&n) { row.first_sent.get_or_insert_with(|| event.at().to_string()); }
        }
    }
    for event in log.visible() {
        match event.event_type.as_str() {
            // The newest return counts even before the newest send: the round
            // keeps a wave delivered when a send came after it with no
            // rejection in between.
            "delivered" => if let Some(n) = event.wave() && let Some(row) = rows.get_mut(&n) {
                row.files = event.fields.get("files").and_then(Value::as_array).map(Vec::len);
                row.unmet = event.fields.get("agreed").and_then(Value::as_array).map_or(0, |items| items.iter().filter(|item| item["met"] == false).count());
                row.delivered = Some(event.at().to_string());
                if matches!(row.state, Some(WaveState::Approved | WaveState::Rejected)) { row.at = Some(event.at().to_string()); }
            },
            "commit" => for n in event.ints("waves") {
                if let Some(row) = rows.get_mut(&n) && row.commit.is_none() {
                    row.commit = event.str_field("sha").map(str::to_string);
                }
                if belongs(n, event.id) && let Some(row) = rows.get_mut(&n) && row.state == Some(WaveState::Committed) {
                    row.at = Some(event.at().to_string());
                }
            },
            "send" => if let Some(n) = event.wave() && let Some(row) = rows.get_mut(&n) {
                row.tokens = event.int("tokens").or(row.tokens);
                if matches!(row.state, Some(WaveState::Running | WaveState::NoAgent)) { row.at = Some(event.at().to_string()); }
            },
            _ => {}
        }
    }
    for (n, sent) in attempts {
        if let Some(row) = rows.get_mut(&n) { row.resends = sent.len().saturating_sub(1); }
    }
    rows
}

/// The short title a list row shows: the title, the criterion's `when`, the
/// question's `gap`, or the first sentence of the text.
fn title(event: &SpecEvent) -> Option<String> {
    event.str_field("title").map(str::to_string)
        .or_else(|| (event.event_type == "criterion").then(|| event.str_field("when").map(str::to_string)).flatten())
        .or_else(|| (event.event_type == "point").then(|| event.str_field("gap").map(str::to_string)).flatten())
        .or_else(|| mustard_core::domain::spec_index::title_of(event))
}

/// The event type behind each list kind.
fn event_type(kind: &str) -> &str {
    if kind == "question" { "point" } else { kind }
}

/// Why a row asks for attention, if it does.
fn attention(event: &SpecEvent, wave: Option<&WaveRow>, qa: &mustard_core::domain::spec_state::Qa, proven: &BTreeSet<u64>) -> Option<&'static str> {
    match event.event_type.as_str() {
        "task" => wave_attention(wave.and_then(|row| row.state)),
        "criterion" if qa.failed_ids.contains(&event.id) => Some("criterion-failing"),
        "criterion" if !proven.contains(&event.id) => Some("criterion-without-proof"),
        "point" if event.str_field("status") == Some("open") => Some("question-open"),
        _ => None,
    }
}

/// Why a wave in `state` asks for attention: stopped with no agent, or
/// rejected.
fn wave_attention(state: Option<WaveState>) -> Option<&'static str> {
    match state {
        Some(WaveState::NoAgent) => Some("wave-no-agent"),
        Some(WaveState::Rejected) => Some("wave-rejected"),
        _ => None,
    }
}

/// What a wave carried: every task some version of which was in it, in the
/// order they entered, with the files of those versions, and how many of
/// them the current version took elsewhere.
#[derive(Default)]
struct Carried {
    tasks: Vec<String>,
    files: BTreeSet<String>,
    returned: usize,
}

/// [`Carried`] of every wave, in one pass over every version of every task.
/// A task whose item was removed is not counted.
fn carried(log: &SpecLog) -> BTreeMap<u64, Carried> {
    let codes = log.codes();
    let current: BTreeMap<&str, Option<u64>> = log.visible().into_iter().filter(|e| e.event_type == "task")
        .filter_map(|e| Some((codes.get(&e.id)?.as_str(), e.wave()))).collect();
    let mut out: BTreeMap<u64, Carried> = BTreeMap::new();
    for event in log.events.iter().filter(|e| e.event_type == "task") {
        let (Some(n), Some(code)) = (event.wave(), codes.get(&event.id)) else { continue };
        let Some(now) = current.get(code.as_str()) else { continue };
        let wave = out.entry(n).or_default();
        if !wave.tasks.contains(code) {
            wave.tasks.push(code.clone());
            wave.returned += usize::from(*now != Some(n));
        }
        let files = event.fields.get("files").and_then(Value::as_array).into_iter().flatten();
        wave.files.extend(files.filter_map(|file| file.get("path").unwrap_or(file).as_str().map(str::to_string)));
    }
    out
}

/// The waves screen: one page of the plan's waves, newest number first, each
/// with its title, its state, what it carried and delivered, its commit, the
/// time from the first send to the accepted delivery, and the model, the
/// effort and the tokens of the usage screen's own row. The total is the
/// plan's, the one the spec screen counts.
pub(super) fn waves(root: &Path, name: &str, log: &SpecLog, params: &Params) -> Value {
    let limit = params.limit.unwrap_or(PAGE).clamp(1, 50);
    let offset = params.offset.unwrap_or(0);
    let states = progress::wave_states(log);
    let rows = wave_rows(log, &states);
    let carried = carried(log);
    let live = super::wave_usage::read(root, name);
    let agents: BTreeMap<u64, Value> = agent_rows(log, &live).rows.into_iter().map(|(_, wave, row)| (wave, row)).collect();
    let codes = log.codes();
    let visible = log.visible();
    let page: Vec<Value> = states.iter().rev().skip(offset).take(limit).map(|(n, state)| {
        let row = rows.get(n);
        let carried = carried.get(n);
        // The first task's title, or the wave's own text when it carried none.
        let title = carried.and_then(|wave| wave.tasks.first())
            .and_then(|code| visible.iter().find(|e| e.event_type == "task" && codes.get(&e.id) == Some(code)))
            .or_else(|| visible.iter().rev().find(|e| e.event_type == "wave" && e.wave() == Some(*n)))
            .and_then(|event| title(event));
        let seconds = row.filter(|_| state.finished()).and_then(|row| instant(row.first_sent.as_deref()?).zip(instant(row.delivered.as_deref()?)))
            .map(|(sent, back)| (back - sent).num_seconds()).filter(|seconds| *seconds >= 0);
        let agent = agents.get(n);
        let spent = |field: &str| agent.map_or(Value::Null, |agent| agent[field].clone());
        json!({"wave":n,"title":title,"state":state.word(),"at":row.and_then(|row|row.at.clone()),"attention":wave_attention(Some(*state)),
            "tasks":carried.map_or(0,|wave|wave.tasks.len()),"returned":carried.map_or(0,|wave|wave.returned),
            "files":carried.map_or(0,|wave|wave.files.len()),"delivered_files":row.and_then(|row|row.files),
            "commit":row.and_then(|row|row.commit.clone()),"seconds":seconds,
            "model_used":spent("model_used"),"model_asked":spent("model_asked"),"effort_asked":spent("effort_asked"),
            "effort_used":spent("effort_used"),"tokens":spent("tokens")})
    }).collect();
    json!({"total":states.len(),"offset":offset,"limit":limit,"rows":page})
}

/// The criteria with a run of their proof, by current version.
fn proven(log: &SpecLog) -> BTreeSet<u64> {
    let currents = current_versions(log);
    log.visible().into_iter().filter(|e| e.event_type == "criterion_run").filter_map(|e| e.int("criterion"))
        .filter_map(|id| currents.get(&id).copied()).collect()
}

/// Every event number mapped to the number of the current version of its
/// item, resolved once.
fn current_versions(log: &SpecLog) -> BTreeMap<u64, u64> {
    log.current_versions().into_iter().map(|(id, event)| (id, event.id)).collect()
}

/// The list screen: one page of one kind of item, those that ask for
/// attention first, with the total and the count of every kind and filter.
pub(super) fn items(log: &SpecLog, params: &Params, languages: &mustard_core::domain::normalize::Languages) -> Value {
    let kind = params.kind.as_deref().filter(|kind| KINDS.contains(kind)).unwrap_or("task");
    let filter = params.filter.as_deref().filter(|filter| FILTERS.contains(filter)).unwrap_or("all");
    let limit = params.limit.unwrap_or(PAGE).clamp(1, 50);
    let offset = params.offset.unwrap_or(0);
    let states = progress::wave_states(log);
    let waves = wave_rows(log, &states);
    let qa = mustard_core::domain::spec_state::qa(log);
    let proven = proven(log);
    let codes = log.codes();
    let visible = log.visible();
    let counts: serde_json::Map<String, Value> = KINDS.iter().map(|kind| ((*kind).to_string(), json!(visible.iter().filter(|e| e.event_type == event_type(kind)).count()))).collect();
    let of_kind: Vec<&SpecEvent> = visible.iter().copied().filter(|e| e.event_type == event_type(kind)).collect();
    let row_of = |event: &SpecEvent| event.wave().and_then(|n| waves.get(&n));
    let committed = |event: &SpecEvent| row_of(event).and_then(|row| row.state) == Some(WaveState::Committed);
    let asks = |event: &SpecEvent| attention(event, row_of(event), &qa, &proven).is_some();
    let filters = json!({"all":of_kind.len(),"attention":of_kind.iter().filter(|e|asks(e)).count(),
        "committed":of_kind.iter().filter(|e|committed(e)).count(),"uncommitted":of_kind.iter().filter(|e|!committed(e)).count()});
    let kept: Vec<&SpecEvent> = of_kind.into_iter().filter(|event| match filter {
        "attention" => asks(event),
        "committed" => committed(event),
        "uncommitted" => !committed(event),
        _ => true,
    }).collect();
    let query = params.query.as_deref().map(str::trim).filter(|query| !query.is_empty());
    // A search keeps its rank, and only the items with every word when some
    // have them all; without one, attention comes first, then the file order.
    let ordered: Vec<&SpecEvent> = match query {
        Some(term) => {
            let ranking = mustard_core::domain::spec_events::ranked_by(kept, term, languages);
            let complete = ranking.complete();
            let shown = if complete > 0 { complete } else { ranking.items.len() };
            ranking.items.into_iter().take(shown).map(|ranked| ranked.event).collect()
        }
        None => {
            let mut kept = kept;
            kept.sort_by_key(|event| !asks(event));
            kept
        }
    };
    let total = ordered.len();
    let rows: Vec<Value> = ordered.into_iter().skip(offset).take(limit).map(|event| {
        let wave = row_of(event);
        let state = if event.event_type == "task" { Some(progress::task_state(event, &states)) } else { None };
        json!({"code":codes.get(&event.id),"title":title(event),"wave":event.wave(),"state":state,
            "at":wave.and_then(|row|row.at.clone()),"files":wave.and_then(|row|row.files),
            "covers":event.fields.get("covers").and_then(Value::as_array).map(Vec::len),
            "resends":wave.map(|row|row.resends),"unmet":wave.map(|row|row.unmet),
            "attention":attention(event,wave,&qa,&proven)})
    }).collect();
    json!({"kind":kind,"filter":filter,"query":query,"counts":counts,"filters":filters,"total":total,"offset":offset,"limit":limit,"rows":rows})
}

/// One item: what was asked, what the agent did, the result against what
/// the task covers, when the wave counts as done, files, covered items, the
/// wave's whole request, the tests run on what it covers, the wave's
/// measured spend and every version of the item.
pub(super) fn item(log: &SpecLog, code: &str) -> Value {
    let codes = log.codes();
    let Some(event) = log.visible().into_iter().find(|e| codes.get(&e.id).is_some_and(|c| c.eq_ignore_ascii_case(code))) else {
        return json!({"item":null,"reason":"panel-item-unknown"});
    };
    let states = progress::wave_states(log);
    let waves = wave_rows(log, &states);
    let row = event.wave().and_then(|n| waves.get(&n));
    let visible = log.visible();
    let wave_event = event.wave().and_then(|n| visible.iter().rev().find(|e| e.event_type == "wave" && e.wave() == Some(n)));
    let dispatch = event.wave().and_then(|n| log.last_dispatch_by_wave().get(&n).copied());
    let delivered = event.wave().and_then(|n| visible.iter().rev().find(|e| e.event_type == "delivered" && e.wave() == Some(n) && dispatch.is_none_or(|start| e.id > start)));
    let step = visible.iter().rev().find(|e| e.event_type == "step" && e.int("item").is_some_and(|item| item == event.id));
    let covers: Vec<u64> = event.ints("covers");
    let currents = current_versions(log);
    let describe = |id: u64| log.get(*currents.get(&id).unwrap_or(&id)).map(|covered| json!({"code":codes.get(&covered.id),"kind":covered.event_type,"title":title(covered)}));
    // The agreed answers of the return, crossed with what this task covers,
    // each read by its item's own text and kind, as a covered item is.
    let agreed: Vec<Value> = delivered.and_then(|e| e.fields.get("agreed")).and_then(Value::as_array).into_iter().flatten()
        .filter(|answer| covers.is_empty() || answer["item"].as_u64().is_some_and(|id| covers.iter().any(|covered| currents.get(covered) == currents.get(&id))))
        .map(|answer| {
            let described = answer["item"].as_u64().and_then(describe).unwrap_or(Value::Null);
            json!({"item":described["code"],"kind":described["kind"],"title":described["title"],"met":answer["met"],"note":answer.get("note").or_else(||answer.get("text"))})
        }).collect();
    let files: Vec<Value> = event.fields.get("files").and_then(Value::as_array).into_iter().flatten()
        .map(|file| file.get("path").cloned().unwrap_or_else(|| file.clone())).collect();
    // The wave's request goes whole, as the agent got it: the item screen is
    // the one place that carries it.
    let wave_request = event.wave().and_then(|n| log.last_by_wave("send").get(&n).and_then(|id| log.send_text(*id)));
    // The tests run on what the task covers: each covered criterion's proof
    // and its newest run, by the criterion's current version.
    let tests: Vec<Value> = covers.iter().filter_map(|id| log.get(*currents.get(id).unwrap_or(id))).filter(|covered| covered.event_type == "criterion").map(|criterion| {
        let run = visible.iter().rev().find(|e| e.event_type == "criterion_run" && e.int("criterion").is_some_and(|id| currents.get(&id).copied().unwrap_or(id) == criterion.id));
        json!({"criterion":codes.get(&criterion.id),"title":title(criterion),"proof":criterion.str_field("proof"),
            "result":run.and_then(|run|run.str_field("result")),"at":run.map(|run|run.at()),"ms":run.and_then(|run|run.int("ms"))})
    }).collect();
    // The newest verdict of the wave's reviewer, from any attempt: a rejection
    // explains why the wave runs again until the next verdict replaces it.
    let review = event.wave().and_then(|n| visible.iter().rev().find(|e| crate::commands::flow::round::is_wave_verdict(e) && e.wave() == Some(n)))
        .map(|verdict| json!({"result":verdict.str_field("result"),"reason":verdict.str_field("text").map(|text| cut_line(text, REVIEW_REASON_CHARS)),
            "files":verdict.fields.get("files").and_then(Value::as_array).cloned().unwrap_or_default(),"at":verdict.at()}));
    let item_code = codes.get(&event.id);
    let mut versions: Vec<&SpecEvent> = log.events.iter().filter(|e| codes.get(&e.id) == item_code).collect();
    versions.sort_by_key(|e| e.id);
    let history: Vec<Value> = versions.iter().enumerate().map(|(index, version)| {
        let changed = index.checked_sub(1).map(|before| crate::commands::spec_events::read::changed_fields(&log.whole_fields(versions[before]), &log.whole_fields(version)));
        json!({"at":version.at(),"author":version.str_field("author"),"wave":version.wave(),"changed":changed})
    }).collect();
    super::safe_value(json!({"item":{"code":item_code,"kind":event.event_type,"title":title(event),"wave":event.wave(),
        "state":(event.event_type=="task").then(||progress::task_state(event,&states)),"at":row.and_then(|row|row.at.clone()),
        "request":log.send_text(event.id).or_else(||event.str_field("text")).map(|text|bounded(text,TEXT_CHARS)),"done":step.or(delivered).and_then(|e|e.str_field("text")).map(|text|bounded(text,TEXT_CHARS)),
        "result":{"unmet":agreed.iter().filter(|answer|answer["met"]==false).count(),"agreed":agreed},"done_when":wave_event.and_then(|e|e.str_field("done_when")),
        "files":files,"delivered_files":delivered.and_then(|e|e.fields.get("files")).and_then(Value::as_array).map(Vec::len),
        "covers":covers.iter().filter_map(|id|describe(*id)).collect::<Vec<_>>(),
        "wave_request":wave_request,"tests":tests,"review":review,
        "wave_tokens":row.and_then(|row|row.tokens),"resends":row.map(|row|row.resends),"history":history}}))
}

/// How much of a long text the item screen carries.
const TEXT_CHARS: usize = 1200;

/// How many characters of the wave reviewer's reason the item screen carries.
const REVIEW_REASON_CHARS: usize = 200;

/// `text` whole when short; otherwise its first paragraphs up to `max`
/// characters, cut on a word, with how long the whole text is.
fn bounded(text: &str, max: usize) -> Value {
    if text.chars().count() <= max {
        return json!({"text":text,"cut":false});
    }
    let head: String = text.chars().take(max).collect();
    let end = head.rfind("\n\n").filter(|end| *end > max / 3).or_else(|| head.rfind(' ')).unwrap_or(head.len());
    json!({"text":head[..end].trim_end(),"cut":true,"chars":text.chars().count()})
}

/// The day after `day` (`AAAA-MM-DD`), or `None` when it does not parse.
fn next_day(day: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok().and_then(|day| day.succ_opt()).map(|day| day.format("%Y-%m-%d").to_string())
}

/// The usage screen: what was measured and when, the last fourteen days of
/// this project, cost by spec, this spec's waves and conductor, the tools,
/// the session and the copies' disk as last measured. Never a delivery text.
pub(super) fn usage(root: &Path, name: Option<&str>, log: Option<&SpecLog>, session: Option<&str>) -> Value {
    let history = super::consumption::history(root, mustard_core::io::spend::machine_dir().as_deref());
    let costs = super::consumption::costs(root);
    // Fourteen days ending on the last counted one; a day without a row is null.
    let measured: BTreeMap<String, u64> = history["project_days"].as_array().into_iter().flatten()
        .filter_map(|row| Some((row["day"].as_str()?.to_string(), row["tokens"].as_u64()?)))
        .fold(BTreeMap::new(), |mut days, (day, tokens)| { *days.entry(day).or_insert(0) += tokens; days });
    let last = history["counted_through"].as_str().map(str::to_string).or_else(|| measured.keys().next_back().cloned());
    let mut days = Vec::new();
    if let Some(last) = &last && let Ok(end) = chrono::NaiveDate::parse_from_str(last, "%Y-%m-%d") {
        let mut day = (end - chrono::Days::new(13)).format("%Y-%m-%d").to_string();
        for _ in 0..14 {
            days.push(json!({"day":day,"tokens":measured.get(&day)}));
            day = next_day(&day).unwrap_or_default();
        }
    }
    let first = measured.keys().next().cloned();
    let trend = mustard_core::io::spend::machine_dir().and_then(|dir| mustard_core::io::spend::load(&dir).ok())
        .and_then(|ledger| mustard_core::domain::spend::trend(&ledger.rows))
        .map(|trend| json!({"day":trend.day,"tokens":trend.tokens,"average_tokens":trend.average.tokens,
            "ratio":if trend.average.tokens>0 {Some((trend.tokens as f64/trend.average.tokens as f64*10.0).round()/10.0)} else {None},
            "basis":"last-busy-day-of-the-machine-against-the-average-of-the-days-before"}));
    let mut by_spec: Vec<(String, u64)> = costs["by_spec"].as_object().into_iter().flatten().filter_map(|(name, cost)| Some((name.clone(), cost.as_u64()?))).collect();
    by_spec.sort_by_key(|(_, cost)| std::cmp::Reverse(*cost));
    let (indexed, _) = index_rows(root);
    let jev = super::jev_usage::Ledger::read(root).project();
    let rtk = crate::shared::rtk_gain::cached(root);
    let live = super::live_usage::snapshot(root, session);
    let agents = live["agents"].as_object();
    let this = name.zip(log).map(|(name, log)| spec_usage(root, name, log));
    let disk = std::fs::read_to_string(copies_disk_file(root)).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    json!({"measured_through":history["counted_through"],"measured_at":history["measured_at"],"available":history["available"],
        "period":{"first":first,"last":last},"project_tokens":history["project_tokens"],"machine_tokens":history["machine_tokens"],"trend":trend,
        "days":days,
        "cost":{"total_micro_usd":costs["known_micro_usd"],"sessions":costs["observed_sessions"],"outside_spec_micro_usd":costs["unattributed_micro_usd"],
            "by_spec":by_spec.iter().map(|(name,cost)|json!({"name":name,"micro_usd":cost})).collect::<Vec<_>>(),
            "specs_without_cost":indexed.iter().filter(|row|!by_spec.iter().any(|(name,_)|*name==row.name)).count(),"basis":costs["basis"]},
        "spec":this,
        "tools":{"rtk":{"configured":rtk["configured"],"status":rtk["status"],"commands":rtk["commands"],"saved_tokens":rtk["saved_tokens"],"percent":rtk["percent"]},
            "jev":{"calls":jev["known_physical_requests"],"by_purpose":jev["by_purpose"],"input_tokens":jev["known_input_tokens"],"cost_micro_usd":jev["cost_micro_usd"]},
            "search":search_calls(root,name,log,&history)},
        "session":{"started_at":live["main"]["started_at"],"agents":agents.map(serde_json::Map::len),
            "working":agents.map(|agents|agents.values().filter(|agent|agent["status"]=="running").count())},
        "copies_disk":disk,"mustard_skills":mustard_skills(root)})
}

/// Quantos caracteres do último ganho de uma skill a tela mostra.
const GAIN_CHARS: usize = 160;

/// As skills que o Mustard criou no projeto, pelo registro delas, em ordem de
/// nome: o estado, quantas vezes cada uma foi com uma tarefa, o último ganho
/// medido, cortado, e os tokens da onda que a criou, quando medidos. O custo
/// em dinheiro não é medido por onda, e não aparece.
fn mustard_skills(root: &Path) -> Value {
    let registry = mustard_core::io::mustard_skills::read(root);
    let rows: Vec<Value> = registry
        .skills
        .iter()
        .map(|(name, skill)| {
            let gain = skill.gain.as_deref().map(|gain| cut_line(gain, GAIN_CHARS));
            json!({"name":name,"state":skill.state,"tasks":skill.deliveries,"gain":gain,"wave_tokens":creating_tokens(root, skill)})
        })
        .collect();
    json!(rows)
}

/// Os tokens da onda que criou a skill `skill`: o consumo medido do agente
/// dela ou, sem medida, o que o envio gravou. `None` sem onda conhecida ou
/// sem medida.
fn creating_tokens(root: &Path, skill: &mustard_core::io::mustard_skills::MustardSkill) -> Option<u64> {
    let wave = skill.wave?;
    let file = mustard_core::io::spec_events::spec_file(root, &skill.spec).ok()?;
    let log = mustard_core::io::spec_events::read(&file).ok()??;
    let live = super::wave_usage::read(root, &skill.spec);
    let measured = log.last_dispatch_by_wave().get(&wave).and_then(|dispatch| super::wave_usage::current_at(&live, wave, *dispatch)).and_then(|usage| usage["tokens"].as_u64());
    measured.or_else(|| log.last_by_wave("send").get(&wave).and_then(|id| sent_field(&log, *id, "tokens")).and_then(Value::as_u64))
}

/// This spec's measured spend: the waves with a measure, how many lack one,
/// and the conductor, taken once.
fn spec_usage(root: &Path, name: &str, log: &SpecLog) -> Value {
    let live = super::wave_usage::read(root, name);
    let dispatches = log.last_dispatch_by_wave();
    let planned = log.planned_waves();
    let visible = log.visible();
    let sent: BTreeMap<u64, &SpecEvent> = visible.iter().filter(|e| e.event_type == "send").filter_map(|e| e.wave().map(|n| (n, *e))).collect();
    let mut known = 0u64;
    let mut unknown = 0usize;
    let mut conductor = visible.iter().filter(|e| e.event_type == "send").filter_map(|e| e.int("caller_tokens")).max();
    for wave in &planned {
        let measured = dispatches.get(wave).and_then(|dispatch| super::wave_usage::current_at(&live, *wave, *dispatch));
        conductor = conductor.max(measured.and_then(|usage| usage["caller_tokens"].as_u64()));
        match measured.and_then(|usage| usage["tokens"].as_u64()).or_else(|| sent.get(wave).and_then(|e| e.int("tokens"))) {
            Some(tokens) => known = known.saturating_add(tokens),
            None => unknown += 1,
        }
    }
    let mut out = json!({"name":name,"wave_tokens":known,"waves_without_measure":unknown,"conductor_tokens":conductor,"measured_at":live["measured_at"]});
    if let Some(fields) = out.as_object_mut() {
        fields.extend(wave_agents(root, name, log, &live));
    }
    out
}

/// O valor de `field` no envio `id` ou, quando essa versão não o traz, na
/// versão que ela substitui: a versão nova de um envio pode levar só o que
/// mudou.
fn sent_field<'a>(log: &'a SpecLog, id: u64, field: &str) -> Option<&'a Value> {
    let mut seen = BTreeSet::new();
    let mut at = Some(id);
    while let Some(id) = at.filter(|id| seen.insert(*id)) {
        let event = log.get(id)?;
        if let Some(value) = event.fields.get(field).filter(|value| !value.is_null()) {
            return Some(value);
        }
        at = event.int("replaces");
    }
    None
}

/// A linha de cada onda enviada, com a posição do despacho dela, e as voltas
/// recusadas e as reprovações da revisão por onda.
struct AgentRows {
    rows: Vec<(u64, u64, Value)>,
    refused: BTreeMap<u64, u64>,
    reviewed: BTreeMap<u64, u64>,
}

/// O que cada onda enviada recebeu e o que deu errado nela, a mais nova
/// primeiro: o modelo que o agente usou (medido no arquivo dele), o modelo e
/// o esforço que o envio pediu, o esforço que valeu no lançamento
/// (`effort_used`), as skills entregues, se o pedido continuou um resumo, os
/// tokens, as voltas recusadas e as reprovações da revisão. A tela do
/// Consumo e a das ondas leem a mesma linha.
fn agent_rows(log: &SpecLog, live: &Value) -> AgentRows {
    let last = log.last_by_wave("send");
    let dispatches = log.last_dispatch_by_wave();
    // A volta recusada: a que nenhuma entrega oficial assumiu e que não espera
    // mais a rodada, ou a que quem conduz a obra reprovou.
    let assumed: BTreeSet<u64> = log.events.iter().filter(|e| e.event_type == "delivered" && !e.returned()).flat_map(|e| e.replaced()).collect();
    let waiting: BTreeSet<u64> = log.unassumed_returns().into_iter().map(|e| e.id).collect();
    let rejected: BTreeSet<u64> = log.events.iter().filter(|e| e.event_type == "send")
        .filter_map(|e| e.fields.get("rejected")?.get("delivered")?.as_u64()).collect();
    let mut refused: BTreeMap<u64, u64> = BTreeMap::new();
    for back in log.events.iter().filter(|e| e.event_type == "delivered" && e.returned()) {
        if let Some(wave) = back.wave() && !assumed.contains(&back.id) && (!waiting.contains(&back.id) || rejected.contains(&back.id)) {
            *refused.entry(wave).or_default() += 1;
        }
    }
    let reviewed: BTreeMap<u64, u64> = log.verdicts_by_wave().into_iter()
        .map(|(wave, verdicts)| (wave, verdicts.iter().filter(|v| v.event_type == "verdict" && v.str_field("result") == Some("rejected")).count() as u64))
        .filter(|(_, count)| *count > 0).collect();
    let mut rows: Vec<(u64, u64, Value)> = Vec::new();
    for (wave, id) in &last {
        let text = |field: &str| sent_field(log, *id, field).and_then(Value::as_str);
        let measured = dispatches.get(wave).and_then(|dispatch| super::wave_usage::current_at(live, *wave, *dispatch));
        let used = measured.and_then(|usage| usage["model_used"].as_str()).or_else(|| text("model_used"));
        let delivered: Vec<&str> = sent_field(log, *id, "skills").and_then(Value::as_array).into_iter().flatten()
            .filter_map(|skill| skill["name"].as_str().or_else(|| skill.as_str())).collect();
        let tokens = measured.and_then(|usage| usage["tokens"].as_u64()).or_else(|| sent_field(log, *id, "tokens").and_then(Value::as_u64));
        rows.push((dispatches.get(wave).copied().unwrap_or(*id), *wave, json!({"wave":wave,"model_used":used,"model_asked":text("model"),"effort_asked":text("effort"),
            "effort_used":text("effort_used"),
            "skills":delivered,"summary_reused":sent_field(log, *id, "summary").is_some(),"tokens":tokens,
            "refused_returns":refused.get(wave).copied().unwrap_or(0),"review_rejections":reviewed.get(wave).copied().unwrap_or(0)})));
    }
    rows.sort_by_key(|(dispatch, _, _)| std::cmp::Reverse(*dispatch));
    AgentRows { rows, refused, reviewed }
}

/// As linhas de [`agent_rows`] das últimas [`PAGE`] ondas despachadas, a mais
/// nova primeiro, e o mesmo somado na spec, com o conhecimento reaproveitado
/// e as chamadas ao Jev e os ganchos que falharam, lidos do histórico inteiro
/// das operações. Só números e nomes: motivo e texto ficam na spec.
fn wave_agents(root: &Path, name: &str, log: &SpecLog, live: &Value) -> serde_json::Map<String, Value> {
    let AgentRows { rows, refused, reviewed } = agent_rows(log, live);
    let mut models: BTreeMap<(String, Option<String>), u64> = BTreeMap::new();
    let mut skills: BTreeMap<String, u64> = BTreeMap::new();
    let mut summaries = 0u64;
    for (_, _, row) in &rows {
        if let Some(model) = row["model_used"].as_str() {
            *models.entry((model.to_string(), row["effort_asked"].as_str().map(str::to_string))).or_default() += 1;
        }
        for skill in row["skills"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            *skills.entry(skill.to_string()).or_default() += 1;
        }
        summaries += u64::from(row["summary_reused"] == true);
    }
    let sent = rows.len();
    let mut models: Vec<_> = models.into_iter().collect();
    models.sort_by_key(|(_, waves)| std::cmp::Reverse(*waves));
    let mut skills: Vec<_> = skills.into_iter().collect();
    skills.sort_by_key(|(_, waves)| std::cmp::Reverse(*waves));
    // O que só o arquivo de operações guarda, sem onda: o Jev que falhou, o
    // gancho que barrou e a consulta de conhecimento que veio pronta.
    let operations = crate::commands::spec_events::conversation::recorded_operations(root, name);
    let mut jev: BTreeMap<String, u64> = BTreeMap::new();
    let mut hooks: BTreeMap<String, u64> = BTreeMap::new();
    let (mut knowledge, mut cached, mut reserves) = (0u64, 0u64, 0u64);
    for event in operations.visible() {
        match event.event_type.as_str() {
            "call" => {
                if let Some(reason) = event.str_field("filter").and_then(|filter| filter.strip_prefix("jev:")) {
                    *jev.entry(reason.to_string()).or_default() += 1;
                }
                // A decisão que o Jev não deu e foi à reserva dele.
                reserves += u64::from(event.str_field("command") == Some(crate::shared::jev::reserve::RESERVE_CALL));
                if event.str_field("command") == Some("knowledge") {
                    knowledge += 1;
                    cached += u64::from(event.fields.get("knowledge_cached") == Some(&Value::Bool(true)));
                }
            }
            "hook" if event.str_field("action") == Some("block") => {
                *hooks.entry(event.str_field("hook").unwrap_or("unknown").to_string()).or_default() += 1;
            }
            _ => {}
        }
    }
    let mut out = serde_json::Map::new();
    out.insert("waves_sent".into(), json!(sent));
    out.insert("waves".into(), json!(rows.into_iter().take(PAGE).map(|(_, _, row)| row).collect::<Vec<_>>()));
    out.insert("models".into(), json!(models.into_iter().map(|((model, effort), waves)| json!({"model":model,"effort":effort,"waves":waves})).collect::<Vec<_>>()));
    out.insert("skills".into(), json!(skills.into_iter().take(PAGE).map(|(name, waves)| json!({"name":name,"waves":waves})).collect::<Vec<_>>()));
    out.insert("reuse".into(), json!({"summaries":summaries,"knowledge_calls":knowledge,"knowledge_cached":cached}));
    out.insert("failures".into(), json!({"jev":{"calls":jev.values().sum::<u64>(),"reasons":jev,"reserves":reserves},
        "hook_blocks":{"total":hooks.values().sum::<u64>(),"by_hook":hooks},
        "refused_returns":refused.values().sum::<u64>(),"review_rejections":reviewed.values().sum::<u64>()}));
    out
}

/// Code searches: every one the project's conversations counted, and the
/// ones recorded as calls of this spec.
fn search_calls(root: &Path, name: Option<&str>, log: Option<&SpecLog>, history: &Value) -> Value {
    let in_spec = name.zip(log).map(|(name, log)| {
        let transport = crate::commands::spec_events::conversation::recorded_operations(root, name);
        log.visible().into_iter().chain(transport.visible()).filter(|e| e.event_type == "call" && e.str_field("command") == Some("search")).count()
    });
    let total: u64 = history["project_days"].as_array().into_iter().flatten().filter_map(|row| row["code_searches"].as_u64()).sum();
    json!({"total":total,"in_spec":in_spec})
}

/// Where the last measure of the copies' disk is kept.
pub(super) fn copies_disk_file(root: &Path) -> std::path::PathBuf {
    root.join(".claude/mustard/usage/copies-disk.json")
}

/// Measures the disk the wave copies of this project take, without following
/// links, and keeps the measure with its date. Only an explicit measure calls
/// this; opening a screen never does.
pub(super) fn measure_copies_disk(root: &Path) -> Value {
    fn size(path: &Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
        if meta.is_dir() {
            std::fs::read_dir(path).map(|entries| entries.flatten().map(|entry| size(&entry.path())).sum()).unwrap_or(0)
        } else {
            meta.len()
        }
    }
    let folder = mustard_core::io::wave_prompt::copies_dir(root);
    let value = json!({"bytes":size(&folder),"measured_at":chrono::Utc::now().to_rfc3339()});
    let written = mustard_core::io::fs::write_atomic(copies_disk_file(root), value.to_string().as_bytes()).is_ok();
    json!({"ok":written,"bytes":value["bytes"],"measured_at":value["measured_at"]})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spec with a committed wave, a wave sent with no agent, a backlog,
    /// a criterion without title proven once, a task rewritten once and a
    /// long delivery text that no screen but the item may carry.
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let folder = root.join(".claude/spec/demo");
        std::fs::create_dir_all(&folder).unwrap();
        let mut rows = vec![
            json!({"v":1,"id":1,"at":"2026-10-01T10:00:00Z","type":"message","text":"Pedido do usuário."}),
            json!({"v":1,"id":2,"at":"2026-10-01T10:01:00Z","type":"criterion","when":"Ao abrir o painel","then":"Os números batem","proof":"echo ok"}),
            json!({"v":1,"id":3,"at":"2026-10-01T10:02:00Z","type":"wave","n":1,"text":"Primeira onda.","criteria":[2],"done_when":"Conferido no painel.","origin":1}),
            json!({"v":1,"id":4,"at":"2026-10-01T10:03:00Z","type":"wave","n":2,"text":"Segunda onda.","criteria":[2],"done_when":"Conferido.","origin":1}),
            json!({"v":1,"id":5,"at":"2026-10-01T10:04:00Z","type":"task","wave":1,"title":"Primeira","text":"Fazer a primeira.","files":[{"path":"src/alpha.rs"}],"covers":[2]}),
            json!({"v":1,"id":6,"at":"2026-10-01T10:05:00Z","type":"task","wave":2,"title":"Segunda","text":"Fazer a segunda.","files":[{"path":"src/beta.rs"}]}),
            json!({"v":1,"id":7,"at":"2026-10-01T10:06:00Z","type":"task","title":"No backlog","text":"Sem onda ainda."}),
            json!({"v":1,"id":8,"at":"2026-10-02T09:00:00Z","type":"send","wave":1,"role":"wave","text":"Pedido da onda 1."}),
            json!({"v":1,"id":9,"at":"2026-10-02T11:00:00Z","type":"delivered","wave":1,"text":format!("SECRET-DELIVERY-TEXT {}","x".repeat(4000)),
                "files":["src/alpha.rs"],"agreed":[{"item":2,"met":true}]}),
            json!({"v":1,"id":10,"at":"2026-10-02T11:05:00Z","type":"commit","sha":"abc123","title":"Primeira","waves":[1],"files":["src/alpha.rs"],"repo":"."}),
            json!({"v":1,"id":11,"at":"2026-10-02T11:06:00Z","type":"criterion_run","criterion":2,"result":"pass","command":"echo ok"}),
            json!({"v":1,"id":12,"at":"2026-10-03T08:00:00Z","type":"task","wave":2,"title":"Segunda, reescrita","text":"Fazer a segunda.","files":[{"path":"src/beta.rs"}],"replaces":6}),
            json!({"v":1,"id":13,"at":"2026-10-03T09:00:00Z","type":"send","wave":2,"role":"wave","text":"Pedido da onda 2."}),
        ];
        for n in 0..10u64 {
            rows.push(json!({"v":1,"id":20+n,"at":"2026-10-04T08:00:00Z","type":"task","title":format!("Extra {n}"),"text":"Mais uma.","files":[{"path":format!("src/gamma{n}.rs")}]}));
        }
        let text = rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n";
        std::fs::write(folder.join("spec.ndjson"), text).unwrap();
        dir
    }

    fn screen(root: &Path, section: &str, params: &Params) -> Value {
        super::super::dashboard::query_with(root, Some("demo"), None, None, section, None, params)
    }

    /// The overview carries the project summary: the newest deliveries
    /// first, a bounded few, each text cut, and how many there are.
    #[test]
    fn the_overview_shows_the_newest_deliveries_of_the_project() {
        let dir = fixture();
        let root = dir.path();
        let answer = screen(root, "overview", &Params::default());
        assert_eq!(answer["deliveries"], json!({"shown":[],"total":0}), "{answer}");
        for n in 1..=7u64 {
            let entry = mustard_core::domain::project_summary::Entry {
                spec: format!("obra-{n}"),
                delivered_at: format!("2026-10-0{n}T10:00:00Z"),
                pr: Some(mustard_core::domain::project_summary::Pr { number: 100 + n, url: None }),
                title: format!("Entrega {n}"),
                delivered: "palavra ".repeat(80),
                rules: vec![],
                skills: vec![],
                cost: None,
            };
            mustard_core::io::project_summary::upsert(root, &entry).unwrap();
        }
        let answer = screen(root, "overview", &Params::default());
        let deliveries = &answer["deliveries"];
        assert_eq!(deliveries["total"], json!(7), "{answer}");
        let shown: Vec<&str> = deliveries["shown"].as_array().unwrap().iter().map(|d| d["spec"].as_str().unwrap()).collect();
        assert_eq!(shown, vec!["obra-7", "obra-6", "obra-5", "obra-4", "obra-3"]);
        let first = &deliveries["shown"][0];
        assert_eq!((first["pr"].clone(), first["title"].clone(), first["delivered_at"].clone()), (json!(107), json!("Entrega 7"), json!("2026-10-07")));
        assert!(first["delivered"].as_str().unwrap().chars().count() <= 200, "{first}");
    }

    /// A spec in execution, in `language`, whose only wave is delivered and
    /// committed, and whose final approval a reopening voided.
    fn finished_and_reopened(language: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), json!({"language":{"text":language}}).to_string()).unwrap();
        let folder = root.join(".claude/spec/demo");
        std::fs::create_dir_all(&folder).unwrap();
        let rows = [
            json!({"v":1,"id":1,"at":"2026-10-01T10:00:00Z","type":"state","author":"binary","phase":"survey","branch":"fix/demo","base":"dev"}),
            json!({"v":1,"id":2,"at":"2026-10-01T10:01:00Z","type":"message","text":"Pedido do usuário."}),
            json!({"v":1,"id":3,"at":"2026-10-01T10:02:00Z","type":"criterion","when":"Ao abrir o painel","then":"Os números batem","proof":"echo ok"}),
            json!({"v":1,"id":4,"at":"2026-10-01T10:03:00Z","type":"wave","n":1,"text":"Primeira onda.","criteria":[3],"done_when":"Conferido.","origin":2}),
            json!({"v":1,"id":5,"at":"2026-10-01T10:04:00Z","type":"task","wave":1,"title":"Primeira","text":"Fazer a primeira.","files":[{"path":"src/alpha.rs"}],"covers":[3]}),
            json!({"v":1,"id":6,"at":"2026-10-01T10:05:00Z","type":"state","author":"user","phase":"approved"}),
            json!({"v":1,"id":7,"at":"2026-10-01T10:06:00Z","type":"state","author":"binary","phase":"running"}),
            json!({"v":1,"id":8,"at":"2026-10-02T09:00:00Z","type":"send","wave":1,"role":"wave","text":"Pedido da onda 1.","agent_launched":true}),
            json!({"v":1,"id":9,"at":"2026-10-02T11:00:00Z","type":"delivered","wave":1,"text":"Feito.","files":["src/alpha.rs"],"agreed":[{"item":3,"met":true}]}),
            json!({"v":1,"id":10,"at":"2026-10-02T11:05:00Z","type":"commit","sha":"abc123","title":"Primeira","waves":[1],"files":["src/alpha.rs"],"repo":"."}),
            json!({"v":1,"id":11,"at":"2026-10-02T11:06:00Z","type":"criterion_run","criterion":3,"result":"pass","command":"echo ok"}),
            json!({"v":1,"id":12,"at":"2026-10-02T12:00:00Z","type":"send","role":"review","text":"Revise a obra."}),
            json!({"v":1,"id":13,"at":"2026-10-02T14:09:35Z","type":"verdict","author":"review","final":true,"result":"approved","text":"Aprovada."}),
            json!({"v":1,"id":14,"at":"2026-10-02T14:09:42Z","type":"state","author":"binary","phase":"closed"}),
            json!({"v":1,"id":15,"at":"2026-10-02T17:13:51Z","type":"state","author":"binary","phase":"running","reason":"Pedido novo."}),
        ];
        let text = rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n";
        std::fs::write(folder.join("spec.ndjson"), text).unwrap();
        dir
    }

    /// O comando do próximo passo é o do passo que o texto diz, e não o da
    /// fase: a obra em execução sem onda nem tarefa a rodar fecha, e o painel
    /// mostra o comando do fechamento. Antes o texto dizia "fechar" e o
    /// comando ao lado dele era o da rodada, que sairia sem nada para enviar.
    #[test]
    fn the_next_step_says_in_the_project_language_what_the_round_would_do() {
        use mustard_core::platform::i18n::{Locale, translate};
        for (language, lang) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
            let dir = finished_and_reopened(language);
            let overview = screen(dir.path(), "overview", &Params::default());
            assert_eq!(overview["spec"]["phase"], "running", "{overview}");
            assert_eq!(overview["spec"]["next"]["command"], "mustard-rt run close --spec demo", "{overview}");
            assert_eq!(overview["spec"]["next"]["text"], translate("resume.step.close_again", lang), "{overview}");
        }
        // The wave sent with no agent goes out again on the next round.
        let dir = fixture();
        let path = dir.path().join(".claude/spec/demo/spec.ndjson");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str(&format!("{}\n", json!({"v":1,"id":40,"at":"2026-10-05T08:00:00Z","type":"state","author":"binary","phase":"running"})));
        std::fs::write(&path, text).unwrap();
        let overview = screen(dir.path(), "overview", &Params::default());
        assert_eq!(overview["spec"]["next"]["text"], translate("resume.step.dispatch_one", Locale::PtBr).replace("{waves}", "2"), "{overview}");
    }

    #[test]
    fn the_overview_stays_small_and_counts_as_the_round() {
        let dir = fixture();
        let overview = screen(dir.path(), "overview", &Params::default());
        assert_eq!(overview["ok"], true, "{overview}");
        assert!(overview.to_string().len() < 8 * 1024, "{} bytes", overview.to_string().len());
        let (finished, planned) = progress::wave_counts(&super::super::dashboard::spec_log(dir.path(), "demo").unwrap());
        assert_eq!(overview["spec"]["waves"], json!({"finished":finished,"planned":planned}));
        assert_eq!(overview["spec"]["waves"], json!({"finished":1,"planned":2}));
        assert_eq!(overview["spec"]["tasks"]["finished"], 1);
        assert_eq!(overview["spec"]["tasks"]["total"], 13);
        assert_eq!(overview["spec"]["criteria"]["passed"], 1);
        assert_eq!(overview["warnings"][0], json!({"kind":"wave-no-agent","waves":[2]}));
    }

    /// The waves screen brings every wave of the plan, newest first and a page
    /// at a time, each with what it carried and delivered, its commit, its
    /// time and the spend of the usage screen's own row; its total is the one
    /// the spec screen counts.
    #[test]
    fn the_waves_screen_pages_every_wave_with_its_own_spend_and_counts_as_the_spec_screen() {
        let dir = fixture();
        let root = dir.path();
        let path = root.join(".claude/spec/demo/spec.ndjson");
        let mut text = std::fs::read_to_string(&path).unwrap();
        for n in 3..=10u64 {
            text.push_str(&format!("{}\n", json!({"v":1,"id":100+n,"at":"2026-10-05T08:00:00Z","type":"wave","n":n,"text":format!("Onda {n}."),"criteria":[2],"done_when":"Ok.","origin":1})));
            text.push_str(&format!("{}\n", json!({"v":1,"id":200+n,"at":"2026-10-05T08:01:00Z","type":"task","wave":n,"title":format!("Tarefa da onda {n}"),"text":"Fazer.","files":[{"path":format!("src/w{n}.rs")}]})));
        }
        // The measured spend of wave 1, and a task that wave 3 carried and the backlog took back.
        for row in [
            json!({"v":1,"id":300,"at":"2026-10-02T11:01:00Z","type":"send","wave":1,"replaces":8,"model":"opus","effort":"xhigh","model_used":"claude-opus-5-5","tokens":4321}),
            json!({"v":1,"id":301,"at":"2026-10-05T09:00:00Z","type":"task","wave":3,"title":"Voltou","text":"Fazer.","files":[{"path":"src/delta.rs"}]}),
            json!({"v":1,"id":302,"at":"2026-10-05T09:10:00Z","type":"task","title":"Voltou","text":"Fazer.","files":[{"path":"src/delta.rs"}],"replaces":301}),
        ] {
            text.push_str(&format!("{row}\n"));
        }
        std::fs::write(&path, text).unwrap();
        let first = screen(root, "waves", &Params::default());
        assert_eq!(first["ok"], true, "{first}");
        let spec = screen(root, "spec", &Params::default());
        assert_eq!((first["total"].clone(), spec["spec"]["counts"]["waves"].clone()), (json!(10), json!(10)), "{first}");
        let numbers = |page: &Value| page["rows"].as_array().unwrap().iter().map(|row| row["wave"].as_u64().unwrap()).collect::<Vec<_>>();
        assert_eq!(numbers(&first), [10, 9, 8, 7, 6, 5, 4, 3], "{first}");
        let rest = screen(root, "waves", &Params { offset: Some(8), ..Params::default() });
        assert_eq!(numbers(&rest), [2, 1], "{rest}");
        let one = &rest["rows"][1];
        assert_eq!((one["title"].clone(), one["state"].clone(), one["commit"].clone()), (json!("Primeira"), json!("committed"), json!("abc123")), "{one}");
        assert_eq!((one["tasks"].clone(), one["files"].clone(), one["delivered_files"].clone(), one["seconds"].clone()), (json!(1), json!(1), json!(1), json!(7200)), "{one}");
        let usage = screen(root, "usage", &Params::default());
        let by_wave = usage["spec"]["waves"].as_array().unwrap().iter().find(|row| row["wave"] == 1).cloned().unwrap_or_else(|| panic!("{usage}"));
        for field in ["model_used", "model_asked", "effort_asked", "tokens"] {
            assert_eq!(one[field], by_wave[field], "{field}: {one} {by_wave}");
        }
        assert_eq!(one["tokens"], 4321, "{one}");
        let two = &rest["rows"][0];
        assert_eq!((two["state"].clone(), two["attention"].clone(), two["tasks"].clone(), two["title"].clone()),
            (json!("no-agent"), json!("wave-no-agent"), json!(1), json!("Segunda, reescrita")), "{two}");
        assert!(two["tokens"].is_null() && two["commit"].is_null() && two["seconds"].is_null(), "{two}");
        let three = &first["rows"][7];
        assert_eq!((three["wave"].clone(), three["tasks"].clone(), three["returned"].clone(), three["files"].clone()), (json!(3), json!(2), json!(1), json!(2)), "{three}");
        assert_eq!(first["rows"][0]["title"], "Tarefa da onda 10", "{first}");
    }

    #[test]
    fn a_task_takes_the_state_of_its_wave_and_without_one_stays_in_the_backlog() {
        let dir = fixture();
        let list = screen(dir.path(), "items", &Params { kind: Some("task".into()), limit: Some(20), ..Params::default() });
        let state = |code: &str| list["rows"].as_array().unwrap().iter().find(|row| row["code"] == code).map(|row| row["state"].clone());
        assert_eq!(state("MSTD-TASK-0002"), Some(json!("no-agent")), "{list}");
        assert_eq!(state("MSTD-TASK-0001"), Some(json!("committed")), "{list}");
        assert_eq!(state("MSTD-TASK-0003"), Some(json!("backlog")), "{list}");
        assert_eq!(list["rows"][0]["code"], "MSTD-TASK-0002", "the row asking for attention comes first: {list}");
        assert_eq!(list["rows"][0]["attention"], "wave-no-agent");
        assert_eq!(list["filters"]["attention"], 1);
    }

    #[test]
    fn the_list_pages_by_eight_and_finds_a_task_by_its_file() {
        let dir = fixture();
        let first = screen(dir.path(), "items", &Params { kind: Some("task".into()), ..Params::default() });
        assert_eq!(first["rows"].as_array().map(Vec::len), Some(8), "{first}");
        assert_eq!(first["total"], 13);
        let more = screen(dir.path(), "items", &Params { kind: Some("task".into()), offset: Some(8), ..Params::default() });
        assert_eq!(more["rows"].as_array().map(Vec::len), Some(5), "{more}");
        let found = screen(dir.path(), "items", &Params { kind: Some("task".into()), query: Some("src/gamma7.rs".into()), ..Params::default() });
        assert_eq!(found["rows"][0]["title"], "Extra 7", "{found}");
    }

    #[test]
    fn a_criterion_without_a_title_is_named_by_its_when() {
        let dir = fixture();
        let list = screen(dir.path(), "items", &Params { kind: Some("criterion".into()), ..Params::default() });
        assert_eq!(list["rows"][0]["title"], "Ao abrir o painel", "{list}");
        assert_eq!(list["counts"]["criterion"], 1);
    }

    #[test]
    fn the_item_history_keeps_every_version_and_the_wave_closing_line() {
        let dir = fixture();
        let item = screen(dir.path(), "item", &Params { item: Some("MSTD-TASK-0002".into()), ..Params::default() });
        let history = item["item"]["history"].as_array().unwrap_or_else(|| panic!("{item}"));
        assert_eq!(history.len(), 2, "{item}");
        assert!(history[1]["changed"]["title"].is_object() || history[1]["changed"]["title"] == true, "{item}");
        assert_eq!(item["item"]["done_when"], "Conferido.");
        assert_eq!(item["item"]["state"], "no-agent");
        let done = screen(dir.path(), "item", &Params { item: Some("MSTD-TASK-0001".into()), ..Params::default() });
        assert_eq!(done["item"]["result"]["agreed"][0]["met"], true, "{done}");
        // An agreed answer reads by its own text and kind, as a covered item does.
        assert_eq!(done["item"]["result"]["agreed"][0]["title"], "Ao abrir o painel", "{done}");
        assert_eq!(done["item"]["result"]["agreed"][0]["kind"], "criterion", "{done}");
        assert_eq!(done["item"]["covers"][0]["title"], "Ao abrir o painel");
    }

    #[test]
    fn a_task_shows_its_whole_wave_request_and_the_tests_run_on_what_it_covers() {
        let dir = fixture();
        let path = dir.path().join(".claude/spec/demo/spec.ndjson");
        let mut text = std::fs::read_to_string(&path).unwrap();
        let request = format!("# demo — onda 1\n\n{}\nFim do pedido.", "linha do pedido ".repeat(150));
        text.push_str(&format!("{}\n", json!({"v":1,"id":40,"at":"2026-10-05T08:00:00Z","type":"send","wave":1,"role":"wave","text":request})));
        std::fs::write(&path, text).unwrap();
        let done = screen(dir.path(), "item", &Params { item: Some("MSTD-TASK-0001".into()), ..Params::default() });
        assert_eq!(done["item"]["wave_request"], json!(request), "the whole request, never cut: {done}");
        let tests = done["item"]["tests"].as_array().unwrap_or_else(|| panic!("{done}"));
        assert_eq!(tests.len(), 1, "{done}");
        assert_eq!(tests[0]["criterion"], "MSTD-CRIT-0001", "{done}");
        assert_eq!(tests[0]["title"], "Ao abrir o painel", "{done}");
        assert_eq!(tests[0]["proof"], "echo ok", "{done}");
        assert_eq!(tests[0]["result"], "pass", "{done}");
        // A task whose wave was never sent and covers nothing has neither.
        let backlog = screen(dir.path(), "item", &Params { item: Some("MSTD-TASK-0003".into()), ..Params::default() });
        assert!(backlog["item"]["wave_request"].is_null(), "{backlog}");
        assert_eq!(backlog["item"]["tests"], json!([]), "{backlog}");
    }

    /// A task shows the newest verdict of its wave's reviewer: approved, or
    /// rejected with a short reason and the files it points at. The final
    /// review's verdict is not the wave reviewer's.
    #[test]
    fn a_task_shows_the_newest_verdict_of_its_wave_reviewer() {
        let dir = fixture();
        let path = dir.path().join(".claude/spec/demo/spec.ndjson");
        let task = |code: &str| screen(dir.path(), "item", &Params { item: Some(code.into()), ..Params::default() });
        let append = |row: Value| {
            let mut text = std::fs::read_to_string(&path).unwrap();
            text.push_str(&format!("{row}\n"));
            std::fs::write(&path, text).unwrap();
        };
        assert!(task("MSTD-TASK-0001")["item"]["review"].is_null(), "no verdict yet");
        let reason = format!("Falta o teste do caso vazio. {}", "Detalhe longo do revisor. ".repeat(40));
        append(json!({"v":1,"id":41,"at":"2026-10-05T09:00:00Z","type":"verdict","author":"review","wave":1,"wave_review":true,
            "result":"rejected","text":reason,"files":["src/alpha.rs"]}));
        append(json!({"v":1,"id":42,"at":"2026-10-05T09:30:00Z","type":"verdict","author":"review","wave":1,"final":true,"result":"approved","text":"Obra conferida."}));
        let rejected = task("MSTD-TASK-0001");
        let review = &rejected["item"]["review"];
        assert_eq!(review["result"], "rejected", "{rejected}");
        assert_eq!(review["files"], json!(["src/alpha.rs"]), "{rejected}");
        let short = review["reason"].as_str().unwrap_or_else(|| panic!("{rejected}"));
        assert!(short.starts_with("Falta o teste do caso vazio.") && short.chars().count() <= 200 && short.ends_with('…'), "{short}");
        assert!(task("MSTD-TASK-0002")["item"]["review"].is_null(), "another wave's verdict stays on its tasks");
        append(json!({"v":1,"id":43,"at":"2026-10-05T10:00:00Z","type":"verdict","author":"review","wave":1,"wave_review":true,
            "result":"approved","text":"Conferi o diff e as regras."}));
        let approved = task("MSTD-TASK-0001");
        assert_eq!(approved["item"]["review"]["result"], "approved", "the newest verdict: {approved}");
        assert_eq!(approved["item"]["review"]["files"], json!([]), "{approved}");
    }

    #[test]
    fn no_screen_but_the_item_carries_a_delivery_text() {
        let dir = fixture();
        for section in ["overview", "spec", "items", "usage"] {
            let value = screen(dir.path(), section, &Params::default());
            assert_eq!(value["ok"], true, "{section}: {value}");
            assert!(!value.to_string().contains("SECRET-DELIVERY-TEXT"), "{section} carries a delivery text");
        }
        let spec = screen(dir.path(), "spec", &Params::default());
        assert_eq!(spec["spec"]["waves"]["committed"], 1, "{spec}");
        assert_eq!(spec["spec"]["waves"]["no_agent"], json!([2]));
        assert_eq!(spec["spec"]["commits"], 1);
    }

    /// The usage screen lists the skills Mustard made: the name, the state,
    /// how many times each went with a task, the last measured gain and the
    /// tokens of the wave that made it, when measured.
    #[test]
    fn the_usage_screen_lists_the_mustard_skills() {
        let dir = fixture();
        let root = dir.path();
        let path = root.join(".claude/spec/demo/spec.ndjson");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str(&format!("{}\n", json!({"v":1,"id":50,"at":"2026-10-02T11:01:00Z","type":"send","wave":1,"replaces":8,"tokens":4321})));
        std::fs::write(&path, text).unwrap();
        mustard_core::io::mustard_skills::change(root, |held| {
            held.record("falta-teste", "created", "2026-10-02T10:00:00Z", "Acertou 3 de 3 com a skill.", &["src/alpha.rs".into()], ("demo", Some(1)), &[]);
            held.count_send(&["falta-teste".into()], &[vec!["falta-teste".into()], vec!["falta-teste".into()]]);
            held.record("parada", "created", "2026-10-02T10:00:00Z", "Ganho pequeno.", &[], ("antiga", None), &[]);
            held.record("parada", "retired", "2026-10-03T10:00:00Z", "", &[], ("demo", None), &[]);
            held.record("sem-ganho", "discarded", "2026-10-03T10:00:00Z", "Sem ganho.", &[], ("demo", None), &[]);
        })
        .unwrap();
        let usage = screen(root, "usage", &Params::default());
        let skills = usage["mustard_skills"].as_array().unwrap_or_else(|| panic!("{usage}"));
        assert_eq!(
            skills[0],
            json!({"name":"falta-teste","state":"active","tasks":2,"gain":"Acertou 3 de 3 com a skill.","wave_tokens":4321}),
            "{usage}"
        );
        assert_eq!((skills[1]["name"].clone(), skills[1]["state"].clone(), skills[1]["wave_tokens"].clone()), (json!("parada"), json!("retired"), Value::Null));
        assert_eq!((skills[2]["name"].clone(), skills[2]["state"].clone()), (json!("sem-ganho"), json!("discarded")));
    }

    #[test]
    fn the_usage_screen_shows_each_wave_model_effort_skills_reuse_and_failures() {
        let dir = fixture();
        let root = dir.path();
        let path = root.join(".claude/spec/demo/spec.ndjson");
        let mut text = std::fs::read_to_string(&path).unwrap();
        let skill = json!({"name":"add-run-command","when":"Comando novo.","path":"/x/.claude/skills/add-run-command/SKILL.md","stale":false});
        for row in [
            // A versão do envio com o pedido, e a do consumo, que leva só o que mudou.
            json!({"v":1,"id":40,"at":"2026-10-02T09:01:00Z","type":"send","wave":1,"role":"wave","replaces":8,"text":"Pedido da onda 1.",
                "model":"opus","effort":"xhigh","skills":[skill],"summary":3}),
            json!({"v":1,"id":41,"at":"2026-10-02T11:01:00Z","type":"send","wave":1,"replaces":40,"model_used":"claude-opus-5-5","effort_used":"high","tokens":1234}),
            // Onda 2: uma volta que ninguém assumiu, outra assumida, a revisão
            // que reprovou e a volta que quem conduz reprovou.
            json!({"v":1,"id":42,"at":"2026-10-03T10:00:00Z","type":"delivered","wave":2,"returned":true,"text":"Primeira volta.","files":[]}),
            json!({"v":1,"id":43,"at":"2026-10-03T10:30:00Z","type":"delivered","wave":2,"returned":true,"text":"Segunda volta.","files":["src/beta.rs"]}),
            json!({"v":1,"id":44,"at":"2026-10-03T10:31:00Z","type":"delivered","wave":2,"replaces":43,"text":"Segunda volta.","files":["src/beta.rs"]}),
            json!({"v":1,"id":45,"at":"2026-10-03T11:00:00Z","type":"verdict","author":"review","wave":2,"final":true,"result":"rejected","text":"Faltou o teste."}),
            json!({"v":1,"id":46,"at":"2026-10-03T12:00:00Z","type":"delivered","wave":2,"returned":true,"text":"Terceira volta.","files":["src/beta.rs"]}),
            json!({"v":1,"id":47,"at":"2026-10-03T12:05:00Z","type":"send","wave":2,"replaces":13,"rejected":{"delivered":46,"reason":"Faltou o arquivo."}}),
        ] {
            text.push_str(&format!("{row}\n"));
        }
        std::fs::write(&path, text).unwrap();
        let operations = mustard_core::io::spec_events::operations_file(root, "demo").unwrap();
        std::fs::create_dir_all(operations.parent().unwrap()).unwrap();
        let observed = [
            json!({"v":1,"id":1,"at":"2026-10-03T09:00:00Z","type":"call","command":"jev-items","filter":"jev:no_credit"}),
            json!({"v":1,"id":2,"at":"2026-10-03T09:01:00Z","type":"call","command":"jev-items","filter":"jev:no_credit"}),
            json!({"v":1,"id":3,"at":"2026-10-03T09:02:00Z","type":"call","command":"knowledge","knowledge_cached":true}),
            json!({"v":1,"id":4,"at":"2026-10-03T09:03:00Z","type":"call","command":"knowledge"}),
            json!({"v":1,"id":5,"at":"2026-10-03T09:04:00Z","type":"hook","author":"hook","hook":"command_guard","action":"block","tool":"Bash","reason":"apaga"}),
            json!({"v":1,"id":6,"at":"2026-10-03T09:05:00Z","type":"hook","author":"hook","hook":"subagent_inject","action":"warn","tool":"Agent","reason":"aviso"}),
            json!({"v":1,"id":7,"at":"2026-10-03T09:06:00Z","type":"call","command":"jev reserve","filter":"reserve","reserve_for":"task triage"}),
        ];
        std::fs::write(&operations, observed.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n").unwrap();

        let usage = screen(root, "usage", &Params::default());
        let spec = &usage["spec"];
        let waves = spec["waves"].as_array().unwrap_or_else(|| panic!("{usage}"));
        assert_eq!(waves.iter().map(|row| row["wave"].clone()).collect::<Vec<_>>(), [json!(2), json!(1)], "newest dispatch first: {usage}");
        let first = &waves[1];
        assert_eq!(first["model_used"], "claude-opus-5-5", "{usage}");
        assert_eq!((first["model_asked"].clone(), first["effort_asked"].clone()), (json!("opus"), json!("xhigh")), "{usage}");
        assert_eq!(first["effort_used"], "high", "the effort that held at the launch: {usage}");
        assert_eq!(first["skills"], json!(["add-run-command"]), "{usage}");
        assert_eq!(first["summary_reused"], true, "{usage}");
        assert_eq!(first["tokens"], 1234, "{usage}");
        let second = &waves[0];
        assert_eq!((second["refused_returns"].clone(), second["review_rejections"].clone()), (json!(2), json!(1)), "{usage}");
        assert!(second["model_used"].is_null() && second["effort_asked"].is_null() && second["effort_used"].is_null(), "nothing recorded, nothing shown: {usage}");
        assert_eq!(spec["models"], json!([{"model":"claude-opus-5-5","effort":"xhigh","waves":1}]), "{usage}");
        assert_eq!(spec["reuse"], json!({"summaries":1,"knowledge_calls":2,"knowledge_cached":1}), "{usage}");
        assert_eq!(spec["failures"]["jev"], json!({"calls":2,"reasons":{"no_credit":2},"reserves":1}), "the reserve counts apart: {usage}");
        assert_eq!(spec["failures"]["hook_blocks"], json!({"total":1,"by_hook":{"command_guard":1}}), "{usage}");
        assert_eq!((spec["failures"]["refused_returns"].clone(), spec["failures"]["review_rejections"].clone()), (json!(2), json!(1)), "{usage}");
        for secret in ["Faltou o arquivo.", "Faltou o teste.", "Primeira volta.", "apaga", "/x/.claude"] {
            assert!(!usage.to_string().contains(secret), "{secret} is a text, never a number of the usage screen");
        }
    }
}
