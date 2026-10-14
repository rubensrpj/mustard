//! O que o mapa passa a guardar de cada declaração: o comentário de
//! documentação escrito em cima dela, a assinatura dela, e cada ligação
//! nomeada entre declarações — quem chama quem, em qual arquivo e em qual
//! linha. A varredura é a de verdade, num projeto de mentira gravado em disco,
//! e a conferência é feita no mapa gravado.

#[path = "support/model.rs"]
mod model;

use std::path::{Path, PathBuf};

use serde_json::Value;

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Varre `dir` e devolve o mapa gravado.
fn scan(dir: &Path) -> Value {
    model::scan(dir, &dir.join(".claude"), &[]).0
}

/// Um projeto de mentira: uma função documentada em português, e outro arquivo
/// que a chama duas vezes de dentro de uma função sua.
fn project(name: &str) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::Builder::new().prefix(&format!("scan-{name}-")).tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    write(&dir, "Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\n");
    write(&dir, "src/lib.rs", "pub mod preco;\npub mod pedido;\n");
    write(
        &dir,
        "src/preco.rs",
        "/// Soma o preço do pedido com o frete.\n\
         ///\n\
         /// O frete vem em centavos.\n\
         pub fn total(preco: u32, frete: u32) -> u32 {\n    \
             preco + frete\n\
         }\n\n\
         pub fn without_doc() -> u32 {\n    \
             0\n\
         }\n",
    );
    write(
        &dir,
        "src/pedido.rs",
        "use crate::preco::total;\n\n\
         /// Fecha o pedido.\n\
         pub fn fechar() -> u32 {\n    \
             let parcial = total(10, 2);\n    \
             total(parcial, 1)\n\
         }\n",
    );
    (temp, dir)
}

