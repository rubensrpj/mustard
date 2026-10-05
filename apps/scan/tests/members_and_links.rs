//! O mapa liga cada método ao tipo dono e ao contrato que ele cumpre: os
//! membros de um tipo, o dono de cada declaração e as implementações de cada
//! método. A varredura é a de verdade, em projetos pequenos gravados em disco,
//! e a conferência é a do núcleo, sobre o mapa gravado.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use mustard_core::domain::project_map::{DeclAt, ProjectMap};
use serde_json::json;

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Um projeto vazio numa pasta temporária.
fn project(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new().prefix(&format!("scan-{name}-")).tempdir().unwrap()
}

/// Varre `dir` e devolve o mapa gravado, como as perguntas o leem, e o relato
/// da passada.
fn scan(dir: &Path, extra: &[&str]) -> (ProjectMap, serde_json::Value) {
    let (map, report) = model::scan(dir, &dir.join(".claude"), extra);
    (serde_json::from_value(map).expect("o mapa se lê"), report)
}

/// O que o mapa liga a uma declaração: os donos dela (do mais interno para o
/// mais externo, com o contrato escrito por último), os membros e as
/// implementações.
#[derive(Debug)]
struct DeclRelations {
    line: u64,
    owners: Vec<String>,
    members: Vec<DeclAt>,
    implements: Vec<DeclAt>,
    implemented_by: Vec<DeclAt>,
}

/// Cada declaração `name` de `file`, em ordem de linha, com as ligações dela.
fn all_relations(map: &ProjectMap, file: &str, name: &str) -> Vec<DeclRelations> {
    let module = map.modules.iter().find(|m| m.path == file).expect("o arquivo está no mapa");
    let mut found: Vec<_> = module.declarations.iter().filter(|d| d.name == name).collect();
    found.sort_by_key(|d| d.line);
    found
        .into_iter()
        .map(|d| DeclRelations {
            line: d.line,
            owners: d.owner.iter().chain(&d.contract).cloned().collect(),
            members: d.members.clone(),
            implements: d.implements.clone(),
            implemented_by: d.implemented_by.clone(),
        })
        .collect()
}

/// A única declaração `name` de `file`, com os donos, os membros e as
/// implementações dela.
fn relations(map: &ProjectMap, file: &str, name: &str) -> DeclRelations {
    let mut found = all_relations(map, file, name);
    assert_eq!(found.len(), 1, "{found:?}");
    found.remove(0)
}

/// Os nomes dos membros, na ordem do mapa.
fn member_names(found: &DeclRelations) -> Vec<&str> {
    found.members.iter().map(|m| m.name.as_str()).collect()
}

fn at(file: &str, line: usize, name: &str) -> DeclAt {
    DeclAt { file: file.to_string(), line, name: name.to_string() }
}

#[test]
fn a_class_with_three_methods_and_a_field_lists_the_three_methods_first() {
    let temp = project("membros-csharp");
    let dir = temp.path();
    write(
        dir,
        "Loja/Carrinho.cs",
        "namespace Loja;\n\n\
         public class Carrinho\n{\n    \
             private int total;\n\n    \
             public void Adicionar() { }\n\n    \
             public void Remover() { }\n\n    \
             public int Somar() { return total; }\n\
         }\n",
    );
    let (map, _) = scan(dir, &[]);

    let cart = relations(&map, "Loja/Carrinho.cs", "Carrinho");
    assert_eq!(member_names(&cart), ["Adicionar", "Remover", "Somar", "total"], "{cart:?}");
    assert_eq!(relations(&map, "Loja/Carrinho.cs", "Somar").owners, ["Carrinho"]);
    assert_eq!(relations(&map, "Loja/Carrinho.cs", "total").owners, ["Carrinho"]);
}

