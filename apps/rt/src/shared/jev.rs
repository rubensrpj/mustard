//! `jev` — o filtro da busca por assunto do mapa pelo Jev, um serviço pago
//! de fora que não escreve texto: lê um estado e uma pergunta de escolha e
//! devolve a chance de cada opção.
//!
//! Implementa a tomada [`MapFilter`] do núcleo. Todos os candidatos vão num
//! pedido só, na ordem do banco, como uma pergunta de escolha: o estado traz
//! a frase, as palavras e um candidato por linha, e as opções são os ids dos
//! candidatos mais `none`, "nenhum destes". Só um pedido acima de
//! [`MAX_REQUEST_TOKENS`] se divide, no menor número de pedidos que mantém a
//! ordem do banco. A chance de cada id, a de `none` e a confiança da escolha
//! voltam para o veredito e o corte do núcleo ([`judged`]), os mesmos de toda
//! implementação.
//!
//! Quando o veredito é dividido, uma segunda olhada relê só os três de maior
//! chance, agora com os mesmos campos numa lista de três: uma escolha entre
//! eles e `none`, e um sim ou não para cada um ("este é o código que se
//! procura?"). A decisão é a do núcleo ([`second_look`]): volta o trecho certo,
//! ou não achei. O segundo pedido usa o mesmo prazo do primeiro; falhando ou
//! passando do prazo, vale o corte relativo da primeira etapa, sem erro.
//!
//! De cada candidato vão só nomes, caminho, assinatura, documentação,
//! comentários, o dono, os membros e títulos de commit, com os cortes medidos
//! no laboratório. Nunca uma linha do corpo nem um texto entre aspas: o
//! candidato do núcleo nem tem onde guardá-los. Todo texto que sai, o pedido
//! e as palavras inclusive, passa antes pela procura de segredo, e o trecho
//! com cara de chave, senha ou token vai como "…".
//!
//! O modelo pedido é uma versão fixa, e o uso guarda o nome do modelo que a
//! resposta diz ter respondido.
//!
//! A chave vem de [`KEY_ENV`] no ambiente ou, sem ela, de `jev.key` no
//! `mustard.json` do projeto ([`load_key`]). O git não pode guardar esse
//! arquivo: guardado, a chave dele não se usa, e a busca avisa. Nenhum
//! comando a grava, e nenhum erro, aviso ou log a leva — nem o corpo da
//! resposta do serviço.

use std::fmt;
use std::io::{self, ErrorKind};
use std::ops::Range;
use std::path::Path;
use std::time::{Duration, Instant};

use mustard_core::domain::map_filter::{
    Finalist, FilterCandidate, FilterError, FilterRequest, FilterUsage, Filtered, MapFilter, Scored, Verdict,
    finalists_of, judged, second_look, verdict,
};
use mustard_core::domain::normalize::split_identifier;
use mustard_core::ProjectConfig;
use serde::Serialize;
use serde_json::ser::Formatter;
use serde_json::{Map, Value, json};

use crate::shared::secret::without_secrets;

// ---------------------------------------------------------------------------
// O serviço
// ---------------------------------------------------------------------------

/// O endereço do serviço.
pub const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";

/// O modelo pedido: uma versão fixa, a que o laboratório mediu. A versão
/// mais nova do serviço mudaria as notas sem aviso; a troca vem com medida
/// nova.
pub const JEV_MODEL: &str = "jev-1.13.0";

/// US$ por milhão de tokens de entrada; a saída não é cobrada.
pub const PRICE_PER_MILLION_INPUT_TOKENS: f64 = 0.042;

/// A variável de ambiente da chave; vence o `mustard.json`.
pub const KEY_ENV: &str = "TYPESAFE_API_KEY";

/// O maior pedido que o serviço aceita, em tokens. Os 100 candidatos do banco
/// cabem num pedido só, de uns 20 mil.
const MAX_REQUEST_TOKENS: u64 = 64_000;

/// Caracteres por token, para estimar o pedido antes de mandar, como o
/// laboratório estimava.
const CHARS_PER_TOKEN: f64 = 3.2;

// A rede de uma busca interativa: quem espera é o agente, no meio do
// trabalho. O laboratório esperava a resposta até 120 s e tentava até 8
// vezes, o que serve a uma medição em lote, não a uma busca.

/// Quanto se espera para abrir a conexão.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Quanto a busca espera o filtro inteiro, repetições e esperas inclusive;
/// passado o prazo, a resposta vem do banco. O serviço saudável responde a
/// escolha entre 100 candidatos num pedido só em 0,5 s em média e em no
/// máximo 1,0 s (as 120 buscas do laboratório). Num período lento do
/// serviço, 100 buscas esperaram de 0,4 a 30 s, sem degrau no meio: com 10 s,
/// 9 delas iriam ao banco, e nenhuma esperaria mais que isso.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Quantas vezes se repete um pedido recusado por excesso (429, 529) ou por
/// falha do serviço (5xx). A chave recusada (401) e a falta de crédito (402)
/// não se repetem.
const MAX_RETRIES: u32 = 2;

/// A espera antes de repetir, quando o serviço não diz quanto.
const DEFAULT_RETRY_WAIT: Duration = Duration::from_millis(500);

/// A maior espera antes de repetir, mesmo que o serviço peça mais.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Os textos do pedido, como o filtro medido os mandava
// ---------------------------------------------------------------------------

// Cada campo que `candidate_fields` põe no candidato, pela chave, entre
// crases, na ordem dele: o teste confere os dois lados.
const ABOUT: &str = "A coding agent is searching a codebase. `request` is what it asked for, in Portuguese or English: either a \
description of what some code does, or the name, or part of the name, of an identifier. The state lists \
candidate declarations, one per line with what the code index knows about it: `kind`, `name` (its name split into words), \
`path` (its file path), `signature`, `documentation`, `last commits of the file` (the titles of the last commits \
that changed its file), `owner` (the names of the type or block that holds it), `comments in the body` (the \
comments inside its code) and `members` (the members it declares, when it is a type). The body of the code is \
not shown.";

/// O texto do estado da segunda olhada: o de `ABOUT`, para a lista curta dos
/// finalistas. Cita os mesmos campos, na mesma ordem; `@COUNT@` vira quantos
/// são.
const ABOUT_FINALISTS: &str = "A coding agent is searching a codebase. `request` is what it asked for, in Portuguese or English: either a \
description of what some code does, or the name, or part of the name, of an identifier. The state lists @COUNT@ \
candidate declarations, one per line, with everything the code index knows about each one: `kind`, `name` (its name split into words), \
`path` (its file path), `signature`, `documentation`, `last commits of the file` (the titles of the last commits \
that changed its file), `owner` (the names of the type or block that holds it), `comments in the body` (the \
comments inside its code) and `members` (the members it declares, when it is a type). The body of the code is \
not shown.";

const ANSWER_YES_WHEN: &str = "The candidate is the code the request asks for: it does, defines or decides what the request describes, \
or its name is the identifier the request names.";

const ANSWER_NO_WHEN: &str = "The candidate is only on a related topic, only uses or calls the thing the request describes, or only \
shares some words with the request.";

/// A pergunta da escolha. A última frase é a saída da opção `none`.
const QUESTION: &str = "Which one of the candidates is the code that the `request` is looking for? Answer with the id of the single \
candidate that best is that code. If none of them is that code, answer `none`.";

/// A pergunta de sim ou não sobre um finalista; `@ID@` vira o id dele.
const FINALIST_QUESTION: &str = "Is candidate `@ID@` the code that the `request` is looking for?";

/// O começo da chave da pergunta de sim ou não de cada finalista; o resto é o
/// id dele.
const FITS_PREFIX: &str = "fits_";

/// O texto da opção "nenhum destes".
const NONE_CRITERION: &str = "None of the candidates is the code the request asks for: each one is only on a related topic, only uses or \
calls it, is only used by it, or only shares some words with the request.";

/// A chave da opção "nenhum destes" nos critérios e nas chances.
const NONE_KEY: &str = "none";

/// A chave da pergunta única do pedido.
const CHOICE_KEY: &str = "q";

/// A chave do estado que traz os candidatos.
const CANDIDATES_KEY: &str = "candidates (one per line: id| fields as JSON)";

const GUESSED_WORDS: &str = "guessed words (the agent's guesses, they may not exist in the code)";

// Os cortes de cada campo do candidato.
const SIGNATURE_CHARS: usize = 300;
const DOCUMENTATION_CHARS: usize = 400;
const BODY_COMMENTS_CHARS: usize = 600;
const FILE_COMMITS: usize = 3;
const MEMBERS: usize = 16;

/// As palavras da linha do dono que não são nomes.
const OWNER_KEYWORDS: [&str; 8] = ["for", "impl", "pub", "mod", "trait", "where", "dyn", "mut"];

// ---------------------------------------------------------------------------
// A chave
// ---------------------------------------------------------------------------

/// A chave do serviço. Não se imprime: o `Debug` escreve reticências, e não
/// há `Display`.
#[derive(Clone)]
pub struct JevKey(String);

impl fmt::Debug for JevKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JevKey(…)")
    }
}

/// A chave achada e, quando o git guarda o `mustard.json` que também traz
/// uma chave, o aviso para tirá-lo do git. A chave do ambiente vale do mesmo
/// jeito.
#[derive(Debug, Clone)]
pub struct LoadedKey {
    pub key: JevKey,
    pub warning: Option<FilterError>,
}

/// A chave do projeto em `root`: [`KEY_ENV`] no ambiente; sem ela, `jev.key`
/// do `mustard.json` que `config` leu. Sem nenhuma das duas,
/// [`FilterError::MissingKey`]; com a do arquivo que o git guarda,
/// [`FilterError::KeyInGit`].
pub fn load_key(root: &Path, config: &ProjectConfig) -> Result<LoadedKey, FilterError> {
    key_in(root, config, std::env::var(KEY_ENV).ok())
}

/// A chave do projeto como em [`load_key`], com `env` no lugar do valor de
/// [`KEY_ENV`]: o teste não depende do ambiente de quem o roda.
pub fn key_in(root: &Path, config: &ProjectConfig, env: Option<String>) -> Result<LoadedKey, FilterError> {
    key_from(env, config.jev_key(), || tracked_by_git(root))
}

