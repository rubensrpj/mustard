//! O que o Mustard deixa no projeto: o mapa do scan, que só pode ficar fora
//! do git; as escolhas do `mustard.json`, entre elas os dois idiomas, contra
//! as configurações locais; e as sobras de um Mustard antigo em arquivos que
//! não são dele.

use std::path::Path;

use mustard_core::io::project_map;
use mustard_core::platform::i18n::{translate, Locale};

use super::CheckResult;

/// The scan only writes outside git: what it recorded may be neither tracked
/// nor show up as a new file. A visible file becomes a WARN with the list, in
/// the language `lang`; with no git, or nothing recorded yet, there is nothing
/// to check. Read-only.
pub(super) fn check_scan_output(root: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "scan-output";
    // What the scan writes: the project map, asked through its door.
    let written: Vec<String> = project_map::exists_at(&project_map::model_path(root))
        .then(|| project_map::MAP_FILE.to_string())
        .into_iter()
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
/// option and rtk's hook disagree, when Claude Code's signature is on, and when
/// a language key holds a value the reader drops — each in the language
/// `lang`, naming what to run or what to write.
pub(super) fn check_switches(root: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "switches";
    let switches = mustard_core::Switches::read(root);
    let mut details = unread_languages(root, lang);
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

/// Each language key `mustard.json` writes and the reader does not take — the
/// short form (`pt`, `en`), another locale, a blank — as a sentence naming the
/// field, its value and the two accepted ones. The verdict is the one reader's
/// own, so the doctor never flags a value the configuration reads, nor passes
/// one it drops.
fn unread_languages(root: &Path, lang: Locale) -> Vec<String> {
    let config = mustard_core::ProjectConfig::load(root);
    let read = config.language();
    [
        ("language.text", config.language.text.as_deref(), read.text.is_some()),
        ("language.code", config.language.code.as_deref(), read.code.is_some()),
    ]
    .into_iter()
    .filter_map(|(field, written, understood)| match written {
        Some(value) if !understood => Some(
            translate("doctor.switches.language_unknown", lang).replace("{field}", field).replace("{value}", value),
        ),
        _ => None,
    })
    .collect()
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
        project_map::write_text(root, "{}").unwrap();
        assert_eq!(check_scan_output(root, Locale::PtBr).status, Status::Ok, "not a repository");

        let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(root).output();
        if !init.is_ok_and(|o| o.status.success()) {
            return; // no git here: nothing the check could measure
        }
        let visible = check_scan_output(root, Locale::PtBr);
        assert_eq!(visible.status, Status::Warn, "{:?}", visible.details);
        assert!(visible.details.join(" ").contains(project_map::MAP_FILE), "{:?}", visible.details);
        let en = check_scan_output(root, Locale::EnUs);
        assert!(en.details.join(" ").contains("visible to git"), "{:?}", en.details);

        std::fs::write(root.join(".git").join("info").join("exclude"), format!("**/{}\n", project_map::MAP_FILE)).unwrap();
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

    /// Um idioma do `mustard.json` fora de `pt-BR` e `en-US` vira aviso que
    /// nomeia o campo, o valor e as duas opções; a forma curta não é aceita. O
    /// idioma da lista e a chave ausente passam.
    #[test]
    fn the_doctor_flags_a_language_outside_the_list() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_file(
            &root.join(".claude").join("settings.local.json"),
            r#"{"attribution":{"commit":"","pr":""},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"rtk hook claude"}]}]}}"#,
        );

        write_file(&root.join("mustard.json"), r#"{"language":{"code":"pt"}}"#);
        let short = check_switches(root, Locale::PtBr);
        assert_eq!(short.status, Status::Warn, "{:?}", short.details);
        let said = short.details.join(" ");
        for named in ["`language.code`", "`pt`", "`pt-BR`", "`en-US`"] {
            assert!(said.contains(named), "{named} is named: {said}");
        }
        assert!(!said.contains("language.text"), "{said}");

        write_file(&root.join("mustard.json"), r#"{"language":{"text":"fr-FR","code":"en-US"}}"#);
        let other = check_switches(root, Locale::EnUs);
        assert_eq!(other.status, Status::Warn, "{:?}", other.details);
        let said = other.details.join(" ");
        assert!(said.contains("`language.text`") && said.contains("`fr-FR`"), "{said}");
        assert!(said.contains("not an accepted language") && !said.contains("language.code"), "{said}");

        for calm in [r#"{"language":{"text":"en-US","code":"pt-BR"}}"#, "{}"] {
            write_file(&root.join("mustard.json"), calm);
            let passed = check_switches(root, Locale::PtBr);
            assert_eq!(passed.status, Status::Ok, "{calm}: {:?}", passed.details);
        }
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
