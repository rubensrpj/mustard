//! `clarity` — mede uma resposta do assistente contra a regra de escrita.
//!
//! A regra pede uma escrita que se lê uma vez, por quem não escreveu o código:
//! uma ideia por frase; nenhuma sigla sem as palavras por extenso; nenhum
//! código do Mustard (`MSTD-RULE-0005`) no lugar do nome do assunto; e nenhuma
//! resposta maior do que o assunto pede. Este módulo confere isso por sinais
//! objetivos — quantas palavras tem cada frase, quais siglas e códigos
//! aparecem, quantas linhas a resposta tem e a nota de facilidade de leitura
//! (o índice de Flesch adaptado ao português). Ele não tenta entender o
//! sentido do texto.
//!
//! Função pura: sem disco, sem log, sem relógio. A única lista fixa é a das
//! poucas siglas que dispensam expansão.
//!
//! Fica fora da medição do texto tudo o que não é texto corrido: blocos de
//! código, código inline, URLs, caminhos de arquivo, linhas de tabela e JSON.
//! Cada linha de texto é medida sozinha: numa resposta de chat a quebra de
//! linha separa ideias, e um item de lista conta como frase. O tamanho é a
//! exceção: conta todas as linhas não vazias ([`MAX_LINES`]), porque é o que o
//! leitor tem de percorrer.
//!
//! Há ainda a medição do idioma. A resposta sai no idioma do projeto, que é o
//! do usuário. O idioma da prosa sai de uma contagem de palavras comuns do
//! português e do inglês ([`COMMON_WORDS_PT`], [`COMMON_WORDS_EN`], que moram
//! em `domain::text`). Não há modelo estatístico: a contagem é determinística
//! e só julga com prosa bastante ([`MIN_LANGUAGE_WORDS`]). Ela vale para todo
//! projeto, qualquer que seja o tom: [`measure_language`] a faz sozinha, e
//! [`measure`] a inclui junto das medições da escrita.

use crate::domain::mustard_id;
use crate::domain::text::{COMMON_WORDS_EN, COMMON_WORDS_PT};
use crate::platform::i18n::{translate, Locale};

/// Palavras acima das quais uma frase conta como longa.
pub const MAX_SENTENCE_WORDS: usize = 25;

/// Linhas não vazias acima das quais a resposta conta como longa demais. Conta
/// a resposta inteira, com código e tabela: é o que o leitor tem de percorrer.
pub const MAX_LINES: usize = 15;

/// A nota mínima de facilidade de leitura, no índice de Flesch adaptado ao
/// português (Martins et al., 1996). Abaixo de 25 a escala diz "muito difícil".
pub const MIN_READING_EASE: i32 = 25;

/// Quantas palavras do começo de uma frase longa vão para o relatório: o
/// bastante para o leitor achar a frase, pouco para não repetir a resposta.
const OPENING_WORDS: usize = 8;

/// Siglas que dispensam expansão. São o vocabulário da web e do git que quem
/// usa o Mustard lê todo dia sem pensar; escrever "HTML (linguagem de marcação
/// de hipertexto)" deixaria a frase mais difícil, o contrário do que a regra
/// pede. A lista é curta de propósito: sigla fora dela precisa das palavras por
/// extenso na primeira vez.
const COMMON_ACRONYMS: &[&str] = &[
    "API", "CPU", "CSS", "HTML", "HTTP", "HTTPS", "ID", "JSON", "OK", "PDF", "PR", "SQL", "URL",
    "UTF",
];

/// Travessão ou hífen colado logo depois do termo: abre um aposto que explica o
/// termo em qualquer ponto da frase ("o slug — o nome curto da spec — mudou").
const APPOSITION_MARKS: &[&str] = &[" — ", " - "];

/// Dois-pontos colado logo depois do termo. Só explica quando o termo abre a
/// frase, como num glossário ("CI: integração contínua"). No meio da frase
/// ("Troquei o slug: agora é outro") ele anuncia o que vem depois, não o termo.
const LABEL_MARK: &str = ": ";

/// Expressões que anunciam a explicação quando ocupam as palavras logo depois
/// do termo ("o CI, ou seja, a integração contínua"). Mais adiante na frase
/// não contam: "o CI falhou, e o que é pior" não explica o CI.
const EXPLAINING_PHRASES: &[&str] = &[
    "que é", "que são", "ou seja", "isto é", "which is", "which are", "meaning",
];

/// Quantas palavras depois do termo podem trazer uma [`EXPLAINING_PHRASES`].
const PHRASE_WINDOW: usize = 2;

/// Pontuação que fecha o termo sem separá-lo do que vem depois (ênfase do
/// markdown, aspas): "**slug** — o nome curto" ainda é o termo colado ao
/// travessão.
const TERM_CLOSERS: &[char] = &['*', '_', '"', '\'', '”', '»'];

/// Pontuação que abre uma palavra sem fazer parte dela (parêntese, aspas,
/// ênfase do markdown).
const LEADERS: &[char] = &['(', '[', '"', '\'', '«', '“', '*', '_'];

/// Pontuação que fecha uma palavra sem fazer parte dela.
const TRAILERS: &[char] = &[
    ')', ']', '"', '\'', '»', '”', '*', '_', '.', ',', ';', ':', '!', '?', '…',
];

/// Pontuação que pode vir colada depois do ponto final de uma frase.
const CLOSERS: &[char] = &['"', '\'', ')', ']', '»', '”', '*', '_'];

/// Palavras de texto corrido abaixo das quais o idioma não é julgado: resposta
/// curta ou só de código não traz palavras comuns bastantes para uma contagem
/// honesta.
pub const MIN_LANGUAGE_WORDS: usize = 30;

/// Palavras comuns que o idioma dominante precisa somar. Trinta palavras de
/// nomes técnicos e quase nenhuma palavra comum não dizem idioma algum.
const MIN_LANGUAGE_MARKERS: usize = 5;

/// Quantas vezes as palavras comuns de um idioma precisam superar as do outro
/// para ele ser o idioma da resposta. Uma resposta em português que cita uma
/// frase em inglês continua em português.
const LANGUAGE_DOMINANCE: usize = 2;

/// Uma frase acima do limite de palavras.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongSentence {
    /// Quantas palavras a frase tem.
    pub words: usize,
    /// As primeiras palavras da frase, para o leitor saber qual é.
    pub opening: String,
}

