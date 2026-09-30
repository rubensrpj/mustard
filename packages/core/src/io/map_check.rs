//! `map_check` — a conferência dos primeiros candidatos da busca.
//!
//! A ordem única dos arquivos ([`crate::io::map_order`]) diz quem vem
//! primeiro, mas não confere se o primeiro traz o que a pergunta pede. Aqui
//! os [`CHECKED`] primeiros candidatos são relidos, cada um, por tudo o que o
//! mapa sabe dele:
//!
//! - todas as colunas do arquivo e das declarações dele no índice: o nome, o
//!   caminho, a assinatura, a documentação, as mensagens de log e de erro, os
//!   textos de que a declaração é dona, os comentários do corpo, os nomes e as
//!   chamadas do corpo, o dono e os membros, os títulos dos commits e quem a
//!   usa (os vínculos do grafo);
//! - a marca do glossário: a palavra que uma edição já ligou à declaração;
//! - o tipo da declaração, que conta para a camada pedida (rota, serviço,
//!   tela) junto com o caminho, que já está no índice;
//! - o corpo, lido do disco: as linhas de começo a fim das declarações do
//!   arquivo que mais palavras da pergunta têm no índice, onde as palavras se
//!   contam nos nomes, nos textos literais e nos comentários.
//!
//! Uma palavra é rara quando é escassa no índice das declarações, perto da
//! mais rara da pergunta ([`RARE_FROM`]). O candidato que cobre as palavras
//! raras sobe na ordem, e o que traz duas delas a até [`NEAR`] palavras uma
//! da outra, no mesmo campo, sobe mais (como o `NEAR` do FTS5, pela posição
//! de cada palavra no vocabulário). A conferência diz também que parte das
//! palavras raras o primeiro traz e que parte o segundo traz: cobrir o
//! primeiro e não o segundo é o que crava a resposta.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::domain::normalize::{plain_words, Normalizer};
use crate::domain::project_map::Found;
use crate::domain::ranking::{idf_x1024, SCALE};
use crate::domain::triage::Lead;
use crate::io::map_glossary::{self, Learned};
use crate::io::map_search::text;
use crate::io::map_words::Word;
use crate::io::project_map::model_path;
use crate::platform::error::Result;

/// Quantos candidatos a conferência relê.
pub(super) const CHECKED: usize = 20;

/// Quantas declarações de cada arquivo têm o corpo lido do disco.
const BODY_DECLS: usize = 3;

/// O máximo de linhas lidas do corpo de uma declaração.
const BODY_LINES: usize = 400;

/// A distância, em palavras, até a qual duas palavras raras juntas no mesmo
/// campo contam como próximas.
const NEAR: i64 = 5;

/// A palavra é rara quando a raridade dela chega a esta parte da raridade da
/// mais rara da pergunta.
const RARE_FROM: f64 = 0.5;

/// A raridade mínima de uma palavra que o índice acha: a palavra que está em
/// todos os documentos tem raridade zero pela conta, e sem este piso o mapa
/// de um documento só não teria palavra rara e nenhum candidato cobriria
/// nada. O piso é bem menor que a raridade de qualquer palavra que separa
/// documentos, então não muda quem é rara onde há o que separar.
const RARITY_FLOOR: f64 = 0.01;

/// O peso da cobertura na ordem, contra a posição recíproca de antes
/// (`1/(60+posição)`, que vai de 0,0164 a 0,0125 nos vinte). É pequeno de
/// propósito: a cobertura desempata os vizinhos e só passa um candidato de
/// mais longe quando ele cobre bem mais que os da frente. Peso maior tirou
/// da frente o arquivo certo que a ordem já tinha posto em primeiro.
const COVERAGE_WEIGHT: f64 = 0.004;

/// O que a proximidade soma à cobertura, que vai de 0 a 1. A proximidade só
/// desempata: com mais que isto ela passa por cima da cobertura e a medida
/// piora.
const NEAR_SHARE: f64 = 0.05;

/// O que a conferência devolve.
pub(super) struct Verdict {
    /// Os candidatos: os [`CHECKED`] primeiros reordenados pela cobertura, o
    /// resto como veio.
    pub files: Vec<Found>,
    /// A parte das palavras raras que o primeiro e o segundo trazem.
    pub lead: Lead,
}