/// A declaração `name` do arquivo `file`, como o mapa a gravou.
fn declaration<'a>(map: &'a Value, file: &str, name: &str) -> &'a Value {
    let module = map["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["path"] == file)
        .unwrap_or_else(|| panic!("{file} não está no mapa: {map}"));
    module["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("{name} não está em {file}: {module}"))
}

#[test]
fn the_map_keeps_the_comment_and_the_signature_of_each_declaration() {
    let (_temp, dir) = project("doc-and-signature");
    let map = scan(&dir);

    let total = declaration(&map, "src/preco.rs", "total");
    assert_eq!(
        total["doc"], "Soma o preço do pedido com o frete. O frete vem em centavos.",
        "o comentário de documentação inteiro, sem as barras: {total}"
    );
    assert_eq!(
        total["signature"], "pub fn total(preco: u32, frete: u32) -> u32",
        "a assinatura, sem o corpo: {total}"
    );
    // O que já se guardava continua lá.
    assert_eq!(total["kind"], "function", "{total}");
    assert_eq!(total["line"], 4, "{total}");
    assert_eq!(total["end_line"], 6, "{total}");

    // Uma declaração sem comentário em cima traz o campo vazio, e ainda assim
    // a assinatura.
    let without_doc = declaration(&map, "src/preco.rs", "without_doc");
    assert_eq!(without_doc.get("doc").map_or("", |d| d.as_str().unwrap_or("")), "", "{without_doc}");
    assert_eq!(without_doc["signature"], "pub fn without_doc() -> u32", "{without_doc}");

}

#[test]
fn the_map_keeps_each_use_of_each_declaration() {
    let (_temp, dir) = project("uses");
    let map = scan(&dir);

    // Quem é usado: as duas chamadas, cada uma com o arquivo, a linha e a
    // declaração de onde parte.
    let total = declaration(&map, "src/preco.rs", "total");
    let used_by: Vec<&str> = total["used_by"].as_array().unwrap().iter().map(|u| u.as_str().unwrap()).collect();
    assert_eq!(used_by, vec!["src/pedido.rs:5:fechar", "src/pedido.rs:6:fechar"], "cada uso, não a contagem: {total}");

    // Quem chama: o outro lado da mesma ligação.
    let close = declaration(&map, "src/pedido.rs", "fechar");
    assert_eq!(close["calls"], serde_json::json!(["total"]), "{close}");

    // Uma declaração que ninguém usa não ganha ligação nenhuma.
    let without_doc = declaration(&map, "src/preco.rs", "without_doc");
    assert!(without_doc.get("used_by").is_none(), "{without_doc}");
    assert!(without_doc.get("calls").is_none(), "{without_doc}");

    // As contagens do grafo continuam onde estavam — a ligação nomeada é nova,
    // e não substitui o que o grafo já dizia dos arquivos.
    assert!(map["graph"]["nodes"].as_u64().unwrap() >= 3, "{}", map["graph"]);

}

/// Um projeto de mentira com um arquivo por linguagem, cada um com uma
/// declaração documentada atrás do seu enfeite: o atributo, o decorador ou a
/// anotação que a linguagem escreve entre o comentário e a declaração.
fn decorated_project(name: &str) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::Builder::new().prefix(&format!("scan-{name}-")).tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    write(&dir, "Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\n");
    write(&dir, "src/lib.rs", "pub mod item;\n");
    write(
        &dir,
        "src/item.rs",
        "/// Um item do pedido.\n\
         #[derive(Debug)]\n\
         pub struct Item {\n    \
             pub preco: u32,\n\
         }\n",
    );
    write(
        &dir,
        "cs/Pedidos.cs",
        "namespace Loja;\n\n\
         /// <summary>Atende os pedidos.</summary>\n\
         public class Pedidos\n\
         {\n    \
             private readonly int _base;\n\n    \
             /// <summary>Monta o controlador com a base.</summary>\n    \
             public Pedidos(int b)\n    \
             {\n        \
                 _base = b;\n    \
             }\n\n    \
             /// <summary>Busca um pedido pelo número.</summary>\n    \
             [HttpGet(\"{id}\")]\n    \
             public int Buscar(int id)\n    \
             {\n        \
                 return id + _base;\n    \
             }\n\
         }\n",
    );
    write(
        &dir,
        "ts/pedidos.controller.ts",
        "/** Controlador dos pedidos. */\n\
         export class PedidosController {\n  \
             /** Lista os pedidos abertos. */\n  \
             @Get()\n  \
             listar(): number[] {\n    \
                 return [];\n  \
             }\n\
         }\n",
    );
    write(
        &dir,
        "ts/Painel.tsx",
        "/** Painel do carrinho. */\n\
         @Component()\n\
         export class Painel {\n  \
             mostrar() {\n    \
                 return <div />;\n  \
             }\n\
         }\n",
    );
    write(
        &dir,
        "py/app.py",
        "# Responde a página inicial.\n\
         @app.get(\"/\")\n\
         def inicio():\n    \
             return \"ok\"\n",
    );
    write(
        &dir,
        "go/pedido.go",
        "package loja\n\n\
         // Pedido guarda o total da compra.\n\
         type Pedido struct {\n\
         \tTotal int\n\
         }\n\n\
         // Repositorio guarda os pedidos.\n\
         type Repositorio interface {\n\
         \t// Carregar lê um pedido.\n\
         \tCarregar() Pedido\n\
         }\n",
    );
    write(
        &dir,
        "php/Pedidos.php",
        "<?php\n\
         namespace App;\n\n\
         class Pedidos\n\
         {\n    \
             /** Lista os pedidos. */\n    \
             #[Route('/lista')]\n    \
             public function lista(): array\n    \
             {\n        \
                 return [];\n    \
             }\n\
         }\n",
    );
    write(&dir, "dart/pubspec.yaml", "name: loja\n");
    write(
        &dir,
        "dart/lib/pedido.dart",
        "int soma(int a, int b) {\n  \
             return a + b;\n\
         }\n\n\
         class Pedido {\n  \
             final int n;\n\n  \
             /// Monta o pedido com o número.\n  \
             Pedido(this.n);\n\n  \
             /// Soma o total com o número.\n  \
             @override\n  \
             int total() {\n    \
                 final parcial = soma(n, 1);\n    \
                 return parcial;\n  \
             }\n\
         }\n",
    );
    (temp, dir)
}

