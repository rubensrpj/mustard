//! File-class discovery — which files are MACHINE-WRITTEN. One module, one
//! responsibility (mirrors graph.rs / extract.rs / mine.rs).
//!
//! Everything that names a tool, ecosystem or convention lives in the catalog
//! (`generated-markers.toml`, embedded at compile time like stopwords.toml);
//! this engine is three generic probes — a head-of-file string/regex search,
//! a path-glob match, and a line-length statistic — plus the repo's own
//! OVERRIDES (.gitattributes `linguist-generated`, .editorconfig
//! `generated_code`), which always beat the catalog in both directions.
//!
//! The verdict feeds the model additively (`Module::file_class` +
//! `Module::marker`); the digest demotes by class through the
//! module-qualified policy functions below ([`index_eligible`],
//! [`anchor_eligible`], [`index_weight`]). The model itself stays complete —
//! like the test-dirs discount, classification never hides a module from the
//! miner, it only shapes the digest projection.
//!
//! Tolerant like the rest of the crate: a catalog row that fails to compile
//! is discarded individually (same contract as the .scm queries), unreadable
//! override files yield no overrides, and a file matching nothing is simply
//! hand-written.

use globset::{GlobBuilder, GlobMatcher};
use regex::{Regex, RegexBuilder};
use std::path::Path;
use std::sync::OnceLock;

/// The four file classes — engine vocabulary, not tool names. WHICH files
/// fall in a class is catalog + override data, never logic here.
pub const GENERATED: &str = "generated";
pub const VENDORED: &str = "vendored";
pub const LOCKFILE: &str = "lockfile";
pub const MINIFIED: &str = "minified";

/// A positive classification: the class plus the marker that decided it —
/// the catalog literal/regex/glob or the override attribute, kept verbatim as
/// provenance so a surprising classification is explainable.
pub struct Classification {
    pub class: String,
    pub marker: String,
}

/// Classify one source file. `rel_path` is the /-normalized path relative to
/// the scanned root; `None` means hand-written. Probe order (first hit wins,
/// deterministic): overrides (always win, both directions), lockfile
/// basename, path globs, head content markers, minified statistic — explicit
/// provenance before arithmetic.
pub fn classify(rel_path: &str, content: &str, overrides: &Overrides) -> Option<Classification> {
    if let Some(rule) = overrides.decide(rel_path) {
        if !rule.generated {
            return None;
        }
        return Some(Classification { class: GENERATED.to_string(), marker: rule.label.clone() });
    }
    let cat = catalog();

    let basename = rel_path.rsplit('/').next().unwrap_or(rel_path).to_ascii_lowercase();
    if cat.lockfiles.iter().any(|l| *l == basename) {
        return Some(Classification { class: LOCKFILE.to_string(), marker: basename });
    }

    if let Some(p) = cat.paths.iter().find(|p| p.matcher.is_match(rel_path)) {
        return Some(Classification { class: p.class.clone(), marker: p.label.clone() });
    }

    // Content markers live in the leading banner; scanning a fixed window of
    // head lines covers the first comment block of every comment syntax
    // without the engine knowing any of them (comment grammars are language
    // data, which must never live here).
    let head: String = content.lines().take(cat.head_lines).collect::<Vec<_>>().join("\n");
    let head_lower = head.to_lowercase();
    for m in &cat.markers {
        let hit = match (&m.literal, &m.regex) {
            (Some(lit), _) => head_lower.contains(lit.as_str()),
            (None, Some(re)) => re.is_match(&head),
            _ => false,
        };
        if hit {
            return Some(Classification { class: m.class.clone(), marker: m.label.clone() });
        }
    }

    if is_minified(content, cat) {
        return Some(Classification {
            class: MINIFIED.to_string(),
            marker: format!("avg_line_len>{}", cat.minified_avg_line_len),
        });
    }
    None
}

// --- digest-facing class policy ----------------------------------------------
// The digest consumes these module-qualified (crate::classify::…), never
// through a digest-local wrapper.

/// Classes whose terms STAY in the digest index (demoted by [`index_weight`]).
/// Lockfiles and minified output are pure machine noise and leave the index
/// entirely.
pub fn index_eligible(class: &str) -> bool {
    class != LOCKFILE && class != MINIFIED
}

