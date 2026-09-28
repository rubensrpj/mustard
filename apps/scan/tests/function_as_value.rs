//! A função entregue como valor, sem ser chamada ali, é usada por quem a
//! entrega: passada a outra função (`xs.map(dobro)`), guardada num nome
//! (`let f = quadrado;`), ligada a um evento (`Salvo += Avisar;`) ou a um
//! atributo da tela (`onClick={limpar}`). Em cada linguagem, o projeto é lido
//! pelo scan de verdade e o mapa diz quem usa cada função, com o arquivo, a
//! linha e a declaração de onde veio o uso.
//!
//! O nome escrito onde vai um valor só liga ao que o arquivo enxerga: a função
//! de um arquivo que ele não alcança não ganha uso, e o campo lido, com o
//! mesmo nome de um método, não é uso do método.

#[path = "support/model.rs"]
mod model;

use serde_json::Value;

/// Escreve os arquivos num projeto novo e devolve o mapa que o scan grava.
fn scanned(label: &str, files: &[(&str, &str)]) -> Value {
    let temp = tempfile::Builder::new().prefix(&format!("scan-valor-{label}-")).tempdir().unwrap();
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

/// A declaração `name` do arquivo `path` do mapa.
fn declaration<'a>(map: &'a Value, path: &str, name: &str) -> &'a Value {
    let module = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == path).unwrap_or_else(|| panic!("{path} no mapa"));
    let decls = module["declarations"].as_array().unwrap();
    decls.iter().find(|d| d["name"] == name).unwrap_or_else(|| panic!("{name} em {path}: {decls:?}"))
}

/// As declarações `name` do arquivo `path` do mapa, na ordem do arquivo.
fn declarations<'a>(map: &'a Value, path: &str, name: &str) -> Vec<&'a Value> {
    let module = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == path).unwrap_or_else(|| panic!("{path} no mapa"));
    module["declarations"].as_array().unwrap().iter().filter(|d| d["name"] == name).collect()
}

/// Os usos provados da declaração `name` do arquivo `path`, como
/// `arquivo:linha:de onde`.
fn proven_uses(map: &Value, path: &str, name: &str) -> Vec<String> {
    let used = declaration(map, path, name)["used_by"].as_array().cloned().unwrap_or_default();
    used.iter().filter_map(|u| u.as_str().map(str::to_string)).collect()
}

/// Todos os usos da declaração, provados ou suspeitos, só pelo lugar.
fn every_use(map: &Value, path: &str, name: &str) -> Vec<String> {
    let used = declaration(map, path, name)["used_by"].as_array().cloned().unwrap_or_default();
    used.iter().filter_map(|u| u.as_str().or_else(|| u["at"].as_str()).map(str::to_string)).collect()
}

const RUST_MAIN: &str = "mod a;\nmod calc;\nuse crate::calc::dobro;\n\n\
struct Pedido {\n    total: u32,\n}\n\n\
impl Pedido {\n    fn metade(x: u32) -> u32 {\n        x / 2\n    }\n\n    \
fn total(&self) -> u32 {\n        self.total\n    }\n\n    \
fn metades(&self, xs: &[u32]) -> Vec<u32> {\n        mostrar(self.total);\n        \
xs.iter().copied().map(Self::metade).collect()\n    }\n}\n\n\
fn quadrado(x: u32) -> u32 {\n    x * x\n}\n\n\
fn mostrar(x: u32) {\n    let _ = x;\n}\n\n\
fn main() {\n    let f = quadrado;\n    let xs = [1u32, 2];\n    \
let _: Vec<u32> = xs.iter().copied().map(dobro).collect();\n    \
let _: Vec<u32> = xs.iter().copied().map(calc::somar_um).collect();\n    \
let _: Vec<u32> = xs.iter().copied().map(crate::a::triplo).collect();\n    \
let _: Vec<u32> = xs.iter().copied().map(solto).collect();\n    mostrar(f(3));\n}\n";

