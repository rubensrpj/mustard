//! A mensagem do pull request, montada do arquivo de eventos e nunca
//! escrita à mão, e a conferência por que passa toda mensagem de commit ou de
//! pull request.

use std::collections::BTreeMap;

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
/// corpo dentro do teto, e nada do que ela nunca leva.
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
        if let Some(at) = folded.find(needle) {
            return Err(MessageRefusal::Forbidden {
                found: (*shown).to_string(),
                excerpt: excerpt_around(&folded, at),
            });
        }
    }
    if let Some((email, at)) = first_email(&folded) {
        return Err(MessageRefusal::Forbidden { found: email, excerpt: excerpt_around(&folded, at) });
    }
    Ok(())
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
/// uma linha por onda entregue, da contagem dos critérios com as falhas pelo
/// nome e de uma linha do que testar à mão, tirada das tarefas do plano, que
/// só lembra e não trava nada. Quando o corpo passa do teto, as listas viram
/// contagem — o detalhe não se perde, porque ele mora na página da spec.
///
/// # Errors
///
/// A spec sem objetivo escrito ([`MessageRefusal::NoTitle`]), o título acima do
/// teto ([`MessageRefusal::TooLong`], que pede outro objetivo em vez de cortar
/// uma frase ao meio) e o texto que traz o que nunca vai num pull request
/// ([`MessageRefusal::Forbidden`]).
pub fn pr_message(log: &SpecLog) -> Result<(String, String), MessageRefusal> {
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

    let mut waves: BTreeMap<u64, String> = BTreeMap::new();
    for event in &visible {
        if event.event_type != "delivered" {
            continue;
        }
        if let (Some(wave), Some(text)) = (event.wave(), event.str_field("text")) {
            waves.insert(wave, first_sentence_of(text));
        }
    }

    // Quantos critérios existem e quais falharam é leitura do QA, que já mora
    // no núcleo e já decide pela execução mais nova de cada um. Contar aqui de
    // novo seria uma segunda resposta para a mesma pergunta.
    let qa = crate::domain::spec_state::qa(log);
    let criteria = qa.criteria;
    let failed: Vec<String> = qa
        .failed_ids
        .iter()
        .map(|id| {
            visible
                .iter()
                .find(|e| e.id == *id)
                .and_then(|e| e.str_field("when"))
                .map_or_else(|| id.to_string(), first_sentence_of)
        })
        .collect();

    let wave_lines: Vec<String> =
        waves.iter().map(|(wave, what)| format!("- onda {wave}: {what}")).collect();
    let criteria_line = if failed.is_empty() {
        format!("Critérios: {criteria}, nenhum com falha.")
    } else {
        format!("Critérios: {criteria}, {} com falha: {}.", failed.len(), failed.join("; "))
    };

    // O que testar à mão: a primeira frase de cada tarefa das ondas do plano.
    let planned = log.planned_waves();
    let tasks: Vec<String> = visible
        .iter()
        .filter(|e| e.event_type == "task" && e.wave().is_some_and(|n| planned.contains(&n)))
        .filter_map(|e| e.str_field("text"))
        .map(|text| first_sentence_of(text).trim().trim_end_matches(['.', '!', '?']).to_string())
        .filter(|task| !task.is_empty())
        .collect();
    let by_hand = (!tasks.is_empty()).then(|| format!("Testar à mão: {}.", tasks.join("; ")));

    let assemble = |lines: &[String], by_hand: Option<&str>| {
        let mut parts: Vec<String> = Vec::new();
        if !summary.is_empty() {
            parts.push(summary.clone());
        }
        if !lines.is_empty() {
            parts.push(lines.join("\n"));
        }
        parts.push(criteria_line.clone());
        parts.extend(by_hand.map(str::to_string));
        parts.join("\n\n")
    };

    let mut body = assemble(&wave_lines, by_hand.as_deref());
    if body.chars().count() > MESSAGE_BODY_MAX {
        // As listas viram contagem: o detalhe fica na página da spec, onde
        // nada se perde, e o corpo continua legível de uma olhada.
        let collapsed = vec![format!("Ondas entregues: {}.", waves.len())];
        body = assemble(&collapsed, by_hand.as_deref());
        if body.chars().count() > MESSAGE_BODY_MAX {
            let counted = format!("Testar à mão: as {} tarefas, na página da spec.", tasks.len());
            body = assemble(&collapsed, Some(&counted));
        }
    }

    check_message(&title, &body, MESSAGE_TITLE_MAX, MESSAGE_BODY_MAX)?;
    Ok((title, body))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::domain::spec_events::tests::obj;
    use crate::domain::spec_events::{parse_log, render_line};

    /// A linha de cada onda no corpo do pull request pula o título — o
    /// cabeçalho e o trecho em negrito sozinho na linha — e fecha a frase
    /// também no ponto de exclamação e no de interrogação.
    ///
    /// Quem lê a primeira frase é a leitura que já existe no pacote. Uma
    /// segunda leitura escrita aqui devolveria o título em negrito inteiro no
    /// lugar da frase, e arrastaria a prosa que vem depois do `!`.
    #[test]
    fn a_linha_da_onda_pula_o_titulo_e_fecha_a_frase_em_qualquer_ponto() {
        let mut lines: Vec<String> = Vec::new();
        let mut id = 0u64;
        let mut push = |fields: Value| {
            id += 1;
            let mut map = obj(fields);
            map.insert("v".into(), json!(1));
            map.insert("id".into(), json!(id));
            map.insert("at".into(), json!("2026-09-16T10:00:00-03:00"));
            map.insert("author".into(), json!("assistant"));
            lines.push(render_line(&map));
        };
        push(json!({"type": "context", "text": "Deixar o Mustard enxuto."}));
        push(json!({
            "type": "delivered",
            "wave": 1,
            "files": ["a.rs"],
            "text": "**O portão de corte**\nA onda parou de perguntar? Depois vem o resto.",
        }));
        push(json!({
            "type": "delivered",
            "wave": 2,
            "files": ["b.rs"],
            "text": "# A prova do vermelho\nFuncionou! E sobrou prosa depois.",
        }));
        push(json!({"type": "pr_summary", "text": "O portão lê o estado."}));

        let log = parse_log(&lines.join("\n"));
        let (_, body) = pr_message(&log).expect("a spec tem objetivo e resumo");
        assert!(
            body.contains("- onda 1: A onda parou de perguntar?"),
            "a linha da onda 1 não pulou o negrito ou não fechou no ponto de \
             interrogação: {body}",
        );
        assert!(
            body.contains("- onda 2: Funcionou!"),
            "a linha da onda 2 não fechou no ponto de exclamação: {body}",
        );
    }

    /// O corpo termina com uma linha do que testar à mão, tirada da primeira
    /// frase de cada tarefa das ondas do plano; a tarefa de uma onda que não
    /// está no plano fica de fora, e a spec sem tarefa não ganha a linha.
    #[test]
    fn o_corpo_diz_o_que_testar_a_mao_a_partir_das_tarefas() {
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
        let (_, body) = pr_message(&log).expect("a spec tem objetivo");
        assert!(
            body.ends_with("Testar à mão: Abrir a spec pelo comando; Fechar com o lint."),
            "{body}"
        );
        assert!(!body.contains("Fora do plano"), "{body}");
        let (_, bare) = pr_message(&events(&[])).expect("a spec tem objetivo");
        assert!(!bare.contains("Testar à mão"), "{bare}");
    }
}
