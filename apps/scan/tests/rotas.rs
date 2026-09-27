//! As rotas do servidor que o mapa guarda: o método, o caminho padronizado, o
//! caminho como foi escrito e a função que atende cada uma. Projetos pequenos
//! com um controlador de C#, um de NestJS, um roteador de axum e um de
//! Express, lidos pelo scan de verdade.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

const CONTROLLER: &str = r#"using Microsoft.AspNetCore.Mvc;

namespace Loja.Controllers;

[ApiController]
[Route("api/[controller]")]
public class PedidosController : ControllerBase
{
    [HttpGet("{id}")]
    public IActionResult Ler(int id) => Ok(id);

    [HttpPost]
    public IActionResult Criar() => Ok();

    [HttpDelete]
    [Route("{id:int}/itens")]
    public IActionResult Remover(int id) => Ok(id);

    [HttpGet(Rotas.Busca)]
    public IActionResult Buscar() => Ok();

    [HttpGet("~/saude")]
    public IActionResult Saude() => Ok();
}
"#;

const MINIMAL_API: &str = r#"using Microsoft.AspNetCore.Builder;

var app = WebApplication.Create(args);
var pedidos = app.MapGroup("/pedidos");
pedidos.MapGet("/{id:int}", Ler);
app.MapPost("/pedidos", (Pedido p) => Results.Ok(p));
app.Run();
"#;

const NEST: &str = r#"import { Body, Controller, Get, Post } from '@nestjs/common';

@Controller('v1/pedidos')
export class PedidosController {
  @Post('edit')
  editar(@Body() corpo: unknown) {
    return corpo;
  }

  @Get(':id')
  ler() {}

  @Put(['itens', 'itens/:id'])
  trocar() {}
}
"#;

const AXUM: &str = r#"use axum::{routing::get, Router};

pub fn rotas() -> Router {
    Router::new().route("/pedidos/:id", get(ler).post(criar))
}

pub fn app() -> Router {
    Router::new().nest("/v1", rotas())
}

async fn ler() -> &'static str {
    "um pedido"
}

async fn criar() {}
"#;

const EXPRESS: &str = r#"import express from 'express';
import { criar } from './pedidos';

const router = express.Router();
router.post('/pedidos', criar);
router.get('/pedidos/:id', (req, res) => res.send('ok'));
const caminho = '/segredo';
router.get(caminho, criar);
const config = new Map();
config.get('CHAVE_SECRETA', '');
export default router;
"#;

const EXPRESS_APP: &str = r#"import express from 'express';

const app = express();
const pedidos = express.Router();
pedidos.get('/:id', ler);
app.use('/api/pedidos', pedidos);

function ler() {}
"#;

/// Um arquivo que não importa framework nenhum, com uma função `get` e uma
/// chamada que tem a forma de uma rota.
const NO_FRAMEWORK: &str = r#"export function get(chave: string) {
  return chave;
}

const router = { get };
router.get('/pedidos', get);
"#;

/// Um arquivo que importa um pacote cujo nome começa pelo do framework.
const OTHER_PACKAGE: &str = r#"import { parse } from 'expression';

const router = { get: parse };
router.get('/pedidos', parse);
"#;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.email=scan@example.com", "-c", "user.name=scan", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Um projeto no git com os arquivos, já no primeiro commit.
fn project() -> tempfile::TempDir {
    let temp = tempfile::Builder::new().prefix("scan-rotas-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join(".git").join("info")).unwrap();
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    for (rel, body) in [
        ("Loja/Controllers/PedidosController.cs", CONTROLLER),
        ("Loja/Program.cs", MINIMAL_API),
        ("api/pedidos.controller.ts", NEST),
        ("src/rotas.rs", AXUM),
        ("web/rotas.ts", EXPRESS),
        ("web/app.ts", EXPRESS_APP),
        ("web/cache.ts", NO_FRAMEWORK),
        ("web/leitor.ts", OTHER_PACKAGE),
    ] {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    temp
}

/// Roda o scan sobre o projeto, com o mapa dentro dele, e devolve o mapa e o
/// relato da passada.
fn scan(dir: &Path) -> (Value, Value) {
    model::scan(dir, &dir.join(".claude"), &[])
}

/// As rotas que o mapa guarda para o arquivo `path`.
fn routes(map: &Value, path: &str) -> Value {
    let module = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(path));
    module.unwrap_or_else(|| panic!("{path} no mapa")).get("routes").cloned().unwrap_or(json!([]))
}

/// Cada rota do arquivo como `MÉTODO caminho -> função:linha`.
fn keys(map: &Value, path: &str) -> Vec<String> {
    let routes = routes(map, path);
    let mut keys: Vec<String> = routes
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let handler = r.get("handler").and_then(Value::as_str).unwrap_or_default();
            format!("{} {} -> {handler}:{}", r["method"].as_str().unwrap(), r["path"].as_str().unwrap(), r["line"])
        })
        .collect();
    keys.sort();
    keys
}

#[test]
fn an_action_joins_the_class_route_with_the_controller_name_and_its_own_template() {
    let temp = project();
    let (map, _) = scan(temp.path());
    let kept = keys(&map, "Loja/Controllers/PedidosController.cs");
    assert!(kept.contains(&"GET api/pedidos/{} -> Ler:9".to_string()), "{kept:?}");
    assert!(kept.contains(&"POST api/pedidos -> Criar:12".to_string()), "{kept:?}");
    let ler = routes(&map, "Loja/Controllers/PedidosController.cs")
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["handler"] == json!("Ler"))
        .cloned()
        .unwrap();
    assert_eq!(ler["written"], json!("api/[controller]/{id}"));
    assert_eq!(ler["framework"], json!("aspnetcore"));
}