/// No Rust, a função passada a `.map(` pelo nome trazido por `use`, pelo
/// módulo, pelo caminho completo e pelo tipo (`Self::metade`), e a guardada
/// num `let`, são usadas pela função que as entrega. O campo `self.total`
/// passado como argumento não é uso do método `total`, e a função de um
/// arquivo que nenhum `mod` alcança não ganha uso.
#[test]
fn a_rust_function_handed_as_a_value_is_used_by_who_hands_it() {
    let map = scanned(
        "rust",
        &[
            ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            ("src/calc.rs", "pub fn dobro(x: u32) -> u32 {\n    x * 2\n}\n\npub fn somar_um(x: u32) -> u32 {\n    x + 1\n}\n"),
            ("src/a.rs", "pub fn triplo(x: u32) -> u32 {\n    x * 3\n}\n"),
            ("src/solto.rs", "pub fn solto(x: u32) -> u32 {\n    x\n}\n"),
            ("src/main.rs", RUST_MAIN),
        ],
    );
    assert_eq!(proven_uses(&map, "src/main.rs", "quadrado"), ["src/main.rs:33:main"]);
    assert_eq!(proven_uses(&map, "src/calc.rs", "dobro"), ["src/main.rs:35:main"]);
    assert_eq!(proven_uses(&map, "src/calc.rs", "somar_um"), ["src/main.rs:36:main"]);
    assert_eq!(proven_uses(&map, "src/a.rs", "triplo"), ["src/main.rs:37:main"]);
    assert_eq!(proven_uses(&map, "src/main.rs", "metade"), ["src/main.rs:20:metades"]);
    let method = declarations(&map, "src/main.rs", "total").into_iter().find(|d| d["kind"] != "field").unwrap();
    assert_eq!(method["used_by"].as_array().cloned().unwrap_or_default(), Vec::<Value>::new(), "the field read is not a use of the method");
    assert_eq!(every_use(&map, "src/solto.rs", "solto"), Vec::<String>::new(), "a file out of sight does not link");
    let calls = declaration(&map, "src/main.rs", "main")["calls"].clone();
    for name in ["quadrado", "dobro", "somar_um", "triplo"] {
        assert!(calls.as_array().unwrap().iter().any(|c| c == name), "{name} in {calls}");
    }
}

/// No TypeScript, a função passada a `.map(` e guardada numa `const` é usada
/// pela função que a entrega, o método entregue por `this.salvar` é usado
/// pelo método que o liga, e no TSX a função dada a `onClick={limpar}` é
/// usada pelo componente. No JavaScript, a função passada a `.map(` também.
#[test]
fn a_typescript_function_handed_as_a_value_is_used_by_who_hands_it() {
    let map = scanned(
        "typescript",
        &[
            ("src/calc.ts", "export function dobro(x: number) {\n  return x * 2;\n}\n\nexport function limpar() {\n  return 0;\n}\n"),
            (
                "src/tela.ts",
                "import { dobro } from './calc';\n\nexport class Tela {\n  salvar() {\n    return 1;\n  }\n\n  \
                 ligar(botao: { onClick: () => number }) {\n    botao.onClick = this.salvar;\n  }\n}\n\n\
                 export function lista(xs: number[]) {\n  const f = dobro;\n  return xs.map(dobro).concat([f(1)]);\n}\n",
            ),
            (
                "src/botao.tsx",
                "import { limpar } from './calc';\n\nexport function Botao() {\n  return <button onClick={limpar}>ok</button>;\n}\n",
            ),
            ("src/velho.js", "import { dobro } from './calc';\n\nexport function antigos(xs) {\n  return xs.map(dobro);\n}\n"),
        ],
    );
    assert_eq!(proven_uses(&map, "src/calc.ts", "dobro"), ["src/tela.ts:14:lista", "src/tela.ts:15:lista", "src/velho.js:4:antigos"]);
    assert_eq!(proven_uses(&map, "src/tela.ts", "salvar"), ["src/tela.ts:9:ligar"]);
    assert_eq!(proven_uses(&map, "src/calc.ts", "limpar"), ["src/botao.tsx:4:Botao"]);
}

const CSHARP_ORDER: &str = "using System;\nusing System.Linq;\nusing System.Collections.Generic;\n\nnamespace Loja;\n\n\
public class Pedido\n{\n    public event Action? Salvo;\n    public int Total { get; set; }\n\n    \
public int Metade(int x) => x / 2;\n\n    \
public void Ligar(List<int> xs, Pedido outro)\n    {\n        \
var metades = xs.Select(Metade).ToList();\n        \
var dobros = xs.Select(Calc.Dobro).ToList();\n        \
Func<int, int> f = Metade;\n        \
Salvo += Avisar;\n        \
Usar(outro.Total);\n        \
Usar(Outro().Total);\n    }\n\n    \
private void Avisar() { }\n\n    \
private Pedido Outro() => this;\n\n    \
private void Usar(int x) { }\n}\n";