/// A prosa da resposta saiu num idioma que não é o do projeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrongLanguage {
    /// O idioma em que a prosa foi escrita.
    pub found: Locale,
    /// O idioma do projeto, o mesmo em que o usuário escreve.
    pub expected: Locale,
}

impl WrongLanguage {
    /// A linha do defeito, no idioma pedido, tirada do catálogo i18n.
    #[must_use]
    pub fn defect(self, lang: Locale) -> String {
        translate("clarity.wrong_language", lang)
            .replace("{found}", self.found.as_str())
            .replace("{expected}", self.expected.as_str())
    }
}

/// O resultado da medição de uma resposta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClarityReport {
    /// Frases acima de [`MAX_SENTENCE_WORDS`], na ordem em que aparecem.
    pub long_sentences: Vec<LongSentence>,
    /// Siglas sem as palavras por extenso nesta resposta nem antes na sessão.
    pub unexpanded_acronyms: Vec<String>,
    /// Códigos do Mustard (`MSTD-RULE-0005`) no texto corrido, cada um uma
    /// vez, na ordem em que aparecem.
    pub internal_codes: Vec<String>,
    /// Linhas de texto corrido, sem código, tabela nem JSON.
    pub prose_lines: usize,
    /// Linhas não vazias da resposta inteira, com código e tabela.
    pub lines: usize,
    /// A resposta passou de [`MAX_LINES`] linhas.
    pub too_long: bool,
    /// A nota de facilidade de leitura do texto corrido (Flesch adaptado ao
    /// português). `None` quando a prosa não está em português ou é curta
    /// demais para uma média honesta.
    pub reading_ease: Option<i32>,
    /// A nota ficou abaixo de [`MIN_READING_EASE`].
    pub hard_to_read: bool,
    /// A prosa saiu noutro idioma que não o do projeto.
    pub wrong_language: Option<WrongLanguage>,
    /// Nenhum defeito encontrado.
    pub passed: bool,
    /// Siglas que esta resposta explicou. O chamador acumula na sessão e
    /// devolve em `already_explained` na próxima medição.
    pub explained: Vec<String>,
}

impl ClarityReport {
    /// Uma linha curta por defeito, no idioma pedido, tirada do catálogo i18n.
    #[must_use]
    pub fn defects(&self, lang: Locale) -> Vec<String> {
        let mut out = Vec::new();
        for sentence in &self.long_sentences {
            out.push(
                translate("clarity.long_sentence", lang)
                    .replace("{words}", &sentence.words.to_string())
                    .replace("{opening}", &sentence.opening),
            );
        }
        for acronym in &self.unexpanded_acronyms {
            out.push(translate("clarity.unexpanded_acronym", lang).replace("{acronym}", acronym));
        }
        for code in &self.internal_codes {
            out.push(translate("clarity.internal_code", lang).replace("{code}", code));
        }
        if self.too_long {
            out.push(
                translate("clarity.too_long", lang)
                    .replace("{lines}", &self.lines.to_string())
                    .replace("{limit}", &MAX_LINES.to_string()),
            );
        }
        if let (true, Some(score)) = (self.hard_to_read, self.reading_ease) {
            out.push(
                translate("clarity.hard_to_read", lang)
                    .replace("{score}", &score.to_string())
                    .replace("{min}", &MIN_READING_EASE.to_string()),
            );
        }
        if let Some(wrong) = self.wrong_language {
            out.push(wrong.defect(lang));
        }
        out
    }
}

/// Mede `text` contra a regra de tom didático.
///
/// `already_explained` são as siglas que respostas anteriores da mesma sessão
/// já explicaram (o campo [`ClarityReport::explained`] de cada medição
/// anterior, acumulado). `expected` é o idioma que o projeto DECLAROU: a prosa
/// da resposta precisa sair nele. `None` quando o projeto não declarou idioma —
/// sem ele não há veredito de idioma, porque o padrão resolvido não é uma
/// escolha.
#[must_use]
pub fn measure(text: &str, already_explained: &[String], expected: Option<Locale>) -> ClarityReport {
    let lines = prose_lines(text);
    let sentences: Vec<&str> = lines.iter().flat_map(|line| split_sentences(line)).collect();

    let long_sentences = long_sentences(&sentences);
    let mut explained = Vec::new();
    let unexpanded_acronyms = unexpanded_acronyms(&sentences, already_explained, &mut explained);
    let internal_codes = internal_codes(&sentences);
    let total_lines = text.lines().filter(|line| !line.trim().is_empty()).count();
    let too_long = total_lines > MAX_LINES;
    let reading_ease = reading_ease(&lines, &sentences);
    let hard_to_read = reading_ease.is_some_and(|score| score < MIN_READING_EASE);
    let wrong_language = expected.and_then(|lang| wrong_language(&lines, lang));
    let passed = long_sentences.is_empty()
        && unexpanded_acronyms.is_empty()
        && internal_codes.is_empty()
        && !too_long
        && !hard_to_read
        && wrong_language.is_none();

    ClarityReport {
        long_sentences,
        unexpanded_acronyms,
        internal_codes,
        prose_lines: lines.len(),
        lines: total_lines,
        too_long,
        reading_ease,
        hard_to_read,
        wrong_language,
        passed,
        explained,
    }
}

// ---------------------------------------------------------------------------
// Código interno
// ---------------------------------------------------------------------------

/// Os códigos do Mustard (`MSTD-RULE-0005`) no texto corrido, cada um uma
/// vez, na ordem em que aparecem: na conversa, o assunto se diz pelo nome.
/// Só esse formato conta; letra com número ("R2 da Cloudflare", "S3", "A4")
/// é texto comum. Código inline já saiu da prosa: entre crases, o código é
/// citação, não conversa.
fn internal_codes(sentences: &[&str]) -> Vec<String> {
    let mut codes = Vec::new();
    for sentence in sentences {
        for (start, end) in mustard_id::find(sentence) {
            push_unique(&mut codes, sentence[start..end].to_string());
        }
    }
    codes
}

// ---------------------------------------------------------------------------
// Facilidade de leitura
// ---------------------------------------------------------------------------