/// A escolha da chave, sobre o valor do ambiente e o do `mustard.json`; o
/// valor em branco vale como ausente. `tracked` diz se o git guarda o
/// arquivo, e só se pergunta quando ele traz uma chave: a chave guardada no
/// git não se usa, e o aviso sai mesmo quando a do ambiente vale.
fn key_from(env: Option<String>, project: Option<&str>, tracked: impl FnOnce() -> bool) -> Result<LoadedKey, FilterError> {
    let project = project.map(str::trim).filter(|key| !key.is_empty());
    let in_git = project.is_some() && tracked();
    let warning = in_git.then_some(FilterError::KeyInGit);
    if let Some(key) = env.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) {
        return Ok(LoadedKey { key: JevKey(key), warning });
    }
    if in_git {
        return Err(FilterError::KeyInGit);
    }
    let key = project.ok_or(FilterError::MissingKey)?;
    Ok(LoadedKey { key: JevKey(key.to_string()), warning: None })
}

/// O git guarda o `mustard.json` de `root`, no índice ou num commit. Sem git
/// ou fora de um repositório, não guarda.
fn tracked_by_git(root: &Path) -> bool {
    mustard_core::platform::git::run(root, &["ls-files", "--error-unmatch", "--", "mustard.json"]).ok
}

// ---------------------------------------------------------------------------
// O filtro
// ---------------------------------------------------------------------------

/// Os prazos da rede, fixos nas constantes; o teste encurta o da resposta.
#[derive(Debug, Clone, Copy)]
struct Timeouts {
    connect: Duration,
    response: Duration,
}

/// O filtro pelo Jev.
#[derive(Debug, Clone)]
pub struct JevFilter {
    key: JevKey,
    endpoint: String,
    timeouts: Timeouts,
}

impl JevFilter {
    /// O filtro com a chave do projeto, no endereço do serviço.
    #[must_use]
    pub fn new(key: JevKey) -> Self {
        Self::at(key, JEV_URL)
    }

    fn at(key: JevKey, endpoint: &str) -> Self {
        Self {
            key,
            endpoint: endpoint.to_string(),
            timeouts: Timeouts { connect: CONNECT_TIMEOUT, response: RESPONSE_TIMEOUT },
        }
    }

    /// Um pedido, com as repetições, até `deadline`. Devolve o documento da
    /// resposta. Cada tentativa leva só o tempo que falta até o prazo, e a
    /// repetição cuja espera passaria do prazo não se faz.
    fn send(&self, payload: &str, deadline: Instant) -> Result<Value, FilterError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(self.timeouts.connect))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let auth = format!("Bearer {}", self.key.0);
        let mut retries = 0;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(FilterError::Timeout);
            }
            let mut response = agent
                .post(&self.endpoint)
                .config()
                .timeout_global(Some(left))
                .build()
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .send(payload.as_bytes())
                .map_err(transport_error)?;
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                let text = response.body_mut().read_to_string().map_err(transport_error)?;
                return serde_json::from_str(&text)
                    .map_err(|_| FilterError::Unreadable("the body is not JSON".to_string()));
            }
            let asked = response.headers().get("retry-after").and_then(|value| value.to_str().ok());
            let wait = retry_wait(asked);
            if retryable(status) && retries < MAX_RETRIES && Instant::now() + wait < deadline {
                retries += 1;
                std::thread::sleep(wait);
                continue;
            }
            return Err(FilterError::Refused { status });
        }
    }
}

impl MapFilter for JevFilter {
    fn filter(&self, request: &FilterRequest) -> Result<Filtered, FilterError> {
        let started = Instant::now();
        // Um prazo só para todos os pedidos: o agente espera a busca inteira.
        let deadline = started + self.timeouts.response;
        if request.candidates.is_empty() {
            return Ok(Filtered {
                verdict: Verdict::NotFound,
                kept: Vec::new(),
                usage: FilterUsage { model: String::new(), ..FilterUsage::default() },
            });
        }
        let head = Head::of(request);
        let lines: Vec<String> = request.candidates.iter().enumerate().map(|(at, c)| candidate_line(at, c)).collect();
        let mut reads = Vec::new();
        let mut input_tokens = 0;
        let mut models: Vec<String> = Vec::new();
        for (range, payload) in batches(&head, &lines)? {
            let doc = self.send(&payload, deadline)?;
            let read = read_choice(&doc, range.start, &request.candidates[range])?;
            input_tokens += read.input_tokens;
            reads.push(read);
            note_model(&mut models, &doc);
        }
        let Merged { scores, none, confidence } = merge(reads);
        let (mut verdict, mut kept) = judged(&scores, none, confidence, request.share);
        if verdict == Verdict::Split {
            // A escolha ficou dividida: relê só os finalistas. A falha dessa
            // etapa não é falha da busca, e vale o corte da primeira.
            let second = self.second_look(&head, request, &scores, deadline);
            input_tokens += second.input_tokens;
            if let Some(model) = second.model {
                note_model(&mut models, &model);
            }
            if let Some((decided, chosen)) = second.decided {
                (verdict, kept) = (decided, chosen);
            }
        }
        Ok(Filtered {
            verdict,
            kept,
            usage: FilterUsage {
                input_tokens,
                millis: started.elapsed().as_millis() as u64,
                cost_micro_usd: cost_micro_usd(input_tokens),
                model: models.join(","),
            },
        })
    }
}

/// O que a segunda olhada devolve: os tokens que o pedido custou, o documento
/// da resposta (para o nome do modelo) e, quando a resposta se leu, a decisão.
struct Second {
    input_tokens: u64,
    model: Option<Value>,
    decided: Option<(Verdict, Vec<Scored>)>,
}

impl JevFilter {
    /// A segunda olhada sobre os finalistas de `scores`: um pedido com uma
    /// escolha entre eles e `none` e um sim ou não para cada um, sob o mesmo
    /// `deadline` da primeira etapa. Qualquer falha (rede, recusa, prazo,
    /// resposta ilegível) deixa `decided` vazio, e os tokens da resposta que
    /// chegou contam do mesmo jeito.
    fn second_look(&self, head: &Head, request: &FilterRequest, scores: &[Scored], deadline: Instant) -> Second {
        let nothing = Second { input_tokens: 0, model: None, decided: None };
        let group: Vec<FilterCandidate> = finalists_of(scores)
            .iter()
            .filter_map(|finalist| request.candidates.iter().find(|c| c.id == finalist.id).cloned())
            .collect();
        if group.is_empty() {
            return nothing;
        }
        let lines: Vec<String> = group.iter().enumerate().map(|(at, c)| candidate_line(at, c)).collect();
        let Ok(payload) = serde_json::to_string(&second_body(head, &lines)) else { return nothing };
        if estimated_tokens(&payload) > MAX_REQUEST_TOKENS {
            return nothing;
        }
        let Ok(doc) = self.send(&payload, deadline) else { return nothing };
        let input_tokens = doc.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0);
        let decided = read_second(&doc, &group).ok();
        Second { input_tokens, model: Some(doc), decided }
    }
}

/// O nome do modelo que a resposta `doc` diz ter respondido, guardado uma vez.
fn note_model(models: &mut Vec<String>, doc: &Value) {
    let model = doc.get("model").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    if !model.is_empty() && !models.iter().any(|seen| seen == model) {
        models.push(model.to_string());
    }
}

/// O custo de `input_tokens` tokens de entrada em milionésimos de dólar,
/// pelo preço de tabela: o preço por milhão de tokens é o preço de cada
/// token em milionésimos.
fn cost_micro_usd(input_tokens: u64) -> u64 {
    (input_tokens as f64 * PRICE_PER_MILLION_INPUT_TOKENS).round() as u64
}

/// O que a pergunta de escolha de um pedido respondeu.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    /// A chance de cada candidato do pedido, na ordem do banco.
    scores: Vec<Scored>,
    /// A chance de "nenhum destes".
    none: f64,
    /// A confiança do serviço na escolha.
    confidence: f64,
    /// Os tokens de entrada que o pedido custou.
    input_tokens: u64,
}

