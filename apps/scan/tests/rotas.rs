//! As rotas do servidor que o mapa guarda: o método, o caminho padronizado, o
//! caminho como foi escrito e a função que atende cada uma. Projetos pequenos
//! com um controlador de C#, um de NestJS, um roteador de axum e um de
//! Express, lidos pelo scan de verdade. O Express vale também no arquivo
//! JavaScript que o traz pelo `require`. No Python, os decoradores do FastAPI
//! e do Flask e a lista de caminhos do Django.

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

/// Cada rota do arquivo como `MÉTODO caminho -> função`.
fn served(map: &Value, path: &str) -> Vec<String> {
    let mut served: Vec<String> = routes(map, path)
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let handler = r.get("handler").and_then(Value::as_str).unwrap_or_default();
            format!("{} {} -> {handler}", r["method"].as_str().unwrap(), r["path"].as_str().unwrap())
        })
        .collect();
    served.sort();
    served
}

/// O `package.json` de um projeto NestJS.
const NEST_PACKAGE: &str = r#"{"name": "app", "dependencies": {"@nestjs/common": "^10.0.0", "@nestjs/core": "^10.0.0"}}"#;

/// O `main.ts` que sobe a aplicação com o prefixo global `options`, escrito
/// como vai no `setGlobalPrefix`.
fn nest_main(options: &str) -> String {
    format!(
        "import {{ NestFactory }} from '@nestjs/core';\nimport {{ RequestMethod }} from '@nestjs/common';\n\
         import {{ AppModule }} from './app.module';\n\nasync function bootstrap() {{\n  \
         const app = await NestFactory.create(AppModule);\n  app.setGlobalPrefix({options});\n  \
         await app.listen(3000);\n}}\nbootstrap();\n"
    )
}

/// Um controlador NestJS com o prefixo `prefix` e um método `@method(path)`
/// atendido por `handler`.
fn nest_controller(prefix: &str, method: &str, path: &str, handler: &str) -> String {
    format!(
        "import {{ Controller, {method} }} from '@nestjs/common';\n\n@Controller('{prefix}')\n\
         export class C {{\n  @{method}({path})\n  {handler}() {{}}\n}}\n"
    )
}

/// O prefixo global do `main.ts` vale para as rotas do projeto dele — a
/// pasta do `package.json` mais perto acima —, e não para as da pasta
/// vizinha com `package.json` próprio.
#[test]
fn the_global_prefix_reaches_the_routes_of_its_own_project_only() {
    let temp = project_with(&[
        ("app/package.json", NEST_PACKAGE),
        ("app/src/main.ts", &nest_main("'api'")),
        ("app/src/x.controller.ts", &nest_controller("v1/x", "Post", "'edit'", "editar")),
        ("vizinho/package.json", NEST_PACKAGE),
        ("vizinho/src/y.controller.ts", &nest_controller("v1/x", "Post", "'edit'", "editar")),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "app/src/x.controller.ts"), ["POST api/v1/x/edit -> editar"]);
    let x = &routes(&map, "app/src/x.controller.ts")[0];
    assert_eq!(x["written"], json!("api/v1/x/edit"));
    assert_eq!(x["local"], json!({"written": "v1/x/edit", "path": "v1/x/edit"}));
    assert_eq!(served(&map, "vizinho/src/y.controller.ts"), ["POST v1/x/edit -> editar"]);
}

/// Os controladores de `saude`, `pedidos/:id` e `saude/x` sob o prefixo
/// global escrito em `options`.
fn nest_with_exclusions(options: &str) -> (tempfile::TempDir, Value) {
    let temp = project_with(&[
        ("package.json", NEST_PACKAGE),
        ("src/main.ts", &nest_main(options)),
        ("src/saude.controller.ts", &nest_controller("saude", "Get", "", "ver")),
        ("src/pedidos.controller.ts", &nest_controller("pedidos", "Get", "':id'", "ler")),
        ("src/detalhe.controller.ts", &nest_controller("saude", "Get", "'x'", "detalhe")),
    ]);
    let (map, _) = scan(temp.path());
    (temp, map)
}

/// O `exclude` tira o prefixo global da rota de caminho igual ao do item,
/// padronizado como o dela; com `method`, só da rota desse método; o item que
/// termina em `(.*)`, de toda rota que começa pelo resto.
#[test]
fn the_exclude_of_the_global_prefix_takes_it_off_the_routes_of_each_item() {
    let (_temp, map) = nest_with_exclusions("'api', { exclude: ['saude'] }");
    assert_eq!(served(&map, "src/saude.controller.ts"), ["GET saude -> ver"]);
    assert_eq!(served(&map, "src/pedidos.controller.ts"), ["GET api/pedidos/{} -> ler"]);
    assert_eq!(served(&map, "src/detalhe.controller.ts"), ["GET api/saude/x -> detalhe"]);

    let (_temp, map) = nest_with_exclusions("'api', { exclude: [{ path: 'saude', method: RequestMethod.POST }] }");
    assert_eq!(served(&map, "src/saude.controller.ts"), ["GET api/saude -> ver"], "a POST exclusion leaves the GET");

    let (_temp, map) = nest_with_exclusions("'api', { exclude: [{ method: RequestMethod.GET, path: 'pedidos/:id' }] }");
    assert_eq!(served(&map, "src/pedidos.controller.ts"), ["GET pedidos/{} -> ler"]);

    let (_temp, map) = nest_with_exclusions("'api', { exclude: ['saude/(.*)'] }");
    assert_eq!(served(&map, "src/detalhe.controller.ts"), ["GET saude/x -> detalhe"]);
    assert_eq!(served(&map, "src/saude.controller.ts"), ["GET api/saude -> ver"]);
}

/// Um roteador de Express escrito num arquivo e montado noutro: o import
/// padrão e o `require` trazem o módulo inteiro, e o prefixo vale para todas
/// as rotas dele; o nome trazido entre chaves, só para as do objeto com esse
/// nome.
#[test]
fn a_router_brought_from_another_file_takes_the_prefix_of_the_mount() {
    let router = "import { Router } from 'express';\n\nconst router = Router();\nrouter.get('/:id', ler);\n\n\
                  function ler() {}\n\nexport default router;\n";
    let app = "import express from 'express';\nimport aves from './aves';\n\nconst app = express();\n\
               app.use('/aves', aves);\n";
    let router_js = "const express = require('express');\nconst router = express.Router();\n\
                     router.get('/:id', ler);\nfunction ler() {}\nmodule.exports = router;\n";
    let app_js = "const express = require('express');\nconst aves = require('./aves');\nconst app = express();\n\
                  app.use('/aves', aves);\n";
    let named = "import { Router } from 'express';\n\nexport const aves = Router();\naves.get('/:id', ler);\n\
                 export const outro = Router();\noutro.get('/outro', ler);\n\nfunction ler() {}\n";
    let named_app = "import express from 'express';\nimport { aves } from './passaros';\n\nconst app = express();\n\
                     app.use('/aves', aves);\n";
    let temp = project_with(&[
        ("ts/aves.ts", router),
        ("ts/app.ts", app),
        ("js/aves.js", router_js),
        ("js/app.js", app_js),
        ("named/passaros.ts", named),
        ("named/app.ts", named_app),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "ts/aves.ts"), ["GET aves/{} -> ler"]);
    assert_eq!(served(&map, "js/aves.js"), ["GET aves/{} -> ler"]);
    assert_eq!(served(&map, "named/passaros.ts"), ["GET aves/{} -> ler", "GET outro -> ler"]);
}

/// A função de rotas do axum escrita noutro arquivo e aninhada pelo caminho
/// (`pedidos::rotas()`) ou pelo nome trazido no `use`.
#[test]
fn an_axum_function_from_another_file_takes_the_prefix_of_the_nest() {
    let pedidos = "use axum::{routing::get, Router};\n\npub fn rotas() -> Router {\n    \
                   Router::new().route(\"/pedidos/:id\", get(ler))\n}\n\nasync fn ler() {}\n";
    let by_path = "use axum::Router;\n\nmod pedidos;\n\nfn app() -> Router {\n    \
                   Router::new().nest(\"/api\", pedidos::rotas())\n}\n";
    let by_use = "use axum::Router;\nuse crate::pedidos::rotas;\n\nmod pedidos;\n\nfn app() -> Router {\n    \
                  Router::new().nest(\"/api\", rotas())\n}\n";
    for main in [by_path, by_use] {
        let temp = project_with(&[
            ("Cargo.toml", "[package]\nname = \"loja\"\nversion = \"0.1.0\"\n\n[dependencies]\naxum = \"0.7\"\n"),
            ("src/pedidos.rs", pedidos),
            ("src/main.rs", main),
        ]);
        let (map, _) = scan(temp.path());
        assert_eq!(served(&map, "src/pedidos.rs"), ["GET api/pedidos/{} -> ler"], "{main}");
    }
}

