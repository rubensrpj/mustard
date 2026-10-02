//! `conventions` — as perguntas estruturais, agnósticas, que o scan faz sobre
//! um caminho de arquivo: ele é de teste? E, sendo, que nome ele testa? Ele é
//! o arquivo de entrada da pasta em que mora?
//!
//! É um predicado puro de caminho, sem nenhuma noção de linguagem, framework
//! ou arquitetura: uma convenção de pasta e uma convenção de nome de arquivo.
//! As listas de teste moram num arquivo de dados só, `test-files.toml`,
//! embutido na compilação; ninguém mantém lista própria, e aqui não há nome de
//! língua nem de ferramenta. Quem lê a regra: o mapa de testes do scan (quais
//! testes cobrem cada arquivo), os pontos de registro do grafo do scan (teste
//! não conta), a evidência de pilha do scan, do projeto e de cada subprojeto
//! (arquivo de teste não diz o que o projeto é), o mapa do projeto (busca e
//! exemplos) e o padrão do projeto.
//!
//! O arquivo de entrada — o que responde pela pasta, com o mesmo nome em toda
//! pasta por regra da língua — mora noutro arquivo de dados, `entry-files.toml`,
//! também embutido, com uma lista por língua. A língua entra só como chave: o
//! código não conhece nenhuma. Quem lê: a importação do scan e o padrão do
//! projeto.
//!
//! O módulo filho que o pai declara dentro de si também vem de dado, em
//! `nested-modules.toml`: a lista das línguas que têm esse arranjo. Quem lê: a
//! conferência de ciclos de importação depois da onda.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// As listas que dizem o que é arquivo de teste, lidas de `test-files.toml`,
/// embutido na compilação. O porquê de cada lista está no cabeçalho dela, lá.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TestFiles {
    /// Pastas de teste em qualquer profundidade.
    dirs: Vec<String>,
    /// Pastas de teste só perto do topo, porque o nome é também do domínio.
    ambiguous_dirs: Vec<String>,
    /// A última posição do caminho, contando a primeira pasta como 0, em que
    /// a pasta ambígua ainda conta.
    ambiguous_max_depth: usize,
    /// Terminações do nome de uma pasta de projeto de teste.
    project_dir_endings: Vec<String>,
    /// Sufixos do nome do arquivo sem a última extensão.
    stem_suffixes: Vec<String>,
    /// Sufixos aceitos só quando começam uma palavra camelCase.
    camel_suffixes: Vec<String>,
    /// Prefixos do nome do arquivo.
    stem_prefixes: Vec<String>,
    /// Marcadores de teste dentro do conteúdo do arquivo.
    inline_markers: Vec<String>,
}

/// Os dados de teste, lidos uma vez. Dado ilegível vira listas vazias, sem
/// derrubar quem pergunta; o teste deste módulo garante que o arquivo
/// embutido se lê.
fn data() -> &'static TestFiles {
    static DATA: OnceLock<TestFiles> = OnceLock::new();
    DATA.get_or_init(|| toml::from_str(include_str!("test-files.toml")).unwrap_or_default())
}

/// Os nomes do arquivo de entrada de cada língua, lidos de
/// `entry-files.toml`, embutido na compilação: a chave é o nome da língua, e o
/// valor, os nomes sem a extensão, na ordem em que a importação os procura.
type EntryFiles = BTreeMap<String, Vec<String>>;

/// Os dados do arquivo de entrada, lidos uma vez. Dado ilegível vira lista
/// vazia, sem derrubar quem pergunta; o teste deste módulo garante que o
/// arquivo embutido se lê.
fn entry_data() -> &'static EntryFiles {
    static DATA: OnceLock<EntryFiles> = OnceLock::new();
    DATA.get_or_init(|| toml::from_str(include_str!("entry-files.toml")).unwrap_or_default())
}

/// Os nomes, sem a extensão, do arquivo que responde pela pasta na língua
/// `language`, na ordem em que a importação os procura. Vazio na língua que
/// não tem arquivo de entrada.
#[must_use]
pub fn entry_file_names(language: &str) -> &'static [String] {
    entry_data().get(language).map_or(&[], Vec::as_slice)
}

