//! A mensagem do commit de cada onda, montada num lugar só: a gravação da
//! entrega confere o resumo por aqui, e a rodada comita com o que sai daqui.
//! A mensagem descreve a mudança e nada do Mustard: nem número de onda, nem
//! tamanho, nem rodapé. O commit é achado de novo pelo conteúdo
//! ([`super::lost_commit`]), e nunca pela mensagem.
use super::{
    answer::RoundRefusal,
    commit::{check_commit_text, covered, git, needs_commit},
    report::WaveReport,
};
use mustard_core::domain::spec_events::{MESSAGE_TITLE_MAX, without_forbidden};
use mustard_core::domain::text::whole_words;
use mustard_core::platform::i18n::{Locale, translate};
use std::path::Path;

/// O teto do resumo que cada entrega traz para o commit: o que o texto do
/// agente de onda diz, e o que cabe inteiro no título ao lado de um tipo e de
/// um escopo curto.
pub(super) const SUMMARY_MAX: usize = 45;

/// O resumo de uma entrega cabe no teto. É a parte da mensagem que se
/// atribui a uma onda só: a rodada segura por ela só a onda do resumo
/// recusado. O que a mensagem nunca leva não recusa nada: sai do resumo na
/// montagem ([`message`]), como sai do pull request. A conta é do que fica
/// no título: o prefixo `tipo(escopo): ` que o agente grava na frente sai
/// antes dela ([`without_type_prefix`]).
pub(super) fn check_summary(wave: &WaveReport) -> Result<(), RoundRefusal> {
    let Some(summary) = wave.commit.as_deref() else { return Ok(()) };
    let chars = without_type_prefix(summary).chars().count();
    if chars > SUMMARY_MAX {
        return Err(RoundRefusal::CommitTooLong { part: "summary".to_string(), chars, max: SUMMARY_MAX });
    }
    Ok(())
}

/// A mensagem do commit de uma onda: o título, o corpo e as palavras do
/// resumo que saíram por serem o que um commit nunca leva.
pub(super) struct Message {
    pub(super) title: String,
    pub(super) body: String,
    pub(super) removed: Vec<String>,
}

/// O commit de uma onda, ainda por fazer: a onda, as que ele cobre — ela e
/// as que ela conserta —, os arquivos da entrega e a mensagem.
pub(super) struct WaveCommit {
    pub(super) wave: u64,
    pub(super) covered: Vec<u64>,
    pub(super) files: Vec<String>,
    pub(super) message: Message,
}

/// O commit de cada entrega de `waves` que mudou arquivo, um por onda, na
/// ordem em que as entregas vieram — a da junção.
pub(super) fn wave_commits(root: &Path, waves: &[&WaveReport], lang: Locale) -> Result<Vec<WaveCommit>, RoundRefusal> {
    let mut commits = Vec::new();
    for wave in waves {
        if let Some(message) = message(root, wave, lang)? {
            commits.push(WaveCommit { wave: wave.wave, covered: covered(std::iter::once(*wave)), files: wave.files.clone(), message });
        }
    }
    Ok(commits)
}

/// O resumo neutro de uma entrega, pelos arquivos dela, no idioma `lang`: o
/// nome do arquivo, quando é um só e cabe, ou a conta deles.
fn neutral(files: &[String], lang: Locale) -> String {
    let named = match files {
        [one] => {
            let name = Path::new(one).file_name().map_or_else(|| one.clone(), |name| name.to_string_lossy().to_string());
            Some(translate("round.commit_neutral_one", lang).replace("{file}", &name))
        }
        _ => None,
    };
    named
        .filter(|summary| summary.chars().count() <= SUMMARY_MAX && without_forbidden(summary).1.is_empty())
        .unwrap_or_else(|| translate("round.commit_neutral_many", lang).replace("{count}", &files.len().to_string()))
}

/// As pastas que só dizem onde o código mora, e não de que parte dele se
/// trata: o escopo as deixa de fora, em qualquer posição do caminho.
const GENERIC_DIRS: &[&str] = &["src", "lib", "libs", "app", "apps", "packages", "crates", "modules", "pkg", "test", "tests", "spec"];

