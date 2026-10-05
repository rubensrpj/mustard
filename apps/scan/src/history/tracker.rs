//! A vida de cada linha de código pelo projeto inteiro: em que commit ela foi
//! posta, de que linha anterior ela veio e por quais commits passou até a
//! ponta.
//!
//! Os commits chegam do mais antigo para o mais novo. De cada arquivo o
//! rastreador guarda a lista das linhas dele, cada uma com o resumo do texto e
//! o nó que diz de onde ela vem. Um trecho do diff acha o lugar dele nessa lista
//! pela posição que o git dá e pelo texto das linhas tiradas, e troca as
//! tiradas pelas postas:
//!
//! - a linha posta no lugar de uma tirada vem dela: primeiro as de texto igual
//!   (só a forma mudou), depois as demais, na ordem;
//! - a linha tirada sem substituta, e a posta sem nenhuma tirada, esperam o fim
//!   do commit: a posta de texto igual ao de uma tirada em qualquer arquivo do
//!   mesmo commit é a mesma linha mudada de lugar;
//! - a linha que não diz nada por si só (uma chave, um `else`) não entra na
//!   conta: ela ocupa lugar na lista, mas não tem nó.
//!
//! Do nó da linha na ponta se sobe a cadeia até o commit que a pôs: esses são
//! os commits da linha. A mudança de lugar entre arquivos não conta como
//! mudança, e só se segue por `moves` vezes.

use std::collections::{HashMap, VecDeque};

use super::diff::{CommitPatch, FilePatch, Hunk, Line};

/// O nó que não existe.
pub(super) const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// A linha nasceu neste commit.
    Born,
    /// Este commit trocou a linha anterior por esta.
    Replaced,
    /// Este commit só mudou a forma da linha anterior: os espaços.
    Reformatted,
    /// Este commit levou a linha, igual, de outro lugar.
    Moved,
    /// Uma história já lida antes: `commit` é o conjunto dela.
    Seed,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    commit: u32,
    prev: u32,
    kind: Kind,
}

/// Uma linha de um arquivo: o resumo do texto e o nó dela.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ln {
    pub hash: u64,
    pub node: u32,
}

/// Um commit lido.
pub(super) struct Commit {
    pub sha: String,
    pub at: i64,
    pub title: String,
    /// O projeto manda ignorar o commit na autoria: toda mudança dele é só
    /// forma.
    pub ignored: bool,
}

pub(super) struct Tracker {
    commits: Vec<Commit>,
    nodes: Vec<Node>,
    /// Os commits, com a marca de só forma, de cada história já lida.
    seeds: Vec<Vec<(u32, bool)>>,
    files: HashMap<String, Vec<Ln>>,
    moves: usize,
    ignored: Vec<String>,
    /// As linhas que o commit em andamento tirou sem substituta, por texto.
    pool: HashMap<u64, VecDeque<u32>>,
    /// As linhas que o commit em andamento pôs sem nenhuma tirada.
    loose: Vec<(u32, u64)>,
}

impl Tracker {
    pub(super) fn new(moves: usize, ignored: Vec<String>) -> Self {
        Tracker {
            commits: Vec::new(),
            nodes: Vec::new(),
            seeds: Vec::new(),
            files: HashMap::new(),
            moves,
            ignored,
            pool: HashMap::new(),
            loose: Vec::new(),
        }
    }

    pub(super) fn commit(&self, at: u32) -> &Commit {
        &self.commits[at as usize]
    }

    /// Os commits lidos até aqui, do mais antigo ao mais novo.
    pub(super) fn commits(&self) -> &[Commit] {
        &self.commits
    }

    /// Um commit de uma leitura anterior, que a história guardada cita pelo
    /// começo do hash.
    pub(super) fn seed_commit(&mut self, id: &str, at: i64, title: &str) -> u32 {
        self.commits.push(Commit { sha: id.to_string(), at, title: title.to_string(), ignored: false });
        u32::try_from(self.commits.len() - 1).unwrap_or(NONE)
    }