/// Uma declaração de um candidato: o número, o tipo e as linhas.
struct Decl {
    id: i64,
    kind: String,
    line: usize,
    end_line: usize,
}

/// A raiz do projeto de um mapa gravado em `model`, quando ele mora no lugar
/// de [`model_path`].
pub(super) fn root_of(model: &Path) -> Option<&Path> {
    let root = model.parent()?.parent()?;
    (model_path(root) == model).then_some(root)
}

/// Os números que uma medida põe no lugar dos da tabela, só nos testes:
/// `MAP_CHECK_WEIGHTS` (`nome=número`, com `coverage` e `near`, separados por
/// espaço).
fn tuned(name: &str, default: f64) -> f64 {
    #[cfg(test)]
    if let Ok(spec) = std::env::var("MAP_CHECK_WEIGHTS") {
        for pair in spec.split_whitespace() {
            if let Some((key, value)) = pair.split_once('=')
                && key == name
            {
                return value.parse().unwrap();
            }
        }
    }
    let _ = name;
    default
}

/// Confere os primeiros candidatos de `files` contra as palavras da pergunta.
/// Com `root`, lê do disco o corpo das declarações; sem ele, confere só pelo
/// mapa.
pub(super) fn check(
    conn: &Connection,
    root: Option<&Path>,
    normalizer: &mut Normalizer,
    asked: &[Word],
    files: Vec<Found>,
) -> Result<Verdict> {
    // Duas palavras que dividem uma forma contam essa forma uma vez só: a
    // declaração que só tem a forma repetida não passa à frente da que tem a
    // outra palavra por causa da conta em dobro.
    let mut taken: HashSet<&str> = HashSet::new();
    let words: Vec<Word> = asked
        .iter()
        .filter(|word| {
            let fresh = word.indexed.iter().all(|form| !taken.contains(form.as_str()));
            taken.extend(word.indexed.iter().map(String::as_str));
            fresh
        })
        .cloned()
        .collect();
    let words = words.as_slice();
    let checked = files.len().min(CHECKED);
    if checked == 0 || words.is_empty() {
        return Ok(Verdict { files, lead: Lead::default() });
    }
    let mut owner: HashMap<i64, usize> = HashMap::new();
    let mut file_owner: HashMap<i64, usize> = HashMap::new();
    let mut decls: Vec<Vec<Decl>> = Vec::with_capacity(checked);
    let mut by_path = conn.prepare("SELECT rowid FROM files WHERE path = ?1")?;
    let mut by_file = conn.prepare("SELECT rowid, kind, line, end_line FROM decls WHERE file = ?1")?;
    for (at, found) in files.iter().take(checked).enumerate() {
        if let Ok(id) = by_path.query_row([&found.path], |row| row.get::<_, i64>(0)) {
            file_owner.insert(id, at);
        }
        let mut list = Vec::new();
        let mut rows = by_file.query([&found.path])?;
        while let Some(row) = rows.next()? {
            let number = |at: usize| -> Result<usize> { Ok(row.get::<_, Option<i64>>(at)?.unwrap_or(0).max(0) as usize) };
            let id = row.get::<_, i64>(0)?;
            list.push(Decl { id, kind: text(row, 1)?, line: number(2)?, end_line: number(3)? });
            owner.insert(id, at);
        }
        decls.push(list);
    }

    let decl_docs: i64 = conn.query_row("SELECT count(*) FROM decl_lengths", [], |row| row.get(0))?;
    let file_docs: i64 = conn.query_row("SELECT count(*) FROM file_lengths", [], |row| row.get(0))?;
    let idf = |seen: usize, docs: i64| idf_x1024(seen, usize::try_from(docs).unwrap_or(0)) as f64 / SCALE as f64;
    // Do caminho de cada candidato, as palavras que ele traz.
    let mut on_path = vec![vec![false; words.len()]; checked];
    // De cada declaração dos candidatos, as palavras que ela traz.
    let mut held: HashMap<i64, Vec<bool>> = owner.keys().map(|&id| (id, vec![false; words.len()])).collect();
    let mut rarity = vec![0.0_f64; words.len()];
    // De cada declaração e campo dos candidatos, a posição de cada palavra.
    let mut spots: HashMap<(i64, String), Vec<(usize, i64)>> = HashMap::new();
    let mut in_decls = conn.prepare("SELECT doc, col, offset FROM decl_vocab WHERE term = ?1")?;
    let mut in_files = conn.prepare("SELECT doc, col FROM file_vocab WHERE term = ?1")?;
    for (at, word) in words.iter().enumerate() {
        let (mut seen_decls, mut seen_files): (HashSet<i64>, HashSet<i64>) = (HashSet::new(), HashSet::new());
        for form in &word.indexed {
            let mut rows = in_decls.query([form])?;
            while let Some(row) = rows.next()? {
                let (doc, offset) = (row.get::<_, i64>(0)?, row.get::<_, i64>(2)?);
                seen_decls.insert(doc);
                if let Some(list) = held.get_mut(&doc) {
                    list[at] = true;
                    spots.entry((doc, text(row, 1)?)).or_default().push((at, offset));
                }
            }
            let mut rows = in_files.query([form])?;
            while let Some(row) = rows.next()? {
                let doc = row.get::<_, i64>(0)?;
                seen_files.insert(doc);
                if let Some(&candidate) = file_owner.get(&doc)
                    && text(row, 1)? == "path"
                {
                    on_path[candidate][at] = true;
                }
            }
        }
        rarity[at] = if !seen_decls.is_empty() {
            idf(seen_decls.len(), decl_docs).max(RARITY_FLOOR)
        } else if !seen_files.is_empty() {
            idf(seen_files.len(), file_docs).max(RARITY_FLOOR)
        } else {
            0.0
        };
    }

    let forms: Vec<Vec<String>> = words.iter().map(|word| word.forms.clone()).collect();
    for (at, docs) in map_glossary::marked(conn, Learned::Decls, &forms)?.iter().enumerate() {
        for doc in docs {
            if let Some(list) = held.get_mut(doc) {
                list[at] = true;
            }
        }
    }
    for list in &decls {
        for decl in list {
            let kind = normalizer.word_forms(&decl.kind.to_lowercase());
            if let Some(mine) = held.get_mut(&decl.id) {
                for (at, word) in words.iter().enumerate() {
                    if word.forms.iter().any(|form| kind.contains(form)) {
                        mine[at] = true;
                    }
                }
            }
        }
    }
    if let Some(root) = root {
        for (candidate, found) in files.iter().take(checked).enumerate() {
            read_bodies(root, &found.path, &decls[candidate], words, normalizer, &mut held);
        }
    }
    // De cada candidato, o caminho e a declaração que mais traz das palavras:
    // o arquivo grande não cobre mais só por ter mais declarações.
    let mut covered = on_path;
    for (candidate, list) in decls.iter().enumerate() {
        let weight = |mine: &[bool]| -> f64 {
            (0..words.len()).filter(|&at| mine[at] || covered[candidate][at]).map(|at| rarity[at]).sum()
        };
        let best = list.iter().filter_map(|decl| held.get(&decl.id)).max_by(|a, b| weight(a).total_cmp(&weight(b)));
        if let Some(best) = best {
            let best = best.clone();
            for (at, mine) in best.iter().enumerate() {
                covered[candidate][at] |= *mine;
            }
        }
    }
    let top = rarity.iter().copied().fold(0.0_f64, f64::max);
    let rare: Vec<usize> = (0..words.len()).filter(|&at| rarity[at] > 0.0 && rarity[at] >= RARE_FROM * top).collect();
    let mut near = vec![false; checked];
    for ((doc, _), list) in &spots {
        let Some(&candidate) = owner.get(doc) else { continue };
        near[candidate] |= list.iter().enumerate().any(|(at, (word, offset))| {
            rare.contains(word)
                && list[at + 1..]
                    .iter()
                    .any(|(other, spot)| other != word && rare.contains(other) && (offset - spot).abs() <= NEAR)
        });
    }
    let total: f64 = rarity.iter().sum();
    let (coverage, near_share) = (tuned("coverage", COVERAGE_WEIGHT), tuned("near", NEAR_SHARE));
    let share = |candidate: usize| -> f64 {
        if total <= 0.0 {
            return 0.0;
        }
        let held: f64 = (0..words.len()).filter(|&at| covered[candidate][at]).map(|at| rarity[at]).sum();
        held / total + if near[candidate] { near_share } else { 0.0 }
    };
    let mut order: Vec<(usize, f64)> =
        (0..checked).map(|at| (at, 1.0 / (60.0 + at as f64 + 1.0) + coverage * share(at))).collect();
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    let rare_total: f64 = rare.iter().map(|&at| rarity[at]).sum();
    let held = |candidate: Option<&(usize, f64)>| -> f64 {
        match candidate {
            Some((candidate, _)) if rare_total > 0.0 => {
                rare.iter().filter(|&&at| covered[*candidate][at]).map(|&at| rarity[at]).sum::<f64>() / rare_total
            }
            _ => 0.0,
        }
    };
    let lead = Lead { first: held(order.first()), second: held(order.get(1)) };
    let mut rest = files;
    let tail = rest.split_off(checked);
    let mut head: Vec<Option<Found>> = rest.into_iter().map(Some).collect();
    let mut out: Vec<Found> = order.iter().filter_map(|(candidate, _)| head[*candidate].take()).collect();
    out.extend(tail);
    Ok(Verdict { files: out, lead })
}

