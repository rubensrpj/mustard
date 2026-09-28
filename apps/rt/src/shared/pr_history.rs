//! A leitura dos pull requests da base para a história das funções: depois
//! de cada atualização do mapa, o título, a descrição e os comentários de
//! revisão presos a linhas de cada pull request que a história guardada cita
//! e o mapa ainda não tem, lidos uma vez pela porta de pull request e
//! gravados no bloco dos pull requests do mapa. O commit de squash ou rebase,
//! que não diz o número no título, pergunta o número ao provedor, e o número
//! fica no mesmo bloco, pelo commit.
//!
//! Só leitura: nada é escrito no servidor. A falta do `gh`, de acesso ou de
//! rede, e o provedor sem adaptador, param a passada sem travar nem avisar: a
//! história segue com o título do commit, o número e a spec.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::project_map::{PullComment, PullOfCommit, PullText};
use mustard_core::io::project_map::{self as store, PullSources};
use mustard_core::ProjectConfig;

use crate::shared::pr_provider::{is_not_found, provider_for, PrProvider, PrText, PrTextRead};

/// Quantas chamadas ao provedor cada atualização do mapa gasta, no máximo,
/// quando o projeto não escreve outro número em `map.pullRequestCalls`. A
/// volta que o provedor responde "nada mudou" não conta.
/// Medido no Mustard, no GitHub pelo `gh`: cada chamada leva de 0,4 a 0,85 s;
/// quatro seguram a atualização em até uns 3 s e leem dois pull requests
/// novos por vez, o que um commit novo da base costuma trazer.
pub(crate) const CALLS_PER_PASS: usize = 4;

/// Lê o que falta dos pull requests da base de `root` pelo provedor do
/// projeto. Sem o remoto `origin`, não há a quem perguntar.
pub(crate) fn refresh(root: &Path) {
    if mustard_core::platform::git::run(root, &["remote", "get-url", "origin"]).out().is_none() {
        return;
    }
    refresh_using(root, &|root| provider_for(root));
}

/// O núcleo testável de [`refresh`]: `provider_of` dá o provedor, pedido só
/// quando há o que ler e a chave `git.pullRequestText` não o desliga.
pub(crate) fn refresh_using(root: &Path, provider_of: &dyn Fn(&Path) -> Box<dyn PrProvider>) {
    let config = ProjectConfig::load(root);
    if !config.git.pull_request_text() {
        return;
    }
    let model = store::model_path(root);
    let Ok(sources) = store::pull_sources_at(&model) else { return };
    let work = plan(&sources);
    if work.is_empty() {
        return;
    }
    let provider = provider_of(root);
    let mut pass = Pass { root, model: &model, provider: provider.as_ref(), left: config.pull_request_calls().or(CALLS_PER_PASS) };
    let _ = pass.run(&sources, work);
}

/// Um commit da história guardada, na ordem da leitura: o id curto, a data e
/// o número do pull request, quando o título o diz.
struct Cited {
    id: String,
    at: i64,
    pr: Option<u32>,
}

/// Os commits da história guardada, na ordem em que a passada os lê: os das
/// listas por função primeiro, porque são os que a resposta mostra, e depois
/// os da janela da montagem, cada grupo do mais novo ao mais antigo, sem
/// repetir.
fn plan(sources: &PullSources) -> Vec<Cited> {
    let mut lineage: Vec<Cited> = sources.lineage.iter().map(|c| Cited { id: c.id.clone(), at: c.at, pr: c.pr }).collect();
    let mut window: Vec<Cited> = sources.window.iter().map(|c| Cited { id: c.id.clone(), at: c.at, pr: c.pr }).collect();
    lineage.sort_by_key(|cited| std::cmp::Reverse(cited.at));
    window.sort_by_key(|cited| std::cmp::Reverse(cited.at));
    let mut seen = BTreeSet::new();
    lineage.into_iter().chain(window).filter(|cited| seen.insert(cited.id.clone())).collect()
}

/// A passada parou antes do fim: o limite de chamadas acabou, o provedor
/// não respondeu ou o mapa não gravou.
struct Stop;

struct Pass<'a> {
    root: &'a Path,
    model: &'a Path,
    provider: &'a dyn PrProvider,
    /// As chamadas que ainda cabem nesta passada.
    left: usize,
}