/// Only hand-written modules (empty class) may surface as term samples,
/// anchor files, hubs or touchpoints — a machine-written file is never the
/// file a caller should read or edit.
pub fn anchor_eligible(class: &str) -> bool {
    class.is_empty()
}

/// Digest index weight of `count` occurrences inside a module of `class`:
/// hand-written counts pass through; machine-written ones are scaled by the
/// catalog multiplier, flooring at 1 so the term remains findable (a query
/// landing only there is answered with reason `generated_only`, not a miss).
pub fn index_weight(count: usize, class: &str) -> usize {
    if anchor_eligible(class) {
        count
    } else {
        (((count as f64) * catalog().index_multiplier).floor() as usize).max(1)
    }
}

// --- repo overrides ------------------------------------------------------------

/// Repo-declared classification overrides, loaded once per scan from the root
/// `.gitattributes` (`linguist-generated` / `-linguist-generated` /
/// `linguist-generated=true|false`) and `.editorconfig`
/// (`generated_code = true|false`). Overrides always WIN over the catalog in
/// both directions: a positive mark classes the file generated with no probe
/// run; a negative mark pins it hand-written even when a banner matches.
/// Within and across files, the LAST matching rule wins (mirroring git's own
/// attribute semantics); `.gitattributes` rules are appended after the
/// `.editorconfig` ones, so git's metadata beats the editor's when both speak.
pub struct Overrides {
    rules: Vec<OverrideRule>,
}

struct OverrideRule {
    matcher: GlobMatcher,
    generated: bool,
    /// Provenance label, e.g. ".gitattributes:linguist-generated".
    label: String,
}

impl Overrides {
    pub fn load(root: &Path) -> Overrides {
        let mut rules = Vec::new();
        if let Ok(txt) = std::fs::read_to_string(root.join(".editorconfig")) {
            parse_editorconfig(&txt, &mut rules);
        }
        if let Ok(txt) = std::fs::read_to_string(root.join(".gitattributes")) {
            parse_gitattributes(&txt, &mut rules);
        }
        Overrides { rules }
    }

    /// The override verdict for a path, if any rule matches (last match wins).
    fn decide(&self, rel: &str) -> Option<&OverrideRule> {
        self.rules.iter().rev().find(|r| r.matcher.is_match(rel))
    }
}

/// `pattern attr…` lines; only the linguist-generated attribute is read.
fn parse_gitattributes(txt: &str, rules: &mut Vec<OverrideRule>) {
    for line in txt.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else { continue };
        let mut generated: Option<bool> = None;
        for attr in parts {
            match attr {
                "linguist-generated" | "linguist-generated=true" => generated = Some(true),
                "-linguist-generated" | "linguist-generated=false" => generated = Some(false),
                _ => {}
            }
        }
        if let (Some(generated), Some(matcher)) = (generated, compile_glob(&anchor(pattern))) {
            rules.push(OverrideRule { matcher, generated, label: ".gitattributes:linguist-generated".to_string() });
        }
    }
}

/// `[section]` globs with `generated_code = true|false` pairs; every other
/// key is ignored.
fn parse_editorconfig(txt: &str, rules: &mut Vec<OverrideRule>) {
    let mut section: Option<String> = None;
    for line in txt.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = Some(line[1..line.len() - 1].to_string());
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if !key.trim().eq_ignore_ascii_case("generated_code") {
            continue;
        }
        let generated = match value.trim().to_ascii_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => continue,
        };
        if let Some(matcher) = section.as_deref().map(anchor).and_then(|g| compile_glob(&g)) {
            rules.push(OverrideRule { matcher, generated, label: ".editorconfig:generated_code".to_string() });
        }
    }
}

/// Root-relative glob for an attribute/section pattern, following both
/// formats' shared convention: a pattern WITHOUT `/` matches the basename at
/// any depth; one WITH `/` is rooted (the leading `/` our rel paths never
/// carry is stripped).
fn anchor(pattern: &str) -> String {
    if pattern.contains('/') {
        pattern.trim_start_matches('/').to_string()
    } else {
        format!("**/{pattern}")
    }
}

// --- catalog --------------------------------------------------------------------

