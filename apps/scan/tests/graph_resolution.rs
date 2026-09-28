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
//!     (`super::super::x::f()`), só o item depois do `super` (`super::soma()`),
//!     com argumento de tipo (`crate::a::Caixa::<u8>::new()`), por um módulo
//!     escrito dentro do arquivo (`crate::a::dentro::Pote::new()`), a partir
//!     do arquivo que responde pela própria pasta (`src/k/mod.rs`) e por um
//!     caminho que não nomeia nada do projeto (`crate::nada::f()`, ao lado de
//!     um `src/a/nada.rs`).
//!     E `graph_rust_inner_module/`: `super` escrito dentro de um módulo do
//!     próprio arquivo (`mod interno { }` de `src/a.rs`) e dentro do trecho de
//!     teste, que sai do módulo antes de subir pasta, também na passada que
//!     reaproveita o arquivo sem relê-lo.
//!     E `graph_rust_call_path/`: `valor` declarado em `src/a.rs` e em
//!     `src/x.rs`, chamado de `src/a.rs` por `super::super::x::valor()`, por
//!     `super::valor()` dentro de um módulo do arquivo e sem caminho.
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
fn graph_resolution_csharp() {
    let v = scan_fixture("graph_csharp");
    assert!(edges(&v) > 0, "C# `using Demo.Models;` must resolve to an internal edge: {}", v["graph"]);
}

#[test]
fn graph_resolution_typescript() {
    let v = scan_fixture("graph_typescript");
    assert!(edges(&v) > 0, "TS relative import `./user` must resolve to an internal edge: {}", v["graph"]);
}

