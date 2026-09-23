//! Quem usa cada declaração, nas 8 linguagens que o scan lê. Em cada uma, um
//! arquivo chama uma função de outro arquivo que ele enxerga (importado, ou do
//! mesmo namespace no C#, no Go e no PHP), chama `x.kind()` com um campo
//! `kind` declarado no arquivo que ele enxerga, e chama `.join(` com uma
//! função `join` declarada num terceiro arquivo que ele não enxerga. O uso só
//! liga à função chamada: o campo não se chama, e o `join` de fora é o da
//! biblioteca, não o do terceiro arquivo. Os nomes se repetem de propósito em
//! todas as linguagens, para que uma ligação pelo nome no projeto inteiro
//! apareça como uso cruzando linguagens.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
