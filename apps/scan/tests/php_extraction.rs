//! End-to-end contract: the generic tree-sitter engine extracts imports,
//! namespaces, and declarations from a PHP file with no PHP-specific logic in
//! `src/`. PHP is wired purely as data (languages.toml + queries/php/*.scm); this
//! guards that the engine copies the generic capture vocabulary verbatim — the
//! `use` import path, the `namespace` name, and each `@definition.<kind>` with
//! its name and supertypes — straight out of a real `.php` source.

use std::process::Command;

/// Scan a project dir holding a single PHP file and return the emitted model.
fn scan_to_model(dir: &std::path::Path) -> serde_json::Value {
    let model = dir.join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", dir.to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&model).expect("read model")).expect("valid model JSON")
}

#[test]
fn php_extraction_pulls_imports_namespaces_and_declarations() {
    // A self-contained PHP file with a namespace, two `use` imports, a class that
    // extends a base, and a method. Written to a temp dir so the test owns its
    // input and stays deterministic (no dependence on the committed fixture).
    let dir = std::env::temp_dir().join(format!("scan-php-extract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("Account.php"),
        "<?php\n\
         \n\
         namespace App\\Domain;\n\
         \n\
         use Illuminate\\Database\\Eloquent\\Model;\n\
         use App\\Contracts\\Auditable;\n\
         \n\
         class Account extends Model implements Auditable\n\
         {\n\
             public function balance(): int\n\
             {\n\
                 return 0;\n\
             }\n\
         }\n",
    )
    .unwrap();

    let v = scan_to_model(&dir);

    // The single PHP module the engine produced.
    let module = v["modules"].as_array().unwrap().iter().find(|m| m["language"] == "php").expect("a php module");

    // Imports: the qualified `use` paths, copied verbatim by the generic engine.
    let imports: Vec<&str> = module["imports"].as_array().unwrap().iter().map(|i| i.as_str().unwrap()).collect();
    assert!(imports.contains(&"Illuminate\\Database\\Eloquent\\Model"), "imports: {imports:?}");
    assert!(imports.contains(&"App\\Contracts\\Auditable"), "imports: {imports:?}");

    // Namespace: the declared `namespace App\Domain;`.
    let namespaces: Vec<&str> = module["namespaces"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    assert_eq!(namespaces, vec!["App\\Domain"], "namespaces: {namespaces:?}");

    // Declarations: the class (kind copied from `@definition.class`) with its
    // base/interface supertypes, plus the method with its member kind.
    let decls = module["declarations"].as_array().unwrap();
    let class = decls.iter().find(|d| d["name"] == "Account").expect("Account declaration");
    assert_eq!(class["kind"], "class", "class kind copied verbatim from the capture suffix");
    let supers: Vec<&str> = class["supertypes"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert!(supers.contains(&"Model"), "extends base captured: {supers:?}");
    assert!(supers.contains(&"Auditable"), "implements interface captured: {supers:?}");
    assert!(decls.iter().any(|d| d["name"] == "balance" && d["kind"] == "method"), "method captured: {decls:?}");

    let _ = std::fs::remove_dir_all(&dir);
}
