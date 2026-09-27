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
