//! Explicit document addresses only. A quoted name or link is an author
//! reference, never a call edge or proof of the described behavior.
use std::collections::BTreeSet;
use std::path::{Component, Path};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    pub text: String,
    pub line: u64,
    pub kind: &'static str,
}

pub fn extract(text: &str) -> Vec<Reference> {
    let mut out = BTreeSet::new();
    let mut fence = None;
    for (at, line) in text.lines().enumerate() {
        let clean = line.trim_start();
        if let Some(marker) = clean.chars().next().filter(|c| matches!(c, '`' | '~')) {
            let width = clean.chars().take_while(|c| *c == marker).count();
            if width >= 3 {
                match fence {
                    None => fence = Some((marker, width)),
                    Some((open, n)) if open == marker && width >= n && clean.chars().skip(width).all(char::is_whitespace) => fence = None,
                    _ => {}
                }
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        let mut rest = line;
        while let Some((_, tail)) = rest.split_once("](") {
            let Some((target, after)) = tail.split_once(')') else { break };
            let target = target.trim().trim_matches(['<', '>']);
            if !target.is_empty() && !target.contains("://") && !target.starts_with('#') {
                out.insert(Reference { text: target.into(), line: at as u64 + 1, kind: "document-link" });
            }
            rest = after;
        }
        let mut rest = line;
        while let Some((_, tail)) = rest.split_once('`') {
            let Some((name, after)) = tail.split_once('`') else { break };
            if !name.is_empty() && !name.chars().any(char::is_whitespace) && name.len() <= 400 {
                out.insert(Reference { text: name.into(), line: at as u64 + 1, kind: "quoted-reference" });
            }
            rest = after;
        }
    }
    out.into_iter().collect()
}

/// Normalize a relative address without traversing above the project root.
/// External schemes, unsupported anchors and platform-specific paths stay
/// unresolved instead of being guessed.
pub fn address(document: &str, reference: &str) -> Option<(String, Option<u64>)> {
    if reference.contains('\\') || reference.contains(':') || reference.starts_with('/') {
        return None;
    }
    let (file, anchor) = reference.split_once('#').map_or((reference, None), |(file, anchor)| (file, Some(anchor)));
    if !file.contains('.') && !file.contains('/') {
        return None;
    }
    let line = if let Some(anchor) = anchor {
        let anchor = anchor.strip_prefix('L')?;
        let (first, last) = anchor.split_once("-L").map_or((anchor, None), |(first, last)| (first, Some(last)));
        let first = first.parse::<u64>().ok().filter(|line| *line > 0)?;
        if let Some(last) = last {
            last.parse::<u64>().ok().filter(|line| *line >= first)?;
        }
        Some(first)
    } else {
        None
    };
    let base = Path::new(document).parent().unwrap_or(Path::new(""));
    let mut parts: Vec<String> = Vec::new();
    for component in base.join(file).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.into()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| (parts.join("/"), line))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_references_outside_examples_and_local_safe_addresses_are_accepted() {
        let text = "# Recovery\nUse `restoreLedger` and [source](../src/store.rs#L12-L18).\n```text\n`fakeName`\n```\nPlain unquotedName is not a reference.\n";
        let found = extract(text);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|r| r.line == 2));
        assert_eq!(address("docs/recovery.md", "../src/store.rs#L12-L18"), Some(("src/store.rs".into(), Some(12))));
        for reference in
            ["../../outside.rs", "https://site.invalid/a.rs", "/tmp/source.rs", "src/a.rs#unknown", "src/a.rs#L12-garbage", "src/a.rs#L12-L2", "C:\\a.rs"]
        {
            assert!(address("docs/recovery.md", reference).is_none(), "{reference}");
        }
    }
}
