//! Agent output is an alternative representation, never native output plus a
//! diagnostic report. Compact only when every record can be restored in order.
use super::Answer;
use crate::domain::code_search::Request;
use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::Path;

pub struct Presentation {
    pub stdout: Vec<u8>,
    pub representation: &'static str,
    pub owner_ranges: usize,
}

/// A resposta ao agente, com a linha do modo barato na frente quando houver.
///
/// A linha vem DEPOIS de a representação ser escolhida, e nunca dentro dela:
/// somada ao corpo antes da escolha, ela pesava na comparação que decide entre
/// a forma agrupada e a nativa, e uma resposta agrupada perdia o agrupamento
/// por causa de um aviso (21/10/2026).
pub fn agent(answer: &Answer, request: &Request, cwd: &Path) -> Presentation {
    let mut shown = chosen(answer, request, cwd);
    let mut head = String::from(cheap_mode_note(answer.report["choice_gate"].as_str()));
    head.push_str(&nothing_found_note(answer, request, &shown));
    if !head.is_empty() {
        let mut stdout = head.into_bytes();
        stdout.extend_from_slice(&shown.stdout);
        shown.stdout = stdout;
    }
    shown
}

/// A linha que diz o que foi procurado e onde, quando nada foi achado.
///
/// Uma resposta vazia não diz se a busca correu e não achou, se o caminho
/// estava errado, ou se o filtro deixou tudo de fora. Em campo, quatro buscas
/// seguidas voltaram com zero bytes, e quem buscou não tinha como saber qual
/// dos três era (21/10/2026).
fn nothing_found_note(answer: &Answer, request: &Request, shown: &Presentation) -> String {
    // Vazio pela saída, quando ela é o texto do programa nativo, e pelo
    // relatório, quando a resposta é o resultado de uma ferramenta: ali o
    // conteúdo vem dentro de um objeto que nunca é vazio.
    let result = &answer.report["result"];
    let empty = shown.stdout.iter().all(u8::is_ascii_whitespace)
        || result["numLines"].as_u64() == Some(0)
        || result["numFiles"].as_u64() == Some(0)
        || result["content"].as_str() == Some("");
    if !empty {
        return String::new();
    }
    let input = &request.input;
    let pattern = ["pattern", "query", "symbol", "file_path"]
        .into_iter()
        .find_map(|key| input[key].as_str())
        .unwrap_or_default();
    let path = ["path", "file_path"].into_iter().find_map(|key| input[key].as_str()).unwrap_or(".");
    if pattern.is_empty() {
        return String::new();
    }
    format!("# nothing found for \"{pattern}\" under {path} with tool {}.\n", request.tool)
}

fn chosen(answer: &Answer, request: &Request, cwd: &Path) -> Presentation {
    if let Some(view)=super::task_view::agent(answer,request) {return view;}
    let native = matches!(request.tool.as_str(), "rg" | "grep" | "git");
    let original = if native {
        answer.stdout.clone()
    } else {
        // Preserve the actual paginated adapter result, not the subprocess's
        // unpaginated stdout. Read retains its path/offset/totalLines contract.
        let mut bytes = answer.report["result"].to_string().into_bytes();
        bytes.push(b'\n');
        bytes
    };
    let fallback = || Presentation {
        stdout: original.clone(),
        representation: if native { "native" } else { "tool-result" },
        owner_ranges: 0,
    };
    if answer.exit_code != 0 || !(native || request.tool == "Grep") {
        return fallback();
    }
    // A filename or count can itself contain ":12:". Those modes are not
    // coordinate records, even when their text happens to fit the grammar.
    if native
        && request.input["args"].as_array().is_some_and(|args| {
            args.iter()
                .filter_map(serde_json::Value::as_str)
                .any(|arg| {
                    matches!(
                        arg.split('=').next().unwrap_or_default(),
                        "--files"
                            | "--files-with-matches"
                            | "--files-without-match"
                            | "--files-without-matches"
                            | "--count"
                            | "--count-matches"
                            | "--json"
                            | "--heading"
                            | "--no-filename"
                            | "--null"
                            | "--null-data"
                            | "--vimgrep"
                            | "-c"
                            | "-l"
                            | "-L"
                            | "-h"
                            | "-0"
                            | "-z"
                    ) || arg.strip_prefix('-').is_some_and(|flags| {
                        !flags.starts_with('-') && flags.contains(['c', 'l', 'L', 'h', '0', 'z'])
                    })
                })
        })
    {
        return fallback();
    }
    if request.tool == "Grep" && answer.report["result"]["mode"] != "content" {
        return fallback();
    }
    let text = if native {
        let Ok(text) = std::str::from_utf8(&answer.stdout) else {
            return fallback();
        };
        text
    } else {
        answer.report["result"]["content"]
            .as_str()
            .unwrap_or_default()
    };
    if text.is_empty() || text.contains(['\r', '\0', '\x1b']) {
        return fallback();
    }
    let Some(records) = records(text) else {
        return fallback();
    };
    let mut page = selection_note(answer, request);
    let has_selection_note = !page.is_empty();
    if !native {
        let _ = writeln!(page,
            "# page offset {}; more {}",
            request.input["offset"].as_u64().unwrap_or(0),
            answer.report["result"]["truncated"]
                .as_bool()
                .unwrap_or(false)
        );
    }
    let (enriched, count) = grouped(answer, cwd, &records, &page, text.ends_with('\n'), true);
    // An explicit Choice request asks for a recommendation in addition to
    // matches. Ordinary searches never grow just to carry scan metadata.
    let explicit_selection = request.choose && (has_selection_note
        || answer.report["evidence"]["recommended_symbols"]
            .as_array()
            .is_some_and(|ids| !ids.is_empty()));
    if count > 0 && (enriched.len() <= original.len() || explicit_selection) {
        return Presentation {
            stdout: enriched.into_bytes(),
            representation: "grouped-current-owners",
            owner_ranges: count,
        };
    }
    let (compact, _) = grouped(answer, cwd, &records, &page, text.ends_with('\n'), false);
    if compact.len() < original.len() {
        return Presentation {
            stdout: compact.into_bytes(),
            representation: "grouped-matches",
            owner_ranges: 0,
        };
    }
    fallback()
}

