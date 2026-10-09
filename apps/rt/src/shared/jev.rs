//! Typed Jev adapter for optional semantic recovery and wave planning.
//! Local rules recover plausible candidates first. Mandatory instructions,
//! exact searches, native authorization and final validation bypass Jev.
//! Candidate relevance uses independent Nouls with one rubric across batches;
//! Choice/Score remain suitable for task kind and ordinal size.
//! Full relevant evidence is partitioned to the provider's documented request
//! size, never silently clipped. Intrinsic task profiles and plausible running
//! relations are separate requests. A provider-independent service validates
//! responses, caches by actual inputs/revision and locks independent processes.
//! Cache hits do not create new usage. Physical attempts include retries;
//! missing usage remains unknown. No secret or response body enters metrics.

mod attempts;

use super::judgement::{JudgementProvider, JudgementRequest, JudgementService, Purpose};
use crate::shared::judgement::{Board, BoardTask, ItemsBoard, ItemsJudged, Judged, WaveJudge};
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Write as _};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use mustard_core::ProjectConfig;
use mustard_core::domain::map_filter::{
    FilterCandidate, FilterError, FilterRequest, FilterUsage, Filtered, MapFilter, Partial, Scored, Verdict, joined, judged,
};
use mustard_core::io::jev_gate;
use serde_json::{Map, Value, json};

use crate::shared::dag::{Judgement, TaskKind};
use crate::shared::jev_budget::Budget;
use crate::shared::paths::sensitive_pattern;
use crate::shared::secret::without_secrets;
use crate::shared::task_size;

// ---------------------------------------------------------------------------
// O serviço
// ---------------------------------------------------------------------------

/// O endereço do serviço.
pub const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";

/// A variável de ambiente que troca o endereço do serviço; sem ela, vale
/// [`JEV_URL`].
pub const URL_ENV: &str = "MUSTARD_JEV_URL";

/// O modelo pedido: uma versão fixa, a que a medida usou. A versão mais nova
/// do serviço mudaria as notas sem aviso; a troca vem com medida nova.
pub const JEV_MODEL: &str = "jev-1.13.0";

/// US$ por milhão de tokens de entrada; a saída não é cobrada.
pub const PRICE_PER_MILLION_INPUT_TOKENS: f64 = 0.042;

/// O maior pedido que o serviço aceita, em tokens: o estado e todas as
/// perguntas juntos. Um pedido montado que ainda passa dele não sai.
const MAX_REQUEST_TOKENS: u64 = 64_000;

/// O tamanho de cada pedido, em tokens estimados: o estado mais a maior
/// pergunta. O serviço aceita 32 mil; a estimativa por caracteres pode errar
/// para menos, e os 4 mil de folga a cobrem. A frase de quem procura, que vai
/// nas duas perguntas, entra na conta da maior delas.
const REQUEST_TOKENS: u64 = 28_000;

/// Quantas opções cabem numa pergunta de escolha do serviço.
const MAX_CHOICES: usize = 255;

/// Os caracteres que cada opção pesa na pergunta de escolha, o id com as
/// aspas e a vírgula, com folga.
const CHOICE_CHARS: usize = 14;

/// Caracteres por token, para estimar o pedido antes de mandar, como a
/// medida estimava.
const CHARS_PER_TOKEN: f64 = 3.2;

/// O menor espaço de uma parte do candidato grande demais, em caracteres.
/// Abaixo dele a pergunta sozinha já come o pedido, e nada se divide.
const MIN_PART_CHARS: usize = 1_000;

/// O rótulo que cada parte de um candidato leva no cabeçalho, ` (part 99 of
/// 99)`, com folga.
const PART_LABEL_CHARS: usize = 24;

/// Quantos caracteres da descrição que o agente deu à busca vão.
const DESCRIBED_CHARS: usize = 300;

/// Quantos caracteres da última fala do agente vão: os últimos.
const SAID_CHARS: usize = 500;

/// O maior arquivo lido atrás de código, em bytes; o maior que isso é dado,
/// não código.
const MAX_FILE_BYTES: u64 = 1_000_000;

// A rede de uma busca interativa: quem espera é o agente, no meio do
// trabalho. A medida esperava a resposta até 120 s e tentava até 8 vezes, o
// que serve a uma medição em lote, não a uma busca.

/// Quanto se espera para abrir a conexão.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Quanto a busca espera o filtro inteiro, repetições e esperas inclusive;
/// passado o prazo, a resposta vem do banco. Num período lento do serviço,
/// 100 buscas esperaram de 0,4 a 30 s, sem degrau no meio: com 10 s, 9 delas
/// iriam ao banco, e nenhuma esperaria mais que isso.
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
// Os textos do pedido, como a medida os mandava
// ---------------------------------------------------------------------------

/// A chave da pergunta de escolha: qual candidato é o código.
const WHERE_KEY: &str = "where";

/// A chave da pergunta de sim ou não: algum candidato é o código.
const EXISTS_KEY: &str = "exists";

/// O texto do sim da pergunta `exists`.
const EXISTS_YES: &str = "At least one candidate is the code the request asks for, or contains it.";

/// O texto do não da pergunta `exists`.
const EXISTS_NO: &str = "No candidate is the code the request asks for: they are only on related topics or only share some words with it.";

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

/// A chave do projeto em `root`: `env`, o valor de [`jev_gate::KEY_ENV`] que quem chama
/// lê do ambiente; sem ela, `jev.key` do `mustard.json` que `config` leu.
/// Sem nenhuma das duas, [`FilterError::MissingKey`]; com a do arquivo que o
/// git guarda, [`FilterError::KeyInGit`]. A regra é a de
/// [`mustard_core::io::jev_gate`].
pub fn key_in(root: &Path, config: &ProjectConfig, env: Option<String>) -> Result<LoadedKey, FilterError> {
    let found = jev_gate::find_key(root, config, env)?;
    Ok(LoadedKey { key: JevKey(found.value().to_string()), warning: found.warning().cloned() })
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
    /// O que sobra do teto do mês; sem ele, só nos testes, nada segura a
    /// chamada.
    budget: Option<Budget>,
    cache: Option<PathBuf>,
    spec: Option<String>,
}

impl JevFilter {
    /// O filtro com a chave do projeto, no endereço do serviço: o de
    /// [`JEV_URL`], ou o que [`URL_ENV`] diz. Toda chamada dele desconta de
    /// `budget`.
    #[must_use]
    pub fn new(root: &Path, key: JevKey, budget: Budget) -> Self {
        let url = std::env::var(URL_ENV).ok().map(|url| url.trim().to_string()).filter(|url| !url.is_empty());
        Self {
            budget: Some(budget),
            cache: Some(mustard_core::io::spec_events::spec_root(root).join(".claude/judgements")),
            ..Self::at(key, url.as_deref().unwrap_or(JEV_URL))
        }
    }

    /// Se o teto do mês já recusou uma chamada deste filtro, ou de uma cópia
    /// dele.
    #[must_use]
    pub fn held_by_budget(&self) -> bool {
        self.budget.as_ref().is_some_and(Budget::refused)
    }

    fn at(key: JevKey, endpoint: &str) -> Self {
        Self {
            key,
            endpoint: endpoint.to_string(),
            timeouts: Timeouts { connect: CONNECT_TIMEOUT, response: RESPONSE_TIMEOUT },
            budget: None,
            cache: None,
            spec: None,
        }
    }

    /// Um pedido, com as repetições, até `deadline`. Devolve o documento da
    /// resposta. Cada tentativa leva só o tempo que falta até o prazo, e a
    /// repetição cuja espera passaria do prazo não se faz.
    fn send(&self, request: &JudgementRequest<'_>, deadline: Instant) -> Result<Value, FilterError> {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_connect(Some(self.timeouts.connect)).http_status_as_error(false).build().new_agent();
        let auth = format!("Bearer {}", self.key.0);
        let mut retries = 0u32;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(FilterError::Timeout);
            }
            let started = Instant::now();
            let mut status = None;
            let mut wait = Duration::ZERO;
            let result = (|| {
                let mut response = agent
                    .post(&self.endpoint)
                    .config()
                    .timeout_global(Some(left))
                    .build()
                    .header("Authorization", &auth)
                    .header("Content-Type", "application/json")
                    .send(request.payload.as_bytes())
                    .map_err(transport_error)?;
                let code = response.status().as_u16();
                status = Some(code);
                if (200..300).contains(&code) {
                    let text = response.body_mut().read_to_string().map_err(transport_error)?;
                    let mut doc: Value = serde_json::from_str(&text).map_err(|_| FilterError::Unreadable("the body is not JSON".into()))?;
                    let object = doc.as_object_mut().ok_or_else(|| FilterError::Unreadable("the body is not a judgement object".into()))?;
                    object.insert("_attempts".into(), json!(retries + 1));
                    Ok(doc)
                } else {
                    wait = retry_wait(response.headers().get("retry-after").and_then(|v| v.to_str().ok()));
                    Err(FilterError::Refused { status: code })
                }
            })();
            self.record_attempt(request, u64::from(retries) + 1, status, &result, started.elapsed());
            if status.is_some_and(retryable) && retries < MAX_RETRIES && Instant::now() + wait < deadline {
                retries += 1;
                std::thread::sleep(wait);
                continue;
            }
            return result;
        }
    }

    /// Todos os pedidos de `payloads` ao mesmo tempo, cada um na sua linha de
    /// execução e todos até `deadline`. Os documentos voltam na ordem dos
    /// pedidos; a falha de um deles é a falha de todos, e vale a do primeiro
    /// na ordem. É a porta de toda chamada ao serviço: o custo estimado de
    /// todos os pedidos juntos se reserva no teto do mês antes de o primeiro
    /// sair, e, passando do que sobra, nenhum sai.
    fn send_all(&self, payloads: &[String], deadline: Instant) -> Result<Vec<Value>, FilterError> {
        self.send_all_for(payloads, deadline, Purpose::Search)
    }

    fn send_all_for(&self, payloads: &[String], deadline: Instant, purpose: Purpose) -> Result<Vec<Value>, FilterError> {
        let service = JudgementService::new(self, self.cache.as_deref());
        let cached: Vec<_> = payloads.iter().map(|payload| service.cached(&JudgementRequest { purpose, payload })).collect();
        if let Some(budget) = &self.budget {
            let estimate =
                payloads.iter().zip(&cached).filter(|(_, cached)| cached.is_none()).map(|(payload, _)| cost_micro_usd(estimated_tokens(payload))).sum();
            budget.reserve(estimate)?;
        }
        let initially_cached = cached.iter().map(Option::is_some).collect::<Vec<_>>();
        // Join every job before returning an error: successful physical
        // requests have already been recorded by the service, including
        // when a different batch fails.
        std::thread::scope(|scope| {
            let running: Vec<_> = payloads
                .iter()
                .zip(cached)
                .map(|(payload, cached)| {
                    let service = &service;
                    scope.spawn(move || match cached {
                        Some(doc) => Ok(doc),
                        None => service.evaluate(&JudgementRequest { purpose, payload }, deadline),
                    })
                })
                .collect();
            let results: Vec<_> =
                running.into_iter().map(|handle| handle.join().unwrap_or_else(|_| Err(FilterError::Network("a request thread failed".into())))).collect();
            if let Some(budget) = &self.budget {
                let unused = results
                    .iter()
                    .zip(&initially_cached)
                    .zip(payloads)
                    .filter(|((result, initial), _)| !**initial && result.as_ref().is_ok_and(|doc| doc.pointer("/_mustard/cached") == Some(&Value::Bool(true))))
                    .map(|(_, payload)| cost_micro_usd(estimated_tokens(payload)))
                    .sum();
                budget.release_cached(unused);
            }
            results.into_iter().collect()
        })
    }
}

impl WaveJudge for JevFilter {
    fn judge_backlog(&self, board: &Board) -> Result<Judged, FilterError> {
        JevFilter::judge_backlog(self, board)
    }
    fn judge_items(&self, board: &ItemsBoard) -> Result<ItemsJudged, FilterError> {
        JevFilter::judge_items(self, board)
    }
    fn held_by_budget(&self) -> bool {
        JevFilter::held_by_budget(self)
    }
}

impl JudgementProvider for JevFilter {
    fn identity(&self) -> String {
        format!("jev-v1:{}:{JEV_MODEL}", self.endpoint)
    }
    fn records_physical_attempts(&self) -> bool {
        self.cache.is_some()
    }
    fn evaluate(&self, request: &JudgementRequest<'_>, deadline: Instant) -> Result<Value, FilterError> {
        self.send(request, deadline)
    }
}

impl MapFilter for JevFilter {
    fn filter(&self, request: &FilterRequest) -> Result<Filtered, FilterError> {
        let started = Instant::now();
        // Um prazo só para a busca inteira: o agente espera.
        let deadline = started + self.timeouts.response;
        if request.candidates.is_empty() {
            return Ok(Filtered { verdict: Verdict::NotFound, kept: Vec::new(), usage: FilterUsage { model: String::new(), ..FilterUsage::default() } });
        }
        let context = Context::of(request);
        let batches = divide(request, &context)?;
        let independent = batches.len() > 1;
        let payloads: Vec<String> = batches
            .iter()
            .map(|batch| {
                if independent {
                    serde_json::to_string(&independent_body(&batch.state, &context, &batch.at))
                        .map_err(|_| FilterError::Unreadable("request did not serialize".into()))
                } else {
                    payload(batch, &context)
                }
            })
            .collect::<Result<_, _>>()?;
        for payload in &payloads {
            validate_wire_size(payload)?;
        }
        let docs = self.send_all(&payloads, deadline)?;
        let mut partials = Vec::with_capacity(batches.len());
        let mut input_tokens = 0;
        let mut model = String::new();
        for (batch, doc) in batches.iter().zip(&docs) {
            let answer = read_answer(doc, batch, &request.candidates)?;
            input_tokens += answer.input_tokens;
            partials.push(Partial { exists: answer.exists, scores: answer.scores });
            if model.is_empty() {
                model = model_of(doc);
            }
        }
        let (exists, notes) = if independent {
            // Binary relevance is absolute per candidate. Independent batches
            // never undergo Choice normalization across different universes.
            let mut scores = std::collections::BTreeMap::<i64, f64>::new();
            for part in &partials {
                for scored in &part.scores {
                    scores.entry(scored.id).and_modify(|n| *n = n.max(scored.score)).or_insert(scored.score);
                }
            }
            let mut notes: Vec<Scored> = scores.into_iter().map(|(id, score)| Scored { id, score }).collect();
            notes.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
            (partials.iter().map(|p| p.exists).fold(0.0, f64::max), notes)
        } else {
            joined(&partials)
        };
        let (verdict, kept) = judged(&notes, exists, request.cut);
        Ok(Filtered {
            verdict,
            kept,
            usage: FilterUsage {
                incomplete: docs.iter().any(unknown_usage),
                input_tokens,
                millis: started.elapsed().as_millis() as u64,
                cost_micro_usd: cost_micro_usd(input_tokens),
                requests: docs.iter().map(request_count).sum(),
                model,
            },
        })
    }
}