#[test]
fn graph_resolution_go() {
    let v = scan_fixture("graph_go");
    assert!(edges(&v) > 0, "Go module-prefixed import must resolve to an internal edge: {}", v["graph"]);
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

#[test]
fn graph_resolution_php() {
    let v = scan_fixture("graph_php");
    assert!(edges(&v) > 0, "PHP `use App\\Models\\User;` must resolve to an internal edge: {}", v["graph"]);
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

/// Um import sem extensão cujo nome tem ponto (`./pedido.service`) liga ao
/// arquivo com esse nome inteiro, e não ao arquivo que só tem o começo dele
/// (`pedido.ts`, ao lado): o ponto do meio é parte do nome.
#[test]
fn a_dotted_import_links_to_the_file_with_the_whole_name() {
    let v = scan_fixture_labeled("dotted", "graph_typescript_aliases");
    assert_eq!(deps_of(&v, "src/app/checkout.ts"), vec!["src/app/pedido.service.ts".to_string()]);
}

/// O import que escreve a extensão de saída no lugar da do arquivo
/// (`./pedido.service.js` para `pedido.service.ts`) segue ligando ao arquivo
/// certo.
#[test]
fn an_import_written_with_the_output_extension_still_links() {
    let v = scan_fixture_labeled("output-ext", "graph_typescript_aliases");
    assert_eq!(deps_of(&v, "src/app/esm.ts"), vec!["src/app/pedido.service.ts".to_string()]);
}

/// O apelido de pasta declarado numa configuração herdada pela da raiz
/// (`@app/*` para `src/app/*`, com comentário e vírgula sobrando no arquivo)
/// liga `@app/pedido` a `src/app/pedido.ts`.
#[test]
fn a_folder_alias_inherited_through_extends_links_to_the_right_file() {
    let v = scan_fixture_labeled("alias", "graph_typescript_aliases");
    assert_eq!(deps_of(&v, "src/usa_apelido.ts"), vec!["src/app/pedido.ts".to_string()]);
}

/// A configuração mais próxima de quem importa vence a da raiz: em `pkg/`,
/// `@app/pedido` é `pkg/lib/pedido.ts`, e não o `src/app/pedido.ts` que a
/// raiz daria.
#[test]
fn the_nearest_configuration_wins_over_the_root_one() {
    let v = scan_fixture_labeled("nearest", "graph_typescript_aliases");
    assert_eq!(deps_of(&v, "pkg/usa.ts"), vec!["pkg/lib/pedido.ts".to_string()]);
}

/// Um ponto na frente é a pasta de quem importa: `from .models import Pedido`
/// em `pkg/views.py` liga a `pkg/models.py`, ao lado.
#[test]
fn a_one_dot_relative_import_links_to_the_file_beside() {
    let v = scan_fixture_labeled("py-one-dot", "graph_python_relative");
    assert_eq!(deps_of(&v, "pkg/views.py"), vec!["pkg/models.py".to_string()]);
}

/// Dois pontos sobem uma pasta, e o resto, cortado nos pontos, é o caminho
/// dentro dela: `from ..core.regras import LIMITE` em `pkg/api/handlers.py`
/// liga a `pkg/core/regras.py`.
#[test]
fn a_two_dot_relative_import_climbs_one_folder() {
    let v = scan_fixture_labeled("py-two-dots", "graph_python_relative");
    assert_eq!(deps_of(&v, "pkg/api/handlers.py"), vec!["pkg/core/regras.py".to_string()]);
}

/// `from . import models` traz um arquivo da própria pasta: liga a
/// `pkg/models.py`, e não à pasta nem ao `__init__.py` dela.
#[test]
fn a_from_dot_import_links_to_the_named_file_of_the_folder() {
    let v = scan_fixture_labeled("py-from-dot", "graph_python_relative");
    assert_eq!(deps_of(&v, "pkg/admin.py"), vec!["pkg/models.py".to_string()]);
}

/// O import relativo que nomeia uma pasta de pacote liga ao `__init__.py`
/// dela: `from .servicos import cobrar` liga a `pkg/servicos/__init__.py`.
#[test]
fn a_relative_import_of_a_package_links_to_its_init_file() {
    let v = scan_fixture_labeled("py-package", "graph_python_relative");
    assert_eq!(deps_of(&v, "pkg/usa_pacote.py"), vec!["pkg/servicos/__init__.py".to_string()]);
}

/// O import sem ponto segue lido a partir da raiz, e não da pasta de quem
/// importa: `from pkg.models import Pedido` em `pkg/api/absoluto.py` liga a
/// `pkg/models.py`.
#[test]
fn an_absolute_import_is_still_read_from_the_root() {
    let v = scan_fixture_labeled("py-absolute", "graph_python_relative");
    assert_eq!(deps_of(&v, "pkg/api/absoluto.py"), vec!["pkg/models.py".to_string()]);
}

/// Três arquivos no mesmo namespace; quem importa o namespace e usa só um tipo
/// dele liga só ao arquivo desse tipo.
#[test]
fn a_namespace_import_links_only_to_the_file_of_the_type_it_uses() {
    let v = scan_fixture_labeled("ns-used", "graph_csharp_namespace");
    assert_eq!(deps_of(&v, "src/Services/PedidoService.cs"), vec!["src/Models/Pedido.cs".to_string()]);
}

/// Quem importa o namespace e não usa nada dele não liga a nenhum arquivo.
#[test]
fn a_namespace_import_with_nothing_used_links_to_nothing() {
    let v = scan_fixture_labeled("ns-unused", "graph_csharp_namespace");
    assert_eq!(deps_of(&v, "src/Services/SemUso.cs"), Vec::<String>::new());
}

/// O nome qualificado completo de um tipo segue ligando ao arquivo que leva o
/// nome dele, use o arquivo o tipo ou não.
#[test]
fn a_fully_qualified_import_still_links_to_the_file_of_the_type() {
    let v = scan_fixture_labeled("ns-qualified", "graph_csharp_namespace");
    assert_eq!(deps_of(&v, "src/Services/Qualificado.cs"), vec!["src/Models/Cliente.cs".to_string()]);
}

/// O nome qualificado de um tipo que nenhum arquivo leva no nome liga só ao
/// arquivo do namespace que declara o que quem importa usa, e não ao
/// namespace inteiro: `Aplicar`, de `Desconto`, mora em `Produto.cs`.
#[test]
fn a_qualified_type_without_its_own_file_links_only_to_what_is_used() {
    let v = scan_fixture_labeled("ns-no-file", "graph_csharp_namespace");
    assert_eq!(deps_of(&v, "src/Services/SemArquivo.cs"), vec!["src/Models/Produto.cs".to_string()]);
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

/// A chamada escrita pelo caminho completo no corpo, sem `use`
/// (`crate::a::b::f()` em `src/main.rs`), liga o arquivo ao que o caminho
/// nomeia: `src/a/b.rs`.
#[test]
fn a_call_by_the_full_path_from_the_crate_links_to_the_file() {
    let v = scan_fixture_labeled("rs-crate-path", "graph_rust_qualified");
    assert_eq!(deps_of(&v, "src/main.rs"), vec!["src/a/b.rs".to_string()]);
}

/// O caminho completo que começa em `super` é lido a partir da pasta de quem
/// chama: `super::x::f()` em `src/a/b.rs` liga a `src/a/x.rs`.
#[test]
fn a_call_by_the_full_path_from_super_links_to_the_file_beside() {
    let v = scan_fixture_labeled("rs-super-path", "graph_rust_qualified");
    assert_eq!(deps_of(&v, "src/a/b.rs"), vec!["src/a/x.rs".to_string()]);
    assert_eq!(deps_of(&v, "src/a/x.rs"), Vec::<String>::new());
}

/// O caminho que não começa no próprio projeto (`std::fs::read()`,
/// `Vec::new()`) não vira import, e o nome chamado segue lido como uso, com o
/// qualificador de antes: dos três caminhos de `src/main.rs`, só
/// `crate::a::b` é import.
#[test]
fn a_call_by_a_path_outside_the_project_is_not_an_import() {
    let v = scan_fixture_labeled("rs-outside-path", "graph_rust_qualified");
    assert_eq!(imports_of(&v, "src/main.rs"), vec!["crate::a::b".to_string()]);
    let module = v["modules"].as_array().unwrap().iter().find(|m| m["path"] == "src/main.rs").unwrap();
    let calls: Vec<&str> = module["calls"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert!(calls.contains(&"fs.read:3"), "a chamada de fora segue como uso: {calls:?}");
}

/// Cada `super` a mais sobe uma pasta: `super::super::x::f()` em
/// `src/a/c/d.rs` liga a `src/a/x.rs`.
#[test]
fn a_call_that_climbs_two_folders_links_to_the_file_there() {
    let v = scan_fixture_labeled("rs-super-super", "graph_rust_qualified");
    assert_eq!(deps_of(&v, "src/a/c/d.rs"), vec!["src/a/x.rs".to_string()]);
}

/// O `super` seguido só do item liga ao arquivo que responde pela pasta de
/// cima: `super::soma()` em `src/a/y.rs` liga a `src/a.rs`.
#[test]
fn a_call_by_super_and_the_item_links_to_the_file_of_the_folder_above() {
    let v = scan_fixture_labeled("rs-super-item", "graph_rust_qualified");
    assert_eq!(deps_of(&v, "src/a/y.rs"), vec!["src/a.rs".to_string()]);
}

/// Os argumentos de tipo saem do caminho: `crate::a::Caixa::<u8>::new()` vira
/// o import `crate::a::Caixa` e liga a `src/a.rs`, onde `Caixa` mora.
#[test]
fn a_call_with_type_arguments_in_the_path_links_to_the_file() {
    let v = scan_fixture_labeled("rs-type-args", "graph_rust_qualified");
    assert_eq!(imports_of(&v, "src/generico.rs"), vec!["crate::a::Caixa".to_string()]);
    assert_eq!(deps_of(&v, "src/generico.rs"), vec!["src/a.rs".to_string()]);
}

/// O caminho que passa por um módulo escrito dentro do arquivo, e termina em
/// tipo, perde do fim quantas partes for preciso até achar arquivo:
/// `crate::a::dentro::Pote::new()` liga a `src/a.rs`.
#[test]
fn a_path_through_a_module_inside_the_file_links_to_the_file() {
    let v = scan_fixture_labeled("rs-inner-module", "graph_rust_qualified");
    assert_eq!(deps_of(&v, "src/interno.rs"), vec!["src/a.rs".to_string()]);
}

/// O arquivo que responde pela própria pasta já é o módulo dela, e o `super`
/// dele sobe a partir da pasta de cima: `super::a::x::f()` em `src/k/mod.rs`
/// liga a `src/a/x.rs`.
#[test]
fn the_file_that_answers_for_its_folder_climbs_from_the_folder_above() {
    let v = scan_fixture_labeled("rs-index-super", "graph_rust_qualified");
    assert_eq!(deps_of(&v, "src/k/mod.rs"), vec!["src/a/x.rs".to_string()]);
}

/// O caminho que não nomeia nada do projeto não liga a um arquivo de outro
/// lugar só porque o caminho dele termina igual: `crate::nada::f()` em
/// `src/sem_alvo.rs` não liga a `src/a/nada.rs`.
#[test]
fn a_crate_path_that_names_nothing_does_not_link_by_the_end_of_another_path() {
    let v = scan_fixture_labeled("rs-no-target", "graph_rust_qualified");
    assert_eq!(imports_of(&v, "src/sem_alvo.rs"), vec!["crate::nada".to_string()]);
    assert_eq!(deps_of(&v, "src/sem_alvo.rs"), Vec::<String>::new());
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
/// fica no arquivo: `super::valor()` dentro de `mod interno { }` de `src/a.rs`
/// é o `valor` de `src/a.rs`, e não o `valor` homônimo de `src/main.rs`, onde o
/// caminho cairia se subisse pasta direto.
fn assert_super_inside_a_module_stays_in_the_file(v: &serde_json::Value) {
    assert_eq!(used_by_of(v, "src/a.rs", "valor"), vec!["src/a.rs:7:perto".to_string()]);
    assert_eq!(used_by_of(v, "src/main.rs", "valor"), Vec::<String>::new());
    assert!(!deps_of(v, "src/a.rs").contains(&"src/main.rs".to_string()), "{:?}", deps_of(v, "src/a.rs"));
}

/// O segundo `super` escrito dentro do mesmo módulo é o que sobe pasta:
/// `super::super::x::dobro()` liga `src/a.rs` a `src/x.rs`.
fn assert_second_super_inside_a_module_climbs_one_folder(v: &serde_json::Value) {
    assert_eq!(deps_of(v, "src/a.rs"), vec!["src/x.rs".to_string()]);
}

/// O `use super::*` do trecho de teste é o próprio arquivo: não vira arquivo
/// coberto pelo teste, e o arquivo de cima não ganha o teste.
fn assert_super_of_the_test_block_stays_in_the_file(v: &serde_json::Value) {
    assert_eq!(list_of(v, "src/a.rs", "test_imports"), vec!["super::*".to_string()]);
    assert_eq!(list_of(v, "src/a.rs", "test_deps"), Vec::<String>::new());
    assert_eq!(list_of(v, "src/main.rs", "tests"), Vec::<String>::new());
}

#[test]
fn super_inside_a_module_of_the_file_links_to_the_file_itself() {
    let v = scan_fixture_labeled("rs-inner-super", "graph_rust_inner_module");
    assert_super_inside_a_module_stays_in_the_file(&v);
}

#[test]
fn super_super_inside_a_module_of_the_file_climbs_one_folder() {
    let v = scan_fixture_labeled("rs-inner-super-super", "graph_rust_inner_module");
    assert_second_super_inside_a_module_climbs_one_folder(&v);
}

#[test]
fn super_of_the_test_block_stays_in_the_file() {
    let v = scan_fixture_labeled("rs-inner-test-block", "graph_rust_inner_module");
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
    let temp = tempfile::Builder::new().prefix(&format!("scan-graph-reuse-{name}-")).tempdir().unwrap();
    let dir = temp.path();
    copy_tree(&fixture(name), dir);
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let (first, report) = scan_in_place(dir);
    assert_eq!(report["full"], serde_json::Value::Bool(true), "{report}");

    let main = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
    std::fs::write(dir.join("src/main.rs"), format!("{main}\npub fn outra() {{}}\n")).unwrap();
    git(dir, &["commit", "-q", "-am", "segundo"]);
    let (second, report) = scan_in_place(dir);
    assert_eq!(report["read"], serde_json::json!(["src/main.rs"]), "só o que mudou é relido: {report}");
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
/// declarações dele: `super::super::x::valor()`, na linha 15 de `src/a.rs`, é
/// o `valor` de `src/x.rs`, e não o `valor` homônimo do próprio arquivo.
fn assert_a_call_by_a_path_links_only_to_the_file_it_names(v: &serde_json::Value) {
    assert_eq!(holders_of(v, "valor", "src/a.rs:15:longe"), vec!["src/x.rs".to_string()]);
}

/// O próprio arquivo só entra quando o caminho o nomeia: `super::valor()`
/// dentro de `mod interno`, na linha 11 de `src/a.rs`, é o `valor` de
/// `src/a.rs`, e não o de `src/x.rs`, que o arquivo também importa.
fn assert_a_path_to_the_file_itself_links_only_to_the_file(v: &serde_json::Value) {
    assert_eq!(holders_of(v, "valor", "src/a.rs:11:perto"), vec!["src/a.rs".to_string()]);
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
/// tipo é essa declaração: `valor()`, na linha 6 de `src/a.rs`, é o `valor`
/// do próprio arquivo, provado, e não o de `src/x.rs`, que o arquivo também
/// importa pelo caminho da linha 15.
fn assert_a_call_without_a_path_links_to_the_file_own_declaration(v: &serde_json::Value) {
    assert_eq!(holders_of(v, "valor", "src/a.rs:6:soma"), vec!["src/a.rs".to_string()]);
    assert!(suspect_holders_of(v, "valor", "src/a.rs:6:soma").is_empty(), "nenhuma suspeita");
}

#[test]
fn a_call_by_a_path_links_only_to_the_file_it_names() {
    let v = scan_fixture_labeled("rs-path-only", "graph_rust_call_path");
    assert_a_call_by_a_path_links_only_to_the_file_it_names(&v);
}

#[test]
fn a_path_to_the_file_itself_links_only_to_the_file() {
    let v = scan_fixture_labeled("rs-path-itself", "graph_rust_call_path");
    assert_a_path_to_the_file_itself_links_only_to_the_file(&v);
}

#[test]
fn a_call_without_a_path_links_to_the_file_own_declaration() {
    let v = scan_fixture_labeled("rs-no-path", "graph_rust_call_path");
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
fn folder_that_passes_on(index: &str, tela: &str) -> Vec<(&'static str, String)> {
    vec![
        ("web/src/pedidos/index.ts", index.to_string()),
        (
            "web/src/pedidos/pedido.service.ts",
            "export function buscarPedido(id: number) {\n  return id;\n}\n".to_string(),
        ),
        ("web/src/tela.ts", tela.to_string()),
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
