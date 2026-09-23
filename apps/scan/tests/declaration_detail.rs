//! O que o mapa passa a guardar de cada declaração: o comentário de
//! documentação escrito em cima dela, a assinatura dela, e cada ligação
//! nomeada entre declarações — quem chama quem, em qual arquivo e em qual
//! linha. A varredura é a de verdade, num projeto de mentira gravado em disco,
//! e a conferência é feita no mapa gravado.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn model_of(dir: &Path) -> PathBuf {
    dir.join(".claude").join("grain.model.json")
}

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Varre `dir` e devolve o mapa gravado.
fn scan(dir: &Path) -> Value {
    let model = model_of(dir);
    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["scan", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--json"])
        .output()
        .expect("run scan");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&model).expect("the map was written");
    serde_json::from_str(&text).expect("the map is JSON")
}

/// Um projeto de mentira: uma função documentada em português, e outro arquivo
/// que a chama duas vezes de dentro de uma função sua.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
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
         pub fn sem_documento() -> u32 {\n    \
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
    dir
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
fn o_mapa_guarda_o_comentario_e_a_assinatura_de_cada_declaracao() {
    let dir = project("doc-e-assinatura");
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
    let sem = declaration(&map, "src/preco.rs", "sem_documento");
    assert_eq!(sem.get("doc").map_or("", |d| d.as_str().unwrap_or("")), "", "{sem}");
    assert_eq!(sem["signature"], "pub fn sem_documento() -> u32", "{sem}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn o_mapa_guarda_cada_uso_de_cada_declaracao() {
    let dir = project("usos");
    let map = scan(&dir);

    // Quem é usado: as duas chamadas, cada uma com o arquivo, a linha e a
    // declaração de onde parte.
    let total = declaration(&map, "src/preco.rs", "total");
    let used_by: Vec<&str> = total["used_by"].as_array().unwrap().iter().map(|u| u.as_str().unwrap()).collect();
    assert_eq!(used_by, vec!["src/pedido.rs:5:fechar", "src/pedido.rs:6:fechar"], "cada uso, não a contagem: {total}");

    // Quem chama: o outro lado da mesma ligação.
    let fechar = declaration(&map, "src/pedido.rs", "fechar");
    assert_eq!(fechar["calls"], serde_json::json!(["total"]), "{fechar}");

    // Uma declaração que ninguém usa não ganha ligação nenhuma.
    let sem = declaration(&map, "src/preco.rs", "sem_documento");
    assert!(sem.get("used_by").is_none(), "{sem}");
    assert!(sem.get("calls").is_none(), "{sem}");

    // As contagens do grafo continuam onde estavam — a ligação nomeada é nova,
    // e não substitui o que o grafo já dizia dos arquivos.
    assert!(map["graph"]["nodes"].as_u64().unwrap() >= 3, "{}", map["graph"]);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Um projeto de mentira com um arquivo por linguagem, cada um com uma
/// declaração documentada atrás do seu enfeite: o atributo, o decorador ou a
/// anotação que a linguagem escreve entre o comentário e a declaração.
fn decorated_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
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
    dir
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
fn o_atributo_e_o_decorador_nao_escondem_o_comentario_nem_viram_chamada() {
    let dir = decorated_project("enfeite");
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
    let construtores: Vec<&str> = every_declaration(&map)
        .into_iter()
        .filter(|(f, d)| d["name"] == "Pedidos" && *f == "cs/Pedidos.cs" || d["name"] == "Pedido" && *f == "dart/lib/pedido.dart")
        .map(|(_, d)| d["kind"].as_str().unwrap())
        .collect();
    assert_eq!(construtores, vec!["class", "method", "class", "method"], "a classe e o construtor dela");

    // O cabeçalho começa depois do enfeite.
    let enfeites = ["#[", "[HttpGet", "@Get", "@Component", "@app", "#[Route", "@override"];
    for (file, d) in every_declaration(&map) {
        let signature = d["signature"].as_str().unwrap_or("");
        assert!(
            !enfeites.iter().any(|e| signature.contains(e)),
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
    let falsas = ["derive", "HttpGet", "Get", "Component", "Route", "override", "get", "inicio", "Carregar", "Pedidos", "Pedido"];
    for m in map["modules"].as_array().unwrap() {
        for call in m["calls"].as_array().into_iter().flatten() {
            // `q.nome:linha` quando a chamada tem qualificador: vale o nome.
            let name = call.as_str().unwrap().rsplit_once(':').unwrap().0.rsplit('.').next().unwrap();
            assert!(!falsas.contains(&name), "{} chama {name}, que não é chamada", m["path"]);
        }
    }

    // No Dart o corpo mora ao lado do cabeçalho: o método termina no seu
    // fecha-chave, e a chamada de dentro dele tem o método como quem usa.
    let total = declaration(&map, "dart/lib/pedido.dart", "total");
    assert_eq!((total["line"].as_u64(), total["end_line"].as_u64()), (Some(13), Some(16)), "{total}");
    let soma = declaration(&map, "dart/lib/pedido.dart", "soma");
    assert_eq!((soma["line"].as_u64(), soma["end_line"].as_u64()), (Some(1), Some(3)), "{soma}");
    assert_eq!(soma["used_by"], serde_json::json!(["dart/lib/pedido.dart:14:total"]), "{soma}");

    let _ = std::fs::remove_dir_all(&dir);
}
