//! `jev` — o filtro da busca por assunto do mapa pelo Jev, um serviço pago
//! de fora que não escreve texto: lê um estado e perguntas e devolve a chance
//! de cada opção.
//!
//! Implementa a tomada [`MapFilter`] do núcleo. Um pedido só leva tudo, na
//! ordem do banco. O estado é um texto, um bloco por candidato: o id, o
//! caminho com as linhas da declaração, a documentação e as primeiras linhas
//! do código dela, lidas do arquivo do projeto; se o estado passa do
//! orçamento ([`STATE_TOKENS`]), as linhas de código caem de 24 para 8, 5 e 3.
//!
//! O pedido faz duas perguntas ao mesmo estado: `where`, uma escolha entre os
//! ids dos candidatos (qual é o código que o agente pediu), e `exists`, um
//! sim ou não (algum candidato é). Cada pergunta leva a frase de quem procura
//! e, quando existem, a descrição que o agente deu à busca e a última fala
//! dele antes dela. A chance de cada id e a de `exists` voltam para o
//! veredito e o corte do núcleo ([`judged`]), os mesmos de toda
//! implementação.
//!
//! O código do projeto vai ao Jev, e por isso todo texto que sai passa antes
//! pela procura de segredo, o estado e o contexto inclusive: o trecho com cara
//! de chave, senha ou token vai como "…". O arquivo lido nunca sai do projeto
//! (caminho absoluto ou com `..` não se lê), não é um arquivo sensível
//! (credenciais, chaves) e não passa de [`MAX_FILE_BYTES`].
//!
//! O modelo pedido é uma versão fixa, e o uso guarda o nome do modelo que a
//! resposta diz ter respondido.
//!
//! A chave vem de [`KEY_ENV`] no ambiente ou, sem ela, de `jev.key` no
//! `mustard.json` do projeto ([`load_key`]). O git não pode guardar esse
//! arquivo: guardado, a chave dele não se usa, e a busca avisa. Nenhum
//! comando a grava, e nenhum erro, aviso ou log a leva — nem o corpo da
//! resposta do serviço.

use std::collections::HashMap;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Component, Path};
use std::time::{Duration, Instant};

use mustard_core::domain::map_filter::{
    FilterCandidate, FilterError, FilterRequest, FilterUsage, Filtered, MapFilter, Scored, Verdict, judged,
};
use mustard_core::ProjectConfig;
use serde_json::{Map, Value, json};

use crate::shared::paths::sensitive_pattern;
use crate::shared::secret::without_secrets;

// ---------------------------------------------------------------------------
// O serviço
// ---------------------------------------------------------------------------

/// O endereço do serviço.
pub const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";

/// O modelo pedido: uma versão fixa, a que a medida usou. A versão mais nova
/// do serviço mudaria as notas sem aviso; a troca vem com medida nova.
pub const JEV_MODEL: &str = "jev-1.13.0";

/// US$ por milhão de tokens de entrada; a saída não é cobrada.
pub const PRICE_PER_MILLION_INPUT_TOKENS: f64 = 0.042;

/// A variável de ambiente da chave; vence o `mustard.json`.
pub const KEY_ENV: &str = "TYPESAFE_API_KEY";

/// O maior pedido que o serviço aceita, em tokens. Passando dele, com o
/// código já reduzido ao mínimo, o pedido não sai.
const MAX_REQUEST_TOKENS: u64 = 64_000;

/// O orçamento do estado, em tokens: acima dele, o código de cada candidato
/// perde linhas até caber. A frase de quem procura, que vai nas duas
/// perguntas, conta duas vezes.
const STATE_TOKENS: u64 = 28_000;

/// Caracteres por token, para estimar o pedido antes de mandar, como a
/// medida estimava.
const CHARS_PER_TOKEN: f64 = 3.2;

/// Quantas linhas do código de cada candidato vão no estado, da primeira
/// tentativa para a última: as que sobram quando o estado passa do orçamento.
const CODE_LINES: [usize; 4] = [24, 8, 5, 3];

/// Quantos caracteres da documentação vão por candidato.
const DOCUMENTATION_CHARS: usize = 300;

