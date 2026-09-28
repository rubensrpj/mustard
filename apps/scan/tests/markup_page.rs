//! A página de marcação com código dentro, lida pelo scan de verdade: a
//! página `.razor` do Blazor, num projeto com o servidor de pedidos em C#. O
//! código dos blocos `@code` entra no mapa com as linhas da página, na classe
//! com o nome do arquivo, e as chamadas da tela escritas nele ligam às rotas
//! do servidor. A marcação da página não é código.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use mustard_core::io::project_map as store;
use serde_json::{json, Value};

const CONTROLLER: &str = "[ApiController]\n[Route(\"api/[controller]\")]\npublic class PedidosController : ControllerBase\n{\n    \
                          [HttpGet(\"{id}\")]\n    public string Get(int id) => \"um\";\n\n    \
                          [HttpPost]\n    public string Criar() => \"ok\";\n}\n";

/// O `.csproj` com o SDK `sdk`.
fn csproj(sdk: &str) -> String {
    format!("<Project Sdk=\"{sdk}\">\n  <PropertyGroup>\n    <TargetFramework>net8.0</TargetFramework>\n  </PropertyGroup>\n</Project>\n")
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

/// O servidor de pedidos em `Api/` e a tela Blazor em `Web/`, com as páginas
/// `pages`, no git, lidos pelo scan; devolve o mapa.
fn scanned(pages: &[(&str, &str)]) -> Value {
    let temp = tempfile::Builder::new().prefix("scan-pagina-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    std::fs::create_dir_all(dir.join(".git").join("info")).unwrap();
    std::fs::write(dir.join(".git").join("info").join("exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();
    let mut files: Vec<(String, String)> = vec![
        ("Api/Api.csproj".to_string(), csproj("Microsoft.NET.Sdk.Web")),
        ("Api/Controllers/PedidosController.cs".to_string(), CONTROLLER.to_string()),
        ("Web/Web.csproj".to_string(), csproj("Microsoft.NET.Sdk.BlazorWebAssembly")),
    ];
    files.extend(pages.iter().map(|(path, body)| ((*path).to_string(), (*body).to_string())));
    for (rel, body) in &files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    model::scan(dir, &dir.join(".claude"), &[]).0
}

/// O arquivo `path` do mapa.
fn module<'a>(map: &'a Value, path: &str) -> &'a Value {
    map["modules"].as_array().unwrap().iter().find(|m| m["path"] == json!(path)).unwrap_or_else(|| panic!("{path} no mapa"))
}

/// As declarações do arquivo como `tipo nome linha-fim`.
fn declarations(map: &Value, path: &str) -> Vec<String> {
    let decls = module(map, path)["declarations"].as_array().cloned().unwrap_or_default();
    decls.iter().map(|d| format!("{} {} {}-{}", d["kind"].as_str().unwrap(), d["name"].as_str().unwrap(), d["line"], d["end_line"])).collect()
}

/// Quem chama a rota `method path` do controlador de pedidos.
fn called_by(map: &Value, method: &str, path: &str) -> Value {
    let routes = module(map, "Api/Controllers/PedidosController.cs")["routes"].clone();
    let route = routes.as_array().unwrap().iter().find(|r| r["method"] == json!(method) && r["path"] == json!(path));
    route.unwrap_or_else(|| panic!("{method} {path}: {routes}")).get("called_by").cloned().unwrap_or(json!([]))
}

const PAGE: &str = "@page \"/pedidos\"\n\
                    @using System.Net.Http.Json\n\
                    @inject HttpClient Http\n\
                    \n\
                    <h3>Pedidos</h3>\n\
                    <p>Fale com contato@code.com</p>\n\
                    <button @onclick=\"Salvar\">Salvar</button>\n\
                    \n\
                    @code {\n    \
                    private string? pedido;\n\
                    \n    \
                    protected override async Task OnInitializedAsync()\n    {\n        \
                    pedido = await Http.GetFromJsonAsync<string>(\"api/pedidos/1\");\n    }\n\
                    \n    \
                    private async Task Salvar()\n    {\n        \
                    await Http.PostAsJsonAsync(\"api/pedidos\", pedido);\n    }\n\
                    \n    \
                    private Task<HttpResponseMessage> Ler(int id) => Http.GetAsync($\"api/pedidos/{id}\");\n\
                    }\n";

/// O código do bloco `@code` entra no mapa com as linhas da página, dentro
/// da classe com o nome do arquivo, e o `@inject` é um membro dela. As
/// chamadas da tela escritas no bloco ligam provadas às rotas do
/// controlador, cada uma com a linha e a função da página. O `GetAsync`,
/// nome que outras bibliotecas também usam, só conta porque o `@inject`
/// declara o `Http` como `HttpClient`.
#[test]
fn a_blazor_page_calling_the_server_in_its_code_block_links_proven_to_the_controller() {
    let map = scanned(&[("Web/Pages/Pedidos.razor", PAGE)]);
    let page = "Web/Pages/Pedidos.razor";
    assert_eq!(module(&map, page)["language"], json!("razor"));
    assert_eq!(
        declarations(&map, page),
        ["class Pedidos 3-23", "field Http 3-3", "field pedido 10-10", "method OnInitializedAsync 12-15", "method Salvar 17-20", "method Ler 22-22"]
    );
    assert_eq!(module(&map, page)["imports"], json!(["System.Net.Http.Json"]));
    assert_eq!(
        called_by(&map, "GET", "api/pedidos/{}"),
        json!(["Web/Pages/Pedidos.razor:14:OnInitializedAsync", "Web/Pages/Pedidos.razor:22:Ler"])
    );
    assert_eq!(called_by(&map, "POST", "api/pedidos"), json!(["Web/Pages/Pedidos.razor:19:Salvar"]));
}

/// A marcação da página não é código, nem a escrita entre dois blocos: o
/// exemplo de chamada mostrado na tela e o endereço com o marcador no meio
/// não põem declaração nem chamada no mapa. Só o código dos blocos entra, na
/// mesma classe.
#[test]
fn the_markup_of_a_page_is_not_read_as_code() {
    let help = "@page \"/ajuda\"\n\
                @inject HttpClient Http\n\
                \n\
                @code {\n    \
                private string? texto;\n\
                }\n\
                <p>Exemplo: var um = await Http.GetStringAsync(\"api/pedidos/1\");</p>\n\
                <p>Escreva para ajuda@code.com ou use @functions sem chave.</p>\n\
                @code {\n    \
                private Task Enviar() => Http.PostAsJsonAsync(\"api/pedidos\", texto);\n\
                }\n";
    let map = scanned(&[("Web/Pages/Ajuda.razor", help)]);
    let page = "Web/Pages/Ajuda.razor";
    assert_eq!(declarations(&map, page), ["class Ajuda 2-11", "field Http 2-2", "field texto 5-5", "method Enviar 10-10"]);
    assert_eq!(called_by(&map, "GET", "api/pedidos/{}"), json!([]));
    assert_eq!(called_by(&map, "POST", "api/pedidos"), json!(["Web/Pages/Ajuda.razor:10:Enviar"]));
}

/// A história por função de uma página se lê como a de um arquivo de
/// código: cada commit vai ao método do bloco que ele mudou, pelas linhas
/// da página, e a classe da página tem o nome do arquivo em cada versão.
#[test]
fn the_history_of_a_page_goes_to_the_method_its_commit_changed() {
    let temp = tempfile::Builder::new().prefix("scan-pagina-historia-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join(".git").join("info").join("exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();
    std::fs::write(dir.join("mustard.json"), json!({"git": {"flow": {"*": "main"}}}).to_string()).unwrap();
    let page = |total: u32| {
        format!(
            "@page \"/pedidos\"\n\n<h3>Pedidos</h3>\n\n@code {{\n    private int Salvar()\n    {{\n        return {total};\n    }}\n\n    \
             private int Ler() => 2;\n}}\n"
        )
    };
    for (total, title) in [(1, "cria a página"), (3, "muda o salvar")] {
        std::fs::create_dir_all(dir.join("Pages")).unwrap();
        std::fs::write(dir.join("Pages/Pedidos.razor"), page(total)).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", title]);
    }
    model::scan(dir, &dir.join(".claude"), &[]);
    let model = model::path_in(&dir.join(".claude"));
    let run = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["history", dir.to_str().unwrap(), "--out", model.to_str().unwrap(), "--file", "Pages/Pedidos.razor", "--json"])
        .output()
        .expect("run scan history");
    assert!(run.status.success(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    let map = store::read_at(&model).expect("the map reads");
    let lineage = map.lineage.iter().find(|found| found.path == "Pages/Pedidos.razor").expect("the page's history");
    let titles = |name: &str| -> Vec<String> {
        let decl = lineage.declarations.iter().find(|decl| decl.name == name).unwrap_or_else(|| panic!("{name}: {lineage:?}"));
        decl.commits
            .iter()
            .map(|change| lineage.commits.iter().find(|commit| commit.id == change.id).unwrap().title.clone())
            .collect()
    };
    assert_eq!(titles("Salvar"), ["muda o salvar", "cria a página"]);
    assert_eq!(titles("Ler"), ["cria a página"]);
    assert_eq!(titles("Pedidos"), ["cria a página"], "the changed line goes to the innermost declaration");
}


/// A declaração `name` do arquivo `path` do mapa.
fn declaration<'a>(map: &'a Value, path: &str, name: &str) -> &'a Value {
    let decls = module(map, path)["declarations"].as_array().unwrap();
    decls.iter().find(|d| d["name"] == json!(name)).unwrap_or_else(|| panic!("{name} em {path}: {decls:?}"))
}

/// A página do Razor Pages: o modelo, o `@model`, dois `@using`, o
/// `@inject`, o bloco de corpo que chama o modelo e o `@functions` que chama
/// o servidor, com texto acentuado na marcação.
const RAZOR_PAGE: &str = "@page\n\
                          @model PedidosModel\n\
                          @using System.Net.Http.Json\n\
                          @using Web.Pages\n\
                          @inject HttpClient Http\n\
                          @{\n    \
                          ViewData[\"Title\"] = \"Pedidos em aberto\";\n    \
                          var total = Model.Total();\n\
                          }\n\
                          <h1>@ViewData[\"Title\"] — ação à vista</h1>\n\
                          <p>Total: @total</p>\n\
                          @functions {\n    \
                          private Task<string?> Ler() => Http.GetFromJsonAsync<string>(\"api/pedidos/1\");\n\
                          }\n";

/// A classe que atende a página, no `.cshtml.cs` ao lado.
const PAGE_MODEL: &str = "namespace Web.Pages;\n\npublic class PedidosModel : PageModel\n{\n    public int Total() => 3;\n}\n";

/// A página `.cshtml` entra no mapa com as linhas dela, na classe com o nome
/// do arquivo: o bloco `@{ }` é o corpo do método `ExecuteAsync`, o
/// `@model` é a propriedade `Model` com o tipo do modelo, o `@inject` e o
/// `@functions` são membros. O `@using` da página alcança o C# do projeto: o
/// arquivo do modelo é importado, e a chamada do bloco liga ao `Total` dele.
/// A chamada do `@functions` liga provada à rota do controlador. A view do
/// MVC só com o bloco de corpo tem a classe e o método; o `_ViewImports`,
/// só com linhas de cabeça, só os imports.
#[test]
fn a_razor_page_maps_its_body_block_as_a_method_and_reaches_the_csharp_of_the_project() {
    let view = "@{\n    ViewData[\"Title\"] = \"Início\";\n}\n<h1>Olá, @User.Identity?.Name</h1>\n";
    let map = scanned(&[
        ("Web/Pages/Pedidos.cshtml", RAZOR_PAGE),
        ("Web/Pages/Pedidos.cshtml.cs", PAGE_MODEL),
        ("Web/Pages/_ViewImports.cshtml", "@using Web\n@addTagHelper *, Microsoft.AspNetCore.Mvc.TagHelpers\n"),
        ("Web/Views/Home/Index.cshtml", view),
    ]);
    let page = "Web/Pages/Pedidos.cshtml";
    assert_eq!(module(&map, page)["language"], json!("cshtml"));
    assert_eq!(
        declarations(&map, page),
        ["class Pedidos 2-14", "field Model 2-2", "field Http 5-5", "method ExecuteAsync 6-9", "method Ler 13-13"]
    );
    assert_eq!(declaration(&map, page, "Model")["signature"], json!("PedidosModel Model"));
    assert_eq!(module(&map, page)["imports"], json!(["System.Net.Http.Json", "Web.Pages"]));
    assert_eq!(module(&map, page)["deps"], json!(["Web/Pages/Pedidos.cshtml.cs"]));
    assert_eq!(declaration(&map, page, "ExecuteAsync")["calls"], json!(["Total"]));
    assert_eq!(called_by(&map, "GET", "api/pedidos/{}"), json!(["Web/Pages/Pedidos.cshtml:13:Ler"]));
    assert_eq!(declarations(&map, "Web/Views/Home/Index.cshtml"), ["class Index 1-3", "method ExecuteAsync 1-3"]);
    assert_eq!(declarations(&map, "Web/Pages/_ViewImports.cshtml"), Vec::<String>::new());
    assert_eq!(module(&map, "Web/Pages/_ViewImports.cshtml")["imports"], json!(["Web"]));
}

/// O `global using` escrito num `.cs` do projeto alcança a página, como
/// alcança os outros arquivos C# dele: a chamada do bloco de corpo liga à
/// classe do namespace que ele traz, e o `System.Net.Http` liga a regra do
/// `HttpClient` na página, num projeto cujo SDK não a liga sozinho. A
/// passada que lê só o que mudou relê a página, que não mudou, e dá o mesmo
/// que a passada inteira.
#[test]
fn a_global_using_of_the_project_reaches_the_page_also_in_the_pass_that_reads_only_what_changed() {
    let temp = tempfile::Builder::new().prefix("scan-pagina-global-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    std::fs::write(dir.join(".git").join("info").join("exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();
    let files = [
        ("Web/Web.csproj", csproj("MSBuild.Sdk.Extras")),
        ("Web/Modelos/Calculo.cs", "namespace Web.Modelos;\n\npublic static class Calculo\n{\n    public static int Total() => 3;\n}\n".to_string()),
        (
            "Web/Pages/Resumo.cshtml",
            "@page\n@inject HttpClient Http\n@{\n    var total = Calculo.Total();\n}\n<p>@total</p>\n\
             @functions {\n    private Task<string> Ler() => Http.GetStringAsync(\"api/pedidos/1\");\n}\n"
                .to_string(),
        ),
    ];
    for (rel, body) in &files {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), body).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let page = "Web/Pages/Resumo.cshtml";
    let calls = |map: &Value| declaration(map, page, "ExecuteAsync").get("calls").cloned().unwrap_or(json!([]));
    let client = |map: &Value| -> Vec<String> {
        let found = module(map, page).get("route_calls").and_then(Value::as_array).cloned().unwrap_or_default();
        found.iter().map(|call| format!("{} {} {}:{}", call["method"], call["path"], call["owner"], call["line"])).collect()
    };
    let (first, _) = model::scan(dir, &dir.join(".claude"), &[]);
    assert_eq!(calls(&first), json!([]), "without the global using the page does not see the namespace");
    assert_eq!(client(&first), Vec::<String>::new(), "without the global using the client rule is off");

    std::fs::write(dir.join("Web/GlobalUsings.cs"), "global using System.Net.Http;\nglobal using Web.Modelos;\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "global"]);
    let (partial, report) = model::scan(dir, &dir.join(".claude"), &[]);
    assert_eq!(report["full"], json!(false), "{report}");
    let whole_out = tempfile::tempdir().unwrap();
    let (whole, _) = model::scan(dir, whole_out.path(), &[]);
    assert_eq!(calls(&whole), json!(["Total"]), "the whole pass");
    assert_eq!(client(&whole), ["\"GET\" \"api/pedidos/{}\" \"Ler\":8"], "the whole pass");
    assert_eq!(calls(&partial), json!(["Total"]), "the pass that reads only what changed");
    assert_eq!(client(&partial), client(&whole), "the pass that reads only what changed");
}