#[test]
fn a_method_written_outside_the_type_is_a_member_with_the_type_as_owner() {
    // No Rust, o método do bloco `impl Tipo`; no enum, o campo de uma variante
    // não é membro.
    let temp = project("membros-impl");
    let dir = temp.path();
    write(dir, "Cargo.toml", "[package]\nname = \"formas\"\nversion = \"0.1.0\"\n");
    write(
        dir,
        "src/lib.rs",
        "pub struct Tipo {\n    pub largura: u32,\n}\n\n\
         impl Tipo {\n    \
             pub const PADRAO: u32 = 1;\n\n    \
             pub fn andar(&self) -> u32 {\n        self.largura\n    }\n\
         }\n\n\
         pub enum Forma {\n    Ponto,\n    Circulo { raio: u32 },\n}\n\n\
         impl Forma {\n    pub fn area(&self) -> u32 {\n        0\n    }\n}\n",
    );
    // No Go, o método com receptor, com e sem ponteiro.
    write(dir, "banco/go.mod", "module banco\n\ngo 1.22\n");
    write(
        dir,
        "banco/conta.go",
        "package banco\n\n\
         type Conta struct {\n\tsaldo int\n}\n\n\
         func (c *Conta) Depositar(valor int) {\n\tc.saldo += valor\n}\n\n\
         func (c Conta) Saldo() int {\n\treturn c.saldo\n}\n",
    );
    let (map, _) = scan(dir, &[]);

    let type_decl = relations(&map, "src/lib.rs", "Tipo");
    assert_eq!(member_names(&type_decl), ["andar", "largura", "PADRAO"], "{type_decl:?}");
    assert_eq!(relations(&map, "src/lib.rs", "andar").owners, ["Tipo"]);
    assert_eq!(relations(&map, "src/lib.rs", "PADRAO").owners, ["Tipo"]);
    let shape = relations(&map, "src/lib.rs", "Forma");
    assert_eq!(member_names(&shape), ["area", "Ponto", "Circulo"], "{shape:?}");

    let account = relations(&map, "banco/conta.go", "Conta");
    assert_eq!(member_names(&account), ["Depositar", "Saldo", "saldo"], "{account:?}");
    assert_eq!(relations(&map, "banco/conta.go", "Depositar").owners, ["Conta"]);
    assert_eq!(relations(&map, "banco/conta.go", "Saldo").owners, ["Conta"]);
}

#[test]
fn a_trait_impl_in_one_file_links_its_method_to_the_trait_method_in_another() {
    let temp = project("membros-traco");
    let dir = temp.path();
    write(dir, "Cargo.toml", "[package]\nname = \"bichos\"\nversion = \"0.1.0\"\n");
    write(dir, "src/lib.rs", "pub mod falante;\npub mod bichos;\n");
    write(dir, "src/falante.rs", "pub trait Falante {\n    fn falar(&self) -> String;\n}\n");
    write(
        dir,
        "src/bichos.rs",
        "use crate::falante::Falante;\n\n\
         pub struct Cao;\n\n\
         impl Falante for Cao {\n    fn falar(&self) -> String {\n        String::from(\"au\")\n    }\n}\n\n\
         pub struct Gato;\n\n\
         impl Gato {\n    pub fn falar(&self) -> String {\n        String::from(\"miau\")\n    }\n}\n",
    );
    let (map, _) = scan(dir, &[]);

    let speak = all_relations(&map, "src/bichos.rs", "falar");
    let (dog, cat) = (&speak[0], &speak[1]);
    assert_eq!(dog.owners, ["Cao", "Falante"], "{dog:?}");
    assert_eq!(dog.implements, [at("src/falante.rs", 2, "falar")]);
    // O método do bloco sem traço não cumpre o traço, mesmo com o mesmo nome.
    assert_eq!(cat.owners, ["Gato"], "{cat:?}");
    assert!(cat.implements.is_empty(), "{cat:?}");

    let contract = relations(&map, "src/falante.rs", "falar");
    assert_eq!(contract.owners, ["Falante"]);
    assert_eq!(contract.implemented_by, [at("src/bichos.rs", dog.line as usize, "falar")]);
    assert_eq!(member_names(&relations(&map, "src/bichos.rs", "Cao")), ["falar"]);
}

#[test]
fn a_typescript_class_implementing_an_interface_links_the_method_of_the_same_name() {
    let temp = project("membros-ts");
    let dir = temp.path();
    write(
        dir,
        "src/contrato.ts",
        "export interface Pagavel {\n  pagar(valor: number): void;\n  estornar(): void;\n}\n",
    );
    write(
        dir,
        "src/pedido.ts",
        "import { Pagavel } from \"./contrato\";\n\n\
         export class Pedido implements Pagavel {\n  \
             pagar(valor: number): void {}\n  \
             estornar(): void {}\n  \
             resumo(): string {\n    return \"\";\n  }\n\
         }\n",
    );
    let (map, _) = scan(dir, &[]);

    assert_eq!(relations(&map, "src/pedido.ts", "pagar").implements, [at("src/contrato.ts", 2, "pagar")]);
    assert_eq!(relations(&map, "src/pedido.ts", "estornar").implements, [at("src/contrato.ts", 3, "estornar")]);
    assert!(relations(&map, "src/pedido.ts", "resumo").implements.is_empty());
    assert_eq!(relations(&map, "src/contrato.ts", "pagar").implemented_by, [at("src/pedido.ts", 4, "pagar")]);
    assert_eq!(member_names(&relations(&map, "src/contrato.ts", "Pagavel")), ["pagar", "estornar"]);
}