/// Todas as declarações do mapa, com o arquivo de cada uma.
fn every_declaration(map: &Value) -> Vec<(&str, &Value)> {
    map["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| {
            let file = m["path"].as_str().unwrap();
            m["declarations"].as_array().into_iter().flatten().map(move |d| (file, d))
        })
        .collect()
}

#[test]
fn an_attribute_and_a_decorator_hide_no_comment_and_are_not_calls() {
    let (_temp, dir) = decorated_project("decoration");
    let map = scan(&dir);

    // Cada declaração tem o seu comentário, com o enfeite no meio ou com a
    // declaração embrulhada (o export, o type do Go, o método do Dart).
    let documented = [
        ("src/item.rs", "Item", "Um item do pedido."),
        ("cs/Pedidos.cs", "Pedidos", "Atende os pedidos."),
        ("cs/Pedidos.cs", "Buscar", "Busca um pedido pelo número."),
        ("ts/pedidos.controller.ts", "PedidosController", "Controlador dos pedidos."),
        ("ts/pedidos.controller.ts", "listar", "Lista os pedidos abertos."),
        ("ts/Painel.tsx", "Painel", "Painel do carrinho."),
        ("py/app.py", "inicio", "Responde a página inicial."),
        ("go/pedido.go", "Pedido", "Pedido guarda o total da compra."),
        ("go/pedido.go", "Repositorio", "Repositorio guarda os pedidos."),
        ("go/pedido.go", "Carregar", "Carregar lê um pedido."),
        ("php/Pedidos.php", "lista", "Lista os pedidos."),
        ("dart/lib/pedido.dart", "total", "Soma o total com o número."),
    ];
    for (file, name, doc) in documented {
        let d = declaration(&map, file, name);
        let got = d.get("doc").and_then(Value::as_str).unwrap_or("");
        assert!(got.contains(doc), "{file}:{name} perdeu o comentário {doc:?}: {d}");
    }

    // A def decorada do Python está no mapa, como função.
    assert_eq!(declaration(&map, "py/app.py", "inicio")["kind"], "function");
    // O construtor e o método de interface entram como método.
    assert_eq!(declaration(&map, "go/pedido.go", "Carregar")["kind"], "method");
    let constructors: Vec<&str> = every_declaration(&map)
        .into_iter()
        .filter(|(f, d)| d["name"] == "Pedidos" && *f == "cs/Pedidos.cs" || d["name"] == "Pedido" && *f == "dart/lib/pedido.dart")
        .map(|(_, d)| d["kind"].as_str().unwrap())
        .collect();
    assert_eq!(constructors, vec!["class", "method", "class", "method"], "a classe e o construtor dela");

    // O cabeçalho começa depois do enfeite.
    let decorations = ["#[", "[HttpGet", "@Get", "@Component", "@app", "#[Route", "@override"];
    for (file, d) in every_declaration(&map) {
        let signature = d["signature"].as_str().unwrap_or("");
        assert!(
            !decorations.iter().any(|e| signature.contains(e)),
            "{file}:{} tem o enfeite no cabeçalho: {signature:?}",
            d["name"]
        );
    }
    assert_eq!(declaration(&map, "cs/Pedidos.cs", "Buscar")["signature"], "public int Buscar(int id)");
    assert_eq!(declaration(&map, "php/Pedidos.php", "lista")["signature"], "public function lista(): array");
    assert_eq!(declaration(&map, "dart/lib/pedido.dart", "total")["signature"], "int total()");

    // Nenhuma declaração aparece usando a si mesma.
    for (file, d) in every_declaration(&map) {
        let name = d["name"].as_str().unwrap();
        let own = format!(":{name}");
        for site in d["used_by"].as_array().into_iter().flatten() {
            let site = site.as_str().unwrap();
            assert!(
                !(site.starts_with(&format!("{file}:")) && site.ends_with(&own)),
                "{file}:{name} usa a si mesma em {site}"
            );
        }
    }

    // Nenhuma chamada vem de dentro de um enfeite, nem do cabeçalho de uma
    // declaração.
    let false_calls = ["derive", "HttpGet", "Get", "Component", "Route", "override", "get", "inicio", "Carregar", "Pedidos", "Pedido"];
    for m in map["modules"].as_array().unwrap() {
        for call in m["calls"].as_array().into_iter().flatten() {
            // `q.nome:linha` quando a chamada tem qualificador: vale o nome.
            let name = call.as_str().unwrap().rsplit_once(':').unwrap().0.rsplit('.').next().unwrap();
            assert!(!false_calls.contains(&name), "{} chama {name}, que não é chamada", m["path"]);
        }
    }

    // No Dart o corpo mora ao lado do cabeçalho: o método começa no seu
    // enfeite, termina no seu fecha-chave, e a chamada de dentro dele tem o
    // método como quem usa.
    let total = declaration(&map, "dart/lib/pedido.dart", "total");
    assert_eq!((total["line"].as_u64(), total["end_line"].as_u64()), (Some(12), Some(16)), "{total}");
    let sum = declaration(&map, "dart/lib/pedido.dart", "soma");
    assert_eq!((sum["line"].as_u64(), sum["end_line"].as_u64()), (Some(1), Some(3)), "{sum}");
    assert_eq!(sum["used_by"], serde_json::json!(["dart/lib/pedido.dart:14:total"]), "{sum}");

}

