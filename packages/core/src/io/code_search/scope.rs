//! Ask the native inventory tool to apply path/glob/type/ignore semantics.
//! Unknown options cannot authorize wider complementary source reads.
use super::{Answer, Request};
use std::collections::BTreeSet;
use std::path::Path;

pub(super) struct Scope {
    pub files: BTreeSet<String>,
    pub method: &'static str,
    pub clues: String,
}

pub(super) fn inventory(
    tree: &Path,
    cwd: &Path,
    request: &Request,
    answer: &Answer,
) -> Result<Scope, String> {
    if request.tool == "grep" {
        return Ok(Scope {
            files: super::occurrences(tree, cwd, request, &answer.report["result"], &answer.stdout)
                .into_iter()
                .map(|hit| hit.0)
                .collect(),
            method: "verified-native-hit-files-only",
            clues: String::new(),
        });
    }
    let (program, args) = if matches!(request.tool.as_str(), "Grep" | "Glob") {
        let path = super::search_path(&request.input)?;
        let mut args = vec!["--files".into(), "--null".into()];
        if request.tool == "Glob" {
            args.extend([
                "--hidden".into(),
                "-g".into(),
                "!.git/**".into(),
                "-g".into(),
                super::text(&request.input, "pattern")?.into(),
            ]);
        }
        for (field, flag) in [("glob", "--glob"), ("type", "--type")] {
            if let Some(value) = request.input[field].as_str() {
                args.extend([flag.into(), value.into()]);
            }
        }
        args.extend(["--".into(), path.into()]);
        ("rg", args)
    } else {
        let source: Vec<_> = request.input["args"]
            .as_array()
            .ok_or("task-scope-args-required")?
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        if request.tool == "rg" {
            ("rg", rg_inventory(&source)?)
        } else if request.tool == "git" {
            ("git", git_inventory(&source)?)
        } else {
            return Err("task-scope-tool-unsupported".into());
        }
    };
    let output = crate::platform::process::command(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("task-native-inventory-unavailable".into());
    }
    let mut files = BTreeSet::new();
    for file in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|file| !file.is_empty())
    {
        let file = std::str::from_utf8(file).map_err(|_| "task-inventory-non-text-path")?;
        let Ok(path) = cwd.join(file).canonicalize() else {
            continue;
        };
        if path.is_file()
            && let Ok(relative) = path.strip_prefix(tree)
        {
            files.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    let clues = if request.tool == "Grep" {
        request.input["pattern"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    } else if request.tool == "rg" {
        rg_clues(&request.input["args"])
    } else {
        String::new()
    };
    Ok(Scope {
        files,
        method: "native-file-inventory-with-original-path-filters",
        clues,
    })
}

fn rg_clues(args: &serde_json::Value) -> String {
    let args: Vec<_> = args
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect();
    if args.contains(&"--files") {
        return String::new();
    }
    let mut explicit = Vec::new();
    let mut first = None;
    let mut at = 0;
    let mut positional = false;
    while let Some(&arg) = args.get(at) {
        at += 1;
        if arg == "--" {
            positional = true;
            continue;
        }
        if !positional && (arg == "-e" || arg == "--regexp") {
            if let Some(value) = args.get(at) {
                explicit.push(*value);
                at += 1;
            }
        } else if !positional && arg.starts_with("--regexp=") {
            explicit.push(arg.trim_start_matches("--regexp="));
        } else if !positional
            && [
                "-g",
                "--glob",
                "--iglob",
                "-t",
                "--type",
                "-T",
                "--type-not",
                "--sort",
                "--sortr",
                "--color",
                "-A",
                "-B",
                "-C",
                "--context",
                "--after-context",
                "--before-context",
            ]
            .contains(&arg)
        {
            at += 1;
        } else if (positional || !arg.starts_with('-')) && first.is_none() {
            first = Some(arg);
        }
    }
    if explicit.is_empty() {
        first.unwrap_or_default().into()
    } else {
        explicit.join(" ")
    }
}

fn rg_inventory(source: &[&str]) -> Result<Vec<String>, String> {
    let mut args = vec!["--files".into(), "--null".into()];
    let mut paths = Vec::new();
    let mut pattern = source.contains(&"--files");
    let mut positional = false;
    let mut at = 0;
    while let Some(&arg) = source.get(at) {
        at += 1;
        if arg == "--" {
            positional = true;
            continue;
        }
        if !positional && arg.starts_with('-') {
            let (flag, attached) = arg
                .split_once('=')
                .map_or((arg, None), |(a, b)| (a, Some(b)));
            match flag {
                "--files" => {}
                "-g" | "--glob" | "--iglob" | "-t" | "--type" | "-T" | "--type-not" => {
                    let value = if let Some(value) = attached {
                        value
                    } else {
                        let value = *source.get(at).ok_or("task-scope-missing-option-value")?;
                        at += 1;
                        value
                    };
                    args.extend([flag.into(), value.into()]);
                }
                "--hidden" | "--no-hidden" | "--no-ignore" | "--no-ignore-vcs"
                | "--no-ignore-parent" | "--no-ignore-global" | "--no-ignore-dot" | "--follow"
                | "--one-file-system" => args.push(arg.into()),
                "-e" | "--regexp" | "--sort" | "--sortr" | "--color" | "-A" | "-B" | "-C"
                | "--context" | "--after-context" | "--before-context" => {
                    if attached.is_none() {
                        source.get(at).ok_or("task-scope-missing-option-value")?;
                        at += 1;
                    }
                    if matches!(flag, "-e" | "--regexp") {
                        pattern = true;
                    }
                }
                "--line-number"
                | "--with-filename"
                | "--no-filename"
                | "--no-heading"
                | "--heading"
                | "--ignore-case"
                | "--smart-case"
                | "--case-sensitive"
                | "--fixed-strings"
                | "--word-regexp"
                | "--line-regexp"
                | "--files-with-matches"
                | "--multiline"
                | "--multiline-dotall"
                | "--pcre2"
                | "--text" => {}
                _ if arg.starts_with('-')
                    && !arg.starts_with("--")
                    && arg[1..].chars().all(|c| "niIsFwlHhUPa".contains(c)) => {}
                _ => return Err("task-scope-unknown-option; native result retained".into()),
            }
        } else if !pattern {
            pattern = true;
        } else {
            paths.push(arg.to_string());
        }
    }
    if paths.is_empty() {
        paths.push(".".into());
    }
    args.push("--".into());
    args.extend(paths);
    Ok(args)
}

fn git_inventory(source: &[&str]) -> Result<Vec<String>, String> {
    if source.first() != Some(&"grep") {
        return Err("task-scope-git-grep-required".into());
    }
    let mut args = vec![
        "ls-files".into(),
        "-c".into(),
        "--exclude-standard".into(),
        "-z".into(),
        "--".into(),
    ];
    let mut pattern = false;
    let mut at = 1;
    while let Some(&arg) = source.get(at) {
        at += 1;
        if arg == "--" {
            args.extend(source[at..].iter().map(|value| value.to_string()));
            return Ok(args);
        }
        if matches!(arg, "-e" | "--regexp") {
            source.get(at).ok_or("task-scope-missing-pattern")?;
            at += 1;
            pattern = true;
        } else if arg.starts_with('-') {
            if !matches!(
                arg,
                "-n" | "--line-number"
                    | "-i"
                    | "--ignore-case"
                    | "-F"
                    | "--fixed-strings"
                    | "-E"
                    | "--extended-regexp"
                    | "-w"
                    | "--word-regexp"
                    | "-l"
                    | "--name-only"
                    | "--full-name"
                    | "--no-color"
            ) {
                return Err("task-scope-unknown-git-option".into());
            }
        } else if !pattern {
            pattern = true;
        } else {
            return Err("task-scope-ambiguous-git-revision-or-path".into());
        }
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patterns_never_turn_into_paths_and_unknown_filters_do_not_widen_scope() {
        assert_eq!(
            rg_inventory(&[
                "-n",
                "--sort=path",
                "--glob",
                "*.ext",
                "-e",
                "--literal",
                "--",
                "src"
            ])
            .unwrap(),
            ["--files", "--null", "--glob", "*.ext", "--", "src"]
        );
        assert_eq!(
            rg_inventory(&["-n", "needle", "src", "other"]).unwrap(),
            ["--files", "--null", "--", "src", "other"]
        );
        assert!(rg_inventory(&["--ignore-file", "custom", "needle", "src"]).is_err());
        assert!(git_inventory(&["grep", "-n", "needle", "HEAD"]).is_err());
        assert_eq!(
            git_inventory(&["grep", "-n", "needle", "--", "src"])
                .unwrap()
                .last()
                .unwrap(),
            "src"
        );
    }
}
