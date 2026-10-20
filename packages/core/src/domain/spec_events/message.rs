//! A mensagem do pull request, montada do arquivo de eventos e nunca
//! escrita à mão, e a conferência por que passa toda mensagem de commit ou de
//! pull request.

use crate::platform::i18n::{translate, Locale};

use super::SpecLog;

/// O teto de caracteres do título de um pull request e de um commit.
pub const MESSAGE_TITLE_MAX: usize = 60;
/// O teto de caracteres do corpo de um pull request e de um commit.
pub const MESSAGE_BODY_MAX: usize = 4_000;

/// O que uma mensagem de commit ou de pull request nunca leva, com o texto que
/// a recusa mostra. A busca é feita sobre o texto dobrado
/// ([`crate::domain::text::fold`]), por isso cada agulha vem em minúscula e sem
/// acento.
///
/// O caminho da máquina entra aqui pelas duas grafias que ele tem: um pull
/// request que cita `/home/alguem/projetos` diz o nome de quem trabalha e a
/// árvore de pastas dessa pessoa, que é dado de usuário como qualquer outro.
const FORBIDDEN_IN_MESSAGE: &[(&str, &str)] = &[
    ("claude.ai", "claude.ai"),
    ("claude", "Claude"),
    ("anthropic", "Anthropic"),
    ("co-authored-by", "Co-Authored-By"),
    ("generated with", "Generated with"),
    ("/home/", "/home/"),
    ("c:\\users\\", "C:\\Users\\"),
];

/// O que contém uma agulha de [`FORBIDDEN_IN_MESSAGE`] e mesmo assim passa,
/// já dobrado: `CLAUDE.md` é o nome de um arquivo do projeto, que o commit
/// que mexe nele precisa poder citar, e não o nome do assistente.
const ALLOWED_IN_MESSAGE: &[&str] = &["claude.md"];

/// Onde `needle` aparece em `folded` fora de um nome de [`ALLOWED_IN_MESSAGE`].
fn forbidden_at(folded: &str, needle: &str) -> Option<usize> {
    let allowed: Vec<(usize, usize)> = ALLOWED_IN_MESSAGE
        .iter()
        .flat_map(|name| folded.match_indices(name).map(|(at, found)| (at, at + found.len())))
        .collect();
    folded
        .match_indices(needle)
        .map(|(at, _)| at)
        .find(|&at| !allowed.iter().any(|&(start, end)| start <= at && at + needle.len() <= end))
}

/// Por que uma mensagem de commit ou de pull request foi recusada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRefusal {
    /// O título ou o corpo passou do teto.
    TooLong {
        /// `title` ou `body`.
        part: &'static str,
        /// Quantos caracteres a parte tem.
        chars: usize,
        /// Quantos ela podia ter.
        max: usize,
    },
    /// A mensagem traz o que ela nunca leva. `found` é o trecho pelo nome e
    /// `excerpt` é o pedaço da mensagem em que ele apareceu, para que a recusa
    /// aponte onde está em vez de mandar procurar.
    Forbidden {
        /// O trecho proibido, pelo nome.
        found: String,
        /// O pedaço da mensagem em que ele apareceu.
        excerpt: String,
    },
    /// A spec não tem objetivo escrito, e é dele que sai o título.
    NoTitle,
}

impl MessageRefusal {
    /// O motivo estável, para quem lê a resposta como dado.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::TooLong { .. } => "message-too-long",
            Self::Forbidden { .. } => "message-forbidden-text",
            Self::NoTitle => "message-no-title",
        }
    }

    /// A recusa em palavras, no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        match self {
            Self::TooLong { part, chars, max } => translate("message.too_long", lang)
                .replace("{part}", part)
                .replace("{chars}", &chars.to_string())
                .replace("{max}", &max.to_string()),
            Self::Forbidden { found, excerpt } => translate("message.forbidden", lang)
                .replace("{found}", found)
                .replace("{excerpt}", excerpt),
            Self::NoTitle => translate("message.no_title", lang).to_string(),
        }
    }
}