/// O projeto web de C# com os arquivos dados, sob `Api/`.
fn web_project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let web = csproj("Microsoft.NET.Sdk.Web");
    let mut all: Vec<(String, String)> = vec![("Api/Api.csproj".to_string(), web)];
    all.extend(files.iter().map(|(path, body)| ((*path).to_string(), (*body).to_string())));
    let borrowed: Vec<(&str, &str)> = all.iter().map(|(path, body)| (path.as_str(), body.as_str())).collect();
    project_with(&borrowed)
}

/// O grupo da API mínima vale em qualquer ponto da cadeia que começa nele,
/// como objeto direto da rota e como objeto de outro grupo.
#[test]
fn a_minimal_api_group_joins_through_a_chain_a_direct_call_and_a_group_of_a_group() {
    let program = "var app = WebApplication.Create(args);\n\
                   var ep = app.MapGroup(\"/pedidos\").WithTags(\"Pedidos\").RequireAuthorization();\n\
                   ep.MapGet(\"/{id}\", Ler).WithName(\"ler\");\n\
                   app.MapGroup(\"/a\").MapGet(\"/b\", B);\n\
                   var x = app.MapGroup(\"/x\");\nvar y = x.MapGroup(\"/y\");\ny.MapGet(\"/z\", Z);\n\
                   var p = \"/montado\";\napp.MapGet(p, M);\n";
    let temp = web_project(&[("Api/Program.cs", program)]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "Api/Program.cs"), ["GET a/b -> B", "GET pedidos/{} -> Ler", "GET x/y/z -> Z"]);
}

/// A extensão que registra rotas no grupo que recebe.
const ORDERS: &str = "namespace Loja;\n\npublic static class Orders\n{\n    \
    public static void MapOrders(this IEndpointRouteBuilder g)\n    {\n        g.MapGet(\"/orders/{id}\", Ler);\n    }\n\n    \
    static string Ler(int id) => \"ok\";\n}\n";

/// O grupo que chega pelo objeto da chamada à extensão, guardado numa
/// variável ou escrito na cadeia, soma o prefixo às rotas dela; a extensão de
/// mesmo nome noutro projeto segue sem ele.
#[test]
fn a_group_handed_to_an_extension_prefixes_its_routes_in_the_same_project() {
    for program in [
        "var app = WebApplication.Create(args);\nvar api = app.MapGroup(\"/api\");\napi.MapOrders();\n",
        "var app = WebApplication.Create(args);\napp.MapGroup(\"/api\").MapOrders();\n",
    ] {
        let temp = web_project(&[
            ("Api/Program.cs", program),
            ("Api/Orders.cs", ORDERS),
            ("Outro/Outro.csproj", &csproj("Microsoft.NET.Sdk.Web")),
            ("Outro/Orders.cs", ORDERS),
        ]);
        let (map, _) = scan(temp.path());
        assert_eq!(served(&map, "Api/Orders.cs"), ["GET api/orders/{} -> Ler"], "{program}");
        assert_eq!(served(&map, "Outro/Orders.cs"), ["GET orders/{} -> Ler"], "{program}");
    }
}

/// A interface dos módulos, como na prova.
const MODULE_CONTRACT: &str = "namespace Loja;\n\npublic interface IModule\n{\n    \
    IEndpointRouteBuilder MapEndpoints(IEndpointRouteBuilder endpoints, ApiVersionSet versionSet);\n}\n";

/// A extensão que monta o grupo `/api` e o entrega a cada módulo.
const MODULE_EXTENSIONS: &str = "namespace Loja;\n\npublic static class ModuleExtensions\n{\n    \
    public static WebApplication MapEndpoints(this WebApplication app, ApiVersionSet versionSet)\n    {\n        \
    var apiGroup = app.MapGroup(\"/api\");\n        foreach (var module in Modules)\n        {\n            \
    module.MapEndpoints(apiGroup, versionSet);\n        }\n        return app;\n    }\n}\n";

/// Um módulo que registra as rotas no grupo que recebe.
const ROLE_MODULE: &str = "namespace Loja;\n\npublic class RoleModule : IModule\n{\n    \
    public IEndpointRouteBuilder MapEndpoints(IEndpointRouteBuilder endpoints, ApiVersionSet versionSet)\n    {\n        \
    var ep = endpoints.MapGroup(\"/roles\").WithTags(\"Roles\").RequireAuthorization();\n        \
    ep.MapGet(\"/{id:guid}\", RoleEndPoints.GetAsync).WithApiVersionSet(versionSet);\n        return endpoints;\n    }\n}\n";

/// A forma da prova: a extensão entrega o grupo `/api` a cada módulo pelo
/// argumento, na posição do parâmetro que o módulo declara.
#[test]
fn a_group_handed_as_an_argument_prefixes_the_routes_of_each_module() {
    let temp = web_project(&[
        ("Api/Infra/IModule.cs", MODULE_CONTRACT),
        ("Api/Infra/ModuleExtensions.cs", MODULE_EXTENSIONS),
        ("Api/Modules/Roles/RoleModule.cs", ROLE_MODULE),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "Api/Modules/Roles/RoleModule.cs"), ["GET api/roles/{} -> GetAsync"]);
}

/// A função que recebe o grupo e o passa adiante, com um grupo no meio, soma
/// os dois prefixos.
#[test]
fn a_group_passed_along_by_another_extension_sums_every_prefix() {
    let v1 = "namespace Loja;\n\npublic static class V1\n{\n    \
              public static void MapV1(this RouteGroupBuilder g)\n    {\n        g.MapGroup(\"/v1\").MapOrders();\n    }\n}\n";
    let program = "var app = WebApplication.Create(args);\napp.MapGroup(\"/api\").MapV1();\n";
    let temp = web_project(&[("Api/Program.cs", program), ("Api/V1.cs", v1), ("Api/Orders.cs", ORDERS)]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "Api/Orders.cs"), ["GET api/v1/orders/{} -> Ler"]);
}

/// A passada que lê só o arquivo mudado — o do roteador montado noutro, o
/// que escreve o prefixo global, o que entrega o grupo — dá as rotas da
/// passada inteira, sem somar o prefixo de novo.
#[test]
fn a_pass_that_reads_only_what_changed_sums_the_prefixes_of_other_files_once() {
    let router = "import { Router } from 'express';\n\nconst router = Router();\nrouter.get('/:id', ler);\n\n\
                  function ler() {}\n\nexport default router;\n";
    let app = "import express from 'express';\nimport aves from './aves';\n\nconst app = express();\n\
               app.use('/aves', aves);\n";
    let program = "var app = WebApplication.Create(args);\nvar api = app.MapGroup(\"/api\");\napi.MapOrders();\n";
    let temp = web_project(&[
        ("Api/Program.cs", program),
        ("Api/Orders.cs", ORDERS),
        ("web/aves.ts", router),
        ("web/app.ts", app),
        ("nest/package.json", NEST_PACKAGE),
        ("nest/src/main.ts", &nest_main("'api'")),
        ("nest/src/pedidos.controller.ts", &nest_controller("pedidos", "Get", "':id'", "ler")),
    ]);
    let dir = temp.path();
    let (first, _) = scan(dir);
    assert_eq!(served(&first, "web/aves.ts"), ["GET aves/{} -> ler"]);
    assert_eq!(served(&first, "nest/src/pedidos.controller.ts"), ["GET api/pedidos/{} -> ler"]);
    assert_eq!(served(&first, "Api/Orders.cs"), ["GET api/orders/{} -> Ler"]);

    let steps: [(&str, String, &str, &[&str]); 4] = [
        ("web/aves.ts", router.replace("router.get('/:id', ler);", "router.get('/:id', ler);\nrouter.post('/', ler);"),
         "web/aves.ts", &["GET aves/{} -> ler", "POST aves -> ler"]),
        ("nest/src/main.ts", nest_main("'v2'"), "nest/src/pedidos.controller.ts", &["GET v2/pedidos/{} -> ler"]),
        ("Api/Program.cs", program.replace("\"/api\"", "\"/loja\""), "Api/Orders.cs", &["GET loja/orders/{} -> Ler"]),
        ("Api/Program.cs", "var app = WebApplication.Create(args);\n".to_string(), "Api/Orders.cs", &["GET orders/{} -> Ler"]),
    ];
    for (path, body, file, expected) in steps {
        std::fs::write(dir.join(path), body).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", path]);
        let (partial, report) = scan(dir);
        assert_eq!(report["full"], json!(false), "{path}: {report}");
        assert_eq!(report["read"], json!([path]), "{path}: {report}");
        let whole_out = tempfile::tempdir().unwrap();
        let (whole, _) = model::scan(dir, whole_out.path(), &[]);
        assert_eq!(served(&whole, file), expected, "{path}: the whole pass");
        for other in ["web/aves.ts", "nest/src/pedidos.controller.ts", "Api/Orders.cs"] {
            assert_eq!(routes(&partial, other), routes(&whole, other), "{path}: {other}");
        }
    }
}