    /// O nó de uma história já lida: as linhas que o têm vêm dos commits de
    /// `set`, cada um com a marca de só forma.
    pub(super) fn seed_node(&mut self, set: Vec<(u32, bool)>) -> u32 {
        self.seeds.push(set);
        let commit = u32::try_from(self.seeds.len() - 1).unwrap_or(NONE);
        self.push(Node { commit, prev: NONE, kind: Kind::Seed })
    }

    /// As linhas de um arquivo como a leitura anterior o deixou.
    pub(super) fn seed_file(&mut self, path: &str, lines: Vec<Ln>) {
        self.files.insert(path.to_string(), lines);
    }

    pub(super) fn lines(&self, path: &str) -> Option<&[Ln]> {
        self.files.get(path).map(Vec::as_slice)
    }

    fn push(&mut self, node: Node) -> u32 {
        self.nodes.push(node);
        u32::try_from(self.nodes.len() - 1).unwrap_or(NONE)
    }

    /// Os commits pelos quais a linha de nó `node` passou, do mais novo ao mais
    /// antigo, cada um com a marca de só forma, somados a `out`.
    pub(super) fn chain(&self, node: u32, out: &mut Vec<(u32, bool)>) {
        let (mut at, mut hops) = (node, 0);
        while at != NONE {
            let node = self.nodes[at as usize];
            match node.kind {
                Kind::Seed => {
                    out.extend(self.seeds[node.commit as usize].iter().copied());
                    return;
                }
                Kind::Born => {
                    out.push((node.commit, false));
                    return;
                }
                Kind::Replaced => {
                    out.push((node.commit, false));
                    at = node.prev;
                }
                Kind::Reformatted => {
                    out.push((node.commit, true));
                    at = node.prev;
                }
                // Mudar de lugar não muda a linha: segue-se para trás sem
                // contar o commit, até o limite de mudanças; passado o limite,
                // o commit da mudança é onde a linha nasceu.
                Kind::Moved if hops < self.moves => {
                    hops += 1;
                    at = node.prev;
                }
                Kind::Moved => {
                    out.push((node.commit, false));
                    return;
                }
            }
        }
    }

    /// Soma o commit `patch` ao que se sabe dos arquivos e devolve o número
    /// dele na lista dos commits lidos.
    pub(super) fn apply(&mut self, patch: &CommitPatch) -> u32 {
        let ignored = self.ignored.iter().any(|rev| patch.sha.starts_with(rev.as_str()));
        self.commits.push(Commit { sha: patch.sha.clone(), at: patch.at, title: patch.title.clone(), ignored });
        let commit = u32::try_from(self.commits.len() - 1).unwrap_or(NONE);
        for file in &patch.files {
            self.file(commit, file);
        }
        self.settle();
        commit
    }

    fn file(&mut self, commit: u32, patch: &FilePatch) {
        let mut state = match &patch.old {
            Some(old) => self.files.remove(old).unwrap_or_default(),
            None => Vec::new(),
        };
        // Os trechos falam do arquivo de antes do commit: cada um que muda o
        // tamanho, e cada um que se achou fora do lugar, desloca os seguintes.
        let mut shift = 0isize;
        for hunk in &patch.hunks {
            shift += self.edit(commit, &mut state, hunk, shift);
        }
        for (at, line) in &patch.born {
            let node = if line.trivial { NONE } else { self.push(Node { commit, prev: NONE, kind: Kind::Born }) };
            state.insert((*at).min(state.len()), Ln { hash: line.hash, node });
        }
        if let Some(new) = &patch.new {
            self.files.insert(new.clone(), state);
        }
    }