fn request_count(doc: &Value) -> u64 {
    if doc.pointer("/_mustard/cached") == Some(&Value::Bool(true)) { 0 } else { doc.get("_attempts").and_then(Value::as_u64).unwrap_or(1) }
}

fn unknown_usage(doc: &Value) -> bool {
    doc.pointer("/usage/input_tokens").and_then(Value::as_u64).is_none() || request_count(doc) > 1
}

/// O nome do modelo que a resposta `doc` diz ter respondido; vazio quando ela
/// não diz.
fn model_of(doc: &Value) -> String {
    doc.get("model").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string()
}

/// O custo de `input_tokens` tokens de entrada em milionésimos de dólar,
/// pelo preço de tabela: o preço por milhão de tokens é o preço de cada
/// token em milionésimos.
fn cost_micro_usd(input_tokens: u64) -> u64 {
    (input_tokens as f64 * PRICE_PER_MILLION_INPUT_TOKENS).round() as u64
}

/// O que a resposta de um pedido disse.
#[derive(Debug, Clone, PartialEq)]
struct Answer {
    /// A chance de cada candidato do pedido ser o código, na ordem dele.
    scores: Vec<Scored>,
    /// A chance de algum candidato do pedido ser o código.
    exists: f64,
    /// Os tokens de entrada que o pedido custou.
    input_tokens: u64,
}

/// A resposta do pedido `batch`, lida do documento `doc`: a chance de cada
/// candidato dele, de `candidates`, na escolha `where` e a de `exists`. Falta
/// de `answers`, de uma chance ou do `exists` é resposta ilegível.
fn read_answer(doc: &Value, batch: &Batch, candidates: &[FilterCandidate]) -> Result<Answer, FilterError> {
    let answers = doc.get("answers").and_then(Value::as_object).ok_or_else(|| FilterError::Unreadable("no answers".to_string()))?;
    let chances = answers.get(WHERE_KEY).and_then(|answer| answer.get("probabilities")).and_then(Value::as_object);
    let mut scores = Vec::with_capacity(batch.at.len());
    for &at in &batch.at {
        let id = candidate_id(at);
        let score = chances
            .and_then(|chances| chances.get(&id))
            .and_then(Value::as_f64)
            .or_else(|| answers.get(&id).and_then(|answer| answer.get("noul")).and_then(Value::as_f64))
            .ok_or_else(|| FilterError::Unreadable(format!("no chance for {id}")))?;
        scores.push(Scored { id: candidates[at].id, score });
    }
    let exists = answers
        .get(EXISTS_KEY)
        .and_then(|answer| answer.get("noul"))
        .and_then(Value::as_f64)
        .ok_or_else(|| FilterError::Unreadable("no answer to the existence".to_string()))?;
    Ok(Answer { scores, exists, input_tokens: doc.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0) })
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

/// Os tokens estimados de `chars` caracteres.
fn tokens_of(chars: usize) -> u64 {
    (chars as f64 / CHARS_PER_TOKEN).ceil() as u64
}

/// Os tokens estimados de um pedido, pelos caracteres.
fn estimated_tokens(payload: &str) -> u64 {
    tokens_of(payload.chars().count())
}

// ---------------------------------------------------------------------------
// O pedido
// ---------------------------------------------------------------------------

/// O que o agente queria, já sem segredo: a frase (ou, sem ela, as palavras),
/// a descrição que ele deu à busca e a última fala dele. É o que acompanha as
/// duas perguntas de cada pedido.
struct Context {
    request: String,
    described: String,
    said: String,
}

impl Context {
    fn of(request: &FilterRequest) -> Self {
        let asked = if request.phrase.trim().is_empty() { request.words.join(" ") } else { request.phrase.clone() };
        let said = squash(&without_secrets(&request.said));
        let kept = said.chars().count().saturating_sub(SAID_CHARS);
        Self {
            request: without_secrets(&asked),
            described: without_secrets(request.described.trim()).chars().take(DESCRIBED_CHARS).collect(),
            said: said.chars().skip(kept).collect(),
        }
    }

    /// O fim das duas instruções: a descrição e a fala, cada uma só quando
    /// existe.
    fn suffix(&self) -> String {
        let mut text = String::new();
        if !self.described.is_empty() {
            let _ = write!(text, " The agent described this search as: \"{}\".", self.described);
        }
        if !self.said.is_empty() {
            let _ = write!(text, " Just before it, the agent wrote: \"{}\".", self.said);
        }
        text
    }

    /// A instrução da pergunta de escolha.
    fn where_instructions(&self) -> String {
        format!("Which candidate is the code that answers this request from a coding agent: \"{}\"?{}", self.request, self.suffix())
    }

    /// A instrução da pergunta de existência.
    fn exists_instructions(&self) -> String {
        format!("Does any candidate contain the code that answers this request from a coding agent: \"{}\"?{}", self.request, self.suffix())
    }

    /// Os caracteres da maior pergunta de um pedido com `choices` opções na
    /// escolha: é ela que entra na conta do limite do serviço.
    fn question_chars(&self, choices: usize) -> usize {
        let choice = self.where_instructions().chars().count() + choices * CHOICE_CHARS;
        let exists = self.exists_instructions().chars().count() + EXISTS_YES.len() + EXISTS_NO.len();
        choice.max(exists)
    }

    /// Quantos caracteres de estado cabem num pedido com `choices` opções,
    /// depois da maior pergunta.
    fn room(&self, choices: usize) -> usize {
        ((REQUEST_TOKENS as f64 * CHARS_PER_TOKEN).round() as usize).saturating_sub(self.question_chars(choices))
    }

    /// Se um estado de `state_chars` caracteres com `choices` candidatos cabe
    /// num pedido: dentro de [`REQUEST_TOKENS`] e de [`MAX_CHOICES`].
    fn fits(&self, state_chars: usize, choices: usize) -> bool {
        choices <= MAX_CHOICES && state_chars <= self.room(choices)
    }
}

/// Um pedido da divisão: o estado e os candidatos dele, pela posição na lista.
#[derive(Debug, Default)]
struct Batch {
    state: String,
    /// A posição na lista de cada candidato do pedido.
    at: Vec<usize>,
    /// Os caracteres do estado.
    chars: usize,
}

/// Check the actual serialized request, including repeated binary questions.
/// Packing estimates are insufficient when a different primitive is used.
fn validate_wire_size(payload: &str) -> Result<(), FilterError> {
    let document: Value = serde_json::from_str(payload).map_err(|_| FilterError::Unreadable("invalid request".into()))?;
    let state = document["state"].as_str().map_or_else(|| document["state"].to_string().chars().count(), |s| s.chars().count());
    let longest = document["questions"]
        .as_object()
        .into_iter()
        .flat_map(|questions| questions.values())
        .map(|question| question.to_string().chars().count())
        .max()
        .unwrap_or_default();
    if tokens_of(payload.chars().count()) > MAX_REQUEST_TOKENS || tokens_of(state + longest) > REQUEST_TOKENS {
        return Err(FilterError::TooLarge { estimated_tokens: tokens_of(payload.chars().count()) });
    }
    Ok(())
}

impl Batch {
    /// O pedido com o bloco de `chars` caracteres a mais, depois de uma linha
    /// em branco quando já há outro.
    fn push(&mut self, at: usize, block: &str, chars: usize) {
        if !self.at.is_empty() {
            self.state.push_str("\n\n");
            self.chars += 2;
        }
        self.state.push_str(block);
        self.chars += chars;
        self.at.push(at);
    }

    /// Os caracteres do estado se o bloco de `chars` caracteres entrasse.
    fn chars_with(&self, chars: usize) -> usize {
        if self.at.is_empty() { chars } else { self.chars + 2 + chars }
    }
}

/// A lista inteira de `request` repartida em pedidos, na ordem dela: os
/// candidatos enchem cada pedido até onde o serviço aceita, e o que sozinho
/// não cabe em um vai em pedidos só dele, em partes. Sem nenhum corte: todo
/// candidato está em pelo menos um pedido, com tudo o que tem. Quando nem a
/// pergunta cabe, nada se divide e o pedido não sai
/// ([`FilterError::TooLarge`]).
fn divide(request: &FilterRequest, context: &Context) -> Result<Vec<Batch>, FilterError> {
    let mut sources = Sources { root: &request.root, files: HashMap::new() };
    let mut batches = Vec::new();
    let mut open = Batch::default();
    for (at, candidate) in request.candidates.iter().enumerate() {
        let head = format!("{}| {}:{}-{}", candidate_id(at), without_secrets(&candidate.path), candidate.line, candidate.end_line);
        let lines = block_lines(candidate, &mut sources);
        let block = std::iter::once(head.as_str()).chain(lines.iter().map(String::as_str)).collect::<Vec<_>>().join("\n");
        let chars = block.chars().count();
        if context.fits(chars, 1) {
            if !open.at.is_empty() && !context.fits(open.chars_with(chars), open.at.len() + 1) {
                batches.push(std::mem::take(&mut open));
            }
            open.push(at, &block, chars);
        } else {
            if !open.at.is_empty() {
                batches.push(std::mem::take(&mut open));
            }
            for part in parts(&head, &lines, context)? {
                let chars = part.chars().count();
                batches.push(Batch { state: part, at: vec![at], chars });
            }
        }
    }
    if !open.at.is_empty() {
        batches.push(open);
    }
    Ok(batches)
}

/// O candidato que sozinho passa do pedido, em partes seguidas: cada uma com
/// o cabeçalho dele, o mesmo id e o rótulo da parte, e as linhas de depois do
/// cabeçalho em ordem, sem perder nenhuma. A linha maior que a parte se parte
/// também.
fn parts(head: &str, lines: &[String], context: &Context) -> Result<Vec<String>, FilterError> {
    let space = context.room(1).saturating_sub(head.chars().count() + PART_LABEL_CHARS + 1);
    if space < MIN_PART_CHARS {
        return Err(FilterError::TooLarge { estimated_tokens: tokens_of(context.question_chars(1)) });
    }
    let mut chunks: Vec<Vec<String>> = vec![Vec::new()];
    let mut used = 0;
    for line in lines {
        for piece in char_chunks(line, space - 1) {
            let need = piece.chars().count() + 1;
            if used + need > space && used > 0 {
                chunks.push(Vec::new());
                used = 0;
            }
            if let Some(chunk) = chunks.last_mut() {
                chunk.push(piece);
            }
            used += need;
        }
    }
    let total = chunks.len();
    Ok(chunks.into_iter().enumerate().map(|(at, chunk)| format!("{head} (part {} of {total})\n{}", at + 1, chunk.join("\n"))).collect())
}

/// `line` em pedaços de até `size` caracteres; a linha vazia é um pedaço só.
fn char_chunks(line: &str, size: usize) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars.chunks(size.max(1)).map(|chunk| chunk.iter().collect()).collect()
}

/// O texto de um pedido: o modelo, o estado e as duas perguntas.
fn payload(batch: &Batch, context: &Context) -> Result<String, FilterError> {
    let text =
        serde_json::to_string(&body(&batch.state, context, &batch.at)).map_err(|_| FilterError::Unreadable("the request did not serialize".to_string()))?;
    let estimated = estimated_tokens(&text);
    if estimated > MAX_REQUEST_TOKENS {
        return Err(FilterError::TooLarge { estimated_tokens: estimated });
    }
    Ok(text)
}

/// Independent relevance questions retain the same query/rubric across
/// partitions; each id refers only to evidence present in this state.
fn independent_body(state: &str, context: &Context, at: &[usize]) -> Value {
    let mut questions = Map::new();
    for &position in at {
        let id = candidate_id(position);
        questions.insert(id.clone(), json!({"type":"noul", "instructions":format!(
            "Does the evidence shown for `{id}` answer this search? Judge this candidate independently of the other candidates. {}", context.where_instructions())}));
    }
    questions.insert(EXISTS_KEY.into(), json!({"type":"noul","instructions":context.exists_instructions()}));
    json!({"model":JEV_MODEL,"state":state,"questions":questions})
}

/// O corpo de um pedido: o modelo, o estado e as duas perguntas, cada uma com
/// a frase e o contexto. As opções da escolha são os ids dos candidatos de
/// `at`, sem descrição.
fn body(state: &str, context: &Context, at: &[usize]) -> Value {
    let mut criteria = Map::new();
    for &position in at {
        criteria.insert(candidate_id(position), Value::Null);
    }
    let mut questions = Map::new();
    questions.insert(
        WHERE_KEY.to_string(),
        json!({
            "type": "choice",
            "instructions": context.where_instructions(),
            "criteria": Value::Object(criteria),
        }),
    );
    questions.insert(
        EXISTS_KEY.to_string(),
        json!({
            "type": "noul",
            "instructions": context.exists_instructions(),
            "criteria": { "true": EXISTS_YES, "false": EXISTS_NO },
        }),
    );
    json!({ "model": JEV_MODEL, "state": state, "questions": Value::Object(questions) })
}