// ---------------------------------------------------------------------------
// As chamadas da tela ligadas às rotas do servidor
// ---------------------------------------------------------------------------

/// Um servidor Express com as rotas `(método, caminho, função)`, uma por
/// linha a partir da quarta, e as funções depois delas.
fn express_server(routes: &[(&str, &str, &str)]) -> String {
    let mut body = String::from("import express from 'express';\n\nconst router = express.Router();\n");
    for (method, path, handler) in routes {
        body.push_str(&format!("router.{method}('{path}', {handler});\n"));
    }
    body.push('\n');
    for (_, _, handler) in routes {
        body.push_str(&format!("function {handler}(req, res) {{}}\n"));
    }
    body
}

/// O `package.json` da tela, que declara o axios.
const SCREEN_PACKAGE: &str = "{\n  \"name\": \"tela\",\n  \"dependencies\": {\n    \"axios\": \"^1.7.0\"\n  }\n}\n";

/// As chamadas da tela que alcançam a rota de `method` e `path` do arquivo.
fn called_by(map: &Value, file: &str, method: &str, path: &str) -> Value {
    let routes = routes(map, file);
    let route = routes.as_array().unwrap().iter().find(|r| r["method"] == json!(method) && r["path"] == json!(path));
    route.unwrap_or_else(|| panic!("{method} {path} em {file}: {routes}")).get("called_by").cloned().unwrap_or(json!([]))
}

/// As chamadas da tela que o mapa guarda para o arquivo.
fn route_calls(map: &Value, path: &str) -> Value {
    let module = map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(path));
    module.unwrap_or_else(|| panic!("{path} no mapa")).get("route_calls").cloned().unwrap_or(json!([]))
}

/// A tela que faz um cliente com `axios.create({ baseURL: '/api' })` e chama
/// `client.get(`/pedidos/${id}`)` liga provada ao `GET api/pedidos/{}` do
/// servidor: a base entra na frente do caminho, e o `${id}` vira a lacuna,
/// como o `:id` da rota.
#[test]
fn a_screen_call_through_a_client_with_a_base_links_proven_to_the_route_and_its_function() {
    let screen = "import axios from 'axios';\n\nconst client = axios.create({ baseURL: '/api' });\n\n\
                  export function carregar(id: string) {\n  return client.get(`/pedidos/${id}`);\n}\n";
    let temp = project_with(&[
        ("servidor/src/pedidos.ts", &express_server(&[("get", "/api/pedidos/:id", "ler")])),
        ("tela/src/pedidos.ts", screen),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(
        route_calls(&map, "tela/src/pedidos.ts"),
        json!([{"method": "GET", "path": "pedidos/{}", "written": "/pedidos/${id}", "line": 6, "owner": "carregar",
                "framework": "axios", "base": {"written": "/api", "path": "api"}}])
    );
    assert_eq!(called_by(&map, "servidor/src/pedidos.ts", "GET", "api/pedidos/{}"), json!(["tela/src/pedidos.ts:6:carregar"]));
}

/// O cliente trazido de outro arquivo — pelo nome ou pelo padrão que o
/// arquivo exporta — põe a base dele na frente do caminho, e o número no
/// meio do caminho vira a lacuna: as duas chamadas ligam provadas. A chamada
/// que só casa com a rota sem o `v1` dela liga suspeita, com a função que
/// atende essa rota. A passada que relê só o servidor liga as chamadas das
/// telas que ela não releu como a passada inteira.
#[test]
fn a_client_brought_from_another_file_carries_its_base_and_a_call_without_the_version_is_suspect() {
    let temp = project_with(&[
        ("servidor/package.json", NEST_PACKAGE),
        ("servidor/src/main.ts", &nest_main("'api'")),
        ("servidor/src/planos.controller.ts", &nest_controller("v1/puzzle/pcp/plans", "Post", "'edit'", "editar")),
        ("servidor/src/pedidos.controller.ts", &nest_controller("pedidos", "Get", "':id'", "ler")),
        ("tela/package.json", SCREEN_PACKAGE),
        ("tela/src/api.ts", "import axios from 'axios';\n\nexport const client = axios.create({ baseURL: '/api' });\n"),
        ("tela/src/http.ts", "import axios from 'axios';\n\nexport default axios.create({ baseURL: '/api' });\n"),
        ("tela/src/planos.ts", "import { client } from './api';\n\nexport function salvar() {\n  return client.post('puzzle/pcp/plans/edit');\n}\n"),
        ("tela/src/pedidos.ts", "import { client } from './api';\n\nexport function abrir() {\n  return client.get('/pedidos/42');\n}\n"),
        ("tela/src/itens.ts", "import http from './http';\n\nexport function listar() {\n  return http.get('/pedidos/7');\n}\n"),
    ]);
    let dir = temp.path();
    let (first, _) = scan(dir);
    assert_eq!(
        called_by(&first, "servidor/src/pedidos.controller.ts", "GET", "api/pedidos/{}"),
        json!(["tela/src/itens.ts:4:listar", "tela/src/pedidos.ts:4:abrir"])
    );
    assert_eq!(
        called_by(&first, "servidor/src/planos.controller.ts", "POST", "api/v1/puzzle/pcp/plans/edit"),
        json!([{"at": "tela/src/planos.ts:4:salvar", "candidates": ["servidor/src/planos.controller.ts:5:editar"]}])
    );

    let controller = dir.join("servidor/src/pedidos.controller.ts");
    let body = std::fs::read_to_string(&controller).unwrap();
    std::fs::write(&controller, format!("// Os pedidos.\n{body}")).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "comentario"]);
    let (partial, report) = scan(dir);
    assert_eq!(report["read"], json!(["servidor/src/pedidos.controller.ts"]), "{report}");
    let whole_out = tempfile::tempdir().unwrap();
    let (whole, _) = model::scan(dir, whole_out.path(), &[]);
    for file in ["servidor/src/pedidos.controller.ts", "servidor/src/planos.controller.ts"] {
        assert_eq!(routes(&partial, file), routes(&whole, file), "{file}");
    }
    assert_eq!(
        called_by(&partial, "servidor/src/pedidos.controller.ts", "GET", "api/pedidos/{}"),
        json!(["tela/src/itens.ts:4:listar", "tela/src/pedidos.ts:4:abrir"])
    );
}

