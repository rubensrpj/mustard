//! Verbatim non-code evidence. Text is searchable, never a generated account
//! of behavior. The resource registry owns format and exclusion choices.
use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct File {
    pub path: String,
    pub blob: String,
    pub sha256: String,
    pub kind: String,
    pub issue: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Section {
    pub line: u64,
    pub end_line: u64,
    pub title: String,
    pub text: String,
}

#[derive(Deserialize)]
pub struct Format {
    extensions: Vec<String>,
    pub kind: String,
    #[serde(default)]
    pub outline: bool,
}

#[derive(Deserialize)]
pub struct Registry {
    pub max_bytes: u64,
    sensitive_keys: Vec<String>,
    sensitive_markers: Vec<String>,
    exclude: Vec<String>,
    format: Vec<Format>,
    #[serde(skip)]
    excluded: Option<GlobSet>,
}

impl Registry {
    pub fn load() -> Result<Self, String> {
        let mut registry: Self = toml::from_str(include_str!("resources.toml")).map_err(|err| err.to_string())?;
        let mut builder = GlobSetBuilder::new();
        for pattern in &registry.exclude {
            builder.add(Glob::new(pattern).map_err(|err| err.to_string())?);
        }
        registry.excluded = Some(builder.build().map_err(|err| err.to_string())?);
        Ok(registry)
    }

    pub fn format(&self, file: &str) -> Option<&Format> {
        if self.excluded.as_ref().is_some_and(|set| set.is_match(file)) || crate::domain::ast::is_test_path(file) {
            return None;
        }
        let extension = Path::new(file).extension()?.to_str()?.to_ascii_lowercase();
        self.format.iter().find(|format| format.extensions.contains(&extension))
    }

    /// Exclude likely credential-bearing text before persisting any excerpt.
    /// This is a conservative native filter, not a universal secret detector.
    pub fn sensitive(&self, text: &str) -> bool {
        if self.sensitive_markers.iter().any(|marker| text.contains(marker)) {
            return true;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            return self.sensitive_value(&value);
        }
        text.lines().any(|line| {
            let Some((key, value)) = line.split_once(':').or_else(|| line.split_once('=')) else { return false };
            self.sensitive_key(key.trim().trim_matches(['\"', '\''])) && !value.trim().trim_matches(['\"', '\'']).is_empty()
        })
    }

    fn sensitive_key(&self, key: &str) -> bool {
        let folded = |word: &str| word.chars().filter(char::is_ascii_alphanumeric).flat_map(char::to_lowercase).collect::<String>();
        self.sensitive_keys.iter().any(|sensitive| folded(sensitive) == folded(key))
    }

    fn sensitive_value(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(object) => object
                .iter()
                .any(|(key, value)| self.sensitive_key(key) && value.as_str().is_some_and(|text| !text.trim().is_empty()) || self.sensitive_value(value)),
            serde_json::Value::Array(values) => values.iter().any(|value| self.sensitive_value(value)),
            _ => false,
        }
    }
}

/// Bound an excerpt at a line boundary, retaining the whole accepted file in
/// the database. Fenced examples do not create documentation headings.
pub fn sections(text: &str, outline: bool, file: &str) -> Vec<Section> {
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut bytes = 0;
    let mut title = file.to_string();
    let mut fence = None;
    for (i, line) in lines.iter().enumerate() {
        let clean = line.trim();
        let heading = if outline && fence.is_none() { heading(clean) } else { None };
        if i > start && (heading.is_some() || i - start >= 32 || bytes + line.len() > 1800) {
            push_section(&mut out, &lines, start, i, &title);
            start = i;
            bytes = 0;
        }
        if let Some(heading) = heading {
            title = heading.to_string();
        }
        if outline {
            let marker = clean.chars().next().filter(|c| matches!(c, '`' | '~'));
            if let Some(marker) = marker {
                let count = clean.chars().take_while(|c| *c == marker).count();
                if count >= 3 {
                    match fence {
                        None => fence = Some((marker, count)),
                        Some((open, width)) if open == marker && count >= width && clean.chars().skip(count).all(char::is_whitespace) => fence = None,
                        _ => {}
                    }
                }
            }
        }
        bytes += line.len();
    }
    push_section(&mut out, &lines, start, lines.len(), &title);
    out
}