#[test]
fn an_interface_repeated_in_two_folders_links_to_the_closest_path_and_a_tie_links_none() {
    let temp = project("membros-repetido");
    let dir = temp.path();
    let contract = "export interface Cobravel {\n  cobrar(): void;\n}\n";
    write(dir, "src/vendas/contrato.ts", contract);
    write(dir, "src/compras/contrato.ts", contract);
    // Mais pastas em comum com a interface de vendas.
    write(dir, "src/vendas/fatura.ts", "export class Fatura implements Cobravel {\n  cobrar(): void {}\n}\n");
    // As mesmas pastas em comum com as duas: empate.
    write(dir, "src/nota.ts", "export class Nota implements Cobravel {\n  cobrar(): void {}\n}\n");
    let (map, _) = scan(dir, &[]);

    assert_eq!(relations(&map, "src/vendas/fatura.ts", "cobrar").implements, [at("src/vendas/contrato.ts", 2, "cobrar")]);
    assert!(relations(&map, "src/nota.ts", "cobrar").implements.is_empty());
    assert!(relations(&map, "src/compras/contrato.ts", "cobrar").implemented_by.is_empty());
}

#[test]
fn an_incremental_pass_that_touches_only_the_class_file_rebuilds_the_link() {
    let temp = project("membros-incremental");
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    write(dir, "src/contrato.ts", "export interface Pagavel {\n  pagar(): void;\n}\n");
    write(dir, "src/pedido.ts", "export class Pedido {\n  pagar(): void {}\n}\n");
    write(dir, "Cargo.toml", "[package]\nname = \"bichos\"\nversion = \"0.1.0\"\n");
    write(dir, "src/lib.rs", "pub mod falante;\npub mod cao;\n");
    write(dir, "src/falante.rs", "pub trait Falante {\n    fn falar(&self);\n}\n");
    write(dir, "src/cao.rs", "pub struct Cao;\n\nimpl crate::falante::Falante for Cao {\n    fn falar(&self) {}\n}\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "first"]);
    let (map, first) = scan(dir, &[]);
    assert_eq!(first["full"], json!(true), "{first}");
    assert!(relations(&map, "src/pedido.ts", "pagar").implements.is_empty());

    // Só o arquivo da classe muda: a classe passa a cumprir a interface.
    write(dir, "src/pedido.ts", "export class Pedido implements Pagavel {\n  pagar(): void {}\n}\n");
    let (map, second) = scan(dir, &[]);
    assert_eq!(second["full"], json!(false), "{second}");
    assert_eq!(second["read"], json!(["src/pedido.ts"]), "{second}");
    assert_eq!(relations(&map, "src/pedido.ts", "pagar").implements, [at("src/contrato.ts", 2, "pagar")]);
    assert_eq!(relations(&map, "src/contrato.ts", "pagar").implemented_by, [at("src/pedido.ts", 2, "pagar")]);

    // Só o arquivo do traço muda: o dono escrito no `impl` volta do mapa com o
    // arquivo que não foi relido, e a ligação continua.
    write(dir, "src/falante.rs", "/// Quem fala.\npub trait Falante {\n    fn falar(&self);\n}\n");
    let (map, third) = scan(dir, &[]);
    assert_eq!(third["read"], json!(["src/falante.rs"]), "{third}");
    assert_eq!(relations(&map, "src/cao.rs", "falar").implements, [at("src/falante.rs", 3, "falar")]);

    // A classe deixa de cumprir a interface: a ligação sai dos dois lados.
    write(dir, "src/pedido.ts", "export class Pedido {\n  pagar(): void {}\n}\n");
    let (map, _) = scan(dir, &[]);
    assert!(relations(&map, "src/pedido.ts", "pagar").implements.is_empty());
    assert!(relations(&map, "src/contrato.ts", "pagar").implemented_by.is_empty());

    // O mapa lido aos poucos é o da leitura inteira.
    let stepped = model::read_bytes(&dir.join(".claude"));
    assert_eq!(scan(dir, &["--all"]).1["full"], json!(true));
    assert_eq!(model::read_bytes(&dir.join(".claude")), stepped);
}
