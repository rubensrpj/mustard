//! A propriedade ou o campo escrito depois do objeto, sem chamada ali
//! (`pedido.Total`, `this.total`, `$this->total`), é usado por quem o lê ou o
//! escreve, em toda linguagem que o scan lê. Em cada uma, o projeto é lido
//! pelo scan de verdade e o mapa diz quem usa cada membro, com o arquivo, a
//! linha e a declaração de onde veio o uso.
//!
//! O membro liga pelo mesmo caminho da chamada de método escrita depois do
//! mesmo objeto: depois do próprio objeto, ao membro do tipo em que está
//! escrito, provado; depois de um nome que não estreita, ao que o arquivo tem
//! à vista, suspeito; depois de um valor (`Criar().Troco`, `pedido.troco` na
//! língua cujo separador de membro só liga valor), a nada. O membro de um
//! arquivo que o leitor não enxerga não ganha uso, nem o membro de mesmo nome
//! lido depois de um nome da biblioteca (`DateTime.Now`). O membro lido não
//! entra nas chamadas de quem o lê.

#[path = "support/model.rs"]
mod model;

use serde_json::Value;

/// Escreve os arquivos num projeto novo e devolve o mapa que o scan grava.
fn scanned(label: &str, files: &[(&str, &str)]) -> Value {
    let temp = tempfile::Builder::new().prefix(&format!("scan-membro-{label}-")).tempdir().unwrap();
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

/// Os usos provados da declaração `name` do arquivo `path`, como
/// `arquivo:linha:de onde`.
fn proven_uses(map: &Value, path: &str, name: &str) -> Vec<String> {
    let used = declaration(map, path, name)["used_by"].as_array().cloned().unwrap_or_default();
    used.iter().filter_map(|u| u.as_str().map(str::to_string)).collect()
}

/// Os usos suspeitos da declaração, só pelo lugar.
fn suspect_uses(map: &Value, path: &str, name: &str) -> Vec<String> {
    let used = declaration(map, path, name)["used_by"].as_array().cloned().unwrap_or_default();
    used.iter().filter_map(|u| u["at"].as_str().map(str::to_string)).collect()
}

/// Todos os usos da declaração, provados ou suspeitos, só pelo lugar.
fn every_use(map: &Value, path: &str, name: &str) -> Vec<String> {
    let used = declaration(map, path, name)["used_by"].as_array().cloned().unwrap_or_default();
    used.iter().filter_map(|u| u.as_str().or_else(|| u["at"].as_str()).map(str::to_string)).collect()
}

/// Os nomes que a declaração chama.
fn calls(map: &Value, path: &str, name: &str) -> Vec<String> {
    let calls = declaration(map, path, name)["calls"].as_array().cloned().unwrap_or_default();
    calls.iter().filter_map(|c| c.as_str().map(str::to_string)).collect()
}

const CSHARP_ORDER: &str = "namespace Loja;\n\npublic class Pedido\n{\n    public int Total { get; set; }\n    \
public int Desconto;\n    public int Troco { get; set; }\n    public int Now { get; set; }\n\n    \
public int Dobro() => this.Total * 2;\n}\n";

const CSHARP_REGISTER: &str = "using System;\n\nnamespace Loja;\n\npublic class Caixa\n{\n    \
public int Fechar(Pedido pedido)\n    {\n        var hora = DateTime.Now;\n        \
return pedido.Desconto + Criar().Troco + pedido.Total;\n    }\n\n    \
private Pedido Criar() => new Pedido();\n}\n";

/// No C#, a propriedade lida pelo próprio objeto (`this.Total`) é usada,
/// provada, pelo método que a lê; o campo e a propriedade lidos pela
/// variável (`pedido.Desconto`, `pedido.Total`) são usados, suspeitos, pelo
/// método que os lê. A propriedade lida do que um método devolve
/// (`Criar().Troco`) não ganha uso, nem a de mesmo nome de uma biblioteca
/// (`DateTime.Now`), nem o campo de mesmo nome de um namespace que o arquivo
/// não enxerga. O membro lido não entra nas chamadas de quem o lê.
#[test]
fn a_csharp_property_or_field_read_after_the_object_is_a_use_of_it() {
    let map = scanned(
        "csharp",
        &[
            ("Loja/Pedido.cs", CSHARP_ORDER),
            ("Loja/Caixa.cs", CSHARP_REGISTER),
            ("Fora/Nota.cs", "namespace Fora;\n\npublic class Nota\n{\n    public int Desconto;\n}\n"),
        ],
    );
    assert_eq!(proven_uses(&map, "Loja/Pedido.cs", "Total"), ["Loja/Pedido.cs:10:Dobro"]);
    assert_eq!(suspect_uses(&map, "Loja/Pedido.cs", "Total"), ["Loja/Caixa.cs:10:Fechar"]);
    assert_eq!(suspect_uses(&map, "Loja/Pedido.cs", "Desconto"), ["Loja/Caixa.cs:10:Fechar"]);
    assert_eq!(every_use(&map, "Loja/Pedido.cs", "Troco"), Vec::<String>::new(), "a property read from a value");
    assert_eq!(every_use(&map, "Loja/Pedido.cs", "Now"), Vec::<String>::new(), "a property read from the library");
    assert_eq!(every_use(&map, "Fora/Nota.cs", "Desconto"), Vec::<String>::new(), "a namespace out of sight");
    let fechar = calls(&map, "Loja/Caixa.cs", "Fechar");
    assert!(!fechar.iter().any(|c| c == "Desconto" || c == "Total"), "a member read is not a call: {fechar:?}");
}

/// No TypeScript e no JavaScript, o campo lido pelo próprio objeto
/// (`this.total`) é usado, provado, pelo método que o lê, e o lido pela
/// variável (`pedido.desconto`), suspeito, também no meio da marcação de uma
/// tela (`<h1>{capa.titulo}</h1>`). O campo lido do que uma função devolve
/// (`criar().troco`) não ganha uso, nem o de mesmo nome de uma biblioteca
/// (`Math.PI`), nem o de mesmo nome de um arquivo que ninguém importa.
#[test]
fn a_typescript_or_javascript_field_read_after_the_object_is_a_use_of_it() {
    let map = scanned(
        "typescript",
        &[
            (
                "src/pedido.ts",
                "export class Pedido {\n  total = 0;\n  desconto = 0;\n  troco = 0;\n  PI = 3;\n\n  \
                 dobro() {\n    return this.total * 2;\n  }\n}\n",
            ),
            (
                "src/caixa.ts",
                "import { Pedido } from './pedido';\n\nfunction criar() {\n  return new Pedido();\n}\n\n\
                 export function fechar(pedido: Pedido) {\n  return pedido.desconto + criar().troco + Math.PI;\n}\n",
            ),
            ("src/nota.ts", "export class Nota {\n  desconto = 1;\n}\n"),
            ("src/conta.js", "export class Conta {\n  saldo = 0;\n\n  ler() {\n    return this.saldo;\n  }\n}\n"),
            ("src/velho.js", "import { Pedido } from './pedido';\n\nexport function antigo(pedido) {\n  return pedido.desconto;\n}\n"),
            ("src/capa.ts", "export class Capa {\n  titulo = '';\n}\n"),
            (
                "src/tela.tsx",
                "import { Capa } from './capa';\n\nexport function Tela(capa: Capa) {\n  return <h1>{capa.titulo}</h1>;\n}\n",
            ),
        ],
    );
    assert_eq!(proven_uses(&map, "src/pedido.ts", "total"), ["src/pedido.ts:8:dobro"]);
    assert_eq!(suspect_uses(&map, "src/pedido.ts", "desconto"), ["src/caixa.ts:8:fechar", "src/velho.js:4:antigo"]);
    assert_eq!(every_use(&map, "src/pedido.ts", "troco"), Vec::<String>::new(), "a field read from a value");
    assert_eq!(every_use(&map, "src/pedido.ts", "PI"), Vec::<String>::new(), "a field read from the library");
    assert_eq!(every_use(&map, "src/nota.ts", "desconto"), Vec::<String>::new(), "a file nobody imports");
    assert_eq!(proven_uses(&map, "src/conta.js", "saldo"), ["src/conta.js:5:ler"]);
    assert_eq!(suspect_uses(&map, "src/capa.ts", "titulo"), ["src/tela.tsx:4:Tela"], "a field read inside the markup of a screen");
}

/// No Python, o atributo da classe lido pelo próprio objeto (`self.total`) é
/// usado, provado, pelo método que o lê, e o lido pela variável
/// (`pedido.desconto`), suspeito. O lido do que uma função devolve
/// (`criar().troco`) não ganha uso, nem o de mesmo nome de um módulo que o
/// arquivo não importa.
#[test]
fn a_python_attribute_read_after_the_object_is_a_use_of_it() {
    let map = scanned(
        "python",
        &[
            (
                "loja/pedido.py",
                "class Pedido:\n    total = 0\n    desconto = 0\n    troco = 0\n\n    def dobro(self):\n        return self.total * 2\n",
            ),
            (
                "loja/caixa.py",
                "from loja.pedido import Pedido\n\n\ndef criar():\n    return Pedido()\n\n\n\
                 def fechar(pedido):\n    return pedido.desconto + criar().troco\n",
            ),
            ("fora/nota.py", "class Nota:\n    desconto = 1\n"),
        ],
    );
    assert_eq!(proven_uses(&map, "loja/pedido.py", "total"), ["loja/pedido.py:7:dobro"]);
    assert_eq!(suspect_uses(&map, "loja/pedido.py", "desconto"), ["loja/caixa.py:9:fechar"]);
    assert_eq!(every_use(&map, "loja/pedido.py", "troco"), Vec::<String>::new(), "an attribute read from a value");
    assert_eq!(every_use(&map, "fora/nota.py", "desconto"), Vec::<String>::new(), "a module out of sight");
}

/// No Rust, o campo lido pelo próprio objeto (`self.total`) é usado, provado,
/// pelo método que o lê. O campo lido de uma variável (`pedido.troco`) não ganha
/// uso: o ponto só liga valor, e sem o tipo dele nada diz de quem é o campo.
/// O campo de mesmo nome de um arquivo que nenhum `mod` alcança também não.
#[test]
fn a_rust_field_read_through_self_is_a_use_of_it() {
    let map = scanned(
        "rust",
        &[
            ("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            (
                "src/main.rs",
                "struct Pedido {\n    total: u32,\n    troco: u32,\n}\n\n\
                 impl Pedido {\n    fn dobro(&self) -> u32 {\n        self.total * 2\n    }\n}\n\n\
                 fn fechar(pedido: &Pedido) -> u32 {\n    pedido.troco\n}\n\n\
                 fn main() {\n    let p = Pedido { total: 1, troco: 2 };\n    let _ = fechar(&p) + p.dobro();\n}\n",
            ),
            ("src/solto.rs", "pub struct Nota {\n    pub total: u32,\n}\n"),
        ],
    );
    assert_eq!(proven_uses(&map, "src/main.rs", "total"), ["src/main.rs:8:dobro"]);
    assert_eq!(every_use(&map, "src/main.rs", "troco"), Vec::<String>::new(), "a field read from a value");
    assert_eq!(every_use(&map, "src/solto.rs", "total"), Vec::<String>::new(), "a file out of sight");
}

/// No Go, o campo lido pelo receptor do método (`pedido.Total`) é usado,
/// suspeito, pelo método que o lê. O campo lido do que uma função devolve
/// (`criar().Troco`) não ganha uso, nem o de mesmo nome de outro pacote.
#[test]
fn a_go_field_read_after_the_object_is_a_use_of_it() {
    let map = scanned(
        "go",
        &[
            ("go.mod", "module exemplo.com/loja\n\ngo 1.21\n"),
            (
                "loja/pedido.go",
                "package loja\n\ntype Pedido struct {\n\tTotal int\n\tTroco int\n}\n\n\
                 func (pedido *Pedido) Dobro() int {\n\treturn pedido.Total * 2\n}\n",
            ),
            ("loja/caixa.go", "package loja\n\nfunc criar() *Pedido {\n\treturn &Pedido{}\n}\n\nfunc Fechar() int {\n\treturn criar().Troco\n}\n"),
            ("fora/nota.go", "package fora\n\ntype Nota struct {\n\tTotal int\n}\n"),
        ],
    );
    assert_eq!(suspect_uses(&map, "loja/pedido.go", "Total"), ["loja/pedido.go:9:Dobro"]);
    assert_eq!(every_use(&map, "loja/pedido.go", "Troco"), Vec::<String>::new(), "a field read from a value");
    assert_eq!(every_use(&map, "fora/nota.go", "Total"), Vec::<String>::new(), "a package out of sight");
}

/// No PHP, a propriedade lida pelo próprio objeto (`$this->total`) é usada,
/// provada, pelo método que a lê. A lida de uma variável (`$pedido->troco`) não
/// ganha uso: a seta só liga valor.
#[test]
fn a_php_property_read_through_this_is_a_use_of_it() {
    let map = scanned(
        "php",
        &[
            ("composer.json", "{\"name\": \"loja/loja\", \"autoload\": {\"psr-4\": {\"Loja\\\\\": \"src/\"}}}\n"),
            (
                "src/Pedido.php",
                "<?php\nnamespace Loja;\n\nclass Pedido {\n    public $total = 0;\n    public $troco = 0;\n\n    \
                 public function dobro() {\n        return $this->total * 2;\n    }\n}\n",
            ),
            ("src/caixa.php", "<?php\nnamespace Loja;\n\nfunction fechar(Pedido $pedido) {\n    return $pedido->troco;\n}\n"),
        ],
    );
    assert_eq!(proven_uses(&map, "src/Pedido.php", "total"), ["src/Pedido.php:9:dobro"]);
    assert_eq!(every_use(&map, "src/Pedido.php", "troco"), Vec::<String>::new(), "a property read from a value");
}

/// No Dart, o campo lido pelo próprio objeto (`this.total`) é usado, provado,
/// pelo método que o lê, e o lido pela variável (`pedido.desconto`),
/// suspeito. O lido do que uma função devolve (`criar().troco`) não ganha
/// uso, nem o de mesmo nome de um arquivo que ninguém importa.
#[test]
fn a_dart_field_read_after_the_object_is_a_use_of_it() {
    let map = scanned(
        "dart",
        &[
            ("pubspec.yaml", "name: loja\n"),
            (
                "lib/pedido.dart",
                "class Pedido {\n  int total = 0;\n  int desconto = 0;\n  int troco = 0;\n\n  int dobro() => this.total * 2;\n}\n",
            ),
            (
                "lib/caixa.dart",
                "import 'pedido.dart';\n\nPedido criar() => Pedido();\n\nint fechar(Pedido pedido) => pedido.desconto + criar().troco;\n",
            ),
            ("lib/nota.dart", "class Nota {\n  int desconto = 1;\n}\n"),
        ],
    );
    assert_eq!(proven_uses(&map, "lib/pedido.dart", "total"), ["lib/pedido.dart:6:dobro"]);
    assert_eq!(suspect_uses(&map, "lib/pedido.dart", "desconto"), ["lib/caixa.dart:5:fechar"]);
    assert_eq!(every_use(&map, "lib/pedido.dart", "troco"), Vec::<String>::new(), "a field read from a value");
    assert_eq!(every_use(&map, "lib/nota.dart", "desconto"), Vec::<String>::new(), "a file nobody imports");
}

/// O acesso opcional (`pedido?.Total`) lê o membro do mesmo objeto que o
/// acesso comum: a propriedade lida e o método chamado depois dele ganham o
/// mesmo uso, suspeito, que ganham depois de `pedido.`, no C#, no TypeScript,
/// no JavaScript e no Dart.
#[test]
fn a_member_read_or_a_call_after_optional_access_is_a_use_like_after_the_plain_one() {
    let csharp = scanned(
        "opcional-csharp",
        &[
            ("Loja/Pedido.cs", "namespace Loja;\n\npublic class Pedido\n{\n    public int Total { get; set; }\n    public int Calcular() => 1;\n}\n"),
            (
                "Loja/Caixa.cs",
                "namespace Loja;\n\npublic class Caixa\n{\n    public int? Fechar(Pedido? pedido)\n    {\n        \
                 return pedido?.Total + pedido?.Calcular();\n    }\n}\n",
            ),
        ],
    );
    assert_eq!(suspect_uses(&csharp, "Loja/Pedido.cs", "Total"), ["Loja/Caixa.cs:7:Fechar"]);
    assert_eq!(suspect_uses(&csharp, "Loja/Pedido.cs", "Calcular"), ["Loja/Caixa.cs:7:Fechar"]);

    let typescript = scanned(
        "opcional-typescript",
        &[
            ("src/pedido.ts", "export class Pedido {\n  total = 0;\n\n  calcular() {\n    return 1;\n  }\n}\n"),
            (
                "src/caixa.ts",
                "import { Pedido } from './pedido';\n\nexport function fechar(pedido?: Pedido) {\n  \
                 return (pedido?.total ?? 0) + (pedido?.calcular() ?? 0);\n}\n",
            ),
            ("src/velho.js", "import { Pedido } from './pedido';\n\nexport function antigo(pedido) {\n  return pedido?.total;\n}\n"),
        ],
    );
    assert_eq!(suspect_uses(&typescript, "src/pedido.ts", "total"), ["src/caixa.ts:4:fechar", "src/velho.js:4:antigo"]);
    assert_eq!(suspect_uses(&typescript, "src/pedido.ts", "calcular"), ["src/caixa.ts:4:fechar"]);

    let dart = scanned(
        "opcional-dart",
        &[
            ("pubspec.yaml", "name: loja\n"),
            ("lib/pedido.dart", "class Pedido {\n  int total = 0;\n\n  int calcular() => 1;\n}\n"),
            (
                "lib/caixa.dart",
                "import 'pedido.dart';\n\nint? fechar(Pedido? pedido) => pedido?.total;\n\nint? somar(Pedido? pedido) => pedido?.calcular();\n",
            ),
        ],
    );
    assert_eq!(suspect_uses(&dart, "lib/pedido.dart", "total"), ["lib/caixa.dart:3:fechar"]);
    assert_eq!(suspect_uses(&dart, "lib/pedido.dart", "calcular"), ["lib/caixa.dart:5:somar"]);
}

/// O nome de uma letra antes do separador é um nome como o de duas: o
/// receptor do método no Go (`func (p *Pedido)`) e o parâmetro da função
/// anônima no C# (`p => p.Total`) leem o campo e a propriedade, que ganham
/// uso suspeito, como depois de `pedido.`.
#[test]
fn a_one_letter_name_before_the_separator_reads_the_member_like_a_longer_one() {
    let go = scanned(
        "uma-letra-go",
        &[
            ("go.mod", "module exemplo.com/loja\n\ngo 1.21\n"),
            (
                "loja/pedido.go",
                "package loja\n\ntype Pedido struct {\n\tTotal int\n}\n\nfunc (p *Pedido) Dobro() int {\n\treturn p.Total * 2\n}\n",
            ),
        ],
    );
    assert_eq!(suspect_uses(&go, "loja/pedido.go", "Total"), ["loja/pedido.go:8:Dobro"]);

    let csharp = scanned(
        "uma-letra-csharp",
        &[
            ("Loja/Pedido.cs", "namespace Loja;\n\npublic class Pedido\n{\n    public int Total { get; set; }\n}\n"),
            (
                "Loja/Caixa.cs",
                "using System.Linq;\n\nnamespace Loja;\n\npublic class Caixa\n{\n    \
                 public int Somar(Pedido[] pedidos) => pedidos.Sum(p => p.Total);\n}\n",
            ),
        ],
    );
    assert_eq!(suspect_uses(&csharp, "Loja/Pedido.cs", "Total"), ["Loja/Caixa.cs:7:Somar"]);
}