/// O `fetch` solto, sem import, liga ao método que as opções dizem, e sem
/// elas ao `GET`.
#[test]
fn a_bare_fetch_links_to_the_method_its_options_name_or_to_get() {
    let screen = "export async function enviar(corpo: string) {\n  return fetch('/api/pedidos', { method: 'POST', body: corpo });\n}\n\n\
                  export async function listar() {\n  return fetch('/api/pedidos');\n}\n";
    let temp = project_with(&[
        ("servidor/src/pedidos.ts", &express_server(&[("get", "/api/pedidos", "listar"), ("post", "/api/pedidos", "criar")])),
        ("tela/src/pedidos.ts", screen),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(called_by(&map, "servidor/src/pedidos.ts", "POST", "api/pedidos"), json!(["tela/src/pedidos.ts:2:enviar"]));
    assert_eq!(called_by(&map, "servidor/src/pedidos.ts", "GET", "api/pedidos"), json!(["tela/src/pedidos.ts:6:listar"]));
}

/// O caminho guardado numa variável, ou montado sobre ela, não é texto
/// escrito na chamada: nem o cliente nem o `fetch` fazem chamada com ele, e
/// a rota fica sem ligação.
#[test]
fn a_path_kept_in_a_variable_makes_no_call_and_no_link() {
    let screen = "import axios from 'axios';\n\nconst client = axios.create({ baseURL: '/api' });\n\
                  const caminho = '/pedidos';\nconst base = '/api';\n\nexport function carregar() {\n  client.get(caminho);\n  \
                  fetch(caminho);\n  return fetch(`${base}/pedidos`);\n}\n";
    let temp = project_with(&[
        ("servidor/src/pedidos.ts", &express_server(&[("get", "/api/pedidos", "listar")])),
        ("tela/src/pedidos.ts", screen),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(route_calls(&map, "tela/src/pedidos.ts"), json!([]));
    assert_eq!(called_by(&map, "servidor/src/pedidos.ts", "GET", "api/pedidos"), json!([]));
}

/// Um método chamado `fetch` de outro objeto (`repo.fetch('/api/pedidos')`)
/// não é o `fetch` solto: não faz chamada da tela.
#[test]
fn a_method_named_like_the_bare_call_is_not_a_screen_call() {
    let screen = "export function carregar(repo: { fetch(p: string): void }) {\n  return repo.fetch('/api/pedidos');\n}\n";
    let temp = project_with(&[
        ("servidor/src/pedidos.ts", &express_server(&[("get", "/api/pedidos", "listar")])),
        ("tela/src/repo.ts", screen),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(route_calls(&map, "tela/src/repo.ts"), json!([]));
    assert_eq!(called_by(&map, "servidor/src/pedidos.ts", "GET", "api/pedidos"), json!([]));
}

/// O controlador de pedidos de um servidor C#: `GET api/pedidos/{}`, atendido
/// por `Get`, e `POST api/pedidos`, por `Criar`.
const PEDIDOS_CONTROLLER: &str = "[ApiController]\n[Route(\"api/[controller]\")]\npublic class PedidosController : ControllerBase\n{\n    \
                                  [HttpGet(\"{id}\")]\n    public string Get(int id) => \"um\";\n\n    \
                                  [HttpPost]\n    public string Criar() => \"ok\";\n}\n";

/// O servidor C# de pedidos, sob `Api/`, com a tela dada nos `files`.
fn with_orders_api(files: &[(&str, &str)]) -> tempfile::TempDir {
    let mut all: Vec<(&str, &str)> = vec![("Api/Controllers/PedidosController.cs", PEDIDOS_CONTROLLER)];
    all.extend_from_slice(files);
    web_project(&all)
}

/// No JavaScript, o `require('axios')` liga a regra como o import: o cliente
/// feito com a base e chamado com o `${id}` liga provado ao `GET` do
/// controlador. O objeto feito pelo `create` de outra coisa, que não é a
/// biblioteca, não é cliente: a chamada dele não é chamada da tela.
#[test]
fn a_javascript_screen_that_requires_axios_links_proven_to_the_route() {
    let screen = "const axios = require('axios');\n\nconst client = axios.create({ baseURL: '/api' });\n\n\
                  function carregar(id) {\n  return client.get(`/pedidos/${id}`);\n}\n\n\
                  const outro = cache.create({ baseURL: '/api' });\noutro.get('/pedidos/2');\n\nmodule.exports = { carregar };\n";
    let temp = with_orders_api(&[("tela/tela.js", screen)]);
    let (map, _) = scan(temp.path());
    assert_eq!(
        route_calls(&map, "tela/tela.js"),
        json!([{"method": "GET", "path": "pedidos/{}", "written": "/pedidos/${id}", "line": 6, "owner": "carregar",
                "framework": "axios", "base": {"written": "/api", "path": "api"}}])
    );
    assert_eq!(
        called_by(&map, "Api/Controllers/PedidosController.cs", "GET", "api/pedidos/{}"),
        json!(["tela/tela.js:6:carregar"])
    );
}

/// No Dart, o `http` que o import traz chama com o endereço inteiro no
/// `Uri.parse`, que vale sem o esquema e a máquina, e com o `$id` como
/// lacuna; o cliente feito com `http.Client()` chama com o caminho do
/// `Uri.https`. As duas ligam provadas às rotas do controlador.
#[test]
fn a_dart_screen_calling_the_http_package_links_proven_to_the_routes() {
    let screen = "import 'package:http/http.dart' as http;\n\nFuture<String> ler(String id) async {\n  \
                  final resposta = await http.get(Uri.parse('https://api.loja.com/api/pedidos/$id'));\n  return resposta.body;\n}\n\n\
                  Future<void> criar(String corpo) async {\n  final cliente = http.Client();\n  \
                  await cliente.post(Uri.https('api.loja.com', '/api/pedidos'), body: corpo);\n}\n";
    let temp = with_orders_api(&[("app/lib/tela.dart", screen)]);
    let (map, _) = scan(temp.path());
    assert_eq!(
        route_calls(&map, "app/lib/tela.dart"),
        json!([
            {"method": "GET", "path": "api/pedidos/{}", "written": "https://api.loja.com/api/pedidos/$id", "line": 4,
             "owner": "ler", "framework": "dart-http"},
            {"method": "POST", "path": "api/pedidos", "written": "/api/pedidos", "line": 10, "owner": "criar",
             "framework": "dart-http"}
        ])
    );
    let api = "Api/Controllers/PedidosController.cs";
    assert_eq!(called_by(&map, api, "GET", "api/pedidos/{}"), json!(["app/lib/tela.dart:4:ler"]));
    assert_eq!(called_by(&map, api, "POST", "api/pedidos"), json!(["app/lib/tela.dart:10:criar"]));
}

/// O cliente do dio feito com `BaseOptions(baseUrl: …)` põe a base, sem o
/// esquema e a máquina, na frente do caminho que ele chama: liga provado.
#[test]
fn a_dio_client_with_a_base_url_links_proven_to_the_route() {
    let screen = "import 'package:dio/dio.dart';\n\nfinal dio = Dio(BaseOptions(baseUrl: 'https://host/api'));\n\n\
                  Future<void> abrir(String id) async {\n  await dio.get('/pedidos/$id');\n}\n";
    let temp = with_orders_api(&[("app/lib/pedidos.dart", screen)]);
    let (map, _) = scan(temp.path());
    assert_eq!(
        route_calls(&map, "app/lib/pedidos.dart"),
        json!([{"method": "GET", "path": "pedidos/{}", "written": "/pedidos/$id", "line": 6, "owner": "abrir",
                "framework": "dio", "base": {"written": "https://host/api", "path": "api"}}])
    );
    assert_eq!(
        called_by(&map, "Api/Controllers/PedidosController.cs", "GET", "api/pedidos/{}"),
        json!(["app/lib/pedidos.dart:6:abrir"])
    );
}

/// A página Blazor, num projeto cujo SDK traz o `System.Net.Http` sem escrevê-lo,
/// chama o `Http` que recebe do framework sem o declarar: o
/// `GetFromJsonAsync` liga provado ao `GET` do controlador, com o número como
/// lacuna, e o `PostAsJsonAsync`, ao `POST`.
#[test]
fn a_blazor_page_calling_the_http_client_links_proven_to_the_controller() {
    let page = "using Microsoft.AspNetCore.Components;\n\nnamespace Web.Pages;\n\npublic partial class PedidosPage : ComponentBase\n{\n    \
                protected override async Task OnInitializedAsync()\n    {\n        \
                var pedido = await Http.GetFromJsonAsync<string>(\"api/pedidos/1\");\n    }\n\n    \
                private async Task Salvar(Pedido p)\n    {\n        await Http.PostAsJsonAsync(\"api/pedidos\", p);\n    }\n}\n";
    let blazor = csproj("Microsoft.NET.Sdk.BlazorWebAssembly");
    let temp = with_orders_api(&[("Web/Web.csproj", &blazor), ("Web/Pages/PedidosPage.cs", page)]);
    let (map, _) = scan(temp.path());
    let api = "Api/Controllers/PedidosController.cs";
    assert_eq!(called_by(&map, api, "GET", "api/pedidos/{}"), json!(["Web/Pages/PedidosPage.cs:9:OnInitializedAsync"]));
    assert_eq!(called_by(&map, api, "POST", "api/pedidos"), json!(["Web/Pages/PedidosPage.cs:14:Salvar"]));
}

/// O cliente declarado com o tipo e com a base posta depois
/// (`_http.BaseAddress = new Uri(…)`) é um cliente só: o `GetAsync` dele liga
/// provado com a base na frente. O `GetAsync` de outro objeto, que não é
/// cliente, não é chamada da tela.
#[test]
fn a_declared_http_client_with_its_base_set_later_links_and_another_object_does_not() {
    let service = "using System.Net.Http;\n\npublic class PedidosService\n{\n    private readonly HttpClient _http;\n    \
                   private readonly IDistributedCache _cache;\n\n    public PedidosService(HttpClient http, IDistributedCache cache)\n    {\n        \
                   _http = http;\n        _http.BaseAddress = new Uri(\"https://loja.com/api/\");\n        _cache = cache;\n    }\n\n    \
                   public Task<HttpResponseMessage> Ler(int id) => _http.GetAsync($\"pedidos/{id}\");\n\n    \
                   public Task<byte[]> Guardado() => _cache.GetAsync(\"pedidos/1\");\n}\n";
    let temp = with_orders_api(&[("Loja/PedidosService.cs", service)]);
    let (map, _) = scan(temp.path());
    assert_eq!(
        route_calls(&map, "Loja/PedidosService.cs"),
        json!([{"method": "GET", "path": "pedidos/{}", "written": "pedidos/{id}", "line": 15, "owner": "Ler",
                "framework": "httpclient", "base": {"written": "https://loja.com/api/", "path": "api"}}])
    );
    assert_eq!(
        called_by(&map, "Api/Controllers/PedidosController.cs", "GET", "api/pedidos/{}"),
        json!(["Loja/PedidosService.cs:15:Ler"])
    );
}

/// Sem o import do cliente, a chamada com a forma de uma chamada da tela não
/// é chamada da tela: nem no JavaScript, nem no Dart — também com um pacote
/// cujo nome começa pelo do cliente —, nem no C# fora de projeto .NET.
#[test]
fn a_call_in_a_file_that_imports_no_client_is_not_a_screen_call() {
    let files = [
        ("tela/sem.js", "const x = require('outra');\n\nx.get('/api/pedidos/1');\n"),
        ("app/lib/sem.dart", "import 'package:outra/outra.dart' as x;\n\nvoid f() {\n  x.get('/api/pedidos/1');\n}\n"),
        ("app/lib/parecido.dart", "import 'package:http_parser/http_parser.dart' as http;\n\nvoid f() {\n  http.get('/api/pedidos/1');\n}\n"),
        ("Solto/Sem.cs", "public class Sem\n{\n    public void F(dynamic x) => x.GetFromJsonAsync<string>(\"api/pedidos/1\");\n}\n"),
    ];
    let temp = with_orders_api(&files);
    let (map, _) = scan(temp.path());
    for (file, _) in files {
        assert_eq!(route_calls(&map, file), json!([]), "{file}");
    }
    assert_eq!(called_by(&map, "Api/Controllers/PedidosController.cs", "GET", "api/pedidos/{}"), json!([]));
}

// ---------------------------------------------------------------------------
// As rotas dos decoradores e da lista de caminhos
// ---------------------------------------------------------------------------

/// Um roteador com prefixo montado no mesmo arquivo, e as rotas do
/// `api_route`, com a lista de métodos e sem ela.
const FASTAPI: &str = r#"from fastapi import APIRouter, FastAPI

app = FastAPI()
router = APIRouter(prefix="/pedidos")


@router.get("/{id}")
def ler(id):
    return id


@app.api_route("/duplo", methods=["GET", "POST"])
def duplo():
    return 1


@app.api_route("/simples")
def simples():
    return 1


app.include_router(router, prefix="/api")
"#;

#[test]
fn a_decorated_route_joins_the_prefix_of_its_router_and_of_the_include_in_the_same_file() {
    let temp = project_with(&[("loja/api.py", FASTAPI)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "loja/api.py");
    assert!(served.contains(&"GET api/pedidos/{} -> ler".to_string()), "{served:?}");
}

#[test]
fn each_method_written_as_text_in_the_list_is_a_route_and_without_the_list_the_default_one() {
    let temp = project_with(&[("loja/api.py", FASTAPI)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "loja/api.py");
    for route in ["GET duplo -> duplo", "POST duplo -> duplo", "GET simples -> simples"] {
        assert!(served.contains(&route.to_string()), "{route}: {served:?}");
    }
    assert_eq!(served.len(), 4, "the path whose methods are written does not also get the default: {served:?}");
}

/// O roteador escrito num arquivo e montado noutro, pelo nome trazido no
/// import ou pelo módulo (`pedidos.router`); o blueprint, do mesmo jeito.
#[test]
fn a_router_included_from_another_file_takes_the_prefix_of_the_include() {
    let router = "from fastapi import APIRouter\n\nrouter = APIRouter(prefix=\"/pedidos\")\n\n\n\
                  @router.get(\"/{id}\")\ndef ler(id):\n    return id\n";
    let by_name = "from fastapi import FastAPI\n\nfrom .pedidos import router\n\napp = FastAPI()\n\
                   app.include_router(router, prefix=\"/api\")\n";
    let by_module = "from fastapi import FastAPI\n\nfrom loja import pedidos\n\napp = FastAPI()\n\
                     app.include_router(pedidos.router, prefix=\"/api\")\n";
    let blueprint = "from flask import Blueprint\n\nbp = Blueprint('aves', __name__, url_prefix='/aves')\n\n\n\
                     @bp.get('/<int:id>')\ndef ave(id):\n    return id\n";
    let register = "from flask import Flask\n\nfrom .aves import bp\n\napp = Flask(__name__)\n\
                    app.register_blueprint(bp, url_prefix='/api')\n";
    for main in [by_name, by_module] {
        let temp = project_with(&[
            ("loja/__init__.py", ""),
            ("loja/pedidos.py", router),
            ("loja/main.py", main),
            ("site/__init__.py", ""),
            ("site/aves.py", blueprint),
            ("site/app.py", register),
        ]);
        let (map, _) = scan(temp.path());
        assert_eq!(served(&map, "loja/pedidos.py"), ["GET api/pedidos/{} -> ler"], "{main}");
        assert_eq!(served(&map, "site/aves.py"), ["GET api/aves/{} -> ave"]);
    }
}

/// A rota de método escrito na lista, a sem lista e a do blueprint montado.
const FLASK: &str = r#"from flask import Blueprint, Flask

app = Flask(__name__)
bp = Blueprint('pedidos', __name__, url_prefix='/pedidos')


@app.route("/aves/<int:id>", methods=["POST"])
def criar(id):
    return id


@app.route("/x")
def x():
    return 1


@bp.get("/<id>")
def ler(id):
    return id


app.register_blueprint(bp, url_prefix='/api')
"#;

#[test]
fn a_route_with_its_methods_written_takes_them_and_one_without_them_takes_the_default() {
    let temp = project_with(&[("loja/web.py", FLASK)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "loja/web.py");
    assert!(served.contains(&"POST aves/{} -> criar".to_string()), "{served:?}");
    assert!(!served.contains(&"GET aves/{} -> criar".to_string()), "{served:?}");
    assert!(served.contains(&"GET x -> x".to_string()), "{served:?}");
}

#[test]
fn a_blueprint_route_joins_the_prefix_of_the_blueprint_and_of_the_register() {
    let temp = project_with(&[("loja/web.py", FLASK)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "loja/web.py");
    assert!(served.contains(&"GET api/pedidos/{} -> ler".to_string()), "{served:?}");
}

/// A variável de mesmo nome guardada dentro de outra função não é o grupo da
/// rota: só a do topo do arquivo vale nas funções escritas depois dela.
#[test]
fn a_group_kept_inside_another_function_is_not_the_group_of_the_route() {
    let api = "from fastapi import APIRouter\n\nrouter = APIRouter()\n\n\ndef outro():\n    \
               router = APIRouter(prefix=\"/outro\")\n    return router\n\n\n\
               @router.get(\"/x\")\ndef ler():\n    return 1\n";
    let temp = project_with(&[("loja/api.py", api)]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "loja/api.py"), ["GET x -> ler"]);
}

/// A lista de caminhos de um app: a rota do `path`, a do `re_path` e a da
/// visão de classe.
const DJANGO_URLS: &str = r#"from django.urls import path, re_path

from . import views

urlpatterns = [
    path('pedidos/<int:id>/', views.ler_pedido),
    re_path(r'^aves/(?P<id>\d+)/$', views.ave),
    path('classe/', views.PedidoView.as_view(), name='classe'),
]
"#;

const DJANGO_VIEWS: &str = "def ler_pedido(request, id):\n    return id\n\n\ndef ave(request, id):\n    return id\n\n\n\
                            class PedidoView:\n    pass\n";

/// A lista de caminhos do projeto, que inclui a do app sob `api/`.
const DJANGO_ROOT: &str = "from django.urls import include, path\n\nurlpatterns = [\n    path('api/', include('loja.urls')),\n]\n";

fn django_app() -> [(&'static str, &'static str); 3] {
    [("loja/__init__.py", ""), ("loja/urls.py", DJANGO_URLS), ("loja/views.py", DJANGO_VIEWS)]
}

#[test]
fn a_path_in_the_list_is_a_route_of_any_method_served_by_the_view_or_its_class() {
    let temp = project_with(&django_app());
    let (map, _) = scan(temp.path());
    let served = served(&map, "loja/urls.py");
    assert!(served.contains(&"* pedidos/{} -> ler_pedido".to_string()), "{served:?}");
    assert!(served.contains(&"* classe -> PedidoView".to_string()), "{served:?}");
}

#[test]
fn a_regular_expression_path_drops_its_anchors_and_reads_the_named_group_as_a_parameter() {
    let temp = project_with(&django_app());
    let (map, _) = scan(temp.path());
    let served = served(&map, "loja/urls.py");
    assert!(served.contains(&"* aves/{} -> ave".to_string()), "{served:?}");
}

#[test]
fn an_include_adds_its_prefix_to_every_route_of_the_file_the_module_names() {
    let mut files = django_app().to_vec();
    files.extend([("projeto/__init__.py", ""), ("projeto/urls.py", DJANGO_ROOT)]);
    let temp = project_with(&files);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "loja/urls.py"), ["* api/aves/{} -> ave", "* api/classe -> PedidoView", "* api/pedidos/{} -> ler_pedido"]);
    assert_eq!(routes(&map, "projeto/urls.py"), json!([]), "the include is not a route");
}

/// As funções que atendem as rotas das listas incluídas.
const DJANGO_LIST_VIEWS: &str = "def ler(request, id):\n    return id\n\n\ndef listar(request):\n    return 1\n";

/// O app `loja`, com as views e a lista de caminhos `urls`.
fn django_list_app(urls: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![("loja/__init__.py", ""), ("loja/views.py", DJANGO_LIST_VIEWS), ("loja/urls.py", urls)]
}

#[test]
fn a_list_written_inside_an_include_gives_its_routes_under_the_include_prefix() {
    let urls = "from django.urls import include, path\n\nfrom . import views\n\n\
                urlpatterns = [\n    path('api/', include([path('pedidos/<int:id>/', views.ler)])),\n]\n";
    let temp = project_with(&django_list_app(urls));
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "loja/urls.py"), ["* api/pedidos/{} -> ler"]);
}

#[test]
fn a_list_kept_in_a_name_and_included_by_it_gives_its_routes_only_under_the_include_prefix() {
    let urls = "from django.urls import include, path\n\nfrom . import views\n\n\
                extra = [\n    path('pedidos/', views.listar),\n]\n\n\
                urlpatterns = [\n    path('api/', include(extra)),\n]\n";
    let temp = project_with(&django_list_app(urls));
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "loja/urls.py"), ["* api/pedidos -> listar"]);
}

#[test]
fn a_list_brought_from_another_file_gives_its_routes_under_the_include_prefix_where_it_is_written() {
    let api = "from django.urls import path\n\nfrom . import views\n\nextra = [\n    path('pedidos/', views.listar),\n]\n";
    let urls = "from django.urls import include, path\n\nfrom .api import extra\n\n\
                urlpatterns = [\n    path('api/', include(extra)),\n]\n";
    let mut files = django_list_app(urls);
    files.push(("loja/api.py", api));
    let temp = project_with(&files);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "loja/api.py"), ["* api/pedidos -> listar"]);
    assert_eq!(routes(&map, "loja/urls.py"), json!([]), "the include is not a route");
}

#[test]
fn a_list_included_with_the_app_name_in_a_tuple_takes_the_same_prefix() {
    let urls = "from django.urls import include, path\n\nfrom . import views\n\n\
                extra = [\n    path('pedidos/', views.listar),\n]\n\n\
                urlpatterns = [\n    path('api/', include((extra, 'loja'))),\n    \
                path('v1/', include(([path('pedidos/<int:id>/', views.ler)], 'loja'))),\n]\n";
    let temp = project_with(&django_list_app(urls));
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "loja/urls.py"), ["* api/pedidos -> listar", "* v1/pedidos/{} -> ler"]);
}