/// Lê do disco as linhas de começo a fim das [`BODY_DECLS`] declarações do
/// arquivo `path` com mais palavras da pergunta no índice, e marca em `held`,
/// declaração por declaração, as palavras que o corpo traz: nos nomes, nos
/// textos literais e nos comentários. O arquivo ausente ou que não é texto
/// não muda nada.
fn read_bodies(
    root: &Path,
    path: &str,
    decls: &[Decl],
    words: &[Word],
    normalizer: &mut Normalizer,
    held: &mut HashMap<i64, Vec<bool>>,
) {
    let mut picks: Vec<(usize, &Decl)> = decls
        .iter()
        .filter_map(|decl| held.get(&decl.id).map(|mine| (mine.iter().filter(|word| **word).count(), decl)))
        .filter(|(count, _)| *count > 0)
        .collect();
    if picks.is_empty() {
        return;
    }
    picks.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.line.cmp(&b.1.line)));
    picks.truncate(BODY_DECLS);
    let Ok(source) = std::fs::read_to_string(root.join(path)) else { return };
    let lines: Vec<&str> = source.lines().collect();
    for (_, decl) in picks {
        let start = decl.line.saturating_sub(1);
        let end = decl.end_line.max(decl.line).min(start + BODY_LINES).min(lines.len());
        let Some(mine) = held.get_mut(&decl.id) else { continue };
        if start >= end {
            continue;
        }
        for token in plain_words(&lines[start..end].join("\n")) {
            let forms = normalizer.word_forms(&token);
            for (at, word) in words.iter().enumerate() {
                if !mine[at] && word.forms.iter().any(|form| forms.contains(form)) {
                    mine[at] = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::normalize::Languages;
    use crate::io::map_search::indexed;
    use crate::io::map_words::question;
    use crate::io::project_map::{self as store, SEARCHED};
    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// Um projeto com o mapa destes módulos gravado pela porta, com o índice.
    fn saved(modules: Vec<Value>) -> TempDir {
        saved_map(json!({ "modules": modules }))
    }

    fn saved_map(map: Value) -> TempDir {
        let dir = tempdir().unwrap();
        store::save_at(&model_path(dir.path()), &map, "scan 1", &languages()).unwrap();
        dir
    }

    /// Um módulo de uma declaração de cinco linhas, com a documentação.
    fn module(path: &str, kind: &str, name: &str, doc: &str) -> Value {
        json!({"path": path, "declarations": [
            {"kind": kind, "name": name, "line": 1, "end_line": 5, "signature": format!("fn {name}()"), "doc": doc}]})
    }

    /// Módulos que não dizem nada da pergunta: fazem as palavras dela serem
    /// raras no índice.
    fn fillers() -> Vec<Value> {
        (0..12).map(|n| module(&format!("src/outro{n}.rs"), "function", &format!("fazer{n}"), "algo bem diferente aqui")).collect()
    }

    /// A conferência da pergunta sobre os arquivos na ordem dada.
    fn verdict(dir: &TempDir, query: &str, root: Option<&Path>, order: &[String]) -> Verdict {
        let db = indexed(&model_path(dir.path()), &languages(), &SEARCHED).unwrap();
        let mut normalizer = Normalizer::new(&languages());
        let words = question(db.conn(), &mut normalizer, query).unwrap();
        let files = order
            .iter()
            .enumerate()
            .map(|(at, path)| Found { path: path.clone(), score: 1000 - at as u64, text: None })
            .collect();
        check(db.conn(), root, &mut normalizer, &words, files).unwrap()
    }

    fn order(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| (*path).to_string()).collect()
    }

    fn paths(verdict: &Verdict) -> Vec<&str> {
        verdict.files.iter().map(|file| file.path.as_str()).collect()
    }

    #[test]
    fn the_root_of_a_model_is_found_only_where_the_model_lives_in_the_project() {
        let root = Path::new("/work/project");
        assert_eq!(root_of(&model_path(root)), Some(root));
        assert_eq!(root_of(Path::new("/tmp/anywhere/map.db")), None);
        assert_eq!(root_of(Path::new("map.db")), None);
    }

    /// O arquivo cujas declarações espalham as palavras (uma em cada) não
    /// cobre a pergunta como o de uma declaração só que traz as duas: a conta
    /// é da melhor declaração, não do arquivo inteiro.
    #[test]
    fn the_candidate_whose_best_declaration_holds_more_of_the_rare_words_rises() {
        let mut modules = fillers();
        modules.push(json!({"path": "src/soma.rs", "declarations": [
            {"kind": "function", "name": "gerar", "line": 1, "end_line": 5, "signature": "fn gerar()", "doc": "gera a fatura"},
            {"kind": "function", "name": "marcar", "line": 6, "end_line": 9, "signature": "fn marcar()", "doc": "marca a cobranca vencida"}]}));
        modules.push(module("src/completa.rs", "function", "emitir", "emite a fatura vencida"));
        let dir = saved(modules);
        let got = verdict(&dir, "fatura vencida", None, &order(&["src/soma.rs", "src/completa.rs"]));
        assert_eq!(paths(&got), ["src/completa.rs", "src/soma.rs"], "{:?}", got.files);
        assert!(got.lead.leads(), "the first covers the rare words and the second does not: {:?}", got.lead);
        assert!(got.lead.first >= 0.99 && got.lead.second < 0.7, "{:?}", got.lead);

        let same = verdict(&dir, "fatura vencida", None, &order(&["src/completa.rs", "src/soma.rs"]));
        assert_eq!(paths(&same), ["src/completa.rs", "src/soma.rs"], "the order stands when the first already covers");
    }

    /// Os dois candidatos que cobrem o mesmo ficam na ordem de antes, e a
    /// frente não crava: o segundo também cobre.
    #[test]
    fn candidates_that_cover_the_same_words_keep_their_order_and_do_not_lead() {
        let mut modules = fillers();
        modules.push(module("src/a.rs", "function", "emitir", "emite a fatura vencida"));
        modules.push(module("src/b.rs", "function", "cobrar", "cobra a fatura vencida"));
        let dir = saved(modules);
        let got = verdict(&dir, "fatura vencida", None, &order(&["src/a.rs", "src/b.rs"]));
        assert_eq!(paths(&got), ["src/a.rs", "src/b.rs"]);
        assert!(!got.lead.leads(), "{:?}", got.lead);
        assert!(got.lead.second >= COVERS_HALF, "{:?}", got.lead);
    }

    const COVERS_HALF: f64 = 0.7;

    #[test]
    fn nothing_to_check_leaves_the_files_and_the_lead_as_they_were() {
        let dir = saved(fillers());
        let empty = verdict(&dir, "fatura", None, &[]);
        assert!(empty.files.is_empty());
        assert_eq!(empty.lead, Lead::default());
        let no_words = verdict(&dir, "de a o", None, &order(&["src/outro0.rs", "src/outro1.rs"]));
        assert_eq!(paths(&no_words), ["src/outro0.rs", "src/outro1.rs"]);
        assert_eq!(no_words.lead, Lead::default());
    }

    /// A palavra que só o corpo da declaração traz, lido do disco de começo
    /// a fim, conta: no texto literal, no comentário e no nome. A que fica
    /// depois da última linha da declaração não conta, e o arquivo que não
    /// está no disco não muda nada nem derruba a conferência.
    #[test]
    fn a_word_only_the_body_carries_counts_when_the_lines_are_read_from_the_disk() {
        let mut modules = fillers();
        modules.push(module("src/b.rs", "function", "cobrar", "cobra a fatura"));
        modules.push(module("src/a.rs", "function", "emitir", "emite a fatura"));
        modules.push(module("src/x.rs", "function", "listar", "lista a cobranca vencida"));
        let dir = saved(modules);
        let listed = order(&["src/b.rs", "src/a.rs"]);
        let body = |inside: &str| format!("fn emitir() {{\n{inside}\n\n\n}}\n\n\n{}\n", "// vencida depois do fim");

        for inside in ["    let motivo = \"vencida\";", "    // a fatura esta vencida", "    let vencida = 1;"] {
            std::fs::create_dir_all(dir.path().join("src")).unwrap();
            std::fs::write(dir.path().join("src/a.rs"), body(inside)).unwrap();
            let got = verdict(&dir, "fatura vencida", Some(dir.path()), &listed);
            assert_eq!(paths(&got), ["src/a.rs", "src/b.rs"], "{inside}: {:?}", got.files);
            let without_disk = verdict(&dir, "fatura vencida", None, &listed);
            assert_eq!(paths(&without_disk), ["src/b.rs", "src/a.rs"], "{inside}: without the disk nothing rises");
        }

        // A palavra só depois do fim da declaração (linha 8) não conta.
        std::fs::write(dir.path().join("src/a.rs"), body("    let outra = 1;")).unwrap();
        let after = verdict(&dir, "fatura vencida", Some(dir.path()), &listed);
        assert_eq!(paths(&after), ["src/b.rs", "src/a.rs"], "{:?}", after.files);

        // O arquivo ausente do disco fica como o mapa o deixou.
        std::fs::remove_file(dir.path().join("src/a.rs")).unwrap();
        let gone = verdict(&dir, "fatura vencida", Some(dir.path()), &listed);
        assert_eq!(paths(&gone), ["src/b.rs", "src/a.rs"], "{:?}", gone.files);
    }

    /// A palavra que uma edição já ligou à declaração no glossário conta como
    /// dela, mesmo sem estar no índice nem no corpo.
    #[test]
    fn a_word_the_glossary_tied_to_the_declaration_counts_for_the_candidate() {
        let mut modules = fillers();
        modules.push(module("src/b.rs", "function", "cobrar", "cobra a fatura"));
        modules.push(module("src/a.rs", "function", "emitir", "emite a fatura"));
        modules.push(module("src/x.rs", "function", "listar", "lista a cobranca vencida"));
        let dir = saved(modules);
        let listed = order(&["src/b.rs", "src/a.rs"]);
        let plain = verdict(&dir, "fatura vencida", None, &listed);
        assert_eq!(paths(&plain), ["src/b.rs", "src/a.rs"], "nothing ties the word to the second yet");

        drop(indexed(&model_path(dir.path()), &languages(), &SEARCHED).unwrap());
        let forms = Normalizer::new(&languages()).word_forms("vencida");
        let edit = rusqlite::Connection::open(model_path(dir.path())).unwrap();
        edit.execute(
            "INSERT INTO glossary_marks(forms, file, name) VALUES (?1, 'src/a.rs', 'emitir')",
            [serde_json::to_string(&forms).unwrap()],
        )
        .unwrap();
        drop(edit);
        let taught = verdict(&dir, "fatura vencida", None, &listed);
        assert_eq!(paths(&taught), ["src/a.rs", "src/b.rs"], "{:?}", taught.files);
    }

    /// A camada que a pergunta pede conta pelo caminho e pelo tipo: a rota
    /// e o serviço de mesmo nome se separam pela palavra da camada.
    #[test]
    fn the_layer_the_question_asks_for_counts_through_the_path_and_the_kind() {
        let mut modules = fillers();
        modules.push(module("src/route/order.rs", "function", "create", "creates the order"));
        modules.push(module("src/service/order.rs", "function", "create", "creates the order"));
        let dir = saved(modules);
        let by_path = verdict(&dir, "service order", None, &order(&["src/route/order.rs", "src/service/order.rs"]));
        assert_eq!(paths(&by_path), ["src/service/order.rs", "src/route/order.rs"], "{:?}", by_path.files);

        let mut modules = fillers();
        modules.push(module("src/plain.rs", "function", "handle", "handles the order"));
        modules.push(module("src/web.rs", "controller", "handle", "handles the order"));
        modules.push(module("src/doc.rs", "function", "explain", "the controller of the order"));
        let dir = saved(modules);
        let by_kind = verdict(&dir, "controller order", None, &order(&["src/plain.rs", "src/web.rs"]));
        assert_eq!(paths(&by_kind), ["src/web.rs", "src/plain.rs"], "{:?}", by_kind.files);
    }

    /// O que o histórico e o grafo dizem da declaração conta: o título de um
    /// commit que a mudou e o nome de quem a usa.
    #[test]
    fn the_history_and_the_names_of_who_uses_a_declaration_count_for_the_candidate() {
        let mut modules = fillers();
        modules.push(module("src/plain.rs", "function", "gravar", "grava o pedido"));
        modules.push(json!({"path": "src/used.rs", "declarations": [
            {"kind": "function", "name": "gravar", "line": 1, "end_line": 5, "signature": "fn gravar()", "doc": "grava o pedido",
             "used_by": ["src/x.rs:7:reembolsar"]}]}));
        let dir = saved(modules);
        let listed = order(&["src/plain.rs", "src/used.rs"]);
        let got = verdict(&dir, "pedido reembolsar", None, &listed);
        assert_eq!(paths(&got), ["src/used.rs", "src/plain.rs"], "who uses the declaration: {:?}", got.files);
    }

    /// Duas palavras raras a até cinco palavras uma da outra, no mesmo campo,
    /// fazem a função subir: o log "Contrato suspenso" passa a função que
    /// traz as duas palavras longe uma da outra e a que traz uma só. A
    /// pergunta usa a palavra como o log a escreve: a raiz de "suspender" é
    /// outra que a de "suspenso", e a conferência conta pela raiz.
    #[test]
    fn two_rare_words_within_five_words_in_one_field_raise_the_function() {
        let mut modules = fillers();
        modules.push(module("src/one.rs", "function", "pausar_um", "suspenso o fornecimento"));
        modules.push(module(
            "src/far.rs",
            "function",
            "pausar_longe",
            "suspenso o fornecimento conforme a regra combinada com a area comercial para todo o contrato ativo",
        ));
        modules.push(json!({"path": "src/near.rs",
            "declarations": [{"kind": "function", "name": "registrar", "line": 1, "end_line": 9, "signature": "fn registrar()"}],
            "texts": [{"line": 3, "kind": "log", "value": "Contrato suspenso", "owner": "registrar"}]}));
        let dir = saved(modules);
        // Dezessete arquivos sem nada da pergunta na frente: os três de
        // interesse ficam nas posições 17, 18 e 19, onde a diferença de
        // posição é menor que o que a proximidade soma.
        let mut listed: Vec<String> = (0..17).map(|n| format!("src/vazio{n}.rs")).collect();
        listed.extend(order(&["src/one.rs", "src/far.rs", "src/near.rs"]));
        let got = verdict(&dir, "contrato suspenso", None, &listed);
        let at = |path: &str| got.files.iter().position(|file| file.path == path).unwrap();
        assert!(at("src/near.rs") < at("src/far.rs"), "the near words rise above the far ones: {:?}", paths(&got));
        assert!(at("src/far.rs") < at("src/one.rs"), "one word covers less than two: {:?}", paths(&got));
        assert_eq!(at("src/near.rs"), 0, "{:?}", paths(&got));
    }
}
