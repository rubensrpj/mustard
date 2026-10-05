//! Characterization + contract tests for import resolution in `graph::build`.
//!
//! One minimal fixture per language under `tests/fixtures/graph_<lang>/`, each
//! with a REAL internal import in that language's idiomatic shape:
//!   * csharp      `using Demo.Models;`            (namespace import)
//!   * typescript  `import { User } from "./user"` (relative path import)
//!   * go          `import "module/internal/model"` (module-prefixed path)
//!   * python      `from mypkg.models import X`     (dotted module path)
//!   * rust        `use crate::util::helper;`       (root-alias + `::` path)
//!   * php         `use App\Models\User;`           (FQCN naming a type)
//!     plus `graph_rust_external_std/`: an EXTERNAL `use std::collections::HashMap;`
//!     beside an internal `src/collections.rs` — must yield ZERO edges (the
//!     root-alias branch only runs for declared aliases like `crate`).
//!     E `graph_typescript_aliases/`: import com ponto no nome
//!     (`./pedido.service`), import com a extensão de saída
//!     (`./pedido.service.js`) e apelido de pasta (`@app/pedido`) declarado na
//!     configuração herdada pela da raiz e redeclarado numa subpasta.
//!     E `graph_python_relative/`: import relativo com um ponto, com dois,
//!     `from . import x`, pasta de pacote com `__init__.py` e import absoluto.
//!     E `graph_csharp_namespace/`: três arquivos num namespace, importado por
//!     quem usa um tipo, por quem não usa nada e pelo nome qualificado.
//!     E `graph_rust_qualified/`: chamadas pelo caminho completo no corpo, sem
//!     `use` — a partir de `crate`, a partir de `super` e por um caminho de
//!     fora do projeto (`std::fs::read`); subindo duas pastas
//!     (`super::super::x::f()`), só o item depois do `super` (`super::sum()`),
//!     com argumento de tipo (`crate::a::Boxed::<u8>::new()`), por um módulo
//!     escrito dentro do arquivo (`crate::a::inside::Jar::new()`), a partir
//!     do arquivo que responde pela própria pasta (`src/k/mod.rs`) e por um
//!     caminho que não nomeia nada do projeto (`crate::nothing::f()`, ao lado de
//!     um `src/a/nothing.rs`).
//!     E `graph_rust_inner_module/`: `super` escrito dentro de um módulo do
//!     próprio arquivo (`mod inner { }` de `src/a.rs`) e dentro do trecho de
//!     teste, que sai do módulo antes de subir pasta, também na passada que
//!     reaproveita o arquivo sem relê-lo.
//!     E `graph_rust_call_path/`: `value` declarado em `src/a.rs` e em
//!     `src/x.rs`, chamado de `src/a.rs` por `super::super::x::value()`, por
//!     `super::value()` dentro de um módulo do arquivo e sem caminho.
//!
//! Characterization baseline (recorded on the code BEFORE the resolution fix):
//! csharp, typescript and go already produced edges; python, rust and php
//! produced 0 edges — PHP FQCNs never matched the namespace index, dotted /
//! `::` paths never reached the path branch (it required a literal '/').
//! After the fix every language must yield edges > 0, and the three languages
//! that already resolved must keep exactly the same edges (non-regression).
//!
//! The fixtures live in dedicated `graph_*` dirs so they never collide with the
//! php_laravel / python_django fixtures used by other test files.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;
#[path = "support/model.rs"]
mod model;

use std::path::PathBuf;
use std::process::Command;

/// A committed fixture root, resolved from the crate manifest dir.
fn fixture(name: &str) -> PathBuf {
    manifest_dir::manifest_dir().join("tests").join("fixtures").join(name)
}

/// Scan a fixture into a temp map and return the parsed value.
/// Mirrors `php_laravel_fixture.rs`: a per-CALL temp dir (label + fixture name
/// + pid) so parallel tests scanning the same fixture never yank each other's
///   dir (the per-language test and the non-regression test share fixtures).
fn scan_fixture_labeled(label: &str, name: &str) -> serde_json::Value {
    let temp = tempfile::Builder::new().prefix(&format!("scan-graph-{}-{}-", label, name)).tempdir().unwrap();
    model::scan(&fixture(name), temp.path(), &[]).0
}

/// Per-language entry point: the test fn name doubles as the temp-dir label.
fn scan_fixture(name: &str) -> serde_json::Value {
    scan_fixture_labeled("lang", name)
}

fn edges(v: &serde_json::Value) -> u64 {
    v["graph"]["edges"].as_u64().expect("graph.edges")
}