/// As listas incluídas, escritas ali ou guardadas num nome, num arquivo que
/// não importa o Django.
#[test]
fn an_included_list_of_paths_in_a_file_that_imports_no_django_has_no_route() {
    let urls = "from . import views\n\nextra = [\n    path('pedidos/', views.listar),\n]\n\n\
                urlpatterns = [\n    path('api/', include(extra)),\n    \
                path('v1/', include([path('pedidos/<int:id>/', views.ler)])),\n]\n";
    let temp = project_with(&django_list_app(urls));
    let (map, _) = scan(temp.path());
    assert_eq!(routes(&map, "loja/urls.py"), json!([]));
}

/// O projeto Django dos testes de pilha: a rota da lista `urlpatterns`.
#[test]
fn the_django_fixture_keeps_the_route_of_its_list() {
    let temp = project_with(&[
        ("manage.py", include_str!("fixtures/python_django/manage.py")),
        ("blog/models.py", include_str!("fixtures/python_django/blog/models.py")),
        ("mysite/settings.py", include_str!("fixtures/python_django/mysite/settings.py")),
        ("mysite/urls.py", include_str!("fixtures/python_django/mysite/urls.py")),
    ]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "mysite/urls.py"), ["*  -> index"]);
}

/// O decorador e a lista com a forma de rota, num arquivo que não importa o
/// framework, ou que importa um pacote cujo nome começa pelo dele.
#[test]
fn a_decorated_file_that_imports_no_framework_has_no_route() {
    let cache = "from cache import app\n\n\n@app.get(\"/x\")\ndef ler():\n    return 1\n\n\n\
                 urlpatterns = [\n    path('pedidos/', ler),\n]\n";
    let other = "from flask_caching import app\n\n\n@app.route(\"/x\")\ndef ler():\n    return 1\n";
    let temp = project_with(&[("loja/cache.py", cache), ("loja/outro.py", other)]);
    let (map, _) = scan(temp.path());
    assert_eq!(routes(&map, "loja/cache.py"), json!([]));
    assert_eq!(routes(&map, "loja/outro.py"), json!([]), "a package whose name starts with the framework's is another one");
}

