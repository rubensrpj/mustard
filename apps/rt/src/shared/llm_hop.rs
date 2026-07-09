//! `llm_hop` — the OPT-IN LLM selection hop of the `feature` retrieval funnel:
//! ONE (max two) `claude` Haiku call that PICKS files from the deterministic
//! candidate pool. It never reads code and never re-ranks by itself — the
//! deterministic funnel supplies ~25 candidates WITH evidence, the model only
//! chooses among them (any hallucinated path is discarded on validation).
//!
//! Opt-in resolution (env → config → default, the gate cascade):
//! `MUSTARD_RETRIEVAL_HOP` env var when set (the per-run kill-switch), else
//! `mustard.json#retrieval.hop`; only the literal `haiku` enables the hop —
//! the default is OFF and the `feature` output stays byte-identical to the
//! deterministic funnel.
//!
//! FAIL-OPEN by construction, mirroring [`crate::shared::translate`]: `claude`
//! absent, spawn error, non-zero exit, hard ~45s timeout, or malformed JSON
//! all yield `None` — the caller keeps the deterministic `insumos` list; the
//! hop can only ever improve the answer, never break the query. The spawn
//! runs in a NEUTRAL temp cwd (no `.claude/` → no project hooks → no
//! recursion into this very harness), with `--no-session-persistence` (no
//! Recents pollution) and `--output-format json` (usage rides along); the
//! prompt goes via stdin.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

/// The pinned hop model — cheap, fast, and only ever CHOOSING from a menu.
const MODEL: &str = "claude-haiku-4-5-20251001";

/// Hard wall-clock ceiling for one `claude` call; past it the child is killed
/// and the hop degrades to the deterministic list.
const TIMEOUT_MS: u64 = 45_000;

/// Poll interval of the timeout wait loop.
const POLL_MS: u64 = 100;

/// Max files one hop call may select (the insumos operating point).
const MAX_PICKS: usize = 10;

/// Max matched terms rendered per candidate evidence line (prompt budget).
const TERMS_SHOWN: usize = 6;

/// The resolved hop mode: `Off` keeps the funnel fully deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HopMode {
    Off,
    Haiku,
}

/// Resolve the hop mode for `root`: the `MUSTARD_RETRIEVAL_HOP` env var when
/// set and non-blank (kill-switch, wins over config), else
/// `mustard.json#retrieval.hop`, else off. Only the literal `haiku` (case-
/// insensitive) enables the hop — any other value is OFF (fail-closed to the
/// deterministic funnel, never to a surprise spawn).
pub fn mode(root: &Path) -> HopMode {
    let value = match std::env::var("MUSTARD_RETRIEVAL_HOP") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_ascii_lowercase(),
        _ => mustard_core::ProjectConfig::load(root).retrieval_hop().unwrap_or_default(),
    };
    if value == "haiku" {
        HopMode::Haiku
    } else {
        HopMode::Off
    }
}

/// One candidate row of the fused pool the hop selects from: the file plus
/// its deterministic evidence — which list(s) surfaced it (`rank` / `digest`
/// / `both`), the 1-based position in each, and the matched terms that carry
/// it.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub file: String,
    pub source: &'static str,
    pub rank_pos: Option<usize>,
    pub digest_pos: Option<usize>,
    pub terms: Vec<String>,
}

/// One selected file: a candidate path (validated ∈ pool) + the model's ≤8-word
/// justification.
#[derive(Debug, Clone)]
pub struct HopPick {
    pub file: String,
    pub why: String,
}

/// The outcome of one `claude` call: the validated picks (possibly empty), the
/// optional re-query terms, and the usage/latency audit for the caller's
/// telemetry.
#[derive(Debug, Clone)]
pub struct HopCall {
    pub files: Vec<HopPick>,
    pub requery: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub duration_ms: u64,
}

/// Run ONE selection call over `candidates` with the default `claude` binary.
/// `None` on any failure (fail-open — the caller keeps the deterministic list).
pub fn select(request: &str, gloss: Option<&str>, candidates: &[Candidate], allow_requery: bool) -> Option<HopCall> {
    select_with_binary("claude", request, gloss, candidates, allow_requery)
}