/// Se o arquivo `rel`, escrito na língua `language`, é o arquivo de entrada
/// da pasta em que mora: o nome dele sem a última extensão é, inteiro e com a
/// caixa como está, um dos nomes de entrada da língua.
#[must_use]
pub fn is_entry_file(rel: &str, language: &str) -> bool {
    let file = rel.rsplit(['/', '\\']).next().unwrap_or(rel);
    let stem = stem_of(file);
    entry_file_names(language).iter().any(|name| name == stem)
}

/// As línguas em que o módulo pai declara o filho dentro da pasta dele, lidas
/// de `nested-modules.toml`, embutido na compilação: a chave é o nome da
/// língua.
type NestedModules = BTreeMap<String, bool>;

/// Os dados dos módulos filhos, lidos uma vez. Dado ilegível vira lista vazia,
/// sem derrubar quem pergunta; o teste deste módulo garante que o arquivo
/// embutido se lê.
fn nested_data() -> &'static NestedModules {
    static DATA: OnceLock<NestedModules> = OnceLock::new();
    DATA.get_or_init(|| toml::from_str(include_str!("nested-modules.toml")).unwrap_or_default())
}

/// Se `child` é um módulo que `parent`, escrito na língua `language`, declara
/// dentro de si: a língua declara o filho no pai (`nested-modules.toml`) e o
/// caminho do filho está na pasta do módulo do pai — a pasta do próprio pai,
/// quando ele é o arquivo de entrada dela, ou a pasta com o nome dele sem a
/// extensão, no outro caso. O arquivo nunca é filho de si mesmo. Os caminhos
/// são relativos à raiz do projeto, com `/` ou `\`.
#[must_use]
pub fn is_declared_child(parent: &str, child: &str, language: &str) -> bool {
    if !nested_data().get(language).copied().unwrap_or(false) {
        return false;
    }
    let (parent, child) = (parent.replace('\\', "/"), child.replace('\\', "/"));
    if parent == child {
        return false;
    }
    let (dir, file) = parent.rsplit_once('/').unwrap_or(("", parent.as_str()));
    let folder = if is_entry_file(&parent, language) {
        dir.to_string()
    } else if dir.is_empty() {
        stem_of(file).to_string()
    } else {
        format!("{dir}/{}", stem_of(file))
    };
    folder.is_empty() || child.strip_prefix(&folder).is_some_and(|rest| rest.len() > 1 && rest.starts_with('/'))
}

/// Os marcadores que, no conteúdo de um arquivo, dizem que ele guarda os
/// próprios testes.
#[must_use]
pub fn inline_test_markers() -> &'static [String] {
    &data().inline_markers
}

/// Whether a relative path points at a test/spec/fixture/mock file by
/// convention — agnostic to any programming language or framework.
///
/// The relative path is normalised to forward slashes and compared
/// case-insensitively. It is a test path when ANY of these holds:
///
/// - a whole path segment is one of the test folders of the data (segment
///   match, not substring — so `attestation/x.rs` is NOT a test path), or one
///   of its ambiguous folders sitting near the top;
/// - a folder of the path (never the file) has a name that ends with one of
///   the test-project endings (`MeuApp.Tests/`);
/// - the filename stem (the final component with its last extension removed)
///   ends with one of the stem suffixes, starts with one of the stem
///   prefixes, or ends with a camelCase test word.
#[must_use]
pub fn is_test_path(rel: &str) -> bool {
    let data = data();
    let slashed = rel.replace('\\', "/");
    let normalised = slashed.to_ascii_lowercase();
    let segments: Vec<&str> = normalised.split('/').collect();
    let folders = segments.len().saturating_sub(1);

    // Pasta: a de teste conta em qualquer profundidade; a ambígua (nome que
    // é também do domínio) só onde a convenção a põe, perto do topo; a pasta
    // de projeto de teste conta pelo fim do nome, em qualquer profundidade.
    for (i, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }
        if has(&data.dirs, segment) {
            return true;
        }
        if i <= data.ambiguous_max_depth && has(&data.ambiguous_dirs, segment) {
            return true;
        }
        if i < folders && data.project_dir_endings.iter().any(|end| segment.ends_with(end.as_str())) {
            return true;
        }
    }

    // Stem convention: inspect only the filename (last segment), with its final
    // extension stripped, so `foo.test.ts` → stem `foo.test`, `foo_test.go` →
    // stem `foo_test`, `test_foo.py` → stem `test_foo`. The original-case stem
    // is kept alongside the lowered one so a camelCase word boundary
    // (`FooSpec`) can be detected.
    let Some(file_lc) = normalised.rsplit('/').next() else {
        return false;
    };
    let file_orig = slashed.rsplit('/').next().unwrap_or(file_lc);
    let stem = stem_of(file_lc);
    let stem_orig = stem_of(file_orig);
    if stem.is_empty() {
        return false;
    }
    if data.stem_suffixes.iter().any(|s| stem.ends_with(s.as_str())) {
        return true;
    }
    if data.stem_prefixes.iter().any(|p| stem.starts_with(p.as_str())) {
        return true;
    }
    camel_suffix_len(stem, stem_orig).is_some()
}