struct MarkerDef {
    class: String,
    /// Lowercased literal sought in the lowercased head window.
    literal: Option<String>,
    /// Case-insensitive compiled regex, run over the verbatim head window.
    regex: Option<Regex>,
    /// Verbatim catalog text — the marker provenance stored on the module.
    label: String,
}

struct PathDef {
    class: String,
    matcher: GlobMatcher,
    label: String,
}

struct Catalog {
    head_lines: usize,
    index_multiplier: f64,
    minified_avg_line_len: usize,
    minified_min_bytes: usize,
    /// Lowercased basenames.
    lockfiles: Vec<String>,
    markers: Vec<MarkerDef>,
    paths: Vec<PathDef>,
}

/// Parsed once per process. A malformed embedded file is a programmer error
/// caught by any test run — same contract as `digest::stopwords` over
/// stopwords.toml; individual rows that fail to compile are discarded.
fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| parse_catalog(include_str!("../generated-markers.toml")))
}

fn parse_catalog(src: &str) -> Catalog {
    let v: toml::Value = toml::from_str(src).expect("generated-markers.toml is not valid TOML");
    let int = |val: Option<&toml::Value>, default: i64| val.and_then(|x| x.as_integer()).unwrap_or(default) as usize;
    let head_lines = int(v.get("head_lines"), 30);
    let index_multiplier = v
        .get("index")
        .and_then(|i| i.get("generated_multiplier"))
        .and_then(|x| x.as_float())
        .unwrap_or(0.25);
    let minified_avg_line_len = int(v.get("minified").and_then(|m| m.get("avg_line_len")), 400);
    let minified_min_bytes = int(v.get("minified").and_then(|m| m.get("min_bytes")), 1024);
    let lockfiles: Vec<String> = v
        .get("lockfiles")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|e| e.as_str().map(|s| s.to_ascii_lowercase())).collect())
        .unwrap_or_default();

    let mut markers = Vec::new();
    if let Some(arr) = v.get("marker").and_then(|x| x.as_array()) {
        for m in arr {
            let Some(class) = m.get("class").and_then(|x| x.as_str()) else { continue };
            if let Some(lit) = m.get("literal").and_then(|x| x.as_str()) {
                markers.push(MarkerDef {
                    class: class.to_string(),
                    literal: Some(lit.to_lowercase()),
                    regex: None,
                    label: lit.to_string(),
                });
            } else if let Some(pat) = m.get("regex").and_then(|x| x.as_str()) {
                // A pattern that fails to compile is discarded individually —
                // never fatal, same contract as the .scm queries.
                if let Ok(re) = RegexBuilder::new(pat).case_insensitive(true).build() {
                    markers.push(MarkerDef {
                        class: class.to_string(),
                        literal: None,
                        regex: Some(re),
                        label: pat.to_string(),
                    });
                }
            }
        }
    }

    let mut paths = Vec::new();
    if let Some(arr) = v.get("path").and_then(|x| x.as_array()) {
        for p in arr {
            let (Some(class), Some(glob)) =
                (p.get("class").and_then(|x| x.as_str()), p.get("glob").and_then(|x| x.as_str()))
            else {
                continue;
            };
            if let Some(matcher) = compile_glob(glob) {
                paths.push(PathDef { class: class.to_string(), matcher, label: glob.to_string() });
            }
        }
    }

    Catalog { head_lines, index_multiplier, minified_avg_line_len, minified_min_bytes, lockfiles, markers, paths }
}

/// Compile a glob ASCII-case-insensitively (paths arrive /-normalized, but
/// their casing follows the filesystem); an invalid pattern is discarded.
fn compile_glob(pattern: &str) -> Option<GlobMatcher> {
    GlobBuilder::new(pattern).case_insensitive(true).build().ok().map(|g| g.compile_matcher())
}

