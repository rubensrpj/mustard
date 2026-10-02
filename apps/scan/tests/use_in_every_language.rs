//! Quem usa cada declaração, nas 8 linguagens que o scan lê. Em cada uma, um
//! arquivo chama uma função de outro arquivo que ele enxerga (importado, ou do
//! mesmo namespace no C#, no Go e no PHP), chama `x.kind()` com um campo
//! `kind` declarado no arquivo que ele enxerga, e chama `.join(` com uma
//! função `join` declarada num terceiro arquivo que ele não enxerga. O uso só
//! liga à função chamada: o campo não se chama, e o `join` de fora é o da
//! biblioteca, não o do terceiro arquivo. Os nomes se repetem de propósito em
//! todas as linguagens, para que uma ligação pelo nome no projeto inteiro
//! apareça como uso cruzando linguagens.
//!
//! O mesmo projeto mostra também o uso sem chamada: em cada linguagem, o tipo
//! `Item` é citado como tipo de parâmetro no arquivo que chama, e no Rust uma
//! constante é citada numa comparação e uma função é chamada pelo caminho do
//! arquivo, sem `use`.
//!
//! Por fim, um projeto à parte mostra o que a linguagem põe à vista sem import
//! no arquivo, e o que ela não põe: o `global using` do C#, o namespace de cima
//! no C#, o pacote do próprio projeto com escopo no TypeScript, a pasta como
//! parte do pacote no Go, o import de arquivo sem `./` no Dart, o namespace de
//! outra linguagem que não responde a um import, e o nome escrito dentro de um
//! `using` ou de um `namespace`, que não é uso.
//!
//! E um projeto em Dart mostra que cada declaração com corpo termina no fim do
//! corpo, em classe, extensão e enum, e que o arquivo `part of` enxerga o dono.
//!
//! Por último, a citação liga pelo que o projeto declara, e não pela letra com
//! que o nome começa: a constante minúscula do Go e do TypeScript ganha quem a
//! usa, o nome da biblioteca que o projeto não declara (`DateTime` no C#) e a
//! variável de dentro da função não ficam guardados, o nome trazido pelo import
//! não é uso na linha do import, e a leitura que relê só o que mudou liga o
//! tipo novo ao arquivo que não mudou, como a leitura inteira. O mesmo projeto
//! mostra que o comando que declara duas constantes dá as duas.
//!
//! Cada ligação diz se é provada ou suspeita: no Rust, no TypeScript e no
//! Python, duas funções `run` em módulos diferentes, chamadas por quem
//! importa uma delas, por quem não importa nenhuma e por uma variável, e um
//! nome declarado uma vez só, chamado sem import. No Rust, o próprio objeto e
//! o tipo escrito antes do método estreitam a ligação, e o nome declarado mais
//! vezes que o teto só se conta.
//!
//! O nome que o próprio código liga a outra coisa vence a declaração do
//! projeto de mesmo nome, no Rust, no TypeScript e no Python: a variável, o
//! parâmetro e o nome tirado de desestruturação, o nome trazido por um import
//! de fora do projeto e o nome que a língua põe em todo arquivo.
//!
//! O arquivo JavaScript entra no mapa como o TypeScript, e o `require` importa
//! como o `import`, nos dois: o arquivo que ele nomeia vira dependência, e o
//! nome que ele traz de fora do projeto não liga. A chamada aberta por um nome
//! que não é peça do projeto nem do arquivo (`File.ReadAllText()` no C#) é da
//! biblioteca, e não liga; a aberta pelo tipo do projeto, pelo campo, pelo
//! parâmetro ou pelo objeto visto pelo tipo de cima segue ligando.
//!
//! E a pasta do projeto de teste some mesmo quando o teste quebra no meio, e
//! nenhum teste do scan monta essa pasta à mão.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;
#[path = "support/model.rs"]
mod model;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// A pasta do projeto de teste, numa pasta temporária. Quem chama guarda o
/// valor até o fim do teste: quando ele sai de cena, a pasta some, também
/// quando uma conferência quebra no meio.
fn project_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new().prefix(&format!("scan-{name}-")).tempdir().unwrap()
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A pasta do projeto onde o scan grava o mapa.
fn map_dir(dir: &Path) -> PathBuf {
    dir.join(".claude")
}

fn scan(dir: &Path) -> Value {
    model::scan(dir, &map_dir(dir), &[]).0
}

/// Uma linguagem do projeto: o arquivo que chama, o texto da chamada da
/// função, e os três arquivos com o que cada um tem dentro.
struct Language {
    caller: &'static str,
    call: &'static str,
    files: [(&'static str, &'static str); 3],
}

fn languages() -> Vec<Language> {
    vec![
        Language {
            caller: "rs/src/main.rs",
            call: "somar(1, 2)",
            files: [
                (
                    "rs/src/main.rs",
                    "mod conta;\nuse crate::conta::somar;\nuse crate::conta::Item;\n\n\
                     fn principal(x: Item) -> u32 {\n    let total = somar(1, 2);\n    let _ = x.kind();\n    \
                     let _ = [\"a\", \"b\"].join(\",\");\n    total\n}\n",
                ),
                (
                    "rs/src/conta.rs",
                    "pub struct Item {\n    pub kind: u32,\n}\n\npub fn somar(a: u32, b: u32) -> u32 {\n    a + b\n}\n",
                ),
                ("rs/src/texto.rs", "pub fn join(partes: &[&str]) -> String {\n    partes[0].to_string()\n}\n"),
            ],
        },
        Language {
            caller: "ts/src/app.ts",
            call: "somar(1, 2)",
            files: [
                (
                    "ts/src/app.ts",
                    "import { somar, Item } from \"./conta\";\n\n\
                     export function principal(x: Item): number {\n  const total = somar(1, 2);\n  x.kind();\n  \
                     [\"a\"].join(\",\");\n  return total;\n}\n",
                ),
                (
                    "ts/src/conta.ts",
                    "export class Item {\n  kind = () => \"item\";\n}\n\n\
                     export function somar(a: number, b: number): number {\n  return a + b;\n}\n",
                ),
                ("ts/src/texto.ts", "export function join(partes: string[]): string {\n  return partes[0];\n}\n"),
            ],
        },
        Language {
            caller: "tsx/src/app.tsx",
            call: "somar(1, 2)",
            files: [
                (
                    "tsx/src/app.tsx",
                    "import { somar, Item } from \"./conta\";\n\n\
                     export function principal(x: Item): number {\n  const total = somar(1, 2);\n  x.kind();\n  \
                     [\"a\"].join(\",\");\n  return total;\n}\n",
                ),
                (
                    "tsx/src/conta.tsx",
                    "export class Item {\n  kind = () => \"item\";\n}\n\n\
                     export function somar(a: number, b: number): number {\n  return a + b;\n}\n",
                ),
                ("tsx/src/texto.tsx", "export function join(partes: string[]): string {\n  return partes[0];\n}\n"),
            ],
        },
        Language {
            caller: "py/pkg/app.py",
            call: "somar(1, 2)",
            files: [
                (
                    "py/pkg/app.py",
                    "from pkg.conta import somar, Item\n\n\n\
                     def principal(x: Item) -> int:\n    total = somar(1, 2)\n    x.kind()\n    \
                     \", \".join([\"a\", \"b\"])\n    return total\n",
                ),
                ("py/pkg/conta.py", "class Item:\n    kind = None\n\n\ndef somar(a, b):\n    return a + b\n"),
                ("py/pkg/texto.py", "def join(partes):\n    return partes[0]\n"),
            ],
        },
        Language {
            caller: "go/conta/app.go",
            call: "somar(1, 2)",
            files: [
                (
                    "go/conta/app.go",
                    "package conta\n\nfunc principal(x Item, nomes Lista) int {\n\ttotal := somar(1, 2)\n\tx.kind()\n\t\
                     nomes.join(\",\")\n\treturn total\n}\n",
                ),
                (
                    "go/conta/conta.go",
                    "package conta\n\ntype Item struct {\n\tkind func() string\n}\n\n\
                     func somar(a int, b int) int {\n\treturn a + b\n}\n",
                ),
                ("go/outro/texto.go", "package outro\n\nfunc join(partes []string) string {\n\treturn partes[0]\n}\n"),
            ],
        },
        Language {
            caller: "cs/App.cs",
            call: "somar(1, 2)",
            files: [
                (
                    "cs/App.cs",
                    "namespace Loja.Conta;\n\npublic class App\n{\n    public int principal(Item x, Lista nomes)\n    {\n        \
                     var total = Calculo.somar(1, 2);\n        x.kind();\n        nomes.join(\",\");\n        \
                     return total;\n    }\n}\n",
                ),
                (
                    "cs/Conta.cs",
                    "namespace Loja.Conta;\n\npublic class Item\n{\n    public System.Func<string> kind = () => \"item\";\n}\n\n\
                     public static class Calculo\n{\n    public static int somar(int a, int b) => a + b;\n}\n",
                ),
                (
                    "cs/Outro/Texto.cs",
                    "namespace Loja.Outro;\n\npublic static class Texto\n{\n    \
                     public static string join(string[] partes) => partes[0];\n}\n",
                ),
            ],
        },
        Language {
            caller: "php/App.php",
            call: "somar(1, 2)",
            files: [
                (
                    "php/App.php",
                    "<?php\n\nnamespace Loja\\Conta;\n\nfunction principal(Item $x, Lista $nomes): int\n{\n    \
                     $total = somar(1, 2);\n    $x->kind();\n    $nomes->join(',');\n    return $total;\n}\n",
                ),
                (
                    "php/Conta.php",
                    "<?php\n\nnamespace Loja\\Conta;\n\nclass Item\n{\n    public $kind;\n}\n\n\
                     function somar(int $a, int $b): int\n{\n    return $a + $b;\n}\n",
                ),
                (
                    "php/Outro/Texto.php",
                    "<?php\n\nnamespace Loja\\Outro;\n\nfunction join(array $partes): string\n{\n    return $partes[0];\n}\n",
                ),
            ],
        },
        Language {
            caller: "dart/lib/app.dart",
            call: "somar(1, 2)",
            files: [
                (
                    "dart/lib/app.dart",
                    "import './conta.dart';\n\nint principal(Item x) {\n  final total = somar(1, 2);\n  x.kind();\n  \
                     ['a'].join(',');\n  return total;\n}\n",
                ),
                (
                    "dart/lib/conta.dart",
                    "class Item {\n  String Function() kind = () => 'item';\n}\n\nint somar(int a, int b) => a + b;\n",
                ),
                ("dart/lib/texto.dart", "String join(List<String> partes) => partes.first;\n"),
            ],
        },
    ]
}

/// A linha, contada de 1, em que o texto aparece pela primeira vez.
fn line_of(body: &str, text: &str) -> usize {
    body.lines().position(|l| l.contains(text)).expect("o texto está no arquivo") + 1
}