/// Um projeto de mentira com uma declaração de cada jeito que a leitura do
/// cabeçalho, do comentário e da linha precisa tratar, uma linguagem por
/// arquivo.
fn header_project(name: &str) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::Builder::new().prefix(&format!("scan-{name}-")).tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    write(&dir, "Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\n");
    write(&dir, "src/lib.rs", "pub mod caixa;\n");
    write(
        &dir,
        "src/caixa.rs",
        "/// Caixa do pedido.\n\
         #[derive(Debug)]\n\
         pub struct Caixa {\n    \
             pub n: u32,\n\
         }\n\n\
         pub const LIMITE: u32 = 10;\n",
    );
    write(
        &dir,
        "cs/Contas.cs",
        &format!(
            "namespace Loja;\n\n\
             public class Contas\n\
             {{\n    \
                 /// <summary>Soma <paramref name=\"a\"/> com <see cref=\"Base\"/>.</summary>\n    \
                 [HttpGet]\n    \
                 public int Somar({PARAMETERS})\n    \
                 {{\n        \
                     x.from(1);\n        \
                     var (a, b) = Par();\n        \
                     return a + b;\n    \
                 }}\n\
             }}\n"
        ),
    );
    write(
        &dir,
        "ts/precos.ts",
        "export const PRECOS = { a: 1, b: 2 };\n\
         export const soma = (a: number, b: number) => { return a + b; };\n",
    );
    write(&dir, "py/total.py", "def total(a):\n    \"\"\"Soma o pedido.\"\"\"\n    return a\n");
    (temp, dir)
}

/// A lista de parâmetros do método do C#, com mais de 200 caracteres.
const PARAMETERS: &str = "int primeiroValorDaSoma, int segundoValorDaSoma, int terceiroValorDaSoma, \
     int quartoValorDaSoma, int quintoValorDaSoma, int sextoValorDaSoma, int setimoValorDaSoma, \
     int oitavoValorDaSoma, int nonoValorDaSoma";