/// A nota de facilidade de leitura da prosa, pelo índice de Flesch adaptado ao
/// português (Martins et al., 1996): 248,835 − 1,015 × palavras por frase −
/// 84,6 × sílabas por palavra. Quanto maior, mais fácil. `None` quando a prosa
/// não está em português (a fórmula é do português) ou tem menos de
/// [`MIN_LANGUAGE_WORDS`] palavras.
fn reading_ease(lines: &[String], sentences: &[&str]) -> Option<i32> {
    let (total, pt, en) = language_counts(lines);
    if total < MIN_LANGUAGE_WORDS || dominant_language(pt, en) != Some(Locale::PtBr) {
        return None;
    }
    let (mut word_count, mut syllable_count) = (0_usize, 0_usize);
    for word in sentences.iter().flat_map(|sentence| words(sentence)) {
        let letters: String = word.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect();
        if !letters.is_empty() {
            word_count += 1;
            syllable_count += syllables_pt(&letters);
        }
    }
    if word_count == 0 || sentences.is_empty() {
        return None;
    }
    let per_sentence = word_count as f64 / sentences.len() as f64;
    let per_word = syllable_count as f64 / word_count as f64;
    Some((248.835 - 1.015 * per_sentence - 84.6 * per_word).round() as i32)
}

/// As sílabas de uma palavra em português, já em minúsculas. Cada grupo de
/// vogais é uma sílaba, e o grupo se parte quando uma vogal com acento agudo
/// ou circunflexo vem depois de outra ("sa-ú-de") ou quando duas vogais fortes
/// se encontram ("po-e-ta", "le-ão"); depois de "ã" e "õ" a vogal fecha o
/// ditongo ("ão", "õe"). O "u" de "que", "qui", "gue" e "gui" é mudo. É uma
/// aproximação: o hiato que só a pronúncia separa ("di-a") conta como uma.
fn syllables_pt(word: &str) -> usize {
    let chars: Vec<char> = word.chars().collect();
    let mut count = 0;
    let mut previous: Option<char> = None;
    for (at, &c) in chars.iter().enumerate() {
        let mute_u = c == 'u'
            && at > 0
            && matches!(chars[at - 1], 'q' | 'g')
            && chars.get(at + 1).is_some_and(|&next| is_vowel(next));
        if !is_vowel(c) || mute_u {
            previous = None;
            continue;
        }
        let starts = match previous {
            None => true,
            Some(before) => {
                !matches!(before, 'ã' | 'õ')
                    && (is_stressed(c) || (is_strong_vowel(before) && is_strong_vowel(c)))
            }
        };
        if starts {
            count += 1;
        }
        previous = Some(c);
    }
    count.max(1)
}

fn is_vowel(c: char) -> bool {
    "aeiouyáàâãéêíóôõúü".contains(c)
}

/// A, E e O, com ou sem acento: duas delas juntas são hiato.
fn is_strong_vowel(c: char) -> bool {
    "aeoáàâãéêóôõ".contains(c)
}

/// Vogal com acento agudo ou circunflexo: abre sílaba própria.
fn is_stressed(c: char) -> bool {
    "áéíóúâêô".contains(c)
}

// ---------------------------------------------------------------------------
// Idioma
// ---------------------------------------------------------------------------

/// Mede só o idioma de `text`: a medição que vale para todo projeto, qualquer
/// que seja o tom. `lang` é o idioma do projeto. `None` quando a prosa está
/// nele, é curta demais ou não tem idioma dominante.
#[must_use]
pub fn measure_language(text: &str, lang: Locale) -> Option<WrongLanguage> {
    wrong_language(&prose_lines(text), lang)
}

/// O idioma dominante da prosa, quando ele não é `expected`. `None` com menos
/// de [`MIN_LANGUAGE_WORDS`] palavras de texto corrido, sem idioma dominante ou
/// com a prosa no idioma certo. As linhas já vêm sem código, tabela nem JSON.
fn wrong_language(lines: &[String], expected: Locale) -> Option<WrongLanguage> {
    let (total, pt, en) = language_counts(lines);
    if total < MIN_LANGUAGE_WORDS {
        return None;
    }
    let found = dominant_language(pt, en)?;
    (found != expected).then_some(WrongLanguage { found, expected })
}

/// Quantas palavras a prosa tem, e quantas delas são palavras comuns do
/// português e do inglês: `(total, pt, en)`.
fn language_counts(lines: &[String]) -> (usize, usize, usize) {
    let (mut total, mut pt, mut en) = (0, 0, 0);
    for word in lines.iter().flat_map(|line| words(line)) {
        total += 1;
        let word = word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if COMMON_WORDS_PT.contains(&word.as_str()) {
            pt += 1;
        } else if COMMON_WORDS_EN.contains(&word.as_str()) {
            en += 1;
        }
    }
    (total, pt, en)
}

/// O idioma cujas palavras comuns somam ao menos [`MIN_LANGUAGE_MARKERS`] e
/// mais de [`LANGUAGE_DOMINANCE`] vezes as do outro. `None` num empate ou numa
/// mistura sem vencedor claro.
fn dominant_language(pt: usize, en: usize) -> Option<Locale> {
    let dominates = |mine: usize, other: usize| {
        mine >= MIN_LANGUAGE_MARKERS && mine > LANGUAGE_DOMINANCE * other
    };
    if dominates(pt, en) {
        Some(Locale::PtBr)
    } else if dominates(en, pt) {
        Some(Locale::EnUs)
    } else {
        None
    }
}

/// Frases com mais de [`MAX_SENTENCE_WORDS`] palavras, com o começo de cada uma.
fn long_sentences(sentences: &[&str]) -> Vec<LongSentence> {
    sentences
        .iter()
        .filter_map(|sentence| {
            let words = words(sentence).count();
            (words > MAX_SENTENCE_WORDS).then(|| LongSentence {
                words,
                opening: words_of(sentence, OPENING_WORDS),
            })
        })
        .collect()
}

/// Siglas sem expansão em nenhum ponto desta resposta nem antes na sessão.
/// As que ganharam expansão entram em `explained`.
fn unexpanded_acronyms(
    sentences: &[&str],
    already_explained: &[String],
    explained: &mut Vec<String>,
) -> Vec<String> {
    // Ordem de primeira aparição, e se alguma ocorrência trouxe a expansão.
    let mut seen: Vec<(String, bool)> = Vec::new();
    for sentence in sentences {
        for (start, end, acronym) in acronyms_in(sentence) {
            let skip = COMMON_ACRONYMS.contains(&acronym)
                || already_explained.iter().any(|done| done == acronym);
            if skip {
                continue;
            }
            let expanded_here = explained_at(sentence, start, end);
            match seen.iter_mut().find(|(known, _)| known == acronym) {
                Some((_, expanded)) => *expanded |= expanded_here,
                None => seen.push((acronym.to_string(), expanded_here)),
            }
        }
    }
    let mut missing = Vec::new();
    for (acronym, expanded) in seen {
        if expanded {
            push_unique(explained, acronym);
        } else {
            missing.push(acronym);
        }
    }
    missing
}

