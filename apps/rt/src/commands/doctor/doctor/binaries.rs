//! De que commit vêm os três programas do Mustard que respondem nesta máquina:
//! o que roda este diagnóstico, o `mustard-rt` do plugin instalado e o primeiro
//! `mustard-rt` do `PATH`, o do sistema. A linha de versão de cada um traz o
//! commit e a data em que ele foi compilado. Quando dois divergem, ou quando um
//! não diz o commit, nada prova que a sessão roda o código que se pensa.

use std::collections::BTreeSet;
use std::path::PathBuf;

use mustard_core::platform::i18n::{translate, Locale};

use super::{CheckResult, Status};

const NAME: &str = "binaries";

/// Um dos três programas: o papel dele, onde ele está e a linha de versão que
/// ele respondeu. Sem caminho, ele não foi achado; sem linha, não respondeu.
struct Program {
    role: &'static str,
    path: Option<PathBuf>,
    version: Option<String>,
}

/// O commit dos três programas, em uma linha, com o aviso quando divergem ou
/// quando um não diz o commit, no idioma `lang`.
pub(super) fn check_binaries(lang: Locale) -> CheckResult {
    let running = Program {
        role: "running",
        path: std::env::current_exe().ok(),
        version: Some(env!("MUSTARD_VERSION_FULL").to_string()),
    };
    let path_env = std::env::var("PATH").unwrap_or_default();
    let programs = [
        running,
        asked("plugin", mustard_core::platform::harness::installed_plugin_rt()),
        asked("system", mustard_core::platform::process::program_file("mustard-rt", cfg!(windows), &path_env)),
    ];
    compare(&programs, lang)
}

/// O programa de `path`, com a linha que o `--version` dele responde.
fn asked(role: &'static str, path: Option<PathBuf>) -> Program {
    let version = path.as_deref().and_then(crate::shared::development_build::version_of);
    Program { role, path, version }
}

/// O commit e a data que a linha de versão carimba — o `g<commit>[-dirty]
/// <data>` de `<versão> (build N, g<commit>[-dirty] <data>)` —, ou `None` na
/// linha de um programa compilado sem o commit.
fn stamp(version: &str) -> Option<&str> {
    let start = version.find(", g")? + 2;
    let end = start + version[start..].find(')')?;
    let stamp = &version[start..end];
    let commit = stamp[1..].split(['-', ' ']).next()?;
    (commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit())).then_some(stamp)
}

/// Os três lado a lado, e o aviso de cada coisa que impede a prova.
fn compare(programs: &[Program], lang: Locale) -> CheckResult {
    let shown: Vec<String> = programs
        .iter()
        .map(|program| {
            let said = match (&program.path, &program.version) {
                (None, _) => "not found".to_string(),
                (Some(_), None) => "did not answer".to_string(),
                (Some(_), Some(version)) => stamp(version).map_or_else(|| format!("{version}, no commit"), str::to_string),
            };
            format!("{} {said}", program.role)
        })
        .collect();
    let mut details = vec![shown.join(" · ")];
    details.extend(programs.iter().filter_map(|program| {
        program.path.as_ref().filter(|_| program.role != "running").map(|path| format!("{}: {}", program.role, path.display()))
    }));
    let answered: Vec<&Program> = programs.iter().filter(|program| program.version.is_some()).collect();
    let unstamped: Vec<&str> = answered
        .iter()
        .filter(|program| program.version.as_deref().and_then(stamp).is_none())
        .map(|program| program.role)
        .collect();
    let stamps: BTreeSet<&str> = answered.iter().filter_map(|program| program.version.as_deref().and_then(stamp)).collect();
    let mut warned = false;
    if !unstamped.is_empty() {
        details.push(translate("doctor.binaries.unstamped", lang).replace("{programs}", &unstamped.join(", ")));
        warned = true;
    }
    if stamps.len() > 1 {
        let listed: Vec<&str> = stamps.into_iter().collect();
        details.push(translate("doctor.binaries.diverge", lang).replace("{stamps}", &listed.join(" ≠ ")));
        warned = true;
    }
    CheckResult { name: NAME, status: if warned { Status::Warn } else { Status::Ok }, details }
}
