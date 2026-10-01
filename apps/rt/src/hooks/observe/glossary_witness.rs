//! `glossary_witness` — a testemunha do glossário do mapa.
//!
//! ## Por quê
//!
//! A busca do mapa acha pelas palavras escritas no código, e a pergunta vem
//! nas palavras de quem pergunta: "sobra" não chega a `leftover`. Quando o
//! Claude edita, logo depois da busca, uma declaração que ela entregou, a
//! edição confirma que a palavra levava àquela declaração. Este observador
//! roda depois de cada Edit, Write e MultiEdit, lê do resultado da
//! ferramenta as linhas que a edição mudou e as passa ao glossário
//! (`io::map_glossary`), que grava a marca. O Read nunca chega aqui: abrir um
//! arquivo não ensina, porque o Claude também abre o arquivo errado.
//!
//! Nunca barra. O erro dele — sem mapa, mapa ilegível, banco travado por
//! outra gravação — some: a espera pela trava é curta, e a edição segue.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::domain::normalize::Languages;
use mustard_core::io::map_glossary::{self, Touched};
use mustard_core::io::project_map::model_path;
use mustard_core::io::spec_events::spec_root;
use serde_json::Value;

/// O observador do glossário.
pub struct GlossaryWitness;

/// Quanto o gancho espera pela trava de outra gravação do mapa antes de
/// desistir: o scan regrava o mapa inteiro de uma vez, e a edição não espera
/// por ele.
const LOCK_WAIT: Duration = Duration::from_millis(150);

impl Observer for GlossaryWitness {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let _ = witness(input, ctx);
    }
}

/// Passa ao glossário a edição da chamada `input`, quando ela tem sessão,
/// linhas mudadas e um arquivo do projeto; `None` quando não há o que passar
/// ou quando o glossário falha.
fn witness(input: &HookInput, ctx: &Ctx) -> Option<()> {
    let session = input.session_id.as_deref().filter(|session| !session.is_empty())?;
    let touched = touched(input.raw.get("tool_response")?);
    if touched.is_empty() {
        return None;
    }
    let file = input.file_path()?;
    let start = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(&ctx.project_dir));
    let root = spec_root(&start);
    let rel = relative_in(&root, Path::new(&file))?;
    let languages = Languages::of(&crate::shared::context::config::project_config_cached(&root));
    map_glossary::confirm_edit(&model_path(&root), session, &rel, &touched, &languages, LOCK_WAIT).ok()?;
    Some(())
}

/// Os trechos que a edição mudou, nas linhas do arquivo de antes dela, lidos
/// dos pedaços do resultado da ferramenta (`structuredPatch`): cada linha
/// tirada é um trecho; as linhas postas entre duas outras, sem nenhuma
/// tirada no lugar, são o trecho dessas duas. As de contexto não mudaram.
fn touched(response: &Value) -> Vec<Touched> {
    let mut out: Vec<Touched> = Vec::new();
    for hunk in response.get("structuredPatch").and_then(Value::as_array).into_iter().flatten() {
        let Some(mut old) = hunk.get("oldStart").and_then(Value::as_u64) else { continue };
        let mut removed = false;
        for line in hunk.get("lines").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            let piece = match line.chars().next() {
                Some('-') => {
                    removed = true;
                    old += 1;
                    Some((old - 1, old - 1))
                }
                Some('+') => (!removed && old > 1).then(|| (old - 1, old)),
                _ => {
                    removed = false;
                    old += 1;
                    None
                }
            };
            if let Some(piece) = piece
                && !out.contains(&piece)
            {
                out.push(piece);
            }
        }
    }
    out
}