/// Acrescenta `item` a `list` só se ainda não estiver lá.
fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

// ---------------------------------------------------------------------------
// Explicação de um termo ou sigla
// ---------------------------------------------------------------------------

/// A ocorrência em `sentence[start..end]` vem acompanhada da explicação?
///
/// A explicação precisa vir LOGO DEPOIS do termo. Conta como explicação:
/// - um parêntese logo depois com texto em minúsculas ("CI (integração
///   contínua)");
/// - o termo sozinho num parêntese depois das palavras que ele resume
///   ("integração contínua (CI)");
/// - o termo seguido direto de travessão ou hífen ([`APPOSITION_MARKS`]), ou
///   de dois-pontos quando o termo abre a frase ([`LABEL_MARK`]);
/// - "que é", "ou seja" e afins nas [`PHRASE_WINDOW`] palavras seguintes.
///
/// Dois-pontos, travessão ou "que é" mais adiante na frase não contam: em "O
/// CI falhou: veja o log" o dois-pontos explica a falha, não o CI.
fn explained_at(sentence: &str, start: usize, end: usize) -> bool {
    let is_decoration =
        |c: char| c.is_whitespace() || matches!(c, '*' | '_' | '"' | '\'' | '“' | '”' | '«' | '»');
    let after = &sentence[end..];
    // O `s` do plural ("slugs") ainda é o termo.
    let after = after
        .strip_prefix('s')
        .filter(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
        .unwrap_or(after);
    let next = after.trim_start_matches(is_decoration);

    if let Some(inner) = next.strip_prefix('(') {
        let inner = inner.split(')').next().unwrap_or_default();
        if inner.chars().any(char::is_lowercase) {
            return true;
        }
    }

    let before = sentence[..start].trim_end_matches(is_decoration);
    if next.starts_with(')')
        && let Some(words_before) = before.strip_suffix('(')
            && words_before.chars().any(char::is_alphabetic) {
                return true;
            }

    let glued = after.trim_start_matches(TERM_CLOSERS);
    if APPOSITION_MARKS.iter().any(|mark| glued.starts_with(mark)) {
        return true;
    }
    let opens_sentence = !before.chars().any(char::is_alphanumeric);
    if opens_sentence && glued.starts_with(LABEL_MARK) {
        return true;
    }

    let following: Vec<String> = after
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|word| !word.is_empty())
        .take(PHRASE_WINDOW)
        .collect();
    EXPLAINING_PHRASES.iter().any(|phrase| {
        let wanted: Vec<&str> = phrase.split_whitespace().collect();
        following.len() >= wanted.len() && following.iter().zip(&wanted).all(|(got, want)| got == want)
    })
}

// ---------------------------------------------------------------------------
// Siglas
// ---------------------------------------------------------------------------

/// Siglas de `sentence`: palavras de 2 a 6 letras maiúsculas, com um `s` de
/// plural opcional ("PRs"). Devolve o trecho da palavra inteira e a sigla.
/// `_` conta como parte da palavra, então `MUSTARD_WORKSPACE_ROOT` não vira
/// três siglas — e `pt_BR` também não vira sigla nenhuma.
/// Fica de fora o que só parece sigla: ver [`mimics_acronym`].
fn acronyms_in(sentence: &str) -> Vec<(usize, usize, &str)> {
    let runs = word_runs(sentence);
    runs.iter()
        .enumerate()
        .filter_map(|(idx, &(start, end))| {
            let run = &sentence[start..end];
            let core = run.strip_suffix('s').filter(|c| is_acronym(c)).unwrap_or(run);
            let counts = is_acronym(core) && !mimics_acronym(sentence, &runs, idx, core);
            counts.then_some((start, end, core))
        })
        .collect()
}

/// O trecho `(início, fim)` de cada palavra de `sentence`, na ordem: letras,
/// dígitos e `_` seguidos.
fn word_runs(sentence: &str) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut run_start: Option<usize> = None;
    let ends = std::iter::once((sentence.len(), ' '));
    for (at, ch) in sentence.char_indices().chain(ends) {
        let in_word = ch.is_alphanumeric() || ch == '_';
        match (in_word, run_start) {
            (true, None) => run_start = Some(at),
            (false, Some(start)) => {
                run_start = None;
                runs.push((start, at));
            }
            _ => {}
        }
    }
    runs
}

/// De 2 a 6 letras, todas maiúsculas sem acento.
fn is_acronym(word: &str) -> bool {
    (2..=6).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_uppercase())
}

/// A palavra `runs[idx]`, com cara de sigla (`core`), é outra coisa escrita em
/// maiúsculas:
/// - ênfase: faz parte de uma sequência de palavras em maiúsculas ("IN THIS
///   CONVERSATION") ou é uma palavra comprida com vogais ("NUNCA", "RESUMO");
/// - a região de um código de idioma ("pt-BR", "en-US");
/// - um numeral romano ("Fase II", "onda IV");
/// - as letras de um rótulo com hífen e número ("AC" em "AC-5");
/// - uma parte de um código do Mustard ("MSTD" e "RULE" em `MSTD-RULE-0005`),
///   que a medição dos códigos já aponta inteiro.
///
/// Limite aceito: ênfase curta e com poucas vogais, sozinha ("MUST"), continua
/// contando como sigla — não há como separá-la de "SSH" só pela forma.
fn mimics_acronym(sentence: &str, runs: &[(usize, usize)], idx: usize, core: &str) -> bool {
    is_roman_numeral(core)
        || is_shouted_word(core)
        || in_shouted_sequence(sentence, runs, idx)
        || is_locale_region(sentence, runs[idx].0, core)
        || opens_internal_code(sentence, runs[idx].1)
        || inside_mustard_id(sentence, runs[idx])
}

/// A palavra `(início, fim)` está dentro de um código do Mustard.
fn inside_mustard_id(sentence: &str, (start, end): (usize, usize)) -> bool {
    mustard_id::find(sentence).iter().any(|&(s, e)| s <= start && end <= e)
}