/// A passada que lê só o arquivo mudado soma os prefixos de outros arquivos
/// como a passada inteira: o do include pelo nome e o do include pelo
/// caminho do módulo.
#[test]
fn a_pass_that_reads_only_the_changed_router_gives_the_same_routes_as_the_whole_pass() {
    let router = "from fastapi import APIRouter\n\nrouter = APIRouter(prefix=\"/pedidos\")\n\n\n\
                  @router.get(\"/{id}\")\ndef ler(id):\n    return id\n";
    let main = "from fastapi import FastAPI\n\nfrom .pedidos import router\n\napp = FastAPI()\n\
                app.include_router(router, prefix=\"/api\")\n";
    let mut files = vec![("loja/pedidos.py", router), ("loja/main.py", main)];
    files.extend(django_app());
    files.extend([("projeto/__init__.py", ""), ("projeto/urls.py", DJANGO_ROOT)]);
    let temp = project_with(&files);
    let dir = temp.path();
    let (first, _) = scan(dir);
    assert_eq!(served(&first, "loja/pedidos.py"), ["GET api/pedidos/{} -> ler"]);

    let added = format!("{router}\n\n@router.post(\"/\")\ndef criar():\n    return 1\n");
    let more_urls = DJANGO_URLS.replace("]\n", "    path('novo/', views.ave),\n]\n");
    let steps: [(&str, String, &str, &[&str]); 2] = [
        ("loja/pedidos.py", added, "loja/pedidos.py", &["GET api/pedidos/{} -> ler", "POST api/pedidos -> criar"]),
        (
            "loja/urls.py",
            more_urls,
            "loja/urls.py",
            &["* api/aves/{} -> ave", "* api/classe -> PedidoView", "* api/novo -> ave", "* api/pedidos/{} -> ler_pedido"],
        ),
    ];
    for (path, body, file, expected) in steps {
        std::fs::write(dir.join(path), body).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", path]);
        let (partial, report) = scan(dir);
        assert_eq!(report["full"], json!(false), "{path}: {report}");
        assert_eq!(report["read"], json!([path]), "{path}: {report}");
        let whole_out = tempfile::tempdir().unwrap();
        let (whole, _) = model::scan(dir, whole_out.path(), &[]);
        assert_eq!(served(&whole, file), expected, "{path}: the whole pass");
        for other in ["loja/pedidos.py", "loja/urls.py"] {
            assert_eq!(routes(&partial, other), routes(&whole, other), "{path}: {other}");
        }
    }
}

// ---------------------------------------------------------------------------
// As rotas do Go e as do Actix
// ---------------------------------------------------------------------------

/// O roteador da biblioteca padrão: o caminho sem método e o que o escreve
/// na frente.
const GO_NET_HTTP: &str = "package api\n\nimport \"net/http\"\n\nfunc Rotas() {\n\
                           \thttp.HandleFunc(\"/pedidos/\", ler)\n\tmux := http.NewServeMux()\n\
                           \tmux.HandleFunc(\"GET /aves/{id}\", ler)\n}\n\n\
                           func ler(w http.ResponseWriter, r *http.Request) {}\n";

