//! O último passo do `init`: o mapa do projeto, criado com a instalação já no
//! disco, para a primeira busca do assistente já ter um mapa de onde responder,
//! também num projeto que ainda não tem código (mapa vazio, sem erro).

use std::io::Write;
use std::path::Path;

use mustard_core::domain::scan::ScanReport;
use mustard_core::platform::error::Result;

/// O scan que a instalação chama: o que está ao lado do programa em
/// execução e, na falta dele, o do `PATH`, como o programa de execução o acha.
pub(super) fn located_scan(root: &Path, out: &Path) -> Result<ScanReport> {
    mustard_core::Scan::locate().scan(root, out)
}

/// Cria o mapa de `project` com `scan`, no lugar onde o mapa mora, e diz em
/// `out` que o criou. O scan que falha nunca derruba a instalação: vira uma
/// linha de aviso, porque o próximo início de sessão ou a próxima busca cria o
/// mapa de novo. Uma escrita em `out` que falha é descartada.
pub(super) fn build(
    project: &Path,
    out: &mut impl Write,
    scan: &dyn Fn(&Path, &Path) -> Result<ScanReport>,
) {
    let _ = writeln!(out, "  reading the project to build its map");
    let model = mustard_core::io::project_map::model_path(project);
    match scan(project, &model) {
        Ok(report) => {
            let noun = if report.files == 1 { "file" } else { "files" };
            let _ = writeln!(out, "  built the project map ({} code {noun})", report.files);
        }
        Err(failed) => {
            let _ = writeln!(
                out,
                "  warning: the project map was not built ({failed}); the next session start or search tries again"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(scan: &dyn Fn(&Path, &Path) -> Result<ScanReport>) -> String {
        let mut out = Vec::new();
        build(Path::new("/project"), &mut out, scan);
        String::from_utf8(out).unwrap()
    }

    /// O scan roda sobre o projeto e o mapa dele, e a instalação diz quantos
    /// arquivos de código o mapa tem; o projeto sem código diz zero, não erro.
    #[test]
    fn the_map_built_is_told_with_its_code_files() {
        let told = said(&|root, out| {
            assert_eq!(root, Path::new("/project"));
            assert_eq!(out, mustard_core::io::project_map::model_path(Path::new("/project")));
            Ok(ScanReport { files: 3, ..ScanReport::default() })
        });
        assert!(told.contains("built the project map (3 code files)"), "{told}");

        let empty = said(&|_, _| Ok(ScanReport::default()));
        assert!(empty.contains("built the project map (0 code files)"), "{empty}");
        assert!(!empty.contains("warning"), "{empty}");
    }

    /// O scan que falha vira uma linha de aviso que diz quando o mapa é
    /// tentado de novo, nunca pânico nem erro.
    #[test]
    fn a_scan_that_fails_becomes_one_warning_line() {
        let told = said(&|_, _| Err(mustard_core::platform::error::Error::check_failed("scan: not found")));
        let warnings: Vec<&str> = told.lines().filter(|line| line.contains("warning")).collect();
        assert_eq!(warnings.len(), 1, "{told}");
        assert!(warnings[0].contains("scan: not found") && warnings[0].contains("next session start or search"), "{told}");
        assert!(!told.contains("built the project map"), "{told}");
    }
}