/// `list` traz `word` inteiro.
fn has(list: &[String], word: &str) -> bool {
    list.iter().any(|entry| entry == word)
}

/// O nome que um arquivo de teste testa: o nome dele sem a marca de teste, na
/// caixa original — `foo.spec.ts` dá `foo`, `x.service.spec.ts` dá
/// `x.service`, `FooTest.cs` dá `Foo`, `login.cy.ts` dá `login`. A marca sai
/// dos mesmos dados que [`is_test_path`] usa: os sufixos, os prefixos e a
/// palavra camelCase.
///
/// Devolve `None` quando o caminho não é de teste, ou quando nada sobra depois
/// de tirar a marca (`tests.rs`). O teste que só é teste pela pasta onde mora
/// (`tests/foo.rs`) não traz marca no nome, e testa o nome que tem.
#[must_use]
pub fn tested_name(rel: &str) -> Option<&str> {
    if !is_test_path(rel) {
        return None;
    }
    let data = data();
    let file = rel.rsplit(['/', '\\']).next()?;
    let stem_orig = stem_of(file);
    let stem = stem_orig.to_ascii_lowercase();
    let cut_end = |len: usize| stem_orig[..stem_orig.len() - len].trim_end_matches(['.', '_', '-']);
    let name = if let Some(suffix) = data.stem_suffixes.iter().find(|s| stem.ends_with(s.as_str())) {
        cut_end(suffix.len())
    } else if let Some(prefix) = data.stem_prefixes.iter().find(|p| stem.starts_with(p.as_str())) {
        stem_orig[prefix.len()..].trim_start_matches(['.', '_', '-'])
    } else if let Some(len) = camel_suffix_len(&stem, stem_orig) {
        cut_end(len)
    } else {
        stem_orig
    };
    (!name.is_empty()).then_some(name)
}

/// Strip the final extension from a filename to get its stem. A leading dot is
/// not treated as an extension separator.
fn stem_of(file: &str) -> &str {
    match file.rfind('.') {
        Some(idx) if idx > 0 => &file[..idx],
        _ => file,
    }
}

