//! Explicit author metadata, extracted before comments lose their line breaks.
//! No business rule is inferred from an identifier or from ordinary prose.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub tag: String,
    pub text: String,
    pub line: u64,
    pub end_line: u64,
}

/// The caller supplies comment text with markers removed, preserving every
/// line, including blanks. Tags must begin a line; email, examples embedded
/// in prose and unknown tags cannot become declarations of business intent.
pub fn parse(text: &str, first_line: u64) -> Vec<Annotation> {
    let mut result: Vec<Annotation> = Vec::new();
    let mut active = false;
    let mut fence: Option<&str> = None;
    for (offset, raw) in text.lines().enumerate() {
        let line = first_line + offset as u64;
        let raw = raw.trim();
        if let Some(marker) = fence {
            if raw.starts_with(marker) {
                fence = None;
            }
            active = false;
            continue;
        }
        if let Some(marker) = ["```", "~~~"]
            .into_iter()
            .find(|marker| raw.starts_with(marker))
        {
            fence = Some(marker);
            active = false;
            continue;
        }
        if let Some(rest) = raw.strip_prefix('@') {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let tag = &rest[..end];
            active = matches!(
                tag,
                "intent"
                    | "domainRule"
                    | "requires"
                    | "ensures"
                    | "sideEffect"
                    | "mutates"
                    | "index"
            );
            if active {
                result.push(Annotation {
                    tag: tag.into(),
                    text: rest[end..].trim().into(),
                    line,
                    end_line: line,
                });
            }
        } else if raw.is_empty() {
            active = false;
        } else if active && let Some(last) = result.last_mut() {
            if !last.text.is_empty() {
                last.text.push(' ');
            }
            last.text.push_str(raw);
            last.end_line = line;
        }
    }
    result.retain(|item| !item.text.is_empty());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_ranges_and_stops_at_unknown_tags_or_blank_lines() {
        let text = "Contact me@intent.example, mention @intent in prose.\n@intent Recuperar\n o plano\n@domainRule Exigir aprovação\n@unknown Não incorporar\ncontinuação desconhecida\n@requires Sessão válida\n\nprosa solta\n@ensures\n";
        assert_eq!(
            parse(text, 10),
            vec![
                Annotation {
                    tag: "intent".into(),
                    text: "Recuperar o plano".into(),
                    line: 11,
                    end_line: 12
                },
                Annotation {
                    tag: "domainRule".into(),
                    text: "Exigir aprovação".into(),
                    line: 13,
                    end_line: 13
                },
                Annotation {
                    tag: "requires".into(),
                    text: "Sessão válida".into(),
                    line: 16,
                    end_line: 16
                },
            ]
        );
    }
    #[test]
    fn code_examples_do_not_become_business_assertions() {
        assert_eq!(
            parse(
                "Example:\n```text\n@intent Fake\n```\n@intent Real\n~~~\n@requires Fake requirement\n~~~\n",
                1
            ),
            vec![Annotation {
                tag: "intent".into(),
                text: "Real".into(),
                line: 5,
                end_line: 5
            }]
        );
    }
}