/// No C#, o grupo de método passado a `Select(`, pelo nome e pelo tipo
/// (`Calc.Dobro`), o guardado numa variável e o ligado a um evento com `+=`
/// são usados pelo método que os entrega. A propriedade lida de uma variável
/// (`outro.Total`) ou do que um método devolve (`Outro().Total`) não é uso do
/// método `Total` de outra classe.
#[test]
fn a_csharp_method_group_is_used_by_who_hands_it() {
    let map = scanned(
        "csharp",
        &[
            ("Loja/Calc.cs", "namespace Loja;\n\npublic static class Calc\n{\n    public static int Dobro(int x) => x * 2;\n}\n"),
            ("Loja/Relatorio.cs", "namespace Loja;\n\npublic class Relatorio\n{\n    public int Total() => 3;\n}\n"),
            ("Loja/Pedido.cs", CSHARP_ORDER),
        ],
    );
    assert_eq!(proven_uses(&map, "Loja/Pedido.cs", "Metade"), ["Loja/Pedido.cs:16:Ligar", "Loja/Pedido.cs:18:Ligar"]);
    assert_eq!(proven_uses(&map, "Loja/Calc.cs", "Dobro"), ["Loja/Pedido.cs:17:Ligar"]);
    assert_eq!(proven_uses(&map, "Loja/Pedido.cs", "Avisar"), ["Loja/Pedido.cs:19:Ligar"]);
    assert_eq!(every_use(&map, "Loja/Relatorio.cs", "Total"), Vec::<String>::new(), "a property read from a value");
}

/// Em Python, Go e Dart, a função passada como argumento é usada pela função
/// que a passa. No PHP, o nome escrito sozinho é uma constante, e a função
/// entregue como valor se escreve `dobro(...)`: também é usada por quem a
/// passa.
#[test]
fn a_function_handed_as_an_argument_is_used_in_python_go_dart_and_php() {
    let python = scanned(
        "python",
        &[
            ("loja/calc.py", "def dobro(x):\n    return x * 2\n"),
            ("usa.py", "from loja.calc import dobro\n\n\ndef usar(xs):\n    return list(map(dobro, xs))\n"),
        ],
    );
    assert_eq!(proven_uses(&python, "loja/calc.py", "dobro"), ["usa.py:5:usar"]);
    let go = scanned(
        "go",
        &[
            ("go.mod", "module exemplo.com/loja\n\ngo 1.21\n"),
            ("calc/calc.go", "package calc\n\nfunc Dobro(x int) int {\n\treturn x * 2\n}\n"),
            (
                "main.go",
                "package main\n\nimport \"exemplo.com/loja/calc\"\n\nfunc aplicar(f func(int) int) int {\n\treturn f(1)\n}\n\n\
                 func main() {\n\taplicar(calc.Dobro)\n}\n",
            ),
        ],
    );
    assert_eq!(proven_uses(&go, "calc/calc.go", "Dobro"), ["main.go:10:main"]);
    let dart = scanned(
        "dart",
        &[
            ("pubspec.yaml", "name: loja\n"),
            ("lib/calc.dart", "int dobro(int x) => x * 2;\n"),
            ("lib/main.dart", "import 'calc.dart';\n\nList<int> usar(List<int> xs) {\n  return xs.map(dobro).toList();\n}\n"),
        ],
    );
    assert_eq!(proven_uses(&dart, "lib/calc.dart", "dobro"), ["lib/main.dart:4:usar"]);
    let php = scanned(
        "php",
        &[
            ("composer.json", "{\"name\": \"loja/loja\", \"autoload\": {\"psr-4\": {\"Loja\\\\\": \"src/\"}}}\n"),
            ("src/calc.php", "<?php\nnamespace Loja;\n\nfunction dobro(int $x): int {\n    return $x * 2;\n}\n"),
            ("src/usa.php", "<?php\nnamespace Loja;\n\nfunction usar(array $xs): array {\n    return array_map(dobro(...), $xs);\n}\n"),
        ],
    );
    assert_eq!(proven_uses(&php, "src/calc.php", "dobro"), ["src/usa.php:5:usar"]);
}
