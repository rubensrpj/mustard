//! A leitura das regras do projeto que todo pedido ao revisor leva: o texto
//! do `CLAUDE.md` da raiz do projeto.

use std::path::Path;

/// O arquivo das regras do projeto, na raiz dele.
pub const PROJECT_RULES_FILE: &str = "CLAUDE.md";

/// O texto do `CLAUDE.md` da raiz do projeto `root`, sem os brancos das
/// pontas. `None` sem o arquivo, com ele ilegível ou só com brancos: aí o
/// pedido ao revisor sai sem a seção das regras.
#[must_use]
pub fn project_rules(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(PROJECT_RULES_FILE)).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// O texto do arquivo vem sem os brancos das pontas; sem o arquivo, ou
    /// com ele só com brancos, não há regras.
    #[test]
    fn the_project_rules_are_the_root_file_text_and_nothing_without_it_or_when_blank() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert_eq!(project_rules(root), None, "no file");
        std::fs::write(root.join(PROJECT_RULES_FILE), " \n\n\t\n").unwrap();
        assert_eq!(project_rules(root), None, "a blank file");
        std::fs::write(root.join(PROJECT_RULES_FILE), "\n# Regras\n\n- Nunca grave no git.\n\n").unwrap();
        assert_eq!(project_rules(root).as_deref(), Some("# Regras\n\n- Nunca grave no git."));
    }
}