/// Quantos caracteres de cada linha de código vão.
const LINE_CHARS: usize = 160;

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
        // Um prazo só para a busca inteira: o agente espera.
        let deadline = started + self.timeouts.response;
        if request.candidates.is_empty() {
            return Ok(Filtered {
                verdict: Verdict::NotFound,
                kept: Vec::new(),
                usage: FilterUsage { model: String::new(), ..FilterUsage::default() },
            });
        }
        let sent = payload(request)?;
        let doc = self.send(&sent, deadline)?;
        let answer = read_answer(&doc, &request.candidates)?;
        let (verdict, kept) = judged(&answer.scores, answer.exists, request.cut);
        Ok(Filtered {
            verdict,
            kept,
            usage: FilterUsage {
                input_tokens: answer.input_tokens,
                millis: started.elapsed().as_millis() as u64,
                cost_micro_usd: cost_micro_usd(answer.input_tokens),
                model: model_of(&doc),
            },
        })
    }
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

/// O que a resposta do serviço disse.
#[derive(Debug, Clone, PartialEq)]
struct Answer {
    /// A chance de cada candidato ser o código, na ordem do banco.
    scores: Vec<Scored>,
    /// A chance de algum candidato ser o código.
    exists: f64,
    /// Os tokens de entrada que o pedido custou.
    input_tokens: u64,
}

/// A resposta lida do documento `doc`: a chance de cada candidato de
/// `candidates` na escolha `where` e a de `exists`. Falta de `answers`, de uma
/// chance ou do `exists` é resposta ilegível.
fn read_answer(doc: &Value, candidates: &[FilterCandidate]) -> Result<Answer, FilterError> {
    let answers = doc
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| FilterError::Unreadable("no answers".to_string()))?;
    let chances = answers
        .get(WHERE_KEY)
        .and_then(|answer| answer.get("probabilities"))
        .and_then(Value::as_object)
        .ok_or_else(|| FilterError::Unreadable("no chances".to_string()))?;
    let mut scores = Vec::with_capacity(candidates.len());
    for (at, candidate) in candidates.iter().enumerate() {
        let id = candidate_id(at);
        let score = chances
            .get(&id)
            .and_then(Value::as_f64)
            .ok_or_else(|| FilterError::Unreadable(format!("no chance for {id}")))?;
        scores.push(Scored { id: candidate.id, score });
    }
    let exists = answers
        .get(EXISTS_KEY)
        .and_then(|answer| answer.get("noul"))
        .and_then(Value::as_f64)
        .ok_or_else(|| FilterError::Unreadable("no answer to the existence".to_string()))?;
    Ok(Answer {
        scores,
        exists,
        input_tokens: doc.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0),
    })
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
/// duas perguntas do pedido.
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
            text.push_str(&format!(" The agent described this search as: \"{}\".", self.described));
        }
        if !self.said.is_empty() {
            text.push_str(&format!(" Just before it, the agent wrote: \"{}\".", self.said));
        }
        text
    }
}

/// O texto do pedido para `request`: o estado com as primeiras linhas de
/// código de cada candidato — 24, e menos (8, 5, 3) enquanto o estado passa do
/// orçamento — e as duas perguntas. Um pedido que ainda passa do limite do
/// serviço não sai ([`FilterError::TooLarge`]).
fn payload(request: &FilterRequest) -> Result<String, FilterError> {
    let context = Context::of(request);
    let mut sources = Sources { root: &request.root, files: HashMap::new() };
    let phrase = context.request.chars().count();
    let mut state = String::new();
    for lines in CODE_LINES {
        state = state_text(&request.candidates, &mut sources, lines);
        if tokens_of(state.chars().count() + 2 * phrase) <= STATE_TOKENS {
            break;
        }
    }
    let text = serde_json::to_string(&body(&state, &context, request.candidates.len()))
        .map_err(|_| FilterError::Unreadable("the request did not serialize".to_string()))?;
    let estimated = estimated_tokens(&text);
    if estimated > MAX_REQUEST_TOKENS {
        return Err(FilterError::TooLarge { estimated_tokens: estimated });
    }
    Ok(text)
}