/// Um motor com um grupo, um grupo feito dele dentro do bloco e uma rota de
/// qualquer método.
const GIN: &str = "package main\n\nimport \"github.com/gin-gonic/gin\"\n\nfunc main() {\n\tr := gin.Default()\n\
                   \tg := r.Group(\"/api\")\n\tg.GET(\"/aves/:id\", lerAve)\n\t{\n\t\tv1 := g.Group(\"/v1\")\n\
                   \t\tv1.GET(\"/x\", lerAve)\n\t}\n\tr.Any(\"/todos\", lerAve)\n\tr.Run()\n}\n\n\
                   func lerAve(c *gin.Context) {}\n";

/// Um arquivo Go sem import de framework, com chamadas que têm a forma das
/// rotas dos dois.
const GO_NO_FRAMEWORK: &str = "package util\n\nimport \"strings\"\n\nfunc Rotas(r Roteador) {\n\
                               \tr.GET(\"/aves/:id\", ler)\n\thttp.HandleFunc(\"/x\", ler)\n\
                               \t_ = strings.TrimSpace(\" \")\n}\n\nfunc ler() {}\n";

#[test]
fn a_standard_library_route_takes_the_method_written_before_its_path_or_any() {
    let temp = project_with(&[("go.mod", "module loja\n\ngo 1.22\n"), ("api/rotas.go", GO_NET_HTTP)]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "api/rotas.go"), ["* pedidos -> ler", "GET aves/{} -> ler"]);
}

#[test]
fn a_route_of_a_group_kept_in_a_variable_joins_every_group_it_was_made_from() {
    let temp = project_with(&[("go.mod", "module loja\n\ngo 1.22\n"), ("main.go", GIN)]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "main.go"), ["* todos -> lerAve", "GET api/aves/{} -> lerAve", "GET api/v1/x -> lerAve"]);
}

#[test]
fn a_go_file_that_imports_no_framework_has_no_route() {
    let temp = project_with(&[("go.mod", "module loja\n\ngo 1.22\n"), ("util/rotas.go", GO_NO_FRAMEWORK)]);
    let (map, _) = scan(temp.path());
    assert_eq!(routes(&map, "util/rotas.go"), json!([]));
}

/// As rotas pelo atributo e pela chamada, montadas num escopo pelo
/// `service` e pelo `configure`.
const ACTIX: &str = r#"use actix_web::{get, route, web, App, HttpServer};

#[get("/aves/{id}")]
async fn ler() -> &'static str {
    "ave"
}

#[route("/multi", method = "GET", method = "POST")]
async fn multi() -> &'static str {
    ""
}

async fn criar() -> &'static str {
    ""
}

fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/x").route(web::post().to(criar)));
    cfg.route("/y", web::get().to(criar));
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .service(multi)
            .service(web::scope("/api").service(ler))
            .service(web::scope("/cfg").configure(config))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
"#;

const ACTIX_CARGO: &str = "[package]\nname = \"loja\"\nversion = \"0.1.0\"\n\n[dependencies]\nactix-web = \"4\"\n";

#[test]
fn an_attribute_route_is_served_by_its_function_and_a_scope_service_adds_its_prefix() {
    let temp = project_with(&[("Cargo.toml", ACTIX_CARGO), ("src/main.rs", ACTIX)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "src/main.rs");
    for route in ["GET api/aves/{} -> ler", "GET multi -> multi", "POST multi -> multi"] {
        assert!(served.contains(&route.to_string()), "{route}: {served:?}");
    }
    assert!(!served.contains(&"GET aves/{} -> ler".to_string()), "{served:?}");
}

#[test]
fn a_resource_route_is_served_by_the_function_of_its_to_and_configure_adds_the_scope() {
    let temp = project_with(&[("Cargo.toml", ACTIX_CARGO), ("src/main.rs", ACTIX)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "src/main.rs");
    for route in ["POST cfg/x -> criar", "GET cfg/y -> criar"] {
        assert!(served.contains(&route.to_string()), "{route}: {served:?}");
    }
    assert_eq!(served.len(), 5, "{served:?}");
}

/// A função com o atributo escrita noutro arquivo, trazida pelo `use` e
/// montada no escopo.
#[test]
fn a_scope_service_adds_its_prefix_to_the_function_of_another_file() {
    let handlers = "use actix_web::get;\n\n#[get(\"/aves/{id}\")]\npub async fn ler() -> &'static str {\n    \"ave\"\n}\n";
    let main = "use actix_web::{web, App};\nuse crate::handlers::ler;\n\nmod handlers;\n\n\
                fn app() {\n    App::new().service(web::scope(\"/api\").service(ler));\n}\n";
    let temp = project_with(&[("Cargo.toml", ACTIX_CARGO), ("src/handlers.rs", handlers), ("src/main.rs", main)]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "src/handlers.rs"), ["GET api/aves/{} -> ler"]);
}

/// Um `src/main.rs` do Actix com a função `ler` e a montagem `app` escrita
/// na função `main`, que registra as rotas.
fn actix_main(app: &str) -> String {
    format!(
        "use actix_web::{{get, web, App}};\n\n#[get(\"/{{id}}\")]\nasync fn ler() -> &'static str {{\n    \"\"\n}}\n\n\
         async fn listar() -> &'static str {{\n    \"\"\n}}\n\n\
         async fn criar() -> &'static str {{\n    \"\"\n}}\n\n\
         fn config(cfg: &mut web::ServiceConfig) {{\n    \
         cfg.service(web::resource(\"/x\").route(web::post().to(criar)));\n    \
         cfg.route(\"/y\", web::get().to(criar));\n}}\n\n\
         fn main() {{\n    {app};\n}}\n"
    )
}

/// As rotas de `src/main.rs` com a montagem `app` que a função `handler`
/// atende.
fn actix_served(app: &str, handler: &str) -> Vec<String> {
    let temp = project_with(&[("Cargo.toml", ACTIX_CARGO), ("src/main.rs", &actix_main(app))]);
    let (map, _) = scan(temp.path());
    let by = format!("-> {handler}");
    served(&map, "src/main.rs").into_iter().filter(|route| route.ends_with(&by)).collect()
}

/// O escopo escrito dentro do `service` de outro escopo soma os dois
/// prefixos, o de fora primeiro.
#[test]
fn a_scope_inside_another_scope_adds_both_prefixes() {
    let served =
        actix_served("App::new().service(web::scope(\"/api\").service(web::scope(\"/pedidos\").service(ler)))", "ler");
    assert_eq!(served, ["GET api/pedidos/{} -> ler"]);
}

/// Três escopos, um dentro do outro: os três prefixos, de fora para dentro.
#[test]
fn three_nested_scopes_add_all_their_prefixes_from_the_outside_in() {
    let served = actix_served(
        "App::new().service(web::scope(\"/api\").service(web::scope(\"/v1\").service(web::scope(\"/pedidos\").service(ler))))",
        "ler",
    );
    assert_eq!(served, ["GET api/v1/pedidos/{} -> ler"]);
}

/// O recurso escrito dentro de um escopo soma o prefixo do escopo ao dele.
#[test]
fn a_resource_inside_a_scope_adds_the_scope_prefix() {
    let served = actix_served(
        "App::new().service(web::scope(\"/api\").service(web::resource(\"/fotos\").route(web::get().to(listar))))",
        "listar",
    );
    assert_eq!(served, ["GET api/fotos -> listar"]);
}

/// O `configure` num escopo dentro de outro leva os dois prefixos às rotas
/// da função que ele nomeia.
#[test]
fn a_configure_in_a_scope_inside_another_adds_both_prefixes() {
    let served = actix_served("App::new().service(web::scope(\"/api\").service(web::scope(\"/v1\").configure(config)))", "criar");
    assert_eq!(served, ["GET api/v1/y -> criar", "POST api/v1/x -> criar"]);
}

/// A função de outro arquivo montada num escopo dentro de outro leva os dois
/// prefixos.
#[test]
fn a_scope_inside_another_adds_both_prefixes_to_the_function_of_another_file() {
    let handlers = "use actix_web::get;\n\n#[get(\"/{id}\")]\npub async fn ler() -> &'static str {\n    \"\"\n}\n";
    let main = "use actix_web::{web, App};\nuse crate::handlers::ler;\n\nmod handlers;\n\n\
                fn app() {\n    App::new().service(web::scope(\"/api\").service(web::scope(\"/pedidos\").service(ler)));\n}\n";
    let temp = project_with(&[("Cargo.toml", ACTIX_CARGO), ("src/handlers.rs", handlers), ("src/main.rs", main)]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "src/handlers.rs"), ["GET api/pedidos/{} -> ler"]);
}

