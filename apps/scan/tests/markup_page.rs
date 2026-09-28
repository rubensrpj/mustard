//! A página de marcação com código dentro, lida pelo scan de verdade: a
//! página `.razor` do Blazor e a `.cshtml` do Razor, num projeto com o
//! servidor de pedidos em C#. O código dos blocos entra no mapa com as linhas
//! da página, na classe com o nome do arquivo, e as chamadas da tela escritas
//! nele ligam às rotas do servidor. As expressões escritas no meio da
//! marcação são comandos do método que desenha a página, o comentário da
//! marcação é comentário, as bases da página são as da classe, e o `@using`
//! do arquivo de imports da pasta vale nas páginas dela. O resto da marcação
//! não é código.

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
/// da classe com o nome do arquivo, e o `@inject` é um membro dela. O
/// `@onclick` da marcação mora no método que desenha a página. As chamadas
/// da tela escritas no bloco ligam provadas às rotas do controlador, cada uma
/// com a linha e a função da página. O `GetAsync`, nome que outras
/// bibliotecas também usam, só conta porque o `@inject` declara o `Http`
/// como `HttpClient`.
#[test]
fn a_blazor_page_calling_the_server_in_its_code_block_links_proven_to_the_controller() {
    let map = scanned(&[("Web/Pages/Pedidos.razor", PAGE)]);
    let page = "Web/Pages/Pedidos.razor";
    assert_eq!(module(&map, page)["language"], json!("razor"));
    assert_eq!(
        declarations(&map, page),
        [
            "class Pedidos 3-23",
            "field Http 3-3",
            "method BuildRenderTree 7-7",
            "field pedido 10-10",
            "method OnInitializedAsync 12-15",
            "method Salvar 17-20",
            "method Ler 22-22"
        ]
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
/// do arquivo: o bloco `@{ }`, com as expressões da marcação logo depois
/// dele, é o corpo do método `ExecuteAsync`, o
/// `@model` é a propriedade `Model` com o tipo do modelo, o `@inject` e o
/// `@functions` são membros. O `@using` da página alcança o C# do projeto: o
/// arquivo do modelo é importado, e a chamada do bloco liga ao `Total` dele.
/// A chamada do `@functions` liga provada à rota do controlador. A view do
/// MVC com o bloco de corpo e uma expressão tem a classe e o método; o
/// `_ViewImports`, só com linhas de cabeça, só os imports.
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
        ["class Pedidos 2-14", "field Model 2-2", "field Http 5-5", "method ExecuteAsync 6-11", "method Ler 13-13"]
    );
    assert_eq!(declaration(&map, page, "Model")["signature"], json!("PedidosModel Model"));
    assert_eq!(module(&map, page)["imports"], json!(["System.Net.Http.Json", "Web.Pages"]));
    assert_eq!(module(&map, page)["deps"], json!(["Web/Pages/Pedidos.cshtml.cs"]));
    assert_eq!(declaration(&map, page, "ExecuteAsync")["calls"], json!(["Total"]));
    assert_eq!(called_by(&map, "GET", "api/pedidos/{}"), json!(["Web/Pages/Pedidos.cshtml:13:Ler"]));
    assert_eq!(declarations(&map, "Web/Views/Home/Index.cshtml"), ["class Index 1-4", "method ExecuteAsync 1-4"]);
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

/// Os usos provados da declaração `name` do arquivo `path`, como
/// `arquivo:linha:de onde`.
fn proven_uses(map: &Value, path: &str, name: &str) -> Vec<String> {
    let used = declaration(map, path, name)["used_by"].as_array().cloned().unwrap_or_default();
    used.iter().filter_map(|u| u.as_str().map(str::to_string)).collect()
}

/// Todos os usos da declaração, provados ou suspeitos, só pelo lugar.
fn every_use(map: &Value, path: &str, name: &str) -> Vec<String> {
    let used = declaration(map, path, name)["used_by"].as_array().cloned().unwrap_or_default();
    used.iter().filter_map(|u| u.as_str().or_else(|| u["at"].as_str()).map(str::to_string)).collect()
}

/// O modelo da página de resumo, no `.cshtml.cs` ao lado dela.
const SUMMARY_MODEL: &str =
    "namespace Web.Pages;\n\npublic class ResumoModel : PageModel\n{\n    public int Total() => 3;\n\n    public int Dobro(int x) => x * 2;\n}\n";

/// A página de resumo: o total pelo modelo, a ajuda do HTML com a função da
/// página dentro, a expressão entre parênteses e, na mesma tela, o endereço
/// e o marcador escrito duas vezes, que não são código.
const SUMMARY: &str = "@page\n\
                       @model ResumoModel\n\
                       @using Web.Pages\n\
                       <h1>Total: @Model.Total()</h1>\n\
                       <p>@Html.Raw(Formatar(Model.Dobro(2)))</p>\n\
                       <p>@(Model.Total() + 1)</p>\n\
                       <p>Escreva para ajuda@Formatar(1).com ou @@Formatar(2)</p>\n\
                       @functions {\n    \
                       private string Formatar(int x) => x.ToString();\n\
                       }\n";

/// O contador do Blazor: o campo mostrado, os botões que ligam um método
/// pelo nome e por uma função anônima, e a chamada ao servidor escrita entre
/// parênteses na marcação.
const COUNTER: &str = "@page \"/contador\"\n\
                       @inject HttpClient Http\n\
                       <p>@contagem</p>\n\
                       <button @onclick=\"Somar\">+</button>\n\
                       <button @onclick=\"() => Zerar(0)\">0</button>\n\
                       <p>@(await Http.GetStringAsync(\"api/pedidos/1\"))</p>\n\
                       @code {\n    \
                       private int contagem;\n    \
                       private void Somar() => contagem++;\n    \
                       private void Zerar(int x) => contagem = x;\n\
                       }\n";

/// As expressões escritas no meio da marcação são comandos do método que
/// desenha a página: o total mostrado pelo modelo (`@Model.Total()`) e o
/// método do modelo chamado dentro da ajuda do HTML são usados pelo
/// `ExecuteAsync`, com a linha da página; a função da página chamada dentro
/// de `@Html.Raw(...)` também. O endereço e o marcador escrito duas vezes não
/// são uso. No Blazor, o `@onclick` com o nome do método e com a função
/// anônima são usos pelo `BuildRenderTree`, e a chamada ao servidor escrita
/// em `@(...)` liga à rota do controlador.
#[test]
fn the_expressions_of_the_markup_are_uses_by_the_method_that_draws_the_page() {
    let map = scanned(&[
        ("Web/Pages/Resumo.cshtml", SUMMARY),
        ("Web/Pages/Resumo.cshtml.cs", SUMMARY_MODEL),
        ("Web/Pages/Contador.razor", COUNTER),
    ]);
    let summary = "Web/Pages/Resumo.cshtml";
    assert_eq!(declarations(&map, summary), ["class Resumo 2-10", "field Model 2-2", "method ExecuteAsync 4-6", "method Formatar 9-9"]);
    assert_eq!(
        every_use(&map, "Web/Pages/Resumo.cshtml.cs", "Total"),
        ["Web/Pages/Resumo.cshtml:4:ExecuteAsync", "Web/Pages/Resumo.cshtml:6:ExecuteAsync"]
    );
    assert_eq!(every_use(&map, "Web/Pages/Resumo.cshtml.cs", "Dobro"), ["Web/Pages/Resumo.cshtml:5:ExecuteAsync"]);
    assert_eq!(proven_uses(&map, summary, "Formatar"), ["Web/Pages/Resumo.cshtml:5:ExecuteAsync"]);
    let counter = "Web/Pages/Contador.razor";
    assert_eq!(proven_uses(&map, counter, "Somar"), ["Web/Pages/Contador.razor:4:BuildRenderTree"]);
    assert_eq!(proven_uses(&map, counter, "Zerar"), ["Web/Pages/Contador.razor:5:BuildRenderTree"]);
    assert_eq!(called_by(&map, "GET", "api/pedidos/{}"), json!(["Web/Pages/Contador.razor:6:BuildRenderTree"]));
}

/// O comentário da marcação (`@* … *@`) é comentário: o texto dele entra no
/// mapa como comentário da página, e nada escrito dentro dele é código — nem
/// a expressão, nem o bloco comentado inteiro, de várias linhas.
#[test]
fn a_markup_comment_is_a_comment_and_nothing_in_it_is_code() {
    let page = "@page\n\
                @* Mostra o resumo do pedido *@\n\
                <p>@Formatar(1)</p>\n\
                @* <p>@Esconder(2)</p> *@\n\
                @*\n\
                @functions { private int Oculto() => 1; }\n\
                *@\n\
                @functions {\n    \
                private string Formatar(int x) => x.ToString();\n    \
                private string Esconder(int x) => x.ToString();\n\
                }\n";
    let map = scanned(&[("Web/Pages/Nota.cshtml", page)]);
    let path = "Web/Pages/Nota.cshtml";
    assert_eq!(declarations(&map, path), ["class Nota 3-11", "method ExecuteAsync 3-3", "method Formatar 9-9", "method Esconder 10-10"]);
    assert_eq!(proven_uses(&map, path, "Formatar"), ["Web/Pages/Nota.cshtml:3:ExecuteAsync"]);
    assert_eq!(every_use(&map, path, "Esconder"), Vec::<String>::new());
    let written = format!("{} {}", module(&map, path)["file_doc"], module(&map, path)["file_comment"]);
    assert!(written.contains("Mostra o resumo do pedido"), "{written}");
}

/// O `@inherits` e o `@implements` da página são as bases da classe dela:
/// a classe da página herda do tipo do projeto e cumpre a interface, e o
/// método da página que a interface pede é quem a implementa.
#[test]
fn the_inherits_and_implements_of_a_page_are_the_bases_of_its_class() {
    let layout = "@using Web.Shared\n\
                  @inherits LayoutBase\n\
                  @implements IFechavel\n\
                  <main>@Corpo</main>\n\
                  @code {\n    \
                  public void Fechar() { }\n\
                  }\n";
    let map = scanned(&[
        ("Web/Shared/MainLayout.razor", layout),
        ("Web/Shared/LayoutBase.cs", "namespace Web.Shared;\n\npublic abstract class LayoutBase\n{\n    public string Corpo => \"\";\n}\n"),
        ("Web/Shared/IFechavel.cs", "namespace Web.Shared;\n\npublic interface IFechavel\n{\n    void Fechar();\n}\n"),
    ]);
    let page = "Web/Shared/MainLayout.razor";
    assert_eq!(declaration(&map, page, "MainLayout")["supertypes"], json!(["IFechavel", "LayoutBase"]));
    assert_eq!(declaration(&map, page, "Fechar")["implements"], json!(["Web/Shared/IFechavel.cs:5:Fechar"]));
    assert_eq!(declaration(&map, "Web/Shared/IFechavel.cs", "Fechar")["implemented_by"], json!(["Web/Shared/MainLayout.razor:6:Fechar"]));
}

/// O `@using` do arquivo de imports da pasta (`_ViewImports.cshtml` e, no
/// Blazor, `_Imports.razor`) vale nas páginas da mesma língua da pasta dele e
/// das de baixo: a chamada à classe do namespace que ele traz liga provada
/// nelas. A view de outra pasta e a página de outra língua na mesma pasta
/// (`.razor` ao lado do `_ViewImports.cshtml`) não o veem. A passada que lê só o que
/// mudou, depois de o arquivo de imports nascer, relê as páginas e liga
/// igual.
#[test]
fn the_using_of_the_folder_imports_file_reaches_the_pages_of_the_folder_and_below() {
    let temp = tempfile::Builder::new().prefix("scan-pagina-imports-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    std::fs::write(dir.join(".git").join("info").join("exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();
    let files = [
        ("Web/Web.csproj", csproj("Microsoft.NET.Sdk.Web")),
        ("Web/Modelos/Calculo.cs", "namespace Web.Modelos;\n\npublic static class Calculo\n{\n    public static int Total() => 3;\n}\n".to_string()),
        ("Web/Pages/Resumo.cshtml", "@page\n<p>@Calculo.Total()</p>\n".to_string()),
        ("Web/Pages/Pedidos/Lista.cshtml", "@page\n<p>@Calculo.Total()</p>\n".to_string()),
        ("Web/Pages/Tela.razor", "<p>@Calculo.Total()</p>\n".to_string()),
        ("Web/Views/Home/Index.cshtml", "<p>@Calculo.Total()</p>\n".to_string()),
        ("Web/Shared/Menu/Item.razor", "<p>@Calculo.Total()</p>\n".to_string()),
    ];
    for (rel, body) in &files {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), body).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let total = "Web/Modelos/Calculo.cs";
    let (first, _) = model::scan(dir, &dir.join(".claude"), &[]);
    assert_eq!(every_use(&first, total, "Total"), Vec::<String>::new(), "without the imports file no page sees the namespace");

    std::fs::write(dir.join("Web/Pages/_ViewImports.cshtml"), "@using Web.Modelos\n@addTagHelper *, Microsoft.AspNetCore.Mvc.TagHelpers\n").unwrap();
    std::fs::write(dir.join("Web/Shared/_Imports.razor"), "@using Web.Modelos\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "imports"]);
    let (partial, report) = model::scan(dir, &dir.join(".claude"), &[]);
    assert_eq!(report["full"], json!(false), "{report}");
    let whole_out = tempfile::tempdir().unwrap();
    let (whole, _) = model::scan(dir, whole_out.path(), &[]);
    let expected = [
        "Web/Pages/Pedidos/Lista.cshtml:2:ExecuteAsync",
        "Web/Pages/Resumo.cshtml:2:ExecuteAsync",
        "Web/Shared/Menu/Item.razor:1:BuildRenderTree",
    ];
    assert_eq!(proven_uses(&whole, total, "Total"), expected, "the whole pass");
    assert_eq!(every_use(&whole, total, "Total"), expected, "the other folder and the other language do not see it");
    assert_eq!(proven_uses(&partial, total, "Total"), expected, "the pass that reads only what changed");
}

/// A classe C# escrita sem namespace está no namespace de todos: a página e
/// o outro arquivo C# do mesmo projeto a chamam sem `using`, e a chamada liga
/// provada. A classe de mesmo nome, sem namespace, de outro projeto do
/// repositório não fica à vista e não ganha uso.
#[test]
fn a_csharp_class_without_a_namespace_is_seen_by_the_whole_project() {
    let format = "public static class Formato\n{\n    public static string Moeda(int x) => x.ToString();\n}\n";
    let map = scanned(&[
        ("Web/Formato.cs", format),
        ("Web/Pages/Preco.cshtml", "@page\n<p>@Formato.Moeda(3)</p>\n@{\n    var texto = Formato.Moeda(4);\n}\n"),
        ("Web/Servicos/Pedidos.cs", "namespace Web.Servicos;\n\npublic class Pedidos\n{\n    public string Ver() => Formato.Moeda(1);\n}\n"),
        ("Outro/Outro.csproj", &csproj("Microsoft.NET.Sdk")),
        ("Outro/Formato.cs", format),
    ]);
    assert_eq!(
        proven_uses(&map, "Web/Formato.cs", "Moeda"),
        ["Web/Pages/Preco.cshtml:2:ExecuteAsync", "Web/Pages/Preco.cshtml:4:ExecuteAsync", "Web/Servicos/Pedidos.cs:5:Ver"]
    );
    assert_eq!(every_use(&map, "Outro/Formato.cs", "Moeda"), Vec::<String>::new(), "another project does not see it");
}

/// O modelo da página de lista, no `.cshtml.cs` ao lado dela: um método para
/// cada cabeçalho de comando de controle e para cada trecho de código da
/// página.
const LIST_MODEL: &str = "namespace Web.Pages;\n\npublic class ListaModel : PageModel\n{\n    \
                          public bool Visivel() => true;\n    public bool Outro() => false;\n    \
                          public int[] Itens() => new int[0];\n    public int Tipo() => 1;\n    \
                          public bool Mais() => false;\n    public int Soma(int x) => x;\n    \
                          public string Titulo { get; set; } = \"\";\n}\n";

/// A página de lista: `@if` com `else if`, `@foreach` com código e
/// marcação nas chaves, `@switch` e `@while`.
const LIST: &str = "@page\n\
                    @model ListaModel\n\
                    @using Web.Pages\n\
                    @if (Model.Visivel()) {\n    \
                    <p>sim</p>\n\
                    } else if (Model.Outro()) {\n    \
                    <p>outro</p>\n\
                    }\n\
                    @foreach (var item in Model.Itens()) {\n    \
                    var dobro = Model.Soma(item);\n    \
                    <p>@dobro</p>\n\
                    }\n\
                    @switch (Model.Tipo()) {\n    \
                    case 1:\n        \
                    <p>um</p>\n        \
                    break;\n\
                    }\n\
                    @while (Model.Mais()) {\n    \
                    <p>de novo</p>\n\
                    }\n";

/// O cabeçalho dos comandos de controle da página é código do método que a
/// desenha: o método do modelo chamado no `@if`, no `else if`, no
/// `@foreach`, no `@switch` e no `@while` é usado pelo `ExecuteAsync`, na
/// linha do cabeçalho; o código escrito entre as chaves do `@foreach`
/// também, na linha dele.
#[test]
fn the_header_of_a_control_of_the_page_is_code_of_the_method_that_draws_it() {
    let map = scanned(&[("Web/Pages/Lista.cshtml.cs", LIST_MODEL), ("Web/Pages/Lista.cshtml", LIST)]);
    let model = "Web/Pages/Lista.cshtml.cs";
    let at = |line: u32| vec![format!("Web/Pages/Lista.cshtml:{line}:ExecuteAsync")];
    assert_eq!(every_use(&map, model, "Visivel"), at(4), "@if");
    assert_eq!(every_use(&map, model, "Outro"), at(6), "else if");
    assert_eq!(every_use(&map, model, "Itens"), at(9), "@foreach");
    assert_eq!(every_use(&map, model, "Soma"), at(10), "the code inside the braces");
    assert_eq!(every_use(&map, model, "Tipo"), at(13), "@switch");
    assert_eq!(every_use(&map, model, "Mais"), at(18), "@while");
}

/// A página cujo bloco de corpo mistura código e marcação: o código dele
/// segue código, e o código em linha escrito na marcação dele também.
const MIXED: &str = "@page\n\
                     @model ListaModel\n\
                     @using Web.Pages\n\
                     @{\n    \
                     var total = Model.Soma(1);\n    \
                     <p>Total: @total, @Model.Tipo()</p>\n    \
                     <div class=\"caixa\">\n        \
                     <span>Formatar(1); Model.Mais();</span>\n    \
                     </div>\n    \
                     @:Linha Formatar(2) @Model.Outro()\n\
                     }\n\
                     @functions {\n    \
                     private string Formatar(int x) => x.ToString();\n\
                     }\n";

/// A marcação escrita dentro do bloco `@{ … }` não é código: o texto de
/// dentro do elemento e o da linha de marcação (`@:`) que parece chamada não
/// é uso da função da página nem do método do modelo. O código do bloco e o
/// código em linha escrito na marcação dele seguem usos pelo `ExecuteAsync`.
#[test]
fn the_markup_inside_a_body_block_is_not_read_as_code() {
    let map = scanned(&[("Web/Pages/Lista.cshtml.cs", LIST_MODEL), ("Web/Pages/Mista.cshtml", MIXED)]);
    let (model, page) = ("Web/Pages/Lista.cshtml.cs", "Web/Pages/Mista.cshtml");
    assert_eq!(every_use(&map, page, "Formatar"), Vec::<String>::new(), "the text of the markup calls nothing");
    assert_eq!(every_use(&map, model, "Mais"), Vec::<String>::new(), "the text of the markup calls nothing");
    assert_eq!(every_use(&map, model, "Soma"), ["Web/Pages/Mista.cshtml:5:ExecuteAsync"]);
    assert_eq!(every_use(&map, model, "Tipo"), ["Web/Pages/Mista.cshtml:6:ExecuteAsync"]);
    assert_eq!(every_use(&map, model, "Outro"), ["Web/Pages/Mista.cshtml:10:ExecuteAsync"]);
}

/// O `@inject`, o `@inherits` e o `@namespace` do `_Imports.razor` valem nas
/// páginas da pasta dele e das de baixo, e o `@namespace` ganha o nome de
/// cada subpasta. A página de `Pages/Admin/` herda a base, fica no namespace
/// `Web.Pages.Admin`, que o C# que o importa enxerga, e o `Http` injetado
/// liga a chamada dela à rota do controlador; o `@inherits` da própria
/// página vence o da pasta. A tag `<Contador />` é uso da classe da página
/// `Contador`, no mesmo namespace. A passada que lê só o que mudou, depois
/// de o `@inject` nascer no arquivo da pasta, relê as páginas e liga igual.
#[test]
fn the_inject_inherits_and_namespace_of_the_folder_imports_file_reach_the_pages_below() {
    let temp = tempfile::Builder::new().prefix("scan-pagina-pasta-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    std::fs::write(dir.join(".git").join("info").join("exclude"), mustard_core::footprint_rules().join("\n") + "\n").unwrap();
    let files = [
        ("Api/Api.csproj", csproj("Microsoft.NET.Sdk.Web")),
        ("Api/Controllers/PedidosController.cs", CONTROLLER.to_string()),
        ("Web/Web.csproj", csproj("Microsoft.NET.Sdk.BlazorWebAssembly")),
        ("Web/_Imports.razor", "@using System.Net.Http.Json\n@using Web.Shared\n@namespace Web\n@inherits LayoutBase\n".to_string()),
        ("Web/Shared/LayoutBase.cs", "namespace Web.Shared;\n\npublic abstract class LayoutBase\n{\n    public string Corpo => \"\";\n}\n".to_string()),
        ("Web/Shared/OutraBase.cs", "namespace Web.Shared;\n\npublic abstract class OutraBase\n{\n}\n".to_string()),
        (
            "Web/Pages/Admin/Painel.razor",
            "<h3>Painel</h3>\n<Contador />\n@code {\n    private Task<HttpResponseMessage> Ler() => Http.GetAsync(\"api/pedidos/1\");\n}\n"
                .to_string(),
        ),
        ("Web/Pages/Admin/Contador.razor", "<p>@contagem</p>\n@code {\n    private int contagem;\n}\n".to_string()),
        ("Web/Pages/Propria.razor", "@inherits OutraBase\n<p>@Corpo</p>\n".to_string()),
        ("Web/Rotas.cs", "using Web.Pages.Admin;\n\nnamespace Web;\n\npublic static class Rotas\n{\n    public static Painel? Tela() => null;\n}\n".to_string()),
    ];
    for (rel, body) in &files {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), body).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let (panel, own) = ("Web/Pages/Admin/Painel.razor", "Web/Pages/Propria.razor");
    let (first, _) = model::scan(dir, &dir.join(".claude"), &[]);
    assert_eq!(called_by(&first, "GET", "api/pedidos/{}"), json!([]), "without the inject the page does not know the client");

    std::fs::write(
        dir.join("Web/_Imports.razor"),
        "@using System.Net.Http.Json\n@using Web.Shared\n@namespace Web\n@inherits LayoutBase\n@inject HttpClient Http\n",
    )
    .unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "inject"]);
    let (partial, report) = model::scan(dir, &dir.join(".claude"), &[]);
    assert_eq!(report["full"], json!(false), "{report}");
    let whole_out = tempfile::tempdir().unwrap();
    let (whole, _) = model::scan(dir, whole_out.path(), &[]);
    for (map, pass) in [(&whole, "the whole pass"), (&partial, "the pass that reads only what changed")] {
        assert_eq!(called_by(map, "GET", "api/pedidos/{}"), json!(["Web/Pages/Admin/Painel.razor:4:Ler"]), "{pass}");
        assert_eq!(declaration(map, panel, "Painel")["supertypes"], json!(["LayoutBase"]), "{pass}");
        assert_eq!(declaration(map, own, "Propria")["supertypes"], json!(["OutraBase"]), "{pass}");
        assert_eq!(module(map, panel)["namespaces"], json!(["Web.Pages.Admin"]), "{pass}");
        assert_eq!(every_use(map, panel, "Painel"), ["Web/Rotas.cs:7:Tela"], "{pass}");
        assert_eq!(every_use(map, "Web/Pages/Admin/Contador.razor", "Contador"), ["Web/Pages/Admin/Painel.razor:2:BuildRenderTree"], "{pass}");
    }
}

/// A propriedade lida na página sem chamada é usada pelo método que desenha a
/// página: a do modelo, lida por `@Model.Titulo` na view, e a lida pelo campo
/// da página do Blazor (`@modelo.Titulo`).
#[test]
fn a_property_read_on_the_page_is_a_use_of_it() {
    let map = scanned(&[
        ("Web/Pages/Lista.cshtml.cs", LIST_MODEL),
        ("Web/Pages/Capa.cshtml", "@page\n@model ListaModel\n@using Web.Pages\n<h1>@Model.Titulo</h1>\n"),
        ("Web/Pages/Cartao.razor", "@using Web.Pages\n<p>@modelo.Titulo</p>\n@code {\n    private ListaModel modelo = new();\n}\n"),
    ]);
    assert_eq!(
        every_use(&map, "Web/Pages/Lista.cshtml.cs", "Titulo"),
        ["Web/Pages/Capa.cshtml:4:ExecuteAsync", "Web/Pages/Cartao.razor:2:BuildRenderTree"]
    );
}
