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
