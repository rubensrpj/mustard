//! `jev` — o filtro da busca por assunto do mapa pelo Jev, um serviço pago
//! de fora que não escreve texto: lê um estado e uma pergunta por candidato e
//! devolve a chance do sim.
//!
//! Implementa a tomada [`MapFilter`] do núcleo. Os candidatos vão em grupos de
//! [`GROUP_SIZE`], na ordem do banco, todos ao mesmo tempo; cada grupo é um
//! pedido. As notas voltam para o [`cut`] do núcleo, o mesmo de toda
//! implementação.
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
//! A chave vale para a máquina inteira: [`KEY_ENV`] no ambiente ou o arquivo
//! [`KEY_FILE`] na pasta do Mustard da máquina ([`key_path`]). Nenhum projeto
//! a configura, nenhum comando a grava, e nenhum erro, aviso ou log a leva —
//! nem o corpo da resposta do serviço.

use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mustard_core::domain::map_filter::{
    FilterCandidate, FilterError, FilterRequest, FilterUsage, Filtered, MapFilter, Scored, cut,
};
use mustard_core::domain::normalize::split_identifier;
use serde_json::{Map, Value, json};

use crate::commands::spec_events::pages::secret::without_secrets;

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

/// A variável de ambiente da chave; vence o arquivo.
pub const KEY_ENV: &str = "TYPESAFE_API_KEY";

/// O arquivo da chave, na pasta do Mustard da máquina, com permissão 600.
pub const KEY_FILE: &str = "jev.key";

/// Candidatos por pedido. Com os 100 candidatos do banco, são 2 pedidos.
pub const GROUP_SIZE: usize = 50;

/// O maior pedido que o serviço aceita, em tokens. Com 50 candidatos, um
/// pedido fica perto de 10 mil.
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
/// passado o prazo, a resposta vem do banco. O serviço saudável responde um
/// pedido de 50 candidatos em menos de 1,1 s (o pior de 25 mil pedidos do
/// laboratório). Num período lento do serviço, 100 buscas esperaram de 0,4
/// a 30 s, sem degrau no meio: com 10 s, 9 delas iriam ao banco, e nenhuma
/// esperaria mais que isso.
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
description of what some code does, or the name, or part of the name, of an identifier. Each question shows \
one candidate declaration with what the code index knows about it: `kind`, `name` (its name split into words), \
`path` (its file path), `signature`, `documentation`, `last commits of the file` (the titles of the last commits \
that changed its file), `owner` (the names of the type or block that holds it), `comments in the body` (the \
comments inside its code) and `members` (the members it declares, when it is a type). The body of the code is \
not shown.";

const ANSWER_YES_WHEN: &str = "The candidate is the code the request asks for: it does, defines or decides what the request describes, \
or its name is the identifier the request names.";

const ANSWER_NO_WHEN: &str = "The candidate is only on a related topic, only uses or calls the thing the request describes, or only \
shares some words with the request.";

const QUESTION: &str = "Is `candidate` the code that the `request` in the state is looking for?";

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

/// A chave achada e, quando o arquivo dela está aberto a outros usuários da
/// máquina, o aviso para fechar a permissão. A chave vale do mesmo jeito.
#[derive(Debug, Clone)]
pub struct LoadedKey {
    pub key: JevKey,
    pub warning: Option<FilterError>,
}

/// Onde mora o arquivo da chave, a partir da pasta pessoal.
#[must_use]
pub fn key_path(home: &Path) -> PathBuf {
    home.join(".cache").join("mustard").join(KEY_FILE)
}

/// A chave da máquina: [`KEY_ENV`] no ambiente; sem ela, o arquivo em
/// [`key_path`]. Sem nenhuma das duas, [`FilterError::MissingKey`].
pub fn load_key() -> Result<LoadedKey, FilterError> {
    let file = mustard_core::platform::harness::home_dir().map(|home| key_path(&home));
    key_from(std::env::var(KEY_ENV).ok(), file.as_deref())
}

