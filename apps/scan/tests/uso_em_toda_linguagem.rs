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

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn scan(dir: &Path) -> Value {
    let model = dir.join(".claude").join("grain.model.json");
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", dir.to_str().unwrap(), "--out", model.to_str().unwrap()])
        .output()
        .expect("run scan");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&model).unwrap()).unwrap()
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
    let dir = std::env::temp_dir().join(format!("scan-uso-em-toda-linguagem-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    let _ = std::fs::remove_dir_all(&dir);
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
    let dir = std::env::temp_dir().join(format!("scan-citacao-em-toda-linguagem-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    let _ = std::fs::remove_dir_all(&dir);
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
    let dir = std::env::temp_dir().join(format!("scan-o-que-a-linguagem-poe-a-vista-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let arquivos = projeto_a_vista();
    for (rel, corpo) in &arquivos {
        write(&dir, rel, corpo);
    }
    let map = scan(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    let modules = map["modules"].as_array().expect("modules");
    let modulo = |arquivo: &str| -> &Value {
        modules.iter().find(|m| m["path"] == arquivo).unwrap_or_else(|| panic!("{arquivo} no mapa"))
    };
    let lista = |v: &Value| -> Vec<String> {
        v.as_array().map_or(Vec::new(), |a| a.iter().map(|u| u.as_str().unwrap().to_string()).collect())
    };
    let usos = |arquivo: &str, nome: &str| -> Vec<String> {
        let m = modulo(arquivo);
        let d = m["declarations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["name"] == nome)
            .unwrap_or_else(|| panic!("{nome} declarado em {arquivo}: {m}"));
        lista(&d["used_by"])
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
    // grafo de import continua só nele.
    let globais = lista(&modulo("Loja/GlobalUsings.cs")["global_imports"]);
    if globais != ["Loja.Dominio"] {
        faltas.push(format!("GlobalUsings.cs guarda o import global Loja.Dominio: {globais:?}"));
    }
    if let Some(m) = modules.iter().find(|m| m["path"] != "Loja/GlobalUsings.cs" && m.get("global_imports").is_some()) {
        faltas.push(format!("só GlobalUsings.cs grava import global: {}", m["path"]));
    }
    if !deps("Loja/GlobalUsings.cs").contains(&"Loja/Dominio/Calculadora.cs".to_string())
        || deps(pedido).contains(&"Loja/Dominio/Calculadora.cs".to_string())
    {
        faltas.push(format!(
            "a aresta de import vai de GlobalUsings.cs a Calculadora.cs, e não de Pedido.cs: {:?} / {:?}",
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
