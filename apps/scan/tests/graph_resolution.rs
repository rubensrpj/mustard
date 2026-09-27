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

use std::path::PathBuf;
use std::process::Command;

/// A committed fixture root, resolved from the crate manifest dir.
fn fixture(name: &str) -> PathBuf {
    manifest_dir::manifest_dir().join("tests").join("fixtures").join(name)
}

/// Scan a fixture into a temp `grain.model.json` and return the parsed value.
/// Mirrors `php_laravel_fixture.rs`: a per-CALL temp dir (label + fixture name
/// + pid) so parallel tests scanning the same fixture never yank each other's
///   dir (the per-language test and the non-regression test share fixtures).
fn scan_fixture_labeled(label: &str, name: &str) -> serde_json::Value {
    let temp = tempfile::Builder::new().prefix(&format!("scan-graph-{}-{}-", label, name)).tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let model = dir.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", fixture(name).to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan over fixture");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&model).expect("read model")).expect("valid model JSON");
    v
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

/// Cascade smoke: with PHP FQCNs resolving, the graph stops collapsing into a
/// single `L0` layer and hubs/touchpoints/fan-in stop being empty.
/// Fixture shape: 3 Models <- 2 Services <- 2 Controllers, every import an
/// internal FQCN (`App\Models\User`, `App\Services\UserService`, ...):
///   UserService -> User; PostService -> Post, Comment;
///   UserController -> UserService, User; PostController -> PostService, Post.
#[test]
fn graph_resolution_php_cascade_layers_hubs_touchpoints() {
    let v = scan_fixture("graph_php_cascade");
    let g = &v["graph"];

    assert_eq!(g["edges"].as_u64(), Some(7), "all 7 internal FQCN imports resolve: {g}");

    // Layers: Models at `L0`, Services at `L1`, Controllers at `L2` — not one
    // flat `L0`.
    let layers = g["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3, "emergent layering must have 3 depths: {g}");
    let l0 = layers.iter().find(|l| l["name"] == "L0").expect("L0 present");
    assert_eq!(l0["modules"].as_u64(), Some(3), "the 3 Models are the innermost layer: {g}");

    // Fan-in: the models are depended upon (User and Post twice each).
    let fan_in = fan_in_modules(&v);
    assert!(!fan_in.is_empty(), "fan-in must not be empty: {g}");
    assert!(
        fan_in.contains(&"app/Models/User.php".to_string()),
        "User model is a fan-in target: {fan_in:?}"
    );

    // Touchpoints/hubs: controllers import across Services + Models (breadth 2).
    let touchpoints = g["touchpoints"].as_array().unwrap();
    assert!(!touchpoints.is_empty(), "touchpoints must not be empty: {g}");
    assert_eq!(touchpoints[0]["breadth"].as_u64(), Some(2), "top hub spans two dirs: {touchpoints:?}");
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