/// O escopo que o commit pode levar, tirado dos arquivos dele: a última pasta
/// que todos eles têm em comum, sem as pastas que só dizem onde o código mora
/// ([`GENERIC_DIRS`]) em qualquer posição do caminho. Nenhum quando os
/// arquivos se espalham ou quando a pasta comum é escondida — a configuração
/// do editor não é parte do módulo; fica de fora o escopo longo demais ou cujo
/// nome traz o que um commit nunca leva.
///
/// O escopo saía do histórico dos arquivos: o primeiro título deles com
/// `tipo(escopo):` servia de molde. Numa correção de verdade os três commits
/// saíram com o mesmo escopo, e um deles só devolvia a configuração do editor,
/// que não tem nada a ver com o módulo. O histórico diz onde aqueles arquivos
/// já estiveram; os arquivos deste commit dizem onde ele mexe. O caminho todo
/// até a pasta só alongava o título e cortava o resumo: fica a última pasta.
fn scope_of(files: &[String]) -> Option<String> {
    fn dirs(file: &str) -> Vec<&str> {
        let mut parts: Vec<&str> = file.split('/').collect();
        parts.pop();
        parts
    }
    let first = files.first()?;
    let mut common = dirs(first);
    for file in &files[1..] {
        let other = dirs(file);
        let shared = common.iter().zip(&other).take_while(|(a, b)| a == b).count();
        common.truncate(shared);
    }
    if common.iter().any(|dir| dir.starts_with('.') || dir.is_empty()) {
        return None;
    }
    let last = common.iter().rev().find(|dir| !GENERIC_DIRS.contains(dir))?;
    Some((*last).to_string()).filter(|scope| scope.chars().count() <= 32 && without_forbidden(scope).1.is_empty())
}

/// Os tipos de commit: os que o ramo dá ao título — `feat` é também o da
/// rodada — e os que saem da frente do resumo ([`without_type_prefix`]).
const COMMIT_TYPES: &[&str] = &["feat", "fix", "refactor", "docs", "test", "perf", "build", "ci", "chore", "style", "revert"];

/// O resumo `summary` sem o prefixo `tipo(escopo): ` ou `tipo: ` — com ou sem
/// o espaço depois dos dois pontos, e sem o `!` de quebra —, repetido ou não:
/// o tipo e o escopo do título são da rodada, e o que o agente grava na frente
/// do resumo dobraria os dois. Só sai o prefixo de um tipo de commit
/// ([`COMMIT_TYPES`]): os dois pontos de uma frase comum (`Cache: limpa
/// chaves`) ficam.
fn without_type_prefix(summary: &str) -> &str {
    fn strip(text: &str) -> Option<&str> {
        let end = text.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(text.len());
        if !COMMIT_TYPES.iter().any(|kind| kind.eq_ignore_ascii_case(&text[..end])) {
            return None;
        }
        let mut rest = &text[end..];
        if let Some(scoped) = rest.strip_prefix('(') {
            rest = &scoped[scoped.find(')')? + 1..];
        }
        rest.strip_prefix('!').unwrap_or(rest).strip_prefix(':')
    }
    let mut rest = summary.trim();
    while let Some(after) = strip(rest) {
        rest = after.trim_start();
    }
    rest
}