#[test]
fn the_header_the_comment_and_the_line_come_out_the_same_in_every_language() {
    assert!(PARAMETERS.len() > 200, "a lista precisa passar do corte antigo");
    let (_temp, dir) = header_project("header");
    let map = scan(&dir);

    // O comentário do C# sai sem as marcas, e a marca fechada deixa o valor.
    let add = declaration(&map, "cs/Contas.cs", "Somar");
    assert_eq!(add["doc"], "Soma a com Base.", "{add}");
    // O cabeçalho traz a lista de parâmetros inteira.
    assert_eq!(add["signature"], format!("public int Somar({PARAMETERS})"), "{add}");

    // A linha é a do primeiro enfeite: no C# ele fica dentro do nó, no Rust
    // fica ao lado, e as duas saem iguais.
    assert_eq!(add["line"], 6, "a linha do [HttpGet]: {add}");
    let boxed = declaration(&map, "src/caixa.rs", "Caixa");
    assert_eq!(boxed["line"], 2, "a linha do #[derive(Debug)]: {boxed}");
    assert_eq!(boxed["doc"], "Caixa do pedido.", "{boxed}");

    // O cabeçalho para onde começa o valor, sem o `=` que sobra.
    let prices = declaration(&map, "ts/precos.ts", "PRECOS");
    assert_eq!(prices["signature"], "export const PRECOS", "{prices}");
    let limit = declaration(&map, "src/caixa.rs", "LIMITE");
    assert_eq!(limit["signature"], "pub const LIMITE: u32", "{limit}");
    // A função em seta fica com os parâmetros, e termina no `=>`.
    let sum = declaration(&map, "ts/precos.ts", "soma");
    let sum_signature = sum["signature"].as_str().unwrap();
    assert!(sum_signature.ends_with("(a: number, b: number) =>"), "{sum}");

    // Sem comentário em cima, a docstring do Python é o comentário.
    let total = declaration(&map, "py/total.py", "total");
    assert_eq!(total["doc"], "Soma o pedido.", "{total}");

    // A palavra que a gramática embrulha num nó de nome é chamada; a palavra
    // da linguagem antes de um parêntese não é.
    let accounts = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == "cs/Contas.cs").unwrap();
    let calls: Vec<&str> = accounts["calls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| c.as_str().unwrap().rsplit_once(':').unwrap().0.rsplit('.').next().unwrap())
        .collect();
    assert!(calls.contains(&"from"), "from é chamada: {calls:?}");
    assert!(!calls.contains(&"var"), "var não é chamada: {calls:?}");

}

/// O arquivo de cada linguagem sem palavra de visibilidade, com declarações
/// que a regra dela deixa ver fora e outras que ela guarda.
const VISIBILITY_FILES: [(&str, &str); 4] = [
    (
        "calc/calc.go",
        "package calc\n\nconst Limit = 10\nconst limit = 3\n\ntype Box struct {\n\tTotal int\n\tcount int\n}\n\n\
         type box struct{}\n\ntype Reader interface {\n\tRead() int\n\treset()\n}\n\ntype Alias = Box\n\n\
         func Sum() int {\n\ttype Local struct{}\n\treturn 1\n}\n\nfunc sum() int { return 2 }\n\n\
         func (b *Box) Add() {}\n\nfunc (b *Box) add() {}\n",
    ),
    (
        "shop/calc.py",
        "LIMIT = 10\n_limit = 3\n\n\nclass Box:\n    total = 0\n    _count = 0\n\n    def __init__(self):\n        pass\n\n\
         \x20   def add(self):\n        pass\n\n    def _reset(self):\n        pass\n\n    @property\n    def size(self):\n        return 1\n\n\n\
         class _Hidden:\n    pass\n\n\n@decorate\nclass Point:\n    x: int = 0\n\n\n\
         def run():\n    def inner():\n        pass\n    return inner\n\n\ndef _helper():\n    pass\n",
    ),
    (
        "lib/calc.dart",
        "const limit = 10;\nconst _limit = 3;\n\nint run() => 1;\nint _helper() => 2;\n\n\
         class Box {\n  int total = 0;\n  int _count = 0;\n  static const max = 5;\n  Box();\n  Box.empty();\n  Box._internal();\n\
         \x20 int add() => 1;\n  int _reset() => 0;\n  int get size => 1;\n}\n\nclass _Hidden {}\n\n\
         enum Color { red, green }\n\nmixin Walker {}\n\nextension Words on String {}\n",
    ),
    (
        "src/Calc.php",
        "<?php\nnamespace Shop;\n\nconst LIMIT = 10;\n\nfunction run(): int { return 1; }\n\n\
         interface Reader { public function read(): int; }\n\ntrait Counts { private function tick(): void {} }\n\n\
         enum Color { case Red; }\n\nclass Calc {\n    public const MAX = 5;\n    private const MIN = 1;\n\
         \x20   public int $total = 0;\n    protected int $count = 0;\n    var $legacy;\n\
         \x20   public function add(): int { return 1; }\n    function plain(): int { return 2; }\n\
         \x20   private function reset(): void {}\n    PROTECTED function guard(): void {}\n\
         \x20   public private(set) int $size = 0;\n}\n",
    ),
];