/// O id de um candidato no pedido: `c000` em diante, pela posição dele na
/// lista.
fn candidate_id(at: usize) -> String {
    format!("c{at:03}")
}

/// As linhas do bloco de um candidato depois do cabeçalho, cada uma com
/// quatro espaços de recuo: a documentação inteira, o título de cada commit
/// que mudou a declaração, cada comentário de revisão preso a ela, os nomes do
/// que ela chama e o código dela, do início ao fim. Cada texto sai sem os
/// segredos antes de qualquer divisão: a divisão não parte um segredo num
/// trecho que a procura já não reconhece.
fn block_lines(candidate: &FilterCandidate, sources: &mut Sources<'_>) -> Vec<String> {
    let clean = |text: &str| squash(&without_secrets(text));
    let mut lines = Vec::new();
    let documentation = clean(&candidate.documentation);
    if !documentation.is_empty() {
        lines.push(format!("    // {documentation}"));
    }
    for (label, texts) in [("commit", &candidate.commits), ("review", &candidate.reviews)] {
        for text in texts {
            let text = clean(text);
            if !text.is_empty() {
                lines.push(format!("    // {label}: {text}"));
            }
        }
    }
    let calls: Vec<String> = candidate.calls.iter().map(|name| clean(name)).filter(|name| !name.is_empty()).collect();
    if !calls.is_empty() {
        lines.push(format!("    // calls: {}", calls.join(", ")));
    }
    lines.extend(sources.excerpt(candidate));
    lines
}

/// Os arquivos do projeto de onde vem o código dos candidatos, cada um lido
/// uma vez.
struct Sources<'a> {
    root: &'a Path,
    /// O texto de cada arquivo já pedido; `None` no que não se lê.
    files: HashMap<String, Option<String>>,
}

impl Sources<'_> {
    /// As linhas da declaração de `candidate`, da primeira à última dela:
    /// cada uma sem o espaço do fim e com quatro espaços de recuo. Vazio
    /// quando o arquivo não se lê.
    fn excerpt(&mut self, candidate: &FilterCandidate) -> Vec<String> {
        let root = self.root;
        let Some(text) = self.files.entry(candidate.path.clone()).or_insert_with(|| read_source(root, &candidate.path)) else {
            return Vec::new();
        };
        let start = candidate.line.saturating_sub(1) as usize;
        let count = candidate.end_line.max(candidate.line) as usize - start;
        let taken: Vec<&str> = text.lines().skip(start).take(count).collect();
        if taken.is_empty() {
            return Vec::new();
        }
        without_secrets(&taken.join("\n")).split('\n').map(|line| format!("    {}", line.trim_end())).collect()
    }
}

/// O texto do arquivo `path` do projeto em `root`. `None` no caminho que sai
/// do projeto (absoluto ou com `..`), no de arquivo sensível (credenciais,
/// chaves), no que falta, não abre ou passa de [`MAX_FILE_BYTES`]; um byte que
/// não é texto vira o caractere de troca.
fn read_source(root: &Path, path: &str) -> Option<String> {
    let relative = Path::new(path);
    if relative.is_absolute() || relative.components().any(|part| matches!(part, Component::ParentDir)) || sensitive_pattern(path).is_some() {
        return None;
    }
    let file = root.join(relative);
    if std::fs::metadata(&file).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read(&file).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Os espaços juntados: toda sequência de brancos vira um espaço, e as
/// pontas somem.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// O backlog julgado para a montagem da onda
// ---------------------------------------------------------------------------

/// O que é uma onda, para o Jev.
const WAVE_DEFINITION: &str = "A wave is the complete package of tasks one agent receives. Eligible producers and consumers may belong to different kinds when their dependencies and the complete package budget permit it. Two concurrent waves must not change the same file.";

/// Quando duas obras mudam a mesma coisa, para a pergunta de bloqueio.
const SAME_CHANGE: &str = "Two works change the same thing when both change the same feature, behavior or text, so that doing them at the same time would conflict in meaning, even in different files.";

fn kind_meaning(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Defect => "Fixes behavior that is wrong for the user or breaks something",
        TaskKind::Feature => "Adds or changes behavior the user will notice",
        TaskKind::TextFix => "Fixes comments, names or wording",
        TaskKind::RemoveUnused => "Removes code, keys or dependencies that nothing uses",
        TaskKind::TestCleanup => "Makes tests faster, shorter or less repeated without changing the product",
    }
}

fn task_key(id: u64) -> String {
    format!("t{id}")
}
fn wave_key(n: u64) -> String {
    format!("w{n}")
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WavesJev {
    pub(crate) filter: Option<std::sync::Arc<dyn WaveJudge>>,
    pub(crate) key_in_git: bool,
    pub(crate) over_budget: bool,
}
impl WavesJev {
    pub(crate) fn held_by_budget(&self) -> bool {
        self.over_budget || self.filter.as_ref().is_some_and(|filter| filter.held_by_budget())
    }
}
pub(crate) fn for_waves(root: &Path, spec: &str) -> WavesJev {
    for_purpose(root, "wave-planning", Some(spec))
}
pub(crate) fn for_context(root: &Path, spec: &str) -> WavesJev {
    for_purpose(root, "context", Some(spec))
}
fn for_purpose(root: &Path, purpose: &str, spec: Option<&str>) -> WavesJev {
    let config = ProjectConfig::load(root);
    if cfg!(test) {
        return WavesJev { filter: None, ..purpose_filter(root, &config, None, None, purpose, spec) };
    }
    let ledger = mustard_core::io::spend::machine_dir();
    purpose_filter(root, &config, std::env::var(jev_gate::KEY_ENV).ok(), ledger.as_deref(), purpose, spec)
}
#[cfg(test)]
fn waves_filter(root: &Path, config: &ProjectConfig, env: Option<String>, ledger: Option<&Path>) -> WavesJev {
    purpose_filter(root, config, env, ledger, "wave-planning", None)
}
fn purpose_filter(root: &Path, config: &ProjectConfig, env: Option<String>, ledger: Option<&Path>, purpose: &str, spec: Option<&str>) -> WavesJev {
    if !config.ai_fallback_enabled() || !jev_gate::setting_allows(config.judgement_filter(purpose)) {
        return WavesJev::default();
    }
    match key_in(root, config, env) {
        Ok(loaded) => {
            let budget = Budget::open(root, config, ledger);
            // Even a spent budget can reuse a valid cached answer. Missing
            // answers still pass through the existing reservation gate.
            let mut filter = JevFilter::new(root, loaded.key, budget);
            filter.spec = spec.map(str::to_string);
            WavesJev { over_budget: filter.budget.as_ref().is_some_and(Budget::is_spent), filter: Some(std::sync::Arc::new(filter)), ..WavesJev::default() }
        }
        Err(error) => WavesJev { key_in_git: error == FilterError::KeyInGit, ..WavesJev::default() },
    }
}
impl JevFilter {
    fn judge_backlog(&self, board: &Board) -> Result<Judged, FilterError> {
        let started = Instant::now();
        let payloads = backlog_payloads(board)?;
        let docs = self.send_all_for(&payloads, started + self.timeouts.response, Purpose::WavePlanning)?;
        let mut answers = Map::new();
        for doc in &docs {
            if let Some(part) = doc["answers"].as_object() {
                answers.extend(part.clone());
            }
        }
        let input_tokens = docs.iter().filter_map(|d| d.pointer("/usage/input_tokens").and_then(Value::as_u64)).sum();
        Ok(Judged {
            tasks: read_judgements(&json!({"answers":answers}), board)?,
            usage: FilterUsage {
                input_tokens,
                incomplete: docs.iter().any(unknown_usage),
                millis: started.elapsed().as_millis() as u64,
                cost_micro_usd: cost_micro_usd(input_tokens),
                requests: docs.iter().map(request_count).sum(),
                model: docs.first().map(model_of).unwrap_or_default(),
            },
        })
    }
}
fn encode_board(state: Value, questions: Map<String, Value>) -> Result<String, FilterError> {
    let payload = json!({"model":JEV_MODEL,"state":state,"questions":questions}).to_string();
    validate_wire_size(&payload)?;
    Ok(payload)
}
fn backlog_payloads(board: &Board) -> Result<Vec<String>, FilterError> {
    let mut payloads = Vec::new();
    for task in &board.backlog {
        let intrinsic = Board { running: Vec::new(), backlog: vec![task.clone()] };
        let (state, questions) = board_parts(&intrinsic);
        payloads.push(encode_board(state, questions)?);
    }
    for wave in &board.running {
        let plausible = board.backlog.iter().filter(|task| wave.tasks.iter().any(|other| related_tasks(task, other))).cloned().collect::<Vec<_>>();
        if plausible.is_empty() {
            continue;
        }
        let running = wave.tasks.iter().filter(|other| plausible.iter().any(|task| related_tasks(task, other))).cloned().collect();
        let pair = Board { backlog: plausible, running: vec![crate::shared::judgement::BoardWave { n: wave.n, tasks: running }] };
        let (mut state, mut questions) = board_parts(&pair);
        questions.retain(|key, _| key.starts_with("blk_"));
        if let Some(definition) = state["definition"].as_object_mut() {
            definition.remove("kinds");
        }
        payloads.push(encode_board(state, questions)?);
    }
    Ok(payloads)
}
/// Only shared files, a known directory, or explicit dependencies justify a
/// semantic interference question. Two absent parents are not a relation.
fn related_tasks(a: &BoardTask, b: &BoardTask) -> bool {
    a.relation(b).semantic_interference()
}
fn board_parts(board: &Board) -> (Value, Map<String, Value>) {
    let view = |task: &BoardTask| {
        json!({
            "title": without_secrets(&task.title),
            "text": without_secrets(&task.text),
            "agent": without_secrets(&task.agent),
            "files": task.files,
            "reads": task.reads,
        })
    };
    let mut kinds = Map::new();
    for kind in TaskKind::ALL {
        kinds.insert(kind.key().to_string(), json!(kind_meaning(kind)));
    }
    let mut running = Map::new();
    for wave in &board.running {
        let tasks: Map<String, Value> = wave.tasks.iter().map(|task| (task_key(task.id), view(task))).collect();
        running.insert(wave_key(wave.n), Value::Object(tasks));
    }
    let mut backlog = Map::new();
    for task in &board.backlog {
        let mut shown = view(task);
        shown["depends_on"] = json!(task.depends_on.iter().map(|title| without_secrets(title)).collect::<Vec<_>>());
        backlog.insert(task_key(task.id), shown);
    }
    let relations: Map<String, Value> = board.backlog.iter().flat_map(|task| board.running.iter().flat_map(move |wave|
        wave.tasks.iter().filter(move |other| related_tasks(task, other)).map(move |other|
            (format!("{}:{}:{}", wave_key(wave.n), task_key(task.id), task_key(other.id)), json!(task.relation(other)))))).collect();
    let state = json!({
        "relations": relations,
        "definition": { "wave": WAVE_DEFINITION, "same_change": SAME_CHANGE, "kinds": kinds.clone() },
        "running": running,
        "backlog": backlog,
    });
    let mut questions = Map::new();
    for task in &board.backlog {
        let key = task_key(task.id);
        questions.insert(
            format!("tipo_{key}"),
            json!({
                "type": "choice",
                "instructions": { "question": format!("What kind of work is `backlog.{key}`?") },
                "criteria": Value::Object(kinds.clone()),
            }),
        );
        questions.insert(format!("tam_{key}"), task_size::question(&key));
        for wave in &board.running {
            if !wave.tasks.iter().any(|running| related_tasks(task, running)) {
                continue;
            }
            let wave = wave_key(wave.n);
            questions.insert(
                format!("blk_{wave}_{key}"),
                json!({
                    "type": "noul",
                    "instructions": format!(
                        "Do `backlog.{key}` and `running.{wave}` change the same thing, as `definition.same_change` defines it?"
                    ),
                }),
            );
        }
    }
    (state, questions)
}

fn read_judgements(doc: &Value, board: &Board) -> Result<BTreeMap<u64, Judgement>, FilterError> {
    let answers = doc.get("answers").and_then(Value::as_object).ok_or_else(|| FilterError::Unreadable("no answers".to_string()))?;
    let missing = |what: String| FilterError::Unreadable(format!("no answer for {what}"));
    let mut judged = BTreeMap::new();
    for task in &board.backlog {
        let key = task_key(task.id);
        let asked = format!("tipo_{key}");
        let kind = answers.get(&asked).ok_or_else(|| missing(asked.clone()))?;
        let name = kind.get("choice").and_then(Value::as_str).unwrap_or_default();
        let kind_of = TaskKind::from_key(name).ok_or_else(|| FilterError::Unreadable(format!("{asked} is not a kind of work")))?;
        let confidence = kind.get("confidence").and_then(Value::as_f64).ok_or_else(|| FilterError::Unreadable(format!("no confidence for {asked}")))?;
        let asked_size = format!("tam_{key}");
        let size = answers.get(&asked_size).and_then(task_size::level_in).ok_or_else(|| missing(asked_size.clone()))?;
        let mut clash = 0.0_f64;
        for wave in &board.running {
            if !wave.tasks.iter().any(|running| related_tasks(task, running)) {
                continue;
            }
            let asked = format!("blk_{}_{key}", wave_key(wave.n));
            let chance = answers.get(&asked).and_then(|answer| answer.get("noul")).and_then(Value::as_f64).ok_or_else(|| missing(asked.clone()))?;
            clash = clash.max(chance);
        }
        judged.insert(task.id, Judgement { kind: kind_of, confidence, clash, size });
    }
    Ok(judged)
}

/// Os caracteres que cada entrada do estado e de cada pergunta pesa além do
/// que ela diz: os dois pontos, as aspas e a vírgula, com folga.
const ENTRY_OVERHEAD_CHARS: usize = 8;

/// Um item combinado como o Jev o vê: o número, o título e o começo do texto.
/// Nenhum arquivo, onda ou tarefa que o liga: a estrutura que a rodada já usa
/// fica fora do estado.
/// A chave de um item no estado e nas perguntas.
fn item_key(id: u64) -> String {
    format!("i{id}")
}

impl JevFilter {
    /// Julga os itens de `board` para a onda das tarefas dele: o estado leva
    /// as tarefas (`task`) e os itens (`items`), e há uma pergunta de sim ou
    /// não por item — ele governa algo que as tarefas mudam ou testam? O
    /// estado que passa do que o serviço aceita sai em partes, todas com as
    /// mesmas tarefas, e as partes vão ao mesmo tempo. Falta de uma resposta
    /// ou uma chance que não é número entre 0 e 1 é resposta ilegível.
    ///
    /// # Errors
    /// A parte que passa do que o serviço aceita mesmo sozinha, a falha do
    /// serviço e a resposta ilegível. Quem chama deixa o pedido como é por
    /// padrão.
    pub(crate) fn judge_items(&self, board: &ItemsBoard) -> Result<ItemsJudged, FilterError> {
        let started = Instant::now();
        let parts = items_payloads(board)?;
        let payloads: Vec<String> = parts.iter().map(|(payload, _)| payload.clone()).collect();
        let docs = self.send_all_for(&payloads, started + self.timeouts.response, Purpose::Context)?;
        let mut chances = BTreeMap::new();
        let mut input_tokens = 0;
        let mut model = String::new();
        for ((_, ids), doc) in parts.iter().zip(&docs) {
            read_item_chances(doc, ids, &mut chances)?;
            input_tokens += doc.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0);
            if model.is_empty() {
                model = model_of(doc);
            }
        }
        Ok(ItemsJudged {
            chances,
            usage: FilterUsage {
                incomplete: docs.iter().any(unknown_usage),
                input_tokens,
                millis: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                cost_micro_usd: cost_micro_usd(input_tokens),
                requests: docs.iter().map(request_count).sum(),
                model,
            },
        })
    }
}

