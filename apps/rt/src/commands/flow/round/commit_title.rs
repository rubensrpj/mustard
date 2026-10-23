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

/// As pastas que só dizem onde o código mora, e não de que parte dele se
/// trata: o escopo sai da pasta de dentro delas.
const GENERIC_DIRS: &[&str] = &["src", "lib", "libs", "app", "apps", "packages", "crates", "modules", "pkg", "test", "tests", "spec"];

/// O escopo do commit, tirado dos arquivos dele: a pasta que todos eles têm em
/// comum, sem as pastas da frente que só dizem onde o código mora
/// ([`GENERIC_DIRS`]). `None` quando os arquivos se espalham, quando a pasta
/// comum é escondida — a configuração do editor não é parte do módulo — ou
/// quando o nome dela traz o que um commit nunca leva.
///
/// O escopo saía do histórico dos arquivos: o primeiro título deles com
/// `tipo(escopo):` servia de molde. Numa correção de verdade os três commits
/// saíram com o mesmo escopo, e um deles só devolvia a configuração do editor,
/// que não tem nada a ver com o módulo. O histórico diz onde aqueles arquivos
/// já estiveram; os arquivos deste commit dizem onde ele mexe.
fn scope_of(files: &[String]) -> Option<String> {
    fn dirs(file: &str) -> Vec<&str> {
        let mut parts: Vec<&str> = file.split('/').collect();
        parts.pop();
        parts
    }
    let mut common = dirs(files.first()?);
    for file in &files[1..] {
        let other = dirs(file);
        let shared = common.iter().zip(&other).take_while(|(a, b)| a == b).count();
        common.truncate(shared);
    }
    while common.first().is_some_and(|dir| GENERIC_DIRS.contains(dir)) {
        common.remove(0);
    }
    if common.is_empty() || common.iter().any(|dir| dir.starts_with('.') || dir.is_empty()) {
        return None;
    }
    let joined = common.join("/");
    let scope = if joined.chars().count() <= 32 { joined } else { (*common.last()?).to_string() };
    (scope.chars().count() <= 32 && without_forbidden(&scope).1.is_empty()).then_some(scope)
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
    let scope = scope_of(&wave.files);
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
            let cut = whole_words(&summary, MESSAGE_TITLE_MAX.saturating_sub(prefix.chars().count()));
            (!cut.is_empty()).then(|| format!("{prefix}{cut}"))
        })
        .unwrap_or_else(|| format!("{kind}: {summary}"));
    let body = if title.ends_with(&summary) { String::new() } else { summary };
    check_commit_text(&title, &body)?;
    Ok(Some(Message { title, body, removed }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O escopo do commit sai da pasta que os arquivos dele têm em comum, sem
    /// as pastas da frente que só dizem onde o código mora. Os arquivos que se
    /// espalham, a pasta escondida e o arquivo solto na raiz não dão escopo.
    ///
    /// Em campo (21/10/2026) os três commits de uma correção saíram com o
    /// mesmo escopo, tirado do histórico dos arquivos, e um deles só devolvia
    /// a configuração do editor.
    #[test]
    fn the_commit_scope_comes_from_the_folder_its_files_share() {
        let files = |list: &[&str]| list.iter().map(|file| (*file).to_string()).collect::<Vec<_>>();

        // Duas partes do mesmo módulo: o escopo é o módulo.
        assert_eq!(scope_of(&files(&["src/order/order.service.ts", "src/order/order.controller.ts"])), Some("order".to_string()));
        // A pasta de dentro vai junto, enquanto couber.
        assert_eq!(scope_of(&files(&["src/order/dto/a.ts", "src/order/dto/b.ts"])), Some("order/dto".to_string()));
        // Um arquivo só: a pasta dele.
        assert_eq!(scope_of(&files(&["packages/core/src/io/fs/mod.rs"])), Some("core/src/io/fs".to_string()));

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
    }
}