/// O corpo de um pedido: o modelo, o estado e as duas perguntas, cada uma com
/// a frase e o contexto. As opções da escolha são os ids dos `count`
/// candidatos, sem descrição.
fn body(state: &str, context: &Context, count: usize) -> Value {
    let suffix = context.suffix();
    let request = &context.request;
    let mut criteria = Map::new();
    for at in 0..count {
        criteria.insert(candidate_id(at), Value::Null);
    }
    let mut questions = Map::new();
    questions.insert(
        WHERE_KEY.to_string(),
        json!({
            "type": "choice",
            "instructions": format!(
                "Which candidate is the code that answers this request from a coding agent: \"{request}\"?{suffix}"
            ),
            "criteria": Value::Object(criteria),
        }),
    );
    questions.insert(
        EXISTS_KEY.to_string(),
        json!({
            "type": "noul",
            "instructions": format!(
                "Does any candidate contain the code that answers this request from a coding agent: \"{request}\"?{suffix}"
            ),
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

/// O estado: um bloco por candidato, separados por uma linha em branco. O
/// bloco tem o id com o caminho e as linhas, a documentação num comentário e
/// até `lines` linhas do código da declaração, cada uma com quatro espaços de
/// recuo. Cada texto sai sem os segredos, antes dos cortes: o corte não parte
/// um segredo num trecho que a procura já não reconhece.
fn state_text(candidates: &[FilterCandidate], sources: &mut Sources<'_>, lines: usize) -> String {
    let blocks: Vec<String> = candidates
        .iter()
        .enumerate()
        .map(|(at, candidate)| {
            let mut block =
                format!("{}| {}:{}-{}", candidate_id(at), without_secrets(&candidate.path), candidate.line, candidate.end_line);
            let documentation: String = squash(&without_secrets(&candidate.documentation))
                .chars()
                .take(DOCUMENTATION_CHARS)
                .collect();
            if !documentation.is_empty() {
                block.push_str("\n    // ");
                block.push_str(&documentation);
            }
            for line in sources.excerpt(candidate, lines) {
                block.push('\n');
                block.push_str(&line);
            }
            block
        })
        .collect();
    blocks.join("\n\n")
}

/// Os arquivos do projeto de onde vem o código dos candidatos, cada um lido
/// uma vez.
struct Sources<'a> {
    root: &'a Path,
    /// O texto de cada arquivo já pedido; `None` no que não se lê.
    files: HashMap<String, Option<String>>,
}

impl Sources<'_> {
    /// As primeiras `lines` linhas da declaração de `candidate`, da primeira à
    /// última dela: cada uma sem o espaço do fim, com até [`LINE_CHARS`]
    /// caracteres e quatro espaços de recuo. Vazio quando o arquivo não se
    /// lê.
    fn excerpt(&mut self, candidate: &FilterCandidate, lines: usize) -> Vec<String> {
        let root = self.root;
        let Some(text) = self.files.entry(candidate.path.clone()).or_insert_with(|| read_source(root, &candidate.path)) else {
            return Vec::new();
        };
        let start = candidate.line.saturating_sub(1) as usize;
        let count = (candidate.end_line.max(candidate.line) as usize - start).min(lines);
        let taken: Vec<&str> = text.lines().skip(start).take(count).collect();
        if taken.is_empty() {
            return Vec::new();
        }
        without_secrets(&taken.join("\n"))
            .split('\n')
            .map(|line| format!("    {}", line.trim_end().chars().take(LINE_CHARS).collect::<String>()))
            .collect()
    }
}

/// O texto do arquivo `path` do projeto em `root`. `None` no caminho que sai
/// do projeto (absoluto ou com `..`), no de arquivo sensível (credenciais,
/// chaves), no que falta, não abre ou passa de [`MAX_FILE_BYTES`]; um byte que
/// não é texto vira o caractere de troca.
fn read_source(root: &Path, path: &str) -> Option<String> {
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative.components().any(|part| matches!(part, Component::ParentDir))
        || sensitive_pattern(path).is_some()
    {
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

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::map_filter::{CutRule, EXISTS_FROM, MAX_KEPT};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
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


    // -- os ajudantes do pedido e da resposta -------------------------------------

    /// Os ids dos candidatos de um pedido, na ordem: as opções da escolha.
    fn ids_of(body: &Value) -> Vec<String> {
        body["questions"][WHERE_KEY]["criteria"].as_object().unwrap().keys().cloned().collect()
    }

    /// A resposta do serviço ao pedido `body`: a chance de cada candidato
    /// pela posição dele na lista, a de `exists` e 1000 tokens de entrada.
    fn answer_by_position(body: &Value, chance_of: impl Fn(usize) -> f64, exists: f64) -> Value {
        let mut chances = Map::new();
        for (at, id) in ids_of(body).into_iter().enumerate() {
            chances.insert(id, json!(chance_of(at)));
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
    fn the_state_is_text_with_a_header_the_documentation_and_the_first_lines_of_each_candidate() {
        let code: Vec<String> = (1..=30).map(|n| format!("line {n}")).collect();
        let project = project_with(&[("src/pay.rs", code.join("\n"))]);
        let mut first = candidate_at(1, "src/pay.rs", 3, 30);
        first.documentation = "  Cobra   o cartão.\n  Sem juros.  ".to_string();
        let second = candidate_at(2, "src/pay.rs", 1, 2);
        let mut asked = request(vec![first, second]);
        asked.root = project.path().to_path_buf();

        let state = state_of(&sent_for(&asked));

        let first_block: Vec<String> = (3..=26).map(|n| format!("    line {n}")).collect();
        let expected = format!(
            "c000| src/pay.rs:3-30\n    // Cobra o cartão. Sem juros.\n{}\n\nc001| src/pay.rs:1-2\n    line 1\n    line 2",
            first_block.join("\n")
        );
        assert_eq!(state, expected, "24 lines of the declaration, the second block after a blank line");
    }

    #[test]
    fn long_documentation_and_long_lines_are_cut_and_the_end_of_a_line_loses_its_blanks() {
        let long_line = format!("{}   \t", "a".repeat(200));
        let project = project_with(&[("src/a.rs", format!("{long_line}\nshort   \n"))]);
        let mut only = candidate_at(1, "src/a.rs", 1, 2);
        only.documentation = "d".repeat(400);
        let mut asked = request(vec![only]);
        asked.root = project.path().to_path_buf();

        let state = state_of(&sent_for(&asked));

        let expected = format!("c000| src/a.rs:1-2\n    // {}\n    {}\n    short", "d".repeat(300), "a".repeat(160));
        assert_eq!(state, expected);
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

    #[test]
    fn the_code_shrinks_to_eight_five_and_three_lines_when_the_state_passes_the_budget() {
        let project = project_with(&[("src/big.rs", format!("{}\n", "a".repeat(160)).repeat(40))]);
        for (count, lines) in [(10, 24), (60, 8), (100, 5), (150, 3)] {
            let candidates = (1..=count).map(|id| candidate_at(id, "src/big.rs", 1, 40)).collect();
            let mut asked = request(candidates);
            asked.root = project.path().to_path_buf();

            let state = state_of(&sent_for(&asked));

            let blocks: Vec<&str> = state.split("\n\n").collect();
            assert_eq!(blocks.len(), count as usize);
            for block in blocks {
                assert_eq!(block.lines().count() - 1, lines, "{count} candidates keep {lines} code lines");
            }
            assert!(estimated_tokens(&state) <= STATE_TOKENS, "{count} candidates: {}", estimated_tokens(&state));
        }
    }

    /// A borda exata do orçamento: o estado com 24 linhas de código mais a
    /// frase duas vezes soma 89.600 caracteres, que dão 28.000 tokens e cabem;
    /// com um caractere a mais, são 28.001 e o código cai para 8 linhas. Sem a
    /// frase contada duas vezes, o caractere a mais ainda caberia.
    #[test]
    fn the_code_keeps_24_lines_up_to_the_exact_budget_counting_the_phrase_twice() {
        let budget = 89_600;
        assert_eq!((tokens_of(budget), tokens_of(budget + 1)), (STATE_TOKENS, STATE_TOKENS + 1));
        let project = project_with(&[("src/big.rs", format!("{}\n", "a".repeat(160)).repeat(40))]);
        // O pedido com `real` candidatos com código, um de cabeçalho só com
        // `pad` caracteres no caminho e uma frase de `phrase` caracteres.
        let build = |real: i64, pad: usize, phrase: usize| {
            let mut candidates: Vec<FilterCandidate> = (1..=real).map(|id| candidate_at(id, "src/big.rs", 1, 40)).collect();
            candidates.push(candidate_at(real + 1, &format!("p/{}", "q".repeat(pad)), 1, 1));
            let mut asked = request(candidates);
            asked.root = project.path().to_path_buf();
            asked.phrase = "x".repeat(phrase);
            asked
        };
        // Os caracteres que contam contra o orçamento com 24 linhas de código.
        let counted = |asked: &FilterRequest| {
            let mut sources = Sources { root: &asked.root, files: HashMap::new() };
            let state = state_text(&asked.candidates, &mut sources, CODE_LINES[0]);
            state.chars().count() + 2 * Context::of(asked).request.chars().count()
        };
        let mut real = 1;
        while counted(&build(real + 1, 1, 10)) <= budget {
            real += 1;
        }
        let spare = budget - counted(&build(real, 1, 10));
        let (pad, phrase) = (1 + spare % 2, 10 + spare / 2);
        let at_the_edge = build(real, pad, phrase);
        let over = build(real, pad + 1, phrase);
        assert_eq!((counted(&at_the_edge), counted(&over)), (budget, budget + 1));

        for (asked, lines) in [(at_the_edge, 24), (over, 8)] {
            let state = state_of(&sent_for(&asked));
            let blocks: Vec<&str> = state.split("\n\n").collect();
            assert_eq!(blocks.len(), real as usize + 1);
            for block in &blocks[..real as usize] {
                assert_eq!(block.lines().count() - 1, lines, "{lines} code lines for {} chars", counted(&asked));
            }
        }
    }

    #[test]
    fn a_secret_in_the_code_the_documentation_the_phrase_or_the_context_does_not_leave_the_machine() {
        let key = format!("ghp_{}", "a1B2c3D4".repeat(5));
        let secret = format!("DB_PASSWORD=S3nh4F0rte2024 {key}");
        let project =
            project_with(&[("src/pay.rs", format!("fn pay() {{\n    let token = \"{key}\";\n}}\n"))]);
        let mut leaky = candidate_at(1, "src/pay.rs", 1, 3);
        leaky.documentation = format!("Cobra o cartão. {secret}");
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
        assert_eq!(
            choice,
            format!("Which candidate is the code that answers this request from a coding agent: \"the candidate that answers\"?{suffix}")
        );
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
    fn the_cut_keeps_at_most_two_pieces_and_the_limit_is_the_setting() {
        let equal = |_: usize| 0.2;
        let kept = filtered(5, 0.9, CutRule::default(), equal);
        assert_eq!(MAX_KEPT, 2);
        assert_eq!(kept.kept.len(), 2, "five equal chances keep two");

        let three = filtered(5, 0.9, CutRule { max_kept: 3, ..CutRule::default() }, equal);
        assert_eq!(three.kept.len(), 3);

        let uneven = filtered(5, 0.9, CutRule::default(), |at| [0.6, 0.3, 0.05, 0.03, 0.02][at]);
        let ids: Vec<i64> = uneven.kept.iter().map(|scored| scored.id).collect();
        assert_eq!(ids, [1, 2], "in the order of the chance");

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
            Reply::json(200, &answer_by_position(body, |at| if at == 0 { 0.9 } else { 0.05 }, 0.9))
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

    /// Manda um pedido real de 2 candidatos, com o código lido de um arquivo
    /// do projeto, e a chave do ambiente, e confere a nota e os tokens. Custa
    /// uma fração de centavo; roda só à mão, com `--ignored` e a chave no
    /// ambiente.
    #[test]
    #[ignore = "calls the paid service with the key from the environment"]
    fn the_real_service_scores_two_candidates() {
        let key = std::env::var(KEY_ENV).expect("the key in the environment");
        let project = project_with(&[
            (
                "src/words.rs",
                "/// Splits an identifier into its words at case changes and separators.\npub fn split_identifier(name: &str) -> String {\n    name.to_string()\n}\n"
                    .to_string(),
            ),
            (
                "src/git.rs",
                "pub fn open_pull_request(title: &str) -> Result<u64, String> {\n    Err(title.to_string())\n}\n".to_string(),
            ),
        ]);
        let mut target = candidate_at(1, "src/words.rs", 2, 4);
        target.documentation = "Splits an identifier into its words at case changes and separators.".to_string();
        let other = candidate_at(2, "src/git.rs", 1, 3);
        let asked = FilterRequest {
            words: vec!["split_identifier".to_string()],
            phrase: "The function that splits an identifier into words.".to_string(),
            root: project.path().to_path_buf(),
            candidates: vec![target, other],
            ..FilterRequest::default()
        };
        let got = JevFilter::new(JevKey(key)).filter(&asked).unwrap();
        assert!(got.usage.input_tokens > 0);
        assert_eq!(got.verdict, Verdict::Found);
        assert_eq!(got.kept.first().map(|s| s.id), Some(1));
        assert!(got.kept.iter().all(|s| (0.0..=1.0).contains(&s.score)));
    }
}
