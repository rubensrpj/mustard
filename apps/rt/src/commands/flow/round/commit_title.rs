//! A mensagem do commit de cada onda, montada num lugar só: a gravação da
//! entrega confere o resumo por aqui, e a rodada comita com o que sai daqui.
//! A mensagem descreve a mudança e nada do Mustard: nem número de onda, nem
//! tamanho, nem rodapé. O commit é achado de novo pelo conteúdo
//! ([`super::lost_commit`]), e nunca pela mensagem.
use super::{
    answer::RoundRefusal,
    commit::{check_commit_text, covered, git, needs_commit, shorten_to},
    report::WaveReport,
};
use mustard_core::domain::spec_events::{MESSAGE_TITLE_MAX, without_forbidden};
use mustard_core::platform::i18n::{Locale, translate};
use std::path::Path;

/// O teto do resumo que cada entrega traz para o commit: o que o texto do
/// agente de onda diz, e o que cabe inteiro no título ao lado de um tipo e de
/// um escopo curto.
pub(super) const SUMMARY_MAX: usize = 45;

/// O resumo de uma entrega cabe no teto. É a parte da mensagem que se
/// atribui a uma onda só: a rodada segura por ela só a onda do resumo
/// recusado. O que a mensagem nunca leva não recusa nada: sai do resumo na
/// montagem ([`message`]), como sai do pull request.
pub(super) fn check_summary(wave: &WaveReport) -> Result<(), RoundRefusal> {
    let Some(summary) = wave.commit.as_deref() else { return Ok(()) };
    let chars = summary.chars().count();
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

/// A mensagem do commit da entrega `wave`, já conferida; `None` quando ela
/// não mudou arquivo ou não trouxe resumo. O título segue o molde do histórico dos arquivos
/// (`tipo(escopo): frase`) com o resumo da entrega, cortado numa palavra
/// inteira quando o escopo não deixa espaço para ele todo; o escopo sai antes
/// de o resumo ficar sem palavra nenhuma, e o escopo que traz o que um commit
/// nunca leva não serve de molde. O corpo traz o resumo inteiro só quando o
/// título o cortou. O tipo é `fix` quando a entrega conserta outra onda, o
/// do ramo quando ele diz um, e `feat` nos outros casos.
///
/// Do resumo sai, palavra a palavra, o que um commit nunca leva
/// ([`without_forbidden`]), e as palavras que saíram vão em
/// [`Message::removed`]: o commit segue, e a resposta da rodada diz o que
/// saiu. O resumo que fica vazio vira um resumo neutro pelos arquivos, no
/// idioma `lang`.
pub(super) fn message(root: &Path, wave: &WaveReport, lang: Locale) -> Result<Option<Message>, RoundRefusal> {
    let Some(said) = wave.commit.as_deref().filter(|_| needs_commit(&wave.files)) else {
        return Ok(None);
    };
    check_summary(wave)?;
    let (kept, removed) = without_forbidden(said.trim());
    let summary = if kept.is_empty() { neutral(&wave.files, lang) } else { kept };
    let mut args = vec!["log", "-40", "--format=%s", "--"];
    args.extend(wave.files.iter().map(String::as_str));
    let history = git(root, &args).unwrap_or_default();
    let scope = history.lines().find_map(|title| {
        let (prefix, _) = title.split_once(": ")?;
        let (kind, scope) = prefix.split_once('(')?;
        let scope = scope.strip_suffix(')')?;
        if !["feat", "fix", "refactor", "docs", "test", "perf", "build", "ci", "chore", "style"].contains(&kind) {
            return None;
        }
        // O escopo que nomeia ondas é dos commits antigos da rodada, e não do
        // projeto: não serve de molde.
        let internal = scope.split('/').any(|part| {
            ["onda-", "ondas-", "wave-", "waves-"]
                .iter()
                .any(|prefix| part.strip_prefix(prefix).is_some_and(|rest| rest.chars().all(|ch| ch.is_ascii_digit() || ch == '-' || ch == '+')))
        });
        let clean = without_forbidden(scope).1.is_empty();
        (!scope.is_empty() && scope.len() <= 32 && !internal && clean).then_some(scope)
    });
    let branch = git(root, &["branch", "--show-current"]).unwrap_or_default();
    let category = branch.split('/').next().unwrap_or_default().trim();
    let kind = if !wave.fixes.is_empty() {
        "fix"
    } else if ["fix", "refactor", "docs", "test", "perf", "build", "ci", "chore", "style"].contains(&category) {
        category
    } else {
        "feat"
    };
    let scoped = scope.map(|scope| format!("{kind}({scope}): "));
    let title = scoped
        .into_iter()
        .chain([format!("{kind}: ")])
        .find_map(|prefix| {
            let cut = shorten_to(&summary, MESSAGE_TITLE_MAX.saturating_sub(prefix.chars().count()));
            (!cut.is_empty()).then(|| format!("{prefix}{cut}"))
        })
        .unwrap_or_else(|| format!("{kind}: {summary}"));
    let body = if title.ends_with(&summary) { String::new() } else { summary };
    check_commit_text(&title, &body)?;
    Ok(Some(Message { title, body, removed }))
}