/// A palavra que termina em `end` é seguida de hífen e número: são as letras
/// de um rótulo ("AC-5"), não uma sigla.
fn opens_internal_code(sentence: &str, end: usize) -> bool {
    sentence[end..].strip_prefix('-').is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// Só I, V e X: "II", "IV", "IX". O I sozinho nem chega a ser candidato.
fn is_roman_numeral(core: &str) -> bool {
    core.bytes().all(|b| matches!(b, b'I' | b'V' | b'X'))
}

/// Cinco letras ou mais e ao menos duas vogais: palavra em maiúsculas por
/// ênfase ("NUNCA", "SEMPRE"). Sigla de verdade raramente tem tantas vogais.
fn is_shouted_word(core: &str) -> bool {
    let vowels = core.bytes().filter(|b| b"AEIOU".contains(b)).count();
    core.len() >= 5 && vowels >= 2
}

/// A palavra está em maiúsculas e encosta, separada só por espaço, em outra
/// palavra em maiúsculas: a frase inteira está gritando ("NÃO USE ISSO").
/// Vírgula quebra a sequência, então "CI, QA e SSH" continua sendo três siglas.
fn in_shouted_sequence(sentence: &str, runs: &[(usize, usize)], idx: usize) -> bool {
    let shouted = |&(start, end): &(usize, usize)| {
        let word = &sentence[start..end];
        // duas letras no mínimo: o artigo "O" ou "A" no começo da frase não
        // transforma "O CI" em ênfase
        word.chars().count() >= 2 && word.chars().all(|c| c.is_alphabetic() && c.is_uppercase())
    };
    let joined = |left: &(usize, usize), right: &(usize, usize)| {
        shouted(left)
            && shouted(right)
            && sentence[left.1..right.0].chars().all(char::is_whitespace)
    };
    let current = &runs[idx];
    let with_previous = idx.checked_sub(1).is_some_and(|prev| joined(&runs[prev], current));
    let with_next = runs.get(idx + 1).is_some_and(|next| joined(current, next));
    with_previous || with_next
}

/// A sigla é a região de um código de idioma `xx-XX`: duas letras maiúsculas
/// depois de hífen e de duas minúsculas que começam a palavra ("pt-BR"). A
/// forma `xx_XX` já é uma palavra só, por causa do `_`.
fn is_locale_region(sentence: &str, start: usize, core: &str) -> bool {
    if core.len() != 2 {
        return false;
    }
    let Some(before) = sentence[..start].strip_suffix('-') else {
        return false;
    };
    let language = before.trim_end_matches(|c: char| c.is_ascii_lowercase());
    let joins = language.chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
    before.len() - language.len() == 2 && !joins
}

// ---------------------------------------------------------------------------
// Texto corrido: o que sobra depois de tirar código, tabela, JSON e caminhos
// ---------------------------------------------------------------------------

/// As linhas de texto corrido da resposta, já limpas de código inline, alvos
/// de link, URLs e caminhos. Linhas vazias, blocos de código, tabelas, JSON e
/// linhas que ficaram sem palavra não entram.
fn prose_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut fence: Option<&str> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(marker) = fence {
            if line.starts_with(marker) {
                fence = None;
            }
            continue;
        }
        if let Some(marker) = fence_marker(line) {
            fence = Some(marker);
            continue;
        }
        if line.is_empty() || is_table_line(line) || is_json_line(line) {
            continue;
        }
        let cleaned = clean_inline(strip_line_marker(line));
        if words(&cleaned).next().is_some() {
            lines.push(cleaned);
        }
    }
    lines
}

/// A cerca que abre um bloco de código (três ou mais crases ou tis).
fn fence_marker(line: &str) -> Option<&str> {
    let fence_char = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = line.len() - line.trim_start_matches(fence_char).len();
    (len >= 3).then(|| &line[..len])
}

/// Linha de tabela markdown: começa com `|` ou tem duas barras fora de código.
fn is_table_line(line: &str) -> bool {
    line.starts_with('|') || strip_inline_code(line).matches('|').count() >= 2
}

/// Linha de JSON solto: abre ou fecha objeto/lista, ou é um par `"chave":`.
fn is_json_line(line: &str) -> bool {
    let mut chars = line.chars();
    match chars.next() {
        Some('{' | '}' | ']') => true,
        Some('[') => chars
            .find(|c| !c.is_whitespace())
            .is_none_or(|c| matches!(c, '{' | '[' | ']' | '"') || c.is_ascii_digit()),
        Some('"') => line.contains("\":"),
        _ => false,
    }
}

/// Tira a marca de citação, título ou item de lista do começo da linha.
fn strip_line_marker(line: &str) -> &str {
    let line = line.trim_start_matches(|c: char| c == '>' || c.is_whitespace());
    if let Some(rest) = line.strip_prefix('#') {
        let rest = rest.trim_start_matches('#');
        if rest.is_empty() || rest.starts_with(' ') {
            return rest.trim();
        }
    }
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return rest.trim();
        }
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim();
        }
    }
    line
}

/// Tira código inline, alvos de link, URLs e caminhos de uma linha de texto.
/// De um caminho descartado sobra só a pontuação final, que ainda marca o fim
/// da frase.
fn clean_inline(line: &str) -> String {
    let without_code = strip_inline_code(line);
    let without_targets = strip_link_targets(&without_code);
    let mut kept: Vec<&str> = Vec::new();
    for token in without_targets.split_whitespace() {
        let lead = token.len() - token.trim_start_matches(LEADERS).len();
        let core = token[lead..].trim_end_matches(TRAILERS);
        if !core.is_empty() && is_url_or_path(core) {
            let trail = &token[lead + core.len()..];
            if !trail.is_empty() {
                kept.push(trail);
            }
        } else {
            kept.push(token);
        }
    }
    kept.join(" ")
}