/// O pedaço de `text` em volta de `at`, com no máximo 40 caracteres de cada
/// lado: é o que a recusa mostra para que ninguém precise procurar.
fn excerpt_around(text: &str, at: usize) -> String {
    let start = text[..at].char_indices().rev().take(40).last().map_or(0, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .take(40)
        .last()
        .map_or(text.len(), |(i, c)| at + i + c.len_utf8());
    text[start..end].trim().to_string()
}

/// O primeiro e-mail que o texto traz: uma palavra, um arroba e um domínio com
/// ponto.
fn first_email(text: &str) -> Option<(String, usize)> {
    for (at, _) in text.match_indices('@') {
        let start = text[..at].rfind(char::is_whitespace).map_or(0, |i| i + 1);
        let end = text[at..].find(char::is_whitespace).map_or(text.len(), |i| at + i);
        let candidate = text[start..end].trim_matches(|c: char| !c.is_alphanumeric());
        let Some((user, domain)) = candidate.split_once('@') else { continue };
        if !user.is_empty() && domain.contains('.') && !domain.starts_with('.') {
            return Some((candidate.to_string(), start));
        }
    }
    None
}

/// Confere uma mensagem de commit ou de pull request contra o modelo: título e
/// corpo dentro do teto, e nada do que ela nunca leva — nem o que liga a
/// mensagem ao trabalho do Mustard ([`work_reference`]).
///
/// A conferência é uma só para as duas mensagens de propósito. Enquanto foram
/// duas, a regra valia onde alguém lembrou de escrevê-la — e a regra existe
/// justamente para o caso em que ninguém está olhando.
///
/// # Errors
///
/// [`MessageRefusal::TooLong`] quando uma das partes passa do teto e
/// [`MessageRefusal::Forbidden`] quando o texto traz o que nunca leva, com o
/// trecho pelo nome e o pedaço em que ele apareceu.
pub fn check_message(
    title: &str,
    body: &str,
    title_max: usize,
    body_max: usize,
) -> Result<(), MessageRefusal> {
    for (part, text, max) in [("title", title, title_max), ("body", body, body_max)] {
        let chars = text.chars().count();
        if chars > max {
            return Err(MessageRefusal::TooLong { part, chars, max });
        }
    }
    let whole = format!("{title}\n{body}");
    let folded = crate::domain::text::fold(&whole);
    for (needle, shown) in FORBIDDEN_IN_MESSAGE {
        if let Some(at) = forbidden_at(&folded, needle) {
            return Err(MessageRefusal::Forbidden {
                found: (*shown).to_string(),
                excerpt: excerpt_around(&folded, at),
            });
        }
    }
    // A pasta pessoal do macOS se lê pela grafia dela: `/users/` em minúscula
    // é rota de serviço, e não a pasta de ninguém.
    if let Some(at) = whole.find("/Users/") {
        return Err(MessageRefusal::Forbidden { found: "/Users/".to_string(), excerpt: excerpt_around(&whole, at) });
    }
    if let Some(found) = work_reference(&whole) {
        let at = whole.find(&found).unwrap_or_default();
        return Err(MessageRefusal::Forbidden { excerpt: excerpt_around(&whole, at), found });
    }
    if let Some((email, at)) = first_email(&folded) {
        return Err(MessageRefusal::Forbidden { found: email, excerpt: excerpt_around(&folded, at) });
    }
    Ok(())
}

/// O que liga uma mensagem ao trabalho do Mustard, e nunca vai num commit
/// nem num pull request: o número de uma onda (`onda 3`, `ondas 4 e 5`,
/// `wave 12`) e o código de um item da spec — duas siglas e um número ligados
/// por hífen. A palavra sem o número, e o código de duas partes como `UTF-8`,
/// passam.
fn work_reference(text: &str) -> Option<String> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '`' | '"' | '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let numbered = words.windows(2).find(|pair| {
        ["onda", "ondas", "wave", "waves"].contains(&crate::domain::text::fold(pair[0]).as_str()) && pair[1].starts_with(|c: char| c.is_ascii_digit())
    });
    if let Some([word, number]) = numbered {
        return Some(format!("{word} {number}"));
    }
    words.iter().map(|word| word.trim_end_matches(['.', '!', '?'])).find(|word| {
        let parts: Vec<&str> = word.split('-').collect();
        parts.len() == 3
            && parts[..2].iter().all(|part| part.len() >= 2 && part.chars().all(|c| c.is_ascii_uppercase()))
            && !parts[2].is_empty()
            && parts[2].chars().all(|c| c.is_ascii_digit())
    }).map(str::to_string)
}