/// Na linguagem que não escreve a visibilidade numa palavra, o mapa marca a
/// declaração que ela deixa ver fora do arquivo ou do pacote pela regra
/// dela: no Go, o nome com maiúscula, e o tipo e a constante só no topo; no
/// Python, o nome sem sublinhado na frente (ou entre sublinhados duplos), no
/// topo do módulo ou no corpo de uma classe; no Dart, o nome sem sublinhado
/// na frente; no PHP, tudo o que é global e o membro que não é `private` nem
/// `protected`.
#[test]
fn the_map_marks_what_each_language_lets_outside_see_by_its_own_rule() {
    let temp = tempfile::Builder::new().prefix("scan-visible-").tempdir().unwrap();
    let dir = temp.path();
    for (file, body) in VISIBILITY_FILES {
        write(dir, file, body);
    }
    let map = scan(dir);
    let marks = |file: &str| {
        let mut seen: [Vec<String>; 2] = Default::default();
        for (_, decl) in every_declaration(&map).into_iter().filter(|(at, _)| *at == file) {
            let exported = decl["syntax"]["exported"] == true;
            seen[usize::from(exported)].push(format!("{} {}", decl["kind"].as_str().unwrap(), decl["name"].as_str().unwrap()));
        }
        seen.iter_mut().for_each(|names| names.sort());
        let [hidden, shown] = seen;
        (shown, hidden)
    };
    let names = |list: &[&str]| -> Vec<String> {
        let mut out: Vec<String> = list.iter().map(ToString::to_string).collect();
        out.sort();
        out
    };
    assert_eq!(
        marks("calc/calc.go"),
        (
            names(&["const Limit", "struct Box", "field Total", "interface Reader", "method Read", "type Alias", "function Sum", "method Add"]),
            names(&["const limit", "field count", "struct box", "method reset", "struct Local", "function sum", "method add"]),
        )
    );
    assert_eq!(
        marks("shop/calc.py"),
        (
            names(&["const LIMIT", "class Box", "field total", "method __init__", "method add", "method size", "class Point", "field x", "function run"]),
            names(&["const _limit", "field _count", "method _reset", "class _Hidden", "function inner", "function _helper"]),
        )
    );
    assert_eq!(
        marks("lib/calc.dart"),
        (
            names(&[
                "const limit",
                "function run",
                "class Box",
                "field total",
                "const max",
                "method Box",
                "method empty",
                "method add",
                "property size",
                "enum Color",
                "enum_member red",
                "enum_member green",
                "mixin Walker",
                "extension Words",
            ]),
            names(&["const _limit", "function _helper", "field _count", "method _internal", "method _reset", "class _Hidden"]),
        )
    );
    assert_eq!(
        marks("src/Calc.php"),
        (
            names(&[
                "const LIMIT",
                "function run",
                "interface Reader",
                "method read",
                "trait Counts",
                "enum Color",
                "enum_member Red",
                "class Calc",
                "const MAX",
                "property total",
                "property legacy",
                "method add",
                "method plain",
                "property size",
            ]),
            names(&["method tick", "const MIN", "property count", "method reset", "method guard"]),
        )
    );
}