/// A mensagem do commit da entrega `wave`, já conferida; `None` quando ela
/// não mudou arquivo ou não trouxe resumo. O título é `tipo(escopo): frase`
/// com o resumo inteiro da entrega: o escopo é o dos arquivos
/// ([`scope_of`]) quando cabe ao lado do resumo todo, e o título sai sem escopo
/// quando ele não cabe — o resumo inteiro vale mais que o escopo. O corte numa
/// palavra inteira, com o resumo inteiro no corpo, só serve ao resumo que não
/// cabe nem ao lado do tipo sozinho. O tipo é `fix` quando a entrega conserta
/// outra onda, o do ramo quando ele diz um ([`COMMIT_TYPES`]), e `feat` nos
/// outros casos.
///
/// Do resumo sai, palavra a palavra, o que um commit nunca leva
/// ([`without_forbidden`]), e as palavras que saíram vão em
/// [`Message::removed`]: o commit segue, e a resposta da rodada diz o que
/// saiu. O resumo que fica vazio vira um resumo neutro pelos arquivos, no
/// idioma `lang`. O prefixo `tipo(escopo): ` que o agente deixa na frente do
/// resumo sai antes ([`without_type_prefix`]): o tipo e o escopo são da
/// rodada, e o título leva um prefixo só.
pub(super) fn message(root: &Path, wave: &WaveReport, lang: Locale) -> Result<Option<Message>, RoundRefusal> {
    let Some(said) = wave.commit.as_deref().filter(|_| needs_commit(&wave.files)) else {
        return Ok(None);
    };
    check_summary(wave)?;
    let (kept, removed) = without_forbidden(without_type_prefix(said));
    let summary = if kept.is_empty() { neutral(&wave.files, lang) } else { kept };
    let branch = git(root, &["branch", "--show-current"]).unwrap_or_default();
    let category = branch.split('/').next().unwrap_or_default().trim();
    let kind = if !wave.fixes.is_empty() {
        "fix"
    } else if COMMIT_TYPES.contains(&category) {
        category
    } else {
        "feat"
    };
    let whole = summary.chars().count();
    let prefix = scope_of(&wave.files)
        .map(|scope| format!("{kind}({scope}): "))
        .filter(|prefix| prefix.chars().count() + whole <= MESSAGE_TITLE_MAX)
        .unwrap_or_else(|| format!("{kind}: "));
    let cut = whole_words(&summary, MESSAGE_TITLE_MAX.saturating_sub(prefix.chars().count()));
    let title = format!("{prefix}{}", if cut.is_empty() { summary.as_str() } else { cut });
    let body = if title.ends_with(&summary) { String::new() } else { summary };
    check_commit_text(&title, &body)?;
    Ok(Some(Message { title, body, removed }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O escopo do commit é a última pasta que os arquivos dele têm em comum,
    /// sem as pastas que só dizem onde o código mora, em qualquer posição. Os
    /// arquivos que se espalham, a pasta escondida e o arquivo solto na raiz
    /// não dão escopo.
    ///
    /// Em campo (21/10/2026) os três commits de uma correção saíram com o
    /// mesmo escopo, tirado do histórico dos arquivos, e um deles só devolvia
    /// a configuração do editor.
    #[test]
    fn the_commit_scope_is_the_last_folder_its_files_share() {
        let files = |list: &[&str]| list.iter().map(|file| (*file).to_string()).collect::<Vec<_>>();

        // Duas partes do mesmo módulo: o escopo é o módulo.
        assert_eq!(scope_of(&files(&["src/order/order.service.ts", "src/order/order.controller.ts"])).as_deref(), Some("order"));
        // A pasta de dentro é o escopo, sem o caminho que leva a ela.
        assert_eq!(scope_of(&files(&["src/order/dto/a.ts", "src/order/dto/b.ts"])).as_deref(), Some("dto"));
        assert_eq!(scope_of(&files(&["packages/core/src/io/fs/mod.rs"])).as_deref(), Some("fs"));
        assert_eq!(scope_of(&files(&["apps/rt/src/commands/flow/round/commit.rs"])).as_deref(), Some("round"));
        // A pasta de código no fim do caminho sai: o escopo é a de antes dela.
        assert_eq!(scope_of(&files(&["packages/core/src/a.rs"])).as_deref(), Some("core"));

        // Dois módulos diferentes: o que sobra é a pasta de código, e ela não
        // é escopo nenhum.
        assert_eq!(scope_of(&files(&["src/order/a.ts", "src/user/b.ts"])), None);
        // A configuração do editor não é parte do módulo.
        assert_eq!(scope_of(&files(&[".vscode/settings.json"])), None);
        // O arquivo solto na raiz não tem pasta.
        assert_eq!(scope_of(&files(&["README.md"])), None);
        assert_eq!(scope_of(&files(&[])), None);
        // A pasta de código sozinha também não.
        assert_eq!(scope_of(&files(&["src/a.ts", "src/b.ts"])), None);
        // O nome longo demais não é escopo.
        assert_eq!(scope_of(&files(&["src/uma_camada_de_armazenamento_longa_demais/a.rs"])), None);
    }

    /// A entrega de uma onda com os arquivos `files` e o resumo `summary`.
    fn delivery(files: &[&str], summary: &str) -> WaveReport {
        WaveReport {
            wave: 1,
            delivered: "A onda saiu.".into(),
            files: files.iter().map(|file| (*file).to_string()).collect(),
            commit: Some(summary.into()),
            proofs: Vec::new(),
            fixes: Vec::new(),
            replan: None,
            undone: Vec::new(),
            taken_elsewhere: false,
            leftovers: Vec::new(),
            agreed: Vec::new(),
            returns: Vec::new(),
            usage: Default::default(),
        }
    }

    /// O tipo e o escopo do título são da rodada: o resumo que o agente grava
    /// já com `tipo(escopo): ` ou `tipo: ` na frente perde esse prefixo, e o
    /// título sai com um só. O que parece prefixo mas não é tipo de commit
    /// fica, e o resumo que era só o prefixo cai no resumo neutro.
    ///
    /// Em campo (23/10/2026) o título saiu `fix: fix(rt): Remoção de tarefa
    /// com dependentes`.
    #[test]
    fn the_title_carries_one_type_prefix_even_when_the_summary_brings_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("src/rt/a.rs", "fn one() {}\n"), ("src/b.rs", "fn one() {}\n")]);
        let title = |files: &[&str], summary: &str| {
            let made = message(root, &delivery(files, summary), Locale::PtBr).unwrap_or_else(|_| panic!("{summary}")).expect("a message");
            assert_eq!(made.body, "", "{summary}: the whole summary is in the title");
            made.title
        };

        // Com escopo comum aos arquivos: o escopo é o da rodada.
        assert_eq!(title(&["src/rt/a.rs"], "fix(rt): Frase"), "feat(rt): Frase");
        assert_eq!(title(&["src/rt/a.rs"], "feat: Frase"), "feat(rt): Frase");
        // Sem escopo comum: o tipo sozinho, também um só.
        assert_eq!(title(&["src/b.rs"], "fix(rt): Frase"), "feat: Frase");
        assert_eq!(title(&["src/b.rs"], "feat: Frase"), "feat: Frase");
        // O marcador de quebra e o prefixo repetido saem junto.
        assert_eq!(title(&["src/b.rs"], "feat!: Frase"), "feat: Frase");
        assert_eq!(title(&["src/b.rs"], "fix: fix(rt): Frase"), "feat: Frase");
        // O prefixo colado na frase, sem espaço depois dos dois pontos, também.
        assert_eq!(title(&["src/b.rs"], "fix:Frase"), "feat: Frase");
        assert_eq!(title(&["src/rt/a.rs"], "fix(rt):Frase"), "feat(rt): Frase");
        // Sem prefixo, o resumo segue como veio.
        assert_eq!(title(&["src/b.rs"], "Frase"), "feat: Frase");
        // Dois pontos de uma frase comum não são prefixo de tipo.
        assert_eq!(title(&["src/b.rs"], "Cache: limpa chaves"), "feat: Cache: limpa chaves");
        assert_eq!(title(&["src/b.rs"], "Fixes: limpa chaves"), "feat: Fixes: limpa chaves");
        // O resumo que era só o prefixo vira o resumo neutro dos arquivos.
        assert_eq!(title(&["src/b.rs"], "fix(rt):"), format!("feat: {}", neutral(&["src/b.rs".to_string()], Locale::PtBr)));
    }

    /// O título leva o resumo inteiro: o escopo, a última pasta comum dos
    /// arquivos, só entra quando cabe ao lado dele, e o título sai sem escopo
    /// quando não cabe. As pastas que só dizem onde o código mora saem do
    /// escopo em qualquer posição.
    ///
    /// Em campo (23/10/2026) o título saiu `fix(rt/src/commands/flow/round):
    /// Título do commit sem o`: o `src` do meio ficou no escopo, e o escopo
    /// cortou o resumo.
    #[test]
    fn the_title_keeps_the_whole_summary_and_drops_the_scope_that_would_cut_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("src/a.rs", "fn one() {}\n")]);
        let made = |files: &[&str], summary: &str| {
            let made = message(root, &delivery(files, summary), Locale::PtBr).unwrap_or_else(|_| panic!("{summary}")).expect("a message");
            assert_eq!(made.body, "", "{summary}: the whole summary is in the title");
            made.title
        };
        let round = ["apps/rt/src/commands/flow/round/commit_title.rs", "apps/rt/src/commands/flow/round/commit.rs"];
        let summary = "Resumo medido sem o tipo e o escopo da frente";
        assert_eq!(summary.chars().count(), SUMMARY_MAX);

        // O escopo é só a última pasta, não o caminho todo até ela.
        assert_eq!(made(&round, "Frase curta"), "feat(round): Frase curta");
        // O resumo do teto ainda cabe ao lado dele.
        assert_eq!(made(&round, summary), format!("feat(round): {summary}"));
        // O escopo não cabe: o título sai sem escopo, com o resumo inteiro.
        let wide = ["src/camada_de_armazenamento/a.rs"];
        assert_eq!(made(&wide, summary), format!("feat: {summary}"));
    }

    /// O teto do resumo vale para o que fica dele no título: o prefixo
    /// `tipo(escopo): ` que o agente grava na frente sai antes da conta.
    #[test]
    fn the_summary_limit_is_measured_without_the_type_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("src/a.rs", "fn one() {}\n")]);
        let fits = "Resumo que cabe todo sem o prefixo antes";
        let over = format!("{fits} ainda");
        assert_eq!((fits.chars().count(), over.chars().count()), (40, SUMMARY_MAX + 1));

        let prefixed = delivery(&["src/a.rs"], &format!("fix(rt): {fits}"));
        assert!(check_summary(&prefixed).is_ok(), "the prefix does not count");
        let made = message(root, &prefixed, Locale::PtBr).unwrap_or_else(|_| panic!("fits")).expect("a message");
        assert_eq!(made.title, format!("feat: {fits}"));

        let Err(refused) = check_summary(&delivery(&["src/a.rs"], &format!("fix(rt): {over}"))) else {
            panic!("a summary over the limit without the prefix is refused");
        };
        assert_eq!(refused.reason(), "commit-too-long");
    }
}