/// A primeira frase de `text`, sem título de markdown e sem negrito.
///
/// Quem lê a frase é a leitura do índice da spec, a mesma que tira o objetivo
/// do primeiro `context`: uma frase só se lê de um jeito só.
fn first_sentence_of(text: &str) -> String {
    let plain = crate::domain::spec_index::after_titles(text).replace("**", "");
    crate::domain::spec_index::first_sentence(&plain).to_string()
}

/// O título e o corpo do pull request desta spec, montados do arquivo de
/// eventos.
///
/// **Ninguém escreve este texto à mão.** O título sai do objetivo da spec; o
/// corpo sai, em ordem de importância, do resumo que o assistente gravou, de
/// uma linha `- resumo` por entrega, com o resumo que a onda deu ao commit, e
/// de uma linha do que testar à mão, tirada das tarefas do plano, que só
/// lembra e não trava nada. O pull request vai ao git da equipe e não leva
/// nada do Mustard: nem número de onda, nem código da spec, nem medida.
/// Quando o corpo passa do teto, sai o que menos importa. A linha do que
/// testar à mão sai em `lang`, o idioma do projeto.
///
/// A entrega ou a tarefa cujo texto traz o que nunca vai num pull request sai
/// do corpo sozinha, sem levar o resto junto: o texto dela foi escrito para a
/// obra, não para o pull request, e recusar tudo por causa dele travaria a
/// entrega por uma linha que ninguém escreveu para a equipe ler.
///
/// # Errors
///
/// A spec sem objetivo escrito ([`MessageRefusal::NoTitle`]), o título acima do
/// teto ([`MessageRefusal::TooLong`], que pede outro objetivo em vez de cortar
/// uma frase ao meio) e o título ou o resumo que traz o que nunca vai num pull
/// request ([`MessageRefusal::Forbidden`]).
pub fn pr_message(log: &SpecLog, lang: Locale) -> Result<(String, String), MessageRefusal> {
    // A linha que passa sozinha pela conferência; o teto é do corpo inteiro,
    // conferido no fim.
    let passes = |line: &str| check_message("", line, usize::MAX, usize::MAX).is_ok();
    let title = crate::domain::spec_index::goal_of(log).ok_or(MessageRefusal::NoTitle)?;
    let visible = log.visible();
    let summary = visible
        .iter()
        .rev()
        .find(|e| e.event_type == "pr_summary")
        .and_then(|e| e.str_field("text"))
        .unwrap_or_default()
        .trim()
        .to_string();

    // Uma linha por entrega, com o resumo que a onda deu ao commit — a volta
    // que a entrega oficial assumiu o guarda —: a mesma frase do commit, que
    // diz a mudança e nada do trabalho do Mustard. A entrega sem resumo não
    // mudou arquivo e não tem o que dizer.
    let mut changes: Vec<String> = Vec::new();
    for event in visible.iter().filter(|e| e.event_type == "delivered") {
        let said = event.replaced().into_iter().max().and_then(|id| log.get(id)).and_then(|back| back.str_field("commit"));
        if let Some(line) = said
            .map(|said| format!("- {}", said.trim()))
            .filter(|line| !changes.contains(line) && passes(line))
        {
            changes.push(line);
        }
    }

    // O que testar à mão: a primeira frase de cada tarefa das ondas do plano.
    let planned = log.planned_waves();
    let tasks: Vec<String> = visible
        .iter()
        .filter(|e| e.event_type == "task" && e.wave().is_some_and(|n| planned.contains(&n)))
        .filter_map(|e| e.str_field("text"))
        .map(|text| first_sentence_of(text).trim().trim_end_matches(['.', '!', '?']).to_string())
        .filter(|task| !task.is_empty() && passes(task))
        .collect();
    let by_hand =
        (!tasks.is_empty()).then(|| translate("message.by_hand", lang).replace("{tasks}", &tasks.join("; ")));

    let assemble = |lines: &[String], by_hand: Option<&str>| {
        let mut parts: Vec<String> = Vec::new();
        if !summary.is_empty() {
            parts.push(summary.clone());
        }
        if !lines.is_empty() {
            parts.push(lines.join("\n"));
        }
        parts.extend(by_hand.map(str::to_string));
        parts.join("\n\n")
    };

    // Acima do teto, sai primeiro o que menos importa: o que testar à mão, e
    // depois as linhas das entregas.
    let mut body = assemble(&changes, by_hand.as_deref());
    if body.chars().count() > MESSAGE_BODY_MAX {
        body = assemble(&changes, None);
        if body.chars().count() > MESSAGE_BODY_MAX {
            body = assemble(&[], None);
        }
    }

    check_message(&title, &body, MESSAGE_TITLE_MAX, MESSAGE_BODY_MAX)?;
    Ok((title, body))
}