/// A linha que diz quando a busca correu no modo barato, e o que traria mais.
///
/// O propósito `locate` desliga os vetores e o filtro do modelo pago, de
/// propósito: é o modo que só acha ocorrências. Mas nada dizia isso a quem
/// buscou. Em campo, 47 buscas seguidas correram assim, e o condutor concluiu
/// que os vetores estavam quebrados (21/10/2026). O motivo já ia no registro,
/// em `choice_gate`; o que faltava era ele aparecer na resposta.
fn cheap_mode_note(gate: Option<&str>) -> &'static str {
    match gate {
        Some("locate") => "# locate mode: no vectors, no paid filter. Use purpose=understand/spec/implement for current-source investigation.\n",
        Some("ai-fallback-disabled") => "# enrichment off by ai.fallback in the project configuration.\n",
        Some("purpose-not-configured") => "# paid filter not configured for search in judgement.search.filter.\n",
        Some("provider-unavailable") => "# paid filter configured but unavailable now; native result only.\n",
        _ => "",
    }
}

fn selection_note(answer: &Answer, request: &Request) -> String {
    if !request.choose { return String::new(); }
    let outcomes: BTreeSet<_> = answer.report["evidence"]["selection"]["outcomes"].as_object().into_iter()
        .flat_map(|values|values.values()).filter_map(serde_json::Value::as_str)
        .filter(|value|matches!(*value,"no-match"|"insufficient-evidence"|"below-acceptance"|"invalid-answer")).collect();
    let mut note=String::new();
    for outcome in outcomes {
        let _ = writeln!(note,"# selection: {outcome} (model judgement; verify source)");
    }
    // The choice the Jev did not make in time stays with the agent.
    if let Some(left) = answer.report["evidence"]["selection"]["choice_left"].as_str() {
        let _ = writeln!(note, "# {left}");
    }
    if note.is_empty() && answer.report["evidence"]["remaining_ambiguities"].as_u64().is_some_and(|n|n>0) {
        note.push_str("# selection: unresolved; inspect source\n");
    }
    note
}

struct Record<'a> {
    file: &'a str,
    line: u64,
    // Keep original digits/text intact, including duplicate matches and spaces.
    rest: &'a str,
}

fn records(text: &str) -> Option<Vec<Record<'_>>> {
    text.lines()
        .map(|row| {
            row.match_indices(':').find_map(|(at, _)| {
                let file = &row[..at];
                let rest = &row[at + 1..];
                let (number, _) = rest.split_once(':')?;
                if file.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let line = number.parse::<u64>().ok().filter(|line| *line > 0)?;
                Some(Record { file, line, rest })
            })
        })
        .collect()
}

