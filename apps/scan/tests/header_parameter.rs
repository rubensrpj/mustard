//! O parâmetro escrito no cabeçalho do tipo, o do construtor primário do C#:
//! o scan de verdade o grava como parâmetro, e não como campo, também quando
//! o cabeçalho passa do teto da assinatura; e a lista de candidatos da busca
//! do mapa, lida do mapa que o scan gravou, nunca o oferece, enquanto o campo
//! escrito no corpo do tipo segue candidato.

#[path = "support/model.rs"]
mod model;

use std::path::Path;
use std::process::Command;

use mustard_core::domain::config::ProjectConfig;
use mustard_core::domain::normalize::Languages;
use mustard_core::io::map_search;
use serde_json::Value;

/// A classe `partial` com construtor primário e um campo no corpo.
const PEDIDO: &str = "namespace Loja;\n\npublic partial class Pedido(IRepositorio repositorio)\n{\n    \
                      private readonly int _desconto = 0;\n}\n\npublic interface IRepositorio { }\n";

/// A estrutura com construtor primário.
const PONTO: &str = "namespace Loja;\n\npublic struct Ponto(int largura)\n{\n    public int Largura => largura;\n}\n";

/// Os parâmetros da classe de cabeçalho comprido: com o tipo, cada um passa
/// de 65 caracteres, e o último começa depois do caractere 600 do cabeçalho.
const LONG_PARAMETERS: [&str; 10] = [
    "primeiroServicoDeCobrancaDaLojaVirtual",
    "segundoServicoDeCobrancaDaLojaVirtual",
    "terceiroServicoDeCobrancaDaLojaVirtual",
    "quartoServicoDeCobrancaDaLojaVirtual",
    "quintoServicoDeCobrancaDaLojaVirtual",
    "sextoServicoDeCobrancaDaLojaVirtual",
    "setimoServicoDeCobrancaDaLojaVirtual",
    "oitavoServicoDeCobrancaDaLojaVirtual",
    "nonoServicoDeCobrancaDaLojaVirtual",
    "conciliadorDeRecebiveisAtrasados",
];

/// A classe cujo cabeçalho passa do teto da assinatura, com um campo no
/// corpo que o último parâmetro alimenta.
fn relatorio() -> String {
    let parameters: Vec<String> =
        LONG_PARAMETERS.iter().map(|name| format!("IServicoDeCobrancaDaLojaVirtual {name}")).collect();
    format!(
        "namespace Loja;\n\npublic sealed class Relatorio({})\n{{\n    \
         private readonly IServicoDeCobrancaDaLojaVirtual _conciliadorDeRecebiveis = conciliadorDeRecebiveisAtrasados;\n}}\n\n\
         public interface IServicoDeCobrancaDaLojaVirtual {{ }}\n",
        parameters.join(", ")
    )
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

/// Um projeto no git com os três arquivos, já no primeiro commit, lido pelo
/// scan com o mapa dentro dele. Devolve a pasta e o mapa gravado.
fn scanned() -> (tempfile::TempDir, Value) {
    let temp = tempfile::Builder::new().prefix("scan-header-parameter-").tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q"]);
    let exclude = mustard_core::footprint_rules().join("\n") + "\n";
    std::fs::write(dir.join(".git").join("info").join("exclude"), exclude).unwrap();
    let files = [
        ("Loja/Pedido.cs", PEDIDO.to_string()),
        ("Loja/Ponto.cs", PONTO.to_string()),
        ("Loja/Relatorio.cs", relatorio()),
    ];
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "primeiro"]);
    let (map, _) = model::scan(dir, &dir.join(".claude"), &[]);
    (temp, map)
}

/// A declaração `name` do arquivo `path` no mapa.
fn declaration<'a>(map: &'a Value, path: &str, name: &str) -> &'a Value {
    map["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|module| module["path"] == path)
        .and_then(|module| module["declarations"].as_array())
        .and_then(|decls| decls.iter().find(|decl| decl["name"] == name))
        .unwrap_or_else(|| panic!("{name} de {path} no mapa"))
}

/// Os nomes dos candidatos da busca com filtro para as palavras `query`, no
/// mapa gravado na pasta do projeto `dir`.
fn candidate_names(dir: &Path, query: &str) -> Vec<String> {
    let languages = Languages::of(&ProjectConfig::default());
    let map = model::path_in(&dir.join(".claude"));
    let found = map_search::candidates_at(&map, query, "", &languages, map_search::any_path).expect("a lista de candidatos lê o mapa");
    found.candidates.into_iter().map(|candidate| candidate.name).collect()
}

/// Cada parâmetro do construtor primário, da classe e da estrutura, é
/// parâmetro no mapa, também o que o teto cortou da assinatura da classe; o
/// campo escrito no corpo segue campo.
#[test]
fn a_parameter_written_in_the_type_header_is_a_parameter_and_not_a_field() {
    let (_temp, map) = scanned();
    let header = declaration(&map, "Loja/Relatorio.cs", "Relatorio")["signature"].as_str().unwrap().to_string();
    let last = LONG_PARAMETERS[LONG_PARAMETERS.len() - 1];
    assert!(!header.contains(last), "o teto corta o último parâmetro da assinatura da classe: {header}");
    let places = LONG_PARAMETERS
        .iter()
        .map(|name| ("Loja/Relatorio.cs", *name))
        .chain([("Loja/Pedido.cs", "repositorio"), ("Loja/Ponto.cs", "largura")]);
    let kinds: Vec<(String, String)> = places
        .map(|(path, name)| (name.to_string(), declaration(&map, path, name)["kind"].as_str().unwrap().to_string()))
        .collect();
    let wrong: Vec<&(String, String)> = kinds.iter().filter(|(_, kind)| kind != "parameter").collect();
    assert!(wrong.is_empty(), "todo parâmetro do cabeçalho é parâmetro: {wrong:?}");
    assert_eq!(declaration(&map, "Loja/Pedido.cs", "_desconto")["kind"], "field");
    assert_eq!(declaration(&map, "Loja/Relatorio.cs", "_conciliadorDeRecebiveis")["kind"], "field");
}

/// A lista de candidatos nunca traz o parâmetro do cabeçalho: nem o que a
/// assinatura da classe traz, nem o que o teto cortou dela. O campo do corpo
/// e a classe que o traz na assinatura seguem candidatos.
#[test]
fn the_filter_candidates_never_offer_a_parameter_of_the_type_header() {
    let (temp, _) = scanned();
    let cut = candidate_names(temp.path(), "conciliador recebiveis");
    assert!(cut.contains(&"_conciliadorDeRecebiveis".to_string()), "{cut:?}");
    assert!(!cut.contains(&"conciliadorDeRecebiveisAtrasados".to_string()), "{cut:?}");
    let kept = candidate_names(temp.path(), "repositorio desconto");
    assert!(kept.contains(&"Pedido".to_string()) && kept.contains(&"_desconto".to_string()), "{kept:?}");
    assert!(!kept.contains(&"repositorio".to_string()), "{kept:?}");
    let other = candidate_names(temp.path(), "primeiro servico cobranca");
    assert!(!other.contains(&"primeiroServicoDeCobrancaDaLojaVirtual".to_string()), "{other:?}");
}
