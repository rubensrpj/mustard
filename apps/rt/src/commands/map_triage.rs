//! `map_triage` — o que a triagem do mapa põe na resposta da busca: o grau,
//! a busca funda do grau 3 para baixo e, sem nenhum achado, a linha que diz
//! que não achou e dá a próxima busca. A triagem em si mora em
//! `mustard_core::io::map_triage`; aqui só se monta o JSON.

use mustard_core::domain::project_map::Found;
use mustard_core::io::map_triage::{Deeper, Link, Triaged, Via};
use mustard_core::platform::i18n::Locale;
use serde_json::{json, Value};

/// A resposta da busca do banco, sem o filtro: os arquivos que mais casam com
/// a pergunta, o grau e, do grau 3 para baixo, o que a busca funda achou.
/// Sem nenhum achado, a linha do que não achou e da próxima busca.
pub(crate) fn bank_report(query: &str, triaged: &Triaged, lang: Locale) -> Value {
    let mut report = json!({ "ok": true, "question": "search", "query": query, "files": files(&triaged.files) });
    add_to(&mut report, triaged);
    if triaged.grade == 0 {
        report["not_found"] = json!(not_found(query, &triaged.words, lang));
    }
    report
}

/// O grau e a busca funda na resposta `report`, seja ela a do banco ou a das
/// peças do filtro. A busca funda só entra quando achou algo.
pub(crate) fn add_to(report: &mut Value, triaged: &Triaged) {
    report["grade"] = json!(triaged.grade);
    if !triaged.deeper.is_empty() {
        report["deeper"] = json!(triaged.deeper.iter().map(entry).collect::<Vec<_>>());
    }
}

/// Os arquivos achados, com a nota e, quando o texto fixo que casou existe,
/// a linha e a declaração onde ele nasce.
fn files(found: &[Found]) -> Vec<Value> {
    found
        .iter()
        .map(|found| {
            let mut file = json!({ "path": found.path, "score": found.score });
            if let Some(text) = &found.text {
                file["text"] = json!({
                    "line": text.line, "kind": text.kind, "value": text.value, "owner": text.owner
                });
            }
            file
        })
        .collect()
}

/// Uma entrada da busca funda: o arquivo, a declaração quando o achado chega
/// a ela, as palavras que ele responde, por onde vieram e se a ligação é
/// provada ou suspeita.
fn entry(deeper: &Deeper) -> Value {
    let mut found = json!({ "path": deeper.path });
    if let Some(decl) = &deeper.decl {
        found["line"] = json!(decl.line);
        found["end_line"] = json!(decl.end_line);
        found["kind"] = json!(decl.kind);
        found["name"] = json!(decl.name);
    }
    found["words"] = json!(deeper.words);
    found["via"] = json!(deeper.via.iter().map(via).collect::<Vec<_>>());
    found["link"] = json!(match deeper.link {
        Link::Proven => "proven",
        Link::Suspected => "suspected",
    });
    found
}

/// Por onde o achado veio, numa palavra e, quando há, o que o guarda: o
/// caminho do teste ou o título do commit.
fn via(via: &Via) -> String {
    match via {
        Via::Comment => "comment".to_string(),
        Via::Test(path) => format!("test {path}"),
        Via::Commit(title) => format!("commit {title}"),
        Via::Glossary => "glossary".to_string(),
    }
}

/// A linha do que não se achou: as palavras da pergunta, já quebradas, e a
/// próxima busca, exata, no texto dos arquivos. A pergunta que só tinha
/// palavras de ligação entra como veio.
fn not_found(query: &str, words: &[String], lang: Locale) -> String {
    let words: Vec<&str> = if words.is_empty() { vec![query.trim()] } else { words.iter().map(String::as_str).collect() };
    let shown = words.iter().map(|word| format!("\"{word}\"")).collect::<Vec<_>>().join(", ");
    let next = format!("grep -rniE \"{}\" .", words.join("|"));
    mustard_core::translate("map.search.not_found", lang).replace("{words}", &shown).replace("{next}", &next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::map_triage::Located;

    fn deeper(path: &str, decl: Option<(&str, u64)>, via: Vec<Via>, link: Link) -> Deeper {
        Deeper {
            path: path.to_string(),
            decl: decl.map(|(name, line)| Located {
                kind: "function".to_string(),
                name: name.to_string(),
                line,
                end_line: line + 9,
            }),
            words: vec!["boleto".to_string()],
            via,
            link,
            score: 1.0,
        }
    }

    #[test]
    fn a_deep_entry_says_where_it_came_from_and_whether_the_link_is_proven() {
        let both = deeper(
            "src/pay.rs",
            Some(("reissue", 12)),
            vec![Via::Comment, Via::Test("tests/pay.rs".to_string()), Via::Commit("Boleto novo".to_string()), Via::Glossary],
            Link::Proven,
        );
        assert_eq!(
            entry(&both),
            json!({"path": "src/pay.rs", "line": 12, "end_line": 21, "kind": "function", "name": "reissue",
                   "words": ["boleto"], "via": ["comment", "test tests/pay.rs", "commit Boleto novo", "glossary"],
                   "link": "proven"})
        );
        let whole = deeper("src/pay.rs", None, vec![Via::Commit("Boleto novo".to_string())], Link::Suspected);
        assert_eq!(
            entry(&whole),
            json!({"path": "src/pay.rs", "words": ["boleto"], "via": ["commit Boleto novo"], "link": "suspected"})
        );
    }

    #[test]
    fn the_not_found_line_carries_the_split_words_and_the_next_exact_search() {
        let words = ["boleto".to_string(), "vencido".to_string()];
        assert_eq!(
            not_found("boletoVencido", &words, Locale::PtBr),
            "Não achei \"boleto\", \"vencido\" no mapa. Próxima busca, exata: grep -rniE \"boleto|vencido\" ."
        );
        assert_eq!(
            not_found("boletoVencido", &words, Locale::EnUs),
            "Found nothing for \"boleto\", \"vencido\" in the map. Next search, exact: grep -rniE \"boleto|vencido\" ."
        );
        assert!(not_found("de", &[], Locale::PtBr).contains("grep -rniE \"de\" ."));
    }
}