/// A escolha lida da resposta de um pedido: a chance de cada id, a de `none`
/// e a confiança. `first` é a posição do primeiro candidato do pedido na
/// lista inteira, de onde vem o id de cada um. Falta de `answers`, de uma
/// chance ou da confiança é resposta ilegível.
fn read_choice(doc: &Value, first: usize, group: &[FilterCandidate]) -> Result<Choice, FilterError> {
    let answer = doc
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| FilterError::Unreadable("no answers".to_string()))?
        .get(CHOICE_KEY)
        .ok_or_else(|| FilterError::Unreadable("no answer to the choice".to_string()))?;
    let chances = answer
        .get("probabilities")
        .and_then(Value::as_object)
        .ok_or_else(|| FilterError::Unreadable("no chances".to_string()))?;
    let chance = |key: &str| {
        chances
            .get(key)
            .and_then(Value::as_f64)
            .ok_or_else(|| FilterError::Unreadable(format!("no chance for {key}")))
    };
    let mut scores = Vec::with_capacity(group.len());
    for (at, candidate) in group.iter().enumerate() {
        scores.push(Scored { id: candidate.id, score: chance(&candidate_id(first + at))? });
    }
    let confidence = answer
        .get("confidence")
        .and_then(Value::as_f64)
        .ok_or_else(|| FilterError::Unreadable("no confidence".to_string()))?;
    Ok(Choice {
        scores,
        none: chance(NONE_KEY)?,
        confidence,
        input_tokens: doc.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// A decisão lida da resposta da segunda olhada: a chance de cada finalista
/// na escolha e o sim de cada um, postos na decisão do núcleo
/// ([`second_look`]). `group` são os finalistas do pedido, na ordem dele;
/// faltar a escolha ou um sim é resposta ilegível.
fn read_second(doc: &Value, group: &[FilterCandidate]) -> Result<(Verdict, Vec<Scored>), FilterError> {
    let choice = read_choice(doc, 0, group)?;
    let answers = doc.get("answers").and_then(Value::as_object);
    let mut finalists = Vec::with_capacity(group.len());
    for (at, (candidate, scored)) in group.iter().zip(&choice.scores).enumerate() {
        let key = format!("{FITS_PREFIX}{}", candidate_id(at));
        let yes = answers
            .and_then(|all| all.get(&key))
            .and_then(|answer| answer.get("noul"))
            .and_then(Value::as_f64)
            .ok_or_else(|| FilterError::Unreadable(format!("no yes for {key}")))?;
        finalists.push(Finalist { id: candidate.id, chance: scored.score, yes });
    }
    Ok(second_look(&finalists, choice.none))
}

/// O que sobra de todos os pedidos juntos.
#[derive(Debug, Clone, PartialEq)]
struct Merged {
    scores: Vec<Scored>,
    none: f64,
    confidence: f64,
}

/// As escolhas de todos os pedidos numa só. Com um pedido, é a escolha dele.
/// Com mais, o pedido que diz "nenhum destes" não tem o certo: as chances dele
/// saem, e vale o que os outros disseram, com a menor chance de "nenhum
/// destes" e a menor confiança entre eles. Se todos dizem "nenhum destes", a
/// menor chance de "nenhum destes" fica.
fn merge(reads: Vec<Choice>) -> Merged {
    let found = |read: &Choice| verdict(read.none, read.confidence) != Verdict::NotFound;
    if !reads.iter().any(found) {
        let none = reads.iter().map(|read| read.none).fold(1.0, f64::min);
        return Merged { scores: Vec::new(), none, confidence: 0.0 };
    }
    let mut merged = Merged { scores: Vec::new(), none: 1.0, confidence: 1.0 };
    for read in reads.into_iter().filter(found) {
        merged.none = merged.none.min(read.none);
        merged.confidence = merged.confidence.min(read.confidence);
        merged.scores.extend(read.scores);
    }
    merged
}

/// O erro de transporte, sem nada do pedido: o tempo esgotado à parte, o
/// resto como falha de rede.
fn transport_error(error: ureq::Error) -> FilterError {
    match error {
        ureq::Error::Timeout(_) => FilterError::Timeout,
        ureq::Error::Io(io) if io.kind() == ErrorKind::TimedOut => FilterError::Timeout,
        other => FilterError::Network(other.to_string()),
    }
}

/// O excesso de pedidos (429), a sobrecarga (529) e a falha do serviço (5xx)
/// passam com o tempo; o resto não.
fn retryable(status: u16) -> bool {
    status == 429 || status >= 500
}

/// A espera antes de repetir: o `retry-after` em segundos, até
/// [`MAX_RETRY_WAIT`]; sem ele, ou ilegível, [`DEFAULT_RETRY_WAIT`].
fn retry_wait(retry_after: Option<&str>) -> Duration {
    retry_after
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map_or(DEFAULT_RETRY_WAIT, |seconds| Duration::from_secs_f64(seconds.min(MAX_RETRY_WAIT.as_secs_f64())))
}

/// Os tokens estimados de um pedido, pelos caracteres.
fn estimated_tokens(payload: &str) -> u64 {
    (payload.chars().count() as f64 / CHARS_PER_TOKEN).ceil() as u64
}

// ---------------------------------------------------------------------------
// O pedido
// ---------------------------------------------------------------------------

/// O que é igual em todos os pedidos de uma busca: a frase (ou, sem ela, as
/// palavras), as palavras como palpites e a instrução da escolha. Nada aqui
/// leva segredo.
struct Head {
    request: String,
    words: Vec<String>,
    instructions: String,
}

impl Head {
    fn of(request: &FilterRequest) -> Self {
        let asked = if request.phrase.trim().is_empty() { request.words.join(" ") } else { request.phrase.clone() };
        Self {
            request: without_secrets(&asked),
            words: request.words.iter().map(|word| without_secrets(word)).collect(),
            instructions: choice_instructions(ABOUT),
        }
    }
}

/// A instrução da escolha: o texto do estado, o que é certo e o que é errado,
/// e a pergunta.
fn choice_instructions(about: &str) -> String {
    format!("{about}\n\nThe right candidate: {ANSWER_YES_WHEN}\nA wrong candidate: {ANSWER_NO_WHEN}\n\n{QUESTION}")
}

/// O texto do estado da segunda olhada para `count` finalistas.
fn finalists_about(count: usize) -> String {
    let count = match count {
        1 => "one",
        2 => "two",
        _ => "three",
    };
    ABOUT_FINALISTS.replace("@COUNT@", count)
}

/// O corpo de um pedido com uma pergunta de escolha: o estado (a frase, as
/// palavras e os candidatos, um por linha), o modelo e a pergunta, cujas
/// opções são os ids dos candidatos de `lines`, a partir da posição `first`, e
/// `none`.
fn body(head: &Head, first: usize, lines: &[String]) -> Value {
    let question = choice_question(&head.instructions, first, lines.len());
    request_body(head, lines, json!({ CHOICE_KEY: question }))
}

/// O corpo de um pedido: o estado (a frase, as palavras e os candidatos de
/// `lines`, um por linha), o modelo e as perguntas.
fn request_body(head: &Head, lines: &[String], questions: Value) -> Value {
    let mut state = Map::new();
    state.insert("request".to_string(), Value::String(head.request.clone()));
    state.insert(GUESSED_WORDS.to_string(), json!(head.words));
    state.insert(CANDIDATES_KEY.to_string(), Value::String(lines.join("\n")));
    let mut body = Map::new();
    body.insert("state".to_string(), Value::Object(state));
    body.insert("model".to_string(), Value::String(JEV_MODEL.to_string()));
    body.insert("questions".to_string(), questions);
    Value::Object(body)
}

/// A pergunta de escolha entre `count` candidatos, a partir da posição
/// `first`, e `none`.
fn choice_question(instructions: &str, first: usize, count: usize) -> Value {
    let mut criteria = Map::new();
    for at in 0..count {
        criteria.insert(candidate_id(first + at), Value::Null);
    }
    criteria.insert(NONE_KEY.to_string(), Value::String(NONE_CRITERION.to_string()));
    let mut question = Map::new();
    question.insert("type".to_string(), json!("choice"));
    question.insert("instructions".to_string(), Value::String(instructions.to_string()));
    question.insert("criteria".to_string(), Value::Object(criteria));
    Value::Object(question)
}

/// O corpo do pedido da segunda olhada: os finalistas de `lines`, de `c000`
/// em diante, com a escolha entre eles e `none` e, para cada um, a pergunta
/// de sim ou não se ele é o código que se procura.
fn second_body(head: &Head, lines: &[String]) -> Value {
    let about = finalists_about(lines.len());
    let mut questions = Map::new();
    questions.insert(CHOICE_KEY.to_string(), choice_question(&choice_instructions(&about), 0, lines.len()));
    for at in 0..lines.len() {
        let id = candidate_id(at);
        questions.insert(
            format!("{FITS_PREFIX}{id}"),
            json!({
                "type": "noul",
                "instructions": format!("{about}\n\n{}", FINALIST_QUESTION.replace("@ID@", &id)),
                "criteria": { "true": ANSWER_YES_WHEN, "false": ANSWER_NO_WHEN },
            }),
        );
    }
    request_body(head, lines, Value::Object(questions))
}

/// O id de um candidato no pedido: `c000` em diante, pela posição dele na
/// lista inteira.
fn candidate_id(at: usize) -> String {
    format!("c{at:03}")
}

/// A linha de um candidato no estado: `id| {campos em JSON}`.
fn candidate_line(at: usize, candidate: &FilterCandidate) -> String {
    format!("{}| {}", candidate_id(at), spaced_json(&candidate_fields(candidate)))
}

/// O JSON com um espaço depois de cada vírgula e de cada dois-pontos, como o
/// laboratório mandava os candidatos. O texto acentuado sai como está.
fn spaced_json(value: &Value) -> String {
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, Spaced);
    if value.serialize(&mut serializer).is_err() {
        return String::new();
    }
    String::from_utf8(out).unwrap_or_default()
}

/// O formato de [`spaced_json`].
struct Spaced;

impl Formatter for Spaced {
    fn begin_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_key<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }
}

/// O texto de um pedido com os candidatos de `lines` a partir de `first`.
fn payload(head: &Head, first: usize, lines: &[String]) -> Result<String, FilterError> {
    serde_json::to_string(&body(head, first, lines))
        .map_err(|_| FilterError::Unreadable("the request did not serialize".to_string()))
}

/// Os pedidos da busca, cada um com os candidatos que leva (a faixa da lista
/// inteira) e o texto. Um pedido só, quando cabe em [`MAX_REQUEST_TOKENS`];
/// senão, o menor número de pedidos que mantém a ordem do banco, cada um
/// levando quantos candidatos couberem. Um candidato que sozinho passa do
/// limite é [`FilterError::TooLarge`], e nada sai.
fn batches(head: &Head, lines: &[String]) -> Result<Vec<(Range<usize>, String)>, FilterError> {
    let whole = payload(head, 0, lines)?;
    if estimated_tokens(&whole) <= MAX_REQUEST_TOKENS {
        return Ok(vec![(0..lines.len(), whole)]);
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let mut end = start + 1;
        let mut text = payload(head, start, &lines[start..end])?;
        let estimated = estimated_tokens(&text);
        if estimated > MAX_REQUEST_TOKENS {
            return Err(FilterError::TooLarge { estimated_tokens: estimated });
        }
        while end < lines.len() {
            let more = payload(head, start, &lines[start..=end])?;
            if estimated_tokens(&more) > MAX_REQUEST_TOKENS {
                break;
            }
            text = more;
            end += 1;
        }
        out.push((start..end, text));
        start = end;
    }
    Ok(out)
}