/// The modules ranked by fan-in (the import TARGETS) — enough to pin which
/// node the single fixture edge points at.
fn fan_in_modules(v: &serde_json::Value) -> Vec<String> {
    v["graph"]["top_fan_in"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["module"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn graph_resolution_python() {
    let v = scan_fixture("graph_python");
    assert!(edges(&v) > 0, "Python `from mypkg.models import X` must resolve to an internal edge: {}", v["graph"]);
}

#[test]
fn graph_resolution_rust() {
    let v = scan_fixture("graph_rust");
    assert!(edges(&v) > 0, "Rust `use crate::util::helper;` must resolve to an internal edge: {}", v["graph"]);
}

/// Regression — root-alias false positive: a fully EXTERNAL import must never
/// edge to an internal module that happens to share a segment name. Before the
/// `root_aliases` gate, the root-alias branch dropped the FIRST segment of any
/// `::`/`\` import and probed the tail against the importer's ancestor dirs,
/// so `use std::collections::HashMap;` plus an internal `src/collections.rs`
/// produced a false edge (reproduced: edges=1). `std` is not a declared root
/// alias for Rust, so the branch must not run at all: edges == 0.
#[test]
fn graph_resolution_rust_external_std_no_false_edge() {
    let v = scan_fixture("graph_rust_external_std");
    assert_eq!(
        edges(&v),
        0,
        "external `std::collections::HashMap` must not edge to src/collections.rs: {}",
        v["graph"]
    );
}

/// Non-regression: the languages that already resolved BEFORE the fix (see the
/// header) must keep exactly the same edges — same count, same target module.
#[test]
fn graph_resolution_no_regression_preexisting() {
    for (fixture_name, target) in [
        ("graph_csharp", "src/Models/User.cs"),
        ("graph_typescript", "src/user.ts"),
        ("graph_go", "internal/model/user.go"),
    ] {
        let v = scan_fixture_labeled("noregress", fixture_name);
        assert_eq!(edges(&v), 1, "{fixture_name}: exactly the one pre-fix edge: {}", v["graph"]);
        assert_eq!(
            fan_in_modules(&v),
            vec![target.to_string()],
            "{fixture_name}: the edge still points at the same module"
        );
    }
}

/// Cascade smoke: with PHP FQCNs resolving, every internal import becomes an
/// edge and the fan-in stops being empty.
/// Fixture shape: 3 Models <- 2 Services <- 2 Controllers, every import an
/// internal FQCN (`App\Models\User`, `App\Services\UserService`, ...):
///   UserService -> User; PostService -> Post, Comment;
///   UserController -> UserService, User; PostController -> PostService, Post.
#[test]
fn graph_resolution_php_cascade_resolves_every_import_and_fills_fan_in() {
    let v = scan_fixture("graph_php_cascade");
    let g = &v["graph"];

    assert_eq!(g["edges"].as_u64(), Some(7), "all 7 internal FQCN imports resolve: {g}");

    // Fan-in: the models are depended upon (User and Post twice each).
    let fan_in = fan_in_modules(&v);
    assert!(!fan_in.is_empty(), "fan-in must not be empty: {g}");
    assert!(
        fan_in.contains(&"app/Models/User.php".to_string()),
        "User model is a fan-in target: {fan_in:?}"
    );
}

/// Os arquivos do projeto que um módulo importa, como o mapa os grava.
fn deps_of(v: &serde_json::Value, path: &str) -> Vec<String> {
    let module = v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} está no mapa"));
    module["deps"]
        .as_array()
        .map(|deps| deps.iter().map(|d| d.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

/// Confere, para cada caso `(nome, módulo, esperado)`, os arquivos do projeto
/// que o módulo importa; a mensagem diz qual caso falhou.
fn assert_deps_of(v: &serde_json::Value, cases: &[(&str, &str, &[&str])]) {
    for (case, path, want) in cases {
        let want: Vec<String> = want.iter().map(|d| d.to_string()).collect();
        assert_eq!(deps_of(v, path), want, "{case}");
    }
}

/// Os apelidos e os nomes de arquivo do TypeScript numa pasta lida uma vez:
///   * um import sem extensão cujo nome tem ponto (`./pedido.service`) liga ao
///     arquivo com esse nome inteiro, e não ao `pedido.ts` ao lado: o ponto do
///     meio é parte do nome;
///   * o import que escreve a extensão de saída no lugar da do arquivo
///     (`./pedido.service.js` para `pedido.service.ts`) segue ligando ao
///     arquivo certo;
///   * o apelido de pasta declarado numa configuração herdada pela da raiz
///     (`@app/*` para `src/app/*`, com comentário e vírgula sobrando no
///     arquivo) liga `@app/pedido` a `src/app/pedido.ts`;
///   * a configuração mais próxima de quem importa vence a da raiz: em `pkg/`,
///     `@app/pedido` é `pkg/lib/pedido.ts`, e não o `src/app/pedido.ts` que a
///     raiz daria.
#[test]
fn graph_typescript_aliases() {
    let v = scan_fixture_labeled("aliases", "graph_typescript_aliases");
    assert_deps_of(
        &v,
        &[
            (
                "a dotted import links to the file with the whole name",
                "src/app/checkout.ts",
                &["src/app/pedido.service.ts"],
            ),
            (
                "an import written with the output extension still links",
                "src/app/esm.ts",
                &["src/app/pedido.service.ts"],
            ),
            (
                "a folder alias inherited through extends links to the right file",
                "src/usa_apelido.ts",
                &["src/app/pedido.ts"],
            ),
            ("the nearest configuration wins over the root one", "pkg/usa.ts", &["pkg/lib/pedido.ts"]),
        ],
    );
}

/// A configuração de apelidos de pasta do projeto só de JavaScript: `@/*` para
/// `src/*`.
const JS_CONFIG_AT_SRC: &str = r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#;

/// O projeto só de JavaScript lê os apelidos da configuração dele: o `.jsx`
/// que importa `@/servicos/pedido` depende do `.js` que a pasta nomeia.
#[test]
fn a_folder_alias_from_the_javascript_configuration_links_a_javascript_import() {
    let v = scan_files(
        "js-alias",
        &[
            ("jsconfig.json", JS_CONFIG_AT_SRC),
            ("src/servicos/pedido.js", "export function buscarPedido(id) {\n  return id;\n}\n"),
            (
                "src/telas/Pedido.jsx",
                "import { buscarPedido } from '@/servicos/pedido';\n\nexport function Pedido() {\n  return buscarPedido(1);\n}\n",
            ),
        ],
    );
    assert_eq!(deps_of(&v, "src/telas/Pedido.jsx"), vec!["src/servicos/pedido.js".to_string()]);
}

/// No projeto com a configuração do TypeScript, o `.js` lê os mesmos apelidos
/// que o `.ts` ao lado dele.
#[test]
fn a_javascript_file_in_a_typescript_project_reads_the_typescript_configuration() {
    let v = scan_files(
        "js-in-ts-alias",
        &[
            ("tsconfig.json", r#"{"compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#),
            ("src/servicos/pedido.js", "export function buscarPedido(id) {\n  return id;\n}\n"),
            ("src/telas/outra.ts", "import { buscarPedido } from '@/servicos/pedido';\n\nexport const x = buscarPedido(1);\n"),
            ("src/telas/pedido.js", "import { buscarPedido } from '@/servicos/pedido';\n\nexport const y = buscarPedido(2);\n"),
        ],
    );
    assert_eq!(deps_of(&v, "src/telas/outra.ts"), vec!["src/servicos/pedido.js".to_string()]);
    assert_eq!(deps_of(&v, "src/telas/pedido.js"), vec!["src/servicos/pedido.js".to_string()]);
}

/// Na mesma pasta, a configuração do TypeScript vence a do JavaScript, na
/// ordem que o registro dá.
#[test]
fn in_the_same_folder_the_typescript_configuration_wins_over_the_javascript_one() {
    let v = scan_files(
        "js-ts-same-folder",
        &[
            ("tsconfig.json", r#"{"compilerOptions":{"paths":{"@/*":["src/a/*"]}}}"#),
            ("jsconfig.json", r#"{"compilerOptions":{"paths":{"@/*":["src/b/*"]}}}"#),
            ("src/a/x.js", "export const x = 1;\n"),
            ("src/b/x.js", "export const x = 2;\n"),
            ("src/usa.js", "import { x } from '@/x';\n\nexport const y = x;\n"),
        ],
    );
    assert_eq!(deps_of(&v, "src/usa.js"), vec!["src/a/x.js".to_string()]);
}

/// A configuração do JavaScript mais próxima de quem importa vence a do
/// TypeScript de uma pasta acima.
#[test]
fn a_nearer_javascript_configuration_wins_over_a_typescript_one_above() {
    let v = scan_files(
        "js-nearer",
        &[
            ("tsconfig.json", r#"{"compilerOptions":{"paths":{"@/*":["src/*"]}}}"#),
            ("web/jsconfig.json", r#"{"compilerOptions":{"paths":{"@/*":["lib/*"]}}}"#),
            ("src/x.js", "export const x = 1;\n"),
            ("web/lib/x.js", "export const x = 2;\n"),
            ("web/usa.js", "import { x } from '@/x';\n\nexport const y = x;\n"),
        ],
    );
    assert_eq!(deps_of(&v, "web/usa.js"), vec!["web/lib/x.js".to_string()]);
}

/// Os imports relativos do Python numa pasta lida uma vez:
///   * um ponto na frente é a pasta de quem importa: `from .models import
///     Pedido` em `pkg/views.py` liga a `pkg/models.py`, ao lado;
///   * dois pontos sobem uma pasta, e o resto, cortado nos pontos, é o caminho
///     dentro dela: `from ..core.regras import LIMITE` em
///     `pkg/api/handlers.py` liga a `pkg/core/regras.py`;
///   * `from . import models` traz um arquivo da própria pasta: liga a
///     `pkg/models.py`, e não à pasta nem ao `__init__.py` dela;
///   * o import relativo que nomeia uma pasta de pacote liga ao `__init__.py`
///     dela: `from .servicos import cobrar` liga a
///     `pkg/servicos/__init__.py`;
///   * o import sem ponto segue lido a partir da raiz, e não da pasta de quem
///     importa: `from pkg.models import Pedido` em `pkg/api/absoluto.py` liga
///     a `pkg/models.py`.
#[test]
fn graph_python_relative() {
    let v = scan_fixture_labeled("py-relative", "graph_python_relative");
    assert_deps_of(
        &v,
        &[
            ("a one-dot relative import links to the file beside", "pkg/views.py", &["pkg/models.py"]),
            ("a two-dot relative import climbs one folder", "pkg/api/handlers.py", &["pkg/core/regras.py"]),
            ("a from-dot import links to the named file of the folder", "pkg/admin.py", &["pkg/models.py"]),
            (
                "a relative import of a package links to its init file",
                "pkg/usa_pacote.py",
                &["pkg/servicos/__init__.py"],
            ),
            ("an absolute import is still read from the root", "pkg/api/absoluto.py", &["pkg/models.py"]),
        ],
    );
}

/// Os imports de namespace do C# numa pasta lida uma vez, com três arquivos no
/// mesmo namespace:
///   * quem importa o namespace e usa só um tipo dele liga só ao arquivo desse
///     tipo;
///   * quem importa o namespace e não usa nada dele não liga a nenhum arquivo;
///   * o nome qualificado completo de um tipo segue ligando ao arquivo que leva
///     o nome dele, use o arquivo o tipo ou não;
///   * o nome qualificado de um tipo que nenhum arquivo leva no nome liga só ao
///     arquivo do namespace que declara o que quem importa usa, e não ao
///     namespace inteiro: `Aplicar`, de `Desconto`, mora em `Produto.cs`.
#[test]
fn graph_csharp_namespace() {
    let v = scan_fixture_labeled("ns", "graph_csharp_namespace");
    assert_deps_of(
        &v,
        &[
            (
                "a namespace import links only to the file of the type it uses",
                "src/Services/PedidoService.cs",
                &["src/Models/Pedido.cs"],
            ),
            ("a namespace import with nothing used links to nothing", "src/Services/SemUso.cs", &[]),
            (
                "a fully qualified import still links to the file of the type",
                "src/Services/Qualificado.cs",
                &["src/Models/Cliente.cs"],
            ),
            (
                "a qualified type without its own file links only to what is used",
                "src/Services/SemArquivo.cs",
                &["src/Models/Produto.cs"],
            ),
        ],
    );
}

/// Os imports que o mapa grava para um módulo, como foram escritos.
fn imports_of(v: &serde_json::Value, path: &str) -> Vec<String> {
    let module = v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} está no mapa"));
    module["imports"]
        .as_array()
        .map(|imports| imports.iter().map(|i| i.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

/// As chamadas do Rust escritas pelo caminho completo no corpo, sem `use`,
/// numa pasta lida uma vez:
///   * `crate::a::b::f()` em `src/main.rs` liga o arquivo ao que o caminho
///     nomeia: `src/a/b.rs`;
///   * o caminho que começa em `super` é lido a partir da pasta de quem chama:
///     `super::x::f()` em `src/a/b.rs` liga a `src/a/x.rs`;
///   * o caminho que não começa no próprio projeto (`std::fs::read()`,
///     `Vec::new()`) não vira import, e o nome chamado segue lido como uso, com
///     o qualificador de antes: dos três caminhos de `src/main.rs`, só
///     `crate::a::b` é import;
///   * cada `super` a mais sobe uma pasta: `super::super::x::f()` em
///     `src/a/c/d.rs` liga a `src/a/x.rs`;
///   * o `super` seguido só do item liga ao arquivo que responde pela pasta de
///     cima: `super::sum()` em `src/a/y.rs` liga a `src/a.rs`;
///   * os argumentos de tipo saem do caminho: `crate::a::Boxed::<u8>::new()`
///     vira o import `crate::a::Boxed` e liga a `src/a.rs`, onde `Boxed` mora;
///   * o caminho que passa por um módulo escrito dentro do arquivo, e termina
///     em tipo, perde do fim quantas partes for preciso até achar arquivo:
///     `crate::a::inside::Jar::new()` liga a `src/a.rs`;
///   * o arquivo que responde pela própria pasta já é o módulo dela, e o
///     `super` dele sobe a partir da pasta de cima: `super::a::x::f()` em
///     `src/k/mod.rs` liga a `src/a/x.rs`;
///   * o caminho que não nomeia nada do projeto não liga a um arquivo de outro
///     lugar só porque o caminho dele termina igual: `crate::nothing::f()` em
///     `src/no_target.rs` não liga a `src/a/nothing.rs`.
#[test]
fn graph_rust_qualified() {
    let v = scan_fixture_labeled("rs-qualified", "graph_rust_qualified");
    assert_deps_of(
        &v,
        &[
            ("a call by the full path from the crate links to the file", "src/main.rs", &["src/a/b.rs"]),
            ("a call by the full path from super links to the file beside", "src/a/b.rs", &["src/a/x.rs"]),
            ("the file beside the super call links to nothing", "src/a/x.rs", &[]),
            ("a call that climbs two folders links to the file there", "src/a/c/d.rs", &["src/a/x.rs"]),
            ("a call by super and the item links to the file of the folder above", "src/a/y.rs", &["src/a.rs"]),
            ("a call with type arguments in the path links to the file", "src/generic.rs", &["src/a.rs"]),
            ("a path through a module inside the file links to the file", "src/inner.rs", &["src/a.rs"]),
            ("the file that answers for its folder climbs from the folder above", "src/k/mod.rs", &["src/a/x.rs"]),
            ("a crate path that names nothing does not link by the end of another path", "src/no_target.rs", &[]),
        ],
    );

    assert_eq!(
        imports_of(&v, "src/main.rs"),
        vec!["crate::a::b".to_string()],
        "a call by a path outside the project is not an import"
    );
    let module = v["modules"].as_array().unwrap().iter().find(|m| m["path"] == "src/main.rs").unwrap();
    let calls: Vec<&str> = module["calls"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert!(calls.contains(&"fs.read:3"), "a chamada de fora segue como uso: {calls:?}");
    assert_eq!(
        imports_of(&v, "src/generic.rs"),
        vec!["crate::a::Boxed".to_string()],
        "a call with type arguments in the path drops them from the import"
    );
    assert_eq!(
        imports_of(&v, "src/no_target.rs"),
        vec!["crate::nothing".to_string()],
        "a crate path that names nothing is still written as an import"
    );
}

/// O que o mapa grava em `key` para o arquivo `path`, como lista de textos.
fn list_of(v: &serde_json::Value, path: &str, key: &str) -> Vec<String> {
    let module = v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} está no mapa"));
    module[key]
        .as_array()
        .map(|items| items.iter().map(|i| i.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

/// Quem usa a declaração `name` do arquivo `path`, como o mapa grava.
fn used_by_of(v: &serde_json::Value, path: &str, name: &str) -> Vec<String> {
    let module = v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} está no mapa"));
    let decl = module["declarations"]
        .as_array()
        .expect("declarations")
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("{name} está em {path}"));
    decl["used_by"]
        .as_array()
        .map(|items| items.iter().map(|i| i.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

/// O `super` escrito dentro de um módulo do próprio arquivo sai desse módulo e
/// fica no arquivo: `super::value()` dentro de `mod inner { }` de `src/a.rs`
/// é o `value` de `src/a.rs`, e não o `value` homônimo de `src/main.rs`, onde o
/// caminho cairia se subisse pasta direto.
fn assert_super_inside_a_module_stays_in_the_file(v: &serde_json::Value) {
    let case = "super inside a module of the file links to the file itself";
    assert_eq!(used_by_of(v, "src/a.rs", "value"), vec!["src/a.rs:7:near".to_string()], "{case}");
    assert_eq!(used_by_of(v, "src/main.rs", "value"), Vec::<String>::new(), "{case}");
    assert!(!deps_of(v, "src/a.rs").contains(&"src/main.rs".to_string()), "{case}: {:?}", deps_of(v, "src/a.rs"));
}

/// O segundo `super` escrito dentro do mesmo módulo é o que sobe pasta:
/// `super::super::x::double()` liga `src/a.rs` a `src/x.rs`.
fn assert_second_super_inside_a_module_climbs_one_folder(v: &serde_json::Value) {
    assert_eq!(
        deps_of(v, "src/a.rs"),
        vec!["src/x.rs".to_string()],
        "super super inside a module of the file climbs one folder"
    );
}

/// O `use super::*` do trecho de teste é o próprio arquivo: não vira arquivo
/// coberto pelo teste, e o arquivo de cima não ganha o teste.
fn assert_super_of_the_test_block_stays_in_the_file(v: &serde_json::Value) {
    let case = "super of the test block stays in the file";
    assert_eq!(list_of(v, "src/a.rs", "test_imports"), vec!["super::*".to_string()], "{case}");
    assert_eq!(list_of(v, "src/a.rs", "test_deps"), Vec::<String>::new(), "{case}");
    assert_eq!(list_of(v, "src/main.rs", "tests"), Vec::<String>::new(), "{case}");
}

/// O `super` dentro de um módulo do arquivo e do trecho de teste, numa pasta
/// lida uma vez: o primeiro fica no arquivo, o segundo sobe pasta e o do
/// trecho de teste é o próprio arquivo.
#[test]
fn graph_rust_inner_module() {
    let v = scan_fixture_labeled("rs-inner", "graph_rust_inner_module");
    assert_super_inside_a_module_stays_in_the_file(&v);
    assert_second_super_inside_a_module_climbs_one_folder(&v);
    assert_super_of_the_test_block_stays_in_the_file(&v);
}

fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Roda o scan dentro do projeto e devolve o mapa gravado e o relato da
/// passada.
fn scan_in_place(dir: &std::path::Path) -> (serde_json::Value, serde_json::Value) {
    model::scan(dir, &dir.join(".claude"), &[])
}

/// Duas passadas sobre a fixture `name` num projeto do git: a leitura inteira
/// e, depois de mudar só `src/main.rs`, a que reaproveita os outros arquivos
/// sem relê-los. Devolve o mapa de cada uma.
fn full_and_kept_pass(name: &str) -> (serde_json::Value, serde_json::Value) {
    full_and_kept_pass_of(name, |dir| copy_tree(&fixture(name), dir), ("src/main.rs", "\npub fn outra() {}\n"))
}

/// As duas passadas de [`full_and_kept_pass`] sobre a árvore que `fill`
/// escreve: a segunda depois de acrescentar `changed.1` ao fim do arquivo
/// `changed.0`, o único relido.
fn full_and_kept_pass_of(
    label: &str,
    fill: impl FnOnce(&std::path::Path),
    changed: (&str, &str),
) -> (serde_json::Value, serde_json::Value) {
    let temp = tempfile::Builder::new().prefix(&format!("scan-graph-reuse-{label}-")).tempdir().unwrap();
    let dir = temp.path();
    fill(dir);
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let (first, report) = scan_in_place(dir);
    assert_eq!(report["full"], serde_json::Value::Bool(true), "{report}");

    let (path, added) = changed;
    let text = std::fs::read_to_string(dir.join(path)).unwrap();
    std::fs::write(dir.join(path), format!("{text}{added}")).unwrap();
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (second, report) = scan_in_place(dir);
    assert_eq!(report["read"], serde_json::json!([path]), "só o que mudou é relido: {report}");
    (first, second)
}

/// A passada que reaproveita `src/a.rs` sem relê-lo sabe o mesmo que a
/// leitura inteira: os módulos do arquivo e as linhas dos imports escritos
/// neles ficam no mapa.
#[test]
fn a_pass_that_keeps_the_file_knows_the_same_modules_of_the_file() {
    let (first, second) = full_and_kept_pass("graph_rust_inner_module");
    for v in [&first, &second] {
        assert_super_inside_a_module_stays_in_the_file(v);
        assert_second_super_inside_a_module_climbs_one_folder(v);
        assert_super_of_the_test_block_stays_in_the_file(v);
    }
}

/// Os arquivos cuja declaração `name` tem `site` entre quem a usa, em ordem.
fn holders_of(v: &serde_json::Value, name: &str, site: &str) -> Vec<String> {
    let mut holders: Vec<String> = v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .filter(|m| {
            m["declarations"].as_array().is_some_and(|decls| {
                decls.iter().any(|d| {
                    d["name"] == name && d["used_by"].as_array().is_some_and(|used| used.iter().any(|u| u == site))
                })
            })
        })
        .map(|m| m["path"].as_str().unwrap().to_string())
        .collect();
    holders.sort();
    holders
}

/// A chamada escrita por um caminho que nomeia um arquivo liga só às
/// declarações dele: `super::super::x::value()`, na linha 15 de `src/a.rs`, é
/// o `value` de `src/x.rs`, e não o `value` homônimo do próprio arquivo.
fn assert_a_call_by_a_path_links_only_to_the_file_it_names(v: &serde_json::Value) {
    assert_eq!(
        holders_of(v, "value", "src/a.rs:15:far"),
        vec!["src/x.rs".to_string()],
        "a call by a path links only to the file it names"
    );
}

/// O próprio arquivo só entra quando o caminho o nomeia: `super::value()`
/// dentro de `mod inner`, na linha 11 de `src/a.rs`, é o `value` de
/// `src/a.rs`, e não o de `src/x.rs`, que o arquivo também importa.
fn assert_a_path_to_the_file_itself_links_only_to_the_file(v: &serde_json::Value) {
    assert_eq!(
        holders_of(v, "value", "src/a.rs:11:near"),
        vec!["src/a.rs".to_string()],
        "a path to the file itself links only to the file"
    );
}

/// Os arquivos cuja declaração `name` tem `site` entre os usos suspeitos, em
/// ordem, cada um com as candidatas que o uso traz.
fn suspect_holders_of(v: &serde_json::Value, name: &str, site: &str) -> Vec<(String, serde_json::Value)> {
    let mut holders: Vec<(String, serde_json::Value)> = Vec::new();
    for m in v["modules"].as_array().expect("modules") {
        for d in m["declarations"].as_array().into_iter().flatten().filter(|d| d["name"] == name) {
            for u in d["used_by"].as_array().into_iter().flatten().filter(|u| u["at"] == site) {
                holders.push((m["path"].as_str().unwrap().to_string(), u["candidates"].clone()));
            }
        }
    }
    holders.sort_by(|a, b| a.0.cmp(&b.0));
    holders
}

/// A chamada sem caminho do nome que o próprio arquivo declara fora de todo
/// tipo é essa declaração: `value()`, na linha 6 de `src/a.rs`, é o `value`
/// do próprio arquivo, provado, e não o de `src/x.rs`, que o arquivo também
/// importa pelo caminho da linha 15.
fn assert_a_call_without_a_path_links_to_the_file_own_declaration(v: &serde_json::Value) {
    let case = "a call without a path links to the file own declaration";
    assert_eq!(holders_of(v, "value", "src/a.rs:6:sum"), vec!["src/a.rs".to_string()], "{case}");
    assert!(suspect_holders_of(v, "value", "src/a.rs:6:sum").is_empty(), "{case}: nenhuma suspeita");
}

/// As chamadas de `src/a.rs` por caminho, numa pasta lida uma vez: a por
/// caminho liga só ao arquivo que nomeia, a que nomeia o próprio arquivo liga só
/// a ele e a sem caminho é a declaração do próprio arquivo.
#[test]
fn graph_rust_call_path() {
    let v = scan_fixture_labeled("rs-call-path", "graph_rust_call_path");
    assert_a_call_by_a_path_links_only_to_the_file_it_names(&v);
    assert_a_path_to_the_file_itself_links_only_to_the_file(&v);
    assert_a_call_without_a_path_links_to_the_file_own_declaration(&v);
}

/// A passada que reaproveita `src/a.rs` sem relê-lo liga as chamadas por
/// caminho como a leitura inteira: os caminhos e as chamadas escritas por
/// eles ficam no mapa.
#[test]
fn a_pass_that_keeps_the_file_links_the_calls_by_path_the_same() {
    let (first, second) = full_and_kept_pass("graph_rust_call_path");
    for v in [&first, &second] {
        assert_a_call_by_a_path_links_only_to_the_file_it_names(v);
        assert_a_path_to_the_file_itself_links_only_to_the_file(v);
        assert_a_call_without_a_path_links_to_the_file_own_declaration(v);
    }
}

/// Escreve os arquivos num projeto novo e devolve o mapa do scan dele.
fn scan_files(label: &str, files: &[(&str, &str)]) -> serde_json::Value {
    let temp = tempfile::Builder::new().prefix(&format!("scan-graph-{label}-")).tempdir().unwrap();
    let root = temp.path().join("repo");
    for (path, body) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, body).unwrap();
    }
    let out = temp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    model::scan(&root, &out, &[]).0
}

/// Um workspace de dois crates: o `lib.rs` do `core` (pacote `demo-core`)
/// repassa o `Leitor` de um módulo filho, e o `app` o importa pelo nome do
/// pacote, sem o caminho do arquivo que o declara.
const WORKSPACE_THAT_PASSES_ON: &[(&str, &str)] = &[
    ("Cargo.toml", "[workspace]\nmembers = [\"core\", \"app\"]\n"),
    ("core/Cargo.toml", "[package]\nname = \"demo-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    ("core/src/lib.rs", "pub mod io;\npub use io::leitor::Leitor;\n"),
    ("core/src/io/mod.rs", "pub mod leitor;\n"),
    ("core/src/io/leitor.rs", "pub struct Leitor;\n\nimpl Leitor {\n    pub fn novo() -> Self {\n        Leitor\n    }\n}\n"),
    (
        "app/Cargo.toml",
        "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\ndemo-core = { path = \"../core\" }\n",
    ),
    ("app/src/main.rs", "use demo_core::Leitor;\n\nfn main() {\n    let _ = Leitor::novo();\n}\n"),
];

/// `use demo_core::Leitor` não nomeia arquivo nenhum do pacote: cai no
/// arquivo raiz dele, que não declara o `Leitor`, mas o repassa, e a
/// dependência vai para o arquivo que o declara.
#[test]
fn a_package_import_of_a_name_the_root_file_passes_on_reaches_the_file_that_declares_it() {
    let v = scan_files("reexport-crate", WORKSPACE_THAT_PASSES_ON);
    assert_eq!(deps_of(&v, "app/src/main.rs"), vec!["core/src/io/leitor.rs".to_string()]);
}

/// `pub use io::leitor::Leitor`, escrito no `lib.rs`, começa pelo módulo filho
/// `io`, sem apelido: lê-se na pasta dos módulos do arquivo raiz, como se o
/// `self` viesse na frente.
#[test]
fn a_path_that_opens_with_a_child_module_resolves_inside_the_folder_of_its_modules() {
    let v = scan_files("reexport-child", WORKSPACE_THAT_PASSES_ON);
    assert_eq!(deps_of(&v, "core/src/lib.rs"), vec!["core/src/io/leitor.rs".to_string()]);
}

/// O serviço e a tela que importa a pasta dele; o `index.ts` da pasta, que só
/// repassa, é o arquivo dado.
fn folder_that_passes_on(index: &str, screen: &str) -> Vec<(&'static str, String)> {
    vec![
        ("web/src/pedidos/index.ts", index.to_string()),
        (
            "web/src/pedidos/pedido.service.ts",
            "export function buscarPedido(id: number) {\n  return id;\n}\n".to_string(),
        ),
        ("web/src/tela.ts", screen.to_string()),
    ]
}

fn scan_owned(label: &str, files: &[(&'static str, String)]) -> serde_json::Value {
    let files: Vec<(&str, &str)> = files.iter().map(|(path, body)| (*path, body.as_str())).collect();
    scan_files(label, &files)
}

/// `export * from './pedido.service'` no `index.ts` da pasta: quem importa a
/// pasta depende do serviço, que declara o nome trazido, e não do `index.ts`;
/// o `index.ts` depende do serviço que repassa.
#[test]
fn an_import_of_a_folder_that_passes_everything_on_reaches_the_file_that_declares_the_name() {
    let files = folder_that_passes_on(
        "export * from './pedido.service';\n",
        "import { buscarPedido } from './pedidos';\n\nexport function tela() {\n  return buscarPedido(1);\n}\n",
    );
    let v = scan_owned("reexport-star", &files);
    assert_eq!(deps_of(&v, "web/src/tela.ts"), vec!["web/src/pedidos/pedido.service.ts".to_string()]);
    assert_eq!(deps_of(&v, "web/src/pedidos/index.ts"), vec!["web/src/pedidos/pedido.service.ts".to_string()]);
}

/// `export { buscarPedido as buscar } from './pedido.service'`: quem importa
/// pede o nome novo (`buscar`), e o repasse o tira do serviço pelo de origem.
#[test]
fn a_name_passed_on_under_another_name_is_followed_by_its_original_name() {
    let files = folder_that_passes_on(
        "export { buscarPedido as buscar } from './pedido.service';\n",
        "import { buscar } from './pedidos';\n\nexport function tela() {\n  return buscar(1);\n}\n",
    );
    let v = scan_owned("reexport-renamed", &files);
    assert_eq!(deps_of(&v, "web/src/tela.ts"), vec!["web/src/pedidos/pedido.service.ts".to_string()]);
    assert_eq!(
        proven_uses_of(&v, "web/src/pedidos/pedido.service.ts", "buscarPedido"),
        vec!["web/src/tela.ts:4:tela".to_string()]
    );
}

/// Dois `index.ts` que repassam tudo um ao outro: a leitura termina, e o nome
/// que nenhum dos dois declara fica no `index.ts` importado.
#[test]
fn two_files_that_pass_everything_on_to_each_other_keep_the_name_nobody_declares() {
    let v = scan_files(
        "reexport-cycle",
        &[
            ("web/src/a/index.ts", "export * from '../b';\n"),
            ("web/src/b/index.ts", "export * from '../a';\n"),
            ("web/src/usa.ts", "import { ninguem } from './a';\n\nexport function usa() {\n  return ninguem();\n}\n"),
        ],
    );
    assert_eq!(deps_of(&v, "web/src/usa.ts"), vec!["web/src/a/index.ts".to_string()]);
}

/// O import de uma pasta chega só ao arquivo de entrada da língua de quem
/// importa: `./velho`, numa pasta que tem `main.ts` e nenhum `index.ts`, não
/// liga a nada — o `main` não abre pasta nenhuma nessa língua.
#[test]
fn an_import_of_a_folder_does_not_reach_a_file_that_is_not_an_entry_of_its_language() {
    let v = scan_files(
        "entry-not-main",
        &[
            ("src/velho/main.ts", "export function ler() {\n  return 1;\n}\n"),
            ("src/app.ts", "import { ler } from './velho';\n\nexport function app() {\n  return ler();\n}\n"),
        ],
    );
    assert_eq!(deps_of(&v, "src/app.ts"), Vec::<String>::new());
}

/// Um crate com `Cargo.toml`, o módulo `a` e o arquivo raiz `root`, que
/// declara `raiz`; `src/a.rs` chama `super::raiz()`.
fn crate_with_root(root: &'static str, body: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        (root, body),
        ("src/a.rs", "pub fn f() -> u8 {\n    super::raiz()\n}\n"),
    ]
}

/// O `super::` de um módulo de topo de um crate de biblioteca abre a raiz do
/// crate: `super::raiz()` em `src/a.rs` liga a `src/lib.rs`, que declara
/// `raiz`.
#[test]
fn super_from_a_top_module_of_a_library_crate_reaches_the_root_file() {
    let files = crate_with_root("src/lib.rs", "pub mod a;\n\npub fn raiz() -> u8 {\n    1\n}\n");
    let v = scan_files("entry-lib", &files);
    assert_eq!(deps_of(&v, "src/a.rs"), vec!["src/lib.rs".to_string()]);
}

/// O mesmo crate, com a raiz no `src/main.rs`: o `super::raiz()` de
/// `src/a.rs` segue ligando a ele.
#[test]
fn super_from_a_top_module_of_a_binary_crate_reaches_the_root_file() {
    let files = crate_with_root("src/main.rs", "mod a;\n\npub fn raiz() -> u8 {\n    1\n}\n\nfn main() {}\n");
    let v = scan_files("entry-main", &files);
    assert_eq!(deps_of(&v, "src/a.rs"), vec!["src/main.rs".to_string()]);
}

/// `require('./pasta')` chega ao `index.js` da pasta.
#[test]
fn a_require_of_a_folder_reaches_its_index_file() {
    let v = scan_files(
        "entry-require",
        &[
            ("pasta/index.js", "function ler() {\n  return 1;\n}\n\nmodule.exports = { ler };\n"),
            ("app.js", "const { ler } = require('./pasta');\n\nfunction app() {\n  return ler();\n}\n"),
        ],
    );
    assert_eq!(deps_of(&v, "app.js"), vec!["pasta/index.js".to_string()]);
}

/// O import não relativo que nomeia uma pasta de pacote chega ao
/// `__init__.py` dela: `from loja.servicos import cobrar` liga a
/// `loja/servicos/__init__.py`.
#[test]
fn an_absolute_import_of_a_package_reaches_its_init_file() {
    let v = scan_files(
        "entry-init",
        &[
            ("loja/servicos/__init__.py", "def cobrar():\n    return 1\n"),
            ("app.py", "from loja.servicos import cobrar\n\n\ndef app():\n    return cobrar()\n"),
        ],
    );
    assert_eq!(deps_of(&v, "app.py"), vec!["loja/servicos/__init__.py".to_string()]);
}

/// Dois projetos lado a lado, cada um com o seu `package.json` e um
/// `src/index.ts` de mesmo caminho: o import do servidor fica no servidor.
#[test]
fn an_import_by_path_stays_inside_the_project_of_the_importer() {
    let v = scan_files(
        "own-project",
        &[
            ("servidor/package.json", "{\n  \"name\": \"servidor\"\n}\n"),
            ("servidor/src/index.ts", "export function x() {\n  return 1;\n}\n"),
            ("servidor/src/app.ts", "import { x } from 'src/index';\n\nexport function app() {\n  return x();\n}\n"),
            ("tela/package.json", "{\n  \"name\": \"tela\"\n}\n"),
            ("tela/src/index.ts", "export function x() {\n  return 2;\n}\n"),
        ],
    );
    assert_eq!(deps_of(&v, "servidor/src/app.ts"), vec!["servidor/src/index.ts".to_string()]);
}

/// `b/c` não é o fim de `web/src/ab/c`: o trecho tem de começar numa parte
/// inteira do caminho.
#[test]
fn a_path_import_matches_only_whole_parts_at_the_end_of_a_path() {
    let v = scan_files(
        "whole-parts",
        &[
            ("web/src/ab/c.ts", "export function c() {\n  return 1;\n}\n"),
            ("web/src/x.ts", "import { c } from 'b/c';\n\nexport function x() {\n  return c();\n}\n"),
        ],
    );
    assert!(deps_of(&v, "web/src/x.ts").is_empty(), "{:?}", deps_of(&v, "web/src/x.ts"));
}

/// Dois projetos Python, cada um com o seu `pyproject.toml` e um
/// `utils/formato.py`; o `comum/registro.py` só existe no `api`.
const TWO_PYTHON_PROJECTS: &[(&str, &str)] = &[
    ("api/pyproject.toml", "[tool.poetry]\nname = \"api\"\n\n[tool.poetry.dependencies]\nrequests = \"^2.0\"\n"),
    ("api/utils/formato.py", "def f():\n    return 1\n"),
    ("api/comum/registro.py", "def r():\n    return 1\n"),
    ("api/main.py", "from utils.formato import f\n\n\ndef main():\n    return f()\n"),
    ("worker/pyproject.toml", "[tool.poetry]\nname = \"worker\"\n"),
    ("worker/utils/formato.py", "def f():\n    return 2\n"),
    ("worker/tarefa.py", "from comum.registro import r\n\n\ndef tarefa():\n    return r()\n"),
];

/// O `pyproject.toml` marca o projeto: o import do `api` liga ao arquivo do
/// `api`, e não aos dois de mesmo caminho.
#[test]
fn a_python_project_is_marked_by_its_manifest_and_its_import_stays_in_it() {
    let v = scan_files("python-projects", TWO_PYTHON_PROJECTS);
    assert_eq!(deps_of(&v, "api/main.py"), vec!["api/utils/formato.py".to_string()]);
}

/// Sem nada no próprio projeto, o import procura no resto do repositório: um
/// projeto pode usar outro pelo nome do módulo.
#[test]
fn an_import_with_nothing_in_its_own_project_reaches_another_project() {
    let v = scan_files("python-other-project", TWO_PYTHON_PROJECTS);
    assert_eq!(deps_of(&v, "worker/tarefa.py"), vec!["api/comum/registro.py".to_string()]);
}

/// `import { x } from '@empresa/core'`, só o nome do pacote, liga ao arquivo
/// raiz dele.
#[test]
fn an_import_of_a_package_by_its_name_alone_reaches_its_root_file() {
    let v = scan_files(
        "package-name",
        &[
            ("pacotes/core/package.json", "{\n  \"name\": \"@empresa/core\"\n}\n"),
            ("pacotes/core/src/index.ts", "export function x() {\n  return 1;\n}\n"),
            ("web/src/tela.ts", "import { x } from '@empresa/core';\n\nexport function tela() {\n  return x();\n}\n"),
        ],
    );
    assert!(
        deps_of(&v, "web/src/tela.ts").contains(&"pacotes/core/src/index.ts".to_string()),
        "{:?}",
        deps_of(&v, "web/src/tela.ts")
    );
}

/// Um crate com `src/config.rs` e `src/cmd/config.rs`: o `crate::config` de
/// `src/cmd/x.rs` parte da raiz do pacote, e o `self::config` do
/// `src/cmd/mod.rs` parte do módulo dele.
const CRATE_WITH_TWO_CONFIGS: &[(&str, &str)] = &[
    ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    ("src/main.rs", "mod cmd;\nmod config;\n\nfn main() {}\n"),
    ("src/config.rs", "pub struct Opcoes;\n"),
    (
        "src/cmd/mod.rs",
        "pub mod config;\npub mod x;\n\nuse self::config::Ajuste;\n\npub fn ajuste() -> Ajuste {\n    Ajuste\n}\n",
    ),
    ("src/cmd/config.rs", "pub struct Ajuste;\n"),
    ("src/cmd/x.rs", "use crate::config::Opcoes;\n\npub fn rodar() -> Opcoes {\n    Opcoes\n}\n"),
];

/// `crate::config` em `src/cmd/x.rs` é o `src/config.rs`, da raiz do
/// pacote, e não o `src/cmd/config.rs` achado subindo da pasta.
#[test]
fn a_path_from_the_root_alias_is_read_from_the_folder_of_the_package_root_file() {
    let v = scan_files("root-alias-root", CRATE_WITH_TWO_CONFIGS);
    assert_eq!(deps_of(&v, "src/cmd/x.rs"), vec!["src/config.rs".to_string()]);
}

/// `self::config` no `src/cmd/mod.rs` é o `src/cmd/config.rs`.
#[test]
fn a_path_from_the_own_module_alias_in_a_folder_file_reaches_its_child() {
    let v = scan_files("module-alias-folder", CRATE_WITH_TWO_CONFIGS);
    let deps = deps_of(&v, "src/cmd/mod.rs");
    assert!(deps.contains(&"src/cmd/config.rs".to_string()), "{deps:?}");
    assert!(!deps.contains(&"src/config.rs".to_string()), "{deps:?}");
}

/// `use self::b::F;` em `src/a.rs` nomeia o módulo filho `src/a/b.rs`, e não
/// o `src/b.rs` ao lado.
#[test]
fn a_path_from_the_own_module_alias_reads_inside_the_folder_of_the_file_modules() {
    let v = scan_files(
        "module-alias",
        &[
            ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ("src/main.rs", "mod a;\nmod b;\n\nfn main() {}\n"),
            ("src/a.rs", "mod b;\n\nuse self::b::F;\n\npub fn f() -> F {\n    F\n}\n"),
            ("src/a/b.rs", "pub struct F;\n"),
            ("src/b.rs", "pub struct F;\n"),
        ],
    );
    assert_eq!(deps_of(&v, "src/a.rs"), vec!["src/a/b.rs".to_string()]);
}

/// Um `go.mod` numa subpasta: o import pelo módulo dele lê a partir da pasta
/// do manifesto.
#[test]
fn a_module_declared_in_a_subfolder_resolves_from_that_folder() {
    let v = scan_files(
        "module-subfolder",
        &[
            ("servico/go.mod", "module exemplo.com/servico\n\ngo 1.21\n"),
            ("servico/interno/x/x.go", "package x\n\nfunc F() int {\n\treturn 1\n}\n"),
            (
                "servico/cmd/main.go",
                "package main\n\nimport \"exemplo.com/servico/interno/x\"\n\nfunc main() {\n\tx.F()\n}\n",
            ),
        ],
    );
    assert_eq!(deps_of(&v, "servico/cmd/main.go"), vec!["servico/interno/x/x.go".to_string()]);
}

/// Dois módulos declarados, cada um importando o outro: os dois ligam.
#[test]
fn every_declared_module_resolves_not_only_the_last_one_seen() {
    let v = scan_files(
        "two-modules",
        &[
            ("a/go.mod", "module exemplo.com/a\n\ngo 1.21\n"),
            ("a/util/u.go", "package util\n\nfunc A() int {\n\treturn 1\n}\n"),
            ("a/main.go", "package main\n\nimport \"exemplo.com/b/util\"\n\nfunc main() {\n\tutil.B()\n}\n"),
            ("b/go.mod", "module exemplo.com/b\n\ngo 1.21\n"),
            ("b/util/u.go", "package util\n\nfunc B() int {\n\treturn 2\n}\n"),
            ("b/main.go", "package main\n\nimport \"exemplo.com/a/util\"\n\nfunc main() {\n\tutil.A()\n}\n"),
        ],
    );
    assert_eq!(deps_of(&v, "a/main.go"), vec!["b/util/u.go".to_string()]);
    assert_eq!(deps_of(&v, "b/main.go"), vec!["a/util/u.go".to_string()]);
}

/// `exemplo.com/servico2/y` não abre com o módulo `exemplo.com/servico`: o
/// módulo só casa seguido de `/` ou no fim do import.
#[test]
fn a_declared_module_matches_only_when_followed_by_a_slash_or_the_end() {
    let v = scan_files(
        "module-prefix",
        &[
            ("go.mod", "module exemplo.com/servico\n\ngo 1.21\n"),
            ("2/y/y.go", "package y\n\nfunc Y() int {\n\treturn 1\n}\n"),
            ("main.go", "package main\n\nimport \"exemplo.com/servico2/y\"\n\nfunc main() {\n\ty.Y()\n}\n"),
        ],
    );
    assert!(deps_of(&v, "main.go").is_empty(), "{:?}", deps_of(&v, "main.go"));
}

/// Um pacote Python que repassa com `*` o `buscar` de um módulo dele, usado
/// pelo nome do pacote, com outro `buscar` noutro pacote.
const PACKAGE_THAT_PASSES_ON: &[(&str, &str)] = &[
    ("pyproject.toml", "[project]\nname = \"loja\"\n"),
    ("loja/__init__.py", "from .servico import *\n"),
    ("loja/servico.py", "def buscar(id):\n    return id\n"),
    ("outra/servico.py", "def buscar(id):\n    return 0\n"),
    ("loja/usa.py", "from loja import buscar\n\n\ndef usa():\n    return buscar(1)\n"),
];

/// `from loja import buscar` chega ao `loja/__init__.py`, que repassa tudo o
/// que `loja/servico.py` declara: a dependência vai ao arquivo que declara o
/// nome, e a chamada `buscar(1)` liga provada a ele, e não ao `buscar` de
/// `outra/servico.py`.
#[test]
fn an_import_by_the_package_name_reaches_the_file_its_init_passes_on() {
    let v = scan_files("python-package-name", PACKAGE_THAT_PASSES_ON);
    assert_eq!(deps_of(&v, "loja/usa.py"), vec!["loja/servico.py".to_string()]);
    assert_eq!(holders_of(&v, "buscar", "loja/usa.py:5:usa"), vec!["loja/servico.py".to_string()]);
    assert!(suspect_holders_of(&v, "buscar", "loja/usa.py:5:usa").is_empty());
}

/// `import util`, de uma parte só, liga ao `util.py` da raiz do projeto.
#[test]
fn an_import_of_one_part_reaches_the_file_of_that_name() {
    let v = scan_files(
        "python-one-part",
        &[
            ("util.py", "def ler():\n    return 1\n"),
            ("app/main.py", "import util\n\n\ndef main():\n    return util.ler()\n"),
        ],
    );
    assert_eq!(deps_of(&v, "app/main.py"), vec!["util.py".to_string()]);
}

/// `import util`, sem apelido, traz ao arquivo só o nome do módulo: a
/// chamada `util.ler()` liga ao `ler` de `util.py`, e `ler()` escrito sozinho
/// não o vê, nem como ligação provada.
#[test]
fn an_import_without_alias_brings_only_the_module_name() {
    let v = scan_files(
        "python-module-name",
        &[
            ("util.py", "def ler():\n    return 1\n"),
            ("app/main.py", "import util\n\n\ndef main():\n    util.ler()\n    return ler()\n"),
        ],
    );
    assert_eq!(holders_of(&v, "ler", "app/main.py:5:main"), vec!["util.py".to_string()]);
    assert!(holders_of(&v, "ler", "app/main.py:6:main").is_empty(), "ler() sozinho não é do módulo importado");
}

/// O `__init__.py` que traz um nome de um módulo e tudo de outro repassa os
/// dois: quem importa os dois nomes pelo pacote liga aos dois arquivos que os
/// declaram.
#[test]
fn an_init_file_passes_on_a_named_import_and_a_star_import() {
    let v = scan_files(
        "python-init-passes-on",
        &[
            ("loja/__init__.py", "from .servico import buscar\nfrom .outro import *\n"),
            ("loja/servico.py", "def buscar(id):\n    return id\n"),
            ("loja/outro.py", "def contar():\n    return 0\n"),
            ("app.py", "from loja import buscar, contar\n\n\ndef app():\n    return buscar(contar())\n"),
        ],
    );
    assert_eq!(deps_of(&v, "app.py"), vec!["loja/outro.py".to_string(), "loja/servico.py".to_string()]);
}

/// `import json`, sem `json.py` no projeto, não liga a nada, e a chamada
/// `json.dumps({})` é de fora: não liga ao `dumps` do projeto.
#[test]
fn an_import_of_one_part_that_names_nothing_of_the_project_stays_outside() {
    let v = scan_files(
        "python-one-part-outside",
        &[
            ("util/texto.py", "def dumps(x):\n    return x\n"),
            ("app.py", "import json\n\n\ndef app():\n    return json.dumps({})\n"),
        ],
    );
    assert!(deps_of(&v, "app.py").is_empty(), "{:?}", deps_of(&v, "app.py"));
    assert!(used_by_of(&v, "util/texto.py", "dumps").is_empty());
}

/// Na língua em que o import de uma parte só é pacote de fora, ele não liga
/// ao arquivo de mesmo nome: `import x from 'react'` não é `src/react.ts`.
#[test]
fn an_import_of_one_part_stays_a_package_where_the_language_says_so() {
    let v = scan_files(
        "one-part-package",
        &[
            ("src/react.ts", "export function r() {\n  return 1;\n}\n"),
            ("src/app.ts", "import x from 'react';\n\nexport function app() {\n  return x;\n}\n"),
        ],
    );
    assert!(deps_of(&v, "src/app.ts").is_empty(), "{:?}", deps_of(&v, "src/app.ts"));
}

/// O manifesto de `path` como o mapa grava: o tipo e as dependências.
fn manifest_of(v: &serde_json::Value, path: &str) -> (String, Vec<String>) {
    let m = v["manifests"]
        .as_array()
        .expect("manifests")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} é manifesto no mapa"));
    let deps = m["dependencies"].as_array().into_iter().flatten().map(|d| d.as_str().unwrap().to_string()).collect();
    (m["kind"].as_str().unwrap().to_string(), deps)
}

/// As pastas dos projetos do mapa, em ordem.
fn project_dirs(v: &serde_json::Value) -> Vec<String> {
    let mut dirs: Vec<String> =
        v["projects"].as_array().expect("projects").iter().map(|p| p["dir"].as_str().unwrap().to_string()).collect();
    dirs.sort();
    dirs
}

/// A lista de dependências de uma por linha, o arquivo de configuração sem
/// dependência e o de seções: cada um marca o seu projeto, e os de
/// dependência trazem só o nome de cada uma, sem versão nem condição.
#[test]
fn the_python_dependency_files_mark_their_projects_and_list_the_names() {
    let v = scan_files(
        "python-manifests",
        &[
            ("api/requirements.txt", "# web\nfastapi==0.110\nflask>=2 ; python_version>\"3\"\n\n-r base.txt\n"),
            ("api/main.py", "def main():\n    return 1\n"),
            ("lib/setup.cfg", "[metadata]\nname = lib\n"),
            ("lib/util.py", "def u():\n    return 1\n"),
            ("web/Pipfile", "[packages]\ndjango = \"*\"\n\n[dev-packages]\npytest = \"*\"\n"),
            ("web/app.py", "def app():\n    return 1\n"),
        ],
    );
    assert_eq!(manifest_of(&v, "api/requirements.txt").1, vec!["fastapi".to_string(), "flask".to_string()]);
    assert_eq!(manifest_of(&v, "web/Pipfile").1, vec!["django".to_string(), "pytest".to_string()]);
    assert!(manifest_of(&v, "lib/setup.cfg").1.is_empty());
    assert_eq!(project_dirs(&v), vec!["api".to_string(), "lib".to_string(), "web".to_string()]);
}

/// Dois manifestos na mesma pasta dão um projeto só.
#[test]
fn two_python_manifests_in_one_folder_make_one_project() {
    let v = scan_files(
        "python-two-manifests",
        &[
            ("svc/requirements.txt", "flask\n"),
            ("svc/pyproject.toml", "[project]\nname = \"svc\"\n"),
            ("svc/app.py", "def app():\n    return 1\n"),
        ],
    );
    assert_eq!(project_dirs(&v), vec!["svc".to_string()]);
}

/// Escreve os arquivos num projeto do git, já no primeiro commit, e devolve o
/// mapa do scan dele.
fn scan_committed(label: &str, files: &[(&str, &str)]) -> serde_json::Value {
    let temp = tempfile::Builder::new().prefix(&format!("scan-graph-git-{label}-")).tempdir().unwrap();
    let dir = temp.path();
    for (path, body) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, body).unwrap();
    }
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    scan_in_place(dir).0
}

/// Um pacote Dart que se importa pelo próprio nome: a biblioteca
/// `lib/loja.dart` repassa `lib/src/pedido_service.dart`, o teste importa o
/// arquivo pelo caminho do pacote, e a tela importa só a biblioteca.
const DART_PACKAGE: &[(&str, &str)] = &[
    ("pubspec.yaml", "name: loja\n\ndependencies:\n  http: ^1.0.0\n"),
    ("lib/loja.dart", "export 'src/pedido_service.dart';\n"),
    (
        "lib/src/pedido_service.dart",
        "class PedidoService {\n  final int limite = 10;\n  int get total => 0;\n  String buscar(int id) {\n    return '';\n  }\n}\n\nenum Estado { aberto, fechado }\n",
    ),
    (
        "test/pedido_service_test.dart",
        "import 'package:loja/src/pedido_service.dart';\n\nvoid main() {\n  PedidoService().buscar(1);\n}\n",
    ),
    (
        "lib/tela.dart",
        "import 'package:loja/loja.dart';\nimport 'package:http/http.dart' as http;\n\nvoid tela() {\n  PedidoService();\n}\n",
    ),
];

/// O import pelo nome do próprio pacote chega ao arquivo dele; a biblioteca
/// que repassa é o que a tela importa, e o que ela repassa fica à vista da
/// tela: o `PedidoService()` liga provado ao arquivo que o declara. O teste
/// fica ligado ao arquivo que ele importa, e o pacote de fora não liga a
/// nada do projeto.
#[test]
fn a_package_import_reaches_its_own_file_and_a_library_passes_on_what_it_exports() {
    let v = scan_committed("dart-package", DART_PACKAGE);
    assert_eq!(deps_of(&v, "test/pedido_service_test.dart"), vec!["lib/src/pedido_service.dart".to_string()]);
    assert_eq!(deps_of(&v, "lib/tela.dart"), vec!["lib/loja.dart".to_string()]);
    assert_eq!(holders_of(&v, "PedidoService", "lib/tela.dart:5:tela"), vec!["lib/src/pedido_service.dart".to_string()]);
    assert_eq!(list_of(&v, "lib/src/pedido_service.dart", "tests"), vec!["test/pedido_service_test.dart".to_string()]);
}

/// Um import com `*` que só escreve o caminho do arquivo (`from loja.util
/// import *`) põe à vista tudo o que o arquivo declara, também o nome que a
/// língua já tem (`open`); o mesmo nome escrito num arquivo sem esse import
/// segue sendo o da língua.
const PYTHON_STAR_IMPORT: &[(&str, &str)] = &[
    ("loja/__init__.py", ""),
    ("loja/util.py", "def open():\n    pass\n"),
    ("loja/outro.py", "def open():\n    pass\n"),
    ("loja/app.py", "from loja.util import *\n\ndef principal():\n    open()\n"),
    ("loja/solto.py", "def solto():\n    open()\n"),
];

#[test]
fn a_star_import_that_writes_only_the_module_puts_the_language_name_it_declares_in_sight() {
    let v = scan_files("py-star", PYTHON_STAR_IMPORT);
    assert_eq!(holders_of(&v, "open", "loja/app.py:4:principal"), vec!["loja/util.py".to_string()]);
    assert!(suspect_holders_of(&v, "open", "loja/app.py:4:principal").is_empty(), "nenhuma suspeita");
    assert!(holders_of(&v, "open", "loja/solto.py:2:solto").is_empty(), "sem o import, o nome é o da língua");
    assert!(suspect_holders_of(&v, "open", "loja/solto.py:2:solto").is_empty(), "sem o import, o nome é o da língua");
}

/// O `mod` marcado com `#[path = "..."]` mora no arquivo que o atributo
/// nomeia, a partir da pasta de quem o escreve, também subindo pasta: o
/// arquivo é dependência de quem escreve o `mod`, e a chamada do que ele
/// declara, sozinha ou pelo nome do módulo, liga provada, mesmo com outro
/// arquivo que declara o mesmo nome. O atributo vale em qualquer ponto da
/// fila de atributos colada ao `mod`, e o `#[cfg(test)]` também, sozinho ou
/// com outra condição (`all(test, unix)`); um item que não é atributo no meio
/// corta a fila.
const RUST_PATH_MODULE: &[(&str, &str)] = &[
    ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n"),
    (
        "src/lib.rs",
        "pub mod leitor;\npub mod terceiro;\npub mod quarto;\npub mod quinto;\npub mod sexto;\npub mod setimo;\n\
         pub mod oitavo;\npub mod nono;\n\n#[cfg(test)]\n#[path = \"../tests/support/medida.rs\"]\nmod medida;\n",
    ),
    ("src/leitor.rs", "#[path = \"../tests/support/outra.rs\"]\nmod outra;\n"),
    ("src/terceiro.rs", "#[path = \"../tests/support/outra.rs\"]\n#[cfg(test)]\nmod outra;\n"),
    ("src/quarto.rs", "#[path = \"../tests/support/outra.rs\"]\n#[allow(dead_code)]\nmod outra;\n"),
    ("src/quinto.rs", "#[path = \"../tests/support/outra.rs\"]\n#[cfg(test)]\n#[allow(dead_code)]\nmod outra;\n"),
    ("src/sexto.rs", "#[cfg(test)]\n#[allow(dead_code)]\n#[path = \"../tests/support/outra.rs\"]\nmod outra;\n"),
    ("src/setimo.rs", "#[path = \"../tests/support/outra.rs\"]\n#[allow(dead_code)]\nconst X: u8 = 0;\nmod outra;\n"),
    ("src/oitavo.rs", "#[cfg(test)]\nconst Y: u8 = 0;\n#[path = \"../tests/support/outra.rs\"]\nmod outra;\n"),
    ("src/nono.rs", "#[cfg(all(test, unix))]\n#[path = \"../tests/support/outra.rs\"]\nmod outra;\n"),
    ("tests/support/medida.rs", "pub fn prose_budget() {}\n"),
    ("tests/support/outra.rs", "pub fn prose_budget() {}\n"),
    (
        "tests/orcamento.rs",
        "#[path = \"support/medida.rs\"]\nmod medida;\n\nuse medida::prose_budget;\n\n#[test]\nfn mede() {\n    prose_budget();\n    medida::prose_budget();\n}\n",
    ),
];

#[test]
fn a_module_with_a_path_attribute_reaches_the_file_it_names() {
    let v = scan_files("rs-path-attr", RUST_PATH_MODULE);
    assert_eq!(deps_of(&v, "tests/orcamento.rs"), vec!["tests/support/medida.rs".to_string()]);
    assert_eq!(deps_of(&v, "src/leitor.rs"), vec!["tests/support/outra.rs".to_string()]);
    // Com `#[cfg(test)]` antes ou depois do `path`, o arquivo é import do
    // teste escrito ali, e não do arquivo.
    assert!(deps_of(&v, "src/lib.rs").is_empty(), "{:?}", deps_of(&v, "src/lib.rs"));
    assert_eq!(list_of(&v, "src/lib.rs", "test_deps"), vec!["tests/support/medida.rs".to_string()]);
    assert!(deps_of(&v, "src/terceiro.rs").is_empty(), "{:?}", deps_of(&v, "src/terceiro.rs"));
    assert_eq!(list_of(&v, "src/terceiro.rs", "test_deps"), vec!["tests/support/outra.rs".to_string()]);
    // Outro atributo no meio da fila não corta o `path`, nem o `#[cfg(test)]`
    // escrito antes ou depois dele. Um item que não é atributo corta: o
    // `path` dele não chega ao `mod`, e o `#[cfg(test)]` dele não é do `mod`.
    // Os seis lado a lado, para que um desvio mostre todos de uma vez.
    let other = || vec!["tests/support/outra.rs".to_string()];
    let none = Vec::<String>::new;
    let queues: Vec<_> = ["src/quarto.rs", "src/quinto.rs", "src/sexto.rs", "src/setimo.rs", "src/oitavo.rs", "src/nono.rs"]
        .into_iter()
        .map(|file| (file, deps_of(&v, file), list_of(&v, file, "test_deps")))
        .collect();
    assert_eq!(
        queues,
        vec![
            ("src/quarto.rs", other(), none()),
            ("src/quinto.rs", none(), other()),
            ("src/sexto.rs", none(), other()),
            ("src/setimo.rs", none(), none()),
            ("src/oitavo.rs", other(), none()),
            ("src/nono.rs", none(), other()),
        ]
    );
    for site in ["tests/orcamento.rs:8:mede", "tests/orcamento.rs:9:mede"] {
        assert_eq!(holders_of(&v, "prose_budget", site), vec!["tests/support/medida.rs".to_string()], "{site}");
        assert!(suspect_holders_of(&v, "prose_budget", site).is_empty(), "{site}: nenhuma suspeita");
    }
}

/// O `use` que junta caminhos num grupo (`a::{self, b::c}`), também com um
/// grupo dentro de outro e a partir do `super`, chega ao arquivo de cada
/// ramo: cada nome trazido liga, provado, ao arquivo que o ramo nomeia, mesmo
/// com outro arquivo que declara o mesmo nome.
const RUST_GROUPED_USE: &[(&str, &str)] = &[
    ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n"),
    ("src/lib.rs", "pub mod eventos;\npub mod fluxo;\npub mod outro;\n"),
    ("src/eventos/mod.rs", "pub mod abrir;\npub mod read;\npub mod write;\n"),
    ("src/eventos/write.rs", "pub fn record_open() {}\npub fn record() {}\n"),
    ("src/eventos/read.rs", "pub fn checkout() {}\n"),
    ("src/outro.rs", "pub fn record_open() {}\npub fn record() {}\npub fn checkout() {}\n"),
    (
        "src/fluxo.rs",
        "use crate::eventos::{self, read::checkout, write::record_open};\n\npub fn abre() {\n    record_open();\n    checkout();\n}\n",
    ),
    (
        "src/eventos/abrir.rs",
        "use super::{\n    self,\n    write::{record, record_open},\n};\n\npub fn fecha() {\n    record();\n    record_open();\n}\n",
    ),
];

#[test]
fn a_grouped_use_reaches_the_file_of_each_branch() {
    let v = scan_files("rs-grouped-use", RUST_GROUPED_USE);
    let paths = |list: &[&str]| list.iter().map(|p| (*p).to_string()).collect::<Vec<_>>();
    assert_eq!(deps_of(&v, "src/fluxo.rs"), paths(&["src/eventos/mod.rs", "src/eventos/read.rs", "src/eventos/write.rs"]));
    assert_eq!(deps_of(&v, "src/eventos/abrir.rs"), paths(&["src/eventos/mod.rs", "src/eventos/write.rs"]));
    for (name, site, file) in [
        ("record_open", "src/fluxo.rs:4:abre", "src/eventos/write.rs"),
        ("checkout", "src/fluxo.rs:5:abre", "src/eventos/read.rs"),
        ("record", "src/eventos/abrir.rs:7:fecha", "src/eventos/write.rs"),
        ("record_open", "src/eventos/abrir.rs:8:fecha", "src/eventos/write.rs"),
    ] {
        assert_eq!(holders_of(&v, name, site), vec![file.to_string()], "{site}");
        assert!(suspect_holders_of(&v, name, site).is_empty(), "{site}: nenhuma suspeita");
    }
}

/// Um módulo Go: o teste mora no mesmo pacote do serviço e chama a função sem
/// import do projeto; outro pacote importa o do serviço pelo caminho do
/// módulo.
const GO_SAME_PACKAGE_TEST: &[(&str, &str)] = &[
    ("go.mod", "module example.com/loja\n\ngo 1.22\n"),
    ("pedidos/servico.go", "package pedidos\n\nfunc Buscar(id int) int {\n\treturn id\n}\n"),
    (
        "pedidos/servico_test.go",
        "package pedidos\n\nimport \"testing\"\n\nfunc TestBuscar(t *testing.T) {\n\tBuscar(1)\n}\n",
    ),
    (
        "api/rotas.go",
        "package api\n\nimport \"example.com/loja/pedidos\"\n\nfunc Rotas() int {\n\treturn pedidos.Buscar(2)\n}\n",
    ),
];

/// O que o mapa sabe do teste do pacote e de quem importa o pacote.
fn assert_the_same_package_test_covers_the_service(v: &serde_json::Value) {
    assert_eq!(holders_of(v, "Buscar", "pedidos/servico_test.go:6:TestBuscar"), vec!["pedidos/servico.go".to_string()]);
    assert_eq!(list_of(v, "pedidos/servico.go", "tests"), vec!["pedidos/servico_test.go".to_string()]);
    assert_eq!(deps_of(v, "api/rotas.go"), vec!["pedidos/servico.go".to_string()], "o código não importa o teste");
}

/// O teste liga ao arquivo que declara o que ele chama por ligação provada,
/// sem importá-lo; e o import do pacote inteiro, escrito no código, não traz
/// o arquivo de teste. A passada que relê só o que mudou chega ao mesmo.
#[test]
fn a_test_covers_the_file_it_calls_without_importing_it_and_code_does_not_import_the_test() {
    let temp = tempfile::Builder::new().prefix("scan-graph-go-same-package-").tempdir().unwrap();
    let dir = temp.path();
    for (path, body) in GO_SAME_PACKAGE_TEST {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, body).unwrap();
    }
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let (full, report) = scan_in_place(dir);
    assert_eq!(report["full"], serde_json::Value::Bool(true), "{report}");
    assert_the_same_package_test_covers_the_service(&full);

    let routes_text = std::fs::read_to_string(dir.join("api/rotas.go")).unwrap();
    std::fs::write(dir.join("api/rotas.go"), format!("{routes_text}\nfunc Outra() {{}}\n")).unwrap();
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (kept, report) = scan_in_place(dir);
    assert_eq!(report["read"], serde_json::json!(["api/rotas.go"]), "só o que mudou é relido: {report}");
    assert_the_same_package_test_covers_the_service(&kept);
}

/// O teste cuja chamada só tem ligação suspeita (duas funções `Buscar` no
/// pacote dele, as duas à vista) não cobre nenhuma das duas pelo uso.
#[test]
fn a_test_whose_call_is_only_suspect_covers_no_file_by_the_use() {
    let mut files = GO_SAME_PACKAGE_TEST.to_vec();
    files.push(("pedidos/outro.go", "package pedidos\n\nfunc Buscar(id int) int {\n\treturn 0\n}\n"));
    let v = scan_committed("go-suspect-use", &files);
    let suspects = suspect_holders_of(&v, "Buscar", "pedidos/servico_test.go:6:TestBuscar");
    assert_eq!(suspects.len(), 2, "{suspects:?}");
    assert!(list_of(&v, "pedidos/servico.go", "tests").is_empty());
    assert!(list_of(&v, "pedidos/outro.go", "tests").is_empty());
}

/// O teste que importa o pacote inteiro segue vendo os arquivos de teste dele.
#[test]
fn a_test_that_imports_the_package_still_sees_its_test_files() {
    let mut files = GO_SAME_PACKAGE_TEST.to_vec();
    files.push((
        "api/rotas_test.go",
        "package api\n\nimport (\n\t\"testing\"\n\n\t\"example.com/loja/pedidos\"\n)\n\nfunc TestRotas(t *testing.T) {\n\tpedidos.Buscar(3)\n}\n",
    ));
    let v = scan_committed("go-test-imports-package", &files);
    assert_eq!(
        deps_of(&v, "api/rotas_test.go"),
        vec!["pedidos/servico.go".to_string(), "pedidos/servico_test.go".to_string()]
    );
}

/// Quem usa a declaração `name` do arquivo `path` com ligação provada: as
/// entradas escritas só com o lugar, sem candidatos.
fn proven_uses_of(v: &serde_json::Value, path: &str, name: &str) -> Vec<String> {
    let module = v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} está no mapa"));
    let decl = module["declarations"]
        .as_array()
        .expect("declarations")
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("{name} está em {path}"));
    decl["used_by"].as_array().into_iter().flatten().filter_map(|u| u.as_str().map(str::to_string)).collect()
}

/// Os lugares de quem usa a primeira declaração `name` do arquivo `path`,
/// provados ou suspeitos, sem repetição.
fn use_places_of(v: &serde_json::Value, path: &str, name: &str) -> Vec<String> {
    let module = v["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == path)
        .unwrap_or_else(|| panic!("{path} está no mapa"));
    let decl = module["declarations"]
        .as_array()
        .expect("declarations")
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("{name} está em {path}"));
    let places: std::collections::BTreeSet<String> = decl["used_by"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|u| u.as_str().or_else(|| u["at"].as_str()).map(str::to_string))
        .collect();
    places.into_iter().collect()
}

/// Uma pasta cujo `index.ts` repassa um nome e um arquivo inteiro com nome, e
/// três telas que a importam: uma pelo nome repassado com apelido, uma pelo
/// nome do arquivo inteiro e uma por um nome que só o arquivo inteiro
/// declara.
const TS_RENAMED_IMPORTS: &[(&str, &str)] = &[
    ("src/pasta/index.ts", "export { Leitor } from './leitor';\nexport * as util from './util';\n"),
    ("src/pasta/leitor.ts", "export function Leitor() {\n  return 1;\n}\n"),
    ("src/pasta/util.ts", "export function soma(a: number) {\n  return a;\n}\n"),
    ("src/app.ts", "import { Leitor as L } from './pasta';\n\nexport function tela() {\n  return L();\n}\n"),
    ("src/app2.ts", "import { util } from './pasta';\n\nexport function tela2() {\n  return util.soma(1);\n}\n"),
    ("src/app3.ts", "import { soma } from './pasta';\n\nexport function tela3() {\n  return soma(1);\n}\n"),
];

/// `import { Leitor as L }` pede à pasta o nome de origem: a dependência é o
/// arquivo que declara o `Leitor`, e `L()` é uso dele, provado.
fn assert_an_import_under_another_name_reaches_the_declaration_by_its_original_name(v: &serde_json::Value) {
    assert_eq!(deps_of(v, "src/app.ts"), vec!["src/pasta/leitor.ts".to_string()]);
    assert_eq!(proven_uses_of(v, "src/pasta/leitor.ts", "Leitor"), vec!["src/app.ts:4:tela".to_string()]);
}

#[test]
fn an_import_under_another_name_reaches_the_declaration_by_its_original_name() {
    let v = scan_files("ts-renamed-import", TS_RENAMED_IMPORTS);
    assert_an_import_under_another_name_reaches_the_declaration_by_its_original_name(&v);
}

/// `export * as util from './util'` oferece `util` como o arquivo inteiro:
/// quem traz `util` depende do `util.ts`, e `util.soma()` é uso provado da
/// `soma` dele. O nome que só o `util.ts` declara não é oferecido pela pasta
/// e fica no `index.ts`.
#[test]
fn a_whole_file_passed_on_under_a_name_is_that_file() {
    let v = scan_files("ts-namespace-export", TS_RENAMED_IMPORTS);
    assert_eq!(deps_of(&v, "src/app2.ts"), vec!["src/pasta/util.ts".to_string()]);
    assert_eq!(proven_uses_of(&v, "src/pasta/util.ts", "soma"), vec!["src/app2.ts:4:tela2".to_string()]);
    assert_eq!(deps_of(&v, "src/app3.ts"), vec!["src/pasta/index.ts".to_string()]);
}

/// A passada que relê só o arquivo que mudou liga o nome trazido com apelido
/// como a leitura inteira: o nome de origem fica com o módulo guardado.
#[test]
fn a_pass_that_keeps_the_file_links_a_name_under_another_name_the_same() {
    let fill = |dir: &std::path::Path| {
        for (path, body) in TS_RENAMED_IMPORTS {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, body).unwrap();
        }
    };
    let (first, second) =
        full_and_kept_pass_of("ts-renamed-import", fill, ("src/app3.ts", "\nexport function outra() {}\n"));
    for v in [&first, &second] {
        assert_an_import_under_another_name_reaches_the_declaration_by_its_original_name(v);
    }
}

/// `use crate::io::{Leitor as L}` e `use crate::io::leitor::Leitor as M`:
/// cada um pede o `Leitor` ao alvo, e o apelido escrito antes do método liga
/// ao método do tipo de origem.
#[test]
fn a_type_brought_under_another_name_links_its_methods_through_the_original_name() {
    let v = scan_files(
        "rust-renamed-use",
        &[
            ("Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ("src/main.rs", "mod app;\nmod io;\nmod outro;\n\nfn main() {}\n"),
            ("src/io/mod.rs", "pub mod leitor;\npub use leitor::Leitor;\n"),
            ("src/io/leitor.rs", "pub struct Leitor;\n\nimpl Leitor {\n    pub fn novo() -> Self {\n        Leitor\n    }\n}\n"),
            ("src/app.rs", "use crate::io::{Leitor as L};\n\npub fn abrir() {\n    let _ = L::novo();\n}\n"),
            ("src/outro.rs", "use crate::io::leitor::Leitor as M;\n\npub fn outro() {\n    let _ = M::novo();\n}\n"),
        ],
    );
    assert_eq!(deps_of(&v, "src/app.rs"), vec!["src/io/leitor.rs".to_string()]);
    assert_eq!(
        proven_uses_of(&v, "src/io/leitor.rs", "novo"),
        vec!["src/app.rs:4:abrir".to_string(), "src/outro.rs:4:outro".to_string()]
    );
}

/// `from loja.servico import buscar as b`: o import guarda `b` com a origem
/// `buscar`, e `b(1)` é uso da função.
#[test]
fn a_function_imported_under_another_name_is_used_by_the_new_name() {
    let v = scan_files(
        "py-renamed-import",
        &[
            ("loja/servico.py", "def buscar(id):\n    return id\n"),
            ("usa.py", "from loja.servico import buscar as b\n\n\ndef usar():\n    return b(1)\n"),
        ],
    );
    let usa = v["modules"].as_array().unwrap().iter().find(|m| m["path"] == "usa.py").expect("usa.py");
    assert_eq!(usa["brought"], serde_json::json!({"loja.servico": {"b": "buscar"}}));
    assert_eq!(proven_uses_of(&v, "loja/servico.py", "buscar"), vec!["usa.py:5:usar".to_string()]);
}

/// `from . import models as m` segue nomeando o `models.py` da pasta.
#[test]
fn a_file_of_the_folder_imported_under_another_name_is_still_that_file() {
    let v = scan_files(
        "py-relative-alias",
        &[
            ("loja/models.py", "def criar():\n    return 1\n"),
            ("loja/usa.py", "from . import models as m\n\n\ndef usar():\n    return m.criar()\n"),
        ],
    );
    assert_eq!(deps_of(&v, "loja/usa.py"), vec!["loja/models.py".to_string()]);
}

/// `const { ler: lerPedido } = require('./pasta/servico')`: `lerPedido()` é
/// uso do `ler` do serviço.
#[test]
fn a_name_taken_from_a_require_under_another_name_is_used_by_the_new_name() {
    let v = scan_files(
        "js-renamed-require",
        &[
            ("pasta/servico.js", "function ler() {\n  return 1;\n}\n\nmodule.exports = { ler };\n"),
            ("app.js", "const { ler: lerPedido } = require('./pasta/servico');\n\nfunction abrir() {\n  return lerPedido();\n}\n"),
        ],
    );
    assert_eq!(proven_uses_of(&v, "pasta/servico.js", "ler"), vec!["app.js:4:abrir".to_string()]);
}

/// A classe com construtor primário escrita depois de um atributo: os tipos
/// do cabeçalho são usados pela classe, e não pelo parâmetro escrito na mesma
/// linha. O campo com valor numa linha do corpo segue dono dos usos dela.
#[test]
fn the_types_of_a_class_header_are_used_by_the_class() {
    let v = scan_files("cs-class-header", CLASS_HEADER);
    let header = vec!["Servico.cs:3:Servico".to_string()];
    assert_eq!(proven_uses_of(&v, "Base.cs", "IServico"), header);
    assert_eq!(proven_uses_of(&v, "Base.cs", "ILog"), header);
    // O lugar do uso da base escrita no cabeçalho; a ligação provada tem
    // teste próprio.
    assert_eq!(use_places_of(&v, "Base.cs", "BaseServico"), header);
    assert!(proven_uses_of(&v, "Base.cs", "IRepo").contains(&header[0]), "{v}");
    assert_eq!(proven_uses_of(&v, "Base.cs", "Pedido"), vec!["Servico.cs:5:_itens".to_string()]);
}

/// A classe com construtor primário e base escritos no cabeçalho, e a base
/// com o construtor dela escrito.
const CLASS_HEADER: &[(&str, &str)] = &[
    (
        "Base.cs",
        "namespace Loja;\n\npublic interface IRepo { }\npublic interface ILog { }\npublic interface IServico { }\npublic class Pedido { }\n\npublic class BaseServico\n{\n    public BaseServico(IRepo repo) { }\n}\n",
    ),
    (
        "Servico.cs",
        "namespace Loja;\n[Obsolete]\npublic class Servico(IRepo repo, ILog log) : BaseServico(repo), IServico\n{\n    private readonly List<Pedido> _itens = new();\n}\n",
    ),
];

/// A base chamada pelo nome no cabeçalho é a classe, e não o construtor dela,
/// que tem o mesmo nome: o uso liga provado à classe, sem candidatas.
#[test]
fn the_base_called_in_a_class_header_is_the_class_and_not_its_constructor() {
    let v = scan_files("cs-header-base", CLASS_HEADER);
    assert_eq!(proven_uses_of(&v, "Base.cs", "BaseServico"), vec!["Servico.cs:3:Servico".to_string()]);
    assert!(suspect_holders_of(&v, "BaseServico", "Servico.cs:3:Servico").is_empty(), "{v}");
}

/// `new Pedido(1)` cria a classe: o construtor escrito dela tem o mesmo nome,
/// mas quem chama o nome chama a classe, e o uso liga provado a ela.
#[test]
fn a_class_created_by_name_is_the_class_and_not_its_constructor() {
    let v = scan_files(
        "cs-new-class",
        &[
            ("Pedido.cs", "namespace Vendas;\n\npublic class Pedido\n{\n    public Pedido(int id) { }\n}\n"),
            (
                "Loja.cs",
                "namespace Vendas;\n\npublic class Loja\n{\n    public void Abrir()\n    {\n        var p = new Pedido(1);\n    }\n}\n",
            ),
        ],
    );
    assert_eq!(proven_uses_of(&v, "Pedido.cs", "Pedido"), vec!["Loja.cs:7:Abrir".to_string()]);
    assert!(suspect_holders_of(&v, "Pedido", "Loja.cs:7:Abrir").is_empty(), "{v}");
}

/// O mesmo no Dart: `Pedido(1)` é a classe, e não o construtor sem nome dela.
#[test]
fn a_dart_class_called_by_name_is_the_class_and_not_its_constructor() {
    let v = scan_files(
        "dart-new-class",
        &[
            ("lib/pedido.dart", "class Pedido {\n  Pedido(this.id);\n  final int id;\n}\n"),
            ("lib/loja.dart", "import 'pedido.dart';\n\nvoid abrir() {\n  final p = Pedido(1);\n}\n"),
        ],
    );
    assert_eq!(proven_uses_of(&v, "lib/pedido.dart", "Pedido"), vec!["lib/loja.dart:4:abrir".to_string()]);
    assert!(suspect_holders_of(&v, "Pedido", "lib/loja.dart:4:abrir").is_empty(), "{v}");
}

/// A classe sem construtor escrito segue provada quando é criada pelo nome.
#[test]
fn a_class_without_a_written_constructor_created_by_name_stays_proven() {
    let v = scan_files(
        "cs-new-plain-class",
        &[
            ("Pedido.cs", "namespace Vendas;\n\npublic class Pedido\n{\n}\n"),
            (
                "Loja.cs",
                "namespace Vendas;\n\npublic class Loja\n{\n    public void Abrir()\n    {\n        var p = new Pedido();\n    }\n}\n",
            ),
        ],
    );
    assert_eq!(proven_uses_of(&v, "Pedido.cs", "Pedido"), vec!["Loja.cs:7:Abrir".to_string()]);
}

/// O construtor nomeado do Dart (`Caixa.vazia()`) tem o nome dele, e não o
/// da classe: segue ligado a quem o chama.
#[test]
fn a_dart_named_constructor_stays_linked_by_its_own_name() {
    let v = scan_files(
        "dart-named-constructor",
        &[
            ("lib/caixa.dart", "class Caixa {\n  Caixa.vazia();\n}\n"),
            ("lib/loja.dart", "import 'caixa.dart';\n\nvoid abrir() {\n  final c = Caixa.vazia();\n}\n"),
        ],
    );
    assert_eq!(proven_uses_of(&v, "lib/caixa.dart", "vazia"), vec!["lib/loja.dart:4:abrir".to_string()]);
}

/// O método de outro tipo com o nome da classe não é construtor dela: segue
/// entre as candidatas de quem chama o nome.
#[test]
fn a_method_of_another_type_named_as_the_class_stays_a_candidate() {
    let v = scan_files(
        "cs-method-named-as-class",
        &[
            ("Pedido.cs", "namespace Vendas;\n\npublic class Pedido\n{\n    public Pedido(int id) { }\n}\n"),
            ("Fabrica.cs", "namespace Vendas;\n\npublic class Fabrica\n{\n    public int Pedido(int id) { return id; }\n}\n"),
            (
                "Loja.cs",
                "namespace Vendas;\n\npublic class Loja\n{\n    public void Abrir()\n    {\n        var p = new Pedido(1);\n    }\n}\n",
            ),
        ],
    );
    let holders: Vec<String> =
        suspect_holders_of(&v, "Pedido", "Loja.cs:7:Abrir").into_iter().map(|(file, _)| file).collect();
    assert!(holders.contains(&"Fabrica.cs".to_string()), "{v}");
}

/// No TypeScript o construtor se chama `constructor`: `new Pedido()` liga à
/// classe como sempre.
#[test]
fn a_typescript_class_created_by_name_stays_linked_to_the_class() {
    let v = scan_files(
        "ts-new-class",
        &[
            ("src/pedido.ts", "export class Pedido {\n  constructor() {}\n}\n"),
            ("src/loja.ts", "import { Pedido } from './pedido';\n\nexport function abrir() {\n  return new Pedido();\n}\n"),
        ],
    );
    assert_eq!(proven_uses_of(&v, "src/pedido.ts", "Pedido"), vec!["src/loja.ts:4:abrir".to_string()]);
    assert!(use_places_of(&v, "src/pedido.ts", "constructor").is_empty(), "{v}");
}

/// `use crate::io::{eventos as store}` traz um módulo com apelido: `store::ler()`
/// é o `ler` desse módulo, e não o de outra pasta que tem o nome do módulo.
#[test]
fn a_module_brought_under_another_name_names_only_that_module() {
    let v = scan_files(
        "rust-renamed-module",
        &[
            ("Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ("src/main.rs", "mod app;\nmod eventos;\nmod io;\n\nfn main() {}\n"),
            ("src/io/mod.rs", "pub mod eventos;\n"),
            ("src/io/eventos.rs", "pub fn ler() -> u8 {\n    1\n}\n"),
            ("src/eventos/mod.rs", "pub mod paginas;\n"),
            ("src/eventos/paginas.rs", "pub fn ler() -> u8 {\n    2\n}\n"),
            ("src/app.rs", "use crate::io::{eventos as store};\n\npub fn abrir() -> u8 {\n    store::ler()\n}\n"),
        ],
    );
    assert_eq!(proven_uses_of(&v, "src/io/eventos.rs", "ler"), vec!["src/app.rs:4:abrir".to_string()]);
    assert!(use_places_of(&v, "src/eventos/paginas.rs", "ler").is_empty(), "{v}");
}

/// `use crate::dominio::indice as idx;` traz o módulo com apelido fora de
/// grupo: `idx::projetar()` é o `projetar` desse módulo, e o de outro módulo
/// com o mesmo nome não recebe o uso, nem quando o arquivo que chama tem o
/// mesmo nome do módulo trazido.
#[test]
fn a_module_brought_alone_under_another_name_names_only_that_module() {
    let v = scan_files(
        "rust-module-alias",
        &[
            ("Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ("src/main.rs", "mod dominio;\nmod io;\nmod outro;\n\nfn main() {}\n"),
            ("src/dominio/mod.rs", "pub mod indice;\n"),
            ("src/dominio/indice.rs", "pub fn projetar(texto: &str) -> usize {\n    texto.len()\n}\n"),
            ("src/outro.rs", "pub fn projetar(texto: &str) -> usize {\n    texto.len() + 1\n}\n"),
            ("src/io/mod.rs", "pub mod indice;\n"),
            (
                "src/io/indice.rs",
                "use crate::dominio::indice as idx;\n\npub fn ler(texto: &str) -> usize {\n    idx::projetar(texto)\n}\n",
            ),
        ],
    );
    assert_eq!(deps_of(&v, "src/io/indice.rs"), vec!["src/dominio/indice.rs".to_string()]);
    assert_eq!(proven_uses_of(&v, "src/dominio/indice.rs", "projetar"), vec!["src/io/indice.rs:4:ler".to_string()]);
    assert!(use_places_of(&v, "src/outro.rs", "projetar").is_empty(), "{v}");
}

/// `use crate::traco::Traco as _` põe o traço à vista sem trazer nome: o `_`
/// escrito depois (`Vec<_>`) não é uso dele.
#[test]
fn a_blank_name_brought_by_an_import_is_not_a_use() {
    let v = scan_files(
        "rust-blank-alias",
        &[
            ("Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ("src/main.rs", "mod app;\nmod traco;\n\nfn main() {}\n"),
            ("src/traco.rs", "pub trait Traco {\n    fn ler(&self) -> u8;\n}\n"),
            (
                "src/app.rs",
                "use crate::traco::Traco as _;\n\npub fn lista() -> Vec<u8> {\n    let v: Vec<_> = Vec::new();\n    v\n}\n",
            ),
        ],
    );
    assert!(use_places_of(&v, "src/traco.rs", "Traco").is_empty(), "{v}");
}

/// O `Cargo.toml` de um crate solto de nome `name`.
fn cargo_of(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")
}

/// Um crate `demo-core` com `traduzir` declarada em `texto.rs` e repassada
/// pelo `lib.rs`, e um `app` que chama `demo_core::traduzir(` com o caminho do
/// pacote, sem nenhum `use`, ao lado de um `traduzir` homônimo dele.
fn package_call(app_main: &'static str) -> Vec<(&'static str, String)> {
    vec![
        ("Cargo.toml", "[workspace]\nmembers = [\"core\", \"app\"]\n".to_string()),
        ("core/Cargo.toml", cargo_of("demo-core")),
        ("core/src/lib.rs", "pub mod texto;\npub use texto::traduzir;\n".to_string()),
        ("core/src/texto.rs", "pub fn traduzir(chave: &str) -> String {\n    chave.to_string()\n}\n".to_string()),
        (
            "app/Cargo.toml",
            format!("{}\n[dependencies]\ndemo-core = {{ path = \"../core\" }}\n", cargo_of("demo-app")),
        ),
        ("app/src/main.rs", app_main.to_string()),
        (
            "app/src/local.rs",
            "pub fn traduzir(chave: &str) -> String {\n    chave.to_uppercase()\n}\n".to_string(),
        ),
    ]
}

/// A chamada escrita com o nome do pacote antes do nome (`demo_core::traduzir(`)
/// liga ao arquivo que declara o nome que o `lib.rs` repassa, e não ao
/// `traduzir` homônimo do próprio crate da chamada.
#[test]
fn a_call_by_the_package_name_links_to_the_file_the_package_passes_the_name_on_from() {
    let files = package_call(
        "mod local;\n\nfn main() {\n    println!(\"{}\", demo_core::traduzir(\"a\"));\n}\n",
    );
    let v = scan_owned("call-package-reexport", &files);
    assert_eq!(proven_uses_of(&v, "core/src/texto.rs", "traduzir"), vec!["app/src/main.rs:4:main".to_string()]);
    assert!(use_places_of(&v, "app/src/local.rs", "traduzir").is_empty(), "{v}");
}

/// O nome do pacote nomeia o arquivo raiz dele e o que ele repassa: o
/// `formatar` de um módulo que o `lib.rs` não repassa não é alcançado por
/// `demo_core::formatar(`, e o `formatar` homônimo do crate da chamada
/// também não.
#[test]
fn a_call_by_the_package_name_of_a_name_the_root_does_not_pass_on_links_to_nothing() {
    let mut files = package_call("mod formato;\nmod local;\n\nfn main() {\n    demo_core::formatar(\"a\");\n}\n");
    files.push(("core/src/formato.rs", "pub fn formatar(texto: &str) -> String {\n    texto.to_string()\n}\n".to_string()));
    files.push(("app/src/formato.rs", "pub fn formatar(texto: &str) -> String {\n    texto.to_string()\n}\n".to_string()));
    let v = scan_owned("call-package-not-passed-on", &files);
    assert!(use_places_of(&v, "core/src/formato.rs", "formatar").is_empty(), "{v}");
    assert!(use_places_of(&v, "app/src/formato.rs", "formatar").is_empty(), "{v}");
}

/// Dois tipos com o mesmo método e um deles no parâmetro: `pedido.cobrar()`
/// com `pedido: &Pedido` é o `cobrar` do `Pedido`, provado, e o `cobrar` da
/// `Nota` não é tocado.
#[test]
fn a_call_on_a_parameter_with_a_written_type_links_to_the_method_of_that_type() {
    let v = scan_files(
        "call-typed-parameter",
        &[
            ("Cargo.toml", &cargo_of("loja")),
            ("src/main.rs", "mod app;\nmod nota;\nmod pedido;\n\nfn main() {}\n"),
            (
                "src/pedido.rs",
                "pub struct Pedido {\n    pub total: u32,\n}\n\nimpl Pedido {\n    pub fn cobrar(&self) -> u32 {\n        self.total\n    }\n}\n",
            ),
            (
                "src/nota.rs",
                "pub struct Nota;\n\nimpl Nota {\n    pub fn cobrar(&self) -> u32 {\n        0\n    }\n}\n",
            ),
            (
                "src/app.rs",
                "use crate::nota::Nota;\nuse crate::pedido::Pedido;\n\npub fn fechar(pedido: &Pedido, _nota: &Nota) -> u32 {\n    pedido.cobrar()\n}\n",
            ),
        ],
    );
    assert_eq!(proven_uses_of(&v, "src/pedido.rs", "cobrar"), vec!["src/app.rs:5:fechar".to_string()]);
    assert!(use_places_of(&v, "src/nota.rs", "cobrar").is_empty(), "{v}");
}

/// A cadeia de campos depois do parâmetro (`ctx.config.language()`) usa o
/// tipo escrito de cada campo, lido do mapa: `Ctx.config` é `Config`, e a
/// chamada é o `language` de `Config`, não o de `Outra`.
#[test]
fn a_call_on_a_field_of_a_typed_parameter_links_to_the_method_of_the_field_type() {
    let v = scan_files(
        "call-typed-field",
        &[
            ("Cargo.toml", &cargo_of("loja")),
            ("src/main.rs", "mod app;\nmod config;\nmod contexto;\nmod outra;\n\nfn main() {}\n"),
            (
                "src/config.rs",
                "pub struct Config {\n    pub lingua: u8,\n}\n\nimpl Config {\n    pub fn language(&self) -> u8 {\n        self.lingua\n    }\n}\n",
            ),
            (
                "src/outra.rs",
                "pub struct Outra;\n\nimpl Outra {\n    pub fn language(&self) -> u8 {\n        0\n    }\n}\n",
            ),
            ("src/contexto.rs", "use crate::config::Config;\n\npub struct Ctx {\n    pub config: Config,\n}\n"),
            (
                "src/app.rs",
                "use crate::contexto::Ctx;\n\npub fn ler(ctx: &Ctx) -> u8 {\n    ctx.config.language()\n}\n",
            ),
        ],
    );
    assert_eq!(proven_uses_of(&v, "src/config.rs", "language"), vec!["src/app.rs:4:ler".to_string()]);
    assert!(use_places_of(&v, "src/outra.rs", "language").is_empty(), "{v}");
}

/// Receptor de tipo que o projeto não tem (`Vec<u8>`) não vira ligação a um
/// método do projeto de mesmo nome: `itens.len()` num arquivo que não vê o
/// `len` da `Fila` não a toca, e o tipo de dois nomes iguais no projeto
/// também não decide.
#[test]
fn a_call_on_a_receiver_whose_type_is_not_the_project_does_not_link_by_the_name() {
    let v = scan_files(
        "call-untyped-receiver",
        &[
            ("Cargo.toml", &cargo_of("loja")),
            ("src/main.rs", "mod app;\nmod fila;\n\nfn main() {}\n"),
            (
                "src/fila.rs",
                "pub struct Fila {\n    pub itens: Vec<u8>,\n}\n\nimpl Fila {\n    pub fn len(&self) -> usize {\n        0\n    }\n}\n",
            ),
            ("src/app.rs", "pub fn contar(itens: &Vec<u8>) -> usize {\n    itens.len()\n}\n"),
        ],
    );
    assert!(use_places_of(&v, "src/fila.rs", "len").is_empty(), "{v}");
}

/// Dois tipos do mesmo namespace com o método `Salvar`, e um serviço que o
/// chama pelo campo `_repo`, pelo parâmetro `repo` e pela classe estática
/// `Util`; o segundo tipo (`ILog`) só existe para o nome não ser único.
const CSHARP_TYPED_RECEIVERS: &[(&str, &str)] = &[
    ("src/IRepo.cs", "namespace Loja;\n\npublic interface IRepo\n{\n    void Salvar(int id);\n}\n"),
    ("src/ILog.cs", "namespace Loja;\n\npublic interface ILog\n{\n    void Salvar(int id);\n}\n"),
    ("src/Util.cs", "namespace Loja;\n\npublic static class Util\n{\n    public static void Fazer()\n    {\n    }\n}\n"),
    (
        "src/Servico.cs",
        "namespace Loja;\n\npublic class Servico\n{\n    private readonly IRepo _repo;\n\n    public void Gravar()\n    {\n        _repo.Salvar(1);\n        Util.Fazer();\n    }\n\n    public void Outro(IRepo repo)\n    {\n        repo.Salvar(2);\n    }\n}\n",
    ),
];

/// O campo do tipo em volta, escrito pelo nome sozinho (`_repo.Salvar(1)`),
/// liga ao método do tipo do campo, provado, e não ao `Salvar` do `ILog`.
#[test]
fn a_call_on_a_field_written_alone_links_to_the_method_of_the_field_type() {
    let v = scan_files("csharp-field-receiver", CSHARP_TYPED_RECEIVERS);
    assert!(
        proven_uses_of(&v, "src/IRepo.cs", "Salvar").contains(&"src/Servico.cs:9:Gravar".to_string()),
        "{v}"
    );
    assert!(!use_places_of(&v, "src/ILog.cs", "Salvar").contains(&"src/Servico.cs:9:Gravar".to_string()), "{v}");
}

/// O parâmetro com tipo escrito (`IRepo repo`) dá o tipo do receptor.
#[test]
fn a_call_on_a_parameter_with_a_written_type_links_to_the_method_of_that_type_in_csharp() {
    let v = scan_files("csharp-parameter-receiver", CSHARP_TYPED_RECEIVERS);
    assert!(
        proven_uses_of(&v, "src/IRepo.cs", "Salvar").contains(&"src/Servico.cs:15:Outro".to_string()),
        "{v}"
    );
    assert!(!use_places_of(&v, "src/ILog.cs", "Salvar").contains(&"src/Servico.cs:15:Outro".to_string()), "{v}");
}

/// O nome escrito sozinho que não é campo do tipo em volta (`Util.Fazer()`,
/// a classe estática) segue pelo caminho de sempre, ao tipo que o nome diz.
#[test]
fn a_name_written_alone_that_is_not_a_field_still_names_the_type() {
    let v = scan_files("csharp-static-receiver", CSHARP_TYPED_RECEIVERS);
    assert_eq!(proven_uses_of(&v, "src/Util.cs", "Fazer"), vec!["src/Servico.cs:10:Gravar".to_string()], "{v}");
}

/// Dois tipos com o método `save`, e um serviço que o chama pelo campo
/// declarado no parâmetro do construtor (`private readonly repo: Repo`) e pelo
/// parâmetro de função com tipo (`repo: Repo`).
const TYPESCRIPT_TYPED_RECEIVERS: &[(&str, &str)] = &[
    ("src/repo.ts", "export class Repo {\n  save(id: number) {\n    return id;\n  }\n}\n"),
    ("src/outro.ts", "export class Outro {\n  save(id: number) {\n    return id;\n  }\n}\n"),
    (
        "src/servico.ts",
        "import { Repo } from './repo';\nimport { Outro } from './outro';\n\nexport class Servico {\n  constructor(private readonly repo: Repo, private outro: Outro) {}\n\n  gravar() {\n    return this.repo.save(1);\n  }\n}\n\nexport function solto(repo: Repo) {\n  return repo.save(2);\n}\n",
    ),
];

/// O parâmetro do construtor com modificador é campo da classe: a chamada
/// `this.repo.save(1)` liga ao `save` do `Repo`, provado, e não ao do `Outro`.
#[test]
fn a_call_on_a_constructor_property_links_to_the_method_of_its_type() {
    let v = scan_files("typescript-property-receiver", TYPESCRIPT_TYPED_RECEIVERS);
    assert!(proven_uses_of(&v, "src/repo.ts", "save").contains(&"src/servico.ts:8:gravar".to_string()), "{v}");
    assert!(!use_places_of(&v, "src/outro.ts", "save").contains(&"src/servico.ts:8:gravar".to_string()), "{v}");
}

/// O parâmetro de função com tipo escrito (`repo: Repo`) dá o tipo do
/// receptor.
#[test]
fn a_call_on_a_parameter_with_a_written_type_links_to_the_method_of_that_type_in_typescript() {
    let v = scan_files("typescript-parameter-receiver", TYPESCRIPT_TYPED_RECEIVERS);
    assert!(proven_uses_of(&v, "src/repo.ts", "save").contains(&"src/servico.ts:13:solto".to_string()), "{v}");
    assert!(!use_places_of(&v, "src/outro.ts", "save").contains(&"src/servico.ts:13:solto".to_string()), "{v}");
}

/// Um tipo `Pedido` sem método algum, o método `Total` de extensão dele
/// (`this Pedido`) numa classe estática à parte, um homônimo de outro tipo e
/// um serviço que chama `pedido.Total()` com o parâmetro de tipo escrito.
const CSHARP_EXTENSION_METHOD: &[(&str, &str)] = &[
    ("src/Pedido.cs", "namespace Loja;\n\npublic class Pedido\n{\n}\n"),
    (
        "src/PedidoExtensions.cs",
        "namespace Loja;\n\npublic static class PedidoExtensions\n{\n    public static int Total(this Pedido pedido)\n    {\n        return 1;\n    }\n}\n",
    ),
    ("src/Carrinho.cs", "namespace Loja;\n\npublic class Carrinho\n{\n    public int Total()\n    {\n        return 2;\n    }\n}\n"),
    (
        "src/Servico.cs",
        "namespace Loja;\n\npublic class Servico\n{\n    public int Somar(Pedido pedido)\n    {\n        return pedido.Total();\n    }\n}\n",
    ),
];

/// O método de extensão mora na classe estática, não no tipo do receptor: o
/// tipo de `pedido` é `Pedido`, que não tem `Total`, e a chamada não liga ao
/// método de extensão por isso. Fica suspeita entre os dois `Total` que o
/// arquivo vê.
#[test]
fn a_call_on_an_extension_method_is_not_proven_by_the_type_of_the_receiver() {
    let v = scan_files("csharp-extension-method", CSHARP_EXTENSION_METHOD);
    let site = "src/Servico.cs:7:Somar".to_string();
    assert!(proven_uses_of(&v, "src/PedidoExtensions.cs", "Total").is_empty(), "{v}");
    assert!(proven_uses_of(&v, "src/Carrinho.cs", "Total").is_empty(), "{v}");
    assert!(use_places_of(&v, "src/PedidoExtensions.cs", "Total").contains(&site), "{v}");
    assert!(use_places_of(&v, "src/Carrinho.cs", "Total").contains(&site), "{v}");
}

/// Dois tipos com `Salvar`, um campo `_repo` do tipo `IRepo` declarado na
/// classe base e um filho que o chama pelo nome sozinho.
const CSHARP_BASE_CLASS_FIELD: &[(&str, &str)] = &[
    ("src/IRepo.cs", "namespace Loja;\n\npublic interface IRepo\n{\n    void Salvar(int id);\n}\n"),
    ("src/ILog.cs", "namespace Loja;\n\npublic interface ILog\n{\n    void Salvar(int id);\n}\n"),
    ("src/Base.cs", "namespace Loja;\n\npublic class Base\n{\n    protected readonly IRepo _repo;\n}\n"),
    (
        "src/Filho.cs",
        "namespace Loja;\n\npublic class Filho : Base\n{\n    public void Gravar()\n    {\n        _repo.Salvar(1);\n    }\n}\n",
    ),
];

/// O campo lido pelo tipo em volta é só o que o tipo declara: `_repo` é do
/// `Base`, e o `Filho` não o herda para o mapa. O tipo do receptor não sai, e
/// a chamada fica suspeita entre os dois `Salvar`, sem provar o do `IRepo`.
#[test]
fn a_call_on_a_field_of_the_base_class_is_not_proven_by_the_type_of_the_field() {
    let v = scan_files("csharp-base-class-field", CSHARP_BASE_CLASS_FIELD);
    let site = "src/Filho.cs:7:Gravar".to_string();
    assert!(proven_uses_of(&v, "src/IRepo.cs", "Salvar").is_empty(), "{v}");
    assert!(use_places_of(&v, "src/IRepo.cs", "Salvar").contains(&site), "{v}");
    assert!(use_places_of(&v, "src/ILog.cs", "Salvar").contains(&site), "{v}");
}

/// Um `Ctx` com o campo `config` do tipo `Config` e, dentro de um módulo em
/// linha do mesmo arquivo, outro `Ctx` de mesmo nome; a função de fora chama
/// `ctx.config.language()` pelo `Ctx` de fora, e `Outra` tem um `language`
/// homônimo.
const RUST_INLINE_MODULE_SHADOW: &[(&str, &str)] = &[
    ("Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    ("src/main.rs", "mod app;\nmod config;\nmod outra;\n\nfn main() {}\n"),
    (
        "src/config.rs",
        "pub struct Config;\n\nimpl Config {\n    pub fn language(&self) -> u8 {\n        1\n    }\n}\n",
    ),
    (
        "src/outra.rs",
        "pub struct Outra;\n\nimpl Outra {\n    pub fn language(&self) -> u8 {\n        2\n    }\n}\n",
    ),
    (
        "src/app.rs",
        "use crate::config::Config;\nuse crate::outra::Outra;\n\npub struct Ctx {\n    pub config: Config,\n}\n\nmod interno {\n    use crate::outra::Outra;\n\n    pub struct Ctx {\n        pub config: Outra,\n    }\n}\n\npub fn ler(ctx: &Ctx) -> u8 {\n    ctx.config.language()\n}\n",
    ),
];

/// O mapa não separa os nomes por módulo em linha: os dois `Ctx` do arquivo
/// empatam, o tipo do receptor não sai, e a chamada não liga ao `language` do
/// `Config` por ele. Fica suspeita entre os dois.
#[test]
fn a_type_that_an_inline_module_of_the_file_repeats_does_not_give_the_type_of_the_receiver() {
    let v = scan_files("rust-inline-module-shadow", RUST_INLINE_MODULE_SHADOW);
    let site = "src/app.rs:17:ler".to_string();
    assert!(proven_uses_of(&v, "src/config.rs", "language").is_empty(), "{v}");
    assert!(use_places_of(&v, "src/config.rs", "language").contains(&site), "{v}");
    assert!(use_places_of(&v, "src/outra.rs", "language").contains(&site), "{v}");
}
