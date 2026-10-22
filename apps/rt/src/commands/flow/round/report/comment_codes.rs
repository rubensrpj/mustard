//! A entrega cuja cópia acrescentou comentário citando o código de um item da
//! spec é recusada. O agente da onda imita o comentário vizinho, e o código do
//! item, que fala da obra, ficava no código depois que ela acabava. Só contam
//! as linhas que a cópia acrescentou, em arquivo de código: o comentário
//! antigo fica, e o texto que não é comentário também.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::source_lang::language_of_path;
use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::platform::git;

use super::super::answer::RoundRefusal;
use super::super::commit::{copy_of, fork_point};
use super::super::removed_check::is_comment_line;

/// Recusa a entrega da onda `wave` quando a cópia dela, comparada ao commit
/// em que nasceu do checkout `root`, acrescentou linha de comentário que cita
/// o código de um item da spec `log`. Sem cópia no disco, nada é conferido.
pub(super) fn refuse_item_codes_in_comments(root: &Path, log: &SpecLog, wave: u64) -> Result<(), RoundRefusal> {
    let Some(copy) = copy_of(log, wave) else { return Ok(()) };
    let codes: BTreeSet<String> = log.codes().into_values().collect();
    if codes.is_empty() {
        return Ok(());
    }
    let base = fork_point(&copy, root).unwrap_or_else(|_| "HEAD".to_string());
    let diff = git::run(&copy, &["-c", "core.quotePath=false", "diff", "--unified=0", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", &base, "--"]);
    let mut found = added_in_diff(&diff.stdout, &codes);
    let fresh = git::run(&copy, &["ls-files", "--others", "--exclude-standard", "-z"]);
    for path in fresh.stdout.split('\0').filter(|path| !path.is_empty() && language_of_path(path).is_some()) {
        let Ok(text) = std::fs::read_to_string(copy.join(path)) else { continue };
        found.extend(text.lines().enumerate().filter_map(|(at, line)| cited(path, at + 1, line, &codes)));
    }
    if found.is_empty() {
        return Ok(());
    }
    Err(RoundRefusal::Refused(Refusal::DeliveryItemCodeInComment { wave, found }))
}

/// As linhas acrescentadas de um `git diff --unified=0` que são comentário,
/// em arquivo de código, e citam um dos códigos `codes`: cada uma como
/// `arquivo:linha (código)`, pela linha do arquivo novo.
fn added_in_diff(diff: &str, codes: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    let mut file: Option<String> = None;
    let mut next = 0usize;
    let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            (file, in_hunk) = (None, false);
        } else if !in_hunk && let Some(path) = line.strip_prefix("+++ ") {
            file = path.trim_matches('"').strip_prefix("b/").map(str::to_string).filter(|path| language_of_path(path).is_some());
        } else if let Some(header) = line.strip_prefix("@@ ") {
            in_hunk = true;
            next = new_start(header);
        } else if in_hunk && let Some(added) = line.strip_prefix('+') {
            if let Some(path) = &file {
                found.extend(cited(path, next, added, codes));
            }
            next += 1;
        }
    }
    found
}

/// A primeira linha do arquivo novo num cabeçalho de trecho (`-a,b +c,d @@`).
fn new_start(header: &str) -> usize {
    let plus = header.split_whitespace().find_map(|part| part.strip_prefix('+')).unwrap_or_default();
    plus.split(',').next().and_then(|start| start.parse().ok()).unwrap_or(0)
}

/// `arquivo:linha (código)` quando a linha `line`, de número `number`, é
/// comentário e cita um dos códigos `codes` como palavra inteira.
fn cited(path: &str, number: usize, line: &str, codes: &BTreeSet<String>) -> Option<String> {
    if !is_comment_line(line) {
        return None;
    }
    let code = line.split(|c: char| !(c.is_alphanumeric() || c == '-')).find(|word| codes.contains(*word))?;
    Some(format!("{path}:{number} ({code})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Só a linha acrescentada que é comentário e cita um código da spec conta:
    /// a removida, a de texto que não é comentário, a que cita um código que
    /// a spec não tem e a de arquivo que não é código ficam.
    #[test]
    fn only_an_added_comment_line_citing_a_spec_code_counts() {
        let codes: BTreeSet<String> = ["ABC-TASK-0003".to_string()].into();
        let diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -3,1 +3,0 @@\n-// ABC-TASK-0003 antigo\n\
                    @@ -9,0 +10,3 @@\n+let name = \"ABC-TASK-0003\";\n+// ABC-TASK-0004 outro\n+    // ABC-TASK-0003: soma\n\
                    diff --git a/notes.md b/notes.md\n--- a/notes.md\n+++ b/notes.md\n@@ -0,0 +1 @@\n+# ABC-TASK-0003\n";
        assert_eq!(added_in_diff(diff, &codes), vec!["src/a.rs:12 (ABC-TASK-0003)".to_string()]);
    }
}