#[test]
fn a_use_only_links_to_what_is_called_and_to_what_the_file_sees() {
    let temp = project_dir("uso-em-toda-linguagem");
    let dir = temp.path().to_path_buf();
    let every = languages();
    for l in &every {
        for (rel, body) in l.files {
            write(&dir, rel, body);
        }
    }
    let map = scan(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let language: HashMap<&str, &str> =
        modules.iter().map(|m| (m["path"].as_str().unwrap(), m["language"].as_str().unwrap())).collect();
    let uses = |d: &Value| -> Vec<String> {
        d.get("used_by")
            .and_then(Value::as_array)
            .map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    let declarations = |name: &str| -> Vec<(String, Value)> {
        modules
            .iter()
            .flat_map(|m| {
                m["declarations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(move |d| d["name"] == name)
                    .map(move |d| (m["path"].as_str().unwrap().to_string(), d.clone()))
            })
            .collect()
    };

    // Em cada linguagem, a função chamada tem um uso só: o do arquivo e da
    // linha da chamada. A declaração de onde a chamada parte fica fora da
    // conferência: no Dart, o scan ainda não sabe onde a função termina.
    let sum = declarations("somar");
    for l in &every {
        let body = l.files.iter().find(|(rel, _)| *rel == l.caller).unwrap().1;
        let place = format!("{}:{}", l.caller, line_of(body, l.call));
        let owner = l.files[1].0;
        let (_, d) = sum.iter().find(|(p, _)| p == owner).unwrap_or_else(|| panic!("somar declarada em {owner}"));
        let u = uses(d);
        assert!(
            u.len() == 1 && (u[0] == place || u[0].starts_with(&format!("{place}:"))),
            "o uso da função chamada, em {owner}, é {place}: {d}"
        );
    }

    // O campo `kind` não se chama: nenhum uso, em nenhuma linguagem que o
    // declara como campo ou propriedade.
    let kind = declarations("kind");
    assert!(kind.len() >= 7, "o campo kind está declarado nas linguagens que leem campo: {kind:?}");
    for (p, d) in &kind {
        assert!(uses(d).is_empty(), "o campo kind de {p} não tem uso: {d}");
    }

    // O `join` do terceiro arquivo, que quem chama não enxerga, não tem uso.
    let join = declarations("join");
    assert_eq!(join.len(), every.len(), "um join por linguagem: {join:?}");
    for (p, d) in &join {
        assert!(uses(d).is_empty(), "o join de {p} não tem uso: {d}");
    }

    // Nenhum uso liga arquivos de linguagens diferentes.
    for m in modules {
        let owner = m["path"].as_str().unwrap();
        for d in m["declarations"].as_array().into_iter().flatten() {
            for u in uses(d) {
                let origin = u.split(':').next().unwrap();
                assert_eq!(language.get(origin), language.get(owner), "o uso {u} de {} em {owner}", d["name"]);
            }
        }
    }

}

/// O que o Rust ganha a mais para o uso sem chamada: uma constante e uma
/// função em `preco.rs`; `pedido.rs` importa a constante e a compara dentro de
/// `abrir`; `caixa.rs`, sem nenhum `use`, chama a função pelo caminho do
/// arquivo dentro de `pagar`.
const PRICE: &str = "pub const LIMITE: u32 = 10;\n\npub fn total(a: u32, b: u32) -> u32 {\n    a + b\n}\n";
const ORDER: &str = "use crate::preco::LIMITE;\n\npub fn abrir(n: u32) -> bool {\n    n > LIMITE\n}\n";
const BOX: &str = "pub fn pagar() -> u32 {\n    crate::preco::total(1, 2)\n}\n";

#[test]
fn a_constant_and_a_type_that_are_mentioned_gain_their_users() {
    let temp = project_dir("citacao-em-toda-linguagem");
    let dir = temp.path().to_path_buf();
    let every = languages();
    for l in &every {
        for (rel, body) in l.files {
            write(&dir, rel, body);
        }
    }
    write(&dir, "rs/src/preco.rs", PRICE);
    write(&dir, "rs/src/pedido.rs", ORDER);
    write(&dir, "rs/src/caixa.rs", BOX);
    let map = scan(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let uses = |file: &str, name: &str| -> Vec<String> {
        let m = modules.iter().find(|m| m["path"] == file).unwrap_or_else(|| panic!("{file} no mapa"));
        let d = m["declarations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["name"] == name)
            .unwrap_or_else(|| panic!("{name} declarado em {file}: {m}"));
        d.get("used_by")
            .and_then(Value::as_array)
            .map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    // Junta as faltas antes de reprovar, para que um defeito só na leitura da
    // citação ou só no caminho pelo arquivo apareça inteiro de uma vez.
    let mut failures: Vec<String> = Vec::new();

    // Em cada linguagem, o tipo citado no parâmetro de `principal` tem o uso
    // com o arquivo e a linha da citação, e `principal` como quem usa.
    for l in &every {
        let body = l.files.iter().find(|(rel, _)| *rel == l.caller).unwrap().1;
        let expected = format!("{}:{}:principal", l.caller, line_of(body, "principal("));
        let u = uses(l.files[1].0, "Item");
        if !u.contains(&expected) {
            failures.push(format!("o tipo Item de {} tem o uso {expected}: {u:?}", l.files[1].0));
        }
    }

    // A constante comparada dentro de `abrir` tem o uso, com `abrir` como quem
    // usa. A linha do `use` que a importa não conta: o nome ali é o caminho do
    // import, não um uso.
    let limit = uses("rs/src/preco.rs", "LIMITE");
    let expected = format!("rs/src/pedido.rs:{}:abrir", line_of(ORDER, "n > LIMITE"));
    if limit != [expected.clone()] {
        failures.push(format!("a constante LIMITE tem só o uso {expected}: {limit:?}"));
    }

    // A função chamada pelo caminho do arquivo, sem `use`, tem o uso na linha
    // da chamada, com `pagar` como quem usa.
    let total = uses("rs/src/preco.rs", "total");
    let expected = format!("rs/src/caixa.rs:{}:pagar", line_of(BOX, "crate::preco::total("));
    if total != [expected.clone()] {
        failures.push(format!("a função total tem só o uso {expected}: {total:?}"));
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// O projeto do que cada linguagem põe à vista. No C#, dois projetos: `Loja`,
/// com um `global using` num arquivo só, e `Outro`, fora dele; `Pedido.cs`, em
/// `Loja.Pedidos`, não tem `using` nenhum. No TypeScript, um pacote `@loja/core`
/// cujo código mora em `src/`, importado por outro pacote pelo nome. No Go,
/// duas pastas com o mesmo `package util`. No Dart, um import de arquivo sem
/// `./`, ao lado de um pacote Go com o mesmo nome do arquivo; e um arquivo
/// Python que importa esse mesmo nome, que no Python não é de ninguém.
fn project_in_sight() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Loja/Loja.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\">\n</Project>\n"),
        ("Loja/GlobalUsings.cs", "global using Loja.Dominio;\n"),
        (
            "Loja/Dominio/Calculadora.cs",
            "namespace Loja.Dominio;\n\npublic class Calculadora\n{\n    public int Total(int n) => n;\n}\n",
        ),
        ("Loja/Regra.cs", "namespace Loja;\n\npublic class Regra\n{\n    public int Arredondar(int n) => n;\n}\n"),
        ("Loja/Pedidos.cs", "namespace Loja;\n\npublic class Pedidos\n{\n}\n"),
        (
            "Loja/Pedidos/Pedido.cs",
            "namespace Loja.Pedidos;\n\npublic class Pedido\n{\n    public void Fechar()\n    {\n        \
             var c = new Calculadora();\n        c.Total(1);\n        var r = new Regra();\n        \
             r.Arredondar(2);\n    }\n}\n",
        ),
        ("Outro/Outro.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\">\n</Project>\n"),
        (
            "Outro/Conta.cs",
            "namespace Outro;\n\npublic class Conta\n{\n    public void Pagar()\n    {\n        \
             var c = new Calculadora();\n        c.Total(3);\n    }\n}\n",
        ),
        (
            "packages/core/package.json",
            "{\n  \"name\": \"@loja/core\",\n  \"exports\": {\n    \"./server/*\": \"./src/server/*.ts\"\n  }\n}\n",
        ),
        ("packages/core/src/server/preco.ts", "export function total(n: number): number {\n  return n;\n}\n"),
        (
            "apps/web/src/pedido.ts",
            "import { total } from '@loja/core/server/preco';\n\nexport function fechar(): number {\n  return total();\n}\n",
        ),
        ("a/util/x.go", "package util\n\nfunc Dobro(n int) int {\n\treturn n * 2\n}\n"),
        ("b/util/y.go", "package util\n\nfunc Dobro(n int) int {\n\treturn n + n\n}\n"),
        ("a/util/usa.go", "package util\n\nfunc Usa() int {\n\treturn Dobro(1)\n}\n"),
        ("dart/lib/pedido.dart", "import 'conta.dart';\n\nint fechar() {\n  return total(1);\n}\n"),
        ("dart/lib/conta.dart", "int total(int n) => n;\n"),
        ("conta/conta.go", "package conta\n\nfunc total(n int) int {\n\treturn n\n}\n"),
        ("py/caixa.py", "import conta\n\n\ndef pagar():\n    return total(1)\n"),
    ]
}

#[test]
fn each_file_sees_what_the_language_puts_in_sight() {
    let temp = project_dir("o-que-a-linguagem-poe-a-vista");
    let dir = temp.path().to_path_buf();
    let files = project_in_sight();
    for (rel, body) in &files {
        write(&dir, rel, body);
    }
    let map = scan(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let module = |file: &str| -> &Value {
        modules.iter().find(|m| m["path"] == file).unwrap_or_else(|| panic!("{file} no mapa"))
    };
    let list = |v: &Value| -> Vec<String> {
        v.as_array().map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    // O lugar de cada uso, provado ou suspeito: aqui se confere o que fica à
    // vista de cada arquivo, e a chamada por uma variável (`c.Total(1)`) é
    // suspeita mesmo com uma candidata só.
    let uses = |file: &str, name: &str| -> Vec<String> {
        let m = module(file);
        let d = m["declarations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["name"] == name)
            .unwrap_or_else(|| panic!("{name} declarado em {file}: {m}"));
        d["used_by"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|u| u.as_str().or_else(|| u["at"].as_str()).unwrap().to_string())
            .collect::<Vec<String>>()
    };
    let deps = |file: &str| list(&module(file)["deps"]);
    let body = |file: &str| files.iter().find(|(rel, _)| *rel == file).unwrap().1;
    // Junta as faltas antes de reprovar, para que cada parte que voltar a ser
    // como antes apareça de uma vez.
    let mut failures: Vec<String> = Vec::new();

    // O `global using` de Loja/GlobalUsings.cs vale para todo arquivo C# do
    // projeto Loja: Pedido.cs usa Total sem `using`, e Conta.cs, do projeto
    // Outro, não o enxerga.
    let order = "Loja/Pedidos/Pedido.cs";
    let total = uses("Loja/Dominio/Calculadora.cs", "Total");
    let expected = format!("{order}:{}:Fechar", line_of(body(order), "c.Total(1)"));
    if !total.contains(&expected) || total.iter().any(|u| u.starts_with("Outro/Conta.cs:")) {
        failures.push(format!("Total tem o uso {expected} e nenhum de Outro/Conta.cs: {total:?}"));
    }
    // O import global fica guardado só no arquivo que o escreve, e a aresta do
    // grafo de import, só nele. Como todo import de namespace, ela liga só aos
    // arquivos que declaram um nome que o próprio arquivo usa: GlobalUsings.cs
    // não usa nada e não liga a nada, e Pedido.cs, que usa Calculadora pelo
    // import global, não ganha aresta por ele.
    let globals = list(&module("Loja/GlobalUsings.cs")["global_imports"]);
    if globals != ["Loja.Dominio"] {
        failures.push(format!("GlobalUsings.cs guarda o import global Loja.Dominio: {globals:?}"));
    }
    if let Some(m) = modules.iter().find(|m| m["path"] != "Loja/GlobalUsings.cs" && m.get("global_imports").is_some()) {
        failures.push(format!("só GlobalUsings.cs grava import global: {}", m["path"]));
    }
    if !deps("Loja/GlobalUsings.cs").is_empty() || deps(order).contains(&"Loja/Dominio/Calculadora.cs".to_string()) {
        failures.push(format!(
            "nem GlobalUsings.cs, que não usa nada, nem Pedido.cs têm aresta para Calculadora.cs: {:?} / {:?}",
            deps("Loja/GlobalUsings.cs"),
            deps(order)
        ));
    }

    // O namespace de cima: Pedido.cs, em Loja.Pedidos, enxerga Loja.
    let round = uses("Loja/Regra.cs", "Arredondar");
    let expected = format!("{order}:{}:Fechar", line_of(body(order), "r.Arredondar(2)"));
    if !round.contains(&expected) {
        failures.push(format!("Arredondar tem o uso {expected}: {round:?}"));
    }
    // O nome escrito dentro do `namespace Loja.Pedidos;` não é uso da classe
    // Pedidos, que agora está à vista.
    let namespace_line = format!("{order}:{}", line_of(body(order), "namespace Loja.Pedidos;"));
    let orders = uses("Loja/Pedidos.cs", "Pedidos");
    if orders.iter().any(|u| u == &namespace_line || u.starts_with(&format!("{namespace_line}:"))) {
        failures.push(format!("a classe Pedidos não tem uso na linha do namespace: {orders:?}"));
    }

    // O pacote do projeto com escopo: `@loja/core/server/preco` acha o arquivo
    // em `src/server/`, onde o package.json põe o código.
    let web = "apps/web/src/pedido.ts";
    let total_ts = uses("packages/core/src/server/preco.ts", "total");
    if !total_ts.iter().any(|u| u.starts_with(&format!("{web}:"))) {
        failures.push(format!("o total do TypeScript tem o uso em {web}: {total_ts:?}"));
    }
    if !deps(web).contains(&"packages/core/src/server/preco.ts".to_string()) {
        failures.push(format!("{web} tem packages/core/src/server/preco.ts nos deps: {:?}", deps(web)));
    }

    // O pacote do Go é a pasta: o Dobro de b/util não ganha o uso de a/util.
    let double_a = uses("a/util/x.go", "Dobro");
    let double_b = uses("b/util/y.go", "Dobro");
    if double_a.is_empty() || double_a.iter().any(|u| !u.starts_with("a/util/usa.go:")) || !double_b.is_empty() {
        failures.push(format!("só o Dobro de a/util/x.go tem o uso de a/util/usa.go: {double_a:?} / {double_b:?}"));
    }

    // O import de arquivo sem `./` no Dart é o arquivo ao lado, e não o pacote
    // Go `conta`; nem o `import conta` do Python responde com esse pacote.
    let total_dart = uses("dart/lib/conta.dart", "total");
    if !total_dart.iter().any(|u| u.starts_with("dart/lib/pedido.dart:")) {
        failures.push(format!("o total de dart/lib/conta.dart tem o uso de dart/lib/pedido.dart: {total_dart:?}"));
    }
    let total_go = uses("conta/conta.go", "total");
    if !total_go.is_empty() {
        failures.push(format!("o total de conta/conta.go não tem uso: {total_go:?}"));
    }
    if deps("dart/lib/pedido.dart") != ["dart/lib/conta.dart"] {
        failures.push(format!("o único dos deps de dart/lib/pedido.dart é dart/lib/conta.dart: {:?}", deps("dart/lib/pedido.dart")));
    }
    if !deps("py/caixa.py").is_empty() {
        failures.push(format!("o import conta do Python não acha o pacote Go: {:?}", deps("py/caixa.py")));
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A biblioteca Dart do projeto: `lib/caixa.dart` tem a função `dobro`, uma
/// classe com construtor com corpo, construtor nomeado, factory, getter e
/// setter, uma extensão com um método e um enum com um método, todos com
/// corpo em várias linhas que chamam `dobro`; `lib/caixa_parte.dart` é parte
/// dela pelo arquivo. `lib/conta.dart` dá nome à biblioteca, e
/// `lib/conta_parte.dart` é parte dela pelo nome.
const BOX_DART: &str = "part 'caixa_parte.dart';\n\nint dobro(int n) {\n  return n * 2;\n}\n\n\
class Caixa {\n  int _v = 0;\n\n  Caixa(int n) {\n    _v = dobro(n);\n  }\n\n  \
Caixa.vazia() {\n    _v = dobro(0);\n  }\n\n  factory Caixa.de(int n) {\n    return Caixa(dobro(n));\n  }\n\n  \
int get valor {\n    return dobro(_v);\n  }\n\n  set valor(int v) {\n    _v = dobro(v);\n  }\n}\n\n\
extension Metade on int {\n  int metade() {\n    return dobro(this) ~/ 4;\n  }\n}\n\n\
enum Cor {\n  azul;\n\n  int peso() {\n    return dobro(1);\n  }\n}\n";
const BOX_PART_DART: &str = "part of 'caixa.dart';\n\nint extra() {\n  return dobro(3);\n}\n";
const ACCOUNT_DART: &str = "library loja.conta;\n\npart 'conta_parte.dart';\n\nint triplo(int n) {\n  return n * 3;\n}\n";
const ACCOUNT_PART_DART: &str = "part of loja.conta;\n\nint usa() {\n  return triplo(1);\n}\n";

/// A linha da chave que fecha o corpo que abre na linha do cabeçalho: a
/// primeira, depois dele, que tem só `}` com o mesmo recuo.
fn end_of_body(body: &str, header: &str) -> usize {
    let start = line_of(body, header);
    let lines: Vec<&str> = body.lines().collect();
    let indent = &lines[start - 1][..lines[start - 1].len() - lines[start - 1].trim_start().len()];
    let closer = format!("{indent}}}");
    start + lines[start..].iter().position(|l| *l == closer).expect("o corpo fecha") + 1
}

#[test]
fn dart_ends_each_declaration_at_the_end_of_the_body_and_the_part_sees_the_owner() {
    let temp = project_dir("dart-fim-do-corpo");
    let dir = temp.path().to_path_buf();
    write(&dir, "lib/caixa.dart", BOX_DART);
    write(&dir, "lib/caixa_parte.dart", BOX_PART_DART);
    write(&dir, "lib/conta.dart", ACCOUNT_DART);
    write(&dir, "lib/conta_parte.dart", ACCOUNT_PART_DART);
    let map = scan(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let module = |file: &str| -> &Value {
        modules.iter().find(|m| m["path"] == file).unwrap_or_else(|| panic!("{file} no mapa"))
    };
    let list = |v: &Value| -> Vec<String> {
        v.as_array().map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    let box_module = module("lib/caixa.dart");
    let declarations = box_module["declarations"].as_array().expect("declarations");
    let mut failures: Vec<String> = Vec::new();

    // Cada declaração com corpo, de quem é o uso de `dobro` dentro dela: o
    // nome da própria declaração, o cabeçalho e o texto da chamada.
    let members = [
        ("dobro", "int dobro(int n) {", None),
        ("Caixa", "  Caixa(int n) {", Some("_v = dobro(n);")),
        ("vazia", "Caixa.vazia() {", Some("_v = dobro(0);")),
        ("de", "factory Caixa.de(int n) {", Some("return Caixa(dobro(n));")),
        ("valor", "int get valor {", Some("return dobro(_v);")),
        ("valor", "set valor(int v) {", Some("_v = dobro(v);")),
        ("metade", "int metade() {", Some("return dobro(this) ~/ 4;")),
        ("peso", "int peso() {", Some("return dobro(1);")),
    ];
    let mut expected: Vec<String> = Vec::new();
    for (name, header, call) in members {
        let line = line_of(BOX_DART, header);
        let end = end_of_body(BOX_DART, header);
        match declarations.iter().find(|d| d["name"] == name && d["line"] == line) {
            Some(d) if d["end_line"] == end => {}
            Some(d) => failures.push(format!("{name}, da linha {line}, termina na linha {end}: {d}")),
            None => failures.push(format!("{name} é declarado na linha {line}")),
        }
        if let Some(call) = call {
            expected.push(format!("lib/caixa.dart:{}:{name}", line_of(BOX_DART, call)));
        }
    }
    // O arquivo `part of 'caixa.dart';` divide a biblioteca com o dono, e a
    // chamada de lá liga ao `dobro` daqui.
    expected.push(format!("lib/caixa_parte.dart:{}:extra", line_of(BOX_PART_DART, "dobro(3)")));
    let double = declarations.iter().find(|d| d["name"] == "dobro").expect("dobro declarado");
    let mut uses = list(&double["used_by"]);
    uses.sort();
    expected.sort();
    if uses != expected {
        failures.push(format!("cada uso de dobro vem da própria declaração, nunca da classe nem do enum: {uses:?}"));
    }

    // O cabeçalho do setter declara `valor`, e não o chama.
    let setter = line_of(BOX_DART, "set valor(int v)");
    let calls = list(&box_module["calls"]);
    if calls.iter().any(|c| c == &format!("valor:{setter}")) {
        failures.push(format!("não há chamada valor na linha {setter}, do setter: {calls:?}"));
    }

    // A parte pelo nome da biblioteca, `part of loja.conta;`, enxerga o dono
    // que se declara `library loja.conta;`.
    let account = module("lib/conta.dart");
    let triple = account["declarations"].as_array().into_iter().flatten().find(|d| d["name"] == "triplo");
    let triple_uses = triple.map(|d| list(&d["used_by"])).unwrap_or_default();
    let expected_use = format!("lib/conta_parte.dart:{}:usa", line_of(ACCOUNT_PART_DART, "triplo(1)"));
    if !triple_uses.contains(&expected_use) {
        failures.push(format!("triplo tem o uso {expected_use}: {triple_uses:?}"));
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// O projeto da citação que liga pelo que o projeto declara. Em cada uma das
/// linguagens C#, Go, Python, PHP e Dart, uma constante é declarada num
/// arquivo e citada sem chamar em outro que o enxerga; no Go, com minúscula,
/// noutro arquivo do mesmo pacote. No TypeScript, `limiteDiario` é importada
/// pelo nome e citada depois, `taxaPadrao` é citada no próprio arquivo, e
/// `parcial` é uma variável de dentro da função. O C# cita `DateTime.Today`,
/// que nenhum arquivo do projeto declara. E três linhas declaram duas
/// constantes cada uma.
fn mention_project() -> Vec<(&'static str, &'static str)> {
    vec![
        ("src/limites.ts", "export const limiteDiario = 10;\n"),
        (
            "src/pedido.ts",
            "import { limiteDiario } from './limites';\n\nexport function podeComprar(valor: number): boolean {\n  \
             return valor <= limiteDiario;\n}\n",
        ),
        (
            "src/taxa.ts",
            "const taxaPadrao = 2;\n\nexport function calcular(valor: number): number {\n  \
             const parcial = valor * 3;\n  return parcial + taxaPadrao;\n}\n",
        ),
        ("src/par.ts", "export const a = 1, b = 2;\n"),
        ("src/tipos.ts", "export interface Velho {\n  nome: string;\n}\n"),
        ("src/usa.ts", "import { Novo } from './tipos';\n\nexport function usar(x: Novo): void {\n  console.log(x);\n}\n"),
        (
            "cs/Regras.cs",
            "namespace Loja;\n\npublic static class Regras\n{\n    public const int Limite = 10;\n    \
             public const int A = 1, B = 2;\n}\n",
        ),
        (
            "cs/Pedido.cs",
            "namespace Loja;\n\npublic class Pedido\n{\n    public bool Pode(int valor)\n    {\n        \
             var hoje = DateTime.Today;\n        return valor <= Regras.Limite;\n    }\n}\n",
        ),
        ("go/conta/limites.go", "package conta\n\nconst limite = 10\n"),
        ("go/conta/pedido.go", "package conta\n\nfunc pode(valor int) bool {\n\treturn valor <= limite\n}\n"),
        ("py/loja/regras.py", "LIMITE = 10\n"),
        ("py/loja/pedido.py", "from loja.regras import LIMITE\n\n\ndef pode(valor):\n    return valor <= LIMITE\n"),
        (
            "php/Regras.php",
            "<?php\n\nnamespace Loja;\n\nclass Regras\n{\n    const LIMITE = 10;\n    const A = 1, B = 2;\n}\n",
        ),
        (
            "php/Pedido.php",
            "<?php\n\nnamespace Loja;\n\nfunction pode(int $valor): bool\n{\n    return $valor <= Regras::LIMITE;\n}\n",
        ),
        ("dart/lib/regras.dart", "const limite = 10;\n\nclass Regras {\n  static const int teto = 20;\n}\n"),
        (
            "dart/lib/pedido.dart",
            "import 'regras.dart';\n\nbool pode(int valor) {\n  return valor <= limite && valor < Regras.teto;\n}\n",
        ),
    ]
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

/// Uma leitura do scan, com os argumentos a mais: o mapa, os bytes dele e se
/// a leitura foi inteira.
fn scan_with_bytes(dir: &Path, extra: &[&str]) -> (Value, Vec<u8>, bool) {
    let (map, report) = model::scan(dir, &map_dir(dir), extra);
    (map, model::read_bytes(&map_dir(dir)), report["full"] == Value::Bool(true))
}

#[test]
fn a_mention_links_by_what_the_project_declares_and_not_by_the_letter() {
    let temp = project_dir("citacao-que-liga");
    let dir = temp.path().to_path_buf();
    let project = mention_project();
    for (rel, body) in &project {
        write(&dir, rel, body);
    }
    git(&dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "primeiro"]);
    let body = |rel: &str| project.iter().find(|(p, _)| *p == rel).unwrap().1;

    let (map, _, _) = scan_with_bytes(&dir, &["--all"]);
    let modules = map["modules"].as_array().expect("modules").clone();
    let module = |file: &str| -> Value {
        modules.iter().find(|m| m["path"] == file).cloned().unwrap_or_else(|| panic!("{file} no mapa"))
    };
    let list = |v: &Value| -> Vec<String> {
        v.as_array().map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    let declaration = |file: &str, name: &str| -> Option<Value> {
        module(file)["declarations"].as_array().into_iter().flatten().find(|d| d["name"] == name).cloned()
    };
    // Junta as faltas antes de reprovar, para que cada regra que quebra
    // apareça inteira de uma vez.
    let mut failures: Vec<String> = Vec::new();

    // Em cada linguagem, a constante tem o tipo const e o uso com o arquivo e
    // a linha da citação, e a declaração de onde a citação vem. No
    // TypeScript, só a linha da citação: a do import não é uso.
    let constants = [
        ("cs/Regras.cs", "Limite", "cs/Pedido.cs", "Regras.Limite", "Pode"),
        ("go/conta/limites.go", "limite", "go/conta/pedido.go", "valor <= limite", "pode"),
        ("py/loja/regras.py", "LIMITE", "py/loja/pedido.py", "valor <= LIMITE", "pode"),
        ("php/Regras.php", "LIMITE", "php/Pedido.php", "Regras::LIMITE", "pode"),
        ("dart/lib/regras.dart", "limite", "dart/lib/pedido.dart", "valor <= limite", "pode"),
        ("dart/lib/regras.dart", "teto", "dart/lib/pedido.dart", "Regras.teto", "pode"),
        ("src/limites.ts", "limiteDiario", "src/pedido.ts", "valor <= limiteDiario", "podeComprar"),
        ("src/taxa.ts", "taxaPadrao", "src/taxa.ts", "parcial + taxaPadrao", "calcular"),
    ];
    for (owner, name, who, mention, enclosing) in constants {
        let expected = vec![format!("{who}:{}:{enclosing}", line_of(body(who), mention))];
        match declaration(owner, name) {
            Some(d) if d["kind"] == "const" && list(&d["used_by"]) == expected => {}
            Some(d) => failures.push(format!("{name}, de {owner}, é const com só o uso {expected:?}: {d}")),
            None => failures.push(format!("{name} está declarado em {owner}")),
        }
    }

    // A variável de dentro da função e o nome da biblioteca que o projeto não
    // declara não ficam entre as citações do arquivo.
    for (file, name) in [("src/taxa.ts", "parcial"), ("cs/Pedido.cs", "DateTime")] {
        let mentions = list(&module(file)["cites"]);
        let found: Vec<&String> = mentions
            .iter()
            .filter(|c| c.starts_with(&format!("{name}:")) || c.contains(&format!(".{name}:")))
            .collect();
        if !found.is_empty() {
            failures.push(format!("{name} não liga a nada do projeto e não fica em {file}: {mentions:?}"));
        }
    }

    // Cada linha que declara duas constantes dá as duas, cada uma com o
    // próprio nome, o tipo const e o próprio cabeçalho.
    let pairs = [
        ("src/par.ts", [("a", "export const a"), ("b", "export const b")]),
        ("cs/Regras.cs", [("A", "public const int A"), ("B", "public const int B")]),
        ("php/Regras.php", [("A", "const A"), ("B", "const B")]),
    ];
    for (file, pair) in pairs {
        for (name, header) in pair {
            match declaration(file, name) {
                Some(d) if d["kind"] == "const" && d["signature"] == header => {}
                Some(d) => failures.push(format!("{name}, de {file}, é const com o cabeçalho {header}: {d}")),
                None => failures.push(format!("{name} está declarado em {file}")),
            }
        }
    }

    // O arquivo que muda passa a declarar o tipo que `src/usa.ts`, que não
    // mudou, já citava: a leitura que relê só o que mudou dá o uso, e o mesmo
    // mapa que a leitura inteira.
    write(&dir, "src/tipos.ts", "export interface Velho {\n  nome: string;\n}\n\nexport interface Novo {\n  id: number;\n}\n");
    let (step, bytes_step, whole) = scan_with_bytes(&dir, &[]);
    if whole {
        failures.push("a segunda leitura relê só o que mudou".to_string());
    }
    let user_file = body("src/usa.ts");
    let expected = vec![format!("src/usa.ts:{}:usar", line_of(user_file, "x: Novo"))];
    let fresh = step["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["path"] == "src/tipos.ts")
        .flat_map(|m| m["declarations"].as_array().cloned().unwrap_or_default())
        .find(|d| d["name"] == "Novo");
    match fresh {
        Some(d) if list(&d["used_by"]) == expected => {}
        other => failures.push(format!("o tipo novo tem o uso {expected:?} do arquivo que não mudou: {other:?}")),
    }
    let (_, bytes_whole, _) = scan_with_bytes(&dir, &["--all"]);
    if bytes_step != bytes_whole {
        failures.push("a leitura que relê só o que mudou dá o mesmo mapa que a leitura inteira".to_string());
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Uma linguagem do projeto das ligações provadas e suspeitas: a pasta dela e
/// os arquivos, cada um com o que tem dentro. Em cada uma, `run` é declarada
/// em dois módulos, `a` e `b`; `with_import` importa a de `a` e a chama;
/// `without_import` a chama sem importar nada; `uses_unique` chama `unica`,
/// declarada uma vez só na linguagem, sem importar nada; e `by_value`
/// importa a `run` de `a` e a chama por uma variável.
struct Links {
    a: &'static str,
    b: &'static str,
    with_import: &'static str,
    without_import: &'static str,
    unique: &'static str,
    uses_unique: &'static str,
    by_value: &'static str,
    files: [(&'static str, &'static str); 7],
}

fn links() -> Vec<Links> {
    vec![
        Links {
            a: "rs/src/a.rs",
            b: "rs/src/b.rs",
            with_import: "rs/src/with_import.rs",
            without_import: "rs/src/without_import.rs",
            unique: "rs/src/unica.rs",
            uses_unique: "rs/src/uses_unique.rs",
            by_value: "rs/src/by_value.rs",
            files: [
                ("rs/src/a.rs", "pub fn run() -> u32 {\n    1\n}\n"),
                ("rs/src/b.rs", "pub fn run() -> u32 {\n    2\n}\n"),
                ("rs/src/with_import.rs", "use crate::a::run;\n\npub fn with_import() -> u32 {\n    run()\n}\n"),
                ("rs/src/without_import.rs", "pub fn without_import() -> u32 {\n    run()\n}\n"),
                ("rs/src/unica.rs", "pub fn unica() -> u32 {\n    3\n}\n"),
                ("rs/src/uses_unique.rs", "pub fn uses_unique() -> u32 {\n    unica()\n}\n"),
                ("rs/src/by_value.rs", "use crate::a::run;\n\npub fn by_value(x: Tarefa) -> u32 {\n    x.run()\n}\n"),
            ],
        },
        Links {
            a: "ts/src/a/tarefa.ts",
            b: "ts/src/b/tarefa.ts",
            with_import: "ts/src/with_import.ts",
            without_import: "ts/src/without_import.ts",
            unique: "ts/src/unica.ts",
            uses_unique: "ts/src/uses_unique.ts",
            by_value: "ts/src/by_value.ts",
            files: [
                ("ts/src/a/tarefa.ts", "export function run(): number {\n  return 1;\n}\n"),
                ("ts/src/b/tarefa.ts", "export function run(): number {\n  return 2;\n}\n"),
                (
                    "ts/src/with_import.ts",
                    "import { run } from \"./a/tarefa\";\n\nexport function withImport(): number {\n  return run();\n}\n",
                ),
                ("ts/src/without_import.ts", "export function withoutImport(): number {\n  return run();\n}\n"),
                ("ts/src/unica.ts", "export function unica(): number {\n  return 3;\n}\n"),
                ("ts/src/uses_unique.ts", "export function usesUnique(): number {\n  return unica();\n}\n"),
                (
                    "ts/src/by_value.ts",
                    "import { run } from \"./a/tarefa\";\n\nexport function byValue(x: Tarefa): number {\n  \
                     return x.run();\n}\n",
                ),
            ],
        },
        Links {
            a: "py/pkg/a/tarefa.py",
            b: "py/pkg/b/tarefa.py",
            with_import: "py/pkg/with_import.py",
            without_import: "py/pkg/without_import.py",
            unique: "py/pkg/unica.py",
            uses_unique: "py/pkg/uses_unique.py",
            by_value: "py/pkg/by_value.py",
            files: [
                ("py/pkg/a/tarefa.py", "def run():\n    return 1\n"),
                ("py/pkg/b/tarefa.py", "def run():\n    return 2\n"),
                ("py/pkg/with_import.py", "from pkg.a.tarefa import run\n\n\ndef with_import():\n    return run()\n"),
                ("py/pkg/without_import.py", "def without_import():\n    return run()\n"),
                ("py/pkg/unica.py", "def unica():\n    return 3\n"),
                ("py/pkg/uses_unique.py", "def uses_unique():\n    return unica()\n"),
                ("py/pkg/by_value.py", "from pkg.a.tarefa import run\n\n\ndef by_value(x):\n    return x.run()\n"),
            ],
        },
    ]
}

/// O projeto do alcance de cada nome, uma pasta por linguagem. No Python,
/// `l/util.py` declara `open` e `dumps`; `l/a.py` traz o módulo pelo pacote
/// (`from l import util`) e chama `util.open(1)` e `open("x")`; `l/b.py`
/// importa `json`, traz `dumps` pelo nome e chama os dois; `l/c.py` traz
/// `loja/servico.py` pelo apelido `s`; `l/d.py` chama `buscar()` sem import.
/// No Go, `api/a.go` importa `strings` e o pacote `util`, e chama o `Join` de
/// cada um. No Dart, `lib/a.dart` importa `dart:convert` com o prefixo `c` e
/// `util.dart` sem prefixo, que declara `jsonEncode`. No TypeScript,
/// `src/util.ts` declara `setTimeout` e `join`; `src/a.ts` o importa como
/// `util` e chama `setTimeout`; `src/b.ts` o importa como `u` e chama
/// `u.join`; `src/aves.ts` e `src/app.ts` declaram cada um o seu `router`, e
/// `app.ts` importa `aves.ts` e cita o `router`. No JavaScript antigo,
/// `servico.js` exporta `ler` pelo objeto `exports`, `outro.js` exporta
/// `gravar` por `module.exports.gravar`, `mais.js` exporta a função `apagar`
/// como o próprio módulo, `junta.js` declara `ler` e o exporta num objeto, e
/// `app.js` traz `ler` de `servico.js` pelo `require` e o chama.
const REACH: &[(&str, &str)] = &[
    ("py/pyproject.toml", "[project]\nname = \"l\"\n"),
    ("py/l/__init__.py", ""),
    ("py/l/util.py", "def open(x):\n    return x\n\n\ndef dumps(x):\n    return x\n"),
    ("py/l/a.py", "from l import util\n\n\ndef usa():\n    util.open(1)\n    return open(\"x\")\n"),
    ("py/l/b.py", "import json\nfrom l.util import dumps\n\n\ndef usa():\n    json.dumps({})\n    return dumps(1)\n"),
    ("py/l/c.py", "import loja.servico as s\n\n\ndef usa():\n    return s.buscar()\n"),
    ("py/l/d.py", "def usa():\n    return buscar()\n"),
    ("py/loja/__init__.py", ""),
    ("py/loja/servico.py", "def buscar():\n    return 1\n"),
    ("go/go.mod", "module example.com/l\n\ngo 1.21\n"),
    ("go/util/texto.go", "package util\n\nfunc Join(a []string, s string) string {\n\treturn s\n}\n"),
    (
        "go/api/a.go",
        "package api\n\nimport (\n\t\"strings\"\n\n\t\"example.com/l/util\"\n)\n\nfunc Usa() string {\n\t\
         strings.Join(nil, \",\")\n\treturn util.Join(nil, \",\")\n}\n",
    ),
    ("dart/pubspec.yaml", "name: l\n"),
    ("dart/lib/util.dart", "String jsonEncode(Object o) {\n  return \"\";\n}\n"),
    (
        "dart/lib/a.dart",
        "import 'dart:convert' as c;\nimport 'util.dart';\n\nvoid usa() {\n  c.jsonEncode({});\n  jsonEncode(1);\n}\n",
    ),
    ("ts/package.json", "{\"name\": \"t\"}\n"),
    (
        "ts/src/util.ts",
        "export function setTimeout(f: () => void, n: number) {\n  return n;\n}\n\n\
         export function join(s: string) {\n  return s;\n}\n",
    ),
    ("ts/src/a.ts", "import * as util from './util';\n\nexport function usa() {\n  setTimeout(() => {}, 1);\n}\n"),
    ("ts/src/b.ts", "import * as u from './util';\n\nexport function usa() {\n  return u.join('a');\n}\n"),
    ("ts/src/aves.ts", "const router = criar();\n\nexport default function aves() {\n  return router;\n}\n"),
    (
        "ts/src/app.ts",
        "import aves from './aves';\n\nconst router = criar();\n\nexport function app() {\n  router.get('/', aves);\n}\n",
    ),
    ("js/package.json", "{\"name\": \"j\"}\n"),
    ("js/servico.js", "exports.ler = function (req, res) {\n  return 1;\n};\n"),
    ("js/outro.js", "module.exports.gravar = (x) => x;\n"),
    ("js/mais.js", "module.exports = function apagar() {\n  return 0;\n};\n"),
    ("js/junta.js", "function ler() {\n  return 2;\n}\n\nmodule.exports = { ler };\n"),
    ("js/app.js", "const { ler } = require('./servico');\n\nfunction app() {\n  return ler();\n}\n"),
];

/// O mapa do projeto do alcance, montado uma vez para todos os testes dele.
fn reach_map() -> &'static Value {
    static MAP: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let temp = project_dir("alcance-do-nome");
        for (rel, body) in REACH {
            write(temp.path(), rel, body);
        }
        scan(temp.path())
    })
}

/// Quantos usos provados e quantos suspeitos a declaração `name` de `file`
/// recebe da linha `place` (`arquivo:linha`).
fn counts_at(file: &str, name: &str, place: &str) -> (usize, usize) {
    let uses = uses_of(reach_map(), file, name);
    let place = format!("{place}:");
    (
        uses.proven.iter().filter(|at| at.starts_with(&place)).count(),
        uses.suspect.iter().filter(|(at, _)| at.starts_with(&place)).count(),
    )
}

/// O nome sozinho que o próprio arquivo declara fora de todo tipo é essa
/// declaração, provada, e nenhuma outra: o `router` citado em `app.ts` é o
/// dele, e não o de `aves.ts`, que ele importa.
#[test]
fn a_bare_name_the_file_declares_is_its_own() {
    assert_eq!(counts_at("ts/src/app.ts", "router", "ts/src/app.ts:6"), (1, 0), "o router do próprio arquivo");
    assert_eq!(counts_at("ts/src/aves.ts", "router", "ts/src/app.ts:6"), (0, 0), "o router de aves.ts");
}

/// O nome da língua escrito sozinho, que o arquivo não declara nem traz pelo
/// nome, é o da língua e não liga: `open("x")` no Python e `setTimeout` no
/// TypeScript, com o arquivo que declara um de mesmo nome importado como
/// módulo.
#[test]
fn a_name_of_the_language_the_file_does_not_bring_links_nowhere() {
    assert_eq!(counts_at("py/l/util.py", "open", "py/l/a.py:6"), (0, 0), "open(\"x\")");
    assert_eq!(counts_at("ts/src/util.ts", "setTimeout", "ts/src/a.ts:4"), (0, 0), "setTimeout");
}

/// O nome escrito depois do apelido de uma biblioteca é dela e não liga:
/// `c.jsonEncode({})` no Dart, com o prefixo `c` de `dart:convert`, e, sem
/// apelido, `json.dumps({})` no Python e `strings.Join` no Go.
#[test]
fn a_name_after_a_library_alias_links_nowhere() {
    assert_eq!(counts_at("dart/lib/util.dart", "jsonEncode", "dart/lib/a.dart:5"), (0, 0), "c.jsonEncode");
    assert_eq!(counts_at("py/l/util.py", "dumps", "py/l/b.py:6"), (0, 0), "json.dumps");
    assert_eq!(counts_at("go/util/texto.go", "Join", "go/api/a.go:10"), (0, 0), "strings.Join");
}

/// O apelido que um import do projeto dá ao módulo nomeia o arquivo dele:
/// `u.join('a')` no TypeScript e `s.buscar()` no Python são provados.
#[test]
fn a_name_after_a_project_alias_is_proven() {
    assert_eq!(counts_at("ts/src/util.ts", "join", "ts/src/b.ts:4"), (1, 0), "u.join");
    assert_eq!(counts_at("py/loja/servico.py", "buscar", "py/l/c.py:5"), (1, 0), "s.buscar");
}

/// O nome sozinho sem nada à vista, declarado noutro arquivo, é suspeito,
/// mesmo declarado uma vez só: nada no arquivo diz que é aquele.
#[test]
fn a_bare_name_with_nothing_in_sight_is_suspect() {
    assert_eq!(counts_at("py/loja/servico.py", "buscar", "py/l/d.py:2"), (0, 1), "buscar() sem import");
    let uses = uses_of(reach_map(), "py/loja/servico.py", "buscar");
    let candidates: Vec<&Vec<String>> =
        uses.suspect.iter().filter(|(at, _)| at.starts_with("py/l/d.py:2:")).map(|(_, c)| c).collect();
    assert_eq!(candidates, [&vec!["py/loja/servico.py:1:buscar".to_string()]]);
}

/// O nome que o arquivo traz segue provado: `util.open(1)` com o módulo
/// trazido do pacote, `dumps(1)` trazido pelo nome, `util.Join` no Go e
/// `jsonEncode(1)` com `util.dart` importado sem prefixo.
#[test]
fn a_name_the_file_brings_stays_proven() {
    assert_eq!(counts_at("py/l/util.py", "open", "py/l/a.py:5"), (1, 0), "util.open(1)");
    assert_eq!(counts_at("py/l/util.py", "dumps", "py/l/b.py:7"), (1, 0), "dumps(1)");
    assert_eq!(counts_at("go/util/texto.go", "Join", "go/api/a.go:11"), (1, 0), "util.Join");
    assert_eq!(counts_at("dart/lib/util.dart", "jsonEncode", "dart/lib/a.dart:6"), (1, 0), "jsonEncode(1)");
}

/// O JavaScript antigo exporta a função pelo objeto `exports`: cada forma
/// declara uma função, com o nome da propriedade ou o da função, e a chamada
/// de quem a traz pelo `require` é provada. O objeto exportado com nomes já
/// declarados não declara nada de novo.
#[test]
fn an_old_style_export_is_a_function_its_importer_reaches() {
    assert_eq!(counts_at("js/servico.js", "ler", "js/app.js:4"), (1, 0), "ler()");
    let map = reach_map();
    let declared = |file: &str| -> Vec<(String, String)> {
        let m = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == file).unwrap();
        m["declarations"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| (d["name"].as_str().unwrap().to_string(), d["kind"].as_str().unwrap().to_string()))
            .collect()
    };
    let function = |name: &str| vec![(name.to_string(), "function".to_string())];
    assert_eq!(declared("js/servico.js"), function("ler"));
    assert_eq!(declared("js/outro.js"), function("gravar"));
    assert_eq!(declared("js/mais.js"), function("apagar"));
    assert_eq!(declared("js/junta.js"), function("ler"));
}

/// Os usos gravados de cada declaração do mapa, pelo arquivo e pelo nome:
/// os provados, como o texto `arquivo:linha:quem`, e os suspeitos, cada um
/// com o lugar e as candidatas.
struct Uses {
    proven: Vec<String>,
    suspect: Vec<(String, Vec<String>)>,
    common_count: u64,
}

fn uses_of(map: &Value, file: &str, name: &str) -> Uses {
    let m = map["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == file)
        .unwrap_or_else(|| panic!("{file} no mapa"));
    let d = m["declarations"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("{name} declarado em {file}: {m}"));
    let mut uses = Uses { proven: Vec::new(), suspect: Vec::new(), common_count: d["common_calls"].as_u64().unwrap_or(0) };
    for u in d["used_by"].as_array().into_iter().flatten() {
        match u.as_str() {
            Some(place) => uses.proven.push(place.to_string()),
            None => uses.suspect.push((
                u["at"].as_str().expect("o uso suspeito tem o lugar").to_string(),
                u["candidates"]
                    .as_array()
                    .expect("o uso suspeito tem as candidatas")
                    .iter()
                    .map(|c| c.as_str().unwrap().to_string())
                    .collect(),
            )),
        }
    }
    uses
}

/// Cada ligação de quem chama diz se é provada ou suspeita, no Rust, no
/// TypeScript e no Python. Com duas funções `run` em módulos diferentes, a
/// chamada no arquivo que importa uma delas é provada só para ela; a chamada
/// num arquivo que não importa nenhuma é suspeita nas duas, com as duas
/// candidatas; e o nome declarado uma vez só na linguagem, sem import e sem
/// nada à vista, é suspeito, com ela de candidata: nada no arquivo diz que é
/// ela. A chamada por uma variável (`x.run()`) é suspeita mesmo com a
/// `run` importada: sem saber o tipo da variável, ela pode ser outro método.
#[test]
fn each_link_says_whether_it_is_proven_or_suspect() {
    let temp = project_dir("ligacao-provada-ou-suspeita");
    let dir = temp.path().to_path_buf();
    let every = links();
    for l in &every {
        for (rel, body) in l.files {
            write(&dir, rel, body);
        }
    }
    let map = scan(&dir);
    let body = |l: &Links, file: &str| l.files.iter().find(|(rel, _)| *rel == file).unwrap().1;
    // Junta as faltas antes de reprovar, para que cada linguagem que voltar a
    // ligar sem marca apareça de uma vez.
    let mut failures: Vec<String> = Vec::new();
    for l in &every {
        let (run_a, run_b) = (uses_of(&map, l.a, "run"), uses_of(&map, l.b, "run"));
        let both = vec![format!("{}:1:run", l.a), format!("{}:1:run", l.b)];

        let place = format!("{}:{}", l.with_import, line_of(body(l, l.with_import), "run()"));
        let of_the_importer = |u: &Uses| {
            u.proven.iter().chain(u.suspect.iter().map(|(at, _)| at)).filter(|at| at.starts_with(&place)).count()
        };
        if run_a.proven.iter().filter(|at| at.starts_with(&place)).count() != 1
            || of_the_importer(&run_a) != 1
            || of_the_importer(&run_b) != 0
        {
            failures.push(format!("a chamada de {place}, que importa a run de {}, é provada só para ela", l.a));
        }

        let place = format!("{}:{}", l.without_import, line_of(body(l, l.without_import), "run()"));
        for (owner, uses) in [(l.a, &run_a), (l.b, &run_b)] {
            let suspect_candidates: Vec<&Vec<String>> =
                uses.suspect.iter().filter(|(at, _)| at.starts_with(&place)).map(|(_, c)| c).collect();
            if suspect_candidates != [&both] || uses.proven.iter().any(|at| at.starts_with(&place)) {
                failures.push(format!("a chamada de {place}, sem import, é suspeita em {owner} com {both:?}: {suspect_candidates:?}"));
            }
        }

        let place = format!("{}:{}", l.uses_unique, line_of(body(l, l.uses_unique), " unica()"));
        let unique = uses_of(&map, l.unique, "unica");
        let only_itself = vec![format!("{}:1:unica", l.unique)];
        let suspect_candidates: Vec<&Vec<String>> =
            unique.suspect.iter().filter(|(at, _)| at.starts_with(&place)).map(|(_, c)| c).collect();
        if suspect_candidates != [&only_itself] || !unique.proven.is_empty() {
            failures.push(format!("unica, declarada uma vez e sem import, é suspeita em {place}: {suspect_candidates:?}"));
        }

        let place = format!("{}:{}", l.by_value, line_of(body(l, l.by_value), "x.run()"));
        let only_a = vec![format!("{}:1:run", l.a)];
        let suspect_candidates: Vec<&Vec<String>> =
            run_a.suspect.iter().filter(|(at, _)| at.starts_with(&place)).map(|(_, c)| c).collect();
        if suspect_candidates != [&only_a] || run_a.proven.iter().any(|at| at.starts_with(&place)) {
            failures.push(format!("a chamada por variável de {place} é suspeita, com a run importada: {suspect_candidates:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// O próprio objeto e o tipo escrito antes do método estreitam a ligação, no
/// Rust: `self.fechar()`, dentro de `Caixa`, é provado para o `fechar` de
/// `Caixa`, e não para o de `Porta`; `Caixa::new()` é provado para o `new` de
/// `Caixa`, que o arquivo importa. `Vec::new()` nomeia um tipo que o projeto
/// não declara: é uma chamada de fora e não liga a nada.
#[test]
fn the_object_itself_and_a_named_type_narrow_the_link() {
    const BOX: &str = "pub struct Caixa;\n\nimpl Caixa {\n    pub fn abrir(&self) -> u32 {\n        self.fechar()\n    }\n\n    \
                         pub fn fechar(&self) -> u32 {\n        1\n    }\n\n    pub fn new() -> Caixa {\n        Caixa\n    }\n}\n";
    const DOOR: &str = "pub struct Porta;\n\nimpl Porta {\n    pub fn fechar(&self) -> u32 {\n        2\n    }\n\n    \
                         pub fn new() -> Porta {\n        Porta\n    }\n}\n";
    const USER: &str = "use crate::caixa::Caixa;\n\npub fn usa() -> Vec<u32> {\n    let _ = Caixa::new();\n    Vec::new()\n}\n";
    let temp = project_dir("ligacao-pelo-tipo");
    let dir = temp.path().to_path_buf();
    write(&dir, "src/caixa.rs", BOX);
    write(&dir, "src/porta.rs", DOOR);
    write(&dir, "src/usa.rs", USER);
    let map = scan(&dir);

    let close = uses_of(&map, "src/caixa.rs", "fechar");
    let expected = format!("src/caixa.rs:{}:abrir", line_of(BOX, "self.fechar()"));
    assert_eq!((close.proven, close.suspect.len()), (vec![expected], 0), "self.fechar() é o fechar de Caixa");
    let close_door = uses_of(&map, "src/porta.rs", "fechar");
    assert!(close_door.proven.is_empty() && close_door.suspect.is_empty(), "o fechar de Porta não é chamado");

    let new = uses_of(&map, "src/caixa.rs", "new");
    let expected = format!("src/usa.rs:{}:usa", line_of(USER, "Caixa::new()"));
    assert_eq!((new.proven, new.suspect.len()), (vec![expected], 0), "Caixa::new() é o new de Caixa, e Vec::new() não");
    let new_door = uses_of(&map, "src/porta.rs", "new");
    assert!(new_door.proven.is_empty() && new_door.suspect.is_empty(), "o new de Porta não é chamado");
}

/// O nome sozinho só alcança um método ou um membro de enum na língua que
/// chama o membro do próprio objeto sem escrevê-lo. No Rust, o `Ok(` e o
/// `trancar()` escritos sozinhos não são o `Ok` de um `enum` do projeto nem o
/// método `trancar` de `Caixa`, mesmo com os dois importados: nenhum ganha o
/// uso. No C#, o `Trancar()` escrito dentro da própria classe é o método dela,
/// provado.
#[test]
fn a_bare_name_reaches_a_member_only_where_the_language_calls_it_without_the_object() {
    const STATE: &str = "pub enum Estado {\n    Ok,\n    Falha,\n}\n";
    const BOX_RS: &str = "pub struct Caixa;\n\nimpl Caixa {\n    pub fn trancar(&self) -> u32 {\n        1\n    }\n}\n";
    const USER: &str = "use crate::caixa::Caixa;\nuse crate::estado::Estado;\n\npub fn usa() -> Result<u32, ()> {\n    \
                       let trancar = || 2;\n    Ok(trancar())\n}\n";
    const BOX_CS: &str = "namespace Loja;\n\npublic class Caixa\n{\n    public int Trancar()\n    {\n        return 1;\n    }\n\n    \
                            public int Abrir()\n    {\n        return Trancar();\n    }\n}\n";
    let temp = project_dir("nome-sozinho-e-membro");
    let dir = temp.path().to_path_buf();
    write(&dir, "rs/src/estado.rs", STATE);
    write(&dir, "rs/src/caixa.rs", BOX_RS);
    write(&dir, "rs/src/usa.rs", USER);
    write(&dir, "cs/Caixa.cs", BOX_CS);
    let map = scan(&dir);

    for (file, name) in [("rs/src/estado.rs", "Ok"), ("rs/src/caixa.rs", "trancar")] {
        let uses = uses_of(&map, file, name);
        assert!(
            uses.proven.is_empty() && uses.suspect.is_empty(),
            "o {name} de {file} não é o nome escrito sozinho: {:?} {:?}",
            uses.proven,
            uses.suspect
        );
    }
    let lock = uses_of(&map, "cs/Caixa.cs", "Trancar");
    let expected = format!("cs/Caixa.cs:{}:Abrir", line_of(BOX_CS, "return Trancar()"));
    assert_eq!(
        (lock.proven, lock.suspect.len()),
        (vec![expected], 0),
        "o Trancar() dentro da classe é o método dela"
    );
}

/// O nome declarado mais vezes que o teto não liga: a chamada que pode
/// alcançar nove declarações só se conta, em cada uma delas, e nenhuma ganha
/// o uso.
#[test]
fn a_name_above_the_ceiling_is_only_counted() {
    let temp = project_dir("nome-comum-so-se-conta");
    let dir = temp.path().to_path_buf();
    let owners: Vec<String> = (1..=9).map(|n| format!("src/m{n}.rs")).collect();
    for (n, owner) in owners.iter().enumerate() {
        write(&dir, owner, &format!("pub fn comum() -> u32 {{\n    {n}\n}}\n"));
    }
    write(&dir, "src/chama.rs", "pub fn chama() -> u32 {\n    comum()\n}\n");
    let map = scan(&dir);
    for owner in &owners {
        let common = uses_of(&map, owner, "comum");
        assert_eq!(
            (common.common_count, common.proven.len(), common.suspect.len()),
            (1, 0, 0),
            "o comum de {owner} só conta a chamada"
        );
    }
}

/// O teto do nome comum vem do `mustard.json` do projeto: com
/// `scan.max_same_name` 9, a mesma chamada que alcança nove declarações liga,
/// suspeita, a cada uma delas, com as nove como candidatas.
#[test]
fn the_project_ceiling_links_a_name_the_default_only_counts() {
    let temp = project_dir("teto-do-projeto");
    let dir = temp.path().to_path_buf();
    let owners: Vec<String> = (1..=9).map(|n| format!("src/m{n}.rs")).collect();
    for (n, owner) in owners.iter().enumerate() {
        write(&dir, owner, &format!("pub fn comum() -> u32 {{\n    {n}\n}}\n"));
    }
    write(&dir, "src/chama.rs", "pub fn chama() -> u32 {\n    comum()\n}\n");
    write(&dir, "mustard.json", r#"{"scan": {"max_same_name": 9}}"#);
    let map = scan(&dir);
    let candidates: Vec<String> = owners.iter().map(|owner| format!("{owner}:1:comum")).collect();
    for owner in &owners {
        let common = uses_of(&map, owner, "comum");
        assert_eq!(
            (common.common_count, common.proven.len(), common.suspect),
            (0, 0, vec![("src/chama.rs:2:chama".to_string(), candidates.clone())]),
            "o comum de {owner} liga suspeito, com as nove candidatas"
        );
    }
}

/// O projeto em que o código liga um nome que o projeto também declara: em
/// cada língua, `fill` declarada num arquivo e importada por outro, que a
/// chama numa função vizinha, e chama pelo mesmo nome uma variável e um
/// parâmetro. No Rust, a função chamada na mesma linha que dá o nome à
/// variável (`let fill = fill();`); no TypeScript, a variável chamada numa
/// função escrita dentro da que a liga. No Rust, `fs` trazido de fora por `use std::fs::{self}` ao lado
/// da pasta `fs` do projeto, e o `Result` que o projeto declara, usado sem
/// import, trazido pelo nome e trazido por `*`.
fn own_name_project() -> Vec<(&'static str, &'static str)> {
    vec![
        ("rs/src/a.rs", "pub fn fill() -> u32 {\n    1\n}\n"),
        (
            "rs/src/b.rs",
            "use crate::a::fill;\n\npub fn with_local() -> u32 {\n    let fill = |x: u32| x + 1;\n    fill(2)\n}\n\n\
             pub fn with_parameter(fill: fn() -> u32) -> u32 {\n    fill()\n}\n\n\
             pub fn same_line() -> u32 {\n    let fill = fill();\n    fill\n}\n\n\
             pub fn vizinha() -> u32 {\n    fill()\n}\n",
        ),
        ("rs/src/fs/mod.rs", "pub mod real;\n\npub struct Disco;\n\npub fn remove_dir_all() -> u32 {\n    0\n}\n"),
        (
            "rs/src/fs/real.rs",
            "use std::fs::{self};\nuse super::Disco;\n\npub fn limpa(_d: Disco) {\n    let _ = fs::remove_dir_all(\"x\");\n}\n",
        ),
        ("rs/src/erro.rs", "pub type Result<T> = std::result::Result<T, Error>;\npub struct Error;\n"),
        ("rs/src/so_erro.rs", "use crate::erro::Error;\n\npub fn so_erro() -> Result<u32, Error> {\n    Ok(1)\n}\n"),
        (
            "rs/src/with_result.rs",
            "use crate::erro::{Error, Result};\n\npub fn with_result() -> Result<u32> {\n    Err(Error)\n}\n",
        ),
        ("rs/src/with_glob.rs", "use crate::erro::*;\n\npub fn with_glob() -> Result<u32> {\n    Ok(1)\n}\n"),
        ("ts/src/a.ts", "export function fill(): number {\n  return 1;\n}\n"),
        (
            "ts/src/b.ts",
            "import { fill } from \"./a\";\n\nexport function withLocal(): number {\n  const fill = (x: number) => x + 1;\n  \
             return fill(2);\n}\n\nexport function withParameter(fill: () => number): number {\n  return fill();\n}\n\n\
             export function withDestructuring(x: { mutate: () => number }): number {\n  const { mutate: fill } = x;\n  \
             return fill();\n}\n\nexport function withNested(x: { mutate: () => number }): number {\n  \
             const { mutate: fill } = x;\n  function chama(): number {\n    return fill();\n  }\n  return chama();\n}\n\n\
             export function vizinha(): number {\n  return fill();\n}\n",
        ),
        ("py/pkg/a.py", "def fill():\n    return 1\n"),
        (
            "py/pkg/b.py",
            "from pkg.a import fill\n\n\ndef with_local():\n    fill = lambda x: x + 1\n    return fill(2)\n\n\n\
             def with_parameter(fill):\n    return fill()\n\n\ndef vizinha():\n    return fill()\n",
        ),
    ]
}

/// O nome que o próprio código liga a outra coisa vence a declaração do
/// projeto de mesmo nome. A variável, o parâmetro e o nome tirado de
/// desestruturação, chamados dentro da função que os liga, também numa função
/// escrita ali dentro, não são a `fill` importada: só a função vizinha a usa,
/// e a linha que dá o nome à variável, que ainda chama a função. `fs::remove_dir_all()`, com `fs`
/// trazido da biblioteca, não é a `remove_dir_all` da pasta `fs` do projeto.
/// O `Result` escrito sozinho é o da língua, a menos que o arquivo o traga
/// pelo nome ou por `*` do arquivo que o declara.
#[test]
fn a_name_the_code_binds_itself_wins_over_the_project_declaration() {
    let temp = project_dir("nome-proprio");
    let dir = temp.path().to_path_buf();
    let project = own_name_project();
    for (rel, body) in &project {
        write(&dir, rel, body);
    }
    let map = scan(&dir);
    let body = |file: &str| project.iter().find(|(rel, _)| *rel == file).unwrap().1;
    let mut failures: Vec<String> = Vec::new();
    for (a, b, who) in [
        ("rs/src/a.rs", "rs/src/b.rs", &["same_line", "vizinha"][..]),
        ("ts/src/a.ts", "ts/src/b.ts", &["vizinha"][..]),
        ("py/pkg/a.py", "py/pkg/b.py", &["vizinha"][..]),
    ] {
        let fill = uses_of(&map, a, "fill");
        let mut proven = fill.proven.clone();
        proven.sort();
        let mut expected_uses: Vec<String> =
            who.iter().map(|f| format!("{b}:{}:{f}", line_of(body(b), &format!("{f}()")) + 1)).collect();
        expected_uses.sort();
        if proven != expected_uses || !fill.suspect.is_empty() {
            failures.push(format!("a fill de {a} só é usada por {who:?}: {:?} {:?}", fill.proven, fill.suspect));
        }
    }
    let remove = uses_of(&map, "rs/src/fs/mod.rs", "remove_dir_all");
    if !remove.proven.is_empty() || !remove.suspect.is_empty() || remove.common_count != 0 {
        failures.push(format!(
            "fs::remove_dir_all, com fs trazido de fora, não liga: {:?} {:?}",
            remove.proven, remove.suspect
        ));
    }
    let result = uses_of(&map, "rs/src/erro.rs", "Result");
    let who = vec![
        format!("rs/src/with_glob.rs:{}:with_glob", line_of(body("rs/src/with_glob.rs"), "Result")),
        format!("rs/src/with_result.rs:{}:with_result", line_of(body("rs/src/with_result.rs"), "-> Result")),
    ];
    if result.proven != who || !result.suspect.is_empty() {
        failures.push(format!(
            "o Result do projeto só vale onde o arquivo o traz: {:?} {:?}",
            result.proven, result.suspect
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Um projeto Rust com uma pasta `fs` que declara `read_to_string`, e um
/// arquivo que a importa e chama o nome de três jeitos: pelo nome que
/// importou (l.4), pelo caminho da biblioteca (l.8) e pelo caminho inteiro do
/// projeto (l.12).
fn library_path_project() -> Vec<(&'static str, &'static str)> {
    vec![
        ("src/lib.rs", "pub mod io;\npub mod leitor;\n"),
        ("src/io/mod.rs", "pub mod fs;\n"),
        ("src/io/fs/mod.rs", "pub fn read_to_string(_: &str) -> String {\n    String::new()\n}\n"),
        (
            "src/leitor.rs",
            "use crate::io::fs;\n\npub fn through_project() -> String {\n    fs::read_to_string(\"a\")\n}\n\n\
             pub fn through_library() -> String {\n    std::fs::read_to_string(\"b\").unwrap()\n}\n\n\
             pub fn through_whole_path() -> String {\n    crate::io::fs::read_to_string(\"c\")\n}\n",
        ),
    ]
}

/// Quem usa a `read_to_string` do projeto, lida no mapa do projeto do caminho
/// da biblioteca.
fn uses_through_library_path() -> (tempfile::TempDir, Uses) {
    let temp = project_dir("caminho-da-biblioteca");
    for (rel, body) in library_path_project() {
        write(temp.path(), rel, body);
    }
    let map = scan(temp.path());
    let uses = uses_of(&map, "src/io/fs/mod.rs", "read_to_string");
    (temp, uses)
}

/// `std::fs::read_to_string("b")` é a função da biblioteca, mesmo com a pasta
/// `fs` do projeto à vista do arquivo: não liga à do projeto, nem como
/// suspeita.
#[test]
fn a_call_through_a_library_path_does_not_link_to_the_project_function() {
    let (_temp, uses) = uses_through_library_path();
    let from_library = "src/leitor.rs:8:through_library";
    assert!(!uses.proven.iter().any(|place| place == from_library), "{:?}", uses.proven);
    assert!(!uses.suspect.iter().any(|(place, _)| place == from_library), "{:?}", uses.suspect);
}

/// `fs::read_to_string("a")`, com o `fs` que o arquivo importou do projeto,
/// segue ligando à função do projeto, provada.
#[test]
fn a_call_through_a_name_imported_from_the_project_stays_proven() {
    let (_temp, uses) = uses_through_library_path();
    assert!(uses.proven.iter().any(|place| place == "src/leitor.rs:4:through_project"), "{:?}", uses.proven);
}

/// `crate::io::fs::read_to_string("c")`, o caminho inteiro do projeto, segue
/// ligando à função do projeto, provada.
#[test]
fn a_call_through_the_whole_project_path_stays_proven() {
    let (_temp, uses) = uses_through_library_path();
    assert!(uses.proven.iter().any(|place| place == "src/leitor.rs:12:through_whole_path"), "{:?}", uses.proven);
}

/// O arquivo que usa o `require`, igual no JavaScript e no TypeScript: traz
/// `ler` do projeto (l.1), `fs` inteiro (l.2) e `readFileSync` (l.3) da
/// biblioteca, e chama os três (l.6 a 8).
const APP_WITH_REQUIRE: &str = "const { ler } = require('./servico');\nconst fs = require('fs');\n\
    const { readFileSync } = require('fs');\n\nfunction criar() {\n  ler(1);\n  fs.readFileSync('z');\n  \
    readFileSync('w');\n}\n\nclass Carrinho {\n  total() {\n    return criar();\n  }\n}\n";

/// O arquivo que declara as funções que o `require` alcança, com uma de mesmo
/// nome que a da biblioteca.
const SERVICE_WITH_REQUIRE: &str = "function ler(id) {\n  return id;\n}\n\nfunction readFileSync(p) {\n  return p;\n}\n\n\
    module.exports = { ler, readFileSync };\n";

/// Um projeto com o `require` no JavaScript e no TypeScript, e um em C# cujo
/// `Util` declara `ReadAllText` e `Combine`, os nomes de `File` e `Path` da
/// biblioteca. O `Servico` chama os dois de fora (l.9 e 10), o do projeto
/// (l.11), o repositório pelo campo e pelo parâmetro (l.12 e 13) e o método do
/// tipo de cima (l.14); o `Leitor` chama pelo caminho inteiro da biblioteca.
fn library_project() -> Vec<(&'static str, &'static str)> {
    vec![
        ("js/src/app.js", APP_WITH_REQUIRE),
        ("js/src/servico.js", SERVICE_WITH_REQUIRE),
        ("ts/src/app.ts", APP_WITH_REQUIRE),
        ("ts/src/servico.ts", SERVICE_WITH_REQUIRE),
        (
            "cs/Util.cs",
            "namespace Demo;\n\npublic static class Util\n{\n    \
             public static string ReadAllText(string p) => p;\n    \
             public static string Combine(string a, string b) => a + b;\n}\n",
        ),
        ("cs/Repo.cs", "namespace Demo;\n\npublic class Repo\n{\n    public void Salvar() { }\n}\n"),
        ("cs/Base.cs", "namespace Demo;\n\npublic class Base\n{\n    public virtual void Fechar() { }\n}\n"),
        (
            "cs/Servico.cs",
            "using System.IO;\n\nnamespace Demo;\n\npublic class Servico : Base\n{\n    \
             private readonly Repo _repo = new Repo();\n\n    \
             public void A() { File.ReadAllText(\"x\"); }\n    \
             public void C() { Path.Combine(\"a\", \"b\"); }\n    \
             public void D() { Util.ReadAllText(\"z\"); }\n    \
             public void E() { _repo.Salvar(); }\n    \
             public void F(Repo repo) { repo.Salvar(); }\n    \
             public override void Fechar() { base.Fechar(); }\n}\n",
        ),
        (
            "cs/Leitor.cs",
            "namespace Demo;\n\npublic class Leitor\n{\n    \
             public void B() { System.IO.File.ReadAllText(\"y\"); }\n}\n",
        ),
        ("cs/Pedido.cs", "namespace Demo;\n\npublic partial class Pedido(Repo repositorio)\n{\n}\n"),
        (
            "cs/Pedido.Grava.cs",
            "namespace Demo;\n\npublic partial class Pedido\n{\n    public void Gravar() { repositorio.Salvar(); }\n}\n",
        ),
        ("cs/Formas.cs", SHAPES_CS),
        ("cs/Caixa.cs", BOX_CS),
        ("js/src/medida.js", "function medir(x) {\n  return x;\n}\n\nmodule.exports = { medir };\n"),
        (
            "js/src/varre.js",
            "const { medir } = require('./medida');\n\nfunction varrer(itens) {\n  \
             for (const item of itens) item.medir();\n  try { } catch (falha) { falha.medir(); }\n}\n",
        ),
    ]
}

/// Cada forma do C# de ligar um nome dentro da função, chamando o `Salvar`
/// do repositório por ele (l.7 a 14): o padrão com tipo, a variável de
/// `out`, a desestruturação, a exceção pega, o padrão de propriedades, o
/// laço que desestrutura e a consulta.
const SHAPES_CS: &str = "namespace Demo;\n\npublic class Formas\n{\n    public void Ligar(object o, string texto)\n    {\n        \
    if (o is Repo concreto) concreto.Salvar();\n        \
    if (Fabrica.Tentar(texto, out var achado)) achado.Salvar();\n        \
    var (primeiro, segundo) = Fabrica.Par();\n        primeiro.Salvar();\n        \
    try { } catch (Falha erro) { erro.Salvar(); }\n        \
    if (o is { } qualquer) qualquer.Salvar();\n        \
    foreach (var (chave, valor) in Fabrica.Pares()) valor.Salvar();\n        \
    var lista = from item in Fabrica.Lista() let dobro = item select dobro.Salvar();\n    }\n}\n";

/// Um comentário que termina em ponto logo antes de duas chamadas: a do
/// método do próprio tipo (l.8) e a do `File` da biblioteca (l.10).
const BOX_CS: &str = "namespace Demo;\n\npublic class Caixa\n{\n    public void Encerrar()\n    {\n        \
    // Soma antes de sair.\n        Somar();\n        // Lê o arquivo.\n        File.ReadAllText(\"c\");\n    }\n\n    \
    private void Somar() { }\n}\n";

/// Os lugares, provados ou suspeitos, que usam a declaração.
fn places(uses: &Uses) -> Vec<String> {
    uses.proven.iter().cloned().chain(uses.suspect.iter().map(|(place, _)| place.clone())).collect()
}

/// O arquivo `.js` entra no mapa com as declarações dele: a função e a
/// classe, com o método.
#[test]
fn a_javascript_file_enters_the_map_with_its_declarations() {
    let temp = project_dir("javascript");
    for (rel, body) in library_project() {
        write(temp.path(), rel, body);
    }
    let map = scan(temp.path());
    let app = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == "js/src/app.js");
    let app = app.unwrap_or_else(|| panic!("o app.js no mapa: {map}"));
    assert_eq!(app["language"], "javascript", "{app}");
    let names: Vec<&str> =
        app["declarations"].as_array().unwrap().iter().filter_map(|d| d["name"].as_str()).collect();
    for name in ["criar", "Carrinho", "total"] {
        assert!(names.contains(&name), "{name} declarado no app.js: {names:?}");
    }
}

/// `const { ler } = require('./servico')` põe o arquivo nas dependências e
/// liga `ler()` a ele, provada; `fs.readFileSync()` e `readFileSync()`, com
/// os nomes trazidos do `fs` pelo `require`, não ligam ao `readFileSync` do
/// projeto. Igual no JavaScript e no TypeScript.
#[test]
fn a_require_imports_like_an_import_in_javascript_and_typescript() {
    let temp = project_dir("require");
    for (rel, body) in library_project() {
        write(temp.path(), rel, body);
    }
    let map = scan(temp.path());
    let mut failures: Vec<String> = Vec::new();
    for (folder, ext) in [("js", "js"), ("ts", "ts")] {
        let app = format!("{folder}/src/app.{ext}");
        let service = format!("{folder}/src/servico.{ext}");
        let module = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == app.as_str()).unwrap();
        if !module["deps"].as_array().into_iter().flatten().any(|d| d == service.as_str()) {
            failures.push(format!("{app} depende de {service}: {}", module["deps"]));
        }
        let read_uses = uses_of(&map, &service, "ler");
        if read_uses.proven != [format!("{app}:6:criar")] || !read_uses.suspect.is_empty() {
            failures.push(format!("ler() liga a {service}, provada: {:?} {:?}", read_uses.proven, read_uses.suspect));
        }
        let from_library = places(&uses_of(&map, &service, "readFileSync"));
        if !from_library.is_empty() {
            failures.push(format!("o readFileSync do fs não liga ao de {service}: {from_library:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// No C#, `File.ReadAllText()`, `Path.Combine()` e
/// `System.IO.File.ReadAllText()` são da biblioteca e não ligam ao `Util` do
/// projeto; `Util.ReadAllText()` liga, provada, e o repositório chamado pelo
/// campo e pelo parâmetro, e o método chamado pelo tipo de cima, seguem
/// ligando.
#[test]
fn a_call_opened_by_a_name_that_is_not_of_the_project_does_not_link() {
    let temp = project_dir("biblioteca-cs");
    for (rel, body) in library_project() {
        write(temp.path(), rel, body);
    }
    let map = scan(temp.path());
    let mut failures: Vec<String> = Vec::new();
    let read = uses_of(&map, "cs/Util.cs", "ReadAllText");
    if read.proven != ["cs/Servico.cs:11:D"] || !read.suspect.is_empty() {
        failures.push(format!("só o Util.ReadAllText() liga ao do Util: {:?} {:?}", read.proven, read.suspect));
    }
    let combine = places(&uses_of(&map, "cs/Util.cs", "Combine"));
    if !combine.is_empty() {
        failures.push(format!("o Path.Combine() não liga ao do Util: {combine:?}"));
    }
    let mut save = places(&uses_of(&map, "cs/Repo.cs", "Salvar"));
    save.retain(|place| !place.starts_with("cs/Formas.cs"));
    save.sort();
    if save != ["cs/Pedido.Grava.cs:5:Gravar", "cs/Servico.cs:12:E", "cs/Servico.cs:13:F"] {
        failures.push(format!(
            "o Salvar() pelo campo, pelo parâmetro e pelo parâmetro do construtor primário noutra parte da classe liga: \
             {save:?}"
        ));
    }
    let close = places(&uses_of(&map, "cs/Base.cs", "Fechar"));
    if close != ["cs/Servico.cs:14:Fechar"] {
        failures.push(format!("o base.Fechar() liga ao do tipo de cima: {close:?}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// O nome que a função liga, em qualquer forma da língua, é do arquivo: a
/// chamada aberta por ele segue ligando. No C#, o padrão com tipo, a variável
/// de `out`, a desestruturação, a exceção pega, o padrão de propriedades, o
/// laço que desestrutura e a consulta; no JavaScript, o laço `for … of` e a
/// exceção pega.
#[test]
fn a_call_opened_by_a_name_the_function_binds_keeps_linking() {
    let temp = project_dir("formas");
    for (rel, body) in library_project() {
        write(temp.path(), rel, body);
    }
    let map = scan(temp.path());
    let mut shapes: Vec<String> = places(&uses_of(&map, "cs/Repo.cs", "Salvar"))
        .into_iter()
        .filter(|place| place.starts_with("cs/Formas.cs"))
        .collect();
    shapes.sort();
    let mut expected_shapes: Vec<String> =
        [7, 8, 10, 11, 12, 13, 14].iter().map(|l| format!("cs/Formas.cs:{l}:Ligar")).collect();
    expected_shapes.sort();
    assert_eq!(shapes, expected_shapes);
    let mut measure = places(&uses_of(&map, "js/src/medida.js", "medir"));
    measure.sort();
    assert_eq!(measure, ["js/src/varre.js:4:varrer", "js/src/varre.js:5:varrer"]);
}

/// O ponto que fecha a frase de um comentário não liga o nome de depois à
/// última palavra do comentário: `Somar()` escrito logo depois é chamada sem
/// qualificador, provada ao método do próprio tipo, e `File.ReadAllText()`
/// segue sendo da biblioteca.
#[test]
fn a_comment_that_ends_in_a_period_does_not_qualify_the_call_after_it() {
    let temp = project_dir("comentario");
    for (rel, body) in library_project() {
        write(temp.path(), rel, body);
    }
    let map = scan(temp.path());
    let sum = uses_of(&map, "cs/Caixa.cs", "Somar");
    assert_eq!((sum.proven, sum.suspect.len()), (vec!["cs/Caixa.cs:8:Encerrar".to_string()], 0));
    let read = places(&uses_of(&map, "cs/Util.cs", "ReadAllText"));
    assert!(!read.iter().any(|place| place.starts_with("cs/Caixa.cs")), "{read:?}");
}

/// Todos os arquivos de texto debaixo de `dir`, menos a pasta da compilação.
fn text_files(dir: &Path, found: &mut Vec<(std::path::PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            text_files(&path, found);
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            found.push((path, text));
        }
    }
}

/// Um teste que quebra no meio não deixa a pasta do projeto para trás: ela é
/// criada, lida pelo scan, e o teste quebra numa linha de execução separada,
/// para o teste de fora seguir e conferir que a pasta sumiu. E nenhum arquivo
/// do scan monta a pasta à mão, que é o que deixava as pastas em `/tmp` quando
/// uma conferência falhava.
#[test]
fn the_test_project_dir_vanishes_even_when_the_test_fails() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let crashing = std::thread::spawn(move || {
        let temp = project_dir("quebra-no-meio");
        let dir = temp.path().to_path_buf();
        write(&dir, "src/lib.rs", "pub fn total() -> u32 {\n    1\n}\n");
        let map = scan(&dir);
        assert!(model::path_in(&map_dir(&dir)).is_file(), "o scan grava o mapa na pasta");
        sender.send(dir).unwrap();
        assert!(map["modules"].as_array().unwrap().is_empty(), "esta conferência quebra de propósito");
    });
    assert!(crashing.join().is_err(), "o teste de dentro devia ter quebrado");
    let dir = receiver.recv().expect("a pasta foi criada antes da quebra");
    assert!(!dir.exists(), "a pasta {} ficou depois da quebra", dir.display());

    // O nome da chamada é montado em partes, para este arquivo não se acusar.
    let call = concat!("temp", "_dir", "()");
    let mut found = Vec::new();
    text_files(&manifest_dir::manifest_dir(), &mut found);
    assert!(found.iter().any(|(c, _)| c.ends_with("src/refresh.rs")), "a busca percorre o scan inteiro");
    let by_hand: Vec<String> = found
        .iter()
        .filter(|(_, text)| text.contains(call))
        .map(|(path, _)| path.display().to_string())
        .collect();
    assert!(by_hand.is_empty(), "estes arquivos montam a pasta do teste à mão: {by_hand:?}");
}

/// `Leitor::novo()` no `app`, com o `Leitor` trazido por `use demo_core::Leitor`
/// do `lib.rs` do pacote, que só o repassa de `io/leitor.rs`: é uso provado
/// do `novo` declarado lá.
#[test]
fn a_call_through_a_name_the_root_file_passes_on_is_a_proven_use() {
    let temp = project_dir("repasse");
    for (rel, body) in [
        ("Cargo.toml", "[workspace]\nmembers = [\"core\", \"app\"]\n"),
        ("core/Cargo.toml", "[package]\nname = \"demo-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("core/src/lib.rs", "pub mod io;\npub use io::leitor::Leitor;\n"),
        ("core/src/io/mod.rs", "pub mod leitor;\n"),
        ("core/src/io/leitor.rs", "pub struct Leitor;\n\nimpl Leitor {\n    pub fn novo() -> Self {\n        Leitor\n    }\n}\n"),
        ("app/Cargo.toml", "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        ("app/src/main.rs", "use demo_core::Leitor;\n\nfn main() {\n    let _ = Leitor::novo();\n}\n"),
    ] {
        write(temp.path(), rel, body);
    }
    let map = scan(temp.path());
    let uses = uses_of(&map, "core/src/io/leitor.rs", "novo");
    assert_eq!(uses.proven, vec!["app/src/main.rs:4:main".to_string()], "{:?}", uses.suspect);
}