/// O que vai de um candidato, na ordem medida, sem os campos vazios. Cada
/// texto sai sem os segredos, antes dos cortes: o corte não parte um segredo
/// num trecho que a procura já não reconhece.
fn candidate_fields(candidate: &FilterCandidate) -> Value {
    let clean = |text: &str| without_secrets(text);
    let commits: Vec<String> = candidate.file_commits.iter().take(FILE_COMMITS).map(|title| clean(title)).collect();
    let members: Vec<String> = candidate.members.iter().map(|member| clean(member)).collect();
    let mut out = Map::new();
    put_text(&mut out, "kind", clean(&candidate.kind));
    put_text(&mut out, "name", split_identifier(&clean(&candidate.name)));
    put_text(&mut out, "path", clean(&candidate.path));
    put_text(&mut out, "signature", squash(&clean(&candidate.signature)).chars().take(SIGNATURE_CHARS).collect());
    put_text(&mut out, "documentation", clip(&clean(&candidate.documentation), DOCUMENTATION_CHARS, "…"));
    put_list(&mut out, "last commits of the file", commits);
    put_text(&mut out, "owner", owner_names(&clean(&candidate.owner)));
    put_text(&mut out, "comments in the body", clip(&clean(&candidate.body_comments), BODY_COMMENTS_CHARS, " …"));
    put_list(&mut out, "members", capped(&members, MEMBERS));
    Value::Object(out)
}

fn put_text(out: &mut Map<String, Value>, key: &str, value: String) {
    if !value.is_empty() {
        out.insert(key.to_string(), Value::String(value));
    }
}

fn put_list(out: &mut Map<String, Value>, key: &str, values: Vec<String>) {
    if !values.is_empty() {
        out.insert(key.to_string(), json!(values));
    }
}

/// Os espaços juntados: toda sequência de brancos vira um espaço, e as
/// pontas somem.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// O texto com os espaços juntados; acima de `max` caracteres, cortado na
/// última palavra inteira, com " …". Sem espaço no trecho, o corte cai no
/// meio da palavra, seguido de `unbroken_mark`.
fn clip(text: &str, max: usize, unbroken_mark: &str) -> String {
    let text = squash(text);
    if text.chars().count() <= max {
        return text;
    }
    let head: String = text.chars().take(max).collect();
    match head.rfind(' ') {
        Some(at) => format!("{} …", &head[..at]),
        None => format!("{head}{unbroken_mark}"),
    }
}

/// Até `max` itens, e depois quantos ficaram de fora: `"... 4 more"`.
fn capped(items: &[String], max: usize) -> Vec<String> {
    let mut out: Vec<String> = items.iter().take(max).cloned().collect();
    if items.len() > max {
        out.push(format!("... {} more", items.len() - max));
    }
    out
}