fn heading(line: &str) -> Option<&str> {
    let count = line.chars().take_while(|c| *c == '#').count();
    let rest = line.get(count..)?;
    (count > 0 && count <= 6 && rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

fn push_section(out: &mut Vec<Section>, lines: &[&str], start: usize, end: usize, title: &str) {
    let text = lines[start..end].concat();
    if !text.trim().is_empty() {
        out.push(Section { line: start as u64 + 1, end_line: end as u64, title: title.into(), text });
    }
}

pub fn query_terms(query: &str, languages: &crate::domain::normalize::Languages) -> Vec<Vec<String>> {
    super::retrieval::Terms::of(query, languages).asked
}

/// Center compact previews on the most specific matching word, rather than
/// hiding a relevant option behind the beginning of a long excerpt.
pub fn preview(section: &Section, terms: &[Vec<String>], languages: &crate::domain::normalize::Languages) -> (String, u64, bool) {
    preview_bounded(section,terms,languages,600)
}

pub fn preview_bounded(section: &Section, terms: &[Vec<String>], languages: &crate::domain::normalize::Languages,max:usize) -> (String,u64,bool) {
    let max=max.max(1);
    let chars: Vec<_> = section.text.char_indices().collect();
    if chars.len() <= max {
        return (section.text.clone(), section.line, false);
    }
    let mut normalizer = crate::domain::normalize::Normalizer::new(languages);
    let mut word_start = 0;
    let mut best = (0, 0);
    for (end, ch) in section.text.char_indices().chain(std::iter::once((section.text.len(), ' '))) {
        if ch.is_alphanumeric() || ch == '_' {
            continue;
        }
        if end > word_start {
            let forms: std::collections::BTreeSet<_> = normalizer.forms(&section.text[word_start..end]).into_iter().flatten().collect();
            let specificity = terms.iter().flatten().filter(|term| forms.contains(*term)).map(String::len).max().unwrap_or(0);
            if specificity > best.0 {
                best = (specificity, word_start);
            }
        }
        word_start = end + ch.len_utf8();
    }
    let anchor = chars.partition_point(|(at, _)| *at < best.1);
    let start = anchor.saturating_sub(100.min(max/3)).min(chars.len() - max);
    let end = start + max;
    let from = chars[start].0;
    let to = chars.get(end).map_or(section.text.len(), |(at, _)| *at);
    let line = section.line + section.text[..from].bytes().filter(|byte| *byte == b'\n').count() as u64;
    (format!("{}{}{}", if from > 0 { "…" } else { "" }, &section.text[from..to], if to < section.text.len() { "…" } else { "" }), line, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_keeps_format_rules_and_sensitive_or_generated_paths_out_of_the_engine() {
        let registry = Registry::load().unwrap();
        assert_eq!(registry.format("docs/Rules.MD").unwrap().kind, "documentation");
        assert!(registry.format("schema/data.sql").is_some());
        for path in
            [".env.json", "src/.env.production.json", "secrets.json", "cfg/credentials.json", "package-lock.json", "build/config.json", "tests/config.json"]
        {
            assert!(registry.format(path).is_none(), "{path}");
        }
    }

    #[test]
    fn excerpts_preserve_lines_and_text_without_promoting_fenced_example_headings() {
        let text = "# Orders\n\nRestore approved order.\n```text\n# Fake capability\n```\n## Archive\nDo not delete.\n";
        let sections = sections(text, true, "rules.md");
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Orders");
        assert_eq!((sections[0].line, sections[0].end_line), (1, 6));
        assert_eq!(sections[1].title, "Archive");
        assert_eq!(sections.iter().map(|s| s.text.as_str()).collect::<String>(), text);
    }

    #[test]
    fn long_sections_split_without_discarding_the_tail_or_splitting_unicode() {
        let text = format!("# Restore\n{}終点", "Preservar aprovação.\n".repeat(300));
        let sections = sections(&text, true, "rules.md");
        assert!(sections.len() > 4);
        assert_eq!(sections.iter().map(|s| s.text.as_str()).collect::<String>(), text);
        assert_eq!(sections.last().unwrap().end_line, text.lines().count() as u64);
    }

    #[test]
    fn compact_preview_includes_a_matching_option_late_in_a_long_unicode_line() {
        let section = Section {
            line: 4,
            end_line: 4,
            title: "configuration".into(),
            text: format!("{} approvalRevisionKey=true {}", "á".repeat(1500), "文".repeat(1200)),
        };
        let languages = crate::domain::normalize::Languages::new(["en-US"]);
        let (text, line, compacted) = preview(&section, &query_terms("approvalRevisionKey", &languages), &languages);
        assert!(text.contains("approvalRevisionKey=true"), "{text}");
        assert!(text.chars().count() <= 602);
        assert_eq!(line, 4);
        assert!(compacted);
    }

    #[test]
    fn credentials_are_excluded_by_content_even_when_the_filename_looks_like_normal_configuration() {
        let registry = Registry::load().unwrap();
        for text in [
            r#"{"type":"service_account","private_key":"fixture-not-a-key"}"#,
            r#"{"nested":{"apiKey":"fixture"}}"#,
            "client_secret: fixture\n",
            "password = 'fixture'\n",
            "arbitrary: -----BEGIN PRIVATE KEY-----\n",
        ] {
            assert!(registry.sensitive(text));
        }
        assert!(!registry.sensitive(r#"{"private":true,"passwordPolicy":{"minLength":8}}"#));
    }
}
