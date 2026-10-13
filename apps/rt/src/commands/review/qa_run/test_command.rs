//! Preserve shell spelling; fix only an unambiguous Jest single root followed
//! by explicit test files. Multiple directory roots and other tools stay intact.
pub(crate) fn canonical(command: &str) -> String {
    let mut result = command.to_string();
    for range in crate::hooks::bash::lex::command_ranges(command).into_iter().rev() {
        let part = &command[range.clone()];
        let segments = crate::hooks::bash::lex::segments(part);
        let Some(segment) = segments.first() else {
            continue;
        };
        let jest = |word: &str| matches!(word.rsplit(['/', '\\']).next().unwrap_or(word), "jest" | "jest.js" | "jest.cmd");
        let wrapper = matches!(segment.name(), "taskset" | "node" | "nodejs" | "npx" | "npm" | "pnpm" | "yarn" | "bun");
        if !(jest(&segment.program.text) || wrapper && segment.args.iter().any(|arg| jest(&arg.text))) {
            continue;
        }
        let Some(index) = segment.args.iter().position(|arg| arg.text == "--roots") else {
            continue;
        };
        let Some(root) = segment.args.get(index + 1).filter(|arg| !arg.text.starts_with('-')) else {
            continue;
        };
        let rest = &segment.args[index + 2..];
        if !rest.iter().any(|arg| arg.text.contains(".spec.") || arg.text.contains(".test.")) {
            continue;
        }
        if rest.iter().take_while(|arg| !arg.text.starts_with('-')).any(|arg| !arg.text.contains(".spec.") && !arg.text.contains(".test.")) {
            continue;
        }
        let mut quote = None;
        let mut escaped = false;
        let start = part.char_indices().find_map(|(at, ch)| {
            if escaped {
                escaped = false;
                return None;
            }
            if ch == '\\' && quote != Some('\'') {
                escaped = true;
                return None;
            }
            if let Some(active) = quote {
                if ch == active {
                    quote = None;
                }
                return None;
            }
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
                return None;
            }
            (part[at..].starts_with("--roots")
                && (at == 0 || part[..at].chars().next_back().is_some_and(char::is_whitespace))
                && part[at + 7..].chars().next().is_some_and(char::is_whitespace))
            .then_some(at)
        });
        let Some(start) = start else {
            continue;
        };
        let after = start + "--roots".len();
        let spaces = part[after..].len() - part[after..].trim_start().len();
        if spaces == 0 || !part[after + spaces..].starts_with(&root.raw) {
            continue;
        }
        result.replace_range(range.start + start..range.start + after + spaces + root.raw.len(), &format!("--roots={}", root.raw));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixes_jest_roots_without_rewriting_multiple_roots_or_other_commands() {
        assert_eq!(
            canonical("taskset -c 0-3 ./node_modules/.bin/jest --roots src src/example.spec.ts"),
            "taskset -c 0-3 ./node_modules/.bin/jest --roots=src src/example.spec.ts"
        );
        assert_eq!(
            canonical("jest --roots 'source files' 'source files/example.test.ts'"),
            "jest --roots='source files' 'source files/example.test.ts'"
        );
        assert_eq!(
            canonical("jest --testNamePattern='name --roots src' --roots src src/example.spec.ts"),
            "jest --testNamePattern='name --roots src' --roots=src src/example.spec.ts"
        );
        for command in [
            "jest --roots src tests",
            "jest --roots src tests src/example.spec.ts",
            "tool --roots src src/example.spec.ts",
            "echo jest --roots src src/example.spec.ts",
            "jest --roots=src src/example.spec.ts",
        ] {
            assert_eq!(canonical(command), command);
        }
    }
}