/// Troca cada trecho de código inline por um espaço. Crase sem par fica como
/// texto.
fn strip_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        out.push_str(&rest[..open]);
        let from_open = &rest[open..];
        let run = from_open.len() - from_open.trim_start_matches('`').len();
        let body = &from_open[run..];
        match body.find(&from_open[..run]) {
            Some(close) => {
                out.push(' ');
                rest = &body[close + run..];
            }
            None => {
                out.push_str(from_open);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Tira o alvo `(...)` de cada link markdown `[texto](alvo)`; o texto fica.
fn strip_link_targets(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("](") {
        out.push_str(&rest[..=at]);
        let target = &rest[at + 2..];
        rest = target.find(')').map_or("", |close| &target[close + 1..]);
    }
    out.push_str(rest);
    out
}

/// URL, tag, caminho de arquivo ou nome de arquivo com extensão.
fn is_url_or_path(core: &str) -> bool {
    let lower = core.to_ascii_lowercase();
    if lower.contains("://") || lower.starts_with("www.") || lower.starts_with("mailto:") {
        return true;
    }
    if core.starts_with('<') {
        return true;
    }
    if core.contains(['/', '\\']) {
        // "e/ou" e "CI/CD" continuam texto; caminho tem raiz, duas barras ou
        // extensão no último trecho.
        let separators = core.matches(['/', '\\']).count();
        let last = core.rsplit(['/', '\\']).next().unwrap_or_default();
        return core.starts_with(['/', '.', '~']) || separators >= 2 || last.contains('.');
    }
    has_file_extension(core)
}

/// `nome.ext`, com extensão curta e ao menos uma minúscula ("mod.rs",
/// "CLAUDE.md"); "3.5" continua número.
fn has_file_extension(core: &str) -> bool {
    core.rsplit_once('.').is_some_and(|(name, ext)| {
        name.chars().any(char::is_alphanumeric)
            && (1..=5).contains(&ext.len())
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
            && ext.chars().any(|c| c.is_ascii_lowercase())
    })
}

// ---------------------------------------------------------------------------
// Frases e palavras
// ---------------------------------------------------------------------------

/// Parte uma linha em frases: termina em `.`, `!`, `?` ou `…` seguido de
/// espaço ou do fim da linha ("3.5" não parte). Frase sem palavra é descartada.
fn split_sentences(line: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (at, ch) in line.char_indices() {
        if at < start || !matches!(ch, '.' | '!' | '?' | '…') {
            continue;
        }
        let closed = line[at + ch.len_utf8()..].trim_start_matches(CLOSERS);
        if closed.is_empty() || closed.starts_with(char::is_whitespace) {
            let end = line.len() - closed.len();
            push_sentence(&mut sentences, &line[start..end]);
            start = end;
        }
    }
    push_sentence(&mut sentences, &line[start..]);
    sentences
}

fn push_sentence<'a>(sentences: &mut Vec<&'a str>, candidate: &'a str) {
    let candidate = candidate.trim();
    if words(candidate).next().is_some() {
        sentences.push(candidate);
    }
}

/// As palavras de um trecho: pedaços entre espaços com ao menos uma letra ou
/// dígito (um travessão solto não é palavra).
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace().filter(|token| token.chars().any(char::is_alphanumeric))
}