/// A escolha da chave, sobre o valor do ambiente e o caminho do arquivo. O
/// valor em branco vale como ausente.
fn key_from(env: Option<String>, file: Option<&Path>) -> Result<LoadedKey, FilterError> {
    if let Some(key) = env.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) {
        return Ok(LoadedKey { key: JevKey(key), warning: None });
    }
    let path = file.ok_or(FilterError::MissingKey)?;
    let text = std::fs::read_to_string(path).map_err(|_| FilterError::MissingKey)?;
    let key = text.trim();
    if key.is_empty() {
        return Err(FilterError::MissingKey);
    }
    Ok(LoadedKey { key: JevKey(key.to_string()), warning: open_to_others(path) })
}

/// O aviso para o arquivo da chave que outros usuários podem ler ou gravar.
#[cfg(unix)]
fn open_to_others(path: &Path) -> Option<FilterError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    (mode & 0o077 != 0).then(|| FilterError::KeyFileOpen { path: path.display().to_string(), mode })
}

/// Fora do Unix, a permissão não se lê em bits: não há aviso.
#[cfg(not(unix))]
fn open_to_others(_path: &Path) -> Option<FilterError> {
    None
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
    /// O filtro com a chave da máquina, no endereço do serviço.
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
        // Um prazo só para todos os grupos: eles correm juntos, e o agente
        // espera o mais lento.
        let deadline = started + self.timeouts.response;
        let state = state(request);
        let groups: Vec<&[FilterCandidate]> = request.candidates.chunks(GROUP_SIZE).collect();
        let mut payloads = Vec::with_capacity(groups.len());
        for group in &groups {
            let payload = serde_json::to_string(&body(&state, group))
                .map_err(|_| FilterError::Unreadable("the request did not serialize".to_string()))?;
            let estimated_tokens = estimated_tokens(&payload);
            if estimated_tokens > MAX_REQUEST_TOKENS {
                return Err(FilterError::TooLarge { estimated_tokens });
            }
            payloads.push(payload);
        }
        let answers: Vec<Result<Value, FilterError>> = std::thread::scope(|scope| {
            let running: Vec<_> =
                payloads.iter().map(|payload| scope.spawn(move || self.send(payload, deadline))).collect();
            running
                .into_iter()
                .map(|thread| {
                    thread.join().unwrap_or_else(|_| Err(FilterError::Network("the request thread stopped".to_string())))
                })
                .collect()
        });
        let mut scores = Vec::with_capacity(request.candidates.len());
        let mut input_tokens = 0;
        let mut models: Vec<String> = Vec::new();
        for (group, answer) in groups.iter().zip(answers) {
            let doc = answer?;
            input_tokens += read_scores(&doc, group, &mut scores)?;
            let model = doc.get("model").and_then(Value::as_str).map(str::trim).unwrap_or_default();
            if !model.is_empty() && !models.iter().any(|seen| seen == model) {
                models.push(model.to_string());
            }
        }
        Ok(Filtered {
            kept: cut(&scores, request.minimum),
            usage: FilterUsage {
                input_tokens,
                millis: started.elapsed().as_millis() as u64,
                cost_micro_usd: cost_micro_usd(input_tokens),
                model: models.join(","),
            },
        })
    }
}

/// O custo de `input_tokens` tokens de entrada em milionésimos de dólar,
/// pelo preço de tabela: o preço por milhão de tokens é o preço de cada
/// token em milionésimos.
fn cost_micro_usd(input_tokens: u64) -> u64 {
    (input_tokens as f64 * PRICE_PER_MILLION_INPUT_TOKENS).round() as u64
}

