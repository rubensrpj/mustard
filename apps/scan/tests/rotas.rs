//! As rotas do servidor que o mapa guarda: o método, o caminho padronizado, o
//! caminho como foi escrito e a função que atende cada uma. Projetos pequenos
//! com um controlador de C#, um de NestJS, um roteador de axum e um de
//! Express, lidos pelo scan de verdade. O Express vale também no arquivo
//! JavaScript que o traz pelo `require`.

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
fn project_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempfile::Builder::new().prefix("scan-rotas-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join(".git").join("info")).unwrap();
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    temp
}

/// O projeto de cada framework, com os arquivos que não ligam nenhum.
fn project() -> tempfile::TempDir {
    project_with(&[
        ("Loja/Controllers/PedidosController.cs", CONTROLLER),
        ("Loja/Program.cs", MINIMAL_API),
        ("api/pedidos.controller.ts", NEST),
        ("src/rotas.rs", AXUM),
        ("web/rotas.ts", EXPRESS),
        ("web/app.ts", EXPRESS_APP),
        ("web/cache.ts", NO_FRAMEWORK),
        ("web/leitor.ts", OTHER_PACKAGE),
    ])
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

/// Uma classe de C# sem nada de framework.
const PLAIN_CSHARP: &str = "namespace Loja;\n\npublic class Caixa\n{\n    public int Total() => 0;\n}\n";

/// A consulta de rota de um framework só se compila quando algum arquivo liga
/// a regra dele: o projeto sem import de framework, com arquivos das línguas
/// que têm regra, não compila nenhuma; o projeto com Express compila só a do
/// Express.
#[test]
fn a_route_rule_is_compiled_only_when_a_file_turns_it_on() {
    let temp = project_with(&[
        ("web/cache.ts", NO_FRAMEWORK),
        ("web/leitor.ts", OTHER_PACKAGE),
        ("src/lib.rs", "pub fn soma() -> u32 { 1 }\n"),
        ("Loja/Caixa.cs", PLAIN_CSHARP),
    ]);
    let (_, report) = scan(temp.path());
    assert_eq!(report["route_rules"], json!([]), "{report}");

    let temp = project_with(&[("web/rotas.ts", EXPRESS), ("web/cache.ts", NO_FRAMEWORK), ("Loja/Caixa.cs", PLAIN_CSHARP)]);
    let (map, report) = scan(temp.path());
    assert_eq!(report["route_rules"], json!(["express/typescript"]), "{report}");
    assert_eq!(keys(&map, "web/rotas.ts").len(), 2, "the rule compiled once still finds the routes");
}

/// A API mínima sem `using` nenhum no arquivo, e as rotas dela.
const BARE_MINIMAL_API: &str = r#"var app = WebApplication.Create(args);
var pedidos = app.MapGroup("/pedidos");
pedidos.MapGet("/{id:int}", Ler);
app.MapPost("/pedidos", (Pedido p) => Results.Ok(p));
app.Run();
"#;
const BARE_ROUTES: [&str; 2] = ["GET pedidos/{} -> Ler:3", "POST pedidos -> :4"];

/// Um projeto .NET com o SDK `sdk`.
fn csproj(sdk: &str) -> String {
    format!("<Project Sdk=\"{sdk}\">\n  <PropertyGroup>\n    <TargetFramework>net8.0</TargetFramework>\n  </PropertyGroup>\n</Project>\n")
}

/// O `global using` do framework, escrito em outro arquivo, liga a regra nos
/// arquivos da pasta do projeto dele, e não nos do projeto vizinho.
#[test]
fn a_global_using_in_another_file_turns_the_rule_on_in_the_files_of_its_project_only() {
    let plain = csproj("Microsoft.NET.Sdk");
    let temp = project_with(&[
        ("Api/Api.csproj", &plain),
        ("Api/GlobalUsings.cs", "global using Microsoft.AspNetCore.Builder;\n"),
        ("Api/Program.cs", BARE_MINIMAL_API),
        ("Outro/Outro.csproj", &plain),
        ("Outro/Program.cs", BARE_MINIMAL_API),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(keys(&map, "Api/Program.cs"), BARE_ROUTES);
    assert_eq!(keys(&map, "Outro/Program.cs"), Vec::<String>::new());
}

/// O projeto web liga a regra nos arquivos dele sem `using` nenhum; o
/// projeto de biblioteca, não.
#[test]
fn a_web_project_turns_the_rule_on_in_its_files_without_any_using() {
    let temp = project_with(&[
        ("Api/Api.csproj", &csproj("Microsoft.NET.Sdk.Web")),
        ("Api/Program.cs", BARE_MINIMAL_API),
        ("Lib/Lib.csproj", &csproj("Microsoft.NET.Sdk")),
        ("Lib/Program.cs", BARE_MINIMAL_API),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(keys(&map, "Api/Program.cs"), BARE_ROUTES);
    assert_eq!(keys(&map, "Lib/Program.cs"), Vec::<String>::new());
}

/// A passada que lê só o que mudou dá as rotas da passada inteira quando o
/// `global using` aparece noutro arquivo e quando o SDK do projeto troca,
/// num sentido e no outro, sem o arquivo das rotas mudar.
#[test]
fn a_pass_that_reads_only_what_changed_follows_the_global_using_and_the_sdk() {
    let plain = csproj("Microsoft.NET.Sdk");
    let temp = project_with(&[
        ("Api/Api.csproj", &plain),
        ("Api/Program.cs", BARE_MINIMAL_API),
        ("Web/Web.csproj", &plain),
        ("Web/Program.cs", BARE_MINIMAL_API),
    ]);
    let dir = temp.path();
    let (first, _) = scan(dir);
    assert_eq!(keys(&first, "Api/Program.cs"), Vec::<String>::new());
    assert_eq!(keys(&first, "Web/Program.cs"), Vec::<String>::new());

    let steps: [(&str, String, [bool; 2]); 3] = [
        ("Api/GlobalUsings.cs", "global using Microsoft.AspNetCore.Builder;\n".to_string(), [true, false]),
        ("Web/Web.csproj", csproj("Microsoft.NET.Sdk.Web"), [true, true]),
        ("Web/Web.csproj", plain.clone(), [true, false]),
    ];
    for (path, body, with_routes) in steps {
        std::fs::write(dir.join(path), body).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", path]);
        let (partial, report) = scan(dir);
        assert_eq!(report["full"], json!(false), "{path}: {report}");
        let whole_out = tempfile::tempdir().unwrap();
        let (whole, _) = model::scan(dir, whole_out.path(), &[]);
        for (file, routes_expected) in ["Api/Program.cs", "Web/Program.cs"].into_iter().zip(with_routes) {
            let expected: Vec<String> =
                if routes_expected { BARE_ROUTES.map(String::from).to_vec() } else { Vec::new() };
            assert_eq!(keys(&whole, file), expected, "{path}: {file} in the whole pass");
            assert_eq!(routes(&partial, file), routes(&whole, file), "{path}: {file}");
        }
    }
}

/// O `@Controller` com objeto de opções: o `path` escrito como texto é o
/// prefixo da classe, exportada ou não; o objeto sem `path` deixa a rota no
/// caminho do método.
#[test]
fn a_nest_controller_with_an_options_object_takes_its_path_as_the_prefix() {
    let head = "import { Controller, Get } from '@nestjs/common';\n\n";
    let exported = format!("{head}@Controller({{ path: 'v1/pedidos', version: '2' }})\nexport class PedidosController {{\n  @Get(':id')\n  ler() {{}}\n}}\n");
    let inner = format!("{head}@Controller({{ version: '1', path: 'v1/itens' }})\nclass ItensController {{\n  @Get(':id')\n  ler() {{}}\n}}\n");
    let hostonly = format!("{head}@Controller({{ host: 'x' }})\nexport class SaudeController {{\n  @Get('saude')\n  ver() {{}}\n}}\n");
    let temp = project_with(&[
        ("api/pedidos.controller.ts", &exported),
        ("api/itens.controller.ts", &inner),
        ("api/saude.controller.ts", &hostonly),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(keys(&map, "api/pedidos.controller.ts"), ["GET v1/pedidos/{} -> ler:5"]);
    assert_eq!(keys(&map, "api/itens.controller.ts"), ["GET v1/itens/{} -> ler:5"]);
    assert_eq!(keys(&map, "api/saude.controller.ts"), ["GET saude -> ver:5"]);
}

/// Um roteador de Express em JavaScript, que traz o framework pelo `require`.
const EXPRESS_JS: &str = "const express = require('express');\nconst router = express.Router();\n\
    function ler(req, res) {\n  res.send('ok');\n}\nrouter.get('/aves/:id', ler);\nmodule.exports = router;\n";

/// Um arquivo JavaScript que traz pelo `require` um pacote cujo nome começa
/// pelo do framework.
const OTHER_PACKAGE_JS: &str =
    "const { parse } = require('expression');\nconst router = { get: parse };\nrouter.get('/aves', parse);\n";

/// O `require('express')` liga a regra do Express no arquivo JavaScript, como
/// o `import`; o `require` de outro pacote não liga.
#[test]
fn a_javascript_file_that_requires_express_has_its_routes() {
    let temp = project_with(&[("src/aves.js", EXPRESS_JS), ("src/leitor.js", OTHER_PACKAGE_JS)]);
    let (map, _) = scan(temp.path());
    assert_eq!(keys(&map, "src/aves.js"), ["GET aves/{} -> ler:3"]);
    assert_eq!(routes(&map, "src/leitor.js"), json!([]));
}