/// Machine-compacted output: enough bytes for the statistic to mean anything
/// and an average line longer than any hand-written convention tolerates.
fn is_minified(content: &str, cat: &Catalog) -> bool {
    if content.len() < cat.minified_min_bytes {
        return false;
    }
    let lines = content.lines().count().max(1);
    content.len() / lines > cat.minified_avg_line_len
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests exercise the GENERIC engine only — every tool/ecosystem
    // marker assertion lives in tests/generated_class.rs, outside src/, so no
    // catalog vocabulary leaks in here.

    fn overrides_from(editorconfig: Option<&str>, gitattributes: Option<&str>) -> Overrides {
        let mut rules = Vec::new();
        if let Some(txt) = editorconfig {
            parse_editorconfig(txt, &mut rules);
        }
        if let Some(txt) = gitattributes {
            parse_gitattributes(txt, &mut rules);
        }
        Overrides { rules }
    }

    #[test]
    fn override_pins_both_directions() {
        let ov = overrides_from(None, Some("src/made.xyz linguist-generated\nsrc/hand.xyz -linguist-generated\n"));
        let made = classify("src/made.xyz", "plain content\n", &ov).expect("positive override classifies");
        assert_eq!(made.class, GENERATED);
        assert!(made.marker.contains(".gitattributes"), "provenance names the override file: {}", made.marker);
        // The negative mark wins even before any catalog probe runs.
        assert!(classify("src/hand.xyz", "plain content\n", &ov).is_none());
    }

    #[test]
    fn gitattributes_beats_editorconfig_and_last_match_wins() {
        // .editorconfig says generated, .gitattributes (appended later, so it
        // wins the last-match scan) says hand-written.
        let ov = overrides_from(
            Some("[*.xyz]\ngenerated_code = true\n"),
            Some("*.xyz -linguist-generated\n"),
        );
        assert!(classify("a/b/c.xyz", "x\n", &ov).is_none(), "git metadata must win");

        // Within one file, the later line overrides the earlier one.
        let ov = overrides_from(None, Some("*.xyz linguist-generated\nsrc/keep.xyz -linguist-generated\n"));
        assert!(classify("src/keep.xyz", "x\n", &ov).is_none());
        assert_eq!(classify("src/other.xyz", "x\n", &ov).expect("still generated").class, GENERATED);
    }

    #[test]
    fn slashless_patterns_match_basenames_at_any_depth() {
        let ov = overrides_from(Some("[*.xyz]\ngenerated_code = true\n"), None);
        assert_eq!(classify("deep/nested/dir/file.xyz", "x\n", &ov).expect("matched").class, GENERATED);
        // A rooted (slashed) pattern only matches under its own path.
        let ov = overrides_from(None, Some("gen/out.xyz linguist-generated\n"));
        assert!(classify("other/gen/out.xyz", "x\n", &ov).is_none(), "rooted pattern must not float");
        assert!(classify("gen/out.xyz", "x\n", &ov).is_some());
    }

    #[test]
    fn minified_statistic_requires_both_thresholds() {
        let ov = overrides_from(None, None);
        // One enormous line: over min_bytes, avg way past the threshold.
        let packed = "x".repeat(4096);
        let got = classify("src/blob.xyz", &packed, &ov).expect("minified");
        assert_eq!(got.class, MINIFIED);
        assert!(got.marker.starts_with("avg_line_len"), "statistic provenance: {}", got.marker);
        // Same density but under min_bytes: too small to call.
        assert!(classify("src/tiny.xyz", &"x".repeat(500), &ov).is_none());
        // Plenty of bytes but ordinary lines: hand-written.
        let normal = "let value = 1;\n".repeat(300);
        assert!(classify("src/normal.xyz", &normal, &ov).is_none());
    }

    #[test]
    fn digest_policy_truth_table() {
        // Index: generated/vendored stay (demoted), lockfile/minified leave.
        assert!(index_eligible(""));
        assert!(index_eligible(GENERATED));
        assert!(index_eligible(VENDORED));
        assert!(!index_eligible(LOCKFILE));
        assert!(!index_eligible(MINIFIED));
        // Anchors: hand-written only.
        assert!(anchor_eligible(""));
        for class in [GENERATED, VENDORED, LOCKFILE, MINIFIED] {
            assert!(!anchor_eligible(class), "{class} must never anchor");
        }
        // Weight: pass-through for hand-written, demoted-but-present otherwise.
        assert_eq!(index_weight(7, ""), 7);
        assert_eq!(index_weight(1, GENERATED), 1, "floor keeps the term findable");
        assert!(index_weight(100, GENERATED) < 100, "machine occurrences never dominate");
    }
}