/// As notas de um grupo, na ordem do banco, somadas a `scores`; devolve os
/// tokens de entrada que o pedido custou. Falta de `answers` ou de uma nota é
/// resposta ilegível.
fn read_scores(doc: &Value, group: &[FilterCandidate], scores: &mut Vec<Scored>) -> Result<u64, FilterError> {
    let answers = doc
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| FilterError::Unreadable("no answers".to_string()))?;
    for (index, candidate) in group.iter().enumerate() {
        let score = answers
            .get(&question_id(index))
            .and_then(|answer| answer.get("noul"))
            .and_then(Value::as_f64)
            .ok_or_else(|| FilterError::Unreadable(format!("no score for {}", question_id(index))))?;
        scores.push(Scored { id: candidate.id, score });
    }
    Ok(doc.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0))
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

/// O estado, igual em todos os grupos: a frase (ou, sem ela, as palavras), as
/// palavras como palpites, o que é a busca e quando responder sim ou não.
fn state(request: &FilterRequest) -> Value {
    let asked = if request.phrase.trim().is_empty() { request.words.join(" ") } else { request.phrase.clone() };
    let words: Vec<String> = request.words.iter().map(|word| without_secrets(word)).collect();
    let mut state = Map::new();
    state.insert("request".to_string(), Value::String(without_secrets(&asked)));
    state.insert(GUESSED_WORDS.to_string(), json!(words));
    state.insert("about".to_string(), Value::String(ABOUT.to_string()));
    state.insert("answer yes when".to_string(), Value::String(ANSWER_YES_WHEN.to_string()));
    state.insert("answer no when".to_string(), Value::String(ANSWER_NO_WHEN.to_string()));
    Value::Object(state)
}

/// O corpo de um pedido: o estado, o modelo e uma pergunta por candidato, de
/// `c00` em diante.
fn body(state: &Value, group: &[FilterCandidate]) -> Value {
    let mut questions = Map::new();
    for (index, candidate) in group.iter().enumerate() {
        questions.insert(
            question_id(index),
            json!({"type": "noul", "instructions": {"question": QUESTION, "candidate": candidate_fields(candidate)}}),
        );
    }
    let mut body = Map::new();
    body.insert("state".to_string(), state.clone());
    body.insert("model".to_string(), Value::String(JEV_MODEL.to_string()));
    body.insert("questions".to_string(), Value::Object(questions));
    Value::Object(body)
}