/// O escopo escrito direto no `App::new()` leva só o prefixo dele: o objeto
/// que não é grupo não soma nada.
#[test]
fn a_scope_right_on_the_app_takes_only_its_own_prefix() {
    let served = actix_served("App::new().service(web::scope(\"/pedidos\").service(ler))", "ler");
    assert_eq!(served, ["GET pedidos/{} -> ler"]);
}

// ---------------------------------------------------------------------------
// As rotas do Laravel, do Symfony, do Dart e do Fastify
// ---------------------------------------------------------------------------

/// O arquivo de rotas da API: a rota com a ação na lista, a do texto
/// `Controlador@acao`, a de um grupo com prefixo e a de um recurso.
const LARAVEL_API: &str = r#"<?php

use App\Http\Controllers\PedidoController;
use Illuminate\Support\Facades\Route;

Route::get('/pedidos/{id}', [PedidoController::class, 'show']);
Route::post('/pedidos', 'PedidoController@store');
Route::middleware('auth')->prefix('v1')->group(function () {
    Route::get('/itens/{id?}', [PedidoController::class, 'item']);
});
Route::apiResource('fotos', FotoController::class);
"#;

const LARAVEL_WEB: &str = "<?php\n\nuse Illuminate\\Support\\Facades\\Route;\n\n\
                           Route::get('/inicio', function () {\n    return view('inicio');\n});\n";

const LARAVEL_CONTROLLER: &str = "<?php\n\nnamespace App\\Http\\Controllers;\n\nclass PedidoController\n{\n    \
                                  public function show($id)\n    {\n        return $id;\n    }\n}\n";

fn laravel_project() -> tempfile::TempDir {
    project_with(&[
        ("composer.json", r#"{"require": {"laravel/framework": "^11.0"}}"#),
        ("routes/api.php", LARAVEL_API),
        ("routes/web.php", LARAVEL_WEB),
        ("app/Http/Controllers/PedidoController.php", LARAVEL_CONTROLLER),
    ])
}

#[test]
fn a_route_of_the_api_routes_file_takes_its_prefix_and_the_action_written_in_the_list() {
    let temp = laravel_project();
    let (map, _) = scan(temp.path());
    let api = served(&map, "routes/api.php");
    assert!(api.contains(&"GET api/pedidos/{} -> show".to_string()), "{api:?}");
    assert_eq!(served(&map, "routes/web.php"), ["GET inicio -> "]);
}

#[test]
fn an_action_written_as_controller_at_method_is_served_by_the_method() {
    let temp = laravel_project();
    let (map, _) = scan(temp.path());
    let served = served(&map, "routes/api.php");
    assert!(served.contains(&"POST api/pedidos -> store".to_string()), "{served:?}");
}

#[test]
fn a_prefix_in_the_middle_of_a_chain_goes_in_front_of_the_routes_of_its_group() {
    let temp = laravel_project();
    let (map, _) = scan(temp.path());
    let served = served(&map, "routes/api.php");
    assert!(served.contains(&"GET api/v1/itens/{} -> item".to_string()), "{served:?}");
}

#[test]
fn an_api_resource_gives_each_route_of_the_resource_with_its_action() {
    let temp = laravel_project();
    let (map, _) = scan(temp.path());
    let served: Vec<String> = served(&map, "routes/api.php").into_iter().filter(|r| r.contains("fotos")).collect();
    assert_eq!(
        served,
        [
            "DELETE api/fotos/{} -> destroy",
            "GET api/fotos -> index",
            "GET api/fotos/{} -> show",
            "PATCH api/fotos/{} -> update",
            "POST api/fotos -> store",
            "PUT api/fotos/{} -> update",
        ]
    );
}

#[test]
fn a_php_file_that_imports_no_framework_has_no_route() {
    let file = "<?php\n\nRoute::get('/pedidos/{id}', [PedidoController::class, 'show']);\nRoute::apiResource('fotos', F::class);\n";
    let temp = project_with(&[("routes/api.php", file)]);
    let (map, _) = scan(temp.path());
    assert_eq!(routes(&map, "routes/api.php"), json!([]));
}

const SYMFONY: &str = r#"<?php

namespace App\Controller;

use Symfony\Component\Routing\Attribute\Route;

#[Route('/api')]
class PedidoController
{
    #[Route('/pedidos/{id}', name: 'pedido', methods: ['GET'])]
    public function show(int $id)
    {
        return $id;
    }

    #[Route('/pedidos', name: 'todos')]
    public function todos()
    {
        return 1;
    }
}
"#;

#[test]
fn a_method_attribute_route_joins_the_prefix_of_its_class_and_takes_the_methods_written() {
    let temp = project_with(&[("src/Controller/PedidoController.php", SYMFONY)]);
    let (map, _) = scan(temp.path());
    assert_eq!(
        served(&map, "src/Controller/PedidoController.php"),
        ["* api/pedidos -> todos", "GET api/pedidos/{} -> show"]
    );
}

const SHELF: &str = r#"import 'package:shelf/shelf.dart';
import 'package:shelf_router/shelf_router.dart';

Response ler(Request request, String id) => Response.ok(id);

Router rotas() {
  final api = Router();
  api.get('/pedidos/<id>', ler);
  final app = Router();
  app.mount('/api/', api.call);
  return app;
}

class Aves {
  @Route.get('/aves/<id>')
  Response ave(Request request, String id) => Response.ok(id);

  Router get router => _$AvesRouter(this);
}

class Site {
  @Route.mount('/v1/')
  Router get _aves => Aves().router;
}
"#;

#[test]
fn a_router_mounted_in_another_takes_the_prefix_of_the_mount() {
    let temp = project_with(&[("pubspec.yaml", "name: loja\n"), ("lib/server.dart", SHELF)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "lib/server.dart");
    assert!(served.contains(&"GET api/pedidos/{} -> ler".to_string()), "{served:?}");
}

#[test]
fn an_annotated_route_takes_the_prefix_of_the_getter_that_mounts_its_class() {
    let temp = project_with(&[("pubspec.yaml", "name: loja\n"), ("lib/server.dart", SHELF)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "lib/server.dart");
    assert!(served.contains(&"GET v1/aves/{} -> ave".to_string()), "{served:?}");
}

const FASTIFY: &str = r#"import Fastify from 'fastify';

const f = Fastify();
f.get('/rapido/:id', { schema: {} }, lerRapido);
f.route({ method: 'POST', url: '/rapido', handler: lerRapido });

async function plugin(app) {
  app.get('/itens', lerRapido);
}
f.register(plugin, { prefix: '/api' });

function lerRapido() {}
"#;

#[test]
fn a_fastify_route_is_served_by_its_last_argument_or_by_the_handler_of_its_object() {
    let temp = project_with(&[("rapido.ts", FASTIFY)]);
    let (map, _) = scan(temp.path());
    let served = served(&map, "rapido.ts");
    for route in ["GET rapido/{} -> lerRapido", "POST rapido -> lerRapido"] {
        assert!(served.contains(&route.to_string()), "{route}: {served:?}");
    }
}

#[test]
fn a_registered_plugin_takes_the_prefix_of_the_register_also_in_javascript() {
    let js = FASTIFY.replace("import Fastify from 'fastify';", "const Fastify = require('fastify');");
    let temp = project_with(&[("rapido.ts", FASTIFY), ("rapido.js", &js)]);
    let (map, _) = scan(temp.path());
    for file in ["rapido.ts", "rapido.js"] {
        let served = served(&map, file);
        assert!(served.contains(&"GET api/itens -> lerRapido".to_string()), "{file}: {served:?}");
        assert_eq!(served.len(), 3, "{file}: {served:?}");
    }
}

#[test]
fn a_plugin_from_another_file_takes_the_prefix_of_the_register() {
    let plugin = "import { FastifyInstance } from 'fastify';\n\nexport async function pedidos(app: FastifyInstance) {\n  \
                  app.get('/itens/:id', ler);\n}\n\nfunction ler() {}\n";
    let server = "import Fastify from 'fastify';\nimport { pedidos } from './pedidos';\n\nconst f = Fastify();\n\
                  f.register(pedidos, { prefix: '/api' });\n";
    let temp = project_with(&[("src/pedidos.ts", plugin), ("src/server.ts", server)]);
    let (map, _) = scan(temp.path());
    assert_eq!(served(&map, "src/pedidos.ts"), ["GET api/itens/{} -> ler"]);
}