fn grouped(
    answer: &Answer,
    cwd: &Path,
    records: &[Record<'_>],
    page: &str,
    trailing_newline: bool,
    owners: bool,
) -> (String, usize) {
    let mut result = page.to_string();
    let mut previous = None;
    let mut seen = BTreeSet::new();
    let mut count = 0;
    for record in records {
        if previous != Some(record.file) {
            result.push_str("@ ");
            result.push_str(record.file);
            result.push('\n');
            if owners && seen.insert(record.file) {
                let ranges: Vec<_> = answer.report["evidence"]["symbols"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|card| {
                        card["source"]["file"] == record.file
                            || card["read"]["input"]["file_path"]
                                .as_str()
                                .is_some_and(|file| cwd.join(record.file) == Path::new(file))
                    })
                    .filter(|card| {
                        card["matched_lines"].as_array().is_some_and(|lines| {
                            records.iter().any(|row| {
                                row.file == record.file && lines.iter().any(|n| n == row.line)
                            })
                        })
                    })
                    .filter_map(|card| {
                        let name = card["name"].as_str()?;
                        let start = card["source"]["line"].as_u64()?;
                        let end = card["source"]["end_line"].as_u64()?;
                        if name.contains(['\n', '\r']) || start == 0 || end < start {
                            return None;
                        }
                        let recommended = answer.report["evidence"]["recommended_symbols"]
                            .as_array()
                            .is_some_and(|ids| ids.contains(&card["id"]));
                        Some(format!(
                            "{name} {start}-{end}{}",
                            if recommended { " [recommended]" } else { "" }
                        ))
                    })
                    .collect();
                if !ranges.is_empty() {
                    result.push_str("# static owners: ");
                    result.push_str(&ranges.join("; "));
                    result.push('\n');
                    count += ranges.len();
                }
            }
            previous = Some(record.file);
        }
        result.push_str(record.rest);
        result.push('\n');
    }
    if !trailing_newline {
        result.pop();
    }
    (result, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Uma resposta sem nada dentro, para o teste pôr só o que ele mede.
    fn empty_answer() -> Answer {
        Answer { report: json!({}), stdout: Vec::new(), stderr: Vec::new(), exit_code: 0 }
    }

    /// A resposta diz quando a busca correu no modo barato, e o que traria
    /// mais. O motivo já ia no registro; o que faltava era aparecer para quem
    /// buscou. Em campo, 47 buscas seguidas correram em `locate`, e o
    /// condutor concluiu que os vetores estavam quebrados (21/10/2026).
    #[test]
    fn the_answer_says_when_the_cheap_mode_left_the_enrichment_out() {
        let note = |gate: &str| cheap_mode_note(Some(gate));
        assert!(note("locate").contains("locate mode"), "o modo barato é dito");
        assert!(note("locate").contains("purpose=understand"), "e diz o que traria mais");
        assert!(note("ai-fallback-disabled").contains("ai.fallback"), "a configuração que desligou é nomeada");
        assert!(note("purpose-not-configured").contains("judgement.search.filter"));
        assert!(note("provider-unavailable").contains("unavailable"));
        assert!(note("eligible").is_empty(), "a busca enriquecida não ganha linha");
        assert!(note("exact-read").is_empty(), "nem a leitura exata");
        assert!(cheap_mode_note(None).is_empty(), "sem portão gravado, nenhuma linha");
    }

    /// A resposta vazia diz o que foi procurado, onde e com qual ferramenta.
    ///
    /// Sem isso, quem buscou não sabe se a busca correu e não achou, se o
    /// caminho estava errado ou se o filtro deixou tudo de fora. Em campo,
    /// quatro buscas seguidas voltaram com zero bytes (21/10/2026).
    #[test]
    fn an_empty_answer_says_what_was_looked_for_and_where() {
        let shown = |stdout: &str| Presentation {
            stdout: stdout.as_bytes().to_vec(),
            representation: "native",
            owner_ranges: 0,
        };
        let asked = |pattern: &str, path: Option<&str>| {
            let mut input = json!({"pattern": pattern, "output_mode": "content"});
            if let Some(path) = path {
                input["path"] = json!(path);
            }
            let body = json!({"tool": "Grep", "input": input, "intent": "procurar", "purpose": "locate"});
            Request::from_json(&body.to_string()).expect("um pedido de busca")
        };
        let mut answer = empty_answer();
        answer.report["result"] = json!({"mode": "content", "content": "", "numLines": 0});
        let note = nothing_found_note(&answer, &asked("frete", Some("src")), &shown(""));
        assert!(note.contains("\"frete\""), "o que foi procurado: {note}");
        assert!(note.contains("under src"), "e onde: {note}");
        assert!(note.contains("tool Grep"), "e com qual ferramenta: {note}");

        // Sem caminho pedido, a busca correu na pasta atual.
        let here = nothing_found_note(&answer, &asked("frete", None), &shown(""));
        assert!(here.contains("under ."), "{here}");

        // Achou alguma coisa: nenhuma linha.
        let mut found = empty_answer();
        found.report["result"] = json!({"mode": "content", "content": "a.rs:1:frete", "numLines": 1});
        assert!(nothing_found_note(&found, &asked("frete", None), &shown("a.rs:1:frete\n")).is_empty());
    }
}
