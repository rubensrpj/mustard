//! O arquivo que serve de exemplo e de base do corte de tamanho: o escrito à
//! mão que não é de teste. O arquivo de teste pelo caminho e o que um módulo
//! declara como teste ficam de fora dos dois, como ficam sem medida de
//! qualidade no scan.

use crate::domain::ast::is_test_path;

use super::{MapModule, WHOLE_FILE_END};

impl MapModule {
    /// Um módulo declara o arquivo como teste: ele traz o trecho que vai da
    /// linha 1 a [`WHOLE_FILE_END`].
    #[must_use]
    pub fn is_declared_test(&self) -> bool {
        self.test_lines.contains(&(1, WHOLE_FILE_END))
    }
}

/// `true` para o arquivo escrito à mão que não é de teste, pelo caminho nem
/// por um módulo que o declare: nenhum arquivo de teste e nenhum escrito por
/// máquina serve de exemplo nem entra na conta do corte de tamanho.
pub(super) fn is_example_material(m: &MapModule) -> bool {
    m.file_class.is_empty() && !is_test_path(&m.path) && !m.is_declared_test()
}

#[cfg(test)]
mod tests {
    use crate::domain::project_map::{examples, ProjectMap, Quality, QualityCuts};
    use crate::platform::i18n::Locale;

    use super::*;

    fn sized(path: String, size: usize) -> MapModule {
        MapModule { path, quality: Quality { size, ..Quality::default() }, ..MapModule::default() }
    }

    /// Um arquivo que o scan deixa sem tamanho, mas com as importações
    /// contadas, como o módulo que declara o arquivo como teste o mede.
    fn declared_test(n: usize, declared: bool) -> MapModule {
        MapModule {
            test_lines: if declared { vec![(1, WHOLE_FILE_END)] } else { Vec::new() },
            quality: Quality { imports: 3, ..Quality::default() },
            ..sized(format!("src/helpers/h{n}.rs"), 0)
        }
    }

    /// Quarenta arquivos de 10 a 400 linhas e vinte arquivos sem tamanho.
    fn forty_with_twenty(declared: bool) -> Vec<MapModule> {
        let mut modules: Vec<MapModule> = (1..=40).map(|n| sized(format!("src/f{n}.rs"), n * 10)).collect();
        modules.extend((0..20).map(|n| declared_test(n, declared)));
        modules
    }

    #[test]
    fn a_file_declared_as_test_stays_out_of_the_size_cut() {
        // Quarenta arquivos: os 5% de cima são 2, e o corte é o terceiro maior.
        let alone = QualityCuts::of(&forty_with_twenty(true)[..40]);
        assert_eq!(alone.size, 380);
        let cuts = QualityCuts::of(&forty_with_twenty(true));
        assert_eq!(cuts.size, 380, "os vinte arquivos declarados como teste não contam: {cuts:?}");
        // O mesmo arquivo sem o módulo que o declara conta como os outros: com
        // sessenta, os 5% de cima são 3, e o corte cai para o quarto maior.
        let counted = QualityCuts::of(&forty_with_twenty(false));
        assert_eq!(counted.size, 370, "{counted:?}");
    }

    #[test]
    fn a_file_declared_as_test_is_no_example_and_the_same_file_undeclared_is() {
        let in_orders = |name: &str, declared: bool| MapModule {
            path: format!("src/orders/{name}.rs"),
            loc: 100,
            deps: vec!["src/core.rs".to_string()],
            test_lines: if declared { vec![(1, WHOLE_FILE_END)] } else { Vec::new() },
            ..MapModule::default()
        };
        let picked = |declared: bool| -> Vec<String> {
            let modules = vec![in_orders("a_helpers", declared), in_orders("b", false), in_orders("c", false), in_orders("d", false)];
            let map = ProjectMap { modules, ..ProjectMap::default() };
            examples(&map, "src/orders/new.rs", Locale::PtBr).picks.into_iter().map(|p| p.path).collect()
        };
        assert_eq!(picked(true), ["src/orders/b.rs", "src/orders/c.rs", "src/orders/d.rs"]);
        assert_eq!(picked(false), ["src/orders/a_helpers.rs", "src/orders/b.rs", "src/orders/c.rs"]);
    }

    #[test]
    fn only_the_whole_file_block_declares_a_test() {
        let with = |lines: Vec<(u64, u64)>| MapModule { test_lines: lines, ..MapModule::default() };
        assert!(with(vec![(40, 60), (1, WHOLE_FILE_END)]).is_declared_test());
        assert!(!with(vec![(40, 60)]).is_declared_test(), "o trecho de teste dentro do arquivo não o faz de teste");
        assert!(!with(vec![(1, 60)]).is_declared_test());
        assert!(!with(Vec::new()).is_declared_test());
    }
}