/// O texto `text` sem as palavras que uma mensagem de commit ou de pull
/// request nunca leva, pela mesma conferência ([`check_message`]), e as que
/// saíram, na ordem do texto. Sai a palavra que a conferência recusa sozinha
/// — o nome do assistente, um e-mail, uma pasta pessoal, o código de um item
/// da spec — e o par que ela recusa junto — o número de uma onda, uma
/// assinatura de duas palavras —; o resto fica como estava. É o que o pull
/// request faz com a linha que falha, no tamanho de uma palavra: o resumo de
/// um commit é uma linha só. O texto que passa inteiro volta como veio; o que
/// não passa nem assim volta vazio, com todas as palavras como saídas.
#[must_use]
pub fn without_forbidden(text: &str) -> (String, Vec<String>) {
    let passes = |line: &str| check_message("", line, usize::MAX, usize::MAX).is_ok();
    if passes(text) {
        return (text.to_string(), Vec::new());
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut keep = vec![true; words.len()];
    // Tirar uma palavra pode juntar duas que só passam separadas: a passada
    // se repete até o texto passar ou nada mais sair.
    loop {
        let before = keep.clone();
        for (at, word) in words.iter().enumerate() {
            if keep[at] && !passes(word) {
                keep[at] = false;
            }
        }
        let alive: Vec<usize> = (0..words.len()).filter(|at| keep[*at]).collect();
        for pair in alive.windows(2) {
            if keep[pair[0]] && keep[pair[1]] && !passes(&format!("{} {}", words[pair[0]], words[pair[1]])) {
                keep[pair[0]] = false;
                keep[pair[1]] = false;
            }
        }
        let kept: Vec<&str> = words.iter().zip(&keep).filter(|(_, kept)| **kept).map(|(word, _)| *word).collect();
        if passes(&kept.join(" ")) || keep == before {
            break;
        }
    }
    let kept: Vec<&str> = words.iter().zip(&keep).filter(|(_, kept)| **kept).map(|(word, _)| *word).collect();
    let joined = kept.join(" ");
    if !passes(&joined) {
        return (String::new(), words.iter().map(|word| (*word).to_string()).collect());
    }
    let removed = words.iter().zip(&keep).filter(|(_, kept)| !**kept).map(|(word, _)| (*word).to_string()).collect();
    (joined, removed)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::spec_events::tests::obj;
    use crate::domain::spec_events::{parse_log, render_line};

    /// Do resumo sai só a palavra, ou o par, que a conferência recusa; o
    /// resto fica, e o que passa inteiro volta como veio.
    #[test]
    fn only_the_forbidden_words_leave_a_summary() {
        let cases = [
            ("Avisa fulano@empresa.com.br no erro", "Avisa no erro", vec!["fulano@empresa.com.br"]),
            ("Corrige a onda 3 da leitura", "Corrige a da leitura", vec!["onda", "3"]),
            ("Gancho do Claude mais curto", "Gancho do mais curto", vec!["Claude"]),
            ("Claude", "", vec!["Claude"]),
            ("Cita o CLAUDE.md no início", "Cita o CLAUDE.md no início", vec![]),
        ];
        for (said, kept, removed) in cases {
            let (left, out) = without_forbidden(said);
            assert_eq!((left.as_str(), out.iter().map(String::as_str).collect::<Vec<_>>()), (kept, removed), "{said}");
        }
    }

    /// O corpo do pull request descreve a mudança e nada do Mustard: uma linha
    /// `- resumo` por entrega, com o resumo que a onda deu ao commit, sem número
    /// de onda nem contagem de critérios. O resumo que cita uma onda ou um
    /// código da spec é recusado, como no commit.
    #[test]
    fn the_pull_request_lists_each_change_by_its_summary_and_nothing_of_the_work() {
        let events = |pr_summary: &str| {
            let mut lines: Vec<String> = Vec::new();
            let fields = [
                json!({"type": "context", "text": "Deixar o Mustard enxuto."}),
                json!({"type": "criterion", "when": "a planilha abre", "then": "lê tudo", "proof": "teste"}),
                json!({"type": "delivered", "wave": 1, "files": ["a.rs"], "text": "A onda 1 entregou.", "commit": "Leitura das planilhas grandes", "returned": true}),
                json!({"type": "delivered", "wave": 1, "files": ["a.rs"], "text": "A onda 1 entregou.", "replaces": 3}),
                json!({"type": "delivered", "wave": 2, "files": ["b.rs"], "text": "A onda 2 entregou.", "commit": "Aviso da rodada", "returned": true}),
                json!({"type": "delivered", "wave": 2, "files": ["b.rs"], "text": "A onda 2 entregou.", "replaces": 5}),
                json!({"type": "criterion_run", "criterion": 2, "result": "pass", "exit": 0, "ms": 3}),
                json!({"type": "pr_summary", "text": pr_summary}),
            ];
            for (i, value) in fields.into_iter().enumerate() {
                let mut map = obj(value);
                map.insert("v".into(), json!(1));
                map.insert("id".into(), json!(i + 1));
                map.insert("at".into(), json!("2026-09-16T10:00:00-03:00"));
                map.insert("author".into(), json!("assistant"));
                lines.push(render_line(&map));
            }
            parse_log(&lines.join("\n"))
        };
        let (_, body) = pr_message(&events("O portão lê o estado."), Locale::PtBr).expect("a spec tem objetivo e resumo");
        assert_eq!(body, "O portão lê o estado.\n\n- Leitura das planilhas grandes\n- Aviso da rodada");
        for cited in ["Fecha a onda 3.", "Cobre o MSTD-CRIT-0001."] {
            let refusal = pr_message(&events(cited), Locale::PtBr).expect_err(cited);
            assert_eq!(refusal.reason(), "message-forbidden-text", "{cited}");
        }
    }

    /// O corpo termina com uma linha do que testar à mão, tirada da primeira
    /// frase de cada tarefa das ondas do plano; a tarefa de uma onda que não
    /// está no plano fica de fora, e a spec sem tarefa não ganha a linha.
    #[test]
    fn the_body_says_what_to_test_by_hand_from_the_tasks() {
        let events = |tasks: &[(u64, &str)]| {
            let mut lines = Vec::new();
            let mut fields = vec![
                json!({"type": "context", "text": "Deixar o Mustard enxuto."}),
                json!({"type": "wave", "n": 1, "text": "Onda 1.", "criteria": [], "done_when": "pronto"}),
            ];
            for (wave, text) in tasks {
                fields.push(json!({"type": "task", "wave": wave, "text": text}));
            }
            for (i, value) in fields.into_iter().enumerate() {
                let mut map = obj(value);
                map.insert("v".into(), json!(1));
                map.insert("id".into(), json!(i + 1));
                map.insert("at".into(), json!("2026-09-17T10:00:00-03:00"));
                map.insert("author".into(), json!("assistant"));
                lines.push(render_line(&map));
            }
            parse_log(&lines.join("\n"))
        };
        let log = events(&[(1, "Abrir a spec pelo comando. E o resto."), (1, "Fechar com o lint!"), (9, "Fora do plano.")]);
        let (_, body) = pr_message(&log, Locale::PtBr).expect("a spec tem objetivo");
        assert!(
            body.ends_with("Testar à mão: Abrir a spec pelo comando; Fechar com o lint."),
            "{body}"
        );
        assert!(!body.contains("Fora do plano"), "{body}");
        let (_, bare) = pr_message(&events(&[]), Locale::PtBr).expect("a spec tem objetivo");
        assert!(!bare.contains("Testar à mão"), "{bare}");
    }

    /// O log de uma spec com objetivo, um resumo, duas entregas e duas tarefas
    /// na onda 1 do plano, cada texto escolhido por quem chama.
    fn spec_with(summary: &str, commits: [&str; 2], tasks: [&str; 2]) -> SpecLog {
        let mut fields = vec![
            json!({"type": "context", "text": "Deixar o Mustard enxuto."}),
            json!({"type": "wave", "n": 1, "text": "Onda 1.", "criteria": [], "done_when": "pronto"}),
        ];
        for task in tasks {
            fields.push(json!({"type": "task", "wave": 1, "text": task}));
        }
        for commit in commits {
            let back = fields.len() + 1;
            fields.push(json!({"type": "delivered", "wave": 1, "files": ["a.rs"], "text": "Saiu.", "commit": commit, "returned": true}));
            fields.push(json!({"type": "delivered", "wave": 1, "files": ["a.rs"], "text": "Saiu.", "replaces": back}));
        }
        fields.push(json!({"type": "pr_summary", "text": summary}));
        let lines: Vec<String> = fields
            .into_iter()
            .enumerate()
            .map(|(i, value)| {
                let mut map = obj(value);
                map.insert("v".into(), json!(1));
                map.insert("id".into(), json!(i + 1));
                map.insert("at".into(), json!("2026-10-19T10:00:00-03:00"));
                map.insert("author".into(), json!("assistant"));
                render_line(&map)
            })
            .collect();
        parse_log(&lines.join("\n"))
    }

    /// A tarefa ou a entrega cujo texto traz o que nunca vai num pull request
    /// sai do corpo sozinha: o resto do pull request segue. O resumo e o
    /// título continuam recusando a mensagem inteira, porque foram escritos
    /// para ela.
    #[test]
    fn only_the_offending_line_leaves_the_body() {
        let log = spec_with(
            "O portão lê o estado.",
            ["Leitura das planilhas", "Cobre o MSTD-CRIT-0001"],
            ["Abrir a spec pelo comando", "Conferir a onda 3 inteira"],
        );
        let (_, body) = pr_message(&log, Locale::PtBr).expect("só as linhas recusadas saem");
        assert!(body.contains("- Leitura das planilhas"), "{body}");
        assert!(!body.contains("MSTD-CRIT-0001"), "{body}");
        assert!(body.contains("Abrir a spec pelo comando"), "{body}");
        assert!(!body.contains("onda 3"), "{body}");

        let refused = spec_with("Fecha a onda 3.", ["Leitura das planilhas", "Aviso"], ["Abrir", "Fechar"]);
        assert_eq!(pr_message(&refused, Locale::PtBr).expect_err("o resumo recusa").reason(), "message-forbidden-text");
    }

    /// A linha do que testar à mão sai no idioma do projeto.
    #[test]
    fn the_test_by_hand_line_follows_the_project_language() {
        let log = spec_with("The gate reads the state.", ["Large sheets", "Round notice"], ["Open the spec", "Close it"]);
        let (_, body) = pr_message(&log, Locale::EnUs).expect("a spec tem objetivo");
        assert!(body.ends_with("Test by hand: Open the spec; Close it."), "{body}");
        assert!(!body.contains("Testar à mão"), "{body}");
    }

    /// `CLAUDE.md` é o nome de um arquivo do projeto, não o do assistente: a
    /// mensagem que o cita passa, e a que diz o nome do assistente continua
    /// recusada.
    #[test]
    fn the_claude_md_file_name_is_not_the_assistant_name() {
        assert_eq!(check_message("Atualiza o CLAUDE.md", "Regras novas no CLAUDE.md da raiz.", 60, 4_000), Ok(()));
        let refusal = check_message("Atualiza o CLAUDE.md", "Escrito com o Claude.", 60, 4_000)
            .expect_err("o nome do assistente continua recusado");
        assert_eq!(refusal.reason(), "message-forbidden-text");
    }
}
