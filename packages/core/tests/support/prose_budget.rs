//! A medida do texto do Mustard que o modelo lê, uma só para todo teste que
//! confere o limite dele: cada um a traz pelo caminho deste arquivo, e a raiz
//! do repositório fica duas pastas acima do pacote que compila o teste.
//!
//! Em cada idioma, todo texto que o modelo lê — os comandos e o estilo de
//! resposta do plugin, os agentes e o mapa do início da sessão que o
//! instalador grava — soma menos de 30.500 bytes. Não há teto por arquivo de agente: o
//! que prende um molde é o que ele diz.
//!
//! A conta é a do disco, byte a byte, como `find … -printf '%s'` a faz. Um
//! arquivo pertence a um idioma quando o caminho dele diz o idioma (uma pasta
//! `pt-BR/` ou um nome terminado em `-pt-BR`); o que não diz idioma nenhum é
//! lido nos dois e conta nas duas somas.

use std::path::{Path, PathBuf};

/// O teto da soma de um idioma, em bytes.
///
/// A conta: o teto era 25.600, e subiu o que os princípios ocupam no idioma em
/// que ocupam mais, o `en-US`. O `principles.md` tinha 4.871 bytes e o lugar
/// dele no molde do mapa, 14; saíram 443 bytes das frases que repetiam um
/// princípio, 181 do mapa, 164 do molde de onda e 98 do de revisão:
/// 25.600 + 4.871 + 14 − 443 = 30.042.
///
/// Ao 30.042 somaram a cobrança nova do primeiro princípio, 93 bytes no
/// `en-US` e 85 no `pt-BR`, a linha dos lugares conferidos no molde da onda,
/// 196 e 192, e o que os moldes de onda e de revisão ganharam depois além da
/// sobra de 209 e 210 bytes que o teto tinha, 43 e 31: a soma medida é
/// 30.042 + 93 + 196 + 43 = 30.374 no `en-US` e 30.042 + 85 + 192 + 31 =
/// 30.350 no `pt-BR`. O teto subiu a 30.400, com folga de 26 bytes no
/// `en-US` e 50 no `pt-BR`.
///
/// Ao 30.400 somou, no objetivo do molde da onda, a regra de ler o código
/// pela ferramenta de leitura e mudá-lo só pela de edição: 134 bytes no
/// `en-US` e 154 no `pt-BR`. Saiu a frase do `Read` sem coordenadas, que ela
/// tornou repetida, 51 e 46 bytes. A soma medida é 30.457 no `en-US` e
/// 30.460 no `pt-BR`. O teto sobe a 30.500, e a folga fica em 43 e 40 bytes.
pub const LANGUAGE_BUDGET: u64 = 30_500;

/// Os dois idiomas do Mustard, como aparecem nos caminhos.
pub const LANGUAGES: [&str; 2] = ["pt-BR", "en-US"];

/// A raiz do repositório na cópia que roda o teste. Quem traz este arquivo
/// declara, na raiz do próprio pacote, o módulo `manifest_dir` dele.
pub fn repo_root() -> PathBuf {
    crate::manifest_dir::manifest_dir().join("../..")
}

/// Todo `.md` debaixo de `dir`, em ordem.
pub fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// O texto que o modelo lê: todo `.md` do plugin, fora a pasta dos binários
/// (o `README.md` dela explica a pasta para quem mantém o projeto e nunca
/// chega a uma janela) e a dos terceiros (o skill-creator só chega ao bloco
/// da tarefa de criar skill, medido pelo teto do bloco), e os moldes que o
/// instalador grava no projeto.
pub fn read_by_the_model() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_md(&root.join("plugin"), &mut files);
    files.retain(|p| !p.starts_with(root.join("plugin/bin")) && !p.starts_with(root.join("plugin/third-party")));
    for dir in ["packages/core/templates/mustard", "packages/core/templates/agents"] {
        collect_md(&root.join(dir), &mut files);
    }
    files
}

/// O idioma que o caminho diz, ou `None` para o texto dos dois.
pub fn language_of(path: &Path) -> Option<&'static str> {
    LANGUAGES.into_iter().find(|lang| {
        path.components().any(|c| c.as_os_str() == *lang)
            || path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.ends_with(&format!("-{lang}")))
    })
}

fn bytes(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display())).len()
}

pub fn shown(path: &Path) -> String {
    path.strip_prefix(repo_root()).unwrap_or(path).display().to_string()
}

/// Em cada idioma, o texto que o modelo lê soma menos de 30.500 bytes; a
/// falha lista cada arquivo somado, com o tamanho dele.
pub fn assert_each_language_under_budget() {
    let files = read_by_the_model();
    assert!(files.len() >= 8, "the walk found almost nothing to measure: {files:?}");

    for lang in LANGUAGES {
        let read: Vec<&PathBuf> =
            files.iter().filter(|p| language_of(p).is_none_or(|own| own == lang)).collect();
        let own = read.iter().filter(|p| language_of(p) == Some(lang)).count();
        assert!(own >= 4, "{lang} has only {own} texts of its own: the map, the style and two agents");
        let total: u64 = read.iter().map(|p| bytes(p)).sum();
        assert!(
            total < LANGUAGE_BUDGET,
            "the {lang} prose adds up to {total} bytes, over the {LANGUAGE_BUDGET} budget:\n{}",
            read.iter().map(|p| format!("{} {}", bytes(p), shown(p))).collect::<Vec<_>>().join("\n"),
        );
    }
}