/// As primeiras `n` palavras de um trecho, separadas por um espaço.
fn words_of(text: &str, n: usize) -> String {
    words(text).take(n).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    /// A frase acima do limite sai com a contagem e o começo dela; a
    /// frase curta e o item de lista curto não saem.
    #[test]
    fn clarity_flags_long_sentences() {
        let long = "Esta frase foi escrita de propósito para passar do limite de palavras \
                    que a regra de tom aceita numa única frase da resposta e por isso \
                    precisa aparecer.";
        let text = format!("Uma frase curta. {long}\n- item curto\n- outro item curto");
        let report = measure(&text, &[], Some(Locale::PtBr));

        assert_eq!(report.long_sentences.len(), 1, "{report:?}");
        assert_eq!(report.long_sentences[0].words, 28);
        assert_eq!(
            report.long_sentences[0].opening,
            "Esta frase foi escrita de propósito para passar"
        );
        assert!(!report.passed);
        assert_eq!(
            report.defects(Locale::PtBr),
            vec!["frase com 28 palavras: \"Esta frase foi escrita de propósito para passar…\""]
        );

        // Exatamente no limite ainda passa; o item de lista é frase própria.
        let at_limit = vec!["palavra"; MAX_SENTENCE_WORDS].join(" ");
        let item = format!("- {}", vec!["item"; MAX_SENTENCE_WORDS + 1].join(" "));
        assert!(measure(&at_limit, &[], Some(Locale::PtBr)).passed);
        assert_eq!(measure(&item, &[], Some(Locale::PtBr)).long_sentences[0].words, MAX_SENTENCE_WORDS + 1);
    }

    /// Sigla sem as palavras por extenso é apontada; com a expansão entre
    /// parênteses (antes ou depois), já expandida na sessão ou de uso comum,
    /// não é.
    #[test]
    fn clarity_flags_unexpanded_acronym() {
        let bare = measure("O CI falhou de novo.", &[], Some(Locale::PtBr));
        assert_eq!(bare.unexpanded_acronyms, vec!["CI"]);
        assert!(!bare.passed);
        assert_eq!(bare.defects(Locale::PtBr), vec!["CI sem as palavras por extenso"]);

        let expanded = measure("O CI (integração contínua) falhou de novo.", &[], Some(Locale::PtBr));
        assert!(expanded.unexpanded_acronyms.is_empty(), "{expanded:?}");
        assert_eq!(expanded.explained, vec!["CI"]);

        let reverse = measure("A integração contínua (CI) falhou.", &[], Some(Locale::PtBr));
        assert!(reverse.unexpanded_acronyms.is_empty(), "{reverse:?}");

        let later = measure("O CI falhou.", &terms(&["CI"]), Some(Locale::PtBr));
        assert!(later.unexpanded_acronyms.is_empty());

        let common = measure("Abri o PR e os PRs com JSON e HTML.", &[], Some(Locale::PtBr));
        assert!(common.unexpanded_acronyms.is_empty(), "{common:?}");
    }

    /// A explicação só conta quando vem logo depois da sigla.
    /// Dois-pontos, travessão ou "que é" mais adiante na frase falam de outra
    /// coisa, e a sigla continua sem explicação.
    #[test]
    fn clarity_explanation_must_follow_the_term() {
        // Mais adiante na frase: aponta.
        for text in [
            "O CI falhou: veja o log.",
            "O CI falhou, e o que é pior, parou tudo.",
            "O CI falhou, e o que e pior, parou tudo.",
            "O CI falhou — veja o log.",
        ] {
            let report = measure(text, &[], Some(Locale::PtBr));
            assert_eq!(report.unexpanded_acronyms, vec!["CI"], "{text}: {report:?}");
            assert!(report.explained.is_empty(), "{text}: {report:?}");
        }

        // Logo depois da sigla: não aponta.
        for text in [
            "CI: integracao continua, falhou.",
            "CI: integração contínua, falhou.",
            "**CI**: integração contínua, falhou.",
            "O CI — integração contínua — falhou.",
            "O **CI** — integração contínua — falhou.",
            "O CI - integração contínua - falhou.",
            "O CI, ou seja, a integração contínua, falhou.",
            "O CI, que é a integração contínua, falhou.",
            "O CI (integração contínua) falhou.",
            "A integração contínua (CI) falhou.",
        ] {
            let report = measure(text, &[], Some(Locale::PtBr));
            assert!(report.unexpanded_acronyms.is_empty(), "{text}: {report:?}");
            assert_eq!(report.explained, vec!["CI"], "{text}");
        }
    }

    /// Blocos de código, código inline, caminhos, links, URLs, tabelas e
    /// JSON não contam como frase nem sigla.
    #[test]
    fn clarity_ignores_code_and_paths() {
        let text = r#"Resumo curto.

```rust
fn slug_for(API: &str) -> String { todo!() } // uma linha de codigo bem comprida que passaria do limite de palavras se fosse texto comum de uma resposta
```

Rode `cargo test -p mustard-core SQL slug` e veja packages/core/src/domain/clarity/mod.rs e CLAUDE.md.
Detalhes em [a página](https://example.com/CI/slug?x=1) e em https://docs.rs/XYZ.

| Coluna | CI | slug |
|--------|----|------|
| uma linha de tabela com muitas palavras que nunca deve contar como frase longa nem como sigla | ABC | slug |

{"key": "SLUG", "ABC": 1}
"#;
        let report = measure(text, &[], Some(Locale::PtBr));
        assert!(report.long_sentences.is_empty(), "{report:?}");
        assert!(report.unexpanded_acronyms.is_empty(), "{report:?}");
        assert_eq!(report.prose_lines, 3);
        assert!(report.passed);
    }

    /// Mais de quinze linhas reprovam a resposta pelo tamanho, e o tamanho
    /// conta a resposta inteira: linhas de código também são lidas.
    #[test]
    fn clarity_reply_over_fifteen_lines_is_too_long() {
        let fits = vec!["Uma linha curta."; MAX_LINES].join("\n\n");
        assert!(measure(&fits, &[], Some(Locale::PtBr)).passed, "blank lines do not count");

        let over = vec!["Uma linha curta."; MAX_LINES + 1].join("\n");
        let report = measure(&over, &[], Some(Locale::PtBr));
        assert!(report.too_long && !report.passed);
        assert_eq!(report.defects(Locale::EnUs), vec!["reply with 16 lines; the limit is 15"]);
        assert_eq!(report.defects(Locale::PtBr), vec!["resposta com 16 linhas; o limite é 15"]);

        let code = format!("Rode isto:\n```text\n{}\n```", vec!["linha"; MAX_LINES].join("\n"));
        let report = measure(&code, &[], Some(Locale::PtBr));
        assert_eq!((report.prose_lines, report.lines), (1, MAX_LINES + 3), "{report:?}");
        assert!(report.too_long, "{report:?}");
    }

    /// Código do Mustard no texto corrido é apontado, cada um uma vez, e pede
    /// o nome do assunto; as partes dele não contam como sigla. Código entre
    /// crases, letra com número fora do formato, versão e palavra comum não
    /// são apontados.
    #[test]
    fn clarity_flags_internal_codes() {
        let report = measure(
            "A regra MSTD-RULE-0008 vale. Veja o MSTD-CRIT-0015 e de novo (MSTD-RULE-0008).",
            &[],
            Some(Locale::PtBr),
        );
        assert_eq!(report.internal_codes, vec!["MSTD-RULE-0008", "MSTD-CRIT-0015"], "{report:?}");
        assert!(report.unexpanded_acronyms.is_empty(), "MSTD and RULE are the code, not acronyms: {report:?}");
        assert!(!report.passed);
        assert_eq!(
            report.defects(Locale::PtBr)[0],
            "MSTD-RULE-0008 é um código interno; diga o assunto pelo nome"
        );
        assert_eq!(report.defects(Locale::EnUs)[0], "MSTD-RULE-0008 is an internal code; name the subject instead");

        for clean in [
            "Rode `MSTD-RULE-0008` no terminal.",
            "Guardei o arquivo no R2 da Cloudflare, no S3 e numa folha A4.",
            "A regra R8 vale, e o C-15 e o AC-5 também.",
            "Instalei a versão v0.3 e o UTF-8 num x86.",
            "O teste E2E passou em 2026.",
            "A regra ficou pronta.",
        ] {
            let report = measure(clean, &[], Some(Locale::PtBr));
            assert!(report.internal_codes.is_empty(), "{clean}: {report:?}");
            assert!(report.unexpanded_acronyms.is_empty(), "{clean}: {report:?}");
        }
        let plain = measure("Guardei o arquivo no R2 da Cloudflare, no S3 e numa folha A4.", &[], Some(Locale::PtBr));
        assert!(plain.passed, "{plain:?}");
    }

    /// As sílabas em português seguem a separação escolar nos casos comuns:
    /// ditongo nasal, hiato com acento, duas vogais fortes e o "u" mudo.
    #[test]
    fn portuguese_syllables_follow_the_common_cases() {
        for (word, want) in [
            ("configuração", 5),
            ("saúde", 3),
            ("leão", 2),
            ("poeta", 3),
            ("queijo", 2),
            ("água", 2),
            ("coordenação", 5),
            ("automação", 4),
            ("é", 1),
            ("psst", 1),
        ] {
            assert_eq!(syllables_pt(word), want, "{word}");
        }
    }

    /// A nota de Flesch só sai para prosa em português com palavras bastantes;
    /// frases curtas de palavras curtas passam, e frases compridas de palavras
    /// compridas reprovam pela nota.
    #[test]
    fn clarity_scores_reading_ease_in_portuguese() {
        let plain = measure(PORTUGUESE_REPLY, &[], Some(Locale::PtBr));
        let score = plain.reading_ease.unwrap_or_else(|| panic!("Portuguese prose is scored: {plain:?}"));
        assert!(score >= MIN_READING_EASE && !plain.hard_to_read && plain.passed, "{plain:?}");

        let dense = "A implementação da configuração automatizada da infraestrutura \
                     organizacional exige documentação complementar significativamente \
                     detalhada. A coordenação interdepartamental das especificações \
                     técnicas necessárias demanda comunicação institucionalizada e \
                     planejamento estratégico permanentemente atualizado. A parametrização \
                     das integrações corporativas depende da homologação das funcionalidades \
                     disponibilizadas pela arquitetura.";
        let report = measure(dense, &[], Some(Locale::PtBr));
        let score = report.reading_ease.unwrap_or_else(|| panic!("dense prose is scored: {report:?}"));
        assert!(score < MIN_READING_EASE && report.hard_to_read && !report.passed, "{report:?}");
        let defect = report.defects(Locale::PtBr).pop().unwrap_or_default();
        assert_eq!(
            defect,
            format!(
                "texto difícil de ler: nota {score} no índice de Flesch, e o mínimo é 25; use \
                 frases e palavras mais curtas"
            )
        );

        // Inglês e prosa curta não recebem nota.
        assert_eq!(measure(ENGLISH_REPLY, &[], Some(Locale::EnUs)).reading_ease, None);
        assert_eq!(measure("Frase curta.", &[], Some(Locale::PtBr)).reading_ease, None);
    }

    /// Ênfase em maiúsculas não é sigla: nem a palavra comprida com vogais,
    /// nem a sequência de palavras gritadas. Sigla de verdade continua
    /// apontada, e a ênfase curta sozinha ("MUST") é o limite aceito.
    #[test]
    fn clarity_ignores_caps_emphasis() {
        for text in [
            "NUNCA rode isso sem ler o RESUMO.",
            "SEMPRE confira antes de seguir.",
            "Use only what was said IN THIS CONVERSATION.",
            "**NÃO USE ISSO** em produção.",
        ] {
            let report = measure(text, &[], Some(Locale::PtBr));
            assert!(report.unexpanded_acronyms.is_empty(), "{text}: {report:?}");
        }

        let real = measure("NUNCA pule o CI. O QA e o SSH falharam.", &[], Some(Locale::PtBr));
        assert_eq!(real.unexpanded_acronyms, vec!["CI", "QA", "SSH"], "{real:?}");
        // Vírgula quebra a sequência: siglas enfileiradas não viram ênfase.
        let listed = measure("Falharam CI, QA, SSH.", &[], Some(Locale::PtBr));
        assert_eq!(listed.unexpanded_acronyms, vec!["CI", "QA", "SSH"], "{listed:?}");

        let short = measure("You MUST read it.", &[], Some(Locale::PtBr));
        assert_eq!(short.unexpanded_acronyms, vec!["MUST"], "{short:?}");
    }

    /// Código de idioma e numeral romano não são sigla; a sigla colada por
    /// hífen a outra coisa que não um idioma continua apontada.
    #[test]
    fn clarity_ignores_locale_codes_and_roman_numerals() {
        for text in [
            "Escrevo em pt-BR, en-US e pt_BR.",
            "A Fase II e a onda IV terminaram, e o capítulo IX também.",
            "O item III vem antes do XI.",
        ] {
            let report = measure(text, &[], Some(Locale::PtBr));
            assert!(report.unexpanded_acronyms.is_empty(), "{text}: {report:?}");
        }

        let near_locale = measure("O CI roda em pt-BR.", &[], Some(Locale::PtBr));
        assert_eq!(near_locale.unexpanded_acronyms, vec!["CI"], "{near_locale:?}");
        let hyphen = measure("O CI-QA e o anti-SSH falharam.", &[], Some(Locale::PtBr));
        assert_eq!(hyphen.unexpanded_acronyms, vec!["CI", "QA", "SSH"], "{hyphen:?}");
    }

    /// Prosa em inglês: 39 palavras, frases curtas, sem sigla.
    const ENGLISH_REPLY: &str = "The wave is done and the tests pass.\n\
        The check now compares the language of the reply with the language of the project.\n\
        It counts the common words of each language.\n\
        A short reply is not judged at all.";

    /// Prosa em português: 35 palavras, frases curtas, sem sigla.
    const PORTUGUESE_REPLY: &str = "A onda terminou e os testes passaram.\n\
        A medição agora compara o idioma da resposta com o idioma do projeto.\n\
        Ela conta as palavras comuns de cada idioma.\n\
        Uma resposta curta não é julgada por ela.";

    /// A resposta em inglês num projeto em português aponta o idioma
    /// errado, com o defeito tirado do catálogo. No idioma certo, a mesma
    /// resposta passa.
    #[test]
    fn clarity_flags_a_reply_in_another_language() {
        let wrong = measure(ENGLISH_REPLY, &[], Some(Locale::PtBr));
        assert_eq!(
            wrong.wrong_language,
            Some(WrongLanguage { found: Locale::EnUs, expected: Locale::PtBr }),
            "{wrong:?}"
        );
        assert!(!wrong.passed);
        assert_eq!(
            wrong.defects(Locale::PtBr),
            vec!["resposta em en-US; o idioma do projeto e do usuário é pt-BR"]
        );

        let right = measure(ENGLISH_REPLY, &[], Some(Locale::EnUs));
        assert_eq!(right.wrong_language, None, "{right:?}");
        assert!(right.passed, "{right:?}");

        // O inverso também: português num projeto em inglês.
        let wrong = measure(PORTUGUESE_REPLY, &[], Some(Locale::EnUs));
        assert_eq!(
            wrong.wrong_language,
            Some(WrongLanguage { found: Locale::PtBr, expected: Locale::EnUs }),
            "{wrong:?}"
        );
        assert!(measure(PORTUGUESE_REPLY, &[], Some(Locale::PtBr)).passed);
    }

    /// Resposta curta, prosa só dentro de código e mistura sem vencedor claro
    /// não são julgadas pelo idioma.
    #[test]
    fn clarity_leaves_short_or_code_only_replies_unjudged() {
        let short = "The wave is done and the tests pass.";
        assert_eq!(measure(short, &[], Some(Locale::PtBr)).wrong_language, None);

        // O inglês mora no bloco de código e no código inline; a prosa em
        // volta é curta.
        let code_only = format!(
            "Rode isto:\n\n```text\n{ENGLISH_REPLY}\n```\n\nE depois `{}`.",
            ENGLISH_REPLY.replace('\n', " ")
        );
        let report = measure(&code_only, &[], Some(Locale::PtBr));
        assert_eq!(report.wrong_language, None, "{report:?}");

        // Uma resposta em português que cita uma frase em inglês continua em
        // português.
        let quoting = format!("{PORTUGUESE_REPLY}\nO aviso dizia: the check is done.");
        let report = measure(&quoting, &[], Some(Locale::PtBr));
        assert_eq!(report.wrong_language, None, "{report:?}");

        // Trinta palavras sem palavra comum não dizem idioma algum.
        let nouns = vec!["palavraextraordinariamentecomprida"; MIN_LANGUAGE_WORDS * 2].join(" ");
        assert_eq!(measure(&nouns, &[], Some(Locale::EnUs)).wrong_language, None);

        // Sem idioma declarado no projeto não há veredito de idioma.
        assert_eq!(measure(ENGLISH_REPLY, &[], None).wrong_language, None);
    }
}
