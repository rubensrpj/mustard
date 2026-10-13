//! Disable ESLint autofixes during validation. Only simple, recognized ESLint
//! invocations and package scripts are rewritten; opaque shell programs retain
//! their declared semantics. The executed spelling is saved in the receipt.
use std::path::Path;

use crate::hooks::bash::lex::{self, Segment};

fn simple(command: &str) -> Option<Segment> {
    let ranges = lex::command_ranges(command);
    if ranges.len() != 1 || command[ranges[0].clone()].trim() != command.trim() || command.contains('#') {
        return None;
    }
    let mut segments = lex::segments(command);
    if segments.len() != 1 {
        return None;
    }
    let segment = segments.pop()?;
    (segment.leading.is_empty() && segment.redirects.is_empty() && !segment.piped && command.trim_start().starts_with(&segment.program.raw)).then_some(segment)
}

fn fixes_eslint(segment: &Segment) -> bool {
    let args = &segment.args;
    let eslint = |name: &str| matches!(name.rsplit(['/', '\\']).next().unwrap_or(name), "eslint" | "eslint.js" | "eslint.cmd");
    let direct = eslint(&segment.program.text);
    let wrapped = matches!(segment.name(), "node" | "nodejs" | "npx" | "pnpm" | "yarn" | "bun")
        && args.first().is_some_and(|word| eslint(&word.text));
    (direct || wrapped) && !args.iter().any(|word| word.text == "--") && args.iter().any(|word| word.text == "--fix")
}

pub(crate) fn readonly(root: &Path, command: &str) -> String {
    let Some(segment) = simple(command) else { return command.into() };
    if fixes_eslint(&segment) {
        return format!("{} --no-fix", command.trim_end());
    }
    if !matches!(segment.name(), "npm" | "pnpm" | "yarn" | "bun") {
        return command.into();
    }
    let args = &segment.args;
    let index = usize::from(args.first().is_some_and(|arg| arg.text == "run"));
    let Some(script) = args.get(index).filter(|arg| !arg.text.starts_with('-')) else { return command.into() };
    if args.iter().any(|arg| matches!(arg.text.as_str(), "--prefix" | "--dir" | "--cwd" | "-C" | "-w" | "--workspace") || arg.text.starts_with("--prefix=") || arg.text.starts_with("--workspace=")) {
        return command.into();
    }
    let package = std::fs::read(root.join("package.json")).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let fixes = package.as_ref().and_then(|value| value["scripts"][&script.text].as_str()).and_then(simple).is_some_and(|script| fixes_eslint(&script));
    if !fixes {
        return command.into();
    }
    let separator = if segment.name() == "npm" && !args.iter().any(|arg| arg.text == "--") { " --" } else { "" };
    format!("{}{separator} --no-fix", command.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disables_only_recognized_eslint_autofixes_and_preserves_filters() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"scripts":{"lint":"eslint . --fix --quiet --cache","opaque":"eslint . --fix && other"}}"#).unwrap();
        for (command, expected) in [
            ("pnpm run lint --ignore-pattern .claude/", "pnpm run lint --ignore-pattern .claude/ --no-fix"),
            ("npm run lint", "npm run lint -- --no-fix"),
            ("npm run lint -- --quiet", "npm run lint -- --quiet --no-fix"),
            ("yarn lint", "yarn lint --no-fix"),
            ("bun run lint", "bun run lint --no-fix"),
            ("eslint . --fix", "eslint . --fix --no-fix"),
            ("npx eslint . --fix", "npx eslint . --fix --no-fix"),
        ] { assert_eq!(readonly(root, command), expected, "{command}"); }
        for command in ["cargo clippy", "eslint .", "pnpm run missing", "pnpm run opaque", "echo 'eslint . --fix'", "pnpm run lint && other", "pnpm run lint # comment", "pnpm --dir other run lint", "pnpm run lint --cwd other", "eslint $(other) --fix", "eslint . --fix -- '--file'", "env --chdir other pnpm run lint"] {
            assert_eq!(readonly(root, command), command, "{command}");
        }
    }
}
