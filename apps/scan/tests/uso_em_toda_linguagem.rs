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
fn pasta_do_projeto(nome: &str) -> tempfile::TempDir {
    tempfile::Builder::new().prefix(&format!("scan-{nome}-")).tempdir().unwrap()
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A pasta do projeto onde o scan grava o mapa.
fn pasta_do_mapa(dir: &Path) -> PathBuf {
    dir.join(".claude")
}

fn scan(dir: &Path) -> Value {
    model::scan(dir, &pasta_do_mapa(dir), &[]).0
}

/// Uma linguagem do projeto: o arquivo que chama, o texto da chamada da
/// função, e os três arquivos com o que cada um tem dentro.
struct Linguagem {
    quem_chama: &'static str,
    chamada: &'static str,
    arquivos: [(&'static str, &'static str); 3],
}

fn linguagens() -> Vec<Linguagem> {
    vec![
        Linguagem {
            quem_chama: "rs/src/main.rs",
            chamada: "somar(1, 2)",
            arquivos: [
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
        Linguagem {
            quem_chama: "ts/src/app.ts",
            chamada: "somar(1, 2)",
            arquivos: [
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
        Linguagem {
            quem_chama: "tsx/src/app.tsx",
            chamada: "somar(1, 2)",
            arquivos: [
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
        Linguagem {
            quem_chama: "py/pkg/app.py",
            chamada: "somar(1, 2)",
            arquivos: [
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
        Linguagem {
            quem_chama: "go/conta/app.go",
            chamada: "somar(1, 2)",
            arquivos: [
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
        Linguagem {
            quem_chama: "cs/App.cs",
            chamada: "somar(1, 2)",
            arquivos: [
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
        Linguagem {
            quem_chama: "php/App.php",
            chamada: "somar(1, 2)",
            arquivos: [
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
        Linguagem {
            quem_chama: "dart/lib/app.dart",
            chamada: "somar(1, 2)",
            arquivos: [
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
fn linha_de(corpo: &str, texto: &str) -> usize {
    corpo.lines().position(|l| l.contains(texto)).expect("o texto está no arquivo") + 1
}

#[test]
fn o_uso_so_liga_a_quem_se_chama_e_a_quem_o_arquivo_enxerga() {
    let temp = pasta_do_projeto("uso-em-toda-linguagem");
    let dir = temp.path().to_path_buf();
    let todas = linguagens();
    for l in &todas {
        for (rel, corpo) in l.arquivos {
            write(&dir, rel, corpo);
        }
    }
    let map = scan(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let lingua: HashMap<&str, &str> =
        modules.iter().map(|m| (m["path"].as_str().unwrap(), m["language"].as_str().unwrap())).collect();
    let usos = |d: &Value| -> Vec<String> {
        d.get("used_by")
            .and_then(Value::as_array)
            .map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    let declaracoes = |nome: &str| -> Vec<(String, Value)> {
        modules
            .iter()
            .flat_map(|m| {
                m["declarations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(move |d| d["name"] == nome)
                    .map(move |d| (m["path"].as_str().unwrap().to_string(), d.clone()))
            })
            .collect()
    };

    // Em cada linguagem, a função chamada tem um uso só: o do arquivo e da
    // linha da chamada. A declaração de onde a chamada parte fica fora da
    // conferência: no Dart, o scan ainda não sabe onde a função termina.
    let somar = declaracoes("somar");
    for l in &todas {
        let corpo = l.arquivos.iter().find(|(rel, _)| *rel == l.quem_chama).unwrap().1;
        let lugar = format!("{}:{}", l.quem_chama, linha_de(corpo, l.chamada));
        let dono = l.arquivos[1].0;
        let (_, d) = somar.iter().find(|(p, _)| p == dono).unwrap_or_else(|| panic!("somar declarada em {dono}"));
        let u = usos(d);
        assert!(
            u.len() == 1 && (u[0] == lugar || u[0].starts_with(&format!("{lugar}:"))),
            "o uso da função chamada, em {dono}, é {lugar}: {d}"
        );
    }

    // O campo `kind` não se chama: nenhum uso, em nenhuma linguagem que o
    // declara como campo ou propriedade.
    let kind = declaracoes("kind");
    assert!(kind.len() >= 7, "o campo kind está declarado nas linguagens que leem campo: {kind:?}");
    for (p, d) in &kind {
        assert!(usos(d).is_empty(), "o campo kind de {p} não tem uso: {d}");
    }

    // O `join` do terceiro arquivo, que quem chama não enxerga, não tem uso.
    let join = declaracoes("join");
    assert_eq!(join.len(), todas.len(), "um join por linguagem: {join:?}");
    for (p, d) in &join {
        assert!(usos(d).is_empty(), "o join de {p} não tem uso: {d}");
    }

    // Nenhum uso liga arquivos de linguagens diferentes.
    for m in modules {
        let dono = m["path"].as_str().unwrap();
        for d in m["declarations"].as_array().into_iter().flatten() {
            for u in usos(d) {
                let de = u.split(':').next().unwrap();
                assert_eq!(lingua.get(de), lingua.get(dono), "o uso {u} de {} em {dono}", d["name"]);
            }
        }
    }

}

/// O que o Rust ganha a mais para o uso sem chamada: uma constante e uma
/// função em `preco.rs`; `pedido.rs` importa a constante e a compara dentro de
/// `abrir`; `caixa.rs`, sem nenhum `use`, chama a função pelo caminho do
/// arquivo dentro de `pagar`.
const PRECO: &str = "pub const LIMITE: u32 = 10;\n\npub fn total(a: u32, b: u32) -> u32 {\n    a + b\n}\n";
const PEDIDO: &str = "use crate::preco::LIMITE;\n\npub fn abrir(n: u32) -> bool {\n    n > LIMITE\n}\n";
const CAIXA: &str = "pub fn pagar() -> u32 {\n    crate::preco::total(1, 2)\n}\n";

#[test]
fn a_constante_e_o_tipo_citados_ganham_quem_os_usa() {
    let temp = pasta_do_projeto("citacao-em-toda-linguagem");
    let dir = temp.path().to_path_buf();
    let todas = linguagens();
    for l in &todas {
        for (rel, corpo) in l.arquivos {
            write(&dir, rel, corpo);
        }
    }
    write(&dir, "rs/src/preco.rs", PRECO);
    write(&dir, "rs/src/pedido.rs", PEDIDO);
    write(&dir, "rs/src/caixa.rs", CAIXA);
    let map = scan(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let usos = |arquivo: &str, nome: &str| -> Vec<String> {
        let m = modules.iter().find(|m| m["path"] == arquivo).unwrap_or_else(|| panic!("{arquivo} no mapa"));
        let d = m["declarations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["name"] == nome)
            .unwrap_or_else(|| panic!("{nome} declarado em {arquivo}: {m}"));
        d.get("used_by")
            .and_then(Value::as_array)
            .map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    // Junta as faltas antes de reprovar, para que um defeito só na leitura da
    // citação ou só no caminho pelo arquivo apareça inteiro de uma vez.
    let mut faltas: Vec<String> = Vec::new();

    // Em cada linguagem, o tipo citado no parâmetro de `principal` tem o uso
    // com o arquivo e a linha da citação, e `principal` como quem usa.
    for l in &todas {
        let corpo = l.arquivos.iter().find(|(rel, _)| *rel == l.quem_chama).unwrap().1;
        let esperado = format!("{}:{}:principal", l.quem_chama, linha_de(corpo, "principal("));
        let u = usos(l.arquivos[1].0, "Item");
        if !u.contains(&esperado) {
            faltas.push(format!("o tipo Item de {} tem o uso {esperado}: {u:?}", l.arquivos[1].0));
        }
    }

    // A constante comparada dentro de `abrir` tem o uso, com `abrir` como quem
    // usa. A linha do `use` que a importa não conta: o nome ali é o caminho do
    // import, não um uso.
    let limite = usos("rs/src/preco.rs", "LIMITE");
    let esperado = format!("rs/src/pedido.rs:{}:abrir", linha_de(PEDIDO, "n > LIMITE"));
    if limite != [esperado.clone()] {
        faltas.push(format!("a constante LIMITE tem só o uso {esperado}: {limite:?}"));
    }

    // A função chamada pelo caminho do arquivo, sem `use`, tem o uso na linha
    // da chamada, com `pagar` como quem usa.
    let total = usos("rs/src/preco.rs", "total");
    let esperado = format!("rs/src/caixa.rs:{}:pagar", linha_de(CAIXA, "crate::preco::total("));
    if total != [esperado.clone()] {
        faltas.push(format!("a função total tem só o uso {esperado}: {total:?}"));
    }

    assert!(faltas.is_empty(), "{}", faltas.join("\n"));
}

/// O projeto do que cada linguagem põe à vista. No C#, dois projetos: `Loja`,
/// com um `global using` num arquivo só, e `Outro`, fora dele; `Pedido.cs`, em
/// `Loja.Pedidos`, não tem `using` nenhum. No TypeScript, um pacote `@loja/core`
/// cujo código mora em `src/`, importado por outro pacote pelo nome. No Go,
/// duas pastas com o mesmo `package util`. No Dart, um import de arquivo sem
/// `./`, ao lado de um pacote Go com o mesmo nome do arquivo; e um arquivo
/// Python que importa esse mesmo nome, que no Python não é de ninguém.
fn projeto_a_vista() -> Vec<(&'static str, &'static str)> {
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
fn cada_arquivo_enxerga_o_que_a_linguagem_poe_a_vista() {
    let temp = pasta_do_projeto("o-que-a-linguagem-poe-a-vista");
    let dir = temp.path().to_path_buf();
    let arquivos = projeto_a_vista();
    for (rel, corpo) in &arquivos {
        write(&dir, rel, corpo);
    }
    let map = scan(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let modulo = |arquivo: &str| -> &Value {
        modules.iter().find(|m| m["path"] == arquivo).unwrap_or_else(|| panic!("{arquivo} no mapa"))
    };
    let lista = |v: &Value| -> Vec<String> {
        v.as_array().map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    // O lugar de cada uso, provado ou suspeito: aqui se confere o que fica à
    // vista de cada arquivo, e a chamada por uma variável (`c.Total(1)`) é
    // suspeita mesmo com uma candidata só.
    let usos = |arquivo: &str, nome: &str| -> Vec<String> {
        let m = modulo(arquivo);
        let d = m["declarations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["name"] == nome)
            .unwrap_or_else(|| panic!("{nome} declarado em {arquivo}: {m}"));
        d["used_by"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|u| u.as_str().or_else(|| u["at"].as_str()).unwrap().to_string())
            .collect::<Vec<String>>()
    };
    let deps = |arquivo: &str| lista(&modulo(arquivo)["deps"]);
    let corpo = |arquivo: &str| arquivos.iter().find(|(rel, _)| *rel == arquivo).unwrap().1;
    // Junta as faltas antes de reprovar, para que cada parte que voltar a ser
    // como antes apareça de uma vez.
    let mut faltas: Vec<String> = Vec::new();

    // O `global using` de Loja/GlobalUsings.cs vale para todo arquivo C# do
    // projeto Loja: Pedido.cs usa Total sem `using`, e Conta.cs, do projeto
    // Outro, não o enxerga.
    let pedido = "Loja/Pedidos/Pedido.cs";
    let total = usos("Loja/Dominio/Calculadora.cs", "Total");
    let esperado = format!("{pedido}:{}:Fechar", linha_de(corpo(pedido), "c.Total(1)"));
    if !total.contains(&esperado) || total.iter().any(|u| u.starts_with("Outro/Conta.cs:")) {
        faltas.push(format!("Total tem o uso {esperado} e nenhum de Outro/Conta.cs: {total:?}"));
    }
    // O import global fica guardado só no arquivo que o escreve, e a aresta do
    // grafo de import, só nele. Como todo import de namespace, ela liga só aos
    // arquivos que declaram um nome que o próprio arquivo usa: GlobalUsings.cs
    // não usa nada e não liga a nada, e Pedido.cs, que usa Calculadora pelo
    // import global, não ganha aresta por ele.
    let globais = lista(&modulo("Loja/GlobalUsings.cs")["global_imports"]);
    if globais != ["Loja.Dominio"] {
        faltas.push(format!("GlobalUsings.cs guarda o import global Loja.Dominio: {globais:?}"));
    }
    if let Some(m) = modules.iter().find(|m| m["path"] != "Loja/GlobalUsings.cs" && m.get("global_imports").is_some()) {
        faltas.push(format!("só GlobalUsings.cs grava import global: {}", m["path"]));
    }
    if !deps("Loja/GlobalUsings.cs").is_empty() || deps(pedido).contains(&"Loja/Dominio/Calculadora.cs".to_string()) {
        faltas.push(format!(
            "nem GlobalUsings.cs, que não usa nada, nem Pedido.cs têm aresta para Calculadora.cs: {:?} / {:?}",
            deps("Loja/GlobalUsings.cs"),
            deps(pedido)
        ));
    }

    // O namespace de cima: Pedido.cs, em Loja.Pedidos, enxerga Loja.
    let arredondar = usos("Loja/Regra.cs", "Arredondar");
    let esperado = format!("{pedido}:{}:Fechar", linha_de(corpo(pedido), "r.Arredondar(2)"));
    if !arredondar.contains(&esperado) {
        faltas.push(format!("Arredondar tem o uso {esperado}: {arredondar:?}"));
    }
    // O nome escrito dentro do `namespace Loja.Pedidos;` não é uso da classe
    // Pedidos, que agora está à vista.
    let linha_do_namespace = format!("{pedido}:{}", linha_de(corpo(pedido), "namespace Loja.Pedidos;"));
    let pedidos = usos("Loja/Pedidos.cs", "Pedidos");
    if pedidos.iter().any(|u| u == &linha_do_namespace || u.starts_with(&format!("{linha_do_namespace}:"))) {
        faltas.push(format!("a classe Pedidos não tem uso na linha do namespace: {pedidos:?}"));
    }

    // O pacote do projeto com escopo: `@loja/core/server/preco` acha o arquivo
    // em `src/server/`, onde o package.json põe o código.
    let web = "apps/web/src/pedido.ts";
    let total_ts = usos("packages/core/src/server/preco.ts", "total");
    if !total_ts.iter().any(|u| u.starts_with(&format!("{web}:"))) {
        faltas.push(format!("o total do TypeScript tem o uso em {web}: {total_ts:?}"));
    }
    if !deps(web).contains(&"packages/core/src/server/preco.ts".to_string()) {
        faltas.push(format!("{web} tem packages/core/src/server/preco.ts nos deps: {:?}", deps(web)));
    }

    // O pacote do Go é a pasta: o Dobro de b/util não ganha o uso de a/util.
    let dobro_a = usos("a/util/x.go", "Dobro");
    let dobro_b = usos("b/util/y.go", "Dobro");
    if dobro_a.is_empty() || dobro_a.iter().any(|u| !u.starts_with("a/util/usa.go:")) || !dobro_b.is_empty() {
        faltas.push(format!("só o Dobro de a/util/x.go tem o uso de a/util/usa.go: {dobro_a:?} / {dobro_b:?}"));
    }

    // O import de arquivo sem `./` no Dart é o arquivo ao lado, e não o pacote
    // Go `conta`; nem o `import conta` do Python responde com esse pacote.
    let total_dart = usos("dart/lib/conta.dart", "total");
    if !total_dart.iter().any(|u| u.starts_with("dart/lib/pedido.dart:")) {
        faltas.push(format!("o total de dart/lib/conta.dart tem o uso de dart/lib/pedido.dart: {total_dart:?}"));
    }
    let total_go = usos("conta/conta.go", "total");
    if !total_go.is_empty() {
        faltas.push(format!("o total de conta/conta.go não tem uso: {total_go:?}"));
    }
    if deps("dart/lib/pedido.dart") != ["dart/lib/conta.dart"] {
        faltas.push(format!("o único dos deps de dart/lib/pedido.dart é dart/lib/conta.dart: {:?}", deps("dart/lib/pedido.dart")));
    }
    if !deps("py/caixa.py").is_empty() {
        faltas.push(format!("o import conta do Python não acha o pacote Go: {:?}", deps("py/caixa.py")));
    }

    assert!(faltas.is_empty(), "{}", faltas.join("\n"));
}

/// A biblioteca Dart do projeto: `lib/caixa.dart` tem a função `dobro`, uma
/// classe com construtor com corpo, construtor nomeado, factory, getter e
/// setter, uma extensão com um método e um enum com um método, todos com
/// corpo em várias linhas que chamam `dobro`; `lib/caixa_parte.dart` é parte
/// dela pelo arquivo. `lib/conta.dart` dá nome à biblioteca, e
/// `lib/conta_parte.dart` é parte dela pelo nome.
const CAIXA_DART: &str = "part 'caixa_parte.dart';\n\nint dobro(int n) {\n  return n * 2;\n}\n\n\
class Caixa {\n  int _v = 0;\n\n  Caixa(int n) {\n    _v = dobro(n);\n  }\n\n  \
Caixa.vazia() {\n    _v = dobro(0);\n  }\n\n  factory Caixa.de(int n) {\n    return Caixa(dobro(n));\n  }\n\n  \
int get valor {\n    return dobro(_v);\n  }\n\n  set valor(int v) {\n    _v = dobro(v);\n  }\n}\n\n\
extension Metade on int {\n  int metade() {\n    return dobro(this) ~/ 4;\n  }\n}\n\n\
enum Cor {\n  azul;\n\n  int peso() {\n    return dobro(1);\n  }\n}\n";
const CAIXA_PARTE_DART: &str = "part of 'caixa.dart';\n\nint extra() {\n  return dobro(3);\n}\n";
const CONTA_DART: &str = "library loja.conta;\n\npart 'conta_parte.dart';\n\nint triplo(int n) {\n  return n * 3;\n}\n";
const CONTA_PARTE_DART: &str = "part of loja.conta;\n\nint usa() {\n  return triplo(1);\n}\n";

/// A linha da chave que fecha o corpo que abre na linha do cabeçalho: a
/// primeira, depois dele, que tem só `}` com o mesmo recuo.
fn fim_do_corpo(corpo: &str, cabecalho: &str) -> usize {
    let inicio = linha_de(corpo, cabecalho);
    let linhas: Vec<&str> = corpo.lines().collect();
    let recuo = &linhas[inicio - 1][..linhas[inicio - 1].len() - linhas[inicio - 1].trim_start().len()];
    let fecha = format!("{recuo}}}");
    inicio + linhas[inicio..].iter().position(|l| *l == fecha).expect("o corpo fecha") + 1
}

#[test]
fn o_dart_termina_cada_declaracao_no_fim_do_corpo_e_a_parte_enxerga_o_dono() {
    let temp = pasta_do_projeto("dart-fim-do-corpo");
    let dir = temp.path().to_path_buf();
    write(&dir, "lib/caixa.dart", CAIXA_DART);
    write(&dir, "lib/caixa_parte.dart", CAIXA_PARTE_DART);
    write(&dir, "lib/conta.dart", CONTA_DART);
    write(&dir, "lib/conta_parte.dart", CONTA_PARTE_DART);
    let map = scan(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let modulo = |arquivo: &str| -> &Value {
        modules.iter().find(|m| m["path"] == arquivo).unwrap_or_else(|| panic!("{arquivo} no mapa"))
    };
    let lista = |v: &Value| -> Vec<String> {
        v.as_array().map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    let caixa = modulo("lib/caixa.dart");
    let declaracoes = caixa["declarations"].as_array().expect("declarations");
    let mut faltas: Vec<String> = Vec::new();

    // Cada declaração com corpo, de quem é o uso de `dobro` dentro dela: o
    // nome da própria declaração, o cabeçalho e o texto da chamada.
    let membros = [
        ("dobro", "int dobro(int n) {", None),
        ("Caixa", "  Caixa(int n) {", Some("_v = dobro(n);")),
        ("vazia", "Caixa.vazia() {", Some("_v = dobro(0);")),
        ("de", "factory Caixa.de(int n) {", Some("return Caixa(dobro(n));")),
        ("valor", "int get valor {", Some("return dobro(_v);")),
        ("valor", "set valor(int v) {", Some("_v = dobro(v);")),
        ("metade", "int metade() {", Some("return dobro(this) ~/ 4;")),
        ("peso", "int peso() {", Some("return dobro(1);")),
    ];
    let mut esperado: Vec<String> = Vec::new();
    for (nome, cabecalho, chamada) in membros {
        let linha = linha_de(CAIXA_DART, cabecalho);
        let fim = fim_do_corpo(CAIXA_DART, cabecalho);
        match declaracoes.iter().find(|d| d["name"] == nome && d["line"] == linha) {
            Some(d) if d["end_line"] == fim => {}
            Some(d) => faltas.push(format!("{nome}, da linha {linha}, termina na linha {fim}: {d}")),
            None => faltas.push(format!("{nome} é declarado na linha {linha}")),
        }
        if let Some(chamada) = chamada {
            esperado.push(format!("lib/caixa.dart:{}:{nome}", linha_de(CAIXA_DART, chamada)));
        }
    }
    // O arquivo `part of 'caixa.dart';` divide a biblioteca com o dono, e a
    // chamada de lá liga ao `dobro` daqui.
    esperado.push(format!("lib/caixa_parte.dart:{}:extra", linha_de(CAIXA_PARTE_DART, "dobro(3)")));
    let dobro = declaracoes.iter().find(|d| d["name"] == "dobro").expect("dobro declarado");
    let mut usos = lista(&dobro["used_by"]);
    usos.sort();
    esperado.sort();
    if usos != esperado {
        faltas.push(format!("cada uso de dobro vem da própria declaração, nunca da classe nem do enum: {usos:?}"));
    }

    // O cabeçalho do setter declara `valor`, e não o chama.
    let setter = linha_de(CAIXA_DART, "set valor(int v)");
    let chamadas = lista(&caixa["calls"]);
    if chamadas.iter().any(|c| c == &format!("valor:{setter}")) {
        faltas.push(format!("não há chamada valor na linha {setter}, do setter: {chamadas:?}"));
    }

    // A parte pelo nome da biblioteca, `part of loja.conta;`, enxerga o dono
    // que se declara `library loja.conta;`.
    let conta = modulo("lib/conta.dart");
    let triplo = conta["declarations"].as_array().into_iter().flatten().find(|d| d["name"] == "triplo");
    let usos_triplo = triplo.map(|d| lista(&d["used_by"])).unwrap_or_default();
    let uso = format!("lib/conta_parte.dart:{}:usa", linha_de(CONTA_PARTE_DART, "triplo(1)"));
    if !usos_triplo.contains(&uso) {
        faltas.push(format!("triplo tem o uso {uso}: {usos_triplo:?}"));
    }

    assert!(faltas.is_empty(), "{}", faltas.join("\n"));
}

/// O projeto da citação que liga pelo que o projeto declara. Em cada uma das
/// linguagens C#, Go, Python, PHP e Dart, uma constante é declarada num
/// arquivo e citada sem chamar em outro que o enxerga; no Go, com minúscula,
/// noutro arquivo do mesmo pacote. No TypeScript, `limiteDiario` é importada
/// pelo nome e citada depois, `taxaPadrao` é citada no próprio arquivo, e
/// `parcial` é uma variável de dentro da função. O C# cita `DateTime.Today`,
/// que nenhum arquivo do projeto declara. E três linhas declaram duas
/// constantes cada uma.
fn projeto_da_citacao() -> Vec<(&'static str, &'static str)> {
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
fn ler(dir: &Path, extra: &[&str]) -> (Value, Vec<u8>, bool) {
    let (mapa, relato) = model::scan(dir, &pasta_do_mapa(dir), extra);
    (mapa, model::read_bytes(&pasta_do_mapa(dir)), relato["full"] == Value::Bool(true))
}

#[test]
fn a_citacao_liga_pelo_que_o_projeto_declara_e_nao_pela_letra() {
    let temp = pasta_do_projeto("citacao-que-liga");
    let dir = temp.path().to_path_buf();
    let projeto = projeto_da_citacao();
    for (rel, corpo) in &projeto {
        write(&dir, rel, corpo);
    }
    git(&dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "primeiro"]);
    let corpo = |rel: &str| projeto.iter().find(|(p, _)| *p == rel).unwrap().1;

    let (map, _, _) = ler(&dir, &["--all"]);
    let modules = map["modules"].as_array().expect("modules").clone();
    let modulo = |arquivo: &str| -> Value {
        modules.iter().find(|m| m["path"] == arquivo).cloned().unwrap_or_else(|| panic!("{arquivo} no mapa"))
    };
    let lista = |v: &Value| -> Vec<String> {
        v.as_array().map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    let declaracao = |arquivo: &str, nome: &str| -> Option<Value> {
        modulo(arquivo)["declarations"].as_array().into_iter().flatten().find(|d| d["name"] == nome).cloned()
    };
    // Junta as faltas antes de reprovar, para que cada regra que quebra
    // apareça inteira de uma vez.
    let mut faltas: Vec<String> = Vec::new();

    // Em cada linguagem, a constante tem o tipo const e o uso com o arquivo e
    // a linha da citação, e a declaração de onde a citação vem. No
    // TypeScript, só a linha da citação: a do import não é uso.
    let constantes = [
        ("cs/Regras.cs", "Limite", "cs/Pedido.cs", "Regras.Limite", "Pode"),
        ("go/conta/limites.go", "limite", "go/conta/pedido.go", "valor <= limite", "pode"),
        ("py/loja/regras.py", "LIMITE", "py/loja/pedido.py", "valor <= LIMITE", "pode"),
        ("php/Regras.php", "LIMITE", "php/Pedido.php", "Regras::LIMITE", "pode"),
        ("dart/lib/regras.dart", "limite", "dart/lib/pedido.dart", "valor <= limite", "pode"),
        ("dart/lib/regras.dart", "teto", "dart/lib/pedido.dart", "Regras.teto", "pode"),
        ("src/limites.ts", "limiteDiario", "src/pedido.ts", "valor <= limiteDiario", "podeComprar"),
        ("src/taxa.ts", "taxaPadrao", "src/taxa.ts", "parcial + taxaPadrao", "calcular"),
    ];
    for (dono, nome, quem, citacao, de) in constantes {
        let esperado = vec![format!("{quem}:{}:{de}", linha_de(corpo(quem), citacao))];
        match declaracao(dono, nome) {
            Some(d) if d["kind"] == "const" && lista(&d["used_by"]) == esperado => {}
            Some(d) => faltas.push(format!("{nome}, de {dono}, é const com só o uso {esperado:?}: {d}")),
            None => faltas.push(format!("{nome} está declarado em {dono}")),
        }
    }

    // A variável de dentro da função e o nome da biblioteca que o projeto não
    // declara não ficam entre as citações do arquivo.
    for (arquivo, nome) in [("src/taxa.ts", "parcial"), ("cs/Pedido.cs", "DateTime")] {
        let citacoes = lista(&modulo(arquivo)["cites"]);
        let achadas: Vec<&String> = citacoes
            .iter()
            .filter(|c| c.starts_with(&format!("{nome}:")) || c.contains(&format!(".{nome}:")))
            .collect();
        if !achadas.is_empty() {
            faltas.push(format!("{nome} não liga a nada do projeto e não fica em {arquivo}: {citacoes:?}"));
        }
    }

    // Cada linha que declara duas constantes dá as duas, cada uma com o
    // próprio nome, o tipo const e o próprio cabeçalho.
    let pares = [
        ("src/par.ts", [("a", "export const a"), ("b", "export const b")]),
        ("cs/Regras.cs", [("A", "public const int A"), ("B", "public const int B")]),
        ("php/Regras.php", [("A", "const A"), ("B", "const B")]),
    ];
    for (arquivo, par) in pares {
        for (nome, cabecalho) in par {
            match declaracao(arquivo, nome) {
                Some(d) if d["kind"] == "const" && d["signature"] == cabecalho => {}
                Some(d) => faltas.push(format!("{nome}, de {arquivo}, é const com o cabeçalho {cabecalho}: {d}")),
                None => faltas.push(format!("{nome} está declarado em {arquivo}")),
            }
        }
    }

    // O arquivo que muda passa a declarar o tipo que `src/usa.ts`, que não
    // mudou, já citava: a leitura que relê só o que mudou dá o uso, e o mesmo
    // mapa que a leitura inteira.
    write(&dir, "src/tipos.ts", "export interface Velho {\n  nome: string;\n}\n\nexport interface Novo {\n  id: number;\n}\n");
    let (passo, bytes_passo, inteira) = ler(&dir, &[]);
    if inteira {
        faltas.push("a segunda leitura relê só o que mudou".to_string());
    }
    let usa = corpo("src/usa.ts");
    let esperado = vec![format!("src/usa.ts:{}:usar", linha_de(usa, "x: Novo"))];
    let novo = passo["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["path"] == "src/tipos.ts")
        .flat_map(|m| m["declarations"].as_array().cloned().unwrap_or_default())
        .find(|d| d["name"] == "Novo");
    match novo {
        Some(d) if lista(&d["used_by"]) == esperado => {}
        other => faltas.push(format!("o tipo novo tem o uso {esperado:?} do arquivo que não mudou: {other:?}")),
    }
    let (_, bytes_inteira, _) = ler(&dir, &["--all"]);
    if bytes_passo != bytes_inteira {
        faltas.push("a leitura que relê só o que mudou dá o mesmo mapa que a leitura inteira".to_string());
    }

    assert!(faltas.is_empty(), "{}", faltas.join("\n"));
}

/// Uma linguagem do projeto das ligações provadas e suspeitas: a pasta dela e
/// os arquivos, cada um com o que tem dentro. Em cada uma, `run` é declarada
/// em dois módulos, `a` e `b`; `com_import` importa a de `a` e a chama;
/// `sem_import` a chama sem importar nada; `usa_unica` chama `unica`,
/// declarada uma vez só na linguagem, sem importar nada; e `por_valor`
/// importa a `run` de `a` e a chama por uma variável.
struct Ligacoes {
    a: &'static str,
    b: &'static str,
    com_import: &'static str,
    sem_import: &'static str,
    unica: &'static str,
    usa_unica: &'static str,
    por_valor: &'static str,
    arquivos: [(&'static str, &'static str); 7],
}

fn ligacoes() -> Vec<Ligacoes> {
    vec![
        Ligacoes {
            a: "rs/src/a.rs",
            b: "rs/src/b.rs",
            com_import: "rs/src/com_import.rs",
            sem_import: "rs/src/sem_import.rs",
            unica: "rs/src/unica.rs",
            usa_unica: "rs/src/usa_unica.rs",
            por_valor: "rs/src/por_valor.rs",
            arquivos: [
                ("rs/src/a.rs", "pub fn run() -> u32 {\n    1\n}\n"),
                ("rs/src/b.rs", "pub fn run() -> u32 {\n    2\n}\n"),
                ("rs/src/com_import.rs", "use crate::a::run;\n\npub fn com_import() -> u32 {\n    run()\n}\n"),
                ("rs/src/sem_import.rs", "pub fn sem_import() -> u32 {\n    run()\n}\n"),
                ("rs/src/unica.rs", "pub fn unica() -> u32 {\n    3\n}\n"),
                ("rs/src/usa_unica.rs", "pub fn usa_unica() -> u32 {\n    unica()\n}\n"),
                ("rs/src/por_valor.rs", "use crate::a::run;\n\npub fn por_valor(x: Tarefa) -> u32 {\n    x.run()\n}\n"),
            ],
        },
        Ligacoes {
            a: "ts/src/a/tarefa.ts",
            b: "ts/src/b/tarefa.ts",
            com_import: "ts/src/com_import.ts",
            sem_import: "ts/src/sem_import.ts",
            unica: "ts/src/unica.ts",
            usa_unica: "ts/src/usa_unica.ts",
            por_valor: "ts/src/por_valor.ts",
            arquivos: [
                ("ts/src/a/tarefa.ts", "export function run(): number {\n  return 1;\n}\n"),
                ("ts/src/b/tarefa.ts", "export function run(): number {\n  return 2;\n}\n"),
                (
                    "ts/src/com_import.ts",
                    "import { run } from \"./a/tarefa\";\n\nexport function comImport(): number {\n  return run();\n}\n",
                ),
                ("ts/src/sem_import.ts", "export function semImport(): number {\n  return run();\n}\n"),
                ("ts/src/unica.ts", "export function unica(): number {\n  return 3;\n}\n"),
                ("ts/src/usa_unica.ts", "export function usaUnica(): number {\n  return unica();\n}\n"),
                (
                    "ts/src/por_valor.ts",
                    "import { run } from \"./a/tarefa\";\n\nexport function porValor(x: Tarefa): number {\n  \
                     return x.run();\n}\n",
                ),
            ],
        },
        Ligacoes {
            a: "py/pkg/a/tarefa.py",
            b: "py/pkg/b/tarefa.py",
            com_import: "py/pkg/com_import.py",
            sem_import: "py/pkg/sem_import.py",
            unica: "py/pkg/unica.py",
            usa_unica: "py/pkg/usa_unica.py",
            por_valor: "py/pkg/por_valor.py",
            arquivos: [
                ("py/pkg/a/tarefa.py", "def run():\n    return 1\n"),
                ("py/pkg/b/tarefa.py", "def run():\n    return 2\n"),
                ("py/pkg/com_import.py", "from pkg.a.tarefa import run\n\n\ndef com_import():\n    return run()\n"),
                ("py/pkg/sem_import.py", "def sem_import():\n    return run()\n"),
                ("py/pkg/unica.py", "def unica():\n    return 3\n"),
                ("py/pkg/usa_unica.py", "def usa_unica():\n    return unica()\n"),
                ("py/pkg/por_valor.py", "from pkg.a.tarefa import run\n\n\ndef por_valor(x):\n    return x.run()\n"),
            ],
        },
    ]
}

/// Os usos gravados de cada declaração do mapa, pelo arquivo e pelo nome:
/// os provados, como o texto `arquivo:linha:quem`, e os suspeitos, cada um
/// com o lugar e as candidatas.
struct Usos {
    provados: Vec<String>,
    suspeitos: Vec<(String, Vec<String>)>,
    comuns: u64,
}

fn usos_de(map: &Value, arquivo: &str, nome: &str) -> Usos {
    let m = map["modules"]
        .as_array()
        .expect("modules")
        .iter()
        .find(|m| m["path"] == arquivo)
        .unwrap_or_else(|| panic!("{arquivo} no mapa"));
    let d = m["declarations"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|d| d["name"] == nome)
        .unwrap_or_else(|| panic!("{nome} declarado em {arquivo}: {m}"));
    let mut usos = Usos { provados: Vec::new(), suspeitos: Vec::new(), comuns: d["common_calls"].as_u64().unwrap_or(0) };
    for u in d["used_by"].as_array().into_iter().flatten() {
        match u.as_str() {
            Some(lugar) => usos.provados.push(lugar.to_string()),
            None => usos.suspeitos.push((
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
    usos
}

/// Cada ligação de quem chama diz se é provada ou suspeita, no Rust, no
/// TypeScript e no Python. Com duas funções `run` em módulos diferentes, a
/// chamada no arquivo que importa uma delas é provada só para ela; a chamada
/// num arquivo que não importa nenhuma é suspeita nas duas, com as duas
/// candidatas; e o nome declarado uma vez só na linguagem é provado mesmo sem
/// import. A chamada por uma variável (`x.run()`) é suspeita mesmo com a
/// `run` importada: sem saber o tipo da variável, ela pode ser outro método.
#[test]
fn each_link_says_whether_it_is_proven_or_suspect() {
    let temp = pasta_do_projeto("ligacao-provada-ou-suspeita");
    let dir = temp.path().to_path_buf();
    let todas = ligacoes();
    for l in &todas {
        for (rel, corpo) in l.arquivos {
            write(&dir, rel, corpo);
        }
    }
    let map = scan(&dir);
    let corpo = |l: &Ligacoes, arquivo: &str| l.arquivos.iter().find(|(rel, _)| *rel == arquivo).unwrap().1;
    // Junta as faltas antes de reprovar, para que cada linguagem que voltar a
    // ligar sem marca apareça de uma vez.
    let mut faltas: Vec<String> = Vec::new();
    for l in &todas {
        let (run_a, run_b) = (usos_de(&map, l.a, "run"), usos_de(&map, l.b, "run"));
        let duas = vec![format!("{}:1:run", l.a), format!("{}:1:run", l.b)];

        let lugar = format!("{}:{}", l.com_import, linha_de(corpo(l, l.com_import), "run()"));
        let de_quem_importa = |u: &Usos| {
            u.provados.iter().chain(u.suspeitos.iter().map(|(at, _)| at)).filter(|at| at.starts_with(&lugar)).count()
        };
        if run_a.provados.iter().filter(|at| at.starts_with(&lugar)).count() != 1
            || de_quem_importa(&run_a) != 1
            || de_quem_importa(&run_b) != 0
        {
            faltas.push(format!("a chamada de {lugar}, que importa a run de {}, é provada só para ela", l.a));
        }

        let lugar = format!("{}:{}", l.sem_import, linha_de(corpo(l, l.sem_import), "run()"));
        for (dono, usos) in [(l.a, &run_a), (l.b, &run_b)] {
            let suspeitas: Vec<&Vec<String>> =
                usos.suspeitos.iter().filter(|(at, _)| at.starts_with(&lugar)).map(|(_, c)| c).collect();
            if suspeitas != [&duas] || usos.provados.iter().any(|at| at.starts_with(&lugar)) {
                faltas.push(format!("a chamada de {lugar}, sem import, é suspeita em {dono} com {duas:?}: {suspeitas:?}"));
            }
        }

        let lugar = format!("{}:{}", l.usa_unica, linha_de(corpo(l, l.usa_unica), " unica()"));
        let unica = usos_de(&map, l.unica, "unica");
        if unica.provados.iter().filter(|at| at.starts_with(&lugar)).count() != 1 || !unica.suspeitos.is_empty() {
            faltas.push(format!("unica, declarada uma vez, é provada em {lugar} sem import: {:?}", unica.provados));
        }

        let lugar = format!("{}:{}", l.por_valor, linha_de(corpo(l, l.por_valor), "x.run()"));
        let so_a = vec![format!("{}:1:run", l.a)];
        let suspeitas: Vec<&Vec<String>> =
            run_a.suspeitos.iter().filter(|(at, _)| at.starts_with(&lugar)).map(|(_, c)| c).collect();
        if suspeitas != [&so_a] || run_a.provados.iter().any(|at| at.starts_with(&lugar)) {
            faltas.push(format!("a chamada por variável de {lugar} é suspeita, com a run importada: {suspeitas:?}"));
        }
    }
    assert!(faltas.is_empty(), "{}", faltas.join("\n"));
}

/// O próprio objeto e o tipo escrito antes do método estreitam a ligação, no
/// Rust: `self.fechar()`, dentro de `Caixa`, é provado para o `fechar` de
/// `Caixa`, e não para o de `Porta`; `Caixa::new()` é provado para o `new` de
/// `Caixa`, que o arquivo importa. `Vec::new()` nomeia um tipo que o projeto
/// não declara: é uma chamada de fora e não liga a nada.
#[test]
fn the_object_itself_and_a_named_type_narrow_the_link() {
    const CAIXA: &str = "pub struct Caixa;\n\nimpl Caixa {\n    pub fn abrir(&self) -> u32 {\n        self.fechar()\n    }\n\n    \
                         pub fn fechar(&self) -> u32 {\n        1\n    }\n\n    pub fn new() -> Caixa {\n        Caixa\n    }\n}\n";
    const PORTA: &str = "pub struct Porta;\n\nimpl Porta {\n    pub fn fechar(&self) -> u32 {\n        2\n    }\n\n    \
                         pub fn new() -> Porta {\n        Porta\n    }\n}\n";
    const USA: &str = "use crate::caixa::Caixa;\n\npub fn usa() -> Vec<u32> {\n    let _ = Caixa::new();\n    Vec::new()\n}\n";
    let temp = pasta_do_projeto("ligacao-pelo-tipo");
    let dir = temp.path().to_path_buf();
    write(&dir, "src/caixa.rs", CAIXA);
    write(&dir, "src/porta.rs", PORTA);
    write(&dir, "src/usa.rs", USA);
    let map = scan(&dir);

    let fechar = usos_de(&map, "src/caixa.rs", "fechar");
    let esperado = format!("src/caixa.rs:{}:abrir", linha_de(CAIXA, "self.fechar()"));
    assert_eq!((fechar.provados, fechar.suspeitos.len()), (vec![esperado], 0), "self.fechar() é o fechar de Caixa");
    let fechar_porta = usos_de(&map, "src/porta.rs", "fechar");
    assert!(fechar_porta.provados.is_empty() && fechar_porta.suspeitos.is_empty(), "o fechar de Porta não é chamado");

    let new = usos_de(&map, "src/caixa.rs", "new");
    let esperado = format!("src/usa.rs:{}:usa", linha_de(USA, "Caixa::new()"));
    assert_eq!((new.provados, new.suspeitos.len()), (vec![esperado], 0), "Caixa::new() é o new de Caixa, e Vec::new() não");
    let new_porta = usos_de(&map, "src/porta.rs", "new");
    assert!(new_porta.provados.is_empty() && new_porta.suspeitos.is_empty(), "o new de Porta não é chamado");
}

/// O nome sozinho só alcança um método ou um membro de enum na língua que
/// chama o membro do próprio objeto sem escrevê-lo. No Rust, o `Ok(` e o
/// `trancar()` escritos sozinhos não são o `Ok` de um `enum` do projeto nem o
/// método `trancar` de `Caixa`, mesmo com os dois importados: nenhum ganha o
/// uso. No C#, o `Trancar()` escrito dentro da própria classe é o método dela,
/// provado.
#[test]
fn a_bare_name_reaches_a_member_only_where_the_language_calls_it_without_the_object() {
    const ESTADO: &str = "pub enum Estado {\n    Ok,\n    Falha,\n}\n";
    const CAIXA_RS: &str = "pub struct Caixa;\n\nimpl Caixa {\n    pub fn trancar(&self) -> u32 {\n        1\n    }\n}\n";
    const USA: &str = "use crate::caixa::Caixa;\nuse crate::estado::Estado;\n\npub fn usa() -> Result<u32, ()> {\n    \
                       let trancar = || 2;\n    Ok(trancar())\n}\n";
    const CAIXA_CS: &str = "namespace Loja;\n\npublic class Caixa\n{\n    public int Trancar()\n    {\n        return 1;\n    }\n\n    \
                            public int Abrir()\n    {\n        return Trancar();\n    }\n}\n";
    let temp = pasta_do_projeto("nome-sozinho-e-membro");
    let dir = temp.path().to_path_buf();
    write(&dir, "rs/src/estado.rs", ESTADO);
    write(&dir, "rs/src/caixa.rs", CAIXA_RS);
    write(&dir, "rs/src/usa.rs", USA);
    write(&dir, "cs/Caixa.cs", CAIXA_CS);
    let map = scan(&dir);

    for (arquivo, nome) in [("rs/src/estado.rs", "Ok"), ("rs/src/caixa.rs", "trancar")] {
        let usos = usos_de(&map, arquivo, nome);
        assert!(
            usos.provados.is_empty() && usos.suspeitos.is_empty(),
            "o {nome} de {arquivo} não é o nome escrito sozinho: {:?} {:?}",
            usos.provados,
            usos.suspeitos
        );
    }
    let trancar = usos_de(&map, "cs/Caixa.cs", "Trancar");
    let esperado = format!("cs/Caixa.cs:{}:Abrir", linha_de(CAIXA_CS, "return Trancar()"));
    assert_eq!(
        (trancar.provados, trancar.suspeitos.len()),
        (vec![esperado], 0),
        "o Trancar() dentro da classe é o método dela"
    );
}

/// O nome declarado mais vezes que o teto não liga: a chamada que pode
/// alcançar nove declarações só se conta, em cada uma delas, e nenhuma ganha
/// o uso.
#[test]
fn a_name_above_the_ceiling_is_only_counted() {
    let temp = pasta_do_projeto("nome-comum-so-se-conta");
    let dir = temp.path().to_path_buf();
    let donos: Vec<String> = (1..=9).map(|n| format!("src/m{n}.rs")).collect();
    for (n, dono) in donos.iter().enumerate() {
        write(&dir, dono, &format!("pub fn comum() -> u32 {{\n    {n}\n}}\n"));
    }
    write(&dir, "src/chama.rs", "pub fn chama() -> u32 {\n    comum()\n}\n");
    let map = scan(&dir);
    for dono in &donos {
        let comum = usos_de(&map, dono, "comum");
        assert_eq!(
            (comum.comuns, comum.provados.len(), comum.suspeitos.len()),
            (1, 0, 0),
            "o comum de {dono} só conta a chamada"
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
fn projeto_do_nome_proprio() -> Vec<(&'static str, &'static str)> {
    vec![
        ("rs/src/a.rs", "pub fn fill() -> u32 {\n    1\n}\n"),
        (
            "rs/src/b.rs",
            "use crate::a::fill;\n\npub fn com_local() -> u32 {\n    let fill = |x: u32| x + 1;\n    fill(2)\n}\n\n\
             pub fn com_parametro(fill: fn() -> u32) -> u32 {\n    fill()\n}\n\n\
             pub fn mesma_linha() -> u32 {\n    let fill = fill();\n    fill\n}\n\n\
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
            "rs/src/com_result.rs",
            "use crate::erro::{Error, Result};\n\npub fn com_result() -> Result<u32> {\n    Err(Error)\n}\n",
        ),
        ("rs/src/com_glob.rs", "use crate::erro::*;\n\npub fn com_glob() -> Result<u32> {\n    Ok(1)\n}\n"),
        ("ts/src/a.ts", "export function fill(): number {\n  return 1;\n}\n"),
        (
            "ts/src/b.ts",
            "import { fill } from \"./a\";\n\nexport function comLocal(): number {\n  const fill = (x: number) => x + 1;\n  \
             return fill(2);\n}\n\nexport function comParametro(fill: () => number): number {\n  return fill();\n}\n\n\
             export function comDesestruturacao(x: { mutate: () => number }): number {\n  const { mutate: fill } = x;\n  \
             return fill();\n}\n\nexport function comAninhada(x: { mutate: () => number }): number {\n  \
             const { mutate: fill } = x;\n  function chama(): number {\n    return fill();\n  }\n  return chama();\n}\n\n\
             export function vizinha(): number {\n  return fill();\n}\n",
        ),
        ("py/pkg/a.py", "def fill():\n    return 1\n"),
        (
            "py/pkg/b.py",
            "from pkg.a import fill\n\n\ndef com_local():\n    fill = lambda x: x + 1\n    return fill(2)\n\n\n\
             def com_parametro(fill):\n    return fill()\n\n\ndef vizinha():\n    return fill()\n",
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
    let temp = pasta_do_projeto("nome-proprio");
    let dir = temp.path().to_path_buf();
    let projeto = projeto_do_nome_proprio();
    for (rel, corpo) in &projeto {
        write(&dir, rel, corpo);
    }
    let map = scan(&dir);
    let corpo = |arquivo: &str| projeto.iter().find(|(rel, _)| *rel == arquivo).unwrap().1;
    let mut faltas: Vec<String> = Vec::new();
    for (a, b, quem) in [
        ("rs/src/a.rs", "rs/src/b.rs", &["mesma_linha", "vizinha"][..]),
        ("ts/src/a.ts", "ts/src/b.ts", &["vizinha"][..]),
        ("py/pkg/a.py", "py/pkg/b.py", &["vizinha"][..]),
    ] {
        let fill = usos_de(&map, a, "fill");
        let mut provados = fill.provados.clone();
        provados.sort();
        let mut esperados: Vec<String> =
            quem.iter().map(|f| format!("{b}:{}:{f}", linha_de(corpo(b), &format!("{f}()")) + 1)).collect();
        esperados.sort();
        if provados != esperados || !fill.suspeitos.is_empty() {
            faltas.push(format!("a fill de {a} só é usada por {quem:?}: {:?} {:?}", fill.provados, fill.suspeitos));
        }
    }
    let remove = usos_de(&map, "rs/src/fs/mod.rs", "remove_dir_all");
    if !remove.provados.is_empty() || !remove.suspeitos.is_empty() || remove.comuns != 0 {
        faltas.push(format!(
            "fs::remove_dir_all, com fs trazido de fora, não liga: {:?} {:?}",
            remove.provados, remove.suspeitos
        ));
    }
    let result = usos_de(&map, "rs/src/erro.rs", "Result");
    let quem = vec![
        format!("rs/src/com_glob.rs:{}:com_glob", linha_de(corpo("rs/src/com_glob.rs"), "Result")),
        format!("rs/src/com_result.rs:{}:com_result", linha_de(corpo("rs/src/com_result.rs"), "-> Result")),
    ];
    if result.provados != quem || !result.suspeitos.is_empty() {
        faltas.push(format!(
            "o Result do projeto só vale onde o arquivo o traz: {:?} {:?}",
            result.provados, result.suspeitos
        ));
    }
    assert!(faltas.is_empty(), "{}", faltas.join("\n"));
}

/// Todos os arquivos de texto debaixo de `dir`, menos a pasta da compilação.
fn arquivos_de_texto(dir: &Path, achados: &mut Vec<(std::path::PathBuf, String)>) {
    for entrada in std::fs::read_dir(dir).unwrap() {
        let caminho = entrada.unwrap().path();
        if caminho.is_dir() {
            if caminho.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            arquivos_de_texto(&caminho, achados);
        } else if let Ok(texto) = std::fs::read_to_string(&caminho) {
            achados.push((caminho, texto));
        }
    }
}

/// Um teste que quebra no meio não deixa a pasta do projeto para trás: ela é
/// criada, lida pelo scan, e o teste quebra numa linha de execução separada,
/// para o teste de fora seguir e conferir que a pasta sumiu. E nenhum arquivo
/// do scan monta a pasta à mão, que é o que deixava as pastas em `/tmp` quando
/// uma conferência falhava.
#[test]
fn a_pasta_do_projeto_de_teste_some_mesmo_quando_o_teste_falha() {
    let (envia, recebe) = std::sync::mpsc::channel();
    let quebra = std::thread::spawn(move || {
        let temp = pasta_do_projeto("quebra-no-meio");
        let dir = temp.path().to_path_buf();
        write(&dir, "src/lib.rs", "pub fn total() -> u32 {\n    1\n}\n");
        let map = scan(&dir);
        assert!(model::path_in(&pasta_do_mapa(&dir)).is_file(), "o scan grava o mapa na pasta");
        envia.send(dir).unwrap();
        assert!(map["modules"].as_array().unwrap().is_empty(), "esta conferência quebra de propósito");
    });
    assert!(quebra.join().is_err(), "o teste de dentro devia ter quebrado");
    let dir = recebe.recv().expect("a pasta foi criada antes da quebra");
    assert!(!dir.exists(), "a pasta {} ficou depois da quebra", dir.display());

    // O nome da chamada é montado em partes, para este arquivo não se acusar.
    let chamada = concat!("temp", "_dir", "()");
    let mut achados = Vec::new();
    arquivos_de_texto(&manifest_dir::manifest_dir(), &mut achados);
    assert!(achados.iter().any(|(c, _)| c.ends_with("src/refresh.rs")), "a busca percorre o scan inteiro");
    let a_mao: Vec<String> = achados
        .iter()
        .filter(|(_, texto)| texto.contains(chamada))
        .map(|(caminho, _)| caminho.display().to_string())
        .collect();
    assert!(a_mao.is_empty(), "estes arquivos montam a pasta do teste à mão: {a_mao:?}");
}
