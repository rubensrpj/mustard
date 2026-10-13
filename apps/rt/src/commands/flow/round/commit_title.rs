//! Project conventions for commit subjects; wave identity stays in a trailer.
use super::{
    answer::RoundRefusal,
    commit::{check_commit_text, git, needs_commit, shorten_to},
    report::WaveReport,
};
use mustard_core::{
    domain::spec_events::MESSAGE_TITLE_MAX,
    platform::i18n::{Locale, translate},
};
use std::{collections::BTreeSet, path::Path};

/// New commits describe the project; legacy wave scopes remain readable only
/// for validation and recovery. A trailer carries the machine association.
pub(super) fn message(root: &Path, waves: &[WaveReport], lang: Locale) -> Result<Option<(String, String)>, RoundRefusal> {
    if !waves.iter().any(|wave| needs_commit(&wave.files) && wave.commit.is_some()) {
        return Ok(None);
    }
    let mut committed: Vec<_> = waves.iter().filter(|wave| needs_commit(&wave.files) && wave.commit.is_some()).collect();
    committed.sort_by_key(|wave| wave.wave);
    let files: BTreeSet<_> = committed.iter().flat_map(|wave| wave.files.iter().map(String::as_str)).collect();
    let mut args = vec!["log", "-40", "--format=%s", "--"];
    args.extend(files);
    let history = git(root, &args).unwrap_or_default();
    let scope = history.lines().find_map(|title| {
        let (prefix, _) = title.split_once(": ")?;
        let (kind, scope) = prefix.split_once('(')?;
        let scope = scope.strip_suffix(')')?;
        if !["feat", "fix", "refactor", "docs", "test", "perf", "build", "ci", "chore", "style"].contains(&kind) {
            return None;
        }
        let internal = scope.split('/').any(|part| {
            ["onda-", "ondas-", "wave-", "waves-"]
                .iter()
                .any(|prefix| part.strip_prefix(prefix).is_some_and(|rest| rest.chars().all(|ch| ch.is_ascii_digit() || ch == '-' || ch == '+')))
        });
        (!scope.is_empty() && scope.len() <= 32 && !internal).then_some(scope)
    });
    let branch = git(root, &["branch", "--show-current"]).unwrap_or_default();
    let category = branch.split('/').next().unwrap_or_default().trim();
    let kind = if committed.iter().any(|wave| !wave.fixes.is_empty()) {
        "fix"
    } else if ["fix", "refactor", "docs", "test", "perf", "build", "ci", "chore", "style"].contains(&category) {
        category
    } else {
        "feat"
    };
    let prefix = scope.map_or_else(|| format!("{kind}: "), |scope| format!("{kind}({scope}): "));
    let room = MESSAGE_TITLE_MAX.saturating_sub(prefix.chars().count());
    let each = room.saturating_sub((committed.len() - 1) * 2) / committed.len();
    let summaries = committed
        .iter()
        .map(|wave| {
            let text = wave.commit.as_deref().unwrap_or_default();
            let short = shorten_to(text, each);
            if short.is_empty() { text.chars().take(each).collect() } else { short }
        })
        .collect::<Vec<_>>()
        .join("; ");
    let title = format!("{prefix}{summaries}");
    let numbers = committed.iter().map(|wave| wave.wave.to_string()).collect::<Vec<_>>().join(",");
    let lines = committed
        .iter()
        .map(|wave| {
            let mut line = translate("round.commit.line", lang)
                .replace("{wave}", &wave.wave.to_string())
                .replace("{summary}", wave.commit.as_deref().unwrap_or_default());
            if !wave.fixes.is_empty() {
                line.push(' ');
                line.push_str(
                    &translate("round.commit.fixes", lang).replace("{waves}", &wave.fixes.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")),
                );
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");
    let body = format!("{lines}\n\nMustard-Waves: {numbers}");
    check_commit_text(&title, &body)?;
    Ok(Some((title, body)))
}