/// O caminho de `file` a partir da raiz `root` do projeto, com a barra
/// normal. O arquivo de uma cópia de trabalho do mesmo projeto — um worktree
/// dele, como a cópia de uma onda — conta pelo caminho dentro da cópia.
fn relative_in(root: &Path, file: &Path) -> Option<String> {
    let rel = match file.strip_prefix(root) {
        Ok(rel) => rel,
        Err(_) => {
            let top = file.ancestors().skip(1).find(|dir| dir.join(".git").exists())?;
            if spec_root(top) != root {
                return None;
            }
            file.strip_prefix(top).ok()?
        }
    };
    Some(rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::map::{map_at, MapOpts, Question};
    use crate::dispatch::run_event;
    use mustard_core::domain::map_filter::FilterError;
    use mustard_core::domain::model::contract::{Trigger, Verdict};
    use mustard_core::io::map_db::MapDb;
    use mustard_core::io::{map_search, project_map as store};
    use serde_json::json;
    use std::time::Instant;

    const SESSION: &str = "s-glossario";

    /// O mapa de um projeto de duas funções num arquivo — a que junta a sobra
    /// da onda, de nome `leftover`, e a que fecha a onda — e de uma cobrança
    /// noutro.
    fn map(leftover: &str) -> Value {
        json!({"modules": [
            {"path": "src/rounds.rs", "loc": 20, "declarations": [
                {"kind": "function", "name": leftover, "line": 1, "end_line": 8},
                {"kind": "function", "name": "close_wave", "line": 10, "end_line": 16}
            ]},
            {"path": "src/billing.rs", "loc": 10, "declarations": [
                {"kind": "function", "name": "charge_payment", "line": 1, "end_line": 5}
            ]}
        ]})
    }

    /// Um projeto que escreve em português e programa em inglês, com o mapa
    /// de [`map`].
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"language": {"text": "pt-BR", "code": "en-US"}}"#).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/rounds.rs"), "fn collect_leftover() {}\n").unwrap();
        store::write_text(root, &map("collect_leftover").to_string()).unwrap();
        dir
    }

    /// Os arquivos que a busca do mapa, sem o filtro, acha para `query`,
    /// perguntada pela sessão do teste: os da resposta parcial, ou o da peça
    /// da cravada.
    fn search(root: &Path, query: &str) -> Vec<String> {
        let opts = MapOpts {
            root: root.to_path_buf(),
            question: Question::Search,
            file: None,
            task: None,
            grep: None,
            query: Some(query.to_string()),
            intent: None,
            described: None,
            said: None,
            path: None,
            name: None,
            pr: None,
            session: Some(SESSION.to_string()),
        };
        let report = map_at(
            &opts,
            &|_, _| panic!("a map outside git is never read again"),
            &|_, _, _, _| panic!("a map without a base never reads a history"),
            &|_, _| Err(FilterError::MissingKey),
        );
        assert_eq!(report["ok"], json!(true), "{report}");
        // A resposta cravada traz a peça do primeiro achado; a parcial, os arquivos.
        let found = report.get("files").or_else(|| report.get("pieces")).and_then(|found| found.as_array());
        found.unwrap().iter().map(|file| file["path"].as_str().unwrap().to_string()).collect()
    }

    /// Os nomes dos candidatos da busca com filtro para `query`.
    fn candidates(root: &Path, query: &str) -> Vec<String> {
        let languages = crate::commands::spec_events::project(root).languages;
        let found = map_search::candidates(root, query, "", &languages, 100).unwrap();
        found.candidates.into_iter().map(|candidate| candidate.name).collect()
    }

    /// A chamada que o Claude Code manda depois da ferramenta `tool` no
    /// arquivo `file`, com a linha `line` do arquivo de antes trocada.
    fn after(root: &Path, tool: &str, file: &str, line: u64) -> HookInput {
        let path = root.join(file).to_string_lossy().into_owned();
        HookInput {
            tool_name: Some(tool.to_string()),
            tool_input: json!({ "file_path": path, "old_string": "let a = 1;", "new_string": "let a = 2;" }),
            hook_event_name: Some("PostToolUse".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            session_id: Some(SESSION.to_string()),
            raw: json!({ "tool_response": { "filePath": path, "structuredPatch": [{
                "oldStart": line - 1, "oldLines": 3, "newStart": line - 1, "newLines": 3,
                "lines": ["     let before = 0;", "-    let a = 1;", "+    let a = 2;", "     let after = 0;"]
            }]}}),
            ..HookInput::default()
        }
    }

    /// Roda os ganchos do `PostToolUse` da chamada, como o Claude Code.
    fn hooked(input: &HookInput) -> Verdict {
        let outcome = run_event(Some(Trigger::PostToolUse), input);
        assert!(outcome.warnings.is_empty(), "{outcome:?}");
        outcome.verdict
    }

    /// A busca por uma palavra em português não acha a função de nome em
    /// inglês; depois da busca que entregou o arquivo dela e da edição da
    /// função, a mesma palavra a acha, no arquivo e entre os candidatos, e o
    /// plural dela também, pela mesma normalização.
    #[test]
    fn an_edit_right_after_the_search_teaches_the_word_of_the_question() {
        let dir = project();
        let root = dir.path();
        assert!(search(root, "sobra").is_empty());
        assert!(!candidates(root, "sobra").contains(&"collect_leftover".to_string()));

        assert_eq!(search(root, "sobra leftover"), ["src/rounds.rs"]);
        assert_eq!(hooked(&after(root, "Edit", "src/rounds.rs", 3)), Verdict::Allow);

        assert_eq!(search(root, "sobra"), ["src/rounds.rs"]);
        assert_eq!(search(root, "sobras"), ["src/rounds.rs"]);
        assert!(candidates(root, "sobra").contains(&"collect_leftover".to_string()));
    }

    /// Abrir o arquivo que a busca entregou não ensina: a leitura nem chama o
    /// gancho do glossário, mesmo com o mesmo resultado da edição.
    #[test]
    fn reading_what_the_search_delivered_teaches_nothing() {
        let dir = project();
        let root = dir.path();
        assert_eq!(search(root, "sobra leftover"), ["src/rounds.rs"]);
        assert_eq!(hooked(&after(root, "Read", "src/rounds.rs", 3)), Verdict::Allow);
        assert!(search(root, "sobra").is_empty());
    }

    /// A edição de uma declaração que a busca não entregou não ensina.
    #[test]
    fn an_edit_outside_what_the_search_delivered_teaches_nothing() {
        let dir = project();
        let root = dir.path();
        assert_eq!(search(root, "sobra leftover"), ["src/rounds.rs"]);
        hooked(&after(root, "Edit", "src/billing.rs", 3));
        assert!(search(root, "sobra").is_empty());
        assert!(search(root, "leftover").iter().all(|file| file != "src/billing.rs"));
    }

    /// Depois de uma busca sem resultado, a primeira declaração editada
    /// ganha a marca, e só ela: a segunda edição não ensina mais.
    #[test]
    fn after_a_search_that_found_nothing_the_first_edited_declaration_teaches() {
        let dir = project();
        let root = dir.path();
        assert!(search(root, "sobra").is_empty());
        hooked(&after(root, "Write", "src/rounds.rs", 3));
        hooked(&after(root, "MultiEdit", "src/billing.rs", 3));
        assert_eq!(search(root, "sobra"), ["src/rounds.rs"]);
    }

    /// Renomear a função no scan derruba a marca: a palavra não leva ao nome
    /// novo, nem ao antigo quando ele volta.
    #[test]
    fn renaming_the_function_drops_the_mark() {
        let dir = project();
        let root = dir.path();
        let languages = crate::commands::spec_events::project(root).languages;
        let model = store::model_path(root);
        assert_eq!(search(root, "sobra leftover"), ["src/rounds.rs"]);
        hooked(&after(root, "Edit", "src/rounds.rs", 3));
        assert_eq!(search(root, "sobra"), ["src/rounds.rs"]);

        store::save_at(&model, &map("gather_remainder"), "", &languages).unwrap();
        assert!(search(root, "sobra").is_empty());
        store::save_at(&model, &map("collect_leftover"), "", &languages).unwrap();
        assert!(search(root, "sobra").is_empty());
    }

    /// Com o mapa ilegível, o gancho deixa a edição seguir, sem pânico.
    #[test]
    fn an_unreadable_map_lets_the_edit_go_on() {
        let dir = project();
        let root = dir.path();
        assert_eq!(search(root, "sobra leftover"), ["src/rounds.rs"]);
        std::fs::write(store::model_path(root), "not a database").unwrap();
        assert_eq!(hooked(&after(root, "Edit", "src/rounds.rs", 3)), Verdict::Allow);
    }

    /// Com o mapa travado por outra gravação, o gancho espera pouco e
    /// desiste: a edição segue logo, e nada se ensina.
    #[test]
    fn a_locked_map_lets_the_edit_go_on_without_waiting_for_the_lock() {
        let dir = project();
        let root = dir.path().to_path_buf();
        assert_eq!(search(&root, "sobra leftover"), ["src/rounds.rs"]);
        let (locked_tx, locked) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let model = store::model_path(&root);
        let holder = std::thread::spawn(move || {
            let mut db = MapDb::open(&model, model.parent().unwrap(), &[]).unwrap();
            db.write(|_| {
                locked_tx.send(()).unwrap();
                released.recv().unwrap();
                Ok(())
            })
            .unwrap();
        });
        locked.recv().unwrap();
        let started = Instant::now();
        assert_eq!(hooked(&after(&root, "Edit", "src/rounds.rs", 3)), Verdict::Allow);
        let waited = started.elapsed();
        release.send(()).unwrap();
        holder.join().unwrap();
        assert!(waited < Duration::from_secs(2), "the edit waited {waited:?}");
        assert!(search(&root, "sobra").is_empty());
    }

    /// Os trechos mudados saem das linhas tiradas e, sem nenhuma tirada no
    /// lugar, das linhas vizinhas das postas; as de contexto não contam.
    #[test]
    fn the_touched_lines_come_from_the_removed_and_the_inserted_lines() {
        let response = json!({"structuredPatch": [
            {"oldStart": 4, "lines": [" a", "-b", "+B", " c", "+d", " e"]},
            {"oldStart": 20, "lines": ["+x", " y"]}
        ]});
        assert_eq!(touched(&response), [(5, 5), (6, 7), (19, 20)]);
        assert!(touched(&json!({"filePath": "x"})).is_empty());
    }
}