#[test]
fn a_route_attribute_without_a_method_takes_the_method_of_the_same_action() {
    let temp = project();
    let (map, _) = scan(temp.path());
    let kept = keys(&map, "Loja/Controllers/PedidosController.cs");
    assert!(kept.contains(&"DELETE api/pedidos/{}/itens -> Remover:15".to_string()), "{kept:?}");
    assert!(!kept.iter().any(|key| key.starts_with("* ")), "{kept:?}");
}

#[test]
fn a_template_that_starts_at_the_root_does_not_join_the_class_route() {
    let temp = project();
    let (map, _) = scan(temp.path());
    let kept = keys(&map, "Loja/Controllers/PedidosController.cs");
    assert!(kept.contains(&"GET saude -> Saude:22".to_string()), "{kept:?}");
    assert_eq!(keys(&map, "Loja/Program.cs")[0], "GET pedidos/{} -> Ler:5", "the group of a minimal API still joins");
}

#[test]
fn a_route_template_that_is_not_written_text_makes_no_route() {
    let temp = project();
    let (map, _) = scan(temp.path());
    let kept = keys(&map, "Loja/Controllers/PedidosController.cs");
    assert!(!kept.iter().any(|key| key.contains("Buscar")), "{kept:?}");
    let express = keys(&map, "web/rotas.ts");
    assert!(!express.iter().any(|key| key.contains("segredo")), "{express:?}");
    assert_eq!(express.len(), 2, "{express:?}");
}

#[test]
fn a_call_with_the_shape_of_a_route_but_a_path_that_does_not_start_like_one_makes_no_route() {
    let temp = project();
    let (map, _) = scan(temp.path());
    let express = keys(&map, "web/rotas.ts");
    assert!(!express.iter().any(|key| key.contains("chave")), "{express:?}");
}

#[test]
fn a_minimal_api_route_joins_the_group_kept_in_a_variable() {
    let temp = project();
    let (map, _) = scan(temp.path());
    assert_eq!(keys(&map, "Loja/Program.cs"), ["GET pedidos/{} -> Ler:5", "POST pedidos -> :6"]);
}

#[test]
fn a_nest_controller_joins_its_prefix_with_the_path_of_each_method() {
    let temp = project();
    let (map, _) = scan(temp.path());
    assert_eq!(
        keys(&map, "api/pedidos.controller.ts"),
        [
            "GET v1/pedidos/{} -> ler:10",
            "POST v1/pedidos/edit -> editar:5",
            "PUT v1/pedidos/itens -> trocar:13",
            "PUT v1/pedidos/itens/{} -> trocar:13"
        ]
    );
}

#[test]
fn each_method_of_an_axum_route_is_a_route_served_by_its_function() {
    let temp = project();
    let (map, _) = scan(temp.path());
    assert_eq!(keys(&map, "src/rotas.rs"), ["GET v1/pedidos/{} -> ler:11", "POST v1/pedidos/{} -> criar:15"]);
    let kept = routes(&map, "src/rotas.rs");
    let written: Vec<&str> = kept.as_array().unwrap().iter().map(|r| r["written"].as_str().unwrap()).collect();
    assert_eq!(written, ["v1/pedidos/:id", "v1/pedidos/:id"]);
}

#[test]
fn an_express_route_keeps_the_function_named_or_the_line_of_the_one_written_there() {
    let temp = project();
    let (map, _) = scan(temp.path());
    assert_eq!(keys(&map, "web/rotas.ts"), ["GET pedidos/{} -> :6", "POST pedidos -> criar:5"]);
}

#[test]
fn a_router_mounted_with_use_takes_the_prefix_of_the_mount() {
    let temp = project();
    let (map, _) = scan(temp.path());
    assert_eq!(keys(&map, "web/app.ts"), ["GET api/pedidos/{} -> ler:8"]);
}

#[test]
fn a_file_that_imports_no_framework_has_no_route() {
    let temp = project();
    let (map, _) = scan(temp.path());
    assert_eq!(routes(&map, "web/cache.ts"), json!([]));
    assert_eq!(routes(&map, "web/leitor.ts"), json!([]), "a package whose name starts with the framework's is another one");
}

#[test]
fn a_pass_that_does_not_read_the_file_again_keeps_the_same_routes() {
    let temp = project();
    let (first, _) = scan(temp.path());
    std::fs::write(temp.path().join("src/outro.rs"), "pub fn outro() {}\n").unwrap();
    git(temp.path(), &["add", "-A"]);
    git(temp.path(), &["commit", "-q", "-m", "segundo"]);
    let (second, report) = scan(temp.path());
    assert_eq!(report["full"], json!(false), "{report}");
    assert_eq!(report["read"], json!(["src/outro.rs"]), "{report}");
    for path in [
        "Loja/Controllers/PedidosController.cs",
        "Loja/Program.cs",
        "api/pedidos.controller.ts",
        "src/rotas.rs",
        "web/rotas.ts",
        "web/app.ts",
    ] {
        assert!(!keys(&first, path).is_empty(), "{path}");
        assert_eq!(routes(&second, path), routes(&first, path), "{path}");
    }
}