/// [`select`] with an explicit binary — the injectable seam the fail-open
/// tests use (a bogus binary must yield `None`, never an error or a panic).
pub fn select_with_binary(
    binary: &str,
    request: &str,
    gloss: Option<&str>,
    candidates: &[Candidate],
    allow_requery: bool,
) -> Option<HopCall> {
    if candidates.is_empty() {
        return None;
    }
    let prompt = build_prompt(request, gloss, candidates, allow_requery);
    let cwd = neutral_cwd()?;
    // The prompt reaches the child as stdin FROM A REAL FILE, not a pipe:
    // measured on Windows, the `claude` CLI deadlocks against a Rust std
    // anonymous pipe once the prompt exceeds the ~4KB pipe buffer (the write
    // blocks, stdin never closes, the child waits for EOF — mutual wait until
    // the timeout). A file handle has no buffer ceiling and needs no writer
    // thread; the child still reads plain stdin to EOF.
    // Unique per process AND per call: product calls are sequential, but the
    // live tests run concurrently in one test binary — a shared name would
    // race the prompt files.
    static CALL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = CALL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let prompt_path = cwd.join(format!("prompt-{}-{seq}.txt", std::process::id()));
    std::fs::write(&prompt_path, prompt.as_bytes()).ok()?;
    let stdin_file = std::fs::File::open(&prompt_path).ok()?;
    let started = std::time::Instant::now();
    let mut child = Command::new(binary)
        .args(["-p", "--model", MODEL, "--no-session-persistence", "--output-format", "json"])
        .current_dir(&cwd)
        .stdin(Stdio::from(stdin_file))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Drain stdout concurrently so a large answer can never fill the pipe and
    // deadlock the timeout wait below.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    // Hard timeout: poll `try_wait` until exit or the ceiling, then kill.
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX) >= TIMEOUT_MS {
                    trace("timeout — killing the child");
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
            }
            Err(_) => break None,
        }
    };
    let out = reader.join().ok();
    let _ = std::fs::remove_file(&prompt_path);
    let out = out?;
    let status = status?;
    trace(&format!("exit={} bytes={} after {}ms", status.code().unwrap_or(-1), out.len(), started.elapsed().as_millis()));
    if !status.success() {
        return None;
    }
    let stdout_text = String::from_utf8_lossy(&out);
    let Some((text, input_tokens, output_tokens)) = parse_envelope(&stdout_text) else {
        trace(&format!("envelope parse failed: {}", &stdout_text.chars().take(300).collect::<String>()));
        return None;
    };
    let Some((files, requery)) = parse_selection(&text, candidates) else {
        trace(&format!("selection parse failed: {}", &text.chars().take(300).collect::<String>()));
        return None;
    };
    trace(&format!("picked {} file(s), requery={:?}", files.len(), requery));
    Some(HopCall {
        files,
        requery: if allow_requery { requery } else { None },
        input_tokens,
        output_tokens,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// The NEUTRAL working directory for the spawn: a temp-dir child with NO
/// `.claude/` inside, so no project hook loads in the child session (the
/// anti-recursion guarantee). `None` when the dir cannot be created.
fn neutral_cwd() -> Option<std::path::PathBuf> {
    let dir = std::env::temp_dir().join("mustard-hop-neutral");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Stage tracing to stderr, gated on `MUSTARD_HOP_DEBUG` — stdout (the
/// product contract) never sees it. Diagnosing a silent fail-open (`None`)
/// otherwise requires guessing which rung degraded.
fn trace(msg: &str) {
    if std::env::var("MUSTARD_HOP_DEBUG").is_ok_and(|v| !v.trim().is_empty()) {
        eprintln!("hop: {msg}");
    }
}

/// Build the selection prompt (EN, technical): the request in both tongues,
/// one evidence line per candidate, and the STRICT output contract. Pure —
/// unit-tested without a spawn.
pub(crate) fn build_prompt(request: &str, gloss: Option<&str>, candidates: &[Candidate], allow_requery: bool) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("You are a code-retrieval selector for a software repository.\n\n");
    let _ = writeln!(out, "Developer request (original): {request}");
    let _ = writeln!(out, "Developer request (English): {}\n", gloss.unwrap_or(request));
    out.push_str(
        "Candidate files from a deterministic retrieval funnel (personalized-PageRank lexical ranker + BM25F digest). \
         Evidence per line: which list(s) surfaced it, 1-based position per list, matched terms:\n\n",
    );
    for (i, c) in candidates.iter().enumerate() {
        let mut ev: Vec<String> = Vec::new();
        if let Some(r) = c.rank_pos {
            ev.push(format!("rank#{r}"));
        }
        if let Some(d) = c.digest_pos {
            ev.push(format!("digest#{d}"));
        }
        if !c.terms.is_empty() {
            let shown: Vec<&str> = c.terms.iter().take(TERMS_SHOWN).map(String::as_str).collect();
            ev.push(format!("terms={}", shown.join(",")));
        }
        let _ = writeln!(out, "{:>2}. {} [{}]", i + 1, c.file, ev.join(" "));
    }
    let _ = write!(
        out,
        "\nTask: pick up to {MAX_PICKS} files a developer would open or edit to fulfil this request, \
         ordered by relevance (most likely first). Choose ONLY paths from the candidate list, copied verbatim. "
    );
    if allow_requery {
        out.push_str(
            "If the list clearly lacks the real target for this request, ALSO set \"requery\" to 2-4 English \
             identifier-style terms (lowercase code vocabulary likely used in the target files); otherwise set \"requery\" to null. ",
        );
    } else {
        out.push_str("This pool was already refined once — always set \"requery\" to null. ");
    }
    out.push_str(
        "\n\nReply with STRICT JSON only (no markdown fences, no prose):\n\
         {\"files\":[{\"file\":\"<candidate path>\",\"why\":\"<up to 8 words>\"}],\"requery\":\"<terms>\"|null}\n",
    );
    out
}

/// Parse the `claude --output-format json` envelope: the answer text (the
/// `result` string) + token usage. `None` on `is_error`, a missing result, or
/// malformed JSON. Input tokens fold the prompt-cache fields in so the audit
/// reports what the call actually consumed.
pub(crate) fn parse_envelope(stdout: &str) -> Option<(String, u64, u64)> {
    let start = stdout.find('{')?;
    let v: Value = serde_json::from_str(stdout[start..].trim()).ok()?;
    if v.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let text = v.get("result")?.as_str()?.to_string();
    let usage = v.get("usage");
    let tok = |k: &str| usage.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    let input = tok("input_tokens") + tok("cache_creation_input_tokens") + tok("cache_read_input_tokens");
    Some((text, input, tok("output_tokens")))
}

/// Parse the model's STRICT selection JSON out of `text` (tolerating fences /
/// prose around the outermost `{...}`), validating every `file` against the
/// candidate pool: a hallucinated path is dropped, duplicates collapse,
/// partial answers are accepted, picks cap at [`MAX_PICKS`]. Returns the
/// validated picks + the optional requery string. `None` only when no JSON
/// object parses at all.
pub(crate) fn parse_selection(text: &str, candidates: &[Candidate]) -> Option<(Vec<HopPick>, Option<String>)> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    let v: Value = serde_json::from_str(&text[start..=end]).ok()?;
    let allowed: BTreeSet<&str> = candidates.iter().map(|c| c.file.as_str()).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut picks: Vec<HopPick> = Vec::new();
    for row in v.get("files").and_then(Value::as_array).into_iter().flatten() {
        let Some(file) = row.get("file").and_then(Value::as_str) else {
            continue;
        };
        let norm = file.trim().replace('\\', "/");
        if !allowed.contains(norm.as_str()) || !seen.insert(norm.clone()) {
            continue;
        }
        let why = row.get("why").and_then(Value::as_str).unwrap_or("").trim().to_string();
        picks.push(HopPick { file: norm, why });
        if picks.len() >= MAX_PICKS {
            break;
        }
    }
    let requery = v
        .get("requery")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("null"))
        .map(str::to_string);
    Some((picks, requery))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(file: &str, source: &'static str, rank_pos: Option<usize>, digest_pos: Option<usize>, terms: &[&str]) -> Candidate {
        Candidate {
            file: file.to_string(),
            source,
            rank_pos,
            digest_pos,
            terms: terms.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn prompt_renders_request_evidence_and_contract() {
        let cands = vec![
            cand("src/a.cs", "both", Some(3), Some(1), &["contract", "installment"]),
            cand("src/b.cs", "rank", Some(1), None, &[]),
        ];
        let p = build_prompt("corrigir o cálculo", Some("fix the calculation"), &cands, true);
        assert!(p.contains("Developer request (original): corrigir o cálculo"), "{p}");
        assert!(p.contains("Developer request (English): fix the calculation"), "{p}");
        assert!(p.contains(" 1. src/a.cs [rank#3 digest#1 terms=contract,installment]"), "{p}");
        assert!(p.contains(" 2. src/b.cs [rank#1]"), "{p}");
        assert!(p.contains("\"requery\""), "{p}");
        assert!(p.contains("STRICT JSON"), "{p}");
        // Without a gloss the EN line echoes the original; the second call
        // forbids the requery.
        let p2 = build_prompt("add a field", None, &cands, false);
        assert!(p2.contains("Developer request (English): add a field"), "{p2}");
        assert!(p2.contains("already refined once"), "{p2}");
    }

    #[test]
    fn envelope_parses_result_and_usage_and_rejects_errors() {
        let out = r#"{"type":"result","is_error":false,"result":"{\"files\":[]}","usage":{"input_tokens":100,"cache_read_input_tokens":50,"output_tokens":20}}"#;
        let (text, input, output) = parse_envelope(out).expect("envelope parses");
        assert_eq!(text, r#"{"files":[]}"#);
        assert_eq!(input, 150, "cache fields fold into the input audit");
        assert_eq!(output, 20);
        // Preamble tolerated; is_error and missing result are None.
        assert!(parse_envelope("warmup\n{\"result\":\"x\"}").is_some());
        assert!(parse_envelope(r#"{"is_error":true,"result":"x"}"#).is_none());
        assert!(parse_envelope(r#"{"type":"result"}"#).is_none());
        assert!(parse_envelope("no json").is_none());
    }

    #[test]
    fn selection_validates_membership_dedups_and_accepts_partial() {
        let cands = vec![
            cand("src/a.cs", "rank", Some(1), None, &[]),
            cand("src/b.cs", "digest", None, Some(1), &[]),
        ];
        // Fenced answer, a hallucinated path, a duplicate, a backslash variant.
        let text = "```json\n{\"files\":[\
            {\"file\":\"src\\\\a.cs\",\"why\":\"main form\"},\
            {\"file\":\"src/ghost.cs\",\"why\":\"made up\"},\
            {\"file\":\"src/a.cs\",\"why\":\"dup\"},\
            {\"file\":\"src/b.cs\"}],\"requery\":null}\n```";
        let (picks, requery) = parse_selection(text, &cands).expect("selection parses");
        assert_eq!(picks.len(), 2, "hallucination + duplicate dropped: {picks:?}");
        assert_eq!(picks[0].file, "src/a.cs");
        assert_eq!(picks[0].why, "main form");
        assert_eq!(picks[1].file, "src/b.cs");
        assert_eq!(picks[1].why, "");
        assert!(requery.is_none());

        // A requery string survives; the literal "null" string does not.
        let (_, rq) = parse_selection(r#"{"files":[],"requery":"payable split installment"}"#, &cands).expect("parses");
        assert_eq!(rq.as_deref(), Some("payable split installment"));
        let (_, rq) = parse_selection(r#"{"files":[],"requery":"null"}"#, &cands).expect("parses");
        assert!(rq.is_none());
        // No JSON at all → None.
        assert!(parse_selection("i could not decide", &cands).is_none());
    }

    /// LIVE probe (ignored by default — needs a real `claude` on PATH): one
    /// tiny selection call end-to-end through the exact spawn path. Run with
    /// `cargo test -p mustard-rt llm_hop -- --ignored --nocapture`.
    #[test]
    #[ignore = "spawns the real claude CLI"]
    fn live_select_smoke() {
        let cands = vec![cand("src/a.cs", "rank", Some(1), None, &["alpha"])];
        let got = select("pick the only file", None, &cands, true);
        eprintln!("live smoke: {got:?}");
        assert!(got.is_some(), "live claude call failed");
    }

    /// LIVE probe at the REAL pool width (ignored): 25 long candidate paths →
    /// a ~4.5KB prompt, the size the sialia funnel produces. Discriminates a
    /// pipe-buffer / EOF hang from a logic failure.
    #[test]
    #[ignore = "spawns the real claude CLI"]
    fn live_select_smoke_wide() {
        let cands: Vec<Candidate> = (0..25)
            .map(|i| Candidate {
                file: format!("backend/Sialia.Backend/Application/Modules/v1/Payables/Services/SomeVeryLongFileName{i:02}.cs"),
                source: "both",
                rank_pos: Some(i + 1),
                digest_pos: Some(i + 1),
                terms: vec!["payable".into(), "split".into(), "installment".into(), "vencimento".into()],
            })
            .collect();
        let prompt = build_prompt("onde fica a lógica de desdobramento de contas a pagar", Some("where is the payable split logic"), &cands, true);
        eprintln!("prompt bytes: {}", prompt.len());
        let got = select("onde fica a lógica de desdobramento de contas a pagar", Some("where is the payable split logic"), &cands, true);
        eprintln!("live wide smoke: picks={:?}", got.as_ref().map(|c| c.files.len()));
        assert!(got.is_some(), "live wide claude call failed");
    }

    #[test]
    fn select_fails_open_when_the_binary_is_absent() {
        // A bogus binary must yield None (spawn error), never a panic or a
        // hang — the fail-open contract the feature funnel stands on.
        let cands = vec![cand("src/a.cs", "rank", Some(1), None, &[])];
        let got = select_with_binary("mustard-definitely-not-a-binary-xyz", "req", None, &cands, true);
        assert!(got.is_none(), "absent binary → None (deterministic list stands)");
        // An empty pool never spawns at all.
        assert!(select_with_binary("mustard-definitely-not-a-binary-xyz", "req", None, &[], true).is_none());
    }
}