/// Os pedidos de um quadro de itens, cada um com os números dos itens que
/// pergunta. Os itens se repartem na ordem em que vêm, enquanto o estado e a
/// maior pergunta cabem num pedido e o pedido inteiro cabe no que o serviço
/// aceita; as tarefas vão em todas as partes. Quadro sem item não pede nada.
fn items_payloads(board: &ItemsBoard) -> Result<Vec<(String, Vec<u64>)>, FilterError> {
    let task: Map<String, Value> = board
        .tasks
        .iter()
        .map(|task| {
            let view = json!({
                "title": without_secrets(&task.title),
                "text": without_secrets(&task.text),
                "agent": without_secrets(&task.agent),
                "files": task.files,
            });
            (task_key(task.id), view)
        })
        .collect();
    let task_chars = Value::Object(task.clone()).to_string().chars().count();
    let criteria = json!({
        "true": { "what": "The item describes a rule or decision about the part of the product or process that the task edits" },
        "false": { "what": "The item is about another part of the product or another kind of work" },
    });
    let entries: Vec<(u64, Value, Value)> = board
        .items
        .iter()
        .map(|item| {
            let key = item_key(item.id);
            let shown = json!({ "title": without_secrets(&item.title), "text": without_secrets(&item.text) });
            let question = json!({
                "type": "noul",
                "instructions": {
                    "question": format!("Does `items.{key}` govern something that `task` changes or tests?"),
                    "focus": "Answer only for what the task changes or tests. Items about other parts of the product or other kinds of work are false.",
                },
                "criteria": criteria.clone(),
            });
            (item.id, shown, question)
        })
        .collect();
    let mut parts: Vec<Vec<usize>> = Vec::new();
    let (mut state, mut questions, mut longest) = (task_chars, 0, 0);
    for (at, (_, shown, question)) in entries.iter().enumerate() {
        // Cada entrada leva também a chave e as vírgulas, e o pedido inteiro
        // ainda tem o modelo e as chaves de fora: a conta deixa uma folga.
        let overhead = item_key(entries[at].0).chars().count() + ENTRY_OVERHEAD_CHARS;
        let (shown_chars, question_chars) = (shown.to_string().chars().count() + overhead, question.to_string().chars().count() + overhead);
        let too_big = |state: usize, questions: usize, longest: usize| {
            tokens_of(state + longest) > REQUEST_TOKENS || tokens_of(state + questions) > MAX_REQUEST_TOKENS / 10 * 9
        };
        let alone = parts.last().is_none_or(Vec::is_empty);
        if !alone && too_big(state + shown_chars, questions + question_chars, longest.max(question_chars)) {
            parts.push(Vec::new());
            (state, questions, longest) = (task_chars, 0, 0);
        } else if alone && parts.is_empty() {
            parts.push(Vec::new());
        }
        state += shown_chars;
        questions += question_chars;
        longest = longest.max(question_chars);
        if let Some(part) = parts.last_mut() {
            part.push(at);
        }
    }
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let items: Map<String, Value> = part.iter().map(|at| (item_key(entries[*at].0), entries[*at].1.clone())).collect();
        let questions: Map<String, Value> = part.iter().map(|at| (item_key(entries[*at].0), entries[*at].2.clone())).collect();
        let body = json!({
            "model": JEV_MODEL,
            "state": { "task": task.clone(), "items": items },
            "questions": questions,
        });
        let text = serde_json::to_string(&body).map_err(|_| FilterError::Unreadable("the request did not serialize".to_string()))?;
        let estimated = estimated_tokens(&text);
        if estimated > MAX_REQUEST_TOKENS || tokens_of(task_chars) > REQUEST_TOKENS {
            return Err(FilterError::TooLarge { estimated_tokens: estimated });
        }
        out.push((text, part.iter().map(|at| entries[*at].0).collect()));
    }
    Ok(out)
}