impl Pass<'_> {
    /// Lê os commits de `work` na ordem: o número de cada um, perguntado ao
    /// provedor quando nem o título nem o mapa o têm, e o texto de cada
    /// número uma vez, quando o mapa não o tem ou quando um commit mais novo
    /// que o da leitura o cita.
    fn run(&mut self, sources: &PullSources, work: Vec<Cited>) -> Result<(), Stop> {
        let mut asked: BTreeMap<String, u32> = sources.asked.iter().map(|c| (c.id.clone(), c.pr)).collect();
        let stored: BTreeMap<u32, &PullText> = sources.texts.iter().map(|t| (t.number, t)).collect();
        // O commit mais novo que cita cada número já sabido: é até onde a
        // leitura do texto vale.
        let mut newest: BTreeMap<u32, (i64, String)> = BTreeMap::new();
        for cited in &work {
            if let Some(number) = cited.pr.or_else(|| asked.get(&cited.id).copied()).filter(|n| *n > 0) {
                let entry = newest.entry(number).or_insert_with(|| (cited.at, cited.id.clone()));
                if cited.at > entry.0 {
                    *entry = (cited.at, cited.id.clone());
                }
            }
        }
        let mut done = BTreeSet::new();
        for cited in &work {
            let number = match cited.pr.or_else(|| asked.get(&cited.id).copied()) {
                Some(number) => number,
                None => {
                    let found = self.ask(&cited.id)?;
                    asked.insert(cited.id.clone(), found);
                    found
                }
            };
            if number == 0 || !done.insert(number) {
                continue;
            }
            let through = newest.get(&number).map_or(cited.id.as_str(), |(_, id)| id.as_str());
            match stored.get(&number) {
                Some(text) if text.through == through => {}
                Some(text) => self.read(number, &text.etag, through)?,
                None => self.read(number, "", through)?,
            }
        }
        Ok(())
    }

    /// Gasta uma chamada; sem nenhuma, a passada para.
    fn spend(&mut self) -> Result<(), Stop> {
        self.left = self.left.checked_sub(1).ok_or(Stop)?;
        Ok(())
    }

    /// O número do pull request que levou o commit `id` à base, perguntado
    /// ao provedor pelo hash inteiro e gravado; 0 quando nenhum o levou ou
    /// quando o provedor não conhece o commit.
    fn ask(&mut self, id: &str) -> Result<u32, Stop> {
        let full = mustard_core::platform::git::run(self.root, &["rev-parse", "--verify", "--quiet", &format!("{id}^{{commit}}")]).out();
        let Some(sha) = full else { return Ok(0) };
        self.spend()?;
        let found = match self.provider.pr_of_commit(sha.trim()) {
            Ok(found) => found,
            Err(reason) if is_not_found(&reason) => None,
            Err(_) => return Err(Stop),
        };
        let pr = found.and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
        store::save_pull_commits_at(self.model, &[PullOfCommit { id: id.to_string(), pr }]).map_err(|_| Stop)?;
        Ok(pr)
    }

    /// Lê o texto e os comentários do pull request `number` e os grava, com
    /// o commit `through` até onde a leitura vale; a leitura começa só com
    /// as duas chamadas no limite. Com a marca `etag` de uma leitura
    /// anterior, o provedor que diz "nada mudou" não gasta chamada, e só o
    /// commit muda. O número que o provedor não acha fica gravado sem
    /// título, para não ser perguntado de novo.
    fn read(&mut self, number: u32, etag: &str, through: &str) -> Result<(), Stop> {
        if self.left < 2 {
            return Err(Stop);
        }
        let text = match self.provider.text(u64::from(number), etag) {
            Ok(PrTextRead::Unchanged) => return store::keep_pull_at(self.model, number, through).map_err(|_| Stop),
            Ok(PrTextRead::Read(text)) => text,
            Err(reason) if is_not_found(&reason) => PrText::default(),
            Err(_) => return Err(Stop),
        };
        self.spend()?;
        let comments = if text.title.is_empty() {
            Vec::new()
        } else {
            self.spend()?;
            self.provider.line_comments(u64::from(number)).map_err(|_| Stop)?
        };
        let saved = PullText { number, title: text.title, body: text.body, etag: text.etag, through: through.to_string() };
        let comments: Vec<PullComment> = comments
            .into_iter()
            .map(|c| PullComment { number, commit: c.commit, path: c.path, line: c.line, body: c.body })
            .collect();
        store::save_pull_at(self.model, &saved, &comments).map_err(|_| Stop)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use mustard_core::domain::project_map::{FileLineage, LineageCommit, Pulls};
    use serde_json::json;

    use super::*;
    use crate::shared::pr_provider::{PrChecks, PrLineComment, PrOpened, PrRef, PrToOpen, PrView};

    /// O que o provedor falso responde: o texto de cada número (sem ele, o
    /// provedor não acha o número), os comentários de cada um e o pull
    /// request de cada commit pelo hash inteiro; com `down`, toda leitura
    /// falha como a falta de acesso.
    #[derive(Default)]
    struct Answers {
        texts: BTreeMap<u64, PrTextRead>,
        comments: BTreeMap<u64, Vec<PrLineComment>>,
        merged: BTreeMap<String, u64>,
        down: bool,
    }

    /// O provedor falso, que anota cada leitura pedida.
    struct FakePr {
        answers: Rc<RefCell<Answers>>,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl FakePr {
        fn answer<T>(&self, call: String, found: impl FnOnce(&Answers) -> Result<T, String>) -> Result<T, String> {
            self.calls.borrow_mut().push(call);
            let answers = self.answers.borrow();
            if answers.down {
                return Err("gh: Bad credentials (HTTP 401)".to_string());
            }
            found(&answers)
        }
    }

    impl PrProvider for FakePr {
        fn provider(&self) -> &'static str {
            "falso"
        }
        fn open(&self, _pr: &PrToOpen) -> Result<PrOpened, String> {
            Err("escrita".to_string())
        }
        fn edit_body(&self, _number: u64, _body: &str) -> Result<(), String> {
            Err("escrita".to_string())
        }
        fn ready(&self, _number: u64) -> Result<(), String> {
            Err("escrita".to_string())
        }
        fn view(&self, _which: PrRef<'_>) -> Result<PrView, String> {
            Err("fora".to_string())
        }
        fn checks(&self, _number: u64) -> Result<PrChecks, String> {
            Err("fora".to_string())
        }
        fn branch_protection(&self, _branch: &str) -> Result<bool, String> {
            Err("fora".to_string())
        }
        fn text(&self, number: u64, known: &str) -> Result<PrTextRead, String> {
            self.answer(format!("text {number} {known}").trim_end().to_string(), |a| {
                a.texts.get(&number).cloned().ok_or_else(|| "gh: Not Found (HTTP 404)".to_string())
            })
        }
        fn line_comments(&self, number: u64) -> Result<Vec<PrLineComment>, String> {
            self.answer(format!("comments {number}"), |a| Ok(a.comments.get(&number).cloned().unwrap_or_default()))
        }
        fn pr_of_commit(&self, sha: &str) -> Result<Option<u64>, String> {
            self.answer(format!("commit {sha}"), |a| Ok(a.merged.get(sha).copied()))
        }
    }

    /// Um projeto com o mapa gravado, o provedor falso e as leituras que ele
    /// recebeu.
    struct Project {
        dir: tempfile::TempDir,
        answers: Rc<RefCell<Answers>>,
        calls: Rc<RefCell<Vec<String>>>,
        built: Cell<usize>,
    }

    impl Project {
        /// O projeto com `config` no `mustard.json` e, na história guardada
        /// da base, os commits `window`: id, data, título e número.
        fn new(config: &str, window: &[(&str, i64, &str, Option<u32>)]) -> Self {
            Self::inside(tempfile::tempdir().unwrap(), config, window)
        }

        /// O mesmo projeto, na pasta `dir`, que já pode ter um repositório.
        fn inside(dir: tempfile::TempDir, config: &str, window: &[(&str, i64, &str, Option<u32>)]) -> Self {
            std::fs::write(dir.path().join("mustard.json"), config).unwrap();
            let commits: Vec<_> =
                window.iter().map(|(id, at, title, pr)| json!({ "id": id, "at": at, "title": title, "pr": pr })).collect();
            let map = json!({ "history": { "base": "main", "paths": [], "commits": commits } });
            store::write_text(dir.path(), &map.to_string()).unwrap();
            Project { dir, answers: Rc::default(), calls: Rc::default(), built: Cell::new(0) }
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        /// Uma atualização do mapa: a passada com o provedor falso.
        fn pass(&self) {
            refresh_using(self.root(), &|_| {
                self.built.set(self.built.get() + 1);
                Box::new(FakePr { answers: Rc::clone(&self.answers), calls: Rc::clone(&self.calls) })
            });
        }

        /// As leituras pedidas desde a última vez.
        fn calls(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.borrow_mut())
        }

        fn pulls(&self) -> Pulls {
            store::read(self.root()).unwrap().pulls
        }
    }

    fn text(title: &str, body: &str, etag: &str) -> PrTextRead {
        PrTextRead::Read(PrText { title: title.to_string(), body: body.to_string(), etag: etag.to_string() })
    }

    const ON: &str = r#"{"git": {"flow": {"*": "main"}}}"#;

    /// O pull request mesclado da base é lido uma vez, com os comentários
    /// presos a linhas, e fica no mapa até onde o commit mais novo que o
    /// cita; a atualização seguinte, sem commit novo, não pergunta nada.
    #[test]
    fn a_merged_pull_request_is_read_once_with_its_line_comments() {
        let project = Project::new(ON, &[("aaaa", 200, "feat: grava o pagamento (#7)", Some(7))]);
        project.answers.borrow_mut().texts.insert(7, text("Grava o pagamento", "Primeiro.\n\nSegundo.", "W/\"e1\""));
        project.answers.borrow_mut().comments.insert(
            7,
            vec![PrLineComment { commit: "c0ffee".into(), path: "src/a.rs".into(), line: 3, body: "cuidado".into() }],
        );
        project.pass();
        assert_eq!(project.calls(), ["text 7", "comments 7"]);
        let pulls = project.pulls();
        let saved: Vec<(u32, &str, &str, &str)> =
            pulls.texts.iter().map(|t| (t.number, t.title.as_str(), t.etag.as_str(), t.through.as_str())).collect();
        assert_eq!(saved, [(7, "Grava o pagamento", "W/\"e1\"", "aaaa")]);
        let comments: Vec<(u32, &str, &str, u64)> =
            pulls.comments.iter().map(|c| (c.number, c.commit.as_str(), c.path.as_str(), c.line)).collect();
        assert_eq!(comments, [(7, "c0ffee", "src/a.rs", 3)]);

        project.pass();
        assert!(project.calls().is_empty(), "um pull request já lido não é lido de novo sem mudança");
    }

    /// Um commit mais novo que cita um pull request já lido faz a volta a
    /// ele com a marca de versão; a resposta "nada mudou" não gasta o limite
    /// e só muda até onde a leitura vale.
    #[test]
    fn a_newer_commit_citing_a_read_pull_request_asks_with_the_mark_and_spends_nothing() {
        let project = Project::new(r#"{"map": {"pullRequestCalls": 2}}"#, &[("aaaa", 200, "grava (#7)", Some(7))]);
        project.answers.borrow_mut().texts.insert(7, text("Grava", "", "W/\"e1\""));
        project.pass();
        assert_eq!(project.calls(), ["text 7", "comments 7"]);

        let newer = |id: &str, at: i64, pr: u32| LineageCommit { id: id.to_string(), at, title: format!("x (#{pr})"), pr: Some(pr) };
        let lineage = FileLineage { path: "src/a.rs".into(), commits: vec![newer("bbbb", 300, 7), newer("cccc", 250, 8)], ..FileLineage::default() };
        store::save_lineage_at(&store::model_path(project.root()), &lineage).unwrap();
        project.answers.borrow_mut().texts.insert(7, PrTextRead::Unchanged);
        project.answers.borrow_mut().texts.insert(8, text("Outro", "", ""));
        project.pass();
        assert_eq!(project.calls(), ["text 7 W/\"e1\"", "text 8", "comments 8"], "a volta sem mudança não gastou uma das duas chamadas");
        let pulls = project.pulls();
        let through: Vec<(u32, &str, &str)> = pulls.texts.iter().map(|t| (t.number, t.title.as_str(), t.through.as_str())).collect();
        assert_eq!(through, [(7, "Grava", "bbbb"), (8, "Outro", "cccc")]);
    }

    /// Com `git.pullRequestText` desligada, nada chama o provedor.
    #[test]
    fn the_key_off_calls_no_provider() {
        let project = Project::new(r#"{"git": {"pullRequestText": false}}"#, &[("aaaa", 200, "grava (#7)", Some(7))]);
        project.answers.borrow_mut().texts.insert(7, text("Grava", "", ""));
        project.pass();
        assert_eq!(project.built.get(), 0, "o provedor nem se monta");
        assert!(project.calls().is_empty());
        assert!(project.pulls().texts.is_empty());

        let on = Project::new(ON, &[("aaaa", 200, "grava (#7)", Some(7))]);
        on.answers.borrow_mut().texts.insert(7, text("Grava", "", ""));
        on.pass();
        assert_eq!(on.built.get(), 1, "sem a chave, a leitura fica ligada");
    }

    /// Sem acesso, a passada para na primeira leitura, sem travar e sem
    /// gravar nada, e a seguinte tenta de novo.
    #[test]
    fn a_provider_without_access_stops_the_pass_and_saves_nothing() {
        let project = Project::new(ON, &[("aaaa", 200, "grava (#7)", Some(7)), ("bbbb", 100, "lê (#6)", Some(6))]);
        project.answers.borrow_mut().down = true;
        project.pass();
        assert_eq!(project.calls(), ["text 7"]);
        assert_eq!(project.pulls(), Pulls::default());
        project.answers.borrow_mut().down = false;
        project.answers.borrow_mut().texts.insert(7, text("Grava", "", ""));
        project.answers.borrow_mut().texts.insert(6, text("Lê", "", ""));
        project.pass();
        assert_eq!(project.calls(), ["text 7", "comments 7", "text 6", "comments 6"]);
    }

    /// O número que o provedor não acha — o de outro repositório no título
    /// — fica gravado sem título e não é perguntado de novo, e a passada
    /// segue para o próximo.
    #[test]
    fn a_number_the_provider_does_not_know_is_kept_without_a_title() {
        let project = Project::new(ON, &[("aaaa", 200, "traz (#5)", Some(5)), ("bbbb", 100, "grava (#7)", Some(7))]);
        project.answers.borrow_mut().texts.insert(7, text("Grava", "", ""));
        project.pass();
        assert_eq!(project.calls(), ["text 5", "text 7", "comments 7"]);
        let pulls = project.pulls();
        let titles: Vec<(u32, &str)> = pulls.texts.iter().map(|t| (t.number, t.title.as_str())).collect();
        assert_eq!(titles, [(5, ""), (7, "Grava")]);
        project.pass();
        assert!(project.calls().is_empty());
    }

    /// Quantas chamadas cada atualização gasta: sem a chave, o padrão, que
    /// lê os dois pull requests novos numa passada; com a chave em 2, um por
    /// passada, do mais novo ao mais velho; com valor inválido, o padrão.
    #[test]
    fn the_calls_per_pass_follow_the_setting() {
        let window = [("aaaa", 200, "grava (#8)", Some(8)), ("bbbb", 100, "lê (#7)", Some(7))];
        for (config, per_pass) in [
            (ON, vec![vec!["text 8", "comments 8", "text 7", "comments 7"]]),
            (r#"{"map": {"pullRequestCalls": 2}}"#, vec![vec!["text 8", "comments 8"], vec!["text 7", "comments 7"]]),
            (r#"{"map": {"pullRequestCalls": 0}}"#, vec![vec!["text 8", "comments 8", "text 7", "comments 7"]]),
            (r#"{"map": {"pullRequestCalls": "dez"}}"#, vec![vec!["text 8", "comments 8", "text 7", "comments 7"]]),
        ] {
            let project = Project::new(config, &window);
            project.answers.borrow_mut().texts.insert(8, text("Grava", "", ""));
            project.answers.borrow_mut().texts.insert(7, text("Lê", "", ""));
            for expected in per_pass {
                project.pass();
                assert_eq!(project.calls(), expected, "{config}");
            }
            project.pass();
            assert!(project.calls().is_empty(), "{config}");
        }
    }

    /// O commit de squash ou rebase, sem número no título, pergunta o
    /// número ao provedor pelo hash inteiro; o número fica no mapa, pelo
    /// commit, e o texto do pull request é lido. O commit que nenhum pull
    /// request levou fica com 0 e não é perguntado de novo.
    #[test]
    fn a_squash_commit_without_a_number_gets_it_from_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "direto na base"]);
        let direct = git(&["rev-parse", "HEAD"]);
        git(&["commit", "-q", "--allow-empty", "-m", "grava o pagamento"]);
        let squash = git(&["rev-parse", "HEAD"]);
        let window = [(&squash[..10], 200, "grava o pagamento", None), (&direct[..10], 100, "direto na base", None)];
        let project = Project::inside(dir, ON, &window);
        project.answers.borrow_mut().merged.insert(squash.clone(), 9);
        project.answers.borrow_mut().texts.insert(9, text("Grava o pagamento", "Por quê.", ""));
        project.pass();
        assert_eq!(project.calls(), [format!("commit {squash}"), "text 9".into(), "comments 9".into(), format!("commit {direct}")]);
        let pulls = project.pulls();
        let asked: Vec<(&str, u32)> = pulls.commits.iter().map(|c| (c.id.as_str(), c.pr)).collect();
        assert_eq!(asked, [(&squash[..10], 9), (&direct[..10], 0)]);
        assert_eq!(pulls.texts.iter().map(|t| t.number).collect::<Vec<_>>(), [9]);
        project.pass();
        assert!(project.calls().is_empty());
    }
}