fn question_id(index: usize) -> String {
    format!("c{index:02}")
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
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    const SECRET: &str = "sk-test-0123456789abcdef";

    fn test_key() -> JevKey {
        JevKey(SECRET.to_string())
    }

    /// Um endereço numa porta que acabou de ser liberada: ninguém ouve ali.
    fn closed_url() -> String {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        format!("http://127.0.0.1:{port}/v1/systemone")
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
    /// 0, e o corpo). Cada conexão tem a sua thread; `gather` segura as
    /// respostas até haver tantas conexões abertas ao mesmo tempo (ou 3 s),
    /// e `peak` guarda o maior número de conexões abertas juntas.
    struct FakeService {
        url: String,
        received: Arc<Mutex<Vec<Received>>>,
        peak: Arc<AtomicUsize>,
    }

    impl FakeService {
        fn start(gather: usize, respond: impl Fn(usize, &Value) -> Reply + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
            let received = Arc::new(Mutex::new(Vec::new()));
            let peak = Arc::new(AtomicUsize::new(0));
            let open = Arc::new(AtomicUsize::new(0));
            let respond: Arc<Responder> = Arc::new(respond);
            let (log, top) = (Arc::clone(&received), Arc::clone(&peak));
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let now = open.fetch_add(1, Ordering::SeqCst) + 1;
                    top.fetch_max(now, Ordering::SeqCst);
                    let (log, open, respond) = (Arc::clone(&log), Arc::clone(&open), Arc::clone(&respond));
                    std::thread::spawn(move || {
                        serve_one(stream, gather, &open, &log, respond.as_ref());
                        open.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            });
            Self { url, received, peak }
        }

        fn received(&self) -> Vec<Received> {
            self.received.lock().unwrap().clone()
        }

        fn filter(&self) -> JevFilter {
            JevFilter::at(test_key(), &self.url)
        }
    }

    fn serve_one(
        mut stream: TcpStream,
        gather: usize,
        open: &AtomicUsize,
        log: &Mutex<Vec<Received>>,
        respond: &Responder,
    ) {
        let Some(received) = read_request(&stream) else { return };
        let number = {
            let mut log = log.lock().unwrap();
            log.push(received.clone());
            log.len() - 1
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while open.load(Ordering::SeqCst) < gather && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
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

    /// A resposta do serviço a um pedido: a nota de cada pergunta pelo nome
    /// do candidato, e 1000 tokens de entrada.
    fn answer_by_name(body: &Value, score_of: impl Fn(&str) -> f64) -> Value {
        let mut answers = Map::new();
        for (id, question) in body["questions"].as_object().unwrap() {
            let name = question["instructions"]["candidate"]["name"].as_str().unwrap_or_default();
            answers.insert(id.clone(), json!({"type": "noul", "noul": score_of(name)}));
        }
        json!({"answers": answers, "usage": {"input_tokens": 1000, "output_tokens": 0}})
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
            minimum: 0,
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
            minimum: 0,
            candidates,
        };
        let service = FakeService::start(1, |_, body| Reply::json(200, &answer_by_name(body, |_| 0.5)));
        service.filter().filter(&asked).unwrap();

        let sent = service.received();
        assert_eq!(sent.len(), 1);
        // O arquivo guarda o pedido como o laboratório o mandou. As diferenças
        // planejadas são só duas: o `about`, que cita cada campo que vai no
        // candidato, e a versão fixa do modelo.
        let mut expected = example["body"].clone();
        expected["state"]["about"] = json!(ABOUT);
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
        let service = FakeService::start(1, |_, body| Reply::json(200, &answer_by_name(body, |_| 0.5)));
        service.filter().filter(&asked).unwrap();

        let sent = service.received()[0].body.to_string();
        assert!(!sent.contains("S3nh4F0rte2024"), "{sent}");
        assert!(!sent.contains(&key[..12]), "{sent}");
        assert!(sent.contains("Troca a chave …"), "the rest of the text still goes: {sent}");
        assert!(sent.contains("Cobra o cartão."), "{sent}");
    }

    #[test]
    fn the_usage_keeps_the_model_that_answered_and_the_cost_of_the_tokens() {
        let service = FakeService::start(2, |_, body| {
            let mut answer = answer_by_name(body, |_| 0.5);
            answer["model"] = json!("jev-1.13.0");
            answer["usage"]["input_tokens"] = json!(10_500);
            Reply::json(200, &answer)
        });
        let got = service.filter().filter(&request((1..=100).map(candidate).collect())).unwrap();
        assert_eq!(service.received().len(), 2);
        assert!(service.received().iter().all(|sent| sent.body["model"] == json!("jev-1.13.0")));
        assert_eq!(got.usage.input_tokens, 21_000);
        assert_eq!(got.usage.model, "jev-1.13.0");
        // 21 mil tokens a US$ 0,042 o milhão: US$ 0,000882.
        assert_eq!(got.usage.cost_micro_usd, 882);

        let silent = FakeService::start(1, |_, body| Reply::json(200, &answer_by_name(body, |_| 0.5)));
        let got = silent.filter().filter(&request(vec![candidate(1)])).unwrap();
        assert_eq!(got.usage.model, "", "an answer that does not say the model leaves it empty");
    }

    #[test]
    fn long_comments_are_cut_at_a_word_and_many_members_are_counted() {
        let mut long = candidate(1);
        long.body_comments = "abcdefghi ".repeat(70);
        assert_eq!(long.body_comments.chars().count(), 700);
        long.members = (1..=20).map(|n| format!("member{n}()")).collect();
        let service = FakeService::start(1, |_, body| Reply::json(200, &answer_by_name(body, |_| 0.5)));
        service.filter().filter(&request(vec![long])).unwrap();

        let sent = &service.received()[0].body["questions"]["c00"]["instructions"]["candidate"];
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
        assert_eq!(state(&asked)["request"], "split identifier");
    }

    // -- os grupos e a nota ----------------------------------------------------

    #[test]
    fn two_hundred_candidates_go_as_four_requests_at_the_same_time() {
        let service = FakeService::start(4, |_, body| {
            Reply::json(
                200,
                &answer_by_name(body, |name| match name {
                    "cand7" => 0.9,
                    "cand180" => 0.8,
                    "cand120" => 0.55,
                    _ => 0.1,
                }),
            )
        });
        let got = service.filter().filter(&request((1..=200).map(candidate).collect())).unwrap();

        let sent = service.received();
        assert_eq!(sent.len(), 4);
        assert_eq!(service.peak.load(Ordering::SeqCst), 4, "the four requests were not open at the same time");
        for received in &sent {
            let ids: Vec<&String> = received.body["questions"].as_object().unwrap().keys().collect();
            assert_eq!(ids.len(), GROUP_SIZE);
            assert_eq!(ids.first().map(|s| s.as_str()), Some("c00"));
            assert_eq!(ids.last().map(|s| s.as_str()), Some("c49"));
        }
        let kept: Vec<(i64, f64)> = got.kept.iter().map(|s| (s.id, s.score)).collect();
        assert_eq!(kept, vec![(7, 0.9), (180, 0.8), (120, 0.55)]);
        assert_eq!(got.usage.input_tokens, 4000);
    }

    #[test]
    fn the_minimum_travels_to_the_cut() {
        let service = FakeService::start(1, |_, body| {
            Reply::json(200, &answer_by_name(body, |name| if name == "cand2" { 0.95 } else { 0.3 }))
        });
        let mut asked = request((1..=3).map(candidate).collect());
        asked.minimum = 2;
        let got = service.filter().filter(&asked).unwrap();
        let ids: Vec<i64> = got.kept.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![2, 1]);
    }

    #[test]
    fn no_candidates_send_no_request() {
        // Porta que ninguém ouve: um pedido que saísse viraria erro de rede.
        let got = JevFilter::at(test_key(), &closed_url()).filter(&request(Vec::new())).unwrap();
        assert!(got.kept.is_empty());
        assert_eq!(got.usage.input_tokens, 0);
    }

    // -- as recusas e as repetições -------------------------------------------

    #[test]
    fn a_refused_key_or_missing_credit_fails_without_repeating() {
        for status in [401, 402] {
            let service = FakeService::start(1, move |_, _| {
                Reply::json(status, &json!({"error": format!("bad key {SECRET}")}))
            });
            let error = service.filter().filter(&request(vec![candidate(1)])).unwrap_err();
            assert_eq!(error, FilterError::Refused { status });
            assert_eq!(service.received().len(), 1, "HTTP {status} was repeated");
        }
    }

    #[test]
    fn too_many_requests_repeats_twice_and_gives_up() {
        let service = FakeService::start(1, |_, _| Reply {
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
        let service = FakeService::start(1, |number, body| {
            if number == 0 {
                Reply { status: 503, headers: vec![("Retry-After", "0".to_string())], body: String::new() }
            } else {
                Reply::json(200, &answer_by_name(body, |_| 0.7))
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
        let service = FakeService::start(1, |_, _| Reply::json(200, &json!({"usage": {"input_tokens": 10}})));
        let error = service.filter().filter(&request(vec![candidate(1)])).unwrap_err();
        assert!(matches!(error, FilterError::Unreadable(_)), "{error:?}");
    }

    #[test]
    fn an_answer_missing_a_score_is_unreadable() {
        let service = FakeService::start(1, |_, _| Reply::json(200, &json!({"answers": {"c00": {"noul": 0.5}}})));
        let error = service.filter().filter(&request(vec![candidate(1), candidate(2)])).unwrap_err();
        assert!(matches!(error, FilterError::Unreadable(_)), "{error:?}");
    }

    #[test]
    fn one_failing_group_fails_the_whole_filter() {
        let service = FakeService::start(1, |number, body| {
            if number == 1 {
                Reply::json(402, &json!({}))
            } else {
                Reply::json(200, &answer_by_name(body, |_| 0.9))
            }
        });
        let error = service.filter().filter(&request((1..=120).map(candidate).collect())).unwrap_err();
        assert_eq!(error, FilterError::Refused { status: 402 });
    }

    #[test]
    fn a_slow_service_times_out() {
        let service = FakeService::start(1, |_, body| {
            std::thread::sleep(Duration::from_millis(1500));
            Reply::json(200, &answer_by_name(body, |_| 0.9))
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
        let service = FakeService::start(1, |_, _| {
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
        let service = FakeService::start(1, |_, _| Reply {
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

    #[test]
    fn a_request_above_the_service_limit_is_not_sent() {
        let service = FakeService::start(1, |_, body| Reply::json(200, &answer_by_name(body, |_| 0.9)));
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
        let service = FakeService::start(1, |_, body| Reply::json(200, &answer_by_name(body, |_| 0.9)));
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
        let refused = FakeService::start(1, {
            let echo = echo.clone();
            move |_, _| Reply::json(401, &echo)
        });
        let unreadable = FakeService::start(1, move |_, _| Reply::json(200, &echo));
        let garbled = FakeService::start(1, |_, _| Reply {
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
            FilterError::KeyFileOpen { path: "/home/x/.cache/mustard/jev.key".to_string(), mode: 0o644 },
        ]);
        for error in errors {
            assert!(!error.to_string().contains(SECRET), "{error}");
            assert!(!format!("{error:?}").contains(SECRET), "{error:?}");
        }
    }

    #[test]
    fn the_environment_key_comes_before_the_file() {
        let home = tempfile::tempdir().unwrap();
        let file = key_path(home.path());
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "from-file\n").unwrap();

        let loaded = key_from(Some(" from-env ".to_string()), Some(&file)).unwrap();
        assert_eq!(loaded.key.0, "from-env");
        let loaded = key_from(Some("  ".to_string()), Some(&file)).unwrap();
        assert_eq!(loaded.key.0, "from-file");
        let loaded = key_from(None, Some(&file)).unwrap();
        assert_eq!(loaded.key.0, "from-file");
    }

    #[test]
    fn without_a_key_anywhere_the_filter_has_no_key() {
        let home = tempfile::tempdir().unwrap();
        let file = key_path(home.path());
        assert_eq!(key_from(None, Some(&file)).unwrap_err(), FilterError::MissingKey);
        assert_eq!(key_from(None, None).unwrap_err(), FilterError::MissingKey);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, " \n").unwrap();
        assert_eq!(key_from(None, Some(&file)).unwrap_err(), FilterError::MissingKey);
    }

    #[test]
    fn the_key_file_lives_in_the_machine_folder() {
        let home = Path::new("/home/someone");
        assert_eq!(key_path(home), home.join(".cache").join("mustard").join("jev.key"));
    }

    #[cfg(unix)]
    #[test]
    fn a_key_file_open_to_others_still_counts_and_warns() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let file = key_path(home.path());
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, SECRET).unwrap();

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let closed = key_from(None, Some(&file)).unwrap();
        assert!(closed.warning.is_none());

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let open = key_from(None, Some(&file)).unwrap();
        assert_eq!(open.key.0, SECRET);
        let warning = open.warning.unwrap();
        assert!(matches!(warning, FilterError::KeyFileOpen { mode: 0o644, .. }), "{warning:?}");
        assert!(warning.to_string().contains("chmod 600"));
        assert!(!warning.to_string().contains(SECRET));
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
            minimum: 0,
            candidates: vec![target, other],
        };
        let got = JevFilter::new(JevKey(key)).filter(&asked).unwrap();
        assert!(got.usage.input_tokens > 0);
        assert_eq!(got.kept.first().map(|s| s.id), Some(1));
        assert!(got.kept.iter().all(|s| (0.0..=1.0).contains(&s.score)));
    }
}
