//! O que o Mustard deixa no projeto: o mapa do scan, que só pode ficar fora
//! do git; as escolhas do `mustard.json` contra as configurações locais; e as
//! sobras de um Mustard antigo em arquivos que não são dele.

use std::path::Path;

use mustard_core::platform::i18n::{translate, Locale};

use super::CheckResult;

/// What the scan writes, inside the project's `.claude/`.
const SCAN_OUTPUTS: &[&str] = &["grain.model.json"];

/// The scan only writes outside git: what it recorded may be neither tracked
/// nor show up as a new file. A visible file becomes a WARN with the list, in
/// the language `lang`; with no git, or nothing recorded yet, there is nothing
/// to check. Read-only.
pub(super) fn check_scan_output(root: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "scan-output";
    let written: Vec<String> = SCAN_OUTPUTS
        .iter()
        .filter(|name| root.join(".claude").join(name).is_file())
        .map(|name| format!(".claude/{name}"))
        .collect();
    let visible = match visible_to_git(root, &written) {
        Some(visible) if !visible.is_empty() => visible,
        _ => return CheckResult::ok(NAME),
    };
    CheckResult::warn(NAME, vec![translate("doctor.scan_output.visible", lang).replace("{paths}", &visible.join(", "))])
}

/// The paths git sees: tracked, or new and not ignored. `None` when git does
/// not answer (no git, outside a repository).
fn visible_to_git(root: &Path, paths: &[String]) -> Option<Vec<String>> {
    let git_ok = |args: &[&str]| mustard_core::platform::git::run(root, args).ok;
    if !git_ok(&["rev-parse", "--is-inside-work-tree"]) {
        return None;
    }
    Some(
        paths
            .iter()
            .filter(|p| git_ok(&["ls-files", "--error-unmatch", "--", p]) || !git_ok(&["check-ignore", "-q", "--", p]))
            .cloned()
            .collect(),
    )
}

/// The choices `mustard.json` holds for the project against what the local
/// settings carry. Only reads. A WARN when Mustard is off here, when the `rtk`
/// option and rtk's hook disagree, and when Claude Code's signature is on — each
/// in the language `lang`, naming what to run.
pub(super) fn check_switches(root: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "switches";
    let switches = mustard_core::Switches::read(root);
    let mut details = Vec::new();
    if !switches.enabled {
        details.push(translate("doctor.switches.off", lang).to_string());
    }
    if switches.rtk_diverges() {
        let key = if switches.rtk { "doctor.switches.rtk_missing" } else { "doctor.switches.rtk_left" };
        details.push(translate(key, lang).to_string());
    }
    if switches.signature_on == Some(true) {
        details.push(translate("doctor.switches.signature_on", lang).to_string());
    }
    if details.is_empty() {
        CheckResult::ok(NAME)
    } else {
        CheckResult::warn(NAME, details)
    }
}

/// What an older Mustard left in files that are not its own — the marks in
/// the `CLAUDE.md` files, the seed's lines in the team's settings, a planted
/// orchestrator. Only reads, through the same list the `upsert` shows; a WARN
/// names the files and says how to take them out.
pub(super) fn check_claude_md(root: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "claude-md";
    let plan = mustard_core::platform::project_seed::cleanup::plan(root);
    if plan.files.is_empty() {
        return CheckResult::ok(NAME);
    }
    let paths: Vec<&str> = plan.files.iter().map(|change| change.path.as_str()).collect();
    CheckResult::warn(NAME, vec![translate("doctor.claude_md.leftovers", lang).replace("{paths}", &paths.join(", "))])
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::super::Status;
    use super::*;
    use crate::commands::doctor::doctor::tests::*;

    /// The scan map visible to git is reported; excluded, it passes; outside a
    /// repository, there is nothing to check.
    #[test]
    fn the_doctor_flags_a_scan_map_that_git_can_see() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".claude").join("grain.model.json"), "{}").unwrap();
        assert_eq!(check_scan_output(root, Locale::PtBr).status, Status::Ok, "not a repository");

        let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(root).output();
        if !init.is_ok_and(|o| o.status.success()) {
            return; // no git here: nothing the check could measure
        }
        let visible = check_scan_output(root, Locale::PtBr);
        assert_eq!(visible.status, Status::Warn, "{:?}", visible.details);
        assert!(visible.details.join(" ").contains(".claude/grain.model.json"), "{:?}", visible.details);
        let en = check_scan_output(root, Locale::EnUs);
        assert!(en.details.join(" ").contains("visible to git"), "{:?}", en.details);

        std::fs::write(root.join(".git").join("info").join("exclude"), "**/.claude/grain.model.json\n").unwrap();
        assert_eq!(check_scan_output(root, Locale::PtBr).status, Status::Ok);
    }

    /// O diagnóstico avisa o Mustard desligado, a opção do rtk que diverge do
    /// gancho nas configurações locais e a assinatura ligada; com tudo
    /// alinhado, fica quieto.
    #[test]
    fn the_doctor_flags_mustard_off_a_diverging_rtk_and_the_signature() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let local = root.join(".claude").join("settings.local.json");
        write_file(
            &local,
            r#"{"attribution":{"commit":"","pr":""},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"rtk hook claude"}]}]}}"#,
        );
        assert_eq!(check_switches(root, Locale::PtBr).status, Status::Ok, "aligned: nothing to say");

        write_file(&root.join("mustard.json"), r#"{"enabled":false,"rtk":false}"#);
        let off = check_switches(root, Locale::PtBr);
        assert_eq!(off.status, Status::Warn);
        let said = off.details.join(" ");
        assert!(said.contains("desligado neste projeto"), "{said}");
        assert!(said.contains("continua no"), "the hook left behind is named: {said}");

        write_file(&root.join("mustard.json"), "{}");
        write_file(&local, r#"{"attribution":{"commit":"assistant","pr":"assistant"}}"#);
        let en = check_switches(root, Locale::EnUs);
        let said = en.details.join(" ");
        assert!(said.contains("is not in"), "the missing hook is named: {said}");
        assert!(said.contains("signature"), "{said}");
        assert!(!said.contains("turned off in this project"), "{said}");
    }

    /// As sobras do Mustard nos `CLAUDE.md` viram aviso com os arquivos; sem
    /// sobra, a conferência passa.
    #[test]
    fn the_doctor_flags_mustard_leftovers_in_claude_md() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert_eq!(check_claude_md(root, Locale::PtBr).status, Status::Ok);
        write_file(
            &root.join("apps/api/CLAUDE.md"),
            "# Api\n<!-- mustard:guards -->\n- A guard.\n<!-- /mustard:guards -->\n",
        );
        let found = check_claude_md(root, Locale::PtBr);
        assert_eq!(found.status, Status::Warn);
        assert!(found.details[0].contains("apps/api/CLAUDE.md"), "{:?}", found.details);
        assert!(found.details[0].contains("mustard-rt run upsert"), "{:?}", found.details);
    }
}