    /// Troca, na lista de `state`, as linhas que o trecho tirou pelas que ele
    /// pôs, com os trechos de antes deslocando o lugar em `shift` linhas.
    /// Devolve quanto os trechos seguintes se deslocam por causa deste.
    fn edit(&mut self, commit: u32, state: &mut Vec<Ln>, hunk: &Hunk, shift: isize) -> isize {
        let taken = hunk.removed.len();
        let hint = if taken == 0 { hunk.old_start } else { hunk.old_start.saturating_sub(1) };
        let hint = usize::try_from(hint as isize + shift).unwrap_or(0);
        let found = locate(state, hint, &hunk.removed);
        let start = found.unwrap_or_else(|| hint.min(state.len()));
        let drift = if found.is_some() { start as isize - hint as isize } else { 0 };
        // Sem achar onde as tiradas estavam, nada se tira: o que ficou é lixo
        // que a lista carrega, e a posição das seguintes se acha pelo texto.
        let before: Vec<u32> = match found {
            Some(_) => state[start..start + taken].iter().map(|line| line.node).collect(),
            None => vec![NONE; taken],
        };

        let real_removed: Vec<usize> = (0..taken).filter(|&k| !hunk.removed[k].trivial).collect();
        let real_added: Vec<usize> = (0..hunk.added.len()).filter(|&j| !hunk.added[j].trivial).collect();
        let mut partner: Vec<Option<usize>> = vec![None; hunk.added.len()];
        let mut used = vec![false; taken];
        let mut by_text: HashMap<u64, VecDeque<usize>> = HashMap::new();
        for &k in &real_removed {
            by_text.entry(hunk.removed[k].hash).or_default().push_back(k);
        }
        for &j in &real_added {
            if let Some(k) = by_text.get_mut(&hunk.added[j].hash).and_then(VecDeque::pop_front) {
                partner[j] = Some(k);
                used[k] = true;
            }
        }
        let mut left = real_removed.iter().copied().filter(|&k| !used[k]).collect::<Vec<_>>().into_iter();
        for &j in &real_added {
            if partner[j].is_some() {
                continue;
            }
            let Some(k) = left.next() else { break };
            partner[j] = Some(k);
            used[k] = true;
        }

        let mut lines = Vec::with_capacity(hunk.added.len());
        for (j, line) in hunk.added.iter().enumerate() {
            let node = if line.trivial {
                NONE
            } else {
                match partner[j].filter(|&k| before[k] != NONE) {
                    Some(k) => {
                        let kind = if hunk.removed[k].squeezed == line.squeezed { Kind::Reformatted } else { Kind::Replaced };
                        self.push(Node { commit, prev: before[k], kind })
                    }
                    None => {
                        let node = self.push(Node { commit, prev: NONE, kind: Kind::Born });
                        self.loose.push((node, line.hash));
                        node
                    }
                }
            };
            lines.push(Ln { hash: line.hash, node });
        }
        for &k in &real_removed {
            if !used[k] && before[k] != NONE {
                self.pool.entry(hunk.removed[k].hash).or_default().push_back(before[k]);
            }
        }
        let end = if found.is_some() { start + taken } else { start };
        let grown = lines.len() as isize - (end - start) as isize;
        state.splice(start..end, lines);
        drift + grown
    }

    /// Fecha o commit: a linha posta sem tirada, de texto igual ao de uma
    /// tirada sem substituta em qualquer arquivo do commit, é essa mesma linha
    /// mudada de lugar.
    fn settle(&mut self) {
        for (node, hash) in std::mem::take(&mut self.loose) {
            if let Some(from) = self.pool.get_mut(&hash).and_then(VecDeque::pop_front) {
                self.nodes[node as usize].prev = from;
                self.nodes[node as usize].kind = Kind::Moved;
            }
        }
        self.pool.clear();
    }
}