/// A chance de sim de cada item de `ids`, lida do documento `doc` e posta em
/// `into`. Falta de `answers`, de uma resposta ou uma chance que não é número
/// entre 0 e 1 é resposta ilegível.
fn read_item_chances(doc: &Value, ids: &[u64], into: &mut BTreeMap<u64, f64>) -> Result<(), FilterError> {
    let answers = doc.get("answers").and_then(Value::as_object).ok_or_else(|| FilterError::Unreadable("no answers".to_string()))?;
    for id in ids {
        let key = item_key(*id);
        let chance = answers
            .get(&key)
            .and_then(|answer| answer.get("noul"))
            .and_then(Value::as_f64)
            .filter(|chance| (0.0..=1.0).contains(chance))
            .ok_or_else(|| FilterError::Unreadable(format!("no chance for {key}")))?;
        into.insert(*id, chance);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::judgement::{BoardItem, BoardWave};
    use mustard_core::domain::map_filter::{CutRule, EXISTS_FROM};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
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
        let Some(received) = read_request(&stream) else {
            return;
        };
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

    // -- os ajudantes do pedido e da resposta -------------------------------------

    /// Os ids dos candidatos de um pedido, na ordem: as opções da escolha.
    fn ids_of(body: &Value) -> Vec<String> {
        if let Some(criteria) = body["questions"][WHERE_KEY]["criteria"].as_object() {
            criteria.keys().cloned().collect()
        } else {
            body["questions"].as_object().unwrap().keys().filter(|id| id.starts_with('c')).cloned().collect()
        }
    }

    /// A posição na lista de um id do pedido (`c012` é a 12).
    fn position_of(id: &str) -> usize {
        id[1..].parse().expect("an id is c and a number")
    }

    /// A resposta do serviço ao pedido `body`: a chance de cada candidato
    /// pela posição dele na lista, a de `exists` e 1000 tokens de entrada.
    fn answer_by_position(body: &Value, chance_of: impl Fn(usize) -> f64, exists: f64) -> Value {
        let mut chances = Map::new();
        for id in ids_of(body) {
            let at = position_of(&id);
            chances.insert(id, json!(chance_of(at)));
        }
        if body["questions"].get(WHERE_KEY).is_none() {
            let mut answers: Map<String, Value> = chances.into_iter().map(|(id, chance)| (id, json!({"type":"noul","noul":chance}))).collect();
            answers.insert(EXISTS_KEY.into(), json!({"type":"noul","noul":exists}));
            return json!({"answers":answers,"usage":{"input_tokens":1000,"output_tokens":0}});
        }
        json!({
            "answers": {
                "where": {"type": "choice", "choice": "c000", "probabilities": chances},
                "exists": {"type": "noul", "noul": exists},
            },
            "usage": {"input_tokens": 1000, "output_tokens": 0},
        })
    }

    /// A resposta em que todos os candidatos têm a mesma chance e algum deles
    /// existe quase com certeza.
    fn sure_answer(body: &Value) -> Value {
        answer_by_position(body, |_| 0.5, 0.9)
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

    /// Um candidato do arquivo `path`, da linha `line` à `end_line`.
    fn candidate_at(id: i64, path: &str, line: u32, end_line: u32) -> FilterCandidate {
        FilterCandidate { path: path.to_string(), line, end_line, ..candidate(id) }
    }

    /// O pedido com uma raiz em que nenhum arquivo existe: o estado leva só
    /// os cabeçalhos.
    fn request(candidates: Vec<FilterCandidate>) -> FilterRequest {
        FilterRequest {
            words: vec!["cand".to_string()],
            phrase: "the candidate that answers".to_string(),
            root: PathBuf::from("/nonexistent/project"),
            candidates,
            ..FilterRequest::default()
        }
    }

    /// Uma pasta de projeto com os arquivos `files` (caminho e texto).
    fn project_with(files: &[(&str, String)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        dir
    }

    /// O pedido que chegou ao serviço de mentira ao filtrar `asked` com a
    /// resposta segura.
    fn sent_for(asked: &FilterRequest) -> Value {
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        service.filter().filter(asked).unwrap();
        let received = service.received();
        assert_eq!(received.len(), 1, "the filter asks once");
        received[0].body.clone()
    }

    /// O texto do estado de um pedido.
    fn state_of(body: &Value) -> String {
        body["state"].as_str().expect("the state is text").to_string()
    }

    /// As duas instruções do pedido: a da escolha e a de `exists`.
    fn instructions_of(body: &Value) -> (String, String) {
        let text = |key: &str| body["questions"][key]["instructions"].as_str().unwrap().to_string();
        (text(WHERE_KEY), text(EXISTS_KEY))
    }

    // -- o estado -------------------------------------------------------------------

    #[test]
    fn the_state_is_text_with_a_header_the_documentation_and_the_whole_declaration_of_each_candidate() {
        let code: Vec<String> = (1..=70).map(|n| format!("line {n}")).collect();
        let project = project_with(&[("src/pay.rs", code.join("\n"))]);
        let mut first = candidate_at(1, "src/pay.rs", 3, 62);
        first.documentation = "  Cobra   o cartão.\n  Sem juros.  ".to_string();
        let second = candidate_at(2, "src/pay.rs", 1, 2);
        let mut asked = request(vec![first, second]);
        asked.root = project.path().to_path_buf();

        let state = state_of(&sent_for(&asked));

        let first_block: Vec<String> = (3..=62).map(|n| format!("    line {n}")).collect();
        let expected =
            format!("c000| src/pay.rs:3-62\n    // Cobra o cartão. Sem juros.\n{}\n\nc001| src/pay.rs:1-2\n    line 1\n    line 2", first_block.join("\n"));
        assert_eq!(state, expected, "the 60 lines of the declaration go, the second block after a blank line");
    }

    #[test]
    fn long_documentation_and_long_lines_go_whole_and_the_end_of_a_line_loses_its_blanks() {
        let long_line = format!("{}   \t", "a".repeat(200));
        let project = project_with(&[("src/a.rs", format!("{long_line}\nshort   \n"))]);
        let mut only = candidate_at(1, "src/a.rs", 1, 2);
        only.documentation = "d".repeat(400);
        let mut asked = request(vec![only]);
        asked.root = project.path().to_path_buf();

        let state = state_of(&sent_for(&asked));

        let expected = format!("c000| src/a.rs:1-2\n    // {}\n    {}\n    short", "d".repeat(400), "a".repeat(200));
        assert_eq!(state, expected);
    }

    #[test]
    fn every_commit_every_review_and_what_the_declaration_calls_go_in_the_block_without_a_cut() {
        let mut only = candidate(1);
        only.commits = (1..=200).map(|n| format!("commit title {n}")).collect();
        only.reviews = vec![format!("review {}", "palavra ".repeat(400)), "second review".to_string()];
        only.calls = vec!["open_account".to_string(), "charge_card".to_string()];
        let state = state_of(&sent_for(&request(vec![only])));

        for n in 1..=200 {
            assert!(state.contains(&format!("\n    // commit: commit title {n}\n")), "commit {n} is in the state");
        }
        assert!(state.contains(&format!("    // review: review {}", "palavra ".repeat(400).trim_end())), "the long review goes whole");
        assert!(state.contains("\n    // review: second review\n"), "{state}");
        assert!(state.contains("\n    // calls: open_account, charge_card"), "{state}");
    }

    #[test]
    fn only_the_lines_of_the_declaration_go_and_a_file_that_cannot_be_read_gives_the_header_alone() {
        let code: Vec<String> = (1..=10).map(|n| format!("line {n}")).collect();
        let project = project_with(&[("src/a.rs", code.join("\n"))]);
        let mut asked = request(vec![candidate_at(1, "src/a.rs", 4, 6), candidate_at(2, "src/gone.rs", 1, 2)]);
        asked.root = project.path().to_path_buf();

        let state = state_of(&sent_for(&asked));

        assert_eq!(state, "c000| src/a.rs:4-6\n    line 4\n    line 5\n    line 6\n\nc001| src/gone.rs:1-2");
    }

    #[test]
    fn a_file_outside_the_project_a_sensitive_one_or_too_big_is_not_read() {
        let outer = project_with(&[
            ("outside.rs", "OUTSIDE_CONTENT\n".to_string()),
            ("sub/src/big.rs", "x".repeat(1_000_001)),
            ("sub/config/credentials/prod.rs", "SENSITIVE_CONTENT\n".to_string()),
            ("sub/keys/server.key", "KEY_CONTENT\n".to_string()),
        ]);
        let root = outer.path().join("sub");
        let absolute = outer.path().join("outside.rs").to_string_lossy().into_owned();
        let mut asked = request(vec![
            candidate_at(1, "../outside.rs", 1, 1),
            candidate_at(2, &absolute, 1, 1),
            candidate_at(3, "src/big.rs", 1, 1),
            candidate_at(4, "config/credentials/prod.rs", 1, 1),
            candidate_at(5, "keys/server.key", 1, 1),
        ]);
        asked.root = root;

        let state = state_of(&sent_for(&asked));

        assert!(!state.contains("OUTSIDE_CONTENT") && !state.contains("SENSITIVE_CONTENT") && !state.contains("KEY_CONTENT"), "{state}");
        assert!(!state.contains("xxxx"), "{state}");
        assert_eq!(state.matches("\n    ").count(), 0, "no code line at all: {state}");
        assert_eq!(state.split("\n\n").count(), 5);
    }

    // -- a divisão da lista em pedidos -------------------------------------------

    /// Um projeto com um arquivo de `lines` linhas de 28 caracteres, e o
    /// pedido com `count` candidatos que o leem inteiro: com 40 linhas, cada
    /// um pesa cerca de 400 tokens.
    fn big_request(count: i64, lines: usize) -> (tempfile::TempDir, FilterRequest) {
        let code: Vec<String> = (1..=lines).map(|n| format!("{n:02}{}", "x".repeat(26))).collect();
        let project = project_with(&[("src/big.rs", code.join("\n"))]);
        let mut asked = request((1..=count).map(|id| candidate_at(id, "src/big.rs", 1, lines as u32)).collect());
        asked.root = project.path().to_path_buf();
        (project, asked)
    }

    /// Os tokens estimados de um pedido do ponto de vista do limite do
    /// serviço: o estado mais a maior pergunta.
    fn limit_tokens(body: &Value) -> u64 {
        let question = |key: &str| estimated_tokens(&body["questions"][key].to_string());
        estimated_tokens(&state_of(body)) + question(WHERE_KEY).max(question(EXISTS_KEY))
    }

    #[test]
    fn a_hundred_and_fifty_candidates_of_four_hundred_tokens_go_in_several_requests_none_above_32_thousand() {
        let (_project, asked) = big_request(150, 40);
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));

        let got = service.filter().filter(&asked).unwrap();

        let received = service.received();
        assert!(received.len() >= 2, "sixty thousand tokens do not fit one request: {}", received.len());
        assert_eq!(got.usage.requests, received.len() as u64);
        // Os pedidos chegam em qualquer ordem, porque saem juntos; cada um
        // leva uma fatia seguida da lista.
        let mut slices: Vec<Vec<String>> = Vec::new();
        for one in &received {
            assert!(limit_tokens(&one.body) <= 32_000, "a request of {} tokens", limit_tokens(&one.body));
            let ids = ids_of(&one.body);
            let state = state_of(&one.body);
            let heads: Vec<&str> = state.split("\n\n").map(|block| block.split('|').next().unwrap()).collect();
            assert_eq!(heads, ids, "the blocks of the state are the options of the choice, in order");
            slices.push(ids);
        }
        slices.sort();
        let sent: Vec<String> = slices.into_iter().flatten().collect();
        let all: Vec<String> = (0..150).map(candidate_id).collect();
        assert_eq!(sent, all, "the 150 go, once each, in slices that follow the order of the list");
    }

    #[test]
    fn a_declaration_of_sixty_lines_goes_whole() {
        let (_project, asked) = big_request(1, 60);
        let state = state_of(&sent_for(&asked));
        assert_eq!(state.lines().count(), 61, "the header and the 60 lines");
        assert!(state.contains("\n    60xxxxxxxxxxxxxxxxxxxxxxxxxx"), "the last line is there: {state}");
    }

    #[test]
    fn a_list_above_the_choice_limit_of_the_service_is_split_even_when_it_is_small() {
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        let got = service.filter().filter(&request((1..=300).map(candidate).collect())).unwrap();
        let received = service.received();
        assert_eq!(received.len(), 2, "255 options and the 45 left");
        let mut sizes: Vec<usize> = received.iter().map(|one| ids_of(&one.body).len()).collect();
        sizes.sort_unstable();
        assert_eq!(sizes, [45, 255]);
        assert_eq!(got.usage.requests, 2);
    }

    #[test]
    fn a_candidate_that_alone_passes_a_request_goes_alone_in_consecutive_parts_with_the_same_id() {
        // 4.000 linhas de 28 caracteres: mais de 40 mil tokens, e não cabem
        // num pedido.
        let code: Vec<String> = (1..=4_000).map(|n| format!("{n:04}{}", "x".repeat(24))).collect();
        let project = project_with(&[("src/huge.rs", code.join("\n")), ("src/small.rs", "small body\n".to_string())]);
        let mut asked = request(vec![candidate_at(1, "src/small.rs", 1, 1), candidate_at(2, "src/huge.rs", 1, 4_000), candidate_at(3, "src/small.rs", 1, 1)]);
        asked.root = project.path().to_path_buf();
        // O pedido da parte do meio existe com chance alta; os outros, baixa.
        let service = FakeService::start(|_, body| {
            let exists = if state_of(body).contains("\n    2000xxxx") { 0.9 } else { 0.1 };
            Reply::json(200, &answer_by_position(body, |_| 1.0, exists))
        });

        let got = service.filter().filter(&asked).unwrap();

        let received = service.received();
        let mut by_head: Vec<(String, Vec<String>)> = received.iter().map(|one| (state_of(&one.body), ids_of(&one.body))).collect();
        by_head.sort();
        let parts: Vec<&(String, Vec<String>)> = by_head.iter().filter(|(state, _)| state.starts_with("c001| ")).collect();
        assert!(parts.len() >= 2, "the huge candidate is in several requests: {}", parts.len());
        assert!(received.len() >= parts.len() + 2, "the small ones before and after it go in their own requests");
        for (state, ids) in &parts {
            assert_eq!(ids, &["c001"], "each part is alone with the same id");
            assert!(state.lines().next().unwrap().starts_with("c001| src/huge.rs:1-4000 (part "), "{}", state.lines().next().unwrap());
        }
        for one in &received {
            assert!(limit_tokens(&one.body) <= 32_000, "a request of {} tokens", limit_tokens(&one.body));
        }
        // Todas as linhas, uma vez só, e em ordem dentro de cada parte: as
        // partes saem em ordem de texto, e o número de cada linha cresce.
        let in_parts: Vec<String> = parts.iter().flat_map(|(state, _)| state.lines().skip(1).map(str::to_string)).collect();
        let expected: Vec<String> = code.iter().map(|line| format!("    {line}")).collect();
        assert_eq!(in_parts, expected, "every line of the declaration is in some part, once, in order");
        // O candidato que se repete fica com a maior nota.
        let huge = got.kept.iter().filter(|scored| scored.id == 2).collect::<Vec<_>>();
        assert_eq!(huge.len(), 1, "the candidate comes back once");
        assert!((huge[0].score - 1.0).abs() < 1e-9, "the best of its parts: {}", huge[0].score);
    }

    #[test]
    fn the_requests_leave_at_the_same_time() {
        let (_project, asked) = big_request(150, 40);
        let (live, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let service = {
            let (live, peak) = (Arc::clone(&live), Arc::clone(&peak));
            FakeService::start(move |_, body| {
                let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(500));
                live.fetch_sub(1, Ordering::SeqCst);
                Reply::json(200, &sure_answer(body))
            })
        };

        let started = Instant::now();
        let got = service.filter().filter(&asked).unwrap();

        assert!(got.usage.requests >= 2);
        assert_eq!(peak.load(Ordering::SeqCst) as u64, got.usage.requests, "every request was in flight together");
        assert!(started.elapsed() < Duration::from_millis(500 * got.usage.requests), "one wait, not one after the other");
    }

    /// A resposta de cada pedido da lista grande: o que tem o candidato 3 diz
    /// `first` de chance de existir e escolhe o `c003` com 0,8; os outros
    /// dizem `others` e repartem a chance igual.
    fn answer_with_the_third(first: f64, others: f64) -> impl Fn(usize, &Value) -> Reply + Send + Sync + 'static {
        move |_, body| {
            let ids = ids_of(body);
            let has = ids.iter().any(|id| id == "c003");
            let chance = |at: usize| {
                let rest = ids.len() as f64 - 1.0;
                if has { if at == 3 { 0.8 } else { 0.2 / rest } } else { 1.0 / ids.len() as f64 }
            };
            Reply::json(200, &answer_by_position(body, chance, if has { first } else { others }))
        }
    }

    #[test]
    fn independent_partition_scores_are_not_globally_normalized() {
        let (_project, asked) = big_request(150, 40);
        let service = FakeService::start(answer_with_the_third(0.9, 0.2));

        let got = service.filter().filter(&asked).unwrap();

        assert_eq!(got.verdict, Verdict::Found);
        let best = got.kept.first().unwrap();
        assert_eq!(best.id, 4, "the candidate in position 3 is the c003");
        assert!((best.score - 0.8).abs() < 1e-9, "absolute candidate relevance is preserved: {}", best.score);
        assert_eq!(got.kept.len(), 1 + got.kept.iter().skip(1).filter(|scored| scored.score >= 0.08).count());
        assert!(got.kept.iter().skip(1).all(|scored| scored.score < best.score));
    }

    #[test]
    fn when_no_request_reaches_the_existence_line_the_answer_is_not_found() {
        let (_project, asked) = big_request(150, 40);
        let service = FakeService::start(answer_with_the_third(0.49, 0.2));

        let got = service.filter().filter(&asked).unwrap();

        assert!(service.received().len() >= 2);
        assert_eq!(got.verdict, Verdict::NotFound);
        assert!(got.kept.is_empty());

        let at_the_line = FakeService::start(answer_with_the_third(0.50, 0.2));
        assert_eq!(at_the_line.filter().filter(&asked).unwrap().verdict, Verdict::Found, "on the line it is found");
    }

    #[test]
    fn the_usage_adds_the_tokens_and_the_cost_of_every_request_and_counts_them() {
        let (_project, asked) = big_request(150, 40);
        let service = FakeService::start(|_, body| {
            let mut answer = sure_answer(body);
            answer["usage"]["input_tokens"] = json!(10_000);
            answer["model"] = json!("jev-1.13.0");
            Reply::json(200, &answer)
        });

        let got = service.filter().filter(&asked).unwrap();

        let requests = service.received().len() as u64;
        assert!(requests >= 2);
        assert_eq!(got.usage.requests, requests);
        assert_eq!(got.usage.input_tokens, 10_000 * requests);
        // US$ 0,042 o milhão: 10.000 tokens custam 420 milionésimos.
        assert_eq!(got.usage.cost_micro_usd, 420 * requests);
        assert_eq!(got.usage.model, "jev-1.13.0");
    }

    #[test]
    fn a_request_that_fails_fails_the_whole_filter() {
        let (_project, asked) = big_request(150, 40);
        let service = FakeService::start(|_, body| {
            if ids_of(body).iter().any(|id| id == "c149") { Reply::json(401, &json!({"error": "no"})) } else { Reply::json(200, &sure_answer(body)) }
        });

        let error = service.filter().filter(&asked).unwrap_err();

        assert_eq!(error, FilterError::Refused { status: 401 });
    }

    #[test]
    fn a_secret_in_the_code_the_documentation_the_history_the_calls_the_phrase_or_the_context_does_not_leave_the_machine() {
        let key = format!("ghp_{}", "a1B2c3D4".repeat(5));
        let secret = format!("DB_PASSWORD=S3nh4F0rte2024 {key}");
        let project = project_with(&[("src/pay.rs", format!("fn pay() {{\n    let token = \"{key}\";\n}}\n"))]);
        let mut leaky = candidate_at(1, "src/pay.rs", 1, 3);
        leaky.documentation = format!("Cobra o cartão. {secret}");
        leaky.commits = vec![format!("troca a senha {secret}")];
        leaky.reviews = vec![format!("não deixe a chave {key} no código")];
        leaky.calls = vec!["charge".to_string()];
        let mut asked = request(vec![leaky]);
        asked.root = project.path().to_path_buf();
        asked.words.push(key.clone());
        asked.phrase = format!("a senha do banco: {secret}");
        asked.described = format!("busca a chave {key}");
        asked.said = format!("vou olhar {secret} agora");

        let sent = sent_for(&asked).to_string();

        assert!(!sent.contains("S3nh4F0rte2024"), "{sent}");
        assert!(!sent.contains(&key[..12]), "{sent}");
        assert!(sent.contains("Cobra o cartão."), "the rest of the text still goes: {sent}");
        assert!(sent.contains("fn pay() {"), "{sent}");
        assert!(sent.contains("vou olhar"), "{sent}");
        assert!(sent.contains("troca a senha") && sent.contains("não deixe a chave"), "the history still goes: {sent}");
        assert!(sent.contains("charge"), "{sent}");
    }

    #[test]
    fn without_candidates_nothing_is_asked() {
        let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        let got = service.filter().filter(&request(Vec::new())).unwrap();
        assert_eq!(got.verdict, Verdict::NotFound);
        assert!(got.kept.is_empty());
        assert!(service.received().is_empty());
    }

    // -- as duas perguntas ------------------------------------------------------------

    #[test]
    fn the_request_has_the_two_questions_and_the_ids_of_the_candidates_without_descriptions() {
        let body = sent_for(&request(vec![candidate(1), candidate(2), candidate(3)]));

        let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "state", "questions"]);
        assert_eq!(body["model"], json!("jev-1.13.0"));
        let questions = body["questions"].as_object().unwrap();
        let names: Vec<&String> = questions.keys().collect();
        assert_eq!(names, ["where", "exists"], "the choice goes first");
        assert_eq!(
            questions["where"],
            json!({
                "type": "choice",
                "instructions": "Which candidate is the code that answers this request from a coding agent: \"the candidate that answers\"?",
                "criteria": {"c000": null, "c001": null, "c002": null},
            })
        );
        assert_eq!(
            questions["exists"],
            json!({
                "type": "noul",
                "instructions": "Does any candidate contain the code that answers this request from a coding agent: \"the candidate that answers\"?",
                "criteria": {
                    "true": "At least one candidate is the code the request asks for, or contains it.",
                    "false": "No candidate is the code the request asks for: they are only on related topics or only share some words with it.",
                },
            })
        );
    }

    #[test]
    fn without_a_phrase_the_words_are_the_request() {
        let mut asked = request(vec![candidate(1)]);
        asked.phrase = "  ".to_string();
        asked.words = vec!["split".to_string(), "identifier".to_string()];
        let (choice, exists) = instructions_of(&sent_for(&asked));
        assert!(choice.contains("agent: \"split identifier\"?"), "{choice}");
        assert!(exists.contains("agent: \"split identifier\"?"), "{exists}");
    }

    #[test]
    fn the_description_and_the_speech_of_the_agent_end_both_instructions_when_they_exist() {
        let mut asked = request(vec![candidate(1)]);
        asked.described = "  Procura o cálculo do frete ".to_string();
        asked.said = "Vou ver\n onde o frete é   calculado.".to_string();
        let (choice, exists) = instructions_of(&sent_for(&asked));
        let suffix = " The agent described this search as: \"Procura o cálculo do frete\". \
                      Just before it, the agent wrote: \"Vou ver onde o frete é calculado.\".";
        assert_eq!(choice, format!("Which candidate is the code that answers this request from a coding agent: \"the candidate that answers\"?{suffix}"));
        assert_eq!(
            exists,
            format!("Does any candidate contain the code that answers this request from a coding agent: \"the candidate that answers\"?{suffix}")
        );
    }

    #[test]
    fn each_part_of_the_context_appears_only_when_it_exists() {
        let mut only_described = request(vec![candidate(1)]);
        only_described.described = "Procura o frete".to_string();
        let (choice, exists) = instructions_of(&sent_for(&only_described));
        assert!(choice.ends_with("? The agent described this search as: \"Procura o frete\"."), "{choice}");
        assert!(exists.ends_with("? The agent described this search as: \"Procura o frete\"."), "{exists}");
        assert!(!choice.contains("Just before it"), "{choice}");

        let mut only_said = request(vec![candidate(1)]);
        only_said.said = "Vou procurar".to_string();
        let (choice, exists) = instructions_of(&sent_for(&only_said));
        assert!(choice.ends_with("? Just before it, the agent wrote: \"Vou procurar\"."), "{choice}");
        assert!(exists.ends_with("? Just before it, the agent wrote: \"Vou procurar\"."), "{exists}");
        assert!(!choice.contains("described"), "{choice}");

        let mut neither = request(vec![candidate(1)]);
        neither.described = "   ".to_string();
        neither.said = " \n ".to_string();
        let (choice, exists) = instructions_of(&sent_for(&neither));
        assert!(choice.ends_with("\"the candidate that answers\"?"), "{choice}");
        assert!(exists.ends_with("\"the candidate that answers\"?"), "{exists}");
    }

    #[test]
    fn the_description_goes_up_to_three_hundred_characters_and_the_speech_the_last_five_hundred() {
        let mut asked = request(vec![candidate(1)]);
        asked.described = "d".repeat(400);
        asked.said = "palavra ".repeat(100);
        let (choice, _) = instructions_of(&sent_for(&asked));

        assert!(choice.contains(&format!("\"{}\".", "d".repeat(300))), "{choice}");
        assert!(!choice.contains(&"d".repeat(301)));
        let said = choice.split("the agent wrote: \"").nth(1).unwrap().trim_end_matches("\".");
        assert_eq!(said.chars().count(), 500);
        assert!(said.starts_with("avra palavra"), "the last 500 of the 799 characters: {said}");
        assert!(said.ends_with("palavra"));
    }

    // -- o veredito e o corte --------------------------------------------------------

    /// O que o filtro devolve para `count` candidatos quando a chance de cada
    /// um é `chance_of` (pela posição), a de existir é `exists` e o corte é
    /// `rule`.
    fn filtered(count: i64, exists: f64, rule: CutRule, chance_of: impl Fn(usize) -> f64 + Send + Sync + 'static) -> Filtered {
        let service = FakeService::start(move |_, body| Reply::json(200, &answer_by_position(body, &chance_of, exists)));
        let mut asked = request((1..=count).map(candidate).collect());
        asked.cut = rule;
        service.filter().filter(&asked).unwrap()
    }

    #[test]
    fn an_existence_chance_below_the_line_is_not_found_and_on_the_line_the_cut_counts() {
        let top = |at: usize| if at == 0 { 0.8 } else { 0.1 };
        let below = filtered(3, 0.49, CutRule::default(), top);
        assert_eq!(below.verdict, Verdict::NotFound);
        assert!(below.kept.is_empty(), "nothing passes when it is not found");

        let on_the_line = filtered(3, 0.50, CutRule::default(), top);
        assert_eq!(on_the_line.verdict, Verdict::Found);
        assert_eq!(on_the_line.kept.first().map(|scored| scored.id), Some(1));

        assert_eq!(EXISTS_FROM, 0.50);
        let moved = filtered(3, 0.4, CutRule { exists_from: 0.3, ..CutRule::default() }, top);
        assert_eq!(moved.verdict, Verdict::Found, "the line is the setting");
    }

    #[test]
    fn every_candidate_that_passes_the_cut_comes_back_with_no_ceiling_on_the_count() {
        let equal = |_: usize| 0.2;
        let kept = filtered(5, 0.9, CutRule::default(), equal);
        assert_eq!(kept.kept.len(), 5, "five equal chances above the line all come back");

        let many = filtered(150, 0.9, CutRule::default(), |_| 1.0 / 150.0);
        assert_eq!(many.kept.len(), 150, "a hundred and fifty equal chances all come back");

        let uneven = filtered(5, 0.9, CutRule::default(), |at| [0.6, 0.3, 0.05, 0.03, 0.02][at]);
        let ids: Vec<i64> = uneven.kept.iter().map(|scored| scored.id).collect();
        assert_eq!(ids, [1, 2], "the cut line is 0,06 and the order is the one of the chance");

        let lonely = filtered(5, 0.9, CutRule::default(), |at| if at == 3 { 0.9 } else { 0.01 });
        assert_eq!(lonely.kept.iter().map(|scored| scored.id).collect::<Vec<_>>(), [4], "a single concentrated chance keeps one");
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
        assert_eq!(service.received().len(), 1, "a hundred candidates go in one request");
        assert!(service.received().iter().all(|sent| sent.body["model"] == json!("jev-1.13.0")));
        assert_eq!(got.usage.input_tokens, 10_500);
        assert_eq!(got.usage.model, "jev-1.13.0");
        // 10.500 tokens a US$ 0,042 o milhão: US$ 0,000441.
        assert_eq!(got.usage.cost_micro_usd, 441);

        let silent = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
        let got = silent.filter().filter(&request(vec![candidate(1)])).unwrap();
        assert_eq!(got.usage.model, "", "an answer that does not say the model leaves it empty");
    }

    // -- as recusas e as repetições -------------------------------------------

    #[test]
    fn a_refused_key_or_missing_credit_fails_without_repeating() {
        for status in [401, 402] {
            let service = FakeService::start(move |_, _| Reply::json(status, &json!({"error": format!("bad key {SECRET}")})));
            let error = service.filter().filter(&request(vec![candidate(1)])).unwrap_err();
            assert_eq!(error, FilterError::Refused { status });
            assert_eq!(service.received().len(), 1, "HTTP {status} was repeated");
        }
    }

    #[test]
    fn too_many_requests_repeats_twice_and_gives_up() {
        let service = FakeService::start(|_, _| Reply { status: 429, headers: vec![("Retry-After", "0".to_string())], body: "{}".to_string() });
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
                Reply::json(200, &answer_by_position(body, |_| 0.7, 0.9))
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
    fn an_answer_missing_a_chance_or_the_existence_is_unreadable() {
        // Falta a chance de um candidato, a escolha, a existência ou o número
        // dela: cada ausência é uma resposta ilegível.
        let answers = [
            json!({"where": {"probabilities": {"c000": 0.5}}, "exists": {"noul": 0.9}}),
            json!({"where": {"probabilities": {"c000": 0.5, "c001": "alta"}}, "exists": {"noul": 0.9}}),
            json!({"where": {"choice": "c000"}, "exists": {"noul": 0.9}}),
            json!({"where": {"probabilities": {"c000": 0.5, "c001": 0.4}}}),
            json!({"where": {"probabilities": {"c000": 0.5, "c001": 0.4}}, "exists": {"noul": "sim"}}),
            json!({"c00": {"noul": 0.5}}),
        ];
        for answer in answers {
            let service = FakeService::start(move |_, _| Reply::json(200, &json!({"answers": answer.clone()})));
            let error = service.filter().filter(&request(vec![candidate(1), candidate(2)])).unwrap_err();
            assert!(matches!(error, FilterError::Unreadable(_)), "{error:?}");
        }
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
        let service = FakeService::start(|_, _| Reply { status: 429, headers: vec![("Retry-After", "2".to_string())], body: "{}".to_string() });
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
        let garbled = FakeService::start(|_, _| Reply { status: 200, headers: Vec::new(), body: format!("not json {SECRET}") });
        let asked = request(vec![candidate(1)]);
        let mut errors = vec![
            refused.filter().filter(&asked).unwrap_err(),
            unreadable.filter().filter(&asked).unwrap_err(),
            garbled.filter().filter(&asked).unwrap_err(),
            JevFilter::at(test_key(), &closed_url()).filter(&asked).unwrap_err(),
        ];
        errors.extend([FilterError::MissingKey, FilterError::Timeout, FilterError::TooLarge { estimated_tokens: 70_000 }, FilterError::KeyInGit]);
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

    /// A chave em `jev.key` do `mustard.json` vai ao serviço, no cabeçalho, e
    /// liga o corte: as notas voltam e o que passa fica.
    #[test]
    fn the_key_in_the_project_file_turns_the_cut_on() {
        let project = project_with_key(SECRET, false);
        let loaded = project_key(project.path(), None).unwrap();
        assert!(loaded.warning.is_none());
        let service = FakeService::start(|_, body| Reply::json(200, &answer_by_position(body, |at| if at == 0 { 0.9 } else { 0.05 }, 0.9)));
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

    /// A busca monta o filtro só quando o `mustard.json` deixa e há chave
    /// válida. Os casos cobrem a chave só no ambiente, só em `jev.key`, em
    /// branco, o `search.filter` em `none`, em `jev` e inválido, e o
    /// `mustard.json` que o git guarda.
    #[test]
    fn the_search_builds_the_filter_only_with_the_setting_on_and_a_valid_key() {
        use mustard_core::platform::i18n::Locale;

        // (nome, mustard.json, git guarda o arquivo, ambiente, ligado?)
        let cases: Vec<(&str, Value, bool, Option<&str>, bool)> = vec![
            ("no key anywhere", json!({}), false, None, false),
            ("environment only", json!({}), false, Some("from-env"), false),
            ("jev.key only", json!({"jev": {"key": "from-file"}}), false, None, false),
            ("blank jev.key", json!({"jev": {"key": "  "}}), false, None, false),
            ("blank environment with jev.key", json!({"jev": {"key": "from-file"}}), false, Some(" "), false),
            ("none with jev.key", json!({"search": {"filter": "none"}, "jev": {"key": "from-file"}}), false, None, false),
            ("none with the environment", json!({"search": {"filter": "none"}}), false, Some("from-env"), false),
            ("legacy jev with jev.key", json!({"search": {"filter": "jev"}, "jev": {"key": "from-file"}}), false, None, false),
            ("invalid setting with jev.key", json!({"search": {"filter": "other"}, "jev": {"key": "from-file"}}), false, None, false),
            ("jev.key in a file git tracks", json!({"jev": {"key": "from-file"}}), true, None, false),
            ("jev.key in a file git tracks with the environment", json!({"jev": {"key": "from-file"}}), true, Some("from-env"), false),
            ("explicit fallback and search provider", json!({"ai":{"fallback": true},"search":{"filter":"jev"}}), false, Some("from-env"), true),
            ("fallback without a provider", json!({"ai":{"fallback":true}}), false, Some("from-env"), false),
        ];
        for (name, config, tracked, env, expected) in cases {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("mustard.json"), config.to_string()).unwrap();
            if tracked {
                assert!(mustard_core::platform::git::run(root.path(), &["init", "-q"]).ok);
                assert!(mustard_core::platform::git::run(root.path(), &["add", "mustard.json"]).ok);
            }
            let env = env.map(str::to_string);

            let loaded = ProjectConfig::load(root.path());
            let mut warnings = Vec::new();
            let searched = crate::shared::search_door::chosen_filter(
                root.path(),
                None,
                Locale::PtBr,
                &loaded,
                &|at, config| crate::shared::search_door::assembled(at, config, env.clone(), None),
                &mut warnings,
            );

            assert_eq!(searched.is_some(), expected, "the search: {name}");
            assert!(waves_filter(root.path(), &loaded, env.clone(), None).filter.is_none(), "search never enables wave assembly: {name}");
        }
    }

    /// A montagem das ondas diz que a chave não valeu por causa do git só
    /// quando o `mustard.json` que o git guarda traz a única chave e o Jev
    /// está ligado: com a chave do ambiente o Jev entra, e com o filtro
    /// desligado, em branco, sem chave ou com a chave fora do git não há o que
    /// avisar.
    #[test]
    fn the_wave_assembly_flags_a_key_only_the_file_git_tracks_holds() {
        // (nome, mustard.json, git guarda o arquivo, ambiente, filtro, aviso)
        type Case = (&'static str, Value, bool, Option<&'static str>, bool, bool);
        let cases: Vec<Case> = vec![
            ("key in a tracked file", json!({"jev": {"key": "from-file"}}), true, None, false, true),
            ("key in a tracked file with a blank environment", json!({"jev": {"key": "from-file"}}), true, Some(" "), false, true),
            ("key in a tracked file with the environment", json!({"jev": {"key": "from-file"}}), true, Some("from-env"), true, false),
            ("key in an untracked file", json!({"jev": {"key": "from-file"}}), false, None, true, false),
            ("tracked file with the filter off", json!({"search": {"filter": "none"}, "jev": {"key": "from-file"}}), true, None, false, false),
            ("tracked file with an invalid filter", json!({"search": {"filter": "other"}, "jev": {"key": "from-file"}}), true, None, false, false),
            ("tracked file with a blank key", json!({"jev": {"key": "  "}}), true, None, false, false),
            ("tracked file without a key", json!({}), true, None, false, false),
        ];
        for (name, mut config, tracked, env, filter, flagged) in cases {
            config["ai"] = json!({"fallback":true});
            config["judgement"] = json!({"wave-planning":{"filter":config.pointer("/search/filter").cloned().unwrap_or(json!("jev"))}});
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("mustard.json"), config.to_string()).unwrap();
            if tracked {
                assert!(mustard_core::platform::git::run(root.path(), &["init", "-q"]).ok);
                assert!(mustard_core::platform::git::run(root.path(), &["add", "mustard.json"]).ok);
            }
            let waves = waves_filter(root.path(), &ProjectConfig::load(root.path()), env.map(str::to_string), None);
            assert_eq!(waves.filter.is_some(), filter, "the filter: {name}");
            assert_eq!(waves.key_in_git, flagged, "the git flag: {name}");
        }
    }

    // -- o backlog julgado para a montagem da onda ---------------------------------

    fn board_task(id: u64, files: &[&str]) -> BoardTask {
        BoardTask {
            id,
            title: format!("Task {id}"),
            text: format!("Text of task {id}."),
            agent: format!("- do {id}"),
            files: files.iter().map(|file| file.to_string()).collect(),
            depends_on: Vec::new(),
            reads: Vec::new(),
            criteria: std::collections::BTreeSet::new(),
        }
    }

    /// Duas tarefas no backlog e uma onda em andamento, com uma tarefa.
    fn board() -> Board {
        let mut second = board_task(12, &["module/b.rs"]);
        second.depends_on = vec!["Task 5".to_string()];
        Board { running: vec![BoardWave { n: 7, tasks: vec![board_task(5, &["module/open.rs"])] }], backlog: vec![board_task(11, &["module/a.rs"]), second] }
    }

    /// A resposta do serviço ao quadro `board()`: a tarefa 11 é um defeito
    /// certo, de tamanho 2,4, e a 12 um recurso incerto, de tamanho 0,6, que
    /// muda o mesmo que a onda 7.
    fn judged_answer() -> Value {
        json!({
            "model": "jev-1.13.0",
            "answers": {
                "tipo_t11": {"type": "choice", "choice": "defect", "probabilities":{"defect":0.9,"feature":0.04,"text_fix":0.02,"remove_unused":0.02,"test_cleanup":0.02}, "confidence": 0.9},
                "tipo_t12": {"type": "choice", "choice": "feature", "probabilities":{"defect":0.2,"feature":0.41,"text_fix":0.13,"remove_unused":0.13,"test_cleanup":0.13}, "confidence": 0.41},
                "tam_t11": {"type": "score", "score": 2.4, "probabilities":{"0":0.01,"1":0.09,"2":0.4,"3":0.5}, "confidence": 0.8},
                "tam_t12": {"type": "score", "score": 0.6, "probabilities":{"0":0.4,"1":0.6,"2":0.0,"3":0.0}, "confidence": 0.7},
                "blk_w7_t11": {"type": "noul", "noul": 0.1},
                "blk_w7_t12": {"type": "noul", "noul": 0.68},
            },
            "usage": {"input_tokens": 2000, "output_tokens": 0},
        })
    }

    /// O backlog inteiro vai numa chamada só, com o modelo fixo, o estado
    /// com a definição, a onda em andamento e o backlog, e as perguntas de
    /// tipo, de tamanho e de bloqueio juntas; a resposta vira o julgamento de
    /// cada tarefa, com os tokens, o custo e o modelo.
    #[test]
    fn profiles_and_plausible_interference_are_separate_requests() {
        let service = FakeService::start(|_, _| Reply::json(200, &judged_answer()));
        let judged = service.filter().judge_backlog(&board()).unwrap();

        let received = service.received();
        assert_eq!(received.len(), 3, "two intrinsic profiles and one interference batch");
        let mut body = received.iter().find(|r| r.body["questions"].get("blk_w7_t11").is_some()).unwrap().body.clone();
        for reply in &received {
            body["questions"].as_object_mut().unwrap().extend(reply.body["questions"].as_object().unwrap().clone());
        }
        assert_eq!(body["model"], json!(JEV_MODEL));
        let mut asked: Vec<&str> = body["questions"].as_object().unwrap().keys().map(String::as_str).collect();
        asked.sort_unstable();
        assert_eq!(asked, vec!["blk_w7_t11", "blk_w7_t12", "tam_t11", "tam_t12", "tipo_t11", "tipo_t12"]);
        let size = &body["questions"]["tam_t11"];
        assert_eq!(size["type"], json!("score"));
        assert!(size["instructions"]["question"].as_str().unwrap().contains("`backlog.t11`"), "{size}");
        assert_eq!(size["criteria"], json!(["One small place", "Two or three files", "Many files in one area", "Many files across several areas"]));
        let state = &body["state"];
        assert!(state["definition"]["wave"].is_string());
        let profile = received.iter().find(|r| r.body["questions"].get("tipo_t11").is_some()).unwrap();
        assert!(profile.body["state"]["definition"]["kinds"]["defect"].is_string());
        assert!(profile.body["state"]["running"].as_object().unwrap().is_empty());
        assert_eq!(state["running"]["w7"]["t5"]["files"], json!(["module/open.rs"]));
        assert_eq!(state["backlog"]["t12"]["depends_on"], json!(["Task 5"]));
        assert_eq!(state["backlog"]["t11"]["text"], json!("Text of task 11."));
        assert_eq!(body["questions"]["tipo_t11"]["criteria"].as_object().unwrap().len(), 5);

        assert_eq!(judged.tasks[&11], Judgement { kind: TaskKind::Defect, confidence: 0.9, clash: 0.1, size: 2.4 });
        assert_eq!(judged.tasks[&12], Judgement { kind: TaskKind::Feature, confidence: 0.41, clash: 0.68, size: 0.6 });
        assert_eq!((judged.tasks[&11].growth_tokens(), judged.tasks[&12].growth_tokens()), (107_000, 53_000));
        assert_eq!((judged.usage.input_tokens, judged.usage.cost_micro_usd, judged.usage.requests, judged.usage.model.as_str()), (6000, 252, 3, "jev-1.13.0"));
    }

    /// Sem onda em andamento não há pergunta de bloqueio, e a tarefa fica sem
    /// choque; a falta de uma resposta, um tipo que não existe, a confiança
    /// ausente e a nota de tamanho que não é nota são resposta ilegível, e o
    /// serviço que recusa é falha.
    #[test]
    fn an_answer_that_misses_a_question_or_a_kind_is_unreadable_and_a_refusal_is_a_failure() {
        let no_wave = Board { running: Vec::new(), backlog: vec![board_task(11, &["a.rs"])] };
        let service = FakeService::start(|_, _| {
            Reply::json(200, &{
                let mut answer = judged_answer();
                answer["answers"].as_object_mut().unwrap().retain(|key, _| key.ends_with("t11"));
                answer
            })
        });
        let judged = service.filter().judge_backlog(&no_wave).unwrap();
        assert_eq!(judged.tasks[&11].clash, 0.0);
        assert_eq!(service.received()[0].body["questions"].as_object().unwrap().len(), 2);

        for (name, answer) in [
            (
                "a missing blk",
                json!({"tipo_t11": {"choice": "defect", "confidence": 0.9}, "tipo_t12": {"choice": "defect", "confidence": 0.9}, "tam_t11": {"score": 1.0}, "tam_t12": {"score": 1.0}, "blk_w7_t11": {"noul": 0.1}}),
            ),
            (
                "an unknown kind",
                json!({"tipo_t11": {"choice": "chore", "confidence": 0.9}, "tipo_t12": {"choice": "defect", "confidence": 0.9}, "tam_t11": {"score": 1.0}, "tam_t12": {"score": 1.0}, "blk_w7_t11": {"noul": 0.1}, "blk_w7_t12": {"noul": 0.1}}),
            ),
            (
                "no confidence",
                json!({"tipo_t11": {"choice": "defect"}, "tipo_t12": {"choice": "defect", "confidence": 0.9}, "tam_t11": {"score": 1.0}, "tam_t12": {"score": 1.0}, "blk_w7_t11": {"noul": 0.1}, "blk_w7_t12": {"noul": 0.1}}),
            ),
            (
                "a missing tam",
                json!({"tipo_t11": {"choice": "defect", "confidence": 0.9}, "tipo_t12": {"choice": "defect", "confidence": 0.9}, "tam_t11": {"score": 1.0}, "blk_w7_t11": {"noul": 0.1}, "blk_w7_t12": {"noul": 0.1}}),
            ),
            (
                "a tam that is not a score",
                json!({"tipo_t11": {"choice": "defect", "confidence": 0.9}, "tipo_t12": {"choice": "defect", "confidence": 0.9}, "tam_t11": {"score": 1.0}, "tam_t12": {"noul": 0.4}, "blk_w7_t11": {"noul": 0.1}, "blk_w7_t12": {"noul": 0.1}}),
            ),
        ] {
            let service = FakeService::start(move |_, _| Reply::json(200, &json!({"answers": answer.clone()})));
            let error = service.filter().judge_backlog(&board()).unwrap_err();
            assert!(matches!(error, FilterError::Unreadable(_)), "{name}: {error:?}");
        }

        let refusing = FakeService::start(|_, _| Reply::json(401, &json!({})));
        assert_eq!(refusing.filter().judge_backlog(&board()).unwrap_err(), FilterError::Refused { status: 401 });
    }

    /// O quadro que passa do tamanho de um pedido não sai, e nenhum segredo
    /// das tarefas vai ao serviço.
    #[test]
    fn a_board_above_the_request_size_is_not_sent_and_secrets_stay_on_the_machine() {
        let service = FakeService::start(|_, _| Reply::json(200, &judged_answer()));
        let mut big = board();
        big.backlog[0].text = "word ".repeat(30_000);
        let error = service.filter().judge_backlog(&big).unwrap_err();
        assert!(matches!(error, FilterError::TooLarge { .. }), "{error:?}");
        assert!(service.received().is_empty(), "nothing is sent");

        let key = format!("ghp_{}", "a1B2c3D4".repeat(5));
        let mut secret = board();
        secret.backlog[0].text = format!("Fix the login. DB_PASSWORD=S3nh4F0rte2024 {key}");
        secret.backlog[1].title = format!("rotate {key}");
        service.filter().judge_backlog(&secret).unwrap();
        let sent = service.received().iter().map(|request| request.body.to_string()).collect::<Vec<_>>().join("\n");
        assert!(!sent.contains("S3nh4F0rte2024") && !sent.contains(&key[..12]), "secrets never leave: {sent}");
        assert!(sent.contains("Fix the login."), "the rest of the text still goes");
    }

    // -- os itens do pedido julgados para uma onda ------------------------------

    fn board_item(id: u64) -> BoardItem {
        BoardItem { id, title: format!("Item {id}"), text: format!("Text of item {id}.") }
    }

    /// A resposta do serviço que dá `chance_of(id)` a cada pergunta de item
    /// do pedido.
    fn chances_answer(body: &Value, chance_of: impl Fn(u64) -> f64) -> Reply {
        let answers: serde_json::Map<String, Value> = body["questions"]
            .as_object()
            .unwrap()
            .keys()
            .map(|key| (key.clone(), json!({"type": "noul", "noul": chance_of(key[1..].parse().unwrap())})))
            .collect();
        Reply::json(200, &json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 700}}))
    }

    /// Os itens de uma onda vão numa chamada só, com o modelo fixo, o estado
    /// com as tarefas dela e os itens — o título e o começo do texto, nada da
    /// estrutura — e uma pergunta de sim ou não por item; a resposta vira a
    /// chance de cada item, com os tokens, o custo e o modelo.
    #[test]
    fn the_items_of_a_wave_are_judged_in_one_request_with_a_question_per_item() {
        let service = FakeService::start(|_, body| chances_answer(body, |id| id as f64 / 100.0));
        let board = ItemsBoard { tasks: vec![board_task(5, &["a.rs"]), board_task(6, &["b.rs"])], items: vec![board_item(11), board_item(12)] };

        let judged = service.filter().judge_items(&board).unwrap();

        let received = service.received();
        assert_eq!(received.len(), 1, "one request for the wave");
        let body = &received[0].body;
        assert_eq!(body["model"], json!(JEV_MODEL));
        let mut asked: Vec<&str> = body["questions"].as_object().unwrap().keys().map(String::as_str).collect();
        asked.sort_unstable();
        assert_eq!(asked, vec!["i11", "i12"]);
        assert_eq!(body["questions"]["i11"]["type"], json!("noul"));
        assert!(body["questions"]["i11"]["instructions"]["question"].as_str().unwrap().contains("items.i11"));
        let state = &body["state"];
        assert_eq!(state["task"]["t5"]["files"], json!(["a.rs"]));
        assert_eq!(state["task"]["t6"]["text"], json!("Text of task 6."));
        assert_eq!(state["items"]["i12"], json!({"title": "Item 12", "text": "Text of item 12."}));
        assert_eq!(judged.chances, BTreeMap::from([(11, 0.11), (12, 0.12)]));
        assert_eq!((judged.usage.input_tokens, judged.usage.cost_micro_usd, judged.usage.requests, judged.usage.model.as_str()), (700, 29, 1, "jev-1.13.0"));
    }

    /// O estado que passa do tamanho de um pedido sai em partes, todas com as
    /// mesmas tarefas e juntas ao mesmo tempo, e cada item é perguntado uma
    /// vez só; os tokens e as chances das partes se somam.
    #[test]
    fn a_state_above_the_request_size_goes_in_parts_with_the_same_task_in_each() {
        let service = FakeService::start(|_, body| chances_answer(body, |_| 0.5));
        let long = "word ".repeat(300);
        let items: Vec<BoardItem> = (1..=700).map(|id| BoardItem { id, title: format!("Item {id}"), text: long.clone() }).collect();
        let board = ItemsBoard { tasks: vec![board_task(5, &["a.rs"])], items };

        let judged = service.filter().judge_items(&board).unwrap();

        let received = service.received();
        assert!(received.len() > 1, "{} requests", received.len());
        let mut seen: Vec<String> = Vec::new();
        for request in &received {
            assert_eq!(request.body["state"]["task"], received[0].body["state"]["task"], "the same task in every part");
            let keys: Vec<String> = request.body["questions"].as_object().unwrap().keys().cloned().collect();
            let state_keys: Vec<String> = request.body["state"]["items"].as_object().unwrap().keys().cloned().collect();
            assert_eq!(
                keys.iter().collect::<std::collections::BTreeSet<_>>(),
                state_keys.iter().collect::<std::collections::BTreeSet<_>>(),
                "a part asks about what it shows"
            );
            seen.extend(keys);
        }
        assert_eq!(seen.len(), 700, "every item asked once");
        assert_eq!(judged.chances.len(), 700);
        assert_eq!(judged.usage.requests, received.len() as u64);
        assert_eq!(judged.usage.input_tokens, 700 * received.len() as u64, "the tokens of the parts add up");
    }

    /// A falta de uma resposta e a chance que não é número entre 0 e 1 são
    /// resposta ilegível, a recusa do serviço é falha, o quadro sem item não
    /// pergunta nada, e nenhum segredo das tarefas vai ao serviço.
    #[test]
    fn an_unreadable_answer_a_refusal_an_empty_board_and_secrets_for_the_items() {
        for (name, answers) in [
            ("a missing answer", json!({"i11": {"noul": 0.5}})),
            ("a chance above 1", json!({"i11": {"noul": 0.5}, "i12": {"noul": 1.5}})),
            ("a chance that is not a number", json!({"i11": {"noul": 0.5}, "i12": {"noul": "high"}})),
        ] {
            let service = FakeService::start(move |_, _| Reply::json(200, &json!({"answers": answers.clone()})));
            let board = ItemsBoard { tasks: vec![board_task(5, &["a.rs"])], items: vec![board_item(11), board_item(12)] };
            let error = service.filter().judge_items(&board).unwrap_err();
            assert!(matches!(error, FilterError::Unreadable(_)), "{name}: {error:?}");
        }

        let refusing = FakeService::start(|_, _| Reply::json(401, &json!({})));
        let board = ItemsBoard { tasks: vec![board_task(5, &[])], items: vec![board_item(11)] };
        assert_eq!(refusing.filter().judge_items(&board).unwrap_err(), FilterError::Refused { status: 401 });

        let service = FakeService::start(|_, body| chances_answer(body, |_| 0.5));
        let empty = ItemsBoard { tasks: vec![board_task(5, &[])], items: Vec::new() };
        assert!(service.filter().judge_items(&empty).unwrap().chances.is_empty());
        assert!(service.received().is_empty(), "nothing to ask, nothing sent");

        let key = format!("ghp_{}", "a1B2c3D4".repeat(5));
        let mut secret = ItemsBoard { tasks: vec![board_task(5, &[])], items: vec![board_item(11)] };
        secret.tasks[0].text = format!("Fix the login. DB_PASSWORD=S3nh4F0rte2024 {key}");
        secret.items[0].text = format!("Rotate {key}");
        service.filter().judge_items(&secret).unwrap();
        let sent = service.received().iter().map(|request| request.body.to_string()).collect::<Vec<_>>().join("\n");
        assert!(!sent.contains("S3nh4F0rte2024") && !sent.contains(&key[..12]), "secrets never leave: {sent}");
        assert!(sent.contains("Fix the login."), "the rest of the text still goes");
    }

    // -- o teto de gasto do mês ----------------------------------------------

    /// Abaixo do teto as três chamadas saem — a busca, a montagem e a escolha
    /// dos itens —; passando do que sobra no mês nenhuma sai, e o serviço não
    /// recebe pedido nenhum; e o que sobra se gasta a cada chamada do mesmo
    /// processo, a estimativa de cada uma contra o que restou. A montagem das
    /// ondas sabe que o teto segurou o Jev só depois da chamada recusada.
    #[test]
    fn the_calls_go_out_only_while_the_estimate_fits_what_is_left_of_the_month() {
        let start = |left: u64| {
            let service = FakeService::start(|_, body| {
                if body["questions"].get("i11").is_some() {
                    chances_answer(body, |_| 0.5)
                } else if body["questions"].as_object().is_some_and(|q| q.keys().any(|key| key.starts_with("tipo_") || key.starts_with("blk_"))) {
                    Reply::json(200, &judged_answer())
                } else {
                    Reply::json(200, &sure_answer(body))
                }
            });
            let filter = JevFilter { budget: Some(Budget::of(left)), ..service.filter() };
            (service, filter)
        };
        let items = ItemsBoard { tasks: vec![board_task(5, &["a.rs"])], items: vec![board_item(11)] };
        let search = request(vec![candidate(1), candidate(2)]);

        let held = |filter: &JevFilter| WavesJev { filter: Some(std::sync::Arc::new(filter.clone())), ..WavesJev::default() }.held_by_budget();

        let (service, filter) = start(10_000_000);
        assert!(filter.filter(&search).is_ok() && filter.judge_backlog(&board()).is_ok() && filter.judge_items(&items).is_ok());
        assert_eq!(service.received().len(), 5, "search, two profiles, interference and context reached the service");
        assert!(!held(&filter), "no call was refused");

        let (service, filter) = start(1);
        assert!(!held(&filter), "nothing is held before a call");
        assert_eq!(filter.filter(&search).unwrap_err(), FilterError::OverBudget);
        assert!(held(&filter), "the refused call holds the Jev");
        assert_eq!(filter.judge_backlog(&board()).unwrap_err(), FilterError::OverBudget);
        assert_eq!(filter.judge_items(&items).unwrap_err(), FilterError::OverBudget);
        assert!(service.received().is_empty(), "nothing went to the service: {:?}", service.received());

        let one: u64 = backlog_payloads(&board()).unwrap().iter().map(|p| cost_micro_usd(estimated_tokens(p))).sum();
        let (service, filter) = start(one + one / 2);
        assert!(filter.judge_backlog(&board()).is_ok());
        assert_eq!(filter.judge_backlog(&board()).unwrap_err(), FilterError::OverBudget, "the first call took its estimate");
        assert_eq!(service.received().len(), 3, "only the first planning operation went out");
    }

    /// A busca dividida em vários pedidos reserva a estimativa de todos antes
    /// de o primeiro sair: faltando um milionésimo para o último, nenhum chega
    /// ao serviço; com a soma exata, todos saem, e nada mais sobra no mês.
    #[test]
    fn a_divided_search_reserves_every_request_before_the_first_goes_out() {
        let (_project, asked) = big_request(150, 40);
        let context = Context::of(&asked);
        let batches = divide(&asked, &context).unwrap();
        assert!(batches.len() >= 2, "the search is divided: {}", batches.len());
        let all: u64 = batches.iter().map(|batch| cost_micro_usd(estimated_tokens(&independent_body(&batch.state, &context, &batch.at).to_string()))).sum();
        let start = |left: u64| {
            let service = FakeService::start(|_, body| Reply::json(200, &sure_answer(body)));
            let budget = Budget::of(left);
            let filter = JevFilter { budget: Some(budget.clone()), ..service.filter() };
            (service, filter, budget)
        };

        let (service, filter, _) = start(all - 1);
        assert_eq!(filter.filter(&asked).unwrap_err(), FilterError::OverBudget);
        assert!(service.received().is_empty(), "no part went out: {}", service.received().len());

        let (service, filter, budget) = start(all);
        assert!(filter.filter(&asked).is_ok());
        assert_eq!(service.received().len(), batches.len(), "every part went out");
        assert!(budget.is_spent(), "the reservation took the whole estimate");
    }

    /// O teto já gasto vale como sem chave na montagem das ondas: sem filtro e
    /// sem aviso de chave. Com sobra, o filtro entra.
    #[test]
    fn spent_budget_preserves_the_adapter_for_cached_judgements() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("mustard.json"),
            r#"{"ai":{"fallback":true},"judgement":{"wave-planning":{"filter":"jev"}},"jev": {"monthly_budget_usd": 0}}"#,
        )
        .unwrap();
        let spent = waves_filter(root.path(), &ProjectConfig::load(root.path()), Some("from-env".to_string()), None);
        assert!(spent.filter.is_some() && spent.over_budget && !spent.key_in_git);

        std::fs::write(
            root.path().join("mustard.json"),
            r#"{"ai":{"fallback":true},"judgement":{"wave-planning":{"filter":"jev"}},"jev": {"monthly_budget_usd": 5}}"#,
        )
        .unwrap();
        let open = waves_filter(root.path(), &ProjectConfig::load(root.path()), Some("from-env".to_string()), None);
        assert!(open.filter.is_some());
    }
    #[test]
    fn intrinsic_profiles_survive_reassignment_but_change_when_the_full_task_changes() {
        let service = FakeService::start(|_, _| Reply::json(200, &judged_answer()));
        let dir = tempfile::tempdir().unwrap();
        let mut filter = service.filter();
        filter.cache = Some(dir.path().into());
        let mut board = board();
        board.running.clear();
        let first = filter.judge_backlog(&board).unwrap();
        assert_eq!(first.usage.requests, 2);
        let mut reassigned = board.clone();
        for task in &mut reassigned.backlog {
            task.id += 100;
        }
        let cached = filter.judge_backlog(&reassigned).unwrap();
        assert_eq!(cached.usage.requests, 0, "reassignment is not a new semantic profile");
        assert_eq!(cached.usage.input_tokens, 0);
        assert_eq!(cached.tasks[&111], first.tasks[&11]);
        assert_eq!(cached.tasks[&112], first.tasks[&12]);
        assert_eq!(service.received().len(), 2);
        board.backlog[0].agent.push_str("\n- preserve the full mandatory new contract");
        let changed = filter.judge_backlog(&board).unwrap();
        assert_eq!(changed.usage.requests, 1, "only the changed complete profile is rejudged");
        assert_eq!(service.received().len(), 3);
    }
    #[test]
    fn physical_attempts_include_retries_and_the_cache_adds_no_http_or_charge() {
        let service = FakeService::start(|number, _| {
            if number == 0 {
                Reply { status: 429, headers: vec![("retry-after", "0".into())], body: "ignored".into() }
            } else {
                Reply::json(200, &json!({"model":JEV_MODEL,"answers":{"q":{"type":"noul","noul":0.8}},"usage":{"input_tokens":123,"output_tokens":0}}))
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let mut filter = service.filter();
        filter.cache = Some(dir.path().into());
        filter.spec = Some("demo".into());
        let payload = json!({"model":JEV_MODEL,"state":"current evidence","questions":{"q":{"type":"noul","instructions":"Is it relevant?"}}}).to_string();
        let first = filter.send_all_for(std::slice::from_ref(&payload), Instant::now() + Duration::from_secs(2), Purpose::Context).unwrap();
        assert_eq!(first[0]["_attempts"], 2);
        let before = std::fs::read_to_string(dir.path().join("attempts.ndjson")).unwrap();
        let attempts = before.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).collect::<Vec<_>>();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0]["status"], 429);
        assert!(attempts[0]["input_tokens"].is_null());
        assert_eq!(attempts[1]["input_tokens"], 123);
        assert_eq!(attempts[1]["spec"], "demo");
        assert!(!before.contains(SECRET) && !before.contains("current evidence"));
        let cached = filter.send_all_for(&[payload], Instant::now() + Duration::from_secs(2), Purpose::Context).unwrap();
        assert_eq!(cached[0]["usage"]["input_tokens"], 0);
        assert_eq!(request_count(&cached[0]), 0);
        assert_eq!(service.received().len(), 2);
        assert_eq!(std::fs::read_to_string(dir.path().join("attempts.ndjson")).unwrap(), before);
    }
    #[test]
    fn shared_reading_never_becomes_a_conflict_but_flow_and_read_write_get_separate_evidence() {
        let mut first = board_task(11, &["writer.rs"]);
        let mut other = board_task(5, &["reader.rs"]);
        first.reads.push("contract.rs".into());
        other.reads.push("contract.rs".into());
        let board = Board { backlog: vec![first.clone()], running: vec![BoardWave { n: 7, tasks: vec![other.clone()] }] };
        assert_eq!(backlog_payloads(&board).unwrap().len(), 1, "common reading alone needs no Jev interference call");
        other.reads.push("writer.rs".into());
        let mut board = Board { backlog: vec![first], running: vec![BoardWave { n: 7, tasks: vec![other] }] };
        let parts = backlog_payloads(&board).unwrap();
        assert_eq!(parts.len(), 2);
        let pair: Value = serde_json::from_str(&parts[1]).unwrap();
        let relation = &pair["state"]["relations"]["w7:t11:t5"];
        assert_eq!(relation["shared_read"], true);
        assert_eq!(relation["read_write"], true);
        assert_eq!(relation["flow"], false);
        board.backlog[0].criteria.insert(90);
        board.running[0].tasks[0].criteria.insert(90);
        let parts = backlog_payloads(&board).unwrap();
        let pair: Value = serde_json::from_str(&parts[1]).unwrap();
        assert_eq!(pair["state"]["relations"]["w7:t11:t5"]["flow"], true);
        board.running[0].tasks[0].files = vec!["writer.rs".into()];
        assert_eq!(backlog_payloads(&board).unwrap().len(), 1, "exact write overlap is already locally reserved");
    }

}
