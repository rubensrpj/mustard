//! Identity carried by the native read command, including older dispatches
//! with language instructions before it. A quoted or mentioned wave number is
//! insufficient: spec, wave and project root must all be present.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDispatch {
    pub spec: String,
    pub wave: u64,
    pub root: String,
}

impl ReadDispatch {
    #[must_use]
    pub fn of(text: &str) -> Option<Self> {
        let mut found = None;
        for line in text.lines() {
            let Some((prefix, rest)) = line.trim().split_once("mustard-rt run read request-") else {
                continue;
            };
            // Native command alone or an instruction introducing that command.
            // Do not interpret a shell chain or arbitrary quoted code as a dispatch.
            if !prefix.is_empty() && !prefix.trim_end_matches('`').trim_end().ends_with(':') && prefix != "`" {
                continue;
            }
            let rest = rest.trim_end_matches('`');
            let Some((number, args)) = rest.split_once(" --root ") else { continue };
            let Some((root, spec)) = args.rsplit_once(" --spec ") else { continue };
            let Some(wave) = number.parse::<u64>().ok().filter(|n| *n > 0) else {
                continue;
            };
            if !number.bytes().all(|c| c.is_ascii_digit())
                || root.is_empty()
                || spec.is_empty()
                || spec.contains(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '`' | '\'' | '"' | '/' | '\\'))
                || root.contains(" --")
            {
                continue;
            }
            let root = root
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| root.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(root);
            let identity = Self {
                spec: spec.into(),
                wave,
                root: root.replace('\\', "/"),
            };
            if found.as_ref().is_some_and(|previous| previous != &identity) {
                return None;
            }
            found = Some(identity);
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_prefixed_read_dispatches_keep_exact_project_spec_and_wave_identity() {
        let command = "mustard-rt run read request-1 --root /work/project --spec original";
        for text in [
            command.to_string(),
            format!("Languages.\n\nRead your request with: {command}\nFinish the task."),
            format!("Idiomas deste projeto.\n\nLeia o seu pedido com: `{command}`"),
        ] {
            assert_eq!(
                ReadDispatch::of(&text),
                Some(ReadDispatch {
                    spec: "original".into(),
                    wave: 1,
                    root: "/work/project".into()
                })
            );
        }
        assert_eq!(
            ReadDispatch::of("mustard-rt run read request-30 --root \"C:\\my project\" --spec original")
                .unwrap()
                .root,
            "C:/my project"
        );
        for text in [
            format!("{command}\n{}", command.replace("request-1 ", "request-10 ")),
            format!("{command} --other x"),
            format!("{command}; echo done"),
            command.replace("request-1 ", "request-0 "),
            command.replace("request-1 ", "request-review "),
            "Only mentions request-1".into(),
        ] {
            assert_eq!(ReadDispatch::of(&text), None, "{text}");
        }
    }
}