/// Onde, em `state`, estão as linhas `removed`: na posição que o git deu, ou
/// na mais próxima dela em que o texto bate. `None` quando não estão em lugar
/// nenhum, porque a lista não é a do arquivo daquele commit (um ramo lido em
/// outra ordem).
fn locate(state: &[Ln], hint: usize, removed: &[Line]) -> Option<usize> {
    if removed.is_empty() {
        return Some(hint.min(state.len()));
    }
    let matches = |at: usize| {
        at + removed.len() <= state.len() && state[at..at + removed.len()].iter().zip(removed).all(|(line, gone)| line.hash == gone.hash)
    };
    if matches(hint) {
        return Some(hint);
    }
    (1..=state.len()).find_map(|distance| {
        hint.checked_sub(distance).filter(|&at| matches(at)).or_else(|| Some(hint + distance).filter(|&at| matches(at)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::diff::summary;

    fn line(text: &str) -> Line {
        summary(text.as_bytes())
    }

    /// Um hunk que troca as linhas `gone`, a partir da linha `at` (a partir de
    /// 1), pelas `put`.
    fn hunk(at: usize, gone: &[&str], put: &[&str]) -> Hunk {
        Hunk { old_start: at, removed: gone.iter().map(|t| line(t)).collect(), added: put.iter().map(|t| line(t)).collect() }
    }

    fn file(path: &str, hunks: Vec<Hunk>) -> FilePatch {
        FilePatch { old: Some(path.into()), new: Some(path.into()), hunks, ..FilePatch::default() }
    }

    fn born(path: &str, text: &[&str]) -> FilePatch {
        FilePatch { old: None, new: Some(path.into()), hunks: vec![hunk(0, &[], text)], ..FilePatch::default() }
    }

    fn commit(title: &str, files: Vec<FilePatch>) -> CommitPatch {
        CommitPatch { sha: format!("{title:0>40}"), at: 0, title: title.into(), files }
    }

    /// Os títulos dos commits da linha `at` de `path`, do mais novo ao mais
    /// antigo, com a marca de só forma.
    fn history(tracker: &Tracker, path: &str, at: usize) -> Vec<(String, bool)> {
        let mut chain = Vec::new();
        tracker.chain(tracker.lines(path).unwrap()[at].node, &mut chain);
        chain.into_iter().map(|(c, form)| (tracker.commit(c).title.clone(), form)).collect()
    }

    fn titled(list: &[(&str, bool)]) -> Vec<(String, bool)> {
        list.iter().map(|(title, form)| ((*title).to_string(), *form)).collect()
    }

    #[test]
    fn a_line_replaced_twice_keeps_every_commit_that_wrote_it() {
        let mut tracker = Tracker::new(5, Vec::new());
        tracker.apply(&commit("cria", vec![born("a.rs", &["fn a() {", "    1", "}"])]));
        tracker.apply(&commit("muda", vec![file("a.rs", vec![hunk(2, &["1"], &["2"])])]));
        tracker.apply(&commit("muda de novo", vec![file("a.rs", vec![hunk(2, &["2"], &["3"])])]));
        assert_eq!(history(&tracker, "a.rs", 1), titled(&[("muda de novo", false), ("muda", false), ("cria", false)]));
        assert_eq!(history(&tracker, "a.rs", 0), titled(&[("cria", false)]), "the line nobody touched is from where it was written");
        assert_eq!(tracker.lines("a.rs").unwrap().len(), 3);
    }

    #[test]
    fn a_change_of_spaces_only_is_a_change_marked_as_form() {
        let mut tracker = Tracker::new(5, Vec::new());
        tracker.apply(&commit("cria", vec![born("a.rs", &["fn a() {", "x + 2", "}"])]));
        tracker.apply(&commit("espaços", vec![file("a.rs", vec![hunk(2, &["x + 2"], &["x  +  2"])])]));
        assert_eq!(history(&tracker, "a.rs", 1), titled(&[("espaços", true), ("cria", false)]));
    }

    #[test]
    fn a_renamed_file_takes_the_lines_and_their_commits_along() {
        let mut tracker = Tracker::new(5, Vec::new());
        tracker.apply(&commit("cria", vec![born("velho.rs", &["fn a() {", "    1", "}"])]));
        tracker.apply(&commit(
            "renomeia",
            vec![FilePatch { old: Some("velho.rs".into()), new: Some("novo.rs".into()), ..FilePatch::default() }],
        ));
        tracker.apply(&commit("muda", vec![file("novo.rs", vec![hunk(2, &["1"], &["2"])])]));
        assert!(tracker.lines("velho.rs").is_none());
        assert_eq!(history(&tracker, "novo.rs", 0), titled(&[("cria", false)]));
        assert_eq!(history(&tracker, "novo.rs", 1), titled(&[("muda", false), ("cria", false)]));
    }

    #[test]
    fn a_line_taken_to_another_file_keeps_its_commits_up_to_the_moves_asked() {
        let steps = |moves: usize| {
            let mut tracker = Tracker::new(moves, Vec::new());
            tracker.apply(&commit("cria", vec![born("origem.rs", &["fn ler() {", "    lido + 1", "}", "fn outra() {", "    2", "}"])]));
            tracker.apply(&commit("muda", vec![file("origem.rs", vec![hunk(2, &["lido + 1"], &["lido + 2"])])]));
            tracker.apply(&commit(
                "move para o meio",
                vec![born("meio.rs", &["fn ler() {", "    lido + 2", "}"]), file("origem.rs", vec![hunk(1, &["fn ler() {", "    lido + 2", "}"], &[])])],
            ));
            tracker.apply(&commit("muda no meio", vec![file("meio.rs", vec![hunk(2, &["lido + 2"], &["lido + 3"])])]));
            tracker.apply(&commit(
                "move para o destino",
                vec![born("destino.rs", &["fn ler() {", "    lido + 3", "}"]), file("meio.rs", vec![hunk(1, &["fn ler() {", "    lido + 3", "}"], &[])])],
            ));
            history(&tracker, "destino.rs", 1)
        };
        assert_eq!(steps(5), titled(&[("muda no meio", false), ("muda", false), ("cria", false)]), "the moves are not changes");
        assert_eq!(steps(1), titled(&[("muda no meio", false), ("move para o meio", false)]), "one move followed: the other is where it was born");
        assert_eq!(steps(0), titled(&[("move para o destino", false)]), "no move followed");
    }

    #[test]
    fn a_line_written_only_in_a_merge_is_from_the_merge() {
        let mut tracker = Tracker::new(5, Vec::new());
        tracker.apply(&commit("cria", vec![born("a.rs", &["fn a() {", "    1", "}"])]));
        let merge = CommitPatch {
            files: vec![FilePatch {
                old: Some("a.rs".into()),
                new: Some("a.rs".into()),
                combined: true,
                born: vec![(3, line("fn resolvida() {")), (4, line("    resolve()"))],
                ..FilePatch::default()
            }],
            ..commit("junta", Vec::new())
        };
        tracker.apply(&merge);
        assert_eq!(tracker.lines("a.rs").unwrap().len(), 5);
        assert_eq!(history(&tracker, "a.rs", 3), titled(&[("junta", false)]));
        assert_eq!(history(&tracker, "a.rs", 0), titled(&[("cria", false)]));
    }

    #[test]
    fn a_line_with_the_text_of_another_elsewhere_does_not_take_its_commit() {
        let mut tracker = Tracker::new(5, Vec::new());
        tracker.apply(&commit("cria a", vec![born("a.rs", &["fn a() {", "    let x = 1;", "    return;", "}"])]));
        tracker.apply(&commit("cria b", vec![file("a.rs", vec![hunk(4, &[], &["fn b() {", "    let y = 2;", "    return;", "}"])])]));
        tracker.apply(&commit("muda b", vec![file("a.rs", vec![hunk(6, &["let y = 2;"], &["let y = 3;"])])]));
        assert_eq!(history(&tracker, "a.rs", 5), titled(&[("muda b", false), ("cria b", false)]), "b's own line, not a's");
        assert_eq!(history(&tracker, "a.rs", 1), titled(&[("cria a", false)]));
        assert!(tracker.lines("a.rs").unwrap()[2].node == NONE, "the `return;` of a says nothing by itself");
        assert!(tracker.lines("a.rs").unwrap()[6].node == NONE, "nor does the one of b");
    }

    #[test]
    fn a_hunk_whose_place_moved_is_found_by_the_text_of_the_lines_it_took() {
        let mut tracker = Tracker::new(5, Vec::new());
        tracker.apply(&commit("cria", vec![born("a.rs", &["um", "dois", "tres", "quatro"])]));
        // O git diz a linha 1, mas as tiradas estão na 3: outro ramo pôs duas
        // linhas antes.
        tracker.apply(&commit("antes", vec![file("a.rs", vec![hunk(0, &[], &["novo1", "novo2"])])]));
        tracker.apply(&commit("muda", vec![file("a.rs", vec![hunk(1, &["um"], &["UM"]), hunk(2, &["dois"], &["DOIS"])])]));
        let lines = tracker.lines("a.rs").unwrap();
        assert_eq!(lines.len(), 6);
        assert_eq!(history(&tracker, "a.rs", 2), titled(&[("muda", false), ("cria", false)]));
        assert_eq!(history(&tracker, "a.rs", 3), titled(&[("muda", false), ("cria", false)]));
        assert_eq!(history(&tracker, "a.rs", 4), titled(&[("cria", false)]));
    }

    #[test]
    fn the_hunks_of_one_commit_speak_of_the_file_before_it_and_a_hunk_that_grew_the_file_moves_the_next() {
        let mut tracker = Tracker::new(5, Vec::new());
        tracker.apply(&commit("cria", vec![born("a.rs", &["p", "x", "q", "x"])]));
        tracker.apply(&commit(
            "muda",
            vec![file("a.rs", vec![hunk(0, &[], &["n1", "n2"]), hunk(4, &["x"], &["X"])])],
        ));
        assert_eq!(history(&tracker, "a.rs", 3), titled(&[("cria", false)]), "the first x was not touched");
        assert_eq!(history(&tracker, "a.rs", 5), titled(&[("muda", false), ("cria", false)]), "the second x, on line 4 before the commit");
    }

    #[test]
    fn a_commit_of_the_ignore_list_is_flagged_and_the_others_are_not() {
        let mut tracker = Tracker::new(5, vec!["000ignora".into()]);
        tracker.apply(&commit("cria", vec![born("a.rs", &["fn a() {", "x + 2", "}"])]));
        let ignored = CommitPatch { sha: "000ignora0000000000000000000000000000000".into(), ..commit("formata", vec![file("a.rs", vec![hunk(2, &["x + 2"], &["(x + 2)"])])]) };
        let at = tracker.apply(&ignored);
        assert!(tracker.commit(at).ignored);
        assert!(!tracker.commit(0).ignored);
    }

    #[test]
    fn a_seeded_history_is_the_start_of_the_chain_of_the_lines_that_have_it() {
        let mut tracker = Tracker::new(5, Vec::new());
        let old = tracker.seed_commit("aaaa", 1, "antigo");
        let node = tracker.seed_node(vec![(old, false)]);
        tracker.seed_file("a.rs", vec![Ln { hash: line("fn a() {").hash, node }, Ln { hash: line("1").hash, node }]);
        tracker.apply(&commit("muda", vec![file("a.rs", vec![hunk(2, &["1"], &["2"])])]));
        assert_eq!(history(&tracker, "a.rs", 1), titled(&[("muda", false), ("antigo", false)]));
        assert_eq!(history(&tracker, "a.rs", 0), titled(&[("antigo", false)]));
    }
}