/// Os nomes da linha do dono, cada um uma vez, sem as palavras da linguagem:
/// `impl<T> Display for Wrapper<T>` vira `Display Wrapper`. Nome é uma letra
/// ASCII ou `_` seguida de ao menos um caractere de nome; o nome de uma letra
/// só fica de fora.
fn owner_names(owner: &str) -> String {
    let mut names: Vec<&str> = Vec::new();
    for run in owner.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        let name = run.trim_start_matches(|c: char| c.is_ascii_digit());
        if name.len() >= 2 && !OWNER_KEYWORDS.contains(&name) && !names.contains(&name) {
            names.push(name);
        }
    }
    names.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::map_filter::CUT_SHARE;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex, OnceLock};

    const SECRET: &str = "sk-test-0123456789abcdef";

    fn test_key() -> JevKey {
        JevKey(SECRET.to_string())
    }

    /// Um endereço em que ninguém responde: a porta fica presa até o fim do
    /// processo, e quem se conecta a ela cai na hora, sem resposta. Uma porta
    /// solta logo depois do `bind` podia ser tomada por um serviço de mentira
    /// de outro teste (deste processo ou de outro que roda ao mesmo tempo), e o
    /// pedido de quem procurava a porta fechada chegava a esse serviço, que
    /// contava um pedido a mais.
    fn closed_url() -> String {
        static URL: OnceLock<String> = OnceLock::new();
        URL.get_or_init(|| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    drop(stream);
                }
            });
            url
        })
        .clone()
    }

    // -- o serviço de mentira ------------------------------------------------

    /// Um pedido que chegou ao serviço de mentira.
    #[derive(Debug, Clone)]
    struct Received {
        authorization: String,
        body: Value,
    }

    /// A resposta que o serviço de mentira devolve a um pedido.
    struct Reply {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    }

    impl Reply {
        fn json(status: u16, body: &Value) -> Self {
            Self { status, headers: Vec::new(), body: body.to_string() }
        }
    }

    type Responder = dyn Fn(usize, &Value) -> Reply + Send + Sync;

    /// Um serviço HTTP em 127.0.0.1, numa thread, que grava cada pedido e
    /// devolve o que `respond` monta para ele (o número do pedido, a partir de
    /// 0, e o corpo). Cada conexão tem a sua thread.
    struct FakeService {
        url: String,
        received: Arc<Mutex<Vec<Received>>>,
    }

    impl FakeService {
        fn start(respond: impl Fn(usize, &Value) -> Reply + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
            let received = Arc::new(Mutex::new(Vec::new()));
            let respond: Arc<Responder> = Arc::new(respond);
            let log = Arc::clone(&received);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let (log, respond) = (Arc::clone(&log), Arc::clone(&respond));
                    std::thread::spawn(move || serve_one(stream, &log, respond.as_ref()));
                }
            });
            Self { url, received }
        }

        fn received(&self) -> Vec<Received> {
            self.received.lock().unwrap().clone()
        }

        fn filter(&self) -> JevFilter {
            JevFilter::at(test_key(), &self.url)
        }
    }

    fn serve_one(mut stream: TcpStream, log: &Mutex<Vec<Received>>, respond: &Responder) {
        let Some(received) = read_request(&stream) else { return };
        let number = {
            let mut log = log.lock().unwrap();
            log.push(received.clone());
            log.len() - 1
        };
        let reply = respond(number, &received.body);
        let mut head = format!("HTTP/1.1 {} X\r\nContent-Type: application/json\r\nConnection: close\r\n", reply.status);
        for (name, value) in &reply.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", reply.body.len()));
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(reply.body.as_bytes());
        let _ = stream.flush();
    }

    fn read_request(stream: &TcpStream) -> Option<Received> {
        let mut reader = BufReader::new(stream);
        let mut authorization = String::new();
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).ok()? == 0 {
                return None;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                match name.trim().to_ascii_lowercase().as_str() {
                    "authorization" => authorization = value.trim().to_string(),
                    "content-length" => length = value.trim().parse().ok()?,
                    _ => {}
                }
            }
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).ok()?;
        Some(Received { authorization, body: serde_json::from_slice(&body).ok()? })
    }

    /// Os candidatos do estado de um pedido, na ordem: o id e os campos.
    fn listed(body: &Value) -> Vec<(String, Value)> {
        body["state"][CANDIDATES_KEY]
            .as_str()
            .unwrap()
            .lines()
            .map(|line| {
                let (id, fields) = line.split_once("| ").unwrap();
                (id.to_string(), serde_json::from_str(fields).unwrap())
            })
            .collect()
    }

    /// A resposta do serviço à escolha de um pedido: a chance de cada
    /// candidato pelo nome dele, a de `none` e a confiança, e 1000 tokens de
    /// entrada.
    fn choice_by_name(body: &Value, chance_of: impl Fn(&str) -> f64, none: f64, confidence: f64) -> Value {
        let mut chances = Map::new();
        for (id, fields) in listed(body) {
            chances.insert(id, json!(chance_of(fields["name"].as_str().unwrap_or_default())));
        }
        chances.insert("none".to_string(), json!(none));
        json!({
            "answers": {"q": {"type": "choice", "choice": "c000", "confidence": confidence, "probabilities": chances}},
            "usage": {"input_tokens": 1000, "output_tokens": 0},
        })
    }

    /// A escolha sem chance para ninguém e o serviço seguro: só `none` baixo.
    fn sure_answer(body: &Value) -> Value {
        choice_by_name(body, |_| 0.5, 0.01, 0.9)
    }

    fn candidate(id: i64) -> FilterCandidate {
        FilterCandidate {
            id,
            kind: "function".to_string(),
            name: format!("cand{id}"),
            path: "src/lib.rs".to_string(),
            line: 1,
            end_line: 2,
            ..FilterCandidate::default()
        }
    }

    fn request(candidates: Vec<FilterCandidate>) -> FilterRequest {
        FilterRequest {
            words: vec!["cand".to_string()],
            phrase: "the candidate that answers".to_string(),
            share: CUT_SHARE,
            candidates,
        }
    }

    // -- o corpo do pedido ---------------------------------------------------

    #[test]
    fn the_request_body_matches_the_one_the_lab_measured() {
        let example: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/jev/request_two_candidates.json")).unwrap();
        let input = &example["input"];
        let text = |v: &Value| v.as_str().unwrap().to_string();
        let texts = |v: &Value| v.as_array().unwrap().iter().map(text).collect::<Vec<_>>();
        let candidates = input["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| FilterCandidate {
                id: c["id"].as_i64().unwrap(),
                kind: text(&c["kind"]),
                name: text(&c["name"]),
                path: text(&c["path"]),
                line: c["line"].as_u64().unwrap() as u32,
                end_line: c["end_line"].as_u64().unwrap() as u32,
                signature: text(&c["signature"]),
                documentation: text(&c["documentation"]),
                owner: text(&c["owner"]),
                members: texts(&c["members"]),
                body_comments: text(&c["body_comments"]),
                file_commits: texts(&c["file_commits"]),
            })
            .collect();
        let asked = FilterRequest {
            words: texts(&input["words"]),
            phrase: text(&input["phrase"]),
            share: CUT_SHARE,
            candidates,
        };
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        service.filter().filter(&asked).unwrap();

        let sent = service.received();
        assert_eq!(sent.len(), 1);
        // O arquivo guarda o pedido como o laboratório o mandou. As diferenças
        // planejadas são só duas: o texto do `about`, que cita cada campo que
        // vai no candidato, e a versão fixa do modelo.
        let mut expected = example["body"].clone();
        let instructions = expected["questions"]["q"]["instructions"].as_str().unwrap().replace("@ABOUT@", ABOUT);
        expected["questions"]["q"]["instructions"] = json!(instructions);
        expected["model"] = json!(JEV_MODEL);
        // Com a ordem dos campos: o texto inteiro é igual ao do laboratório.
        assert_eq!(sent[0].body.to_string(), expected.to_string());
    }

    /// Um candidato com todos os campos cheios.
    fn full_candidate() -> FilterCandidate {
        FilterCandidate {
            id: 1,
            kind: "method".to_string(),
            name: "chargeCard".to_string(),
            path: "src/pay/card.rs".to_string(),
            line: 3,
            end_line: 9,
            signature: "fn charge_card(&self, total: u32)".to_string(),
            documentation: "Cobra o cartão.".to_string(),
            owner: "CardGateway PaymentPort".to_string(),
            members: vec!["charge()".to_string()],
            body_comments: "manda ao banco".to_string(),
            file_commits: vec!["Cartão sem juros".to_string()],
        }
    }

    /// Os nomes entre crases do texto, na ordem.
    fn quoted(text: &str) -> Vec<String> {
        text.split('`').skip(1).step_by(2).map(str::to_string).collect()
    }

    #[test]
    fn the_about_names_each_field_of_the_candidate_in_its_order() {
        let fields = candidate_fields(&full_candidate());
        let keys: Vec<String> = fields.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys.len(), 9, "every field is filled: {keys:?}");
        let named: Vec<String> = quoted(ABOUT).into_iter().skip_while(|name| name == "request").collect();
        assert_eq!(named, keys);
    }

    #[test]
    fn a_secret_in_any_text_does_not_leave_the_machine() {
        let key = format!("ghp_{}", "a1B2c3D4".repeat(5));
        let secret = format!("DB_PASSWORD=S3nh4F0rte2024 {key}");
        let mut leaky = full_candidate();
        for text in [
            &mut leaky.name,
            &mut leaky.path,
            &mut leaky.signature,
            &mut leaky.documentation,
            &mut leaky.owner,
            &mut leaky.body_comments,
        ] {
            text.push_str(&format!(" {secret}"));
        }
        leaky.members.push(key.clone());
        leaky.file_commits = vec![format!("Troca a chave {key}")];
        let mut asked = request(vec![leaky]);
        asked.words.push(key.clone());
        asked.phrase = format!("a senha do banco: {secret}");
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        service.filter().filter(&asked).unwrap();

        let sent = service.received()[0].body.to_string();
        assert!(!sent.contains("S3nh4F0rte2024"), "{sent}");
        assert!(!sent.contains(&key[..12]), "{sent}");
        assert!(sent.contains("Troca a chave …"), "the rest of the text still goes: {sent}");
        assert!(sent.contains("Cobra o cartão."), "{sent}");
    }

    #[test]
    fn the_usage_keeps_the_model_that_answered_and_the_cost_of_the_tokens() {
        let service = FakeService::start(|_, body| {
            let mut answer = sure_answer(body);
            answer["model"] = json!("jev-1.13.0");
            answer["usage"]["input_tokens"] = json!(10_500);
            Reply::json(200, &answer)
        });
        let got = service.filter().filter(&request((1..=100).map(candidate).collect())).unwrap();
        assert_eq!(service.received().len(), 1);
        assert!(service.received().iter().all(|sent| sent.body["model"] == json!("jev-1.13.0")));
        assert_eq!(got.usage.input_tokens, 10_500);
        assert_eq!(got.usage.model, "jev-1.13.0");
        // 10.500 tokens a US$ 0,042 o milhão: US$ 0,000441.
        assert_eq!(got.usage.cost_micro_usd, 441);

        let silent = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        let got = silent.filter().filter(&request(vec![candidate(1)])).unwrap();
        assert_eq!(got.usage.model, "", "an answer that does not say the model leaves it empty");
    }

    #[test]
    fn long_comments_are_cut_at_a_word_and_many_members_are_counted() {
        let mut long = candidate(1);
        long.body_comments = "abcdefghi ".repeat(70);
        assert_eq!(long.body_comments.chars().count(), 700);
        long.members = (1..=20).map(|n| format!("member{n}()")).collect();
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        service.filter().filter(&request(vec![long])).unwrap();

        let sent = &listed(&service.received()[0].body)[0].1;
        let comments = sent["comments in the body"].as_str().unwrap();
        assert_eq!(comments, format!("{} …", vec!["abcdefghi"; 60].join(" ")));
        assert!(comments.chars().count() <= BODY_COMMENTS_CHARS + 2);
        let members: Vec<&str> = sent["members"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(members.len(), 17);
        assert_eq!(members[15], "member16()");
        assert_eq!(members[16], "... 4 more");
    }

    #[test]
    fn short_fields_go_whole_and_empty_fields_do_not_go() {
        let mut short = candidate(1);
        short.body_comments = "a b  c".to_string();
        short.members = (1..=16).map(|n| format!("m{n}")).collect();
        short.owner = "impl<T> fmt::Display for Wrapper<T> where T: Clone".to_string();
        short.file_commits = vec!["one".into(), "two".into(), "three".into(), "four".into()];
        let fields = candidate_fields(&short);
        assert_eq!(fields["comments in the body"], "a b c");
        assert_eq!(fields["members"].as_array().unwrap().len(), 16);
        assert_eq!(fields["owner"], "fmt Display Wrapper Clone");
        assert_eq!(fields["last commits of the file"], json!(["one", "two", "three"]));
        assert!(fields.get("documentation").is_none());
        assert!(fields.get("signature").is_none());
    }

    #[test]
    fn documentation_and_signature_have_their_own_cuts() {
        let mut long = candidate(1);
        long.documentation = format!("{} tail", "word ".repeat(100));
        long.signature = format!("fn  {}", "x".repeat(400));
        let fields = candidate_fields(&long);
        let doc = fields["documentation"].as_str().unwrap();
        assert!(doc.ends_with("word …"));
        assert!(doc.chars().count() <= DOCUMENTATION_CHARS + 2);
        // Uma palavra só, longa demais: cortada no meio, com a marca colada.
        long.documentation = "y".repeat(500);
        assert_eq!(candidate_fields(&long)["documentation"], format!("{}…", "y".repeat(400)));
        let signature = fields["signature"].as_str().unwrap();
        assert!(signature.starts_with("fn xxx"));
        assert_eq!(signature.chars().count(), SIGNATURE_CHARS);
    }

    #[test]
    fn without_a_phrase_the_words_are_the_request() {
        let mut asked = request(vec![candidate(1)]);
        asked.phrase = "  ".to_string();
        asked.words = vec!["split".to_string(), "identifier".to_string()];
        assert_eq!(Head::of(&asked).request, "split identifier");
    }

    // -- a escolha e a nota ------------------------------------------------------

    #[test]
    fn a_hundred_candidates_go_in_one_choice_with_none_and_no_yes_or_no_question() {
        let service = FakeService::start(|_, body| {
            Reply::json(200, &choice_by_name(body, |name| if name == "cand7" { 0.9 } else { 0.001 }, 0.05, 0.9))
        });
        let got = service.filter().filter(&request((1..=100).map(candidate).collect())).unwrap();

        let sent = service.received();
        assert_eq!(sent.len(), 1, "the hundred candidates must go in one request");
        let body = &sent[0].body;
        assert!(!body.to_string().contains("noul"), "no yes-or-no question is asked");
        let questions = body["questions"].as_object().unwrap();
        assert_eq!(questions.len(), 1);
        let question = &questions["q"];
        assert_eq!(question["type"], "choice");
        let criteria = question["criteria"].as_object().unwrap();
        let keys: Vec<&str> = criteria.keys().map(String::as_str).collect();
        let expected: Vec<String> = (0..100).map(|at| format!("c{at:03}")).chain(["none".to_string()]).collect();
        assert_eq!(keys, expected.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(criteria.iter().all(|(key, value)| (key == "none") != value.is_null()));
        let instructions = question["instructions"].as_str().unwrap();
        assert!(instructions.starts_with(ABOUT));
        assert!(instructions.ends_with("If none of them is that code, answer `none`."));
        let lines = listed(body);
        assert_eq!(lines.len(), 100);
        assert_eq!(lines.first().map(|(id, _)| id.as_str()), Some("c000"));
        assert_eq!(lines.last().map(|(id, _)| id.as_str()), Some("c099"));
        assert_eq!(body["model"], json!("jev-1.13.0"));

        assert_eq!(got.verdict, Verdict::Sure);
        let kept: Vec<(i64, f64)> = got.kept.iter().map(|s| (s.id, s.score)).collect();
        assert_eq!(kept, vec![(7, 0.9)]);
        assert_eq!(got.usage.input_tokens, 1000);
    }

    #[test]
    fn a_chance_of_none_of_six_tenths_is_not_found_and_keeps_nothing() {
        let service = FakeService::start(|_, body| {
            Reply::json(200, &choice_by_name(body, |name| if name == "cand2" { 0.3 } else { 0.05 }, 0.6, 0.8))
        });
        let got = service.filter().filter(&request((1..=3).map(candidate).collect())).unwrap();
        assert_eq!(got.verdict, Verdict::NotFound);
        assert!(got.kept.is_empty(), "{:?}", got.kept);

        // Abaixo de meio, e a confiança alta: certo, e o corte vale.
        let sure = FakeService::start(|_, body| {
            Reply::json(200, &choice_by_name(body, |name| if name == "cand2" { 0.9 } else { 0.03 }, 0.04, 0.9))
        });
        let got = sure.filter().filter(&request((1..=3).map(candidate).collect())).unwrap();
        assert_eq!(got.verdict, Verdict::Sure);
        assert_eq!(got.kept.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2]);
    }

    /// A resposta do serviço à segunda olhada: a escolha entre os finalistas
    /// (a chance de cada um pelo nome, a de `none`) e o sim de cada finalista
    /// pelo nome.
    fn second_by_name(
        body: &Value,
        chance_of: impl Fn(&str) -> f64,
        none: f64,
        yes_of: impl Fn(&str) -> f64,
    ) -> Value {
        let mut answer = choice_by_name(body, chance_of, none, 0.9);
        for (id, fields) in listed(body) {
            let yes = yes_of(fields["name"].as_str().unwrap_or_default());
            answer["answers"][format!("{FITS_PREFIX}{id}")] = json!({"type": "noul", "noul": yes});
        }
        answer
    }

    /// A primeira resposta dividida: as chances de cand1 a cand3 são 0,4, 0,3
    /// e 0,2, as dos outros 0,03 (abaixo de 0,10 vezes a maior), a de `none`
    /// é 0 e a confiança 0,5.
    fn split_first(body: &Value) -> Value {
        choice_by_name(
            body,
            |name| match name {
                "cand1" => 0.4,
                "cand2" => 0.3,
                "cand3" => 0.2,
                _ => 0.03,
            },
            0.0,
            0.5,
        )
    }

    /// Um serviço que responde dividido ao primeiro pedido e ao segundo com
    /// `second`, que recebe o corpo dele.
    fn splitting(second: impl Fn(&Value) -> Reply + Send + Sync + 'static) -> FakeService {
        FakeService::start(move |number, body| {
            if number == 0 { Reply::json(200, &split_first(body)) } else { second(body) }
        })
    }

    fn kept_ids(got: &Filtered) -> Vec<i64> {
        got.kept.iter().map(|s| s.id).collect()
    }

    #[test]
    fn a_confidence_of_seven_tenths_asks_no_second_time() {
        let service = FakeService::start(|_, body| Reply::json(200, &choice_by_name(body, |_| 0.3, 0.0, 0.7)));
        let got = service.filter().filter(&request((1..=5).map(candidate).collect())).unwrap();
        assert_eq!(service.received().len(), 1, "a sure choice is delivered by the cut alone");
        assert_eq!(got.verdict, Verdict::Sure);
    }

    #[test]
    fn a_split_choice_asks_again_about_the_three_biggest_chances_with_one_choice_and_three_yes_questions() {
        let service = splitting(|body| Reply::json(200, &second_by_name(body, |_| 0.3, 0.05, |_| 0.9)));
        let got = service.filter().filter(&request((1..=5).map(candidate).collect())).unwrap();

        let sent = service.received();
        assert_eq!(sent.len(), 2, "one request for the choice and one for the second look");
        assert_eq!(listed(&sent[0].body).len(), 5);
        let second = &sent[1].body;
        assert_eq!(second["model"], json!(JEV_MODEL));
        assert_eq!(second["state"]["request"], json!("the candidate that answers"));
        let lines = listed(second);
        let names: Vec<(&str, &str)> =
            lines.iter().map(|(id, fields)| (id.as_str(), fields["name"].as_str().unwrap())).collect();
        assert_eq!(names, [("c000", "cand1"), ("c001", "cand2"), ("c002", "cand3")], "the three biggest chances, in that order");

        let questions = second["questions"].as_object().unwrap();
        let keys: Vec<&str> = questions.keys().map(String::as_str).collect();
        assert_eq!(keys, ["q", "fits_c000", "fits_c001", "fits_c002"]);
        let about = finalists_about(3);
        let choice = &questions["q"];
        assert_eq!(choice["type"], "choice");
        let criteria: Vec<&str> = choice["criteria"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(criteria, ["c000", "c001", "c002", "none"]);
        let instructions = choice["instructions"].as_str().unwrap();
        assert!(instructions.starts_with(&about), "{instructions}");
        assert!(instructions.ends_with("If none of them is that code, answer `none`."), "{instructions}");
        for id in ["c000", "c001", "c002"] {
            let fits = &questions[&format!("fits_{id}")];
            assert_eq!(fits["type"], "noul");
            assert_eq!(
                fits["instructions"],
                json!(format!("{about}\n\nIs candidate `{id}` the code that the `request` is looking for?"))
            );
            assert_eq!(fits["criteria"], json!({"true": ANSWER_YES_WHEN, "false": ANSWER_NO_WHEN}));
        }
        assert_eq!(got.verdict, Verdict::Sure);
        assert_eq!(kept_ids(&got), vec![1], "all three say yes at 0,9 and cand1 wins the choice by name of the tie");
    }

    #[test]
    fn the_text_of_the_second_look_names_each_field_of_the_candidate_in_its_order() {
        let fields = candidate_fields(&full_candidate());
        let keys: Vec<String> = fields.as_object().unwrap().keys().cloned().collect();
        for count in 1..=3 {
            let named: Vec<String> = quoted(&finalists_about(count)).into_iter().skip_while(|name| name == "request").collect();
            assert_eq!(named, keys, "{count} finalists");
        }
        assert!(finalists_about(3).contains("lists three candidate declarations"));
        assert!(finalists_about(2).contains("lists two candidate declarations"));
        assert!(!finalists_about(1).contains('@'));
    }

    #[test]
    fn the_winner_of_the_second_choice_with_a_yes_of_four_tenths_is_the_only_piece_delivered() {
        // O vencedor da escolha (cand2) tem sim 0,4; cand1 tem 0,95 e não vence.
        let service = splitting(|body| {
            Reply::json(
                200,
                &second_by_name(
                    body,
                    |name| if name == "cand2" { 0.7 } else { 0.1 },
                    0.05,
                    |name| match name {
                        "cand1" => 0.95,
                        "cand2" => 0.4,
                        _ => 0.1,
                    },
                ),
            )
        });
        let got = service.filter().filter(&request((1..=5).map(candidate).collect())).unwrap();
        assert_eq!(got.verdict, Verdict::Sure);
        assert_eq!(kept_ids(&got), vec![2]);
        assert!((got.kept[0].score - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn a_winner_with_a_yes_below_four_tenths_gives_way_to_the_finalist_with_the_highest_yes() {
        let service = splitting(|body| {
            Reply::json(
                200,
                &second_by_name(
                    body,
                    |name| if name == "cand1" { 0.7 } else { 0.1 },
                    0.05,
                    |name| match name {
                        "cand1" => 0.399,
                        "cand2" => 0.5,
                        "cand3" => 0.8,
                        _ => 0.0,
                    },
                ),
            )
        });
        let got = service.filter().filter(&request((1..=5).map(candidate).collect())).unwrap();
        assert_eq!(kept_ids(&got), vec![3]);
    }

    #[test]
    fn every_yes_below_four_tenths_is_not_found_with_nothing_kept() {
        let service = splitting(|body| Reply::json(200, &second_by_name(body, |_| 0.3, 0.05, |_| 0.399)));
        let got = service.filter().filter(&request((1..=5).map(candidate).collect())).unwrap();
        assert_eq!(service.received().len(), 2);
        assert_eq!(got.verdict, Verdict::NotFound);
        assert!(got.kept.is_empty(), "{:?}", got.kept);
    }

    #[test]
    fn the_second_look_adds_its_tokens_cost_and_model_to_the_usage() {
        let service = splitting(|body| {
            let mut answer = second_by_name(body, |_| 0.3, 0.05, |_| 0.9);
            answer["model"] = json!("jev-1.13.0");
            answer["usage"]["input_tokens"] = json!(2500);
            Reply::json(200, &answer)
        });
        let got = service.filter().filter(&request((1..=5).map(candidate).collect())).unwrap();
        assert_eq!(got.usage.input_tokens, 3500, "1000 of the choice and 2500 of the second look");
        // 3.500 tokens a US$ 0,042 o milhão: 147 milionésimos de dólar.
        assert_eq!(got.usage.cost_micro_usd, 147);
        assert_eq!(got.usage.model, "jev-1.13.0");
    }

    #[test]
    fn a_second_look_that_fails_falls_back_to_the_relative_cut_without_an_error() {
        // Recusa do serviço no segundo pedido: nada de erro, e o corte da
        // primeira etapa (só as chances acima de 0,04 ficam).
        let refused = FakeService::start(|number, body| {
            if number == 0 { Reply::json(200, &split_first(body)) } else { Reply::json(402, &json!({})) }
        });
        let got = refused.filter().filter(&request((1..=5).map(candidate).collect())).unwrap();
        assert_eq!(refused.received().len(), 2, "a refused key is not repeated");
        assert_eq!(got.verdict, Verdict::Split);
        assert_eq!(kept_ids(&got), vec![1, 2, 3]);
        assert_eq!(got.usage.input_tokens, 1000);

        // Resposta legível só pela metade: falta o sim de um finalista. Os
        // tokens da resposta que chegou contam.
        let unreadable = splitting(|body| {
            let mut answer = second_by_name(body, |_| 0.3, 0.05, |_| 0.9);
            answer["answers"].as_object_mut().unwrap().remove("fits_c001");
            Reply::json(200, &answer)
        });
        let got = unreadable.filter().filter(&request((1..=5).map(candidate).collect())).unwrap();
        assert_eq!((got.verdict, kept_ids(&got)), (Verdict::Split, vec![1, 2, 3]));
        assert_eq!(got.usage.input_tokens, 2000);
    }

    #[test]
    fn a_second_look_past_the_deadline_falls_back_to_the_relative_cut() {
        let service = FakeService::start(|number, body| {
            if number == 0 {
                return Reply::json(200, &split_first(body));
            }
            std::thread::sleep(Duration::from_millis(1500));
            Reply::json(200, &second_by_name(body, |_| 0.3, 0.05, |_| 0.9))
        });
        let mut filter = service.filter();
        filter.timeouts.response = Duration::from_millis(500);
        let started = Instant::now();
        let got = filter.filter(&request((1..=5).map(candidate).collect())).unwrap();
        assert!(started.elapsed() < Duration::from_millis(1200), "waited {:?}", started.elapsed());
        assert_eq!((got.verdict, kept_ids(&got)), (Verdict::Split, vec![1, 2, 3]));
    }

    #[test]
    fn the_share_of_the_request_travels_to_the_cut() {
        let service = FakeService::start(|_, body| {
            Reply::json(
                200,
                &choice_by_name(body, |name| if name == "cand2" { 0.5 } else { 0.4 }, 0.0, 0.9),
            )
        });
        let mut asked = request((1..=3).map(candidate).collect());
        asked.share = 0.5;
        let got = service.filter().filter(&asked).unwrap();
        let ids: Vec<i64> = got.kept.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![2, 1, 3], "0,4 is above half of 0,5 and the bank order breaks the tie");
        asked.share = 0.9;
        let got = service.filter().filter(&asked).unwrap();
        assert_eq!(got.kept.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2]);
    }

    /// Um candidato com 16 membros de mil caracteres: uns 16 mil caracteres
    /// no pedido. Doze deles enchem um pedido do limite.
    fn heavy_candidate(id: i64) -> FilterCandidate {
        FilterCandidate { members: vec!["x".repeat(1000); 16], ..candidate(id) }
    }

    #[test]
    fn a_batch_above_the_ceiling_splits_in_the_smallest_number_of_requests_in_the_bank_order() {
        for (count, requests) in [(12, 1), (13, 2), (24, 2), (25, 3), (36, 3), (37, 4)] {
            let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
            let candidates: Vec<FilterCandidate> = (1..=count).map(heavy_candidate).collect();
            service.filter().filter(&request(candidates)).unwrap();

            let sent = service.received();
            assert_eq!(sent.len(), requests, "{count} heavy candidates");
            let mut next = 0;
            for received in &sent {
                let payload = received.body.to_string();
                assert!(estimated_tokens(&payload) <= MAX_REQUEST_TOKENS, "{count}: a request passed the limit");
                for (id, _) in listed(&received.body) {
                    assert_eq!(id, format!("c{next:03}"), "{count}: the bank order broke");
                    next += 1;
                }
            }
            assert_eq!(next, count as usize, "{count}: every candidate goes once");
        }
    }

    #[test]
    fn split_requests_sum_the_usage_and_the_group_that_found_something_wins() {
        // O primeiro pedido não tem o certo e diz "nenhum destes"; o segundo
        // acha o cand20.
        let service = FakeService::start(|number, body| {
            let mut answer = if number == 0 {
                choice_by_name(body, |_| 0.01, 0.9, 0.9)
            } else {
                choice_by_name(body, |name| if name == "cand20" { 0.8 } else { 0.01 }, 0.05, 0.9)
            };
            answer["model"] = json!("jev-1.13.0");
            Reply::json(200, &answer)
        });
        let candidates: Vec<FilterCandidate> = (1..=24).map(heavy_candidate).collect();
        let got = service.filter().filter(&request(candidates)).unwrap();
        assert_eq!(service.received().len(), 2);
        assert_eq!(got.verdict, Verdict::Sure);
        assert_eq!(got.kept.iter().map(|s| s.id).collect::<Vec<_>>(), vec![20]);
        assert_eq!(got.usage.input_tokens, 2000);
        assert_eq!(got.usage.cost_micro_usd, 84);
        assert_eq!(got.usage.model, "jev-1.13.0");

        // Nenhum pedido acha: não achei.
        let none = FakeService::start(|_, body| Reply::json(200, &choice_by_name(body, |_| 0.01, 0.8, 0.9)));
        let candidates: Vec<FilterCandidate> = (1..=24).map(heavy_candidate).collect();
        let got = none.filter().filter(&request(candidates)).unwrap();
        assert_eq!(got.verdict, Verdict::NotFound);
        assert!(got.kept.is_empty());
    }

    #[test]
    fn merging_takes_the_lowest_none_and_confidence_of_the_groups_that_found_something() {
        let read = |id: i64, chance: f64, none: f64, confidence: f64| Choice {
            scores: vec![Scored { id, score: chance }],
            none,
            confidence,
            input_tokens: 0,
        };
        let merged = merge(vec![read(1, 0.7, 0.2, 0.8), read(2, 0.1, 0.9, 0.9), read(3, 0.6, 0.3, 0.75)]);
        assert_eq!(merged.scores.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 3]);
        assert!((merged.none - 0.2).abs() < f64::EPSILON);
        assert!((merged.confidence - 0.75).abs() < f64::EPSILON);
        let merged = merge(vec![read(1, 0.1, 0.9, 0.5), read(2, 0.1, 0.7, 0.6)]);
        assert!(merged.scores.is_empty());
        assert!((merged.none - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn no_candidates_send_no_request() {
        // Porta que ninguém ouve: um pedido que saísse viraria erro de rede.
        let got = JevFilter::at(test_key(), &closed_url()).filter(&request(Vec::new())).unwrap();
        assert_eq!(got.verdict, Verdict::NotFound);
        assert!(got.kept.is_empty());
        assert_eq!(got.usage.input_tokens, 0);
    }

    // -- as recusas e as repetições -------------------------------------------

    #[test]
    fn a_refused_key_or_missing_credit_fails_without_repeating() {
        for status in [401, 402] {
            let service = FakeService::start(move |_, _| {
                Reply::json(status, &json!({"error": format!("bad key {SECRET}")}))
            });
            let error = service.filter().filter(&request(vec![candidate(1)])).unwrap_err();
            assert_eq!(error, FilterError::Refused { status });
            assert_eq!(service.received().len(), 1, "HTTP {status} was repeated");
        }
    }

    #[test]
    fn too_many_requests_repeats_twice_and_gives_up() {
        let service = FakeService::start(|_, _| Reply {
            status: 429,
            headers: vec![("Retry-After", "0".to_string())],
            body: "{}".to_string(),
        });
        let error = service.filter().filter(&request(vec![candidate(1)])).unwrap_err();
        assert_eq!(error, FilterError::Refused { status: 429 });
        assert_eq!(service.received().len(), 3);
    }

    #[test]
    fn a_service_failure_is_repeated_and_the_next_answer_counts() {
        let service = FakeService::start(|number, body| {
            if number == 0 {
                Reply { status: 503, headers: vec![("Retry-After", "0".to_string())], body: String::new() }
            } else {
                Reply::json(200, &choice_by_name(body, |_| 0.7, 0.01, 0.9))
            }
        });
        let got = service.filter().filter(&request(vec![candidate(1)])).unwrap();
        assert_eq!(service.received().len(), 2);
        assert_eq!(got.kept.len(), 1);
    }

    #[test]
    fn the_wait_before_repeating_follows_the_service_up_to_five_seconds() {
        assert_eq!(retry_wait(Some("1.5")), Duration::from_millis(1500));
        assert_eq!(retry_wait(Some("5")), MAX_RETRY_WAIT);
        assert_eq!(retry_wait(Some("60")), MAX_RETRY_WAIT);
        assert_eq!(retry_wait(None), DEFAULT_RETRY_WAIT);
        assert_eq!(retry_wait(Some("Wed, 21 Oct 2015 07:28:00 GMT")), DEFAULT_RETRY_WAIT);
    }

    #[test]
    fn an_answer_without_answers_is_unreadable() {
        let service = FakeService::start(|_, _| Reply::json(200, &json!({"usage": {"input_tokens": 10}})));
        let error = service.filter().filter(&request(vec![candidate(1)])).unwrap_err();
        assert!(matches!(error, FilterError::Unreadable(_)), "{error:?}");
    }

    #[test]
    fn an_answer_missing_a_score_is_unreadable() {
        // Falta a chance de um candidato, a de `none` ou a confiança: cada
        // ausência é uma resposta ilegível.
        let answers = [
            json!({"q": {"confidence": 0.9, "probabilities": {"c000": 0.5, "none": 0.1}}}),
            json!({"q": {"confidence": 0.9, "probabilities": {"c000": 0.5, "c001": 0.4}}}),
            json!({"q": {"probabilities": {"c000": 0.5, "c001": 0.4, "none": 0.1}}}),
            json!({"q": {"confidence": 0.9}}),
            json!({"c00": {"noul": 0.5}}),
        ];
        for answer in answers {
            let service = FakeService::start(move |_, _| Reply::json(200, &json!({"answers": answer.clone()})));
            let error = service.filter().filter(&request(vec![candidate(1), candidate(2)])).unwrap_err();
            assert!(matches!(error, FilterError::Unreadable(_)), "{error:?}");
        }
    }

    #[test]
    fn one_failing_request_of_a_split_batch_fails_the_whole_filter() {
        let service = FakeService::start(|number, body| {
            if number == 1 {
                Reply::json(402, &json!({}))
            } else {
                Reply::json(200, &sure_answer(body))
            }
        });
        let candidates: Vec<FilterCandidate> = (1..=24).map(heavy_candidate).collect();
        let error = service.filter().filter(&request(candidates)).unwrap_err();
        assert_eq!(error, FilterError::Refused { status: 402 });
    }

    #[test]
    fn a_slow_service_times_out() {
        let service = FakeService::start(|_, body| {
            std::thread::sleep(Duration::from_millis(1500));
            Reply::json(200, &sure_answer(body))
        });
        let mut filter = service.filter();
        filter.timeouts.response = Duration::from_millis(200);
        let error = filter.filter(&request(vec![candidate(1)])).unwrap_err();
        assert_eq!(error, FilterError::Timeout);
    }

    #[test]
    fn the_deadline_covers_the_repeats_too() {
        // Cada tentativa falha em 300 ms; com o prazo de 500 ms, a segunda
        // tentativa leva só os 200 ms que faltam, e não há terceira.
        let service = FakeService::start(|_, _| {
            std::thread::sleep(Duration::from_millis(300));
            Reply { status: 503, headers: vec![("Retry-After", "0".to_string())], body: String::new() }
        });
        let mut filter = service.filter();
        filter.timeouts.response = Duration::from_millis(500);
        let started = Instant::now();
        let error = filter.filter(&request(vec![candidate(1)])).unwrap_err();
        let waited = started.elapsed();
        assert_eq!(error, FilterError::Timeout);
        assert_eq!(service.received().len(), 2);
        assert!(waited < Duration::from_millis(800), "waited {waited:?} past the 500 ms deadline");
    }

    #[test]
    fn a_repeat_whose_wait_passes_the_deadline_is_not_made() {
        // O serviço pede 2 s antes de repetir; com o prazo de 500 ms, a
        // recusa volta na hora, sem esperar nem repetir.
        let service = FakeService::start(|_, _| Reply {
            status: 429,
            headers: vec![("Retry-After", "2".to_string())],
            body: "{}".to_string(),
        });
        let mut filter = service.filter();
        filter.timeouts.response = Duration::from_millis(500);
        let started = Instant::now();
        let error = filter.filter(&request(vec![candidate(1)])).unwrap_err();
        let waited = started.elapsed();
        assert_eq!(error, FilterError::Refused { status: 429 });
        assert_eq!(service.received().len(), 1);
        assert!(waited < Duration::from_millis(500), "waited {waited:?} for a repeat that could not finish in time");
    }

    #[test]
    fn a_closed_port_is_a_network_error() {
        let filter = JevFilter::at(test_key(), &closed_url());
        let error = filter.filter(&request(vec![candidate(1)])).unwrap_err();
        assert!(matches!(error, FilterError::Network(_)), "{error:?}");
    }

    /// O endereço dado como fechado fica preso até o fim do processo: nenhum
    /// serviço de mentira, deste processo ou de outro que roda ao mesmo
    /// tempo, recebe a mesma porta, e quem se conecta a ela cai sem resposta.
    /// Uma porta solta logo depois do `bind` era tomada por um serviço vizinho,
    /// que então recebia o pedido de quem procurava a porta fechada e contava
    /// um pedido a mais.
    #[test]
    fn the_closed_address_stays_taken_and_drops_whoever_connects() {
        let url = closed_url();
        let address = url.trim_start_matches("http://").split('/').next().unwrap();
        let taken = TcpListener::bind(address).expect_err("another service could take the port of the closed address");
        assert_eq!(taken.kind(), ErrorKind::AddrInUse);

        let mut stream = TcpStream::connect(address).expect("the port takes the connection");
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut byte = [0u8; 1];
        match stream.read(&mut byte) {
            Ok(read) => assert_eq!(read, 0, "nothing is ever answered"),
            Err(error) => assert_eq!(error.kind(), ErrorKind::ConnectionReset, "{error:?}"),
        }
        assert_eq!(closed_url(), url, "every test asks the same taken address");
    }

    #[test]
    fn a_request_above_the_service_limit_is_not_sent() {
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        let mut asked = request(vec![candidate(1)]);
        asked.phrase = "x".repeat(250_000);
        let error = service.filter().filter(&asked).unwrap_err();
        assert!(matches!(error, FilterError::TooLarge { estimated_tokens } if estimated_tokens > MAX_REQUEST_TOKENS));
        assert!(service.received().is_empty());
    }

    #[test]
    fn the_estimate_counts_characters_per_token() {
        // A divisa do limite: 204.800 caracteres são 64 mil tokens e ainda
        // saem; com um caractere a mais, o pedido já passa do limite.
        assert_eq!(estimated_tokens(&"x".repeat(204_800)), 64_000);
        assert_eq!(estimated_tokens(&"x".repeat(204_801)), 64_001);
        assert_eq!(MAX_REQUEST_TOKENS, 64_000);
        // Conta caracteres, não bytes: 16 letras acentuadas são 16
        // caracteres (5 tokens), mas 32 bytes.
        assert_eq!(estimated_tokens(&"ã".repeat(16)), 5);
    }

    // -- a chave ---------------------------------------------------------------

    #[test]
    fn the_key_goes_only_in_the_authorization_header() {
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        service.filter().filter(&request(vec![candidate(1)])).unwrap();
        let sent = &service.received()[0];
        assert_eq!(sent.authorization, format!("Bearer {SECRET}"));
        assert!(!sent.body.to_string().contains(SECRET));
    }

    #[test]
    fn the_key_never_shows_in_debug_or_in_errors() {
        let key = test_key();
        assert!(!format!("{key:?}").contains(SECRET));
        let filter = JevFilter::at(key, &closed_url());
        assert!(!format!("{filter:?}").contains(SECRET));

        // O serviço devolve a chave no corpo da recusa e da resposta sem notas:
        // nenhuma mensagem de erro a repete.
        let echo = json!({"error": format!("key {SECRET} refused"), "usage": {"input_tokens": 1}});
        let refused = FakeService::start({
            let echo = echo.clone();
            move |_, _| Reply::json(401, &echo)
        });
        let unreadable = FakeService::start(move |_, _| Reply::json(200, &echo));
        let garbled = FakeService::start(|_, _| Reply {
            status: 200,
            headers: Vec::new(),
            body: format!("not json {SECRET}"),
        });
        let asked = request(vec![candidate(1)]);
        let mut errors = vec![
            refused.filter().filter(&asked).unwrap_err(),
            unreadable.filter().filter(&asked).unwrap_err(),
            garbled.filter().filter(&asked).unwrap_err(),
            JevFilter::at(test_key(), &closed_url()).filter(&asked).unwrap_err(),
        ];
        errors.extend([
            FilterError::MissingKey,
            FilterError::Timeout,
            FilterError::TooLarge { estimated_tokens: 70_000 },
            FilterError::KeyInGit,
        ]);
        for error in errors {
            assert!(!error.to_string().contains(SECRET), "{error}");
            assert!(!format!("{error:?}").contains(SECRET), "{error:?}");
        }
    }

    /// Um projeto numa pasta nova com `mustard.json` trazendo `key` em
    /// `jev.key`; com `git`, a pasta é um repositório, ainda sem o arquivo.
    fn project_with_key(key: &str, git: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if git {
            assert!(mustard_core::platform::git::run(dir.path(), &["init", "-q"]).ok);
        }
        let config = json!({"language": {"text": "pt-BR"}, "jev": {"key": key}});
        std::fs::write(dir.path().join("mustard.json"), config.to_string()).unwrap();
        dir
    }

    /// A chave como a busca a acha no projeto em `root`, com `env` no lugar
    /// do ambiente de quem roda o teste.
    fn project_key(root: &Path, env: Option<&str>) -> Result<LoadedKey, FilterError> {
        key_in(root, &ProjectConfig::load(root), env.map(str::to_string))
    }

    #[test]
    fn the_environment_key_comes_before_the_project_file() {
        let loaded = key_from(Some(" from-env ".to_string()), Some("from-file"), || false).unwrap();
        assert_eq!(loaded.key.0, "from-env");
        let loaded = key_from(Some("  ".to_string()), Some(" from-file "), || false).unwrap();
        assert_eq!(loaded.key.0, "from-file");
        let loaded = key_from(None, Some("from-file"), || false).unwrap();
        assert_eq!(loaded.key.0, "from-file");
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn without_a_key_anywhere_the_filter_has_no_key() {
        let never = || panic!("without a key in the file, git is not asked");
        assert_eq!(key_from(None, None, never).unwrap_err(), FilterError::MissingKey);
        assert_eq!(key_from(Some(" ".to_string()), Some("  "), never).unwrap_err(), FilterError::MissingKey);
        let loaded = key_from(Some("from-env".to_string()), None, never).unwrap();
        assert!(loaded.warning.is_none());
    }

    /// A chave em `jev.key` do `mustard.json` vai ao serviço, no cabeçalho, e
    /// liga o corte: as notas voltam e o que passa fica.
    #[test]
    fn the_key_in_the_project_file_turns_the_cut_on() {
        let project = project_with_key(SECRET, false);
        let loaded = project_key(project.path(), None).unwrap();
        assert!(loaded.warning.is_none());
        let service = FakeService::start(|_, body| {
            Reply::json(200, &choice_by_name(body, |name| if name == "cand1" { 0.9 } else { 0.05 }, 0.05, 0.9))
        });
        let got = JevFilter::at(loaded.key, &service.url).filter(&request(vec![candidate(1), candidate(2)])).unwrap();
        assert_eq!(service.received()[0].authorization, format!("Bearer {SECRET}"));
        assert_eq!(got.kept.first().map(|scored| scored.id), Some(1));

        let bare = tempfile::tempdir().unwrap();
        assert_eq!(project_key(bare.path(), None).unwrap_err(), FilterError::MissingKey);
    }

    /// O `mustard.json` que o git guarda não entrega a chave: sem a do
    /// ambiente, não há chave e o motivo é o git; com ela, vale a do ambiente,
    /// com o mesmo aviso.
    #[test]
    fn a_key_in_a_file_that_git_tracks_is_not_used_and_warns() {
        let project = project_with_key(SECRET, true);
        assert_eq!(project_key(project.path(), None).unwrap().key.0, SECRET, "out of git, the key counts");

        assert!(mustard_core::platform::git::run(project.path(), &["add", "mustard.json"]).ok);
        let refused = project_key(project.path(), None).unwrap_err();
        assert_eq!(refused, FilterError::KeyInGit);
        assert!(!refused.to_string().contains(SECRET));
        let with_env = project_key(project.path(), Some("from-env")).unwrap();
        assert_eq!(with_env.key.0, "from-env");
        assert_eq!(with_env.warning, Some(FilterError::KeyInGit));
    }

    // -- o serviço de verdade ----------------------------------------------------

    /// Manda um pedido real de 2 candidatos com a chave do ambiente e confere
    /// a nota e os tokens. Custa uma fração de centavo; roda só à mão, com
    /// `--ignored` e a chave no ambiente.
    #[test]
    #[ignore = "calls the paid service with the key from the environment"]
    fn the_real_service_scores_two_candidates() {
        let key = std::env::var(KEY_ENV).expect("the key in the environment");
        let mut target = candidate(1);
        target.name = "split_identifier".to_string();
        target.path = "packages/core/src/domain/project_map.rs".to_string();
        target.signature = "pub fn split_identifier(name: &str) -> String".to_string();
        target.documentation = "Splits an identifier into its words at case changes and separators.".to_string();
        let mut other = candidate(2);
        other.name = "open_pull_request".to_string();
        other.signature = "pub fn open_pull_request(title: &str) -> Result<u64, String>".to_string();
        let asked = FilterRequest {
            words: vec!["split_identifier".to_string()],
            phrase: "The function that splits an identifier into words.".to_string(),
            share: CUT_SHARE,
            candidates: vec![target, other],
        };
        let got = JevFilter::new(JevKey(key)).filter(&asked).unwrap();
        assert!(got.usage.input_tokens > 0);
        assert_eq!(got.kept.first().map(|s| s.id), Some(1));
        assert!(got.kept.iter().all(|s| (0.0..=1.0).contains(&s.score)));
    }
}