/// The length of the camelCase test word of the data that `stem` (lowered)
/// ends with, when, in the original-case `stem_orig`, that word begins a
/// capitalised word preceded by a lowercase letter or digit — the `FooSpec` /
/// `OrderTest` convention. `None` for `latest` / `attest`, where the trailing
/// letters are not a separate word.
fn camel_suffix_len(stem: &str, stem_orig: &str) -> Option<usize> {
    if stem.len() != stem_orig.len() {
        // Lengths differ only under non-ASCII case folding; fall back to no
        // camel match rather than risk a byte-index mismatch.
        return None;
    }
    for suffix in &data().camel_suffixes {
        if !stem.ends_with(suffix.as_str()) {
            continue;
        }
        let start = stem.len() - suffix.len();
        if start == 0 {
            // The whole stem is exactly `test`/`spec`; that is covered by the
            // segment rule when it is a directory, and a bare file stem of
            // `test`/`spec` is not a camelCase compound.
            continue;
        }
        let bytes = stem_orig.as_bytes();
        let prev = bytes[start - 1] as char;
        let first = bytes[start] as char;
        if (prev.is_ascii_lowercase() || prev.is_ascii_digit()) && first.is_ascii_uppercase() {
            return Some(suffix.len());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_segment_matches() {
        assert!(is_test_path("tests/"));
        assert!(is_test_path("tests/foo.rs"));
        assert!(is_test_path("src/__tests__/x.ts"));
        assert!(is_test_path("pkg/spec/runner.rb"));
        assert!(is_test_path("a/specs/b.rs"));
        assert!(is_test_path("data/testdata/sample.json"));
        assert!(is_test_path("a/fixtures/b.json"));
        assert!(is_test_path("a/__mocks__/b.js"));
    }

    #[test]
    fn test_stem_suffix_and_prefix_match() {
        assert!(is_test_path("src/foo.test.ts"));
        assert!(is_test_path("foo_test.go"));
        assert!(is_test_path("test_foo.py"));
        assert!(is_test_path("bar.spec.js"));
        assert!(is_test_path("a/b-test.kt"));
        assert!(is_test_path("a/widget-spec.rb"));
        // A stem ending in `tests` (no extension separator) is a test stem.
        assert!(is_test_path("integrationtests.go"));
        // `spec_` is NOT a prefix convention. The spec conventions in use are
        // suffixes (`widget-spec.rb`, `bar.spec.js`), both asserted above,
        // while `spec_*` names modules about specifications.
        assert!(!is_test_path("src/spec_views.rs"), "a module about specs is not a test");
        assert!(!is_test_path("src/commands/spec_draft.rs"));
        // `test_` stays: it is the established convention.
        assert!(is_test_path("test_runner.rb"));
    }

    #[test]
    fn an_ambiguous_segment_counts_only_where_the_convention_puts_it() {
        // `spec/` beside the source tree is the convention; `spec/` inside it
        // is a domain folder. Same word, decided by where it sits — the
        // distinction that cost this workspace 46 production modules.
        assert!(is_test_path("spec/models/user_spec.rb"), "project-root spec/ is test terrain");
        assert!(is_test_path("apps/api/spec/thing.rb"), "one level of monorepo nesting still is");
        assert!(!is_test_path("apps/rt/src/commands/spec/cli.rs"), "inside the source tree it is a domain folder");
        assert!(!is_test_path("packages/core/src/domain/spec/contract.rs"));
        // Unambiguous names need no such qualification — nobody calls a domain
        // folder `__tests__`, so depth never rescues one.
        assert!(is_test_path("apps/rt/src/deep/nested/__tests__/x.ts"));
        assert!(is_test_path("apps/rt/src/deep/nested/fixtures/x.rs"));
    }

    #[test]
    fn foospec_camel_stem_is_test() {
        // `FooSpec` / `OrderTest` — the suffix begins a capitalised camelCase
        // word, so the stem reads as a spec/test by convention.
        assert!(is_test_path("FooSpec.ts"));
        assert!(is_test_path("a/b/OrderTest.java"));
        assert!(is_test_path("UserServiceSpec.scala"));
        // `latest` / `attest` end in the same letters but are not a word
        // boundary — they must NOT match.
        assert!(!is_test_path("src/latest.rs"));
        assert!(!is_test_path("src/attest.go"));
    }

    #[test]
    fn non_test_paths_rejected() {
        assert!(!is_test_path("src/models.rs"));
        assert!(!is_test_path("attestation/x.rs"));
        assert!(!is_test_path("src/domain/config.rs"));
        // `attestation` contains `test` as a substring but not as a segment.
        assert!(!is_test_path("a/attestation/b.rs"));
        // A plain dir that merely contains `spec` as substring is not a match.
        assert!(!is_test_path("src/specimens/x.rs"));
    }

    #[test]
    fn backslash_paths_are_normalised() {
        assert!(is_test_path(r"src\__tests__\x.ts"));
        assert!(is_test_path(r"src\foo.test.ts"));
        assert!(!is_test_path(r"src\models.rs"));
    }

    #[test]
    fn the_tested_name_is_the_test_name_without_its_mark() {
        // Uma marca de cada forma: sufixo com ponto, sufixo com sublinhado,
        // prefixo e a palavra camelCase.
        assert_eq!(tested_name("src/foo.spec.ts"), Some("foo"));
        assert_eq!(tested_name("pkg/foo_test.go"), Some("foo"));
        assert_eq!(tested_name("tests/test_foo.py"), Some("foo"));
        assert_eq!(tested_name("Orders/FooTest.cs"), Some("Foo"));
        // O nome com ponto no meio perde só a marca do fim.
        assert_eq!(tested_name("src/x.service.spec.ts"), Some("x.service"));
        assert_eq!(tested_name("FooTests.cs"), Some("Foo"));
        assert_eq!(tested_name(r"src\foo.test.ts"), Some("foo"));
        // Teste só pela pasta: não há marca a tirar.
        assert_eq!(tested_name("tests/foo.rs"), Some("foo"));
        // Só a marca, nada sobra.
        assert_eq!(tested_name("src/tests.rs"), None);
        // Arquivo que não é teste não testa nome nenhum, nem quando termina
        // nas mesmas letras da marca.
        assert_eq!(tested_name("src/foo.ts"), None);
        assert_eq!(tested_name("src/latest.rs"), None);
    }

    #[test]
    fn the_embedded_data_reads_and_every_list_has_entries() {
        let data = data();
        for (name, list) in [
            ("dirs", &data.dirs),
            ("ambiguous_dirs", &data.ambiguous_dirs),
            ("project_dir_endings", &data.project_dir_endings),
            ("stem_suffixes", &data.stem_suffixes),
            ("camel_suffixes", &data.camel_suffixes),
            ("stem_prefixes", &data.stem_prefixes),
            ("inline_markers", &data.inline_markers),
        ] {
            assert!(!list.is_empty(), "a lista {name} saiu vazia do arquivo de dados");
        }
        assert_eq!(data.ambiguous_max_depth, 2);
        assert_eq!(inline_test_markers(), ["#[cfg(test)]".to_string()]);
    }

    #[test]
    fn the_embedded_entry_file_list_reads_and_no_language_comes_empty() {
        let data = entry_data();
        assert!(!data.is_empty(), "a lista dos arquivos de entrada saiu vazia do arquivo de dados");
        for (language, names) in data {
            assert!(!names.is_empty(), "a língua {language} veio sem nenhum nome de entrada");
        }
        assert_eq!(entry_file_names("typescript"), ["index".to_string()]);
        assert!(is_entry_file("src/pasta/index.ts", "typescript"));
        assert!(!is_entry_file("src/pasta/index.go", "go"));
        // O nome se compara inteiro e com a caixa como está.
        assert!(!is_entry_file("src/pasta/index.d.ts", "typescript"));
        assert!(!is_entry_file("src/pasta/Index.ts", "typescript"));
        assert!(!is_entry_file("src/pasta/reindex.ts", "typescript"));
    }

    #[test]
    fn the_embedded_nested_module_list_reads_and_each_language_has_entry_files() {
        let data = nested_data();
        assert!(!data.is_empty(), "a lista dos módulos filhos saiu vazia do arquivo de dados");
        for language in data.keys() {
            assert!(
                !entry_file_names(language).is_empty(),
                "a língua {language} nomeia módulo filho e não tem arquivo de entrada para achar a pasta do pai"
            );
        }
    }

    #[test]
    fn a_child_module_lives_in_the_folder_of_the_module_of_its_parent() {
        // `a.rs` e `a/mod.rs` têm os filhos em `a/`; o crate declara os
        // módulos de topo na pasta do `lib` e do `main`.
        assert!(is_declared_child("src/a.rs", "src/a/x.rs", "rust"));
        assert!(is_declared_child("src/a/mod.rs", "src/a/x.rs", "rust"));
        assert!(is_declared_child("src/a.rs", "src/a/b/c.rs", "rust"));
        assert!(is_declared_child("src/lib.rs", "src/x.rs", "rust"));
        assert!(is_declared_child("src/main.rs", "src/x/y.rs", "rust"));
        assert!(is_declared_child("a.rs", "a/x.rs", "rust"));
        assert!(is_declared_child(r"src\a.rs", r"src\a\x.rs", "rust"));
        assert!(is_declared_child("v1.2/a.rs", "v1.2/a/x.rs", "rust"));
        // O irmão, o filho de outro pai, a pasta de nome parecido, a pasta de
        // fora e o próprio arquivo não são filhos.
        assert!(!is_declared_child("src/a.rs", "src/b.rs", "rust"));
        assert!(!is_declared_child("src/a/x.rs", "src/a/y.rs", "rust"));
        assert!(!is_declared_child("src/a.rs", "src/ab/x.rs", "rust"));
        assert!(!is_declared_child("src/a.rs", "lib/a/x.rs", "rust"));
        assert!(!is_declared_child("src/a/mod.rs", "src/b/x.rs", "rust"));
        assert!(!is_declared_child("src/a.rs", "src/a.rs", "rust"));
        assert!(!is_declared_child("src/a.rs", "src/a", "rust"));
    }

    #[test]
    fn only_a_language_that_nests_its_modules_has_declared_children() {
        // O `index` que reexporta o `./x` e o `./x` que importa o `index` são
        // um ciclo de verdade no TypeScript; a língua sem chave não declara
        // filho, nem a desconhecida.
        assert!(!is_declared_child("src/pasta/index.ts", "src/pasta/x.ts", "typescript"));
        assert!(!is_declared_child("src/a.ts", "src/a/x.ts", "typescript"));
        assert!(!is_declared_child("src/a.py", "src/a/x.py", "python"));
        assert!(!is_declared_child("src/a.rs", "src/a/x.rs", ""));
        assert!(!is_declared_child("src/a.rs", "src/a/x.rs", "unknown"));
    }

    #[test]
    fn end_to_end_folders_a_top_level_integration_folder_and_a_test_project_folder_hold_tests() {
        // Pastas de ponta a ponta em qualquer profundidade, o sufixo `.cy`, a
        // pasta ambígua no topo e a pasta de projeto de teste pelo fim do nome.
        let paths = [
            "e2e/helpers/login.ts",
            "apps/web/e2e/pedido.ts",
            "cypress/support/commands.ts",
            "app/login.cy.ts",
            "integration/pedidos.ts",
            "a/b/integration/x.ts",
            "MeuApp.Tests/Helpers/Fixture.cs",
            "src/Loja.UnitTests/PedidoFixture.cs",
            "src/Loja.IntegrationTests/Api/Setup.cs",
            "Loja.Test/Base.cs",
            "front/Loja.Specs/x.ts",
            r"MeuApp.Tests\Helpers\Fixture.cs",
        ];
        let missed: Vec<&str> = paths.into_iter().filter(|path| !is_test_path(path)).collect();
        assert!(missed.is_empty(), "deveriam ser teste: {missed:?}");
    }

    #[test]
    fn domain_folders_that_resemble_the_new_test_names_stay_production() {
        // A pasta das integrações com outro sistema, a pasta ambígua fundo
        // demais, a pasta cujo nome termina nas letras sem o ponto, o arquivo
        // cujo nome termina numa terminação de pasta, e `latest`.
        let paths = [
            "src/integrations/pagamento.ts",
            "a/b/c/integration/x.ts",
            "Contests/Placar.cs",
            "src/Contest/Placar.cs",
            "src/Pedido.Tests",
            "latest.ts",
            "src/policy.ts",
        ];
        let taken: Vec<&str> = paths.into_iter().filter(|path| is_test_path(path)).collect();
        assert!(taken.is_empty(), "deveriam seguir produção: {taken:?}");
    }

    #[test]
    fn an_end_to_end_spec_file_tests_the_name_before_its_mark() {
        assert_eq!(tested_name("app/login.cy.ts"), Some("login"));
        assert_eq!(tested_name("cypress/e2e/pedido.cy.js"), Some("pedido"));
    }
}
