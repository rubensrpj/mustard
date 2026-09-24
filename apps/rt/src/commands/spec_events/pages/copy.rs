//! A cópia da spec para o banco de dados da página publicada dela.
//!
//! O claude.ai não enxerga o disco da máquina: a página publicada de uma spec
//! é um template do Mustard que lê um banco de dados guardado junto dela
//! (`mustard_core::platform::page_templates`). O binário não monta página
//! nenhuma; nos marcos (a aprovação, o fim de cada rodada e o fechamento) e
//! uma vez depois de cada pedido do usuário que muda o plano, na gravação que
//! leva `--copy`, ele prepara em arquivos o que vai para o banco, e o
//! orquestrador copia esses arquivos ele mesmo, sem agente, com a ferramenta do
//! banco (`ArtifactData`), sem ler os itens.
//!
//! ## O que a cópia leva
//!
//! - cada faixa de [`RANGE_WIDTH`] números tocada por um item com número
//!   maior que o do último copiado, ou por um expurgo gravado depois dele: a
//!   faixa vai inteira, montada do arquivo de eventos, não só o que é novo
//!   nela. O último copiado é o maior `last` das cópias da página da spec
//!   gravadas depois da última publicação do template dela que deu certo;
//!   sem cópia depois dela, a página acabou de nascer, e a cópia leva a spec
//!   inteira (toda faixa que tem item). O número decide, não o horário:
//!   vários itens caem no mesmo segundo;
//! - sem os registros internos ([`LEFT_OUT`]): o texto que um gancho colocou
//!   na conversa, a chamada de um comando e o aviso de um gancho;
//! - sem o item que guarda um trecho com cara de segredo: ele fica fora até
//!   ser expurgado, e o marco diz o código dele;
//! - a faixa que ficou sem nenhum item sai do banco (`delete`);
//! - o documento das coisas calculadas, trocado a cada cópia: o estado de cada
//!   onda, o pedido de cada onda que ainda não saiu e a economia do rtk.
//!
//! A faixa que passar de [`RANGE_MAX_BYTES`] se parte em pedaços, em ordem,
//! dentro dela mesma: o primeiro pedaço leva o nome do início da faixa, e os
//! seguintes o nome do início com o número do pedaço (`2000`, `2000-2`,
//! `2000-3`…).
//!
//! Cada documento vai num arquivo JSON próprio, dentro de `copy/` na pasta da
//! spec, e cada lote (`spec-<n>.json`) é a lista `writes` de uma chamada da
//! ferramenta do banco, com até [`BATCH_MAX`] escritas. A cópia feita vira um
//! registro `copy` na spec, gravado pela conversa, com o número até onde a
//! cópia foi: é por ele que a cópia seguinte começa.
//!
//! ## A primeira cópia e a spec antiga
//!
//! A página da spec é o template do Mustard, publicado com `template: true`.
//! A publicação sem essa marca é a página inteira que uma versão antiga
//! publicou: ela não tem banco de dados e fica parada, como um retrato. Por
//! isso a spec sem template, antiga ou nova, ganha o template no primeiro
//! marco, num link novo, e a barra de status passa a mostrar esse link.
//!
//! A primeira cópia leva a spec inteira, e o orquestrador a copia como
//! qualquer outra, lote por lote, sem agente: cada lote é uma chamada da
//! ferramenta do banco, que lê os documentos pelo `file_path` sem passar os
//! itens pela conversa. Assim a página já tem os itens quando a pergunta de
//! aprovação é feita. Se a cópia não foi gravada, o marco seguinte manda de
//! novo a spec inteira, no mesmo link.
//!
//! Quando o template nasce num marco que não é a aprovação — a spec foi
//! aprovada por uma versão antiga —, a cópia segue igual, sem nota nenhuma.
//!
//! ## O molde novo na página já publicada
//!
//! A publicação do template grava em `stamp` o carimbo do molde publicado: a
//! versão do Mustard e a impressão do conteúdo montado
//! (`page_templates::template_stamp`). Num marco, quando o molde que o
//! programa rodando monta tem outro carimbo que o da última publicação que deu
//! certo, ou ela não tem carimbo, o molde novo é escrito no projeto e a ordem
//! manda publicá-lo de novo no mesmo endereço, antes dos lotes, e gravar a
//! publicação com o carimbo. O banco da página continua no mesmo endereço:
//! a cópia depois da república segue de onde parou, sem levar a spec inteira
//! de novo. Com o mesmo carimbo, nada é publicado de novo. O mesmo vale para
//! a página do projeto, cujo carimbo é o da última publicação dela gravada em
//! qualquer spec do projeto.
//!
//! ## A linha da spec na página do projeto
//!
//! Nos marcos, a linha da spec vai para o banco da página do projeto quando a
//! fase dela mudou desde a última cópia dela (o `phase` do último `copy` da
//! página do projeto). Com a página do projeto ainda sem endereço, ou
//! publicada de novo nesta spec depois da última cópia, vão todas as linhas do
//! índice. Os lotes dela são `project-<n>.json`.
//!
//! A página do projeto que uma versão antiga publicou inteira não tem banco:
//! a linha do projeto do índice guarda o endereço dela sem a marca do
//! template (`domain::spec_index::project_page`). O primeiro marco a trata
//! como a página ainda sem endereço: manda publicar o template num link novo
//! e copiar todas as linhas para ele. A antiga fica parada, e nenhum lote vai
//! para ela.
//!
//! A preparação inteira — ler o arquivo de eventos, apagar a cópia anterior e
//! gravar os arquivos novos — acontece com a trava do arquivo de eventos
//! presa: duas rodadas ao mesmo tempo nunca misturam os arquivos de uma com os
//! da outra, e nenhuma gravação entra no meio.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{BlockQuery, Hidden, Refusal, SpecEvent, SpecLog, PURGED_MARK};
use mustard_core::domain::spec_index::{is_template, project_page, published_to, ProjectRow, PROJECT_PAGE, SPEC_PAGE};
use mustard_core::io::spec_events as store;
use mustard_core::io::spec_index::later;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::platform::page_templates::{
    project_page_template, spec_page_template, template_stamp, COMPUTED, PROJECT_CAPABILITIES, RANGES,
    RANGE_MAX_BYTES, RANGE_WIDTH, SPECS, SPEC_CAPABILITIES,
};
use mustard_core::view::document::{RtkDay, WaveState};
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use super::relative;

/// A pasta da cópia, dentro da pasta da spec.
pub(crate) const FOLDER: &str = "copy";

/// Quantas escritas cabem numa chamada da ferramenta do banco.
pub(crate) const BATCH_MAX: usize = 50;

/// Os registros internos, que a cópia não leva: o texto que um gancho colocou
/// na conversa, a chamada de um comando e o aviso de um gancho.
pub(crate) const LEFT_OUT: &[&str] = &["injection", "hook", "call"];

/// O template da página da spec, como a instalação o deixa no projeto.
pub(crate) const SPEC_TEMPLATE: &str = ".claude/mustard/pages/spec.html";

/// O template da página do projeto, como a instalação o deixa no projeto.
pub(crate) const PROJECT_TEMPLATE: &str = ".claude/mustard/pages/project.html";

/// A cópia preparada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prepared {
    /// A pasta da cópia, relativa ao projeto.
    pub folder: String,
    pub spec: Target,
    /// A página do projeto, quando alguma linha vai para o banco dela.
    pub project: Option<Target>,
    /// O código de cada item que guarda um trecho com cara de segredo e por
    /// isso fica fora da cópia, na ordem do arquivo.
    pub withheld: Vec<String>,
}

/// O que vai para o banco de uma página.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// O endereço da página; sem ele, a página ainda não foi publicada.
    pub url: Option<String>,
    /// Os lotes, relativos ao projeto, na ordem em que vão.
    pub batches: Vec<String>,
    /// O `--json` do `run write copy` que grava a cópia feita.
    pub record: Value,
    /// A página já foi publicada inteira por uma versão antiga, sem banco: o
    /// template sai num link novo, e a antiga fica parada.
    pub old: bool,
    /// A página já tem endereço, mas foi publicada com um molde de outro
    /// carimbo, ou sem carimbo: o marco a publica de novo no mesmo endereço,
    /// antes dos lotes.
    pub republish: bool,
    /// O carimbo do molde que o programa rodando monta, gravado com a
    /// publicação.
    pub stamp: String,
    /// `{coleção}/{doc_id}` de cada documento que já existe no banco antes
    /// desta cópia: o banco recusa a troca de um documento assim sem a
    /// versão dele. Vazio na primeira cópia, quando nada existe ainda.
    pub existing: Vec<String>,
}

/// Prepara a cópia da spec `spec` do projeto `root` para o banco da página
/// dela e a da linha dela para o banco da página do projeto, quando a fase
/// mudou desde a última cópia dela. É a mesma cópia nos marcos e depois de um
/// pedido do usuário: quem manda publicar a página que ainda não tem endereço
/// é a ordem de um marco ([`Prepared::order`]).
///
/// # Errors
///
/// A recusa do nome da spec, a da spec sem arquivo de eventos e a falha de
/// gravação dos arquivos.
pub(crate) fn prepare(root: &Path, spec: &str, lang: Locale) -> Result<Prepared, Refusal> {
    let paths = ClaudePaths::for_project(root).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    let spec_paths = paths.for_spec(spec.trim()).map_err(|_| Refusal::BadSpecName { spec: spec.to_string() })?;
    prepare_in(root, spec, &spec_paths.spec_ndjson_path(), spec_paths.dir().join(FOLDER), lang)
}

/// A cópia de um marco da spec `spec`, lida do arquivo de eventos
/// `spec_ndjson` e gravada em `copy_folder`, em vez de onde [`ClaudePaths`]
/// os poria: o descarte usa isto para ler o arquivo já movido para a pasta
/// arquivada, ou para gravar os lotes numa pasta temporária quando a pasta da
/// spec vai ser apagada, e não sobreviveria até a cópia acabar.
///
/// # Errors
///
/// As recusas de [`prepare`].
pub(crate) fn prepare_milestone_at(
    root: &Path,
    spec: &str,
    spec_ndjson: &Path,
    copy_folder: PathBuf,
    lang: Locale,
) -> Result<Prepared, Refusal> {
    prepare_in(root, spec, spec_ndjson, copy_folder, lang)
}

/// O núcleo de [`prepare`] e [`prepare_milestone_at`]: lê `spec_ndjson` com a
/// trava presa e grava os lotes em `copy_folder`.
fn prepare_in(
    root: &Path,
    spec: &str,
    spec_ndjson: &Path,
    copy_folder: PathBuf,
    lang: Locale,
) -> Result<Prepared, Refusal> {
    let paths = ClaudePaths::for_project(root).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    // O rtk roda antes da trava: ninguém espera por ele para gravar.
    let rtk = super::rtk_days(root);
    let index = paths.spec_index_path();
    // As publicações da página do projeto gravadas nas outras specs também
    // são lidas antes da trava: a leitura de cada arquivo pega a trava dele,
    // e a desta spec já estaria presa. Só com a página do projeto já
    // publicada como template há carimbo a conferir.
    let others =
        if template_project_url(&index).is_some() { other_project_logs(root, spec.trim()) } else { Vec::new() };
    let place = Place { root, spec: spec.trim(), folder: copy_folder, index };
    store::with_locked_log(spec_ndjson, |log| build(&place, log, &rtk, &others, lang))?
        .unwrap_or_else(|| Err(Refusal::NoSpecFile { spec: spec.trim().to_string() }))
}

/// Onde a cópia de uma spec é preparada.
struct Place<'a> {
    root: &'a Path,
    spec: &'a str,
    folder: PathBuf,
    index: PathBuf,
}

/// Monta e grava a cópia a partir de `log`, lido com a trava presa.
fn build(
    place: &Place,
    log: &SpecLog,
    rtk: &[RtkDay],
    others: &[ProjectLog],
    lang: Locale,
) -> Result<Prepared, Refusal> {
    let SpecPage { url, since, old, republished, stamp: published } = spec_page(log);
    clear(&place.folder)?;
    let mut writes: Vec<Value> = Vec::new();
    let ranges = dirty_ranges(log, since);
    // O documento de uma faixa que começa até `since` já está no banco desde
    // uma cópia anterior; o mesmo vale para o documento calculado, fora da
    // primeira cópia. Na primeira cópia (`since == 0`) nada existe ainda,
    // nem a faixa que começa em zero. A ordem lê a versão de cada um antes
    // de trocar.
    let mut existing: Vec<String> = ranges
        .iter()
        .filter(|&&start| since != 0 && start <= since)
        .map(|&start| format!("{RANGES}/{start}"))
        .collect();
    for start in &ranges {
        writes.extend(range_writes(place, log, *start)?);
    }
    let (collection, doc) = COMPUTED.split_once('/').unwrap_or((COMPUTED, "current"));
    writes.push(set(place, collection, doc, &computed(place, log, rtk, lang))?);
    // Uma república no mesmo endereço zera `since`, mas o documento calculado
    // já está no banco desde a primeira publicação daquele endereço: a
    // primeira cópia depois da república também tem de nomeá-lo.
    if since != 0 || republished {
        existing.push(COMPUTED.to_string());
    }
    let template = spec_page_template(lang);
    let stamp = template_stamp(&template).unwrap_or_default().to_string();
    // A ordem de um marco publica de novo; depois de um pedido, a página com
    // molde velho fica para o próximo marco.
    let republish = url.is_some() && published.as_deref() != Some(stamp.as_str());
    if url.is_none() || republish {
        ensure_template(place.root, SPEC_TEMPLATE, &template)?;
    }
    let spec = Target {
        url,
        batches: batches(place, "spec", &writes)?,
        record: json!({ "page": SPEC_PAGE, "last": log.max_id() }),
        old,
        republish,
        stamp,
        existing,
    };
    let project = project_rows(place, log, others, lang)?;
    Ok(Prepared { folder: relative(place.root, &place.folder), spec, project, withheld: withheld(log) })
}

/// A página da spec, como o arquivo de eventos a conta.
struct SpecPage {
    /// O endereço da última publicação do template que deu certo.
    url: Option<String>,
    /// O número do último item copiado para o banco dela: o maior `last` das
    /// cópias gravadas para esse endereço, ou zero. A república no mesmo
    /// endereço não zera a conta: o banco continua lá.
    since: u64,
    /// Uma versão antiga publicou a página inteira, sem template.
    old: bool,
    /// O endereço da última publicação já tinha sido publicado antes dela: o
    /// documento calculado já está no banco desde essa publicação anterior.
    republished: bool,
    /// O carimbo do molde dessa publicação; `None` na publicação de antes do
    /// carimbo.
    stamp: Option<String>,
}

/// A página da spec do arquivo `log`. Só a publicação do template conta: a
/// página inteira de uma versão antiga não tem banco de dados.
fn spec_page(log: &SpecLog) -> SpecPage {
    let visible = log.visible();
    let published: Vec<(u64, &str, bool, Option<&str>)> = visible
        .iter()
        .filter_map(|e| published_to(e, SPEC_PAGE).map(|url| (e.id, url, is_template(e), e.str_field("stamp"))))
        .collect();
    let old = published.iter().any(|(_, _, template, _)| !template);
    let Some((at, url, _, stamp)) = published.iter().copied().rfind(|(_, _, template, _)| *template) else {
        return SpecPage { url: None, since: 0, old, republished: false, stamp: None };
    };
    let republished = published.iter().any(|&(id, other, _, _)| id < at && other == url);
    // Cada cópia foi para o endereço da última publicação do template antes
    // dela: conta a que foi para o endereço de agora, mesmo antes de uma
    // república nele.
    let mut current: Option<&str> = None;
    let mut since = 0;
    for event in &visible {
        if let Some(to) = published_to(event, SPEC_PAGE).filter(|_| is_template(event)) {
            current = Some(to);
        } else if copy_of(event) == Some(SPEC_PAGE) && current == Some(url) {
            since = since.max(event.int("last").unwrap_or(0));
        }
    }
    SpecPage { url: Some(url.to_string()), since, old, republished, stamp: stamp.map(str::to_string) }
}

/// Uma publicação do template da página do projeto que deu certo, gravada
/// numa spec: o número dela no arquivo, a hora, o endereço e o carimbo do
/// molde publicado.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectPublication {
    id: u64,
    at: String,
    url: String,
    stamp: Option<String>,
}

/// Uma cópia para o banco da página do projeto gravada numa spec: o número
/// dela no arquivo e a hora.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectCopy {
    id: u64,
    at: String,
}

/// A página do projeto como o arquivo de eventos de uma spec a conta: a hora
/// do primeiro evento, quando a pasta da spec nasceu, e as publicações do
/// template e as cópias para o banco dela, na ordem do arquivo.
#[derive(Debug, Clone, Default)]
struct ProjectLog {
    born: Option<String>,
    publications: Vec<ProjectPublication>,
    copies: Vec<ProjectCopy>,
}

/// A página do projeto como o arquivo `log` a conta.
fn project_log(log: &SpecLog) -> ProjectLog {
    let visible = log.visible();
    let publications = visible
        .iter()
        .filter(|e| is_template(e))
        .filter_map(|e| {
            published_to(e, PROJECT_PAGE).map(|url| ProjectPublication {
                id: e.id,
                at: e.at().to_string(),
                url: url.to_string(),
                stamp: e.str_field("stamp").map(str::to_string),
            })
        })
        .collect();
    let copies = visible
        .iter()
        .filter(|e| copy_of(e) == Some(PROJECT_PAGE))
        .map(|e| ProjectCopy { id: e.id, at: e.at().to_string() })
        .collect();
    ProjectLog { born: log.events.first().map(|e| e.at().to_string()), publications, copies }
}

/// A página do projeto como a contam as specs vivas do projeto `root` fora
/// da spec `spec`.
fn other_project_logs(root: &Path, spec: &str) -> Vec<ProjectLog> {
    mustard_core::io::spec_index::read_specs(root)
        .into_iter()
        .filter(|(name, _)| name != spec)
        .map(|(_, log)| project_log(&log))
        .collect()
}

/// O endereço da página do projeto no índice `index`, quando ela é o
/// template do Mustard.
fn template_project_url(index: &Path) -> Option<String> {
    let content = mustard_core::io::fs::lock::read_shared(index).ok()?;
    project_page(&content).filter(|page| page.template).map(|page| page.url)
}

/// A página de um registro de cópia.
fn copy_of(event: &SpecEvent) -> Option<&str> {
    (event.event_type == "copy").then(|| event.str_field("page")).flatten()
}

/// O item como vai para o banco: a linha do arquivo, sem o campo de busca,
/// que a página não usa. `None` quando ele guarda um trecho com cara de
/// segredo.
fn body_of(event: &SpecEvent) -> Option<Value> {
    let mut fields = event.fields.clone();
    fields.remove("search");
    let body = Value::Object(fields);
    (!holds_secret(&body)).then_some(body)
}

/// A linha que a cópia nunca leva: o registro interno, a linha que o formato
/// antigo do expurgo esvaziou, que saiu da leitura, e a volta que o próprio
/// agente gravou (`returned`), que a página só conhece pela versão oficial
/// que a rodada ou o fechamento grava no lugar dela.
fn never_copied(event: &SpecEvent, log_hidden: &std::collections::BTreeMap<u64, Hidden>) -> bool {
    LEFT_OUT.contains(&event.event_type.as_str())
        || matches!(log_hidden.get(&event.id), Some(Hidden::Purged { .. }))
        || event.returned()
}

/// Os itens com número maior que `since` que vão para o banco, em ordem de
/// número, cada um com o corpo dele. O item com cara de segredo fica fora.
fn items_after(log: &SpecLog, since: u64) -> Vec<(u64, Value)> {
    let hidden = log.hidden();
    log.events
        .iter()
        .filter(|e| e.id > since && !never_copied(e, &hidden))
        .filter_map(|e| body_of(e).map(|body| (e.id, body)))
        .collect()
}

/// Os itens de número até `since` que um expurgo gravado depois de `since`
/// tocou: todas as versões de cada alvo e, num ponto do levantamento, o outro
/// lado do par. Cada um vai com a versão limpa, que substitui a do banco, ou
/// sem corpo, quando ainda guarda um trecho com cara de segredo e sai do banco.
fn purged_since(log: &SpecLog, since: u64) -> Vec<(u64, Option<Value>)> {
    let codes = log.codes();
    let mut touched: BTreeSet<u64> = BTreeSet::new();
    for purge in log.events.iter().filter(|e| e.id > since && e.event_type == "purge") {
        for target in purge.fields.get("targets").and_then(Value::as_array).into_iter().flatten() {
            let found: Vec<u64> = match (target.as_u64(), target.as_str()) {
                (Some(id), _) => vec![id],
                (None, Some(code)) => {
                    codes.iter().filter(|(_, c)| c.as_str() == code.trim()).map(|(id, _)| *id).collect()
                }
                _ => Vec::new(),
            };
            for id in found {
                // Todas as versões do item têm o mesmo código.
                let code = codes.get(&id);
                touched.extend(codes.iter().filter(|(_, c)| Some(*c) == code).map(|(other, _)| *other));
                touched.insert(id);
            }
        }
    }
    // O fechamento de um ponto copia a lacuna do original: o expurgo toca os
    // dois lados do par.
    let pairs: Vec<u64> = log
        .events
        .iter()
        .filter(|e| e.event_type == "point")
        .filter_map(|e| e.int("closes").map(|closes| (e.id, closes)))
        .filter(|(closing, closes)| touched.contains(closing) || touched.contains(closes))
        .flat_map(|(closing, closes)| [closing, closes])
        .collect();
    touched.extend(pairs);
    let hidden = log.hidden();
    touched
        .into_iter()
        .filter(|id| *id <= since)
        .filter_map(|id| log.get(id))
        .filter(|e| !never_copied(e, &hidden))
        .map(|e| (e.id, body_of(e)))
        .collect()
}

/// O início da faixa de [`RANGE_WIDTH`] números que leva o item `id`.
fn range_start(id: u64) -> u64 {
    (id / RANGE_WIDTH) * RANGE_WIDTH
}

/// O início de cada faixa tocada por um item novo depois de `since` ou por um
/// expurgo gravado depois dele, em ordem de número.
fn dirty_ranges(log: &SpecLog, since: u64) -> BTreeSet<u64> {
    items_after(log, since)
        .into_iter()
        .map(|(id, _)| range_start(id))
        .chain(purged_since(log, since).into_iter().map(|(id, _)| range_start(id)))
        .collect()
}

/// Os itens da faixa que começa em `start`, em ordem de número, lidos do
/// arquivo inteiro — não só o que é novo: a faixa trocada vai inteira.
fn range_items(log: &SpecLog, start: u64) -> Vec<(u64, Value)> {
    let hidden = log.hidden();
    let end = start + RANGE_WIDTH;
    log.events
        .iter()
        .filter(|e| e.id >= start && e.id < end && !never_copied(e, &hidden))
        .filter_map(|e| body_of(e).map(|body| (e.id, body)))
        .collect()
}

/// Os itens de uma faixa em pedaços de até [`RANGE_MAX_BYTES`], em ordem,
/// dentro da faixa; vazio quando ela ficou sem item.
fn range_chunks(items: &[(u64, Value)]) -> Vec<Vec<Value>> {
    let mut chunks: Vec<Vec<Value>> = Vec::new();
    let mut current: Vec<Value> = Vec::new();
    let mut current_len = 0usize;
    for (_, body) in items {
        let len = body.to_string().len();
        if !current.is_empty() && current_len + len > RANGE_MAX_BYTES {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current_len += len;
        current.push(body.clone());
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// As escritas da faixa que começa em `start`: um documento por pedaço, o
/// primeiro com o nome do início e os seguintes com o início e o número do
/// pedaço, ou o apagamento do documento do início quando a faixa ficou sem
/// item. O primeiro pedaço leva também `chunks`, quantos pedaços a faixa tem
/// agora: se ela encolher depois, um pedaço velho pode sobrar no banco sem
/// ninguém apagar, e é por essa contagem que a leitura da página sabe até
/// onde ler, sem repetir os itens do pedaço que sobrou.
fn range_writes(place: &Place, log: &SpecLog, start: u64) -> Result<Vec<Value>, Refusal> {
    let items = range_items(log, start);
    let chunks = range_chunks(&items);
    if chunks.is_empty() {
        return Ok(vec![json!({ "op": "delete", "collection": RANGES, "doc_id": start.to_string() })]);
    }
    let mut writes = Vec::new();
    for (n, chunk) in chunks.iter().enumerate() {
        let doc_id = if n == 0 { start.to_string() } else { format!("{start}-{}", n + 1) };
        let mut body = json!({ "seq": start * 1000 + n as u64, "items": chunk });
        if n == 0 {
            body["chunks"] = json!(chunks.len());
        }
        writes.push(set(place, RANGES, &doc_id, &body)?);
    }
    Ok(writes)
}

/// O código de cada item que guarda um trecho com cara de segredo e por isso
/// fica fora da cópia, uma vez só, na ordem do arquivo.
pub(crate) fn withheld(log: &SpecLog) -> Vec<String> {
    let codes = log.codes();
    let hidden = log.hidden();
    let mut out: Vec<String> = Vec::new();
    for event in log.events.iter().filter(|e| !never_copied(e, &hidden)) {
        let mut fields = event.fields.clone();
        fields.remove("search");
        if holds_secret(&Value::Object(fields)) {
            let code = codes.get(&event.id).cloned().unwrap_or_else(|| event.id.to_string());
            if !out.contains(&code) {
                out.push(code);
            }
        }
    }
    out
}

/// Algum texto de `value`, em qualquer profundidade, tem um trecho com cara
/// de segredo.
fn holds_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => !super::secret::secret_excerpts(text).is_empty(),
        Value::Array(items) => items.iter().any(holds_secret),
        Value::Object(map) => map.values().any(holds_secret),
        _ => false,
    }
}

/// `value` com cada trecho com cara de segredo trocado por "…", em qualquer
/// profundidade: o que o binário calcula não é item da spec e não tem como
/// ser expurgado, então sai limpo.
fn redacted(value: Value) -> Value {
    match value {
        Value::String(mut text) => {
            for excerpt in super::secret::secret_excerpts(&text) {
                text = text.replace(&excerpt, PURGED_MARK);
            }
            Value::String(text)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(redacted).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, redacted(v))).collect()),
        other => other,
    }
}

/// O documento das coisas calculadas: o nome da spec, o número do último item
/// copiado, o estado de cada onda, o pedido de cada onda que ainda não saiu e
/// a economia do rtk. O número do último item muda a cada cópia, mesmo numa
/// cópia que só apaga um item do meio sem tocar onda, pedido ou rtk: é por
/// ele que a escuta da página aberta sabe que há algo novo para ler.
fn computed(place: &Place, log: &SpecLog, rtk: &[RtkDay], lang: Locale) -> Value {
    let running = crate::commands::flow::round::waves_in_progress(log).into_keys().collect();
    let flight = mustard_core::io::wave_prompt::Flight { running, ..Default::default() };
    let sent: BTreeSet<u64> =
        log.visible().into_iter().filter(|e| e.event_type == "send").filter_map(SpecEvent::wave).collect();
    let prompts: Map<String, Value> = mustard_core::io::wave_prompt::prompts(place.root, place.spec, log, lang, &flight)
        .into_iter()
        .filter(|built| !sent.contains(&built.wave))
        .map(|built| (built.wave.to_string(), Value::String(built.text)))
        .collect();
    let waves: Map<String, Value> = crate::commands::flow::round::wave_states(log)
        .into_iter()
        .map(|(n, state)| (n.to_string(), json!(state_name(state))))
        .collect();
    let rtk: Vec<Value> = rtk
        .iter()
        .map(|day| json!({ "date": day.date, "commands": day.commands, "input": day.input, "saved": day.saved }))
        .collect();
    let spend = spend_line(log, lang);
    // Os mesmos números da linha do gasto, cada um no seu campo: o quadro
    // Gasto da página os mostra em partes, sem ler a frase de volta.
    let tokens = spend_of(log).map(|s| {
        json!({ "waves": s.waves, "caller": s.caller, "total": s.total, "turns": s.turns_per_task })
    });
    redacted(json!({
        "spec": place.spec, "last": log.max_id(), "waves": waves, "prompts": prompts, "rtk": rtk, "spend": spend,
        "tokens": tokens,
    }))
}

/// Uma linha só com o gasto da obra inteira: os tokens de toda onda somados
/// ao que quem despachou já gastou, e os turnos por tarefa de quem entregou
/// onda (o total de turnos de cada envio de onda, dividido pelo total de
/// tarefas das ondas que enviaram). `None` sem nenhum token registrado
/// ainda.
fn spend_line(log: &SpecLog, lang: Locale) -> Option<String> {
    let spend = spend_of(log)?;
    Some(
        translate("round.spend.line", lang)
            .replace("{waves}", &spend.waves.to_string())
            .replace("{caller}", &spend.caller.to_string())
            .replace("{total}", &spend.total.to_string())
            .replace("{turns}", &spend.turns_per_task.to_string()),
    )
}

/// O gasto da obra inteira, em números: os tokens das ondas, os de quem
/// despachou, o total e os turnos por tarefa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Spend {
    waves: u64,
    caller: u64,
    total: u64,
    turns_per_task: u64,
}

/// A conta de [`spend_line`], antes de virar frase. `None` sem nenhum token
/// registrado ainda.
fn spend_of(log: &SpecLog) -> Option<Spend> {
    let visible = log.visible();
    let wave_sends: Vec<&SpecEvent> = visible
        .iter()
        .copied()
        .filter(|e| e.event_type == "send" && e.str_field("role") == Some("wave"))
        .collect();
    let wave_tokens: u64 = wave_sends.iter().filter_map(|e| e.int("tokens")).sum();
    let wave_turns: u64 = wave_sends.iter().filter_map(|e| e.int("steps")).sum();
    let waves_sent: BTreeSet<u64> = wave_sends.iter().filter_map(|e| e.wave()).collect();
    let task_count: u64 = waves_sent
        .into_iter()
        .map(|wave| log.block(BlockQuery::Wave(wave)).into_iter().filter(|e| e.event_type == "task").count() as u64)
        .sum();
    let caller_tokens: u64 =
        visible.iter().filter(|e| e.event_type == "send").filter_map(|e| e.int("caller_tokens")).max().unwrap_or(0);
    let total_tokens = wave_tokens + caller_tokens;
    if total_tokens == 0 {
        return None;
    }
    let turns_per_task = wave_turns.checked_div(task_count).unwrap_or(0);
    Some(Spend { waves: wave_tokens, caller: caller_tokens, total: total_tokens, turns_per_task })
}

/// O nome do estado de uma onda no documento das coisas calculadas.
fn state_name(state: WaveState) -> &'static str {
    match state {
        WaveState::Todo => "todo",
        WaveState::Running => "running",
        WaveState::Approved => "approved",
        WaveState::Rejected => "rejected",
    }
}

/// As linhas do índice que vão para o banco da página do projeto: todas,
/// quando ela ainda não tem endereço ou foi publicada de novo nesta spec
/// depois da última cópia; senão, a linha desta spec, quando a fase dela
/// mudou desde a última cópia; senão, nenhuma. A página antiga, publicada
/// inteira por uma versão antiga, conta como a que ainda não tem endereço: o
/// endereço dela nunca recebe lote.
fn project_rows(
    place: &Place,
    log: &SpecLog,
    others: &[ProjectLog],
    lang: Locale,
) -> Result<Option<Target>, Refusal> {
    let page = mustard_core::io::fs::lock::read_shared(&place.index).ok().and_then(|content| project_page(&content));
    let old = page.as_ref().is_some_and(|page| !page.template);
    let url = page.filter(|page| page.template).map(|page| page.url);
    let rows = mustard_core::io::spec_index::read_rows(place.root);
    let Some(own) = rows.iter().find(|row| row.name == place.spec) else {
        return Ok(None);
    };
    let visible = log.visible();
    let published = visible.iter().filter(|e| published_to(e, PROJECT_PAGE).is_some()).map(|e| e.id).next_back();
    let copied = visible.iter().rfind(|e| copy_of(e) == Some(PROJECT_PAGE));
    let copied_phase = copied.and_then(|e| e.str_field("phase")).map(str::to_string);
    // As publicações do template em todas as specs, as desta por último: no
    // mesmo segundo, a desta vale.
    let own_log = project_log(log);
    let own_publications = &own_log.publications;
    let all: Vec<&ProjectPublication> =
        others.iter().flat_map(|l| l.publications.iter()).chain(own_publications.iter()).collect();
    // A república no mesmo endereço não zera o banco dele: não conta como
    // página nova, que pede todas as linhas.
    let same_address = own_publications.last().is_some_and(|last| {
        all.iter().any(|p| p.url == last.url && !std::ptr::eq(*p, last) && !later(&p.at, &last.at))
    });
    let fresh = !same_address
        && match (published, copied) {
            (Some(published), Some(copied)) => published > copied.id,
            (Some(_), None) => true,
            _ => false,
        };
    let latest = all.iter().copied().fold(None::<&ProjectPublication>, |best, p| match best {
        Some(best) if later(&best.at, &p.at) => Some(best),
        _ => Some(p),
    });
    let published_stamp = latest.filter(|p| url.as_deref() == Some(p.url.as_str())).and_then(|p| p.stamp.as_deref());
    let template = project_page_template(lang);
    let stamp = template_stamp(&template).unwrap_or_default().to_string();
    let republish = url.is_some() && published_stamp != Some(stamp.as_str());
    let chosen: Vec<&ProjectRow> = if url.is_none() || fresh {
        rows.iter().collect()
    } else if copied.is_none() || own.phase != copied_phase || republish {
        vec![own]
    } else {
        return Ok(None);
    };
    if url.is_none() || republish {
        ensure_template(place.root, PROJECT_TEMPLATE, &template)?;
    }
    // A linha desta spec já está no banco do endereço de agora quando uma
    // cópia anterior para esse endereço a levou: uma cópia desta spec, que
    // leva sempre a própria linha, ou a cópia de todas as linhas feita por
    // outra spec depois que a pasta desta nasceu. O banco recusa a troca sem
    // a versão, e a ordem a nomeia.
    let logs: Vec<&ProjectLog> = others.iter().chain(std::iter::once(&own_log)).collect();
    let ours = logs.len() - 1;
    let own_there = url.as_deref().is_some_and(|url| {
        logs.iter().enumerate().any(|(spec, project)| {
            project.copies.iter().any(|copy| {
                went_to(copy, spec, &logs, url)
                    && (spec == ours
                        || (carries_all(copy, spec, &logs)
                            && own_log.born.as_deref().is_some_and(|born| !later(born, &copy.at))))
            })
        })
    });
    let existing = if own_there { vec![format!("{SPECS}/{}", own.name)] } else { Vec::new() };
    let mut writes = Vec::new();
    for row in chosen {
        writes.push(set(place, SPECS, &row.name, &row_body(row))?);
    }
    let mut record = json!({ "page": PROJECT_PAGE });
    if let Some(phase) = &own.phase {
        record["phase"] = json!(phase);
    }
    Ok(Some(Target {
        url,
        batches: batches(place, "project", &writes)?,
        record,
        old,
        republish,
        stamp,
        existing,
    }))
}

/// A publicação `p`, gravada na spec de posição `p_spec` em `logs`, veio
/// antes do evento `id`, da hora `at`, gravado na spec de posição `spec`: na
/// mesma spec, pelo número no arquivo; em outra, pela hora, e no mesmo
/// segundo conta como antes.
fn precedes(p: &ProjectPublication, p_spec: usize, spec: usize, id: u64, at: &str) -> bool {
    if p_spec == spec { p.id < id } else { !later(&p.at, at) }
}

/// A cópia `copy`, gravada na spec de posição `spec` em `logs`, foi para o
/// endereço `url`: o da última publicação do template antes dela, gravada em
/// qualquer spec, e no mesmo segundo a da própria spec vale. Sem publicação
/// antes dela, a página nasceu fora das specs, com o endereço gravado direto
/// no índice: a cópia foi para `url` quando toda publicação depois dela foi
/// para `url` também.
fn went_to(copy: &ProjectCopy, spec: usize, logs: &[&ProjectLog], url: &str) -> bool {
    let all: Vec<(usize, &ProjectPublication)> =
        logs.iter().enumerate().flat_map(|(i, l)| l.publications.iter().map(move |p| (i, p))).collect();
    // As da própria spec por último: no mesmo segundo, elas valem.
    let ordered = all.iter().filter(|(i, _)| *i != spec).chain(all.iter().filter(|(i, _)| *i == spec));
    let latest = ordered.filter(|(i, p)| precedes(p, *i, spec, copy.id, &copy.at)).fold(
        None::<&ProjectPublication>,
        |best, (_, p)| match best {
            Some(best) if later(&best.at, &p.at) => Some(best),
            _ => Some(p),
        },
    );
    match latest {
        Some(p) => p.url == url,
        None => all.iter().all(|(_, p)| p.url == url),
    }
}

/// A cópia `copy`, gravada na spec de posição `spec` em `logs`, levou todas
/// as linhas do índice. O registro da cópia não diz quais linhas levou, então
/// a conta segue a regra que a escolhe: é a primeira cópia daquela spec
/// depois de uma publicação dela num endereço que nenhuma spec tinha
/// publicado antes, com o banco vazio.
fn carries_all(copy: &ProjectCopy, spec: usize, logs: &[&ProjectLog]) -> bool {
    let project = logs[spec];
    let Some(publication) = project.publications.iter().rfind(|p| p.id < copy.id) else {
        return false;
    };
    if project.copies.iter().any(|c| c.id > publication.id && c.id < copy.id) {
        return false;
    }
    !logs.iter().enumerate().any(|(i, l)| {
        l.publications
            .iter()
            .any(|p| p.url == publication.url && precedes(p, i, spec, publication.id, &publication.at))
    })
}

/// A linha de uma spec como vai para o banco da página do projeto.
fn row_body(row: &ProjectRow) -> Value {
    redacted(json!({
        "name": row.name, "goal": row.goal, "phase": row.phase, "branch": row.branch,
        "created": row.created, "updated": row.updated, "url": row.url,
    }))
}

/// Grava o documento `doc_id` da coleção `collection` num arquivo próprio
/// e devolve a escrita que o manda para o banco pelo arquivo.
fn set(place: &Place, collection: &str, doc_id: &str, body: &Value) -> Result<Value, Refusal> {
    let path = place.folder.join(collection).join(format!("{doc_id}.json"));
    write(&path, &body.to_string())?;
    Ok(json!({ "op": "set", "collection": collection, "doc_id": doc_id, "file_path": relative(place.root, &path) }))
}

/// Grava as escritas em lotes de até [`BATCH_MAX`], `<nome>-1.json`,
/// `<nome>-2.json`…, uma escrita por linha, e devolve os caminhos.
fn batches(place: &Place, name: &str, writes: &[Value]) -> Result<Vec<String>, Refusal> {
    let mut out = Vec::new();
    for (n, chunk) in writes.chunks(BATCH_MAX).enumerate() {
        let path = place.folder.join(format!("{name}-{}.json", n + 1));
        let lines: Vec<String> = chunk.iter().map(Value::to_string).collect();
        write(&path, &format!("[\n{}\n]\n", lines.join(",\n")))?;
        out.push(relative(place.root, &path));
    }
    Ok(out)
}

/// Apaga a cópia anterior: os lotes dela não valem mais.
fn clear(folder: &Path) -> Result<(), Refusal> {
    if !folder.exists() {
        return Ok(());
    }
    mustard_core::io::fs::remove_dir_all(folder).map_err(|e| Refusal::Io { detail: e.to_string() })
}

/// Deixa no projeto o template que a ordem manda publicar, `built`, o que o
/// binário rodando monta: escreve quando ele falta, e também quando o carimbo
/// gravado no começo dele — a versão e a impressão do conteúdo, que
/// [`spec_page_template`] e [`project_page_template`] deixam
/// ([`template_stamp`]) — não é o de `built`. O modelo velho ficaria lendo uma
/// coleção que a cópia de agora não escreve mais, ou sem o que a versão nova
/// mostra, e a página abriria sem dizer por quê.
fn ensure_template(root: &Path, template: &str, built: &str) -> Result<(), Refusal> {
    let path = root.join(template);
    if let Ok(existing) = std::fs::read_to_string(&path)
        && template_stamp(&existing).is_some()
        && template_stamp(&existing) == template_stamp(built)
    {
        return Ok(());
    }
    write(&path, built)
}

fn write(path: &Path, text: &str) -> Result<(), Refusal> {
    mustard_core::io::fs::write_atomic(path, text.as_bytes()).map_err(|e| Refusal::Io { detail: e.to_string() })
}

impl Prepared {
    /// A cópia como a resposta de um passo a mostra.
    pub(crate) fn to_value(&self) -> Value {
        let target = |t: &Target| {
            let mut out = json!({ "published": t.url.is_some(), "batches": t.batches, "record": t.record });
            if t.republish {
                out["republish"] = json!(true);
            }
            out
        };
        let mut out = json!({ "folder": self.folder, "spec": target(&self.spec) });
        if let Some(project) = &self.project {
            out["project"] = target(project);
        }
        out
    }

    /// As páginas que o marco publica, na ordem em que são publicadas: a que
    /// ainda não tem endereço e a que é publicada de novo no mesmo endereço,
    /// com o molde novo.
    pub(crate) fn to_publish(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.spec.url.is_none() || self.spec.republish {
            out.push(SPEC_PAGE);
        }
        if self.project.as_ref().is_some_and(|p| p.url.is_none() || p.republish) {
            out.push(PROJECT_PAGE);
        }
        out
    }

    /// A ordem da cópia, uma frase por passo: publicar a página que ainda não
    /// tem endereço, no marco `milestone`, num link novo quando ela ainda é a
    /// página inteira de uma versão antiga, e publicar de novo no mesmo
    /// endereço a que foi publicada com um molde de outro carimbo; copiar os lotes de cada
    /// página, nomeando os documentos que já existem no banco para ler a
    /// versão de cada um antes de trocar, e, fora do descarte, gravar cada
    /// cópia feita; no fim, não levar os endereços para a resposta. Quem copia
    /// é o orquestrador, sem agente, também na primeira cópia. Sem marco, como
    /// depois de um pedido, nada é publicado: a página sem endereço fica para
    /// o próximo marco, e a de molde velho é copiada no endereço que já tem.
    /// No descarte a spec já é terminal, sem cópia seguinte para continuar
    /// dela, então a ordem não pede o registro da cópia.
    pub(crate) fn order(&self, spec: &str, milestone: Option<&str>, lang: Locale) -> Vec<String> {
        let mut out = Vec::new();
        let record_next = milestone != Some("discard");
        for (target, key, name, template, capabilities) in self.targets() {
            let page = translate(name, lang);
            if target.url.is_none() {
                let Some(milestone) = milestone else { continue };
                out.push(
                    translate("page.copy.publish", lang)
                        .replace("{page}", page)
                        .replace("{template}", template)
                        .replace("{capabilities}", capabilities)
                        .replace("{spec}", spec)
                        .replace("{key}", key)
                        .replace("{milestone}", milestone)
                        .replace("{stamp}", &target.stamp),
                );
                if target.old {
                    out.push(translate("page.copy.old_page", lang).replace("{page}", page));
                }
            }
            if let (true, Some(milestone), Some(url)) = (target.republish, milestone, target.url.as_deref()) {
                out.push(
                    translate("page.copy.republish", lang)
                        .replace("{page}", page)
                        .replace("{template}", template)
                        .replace("{capabilities}", capabilities)
                        .replace("{spec}", spec)
                        .replace("{key}", key)
                        .replace("{milestone}", milestone)
                        .replace("{stamp}", &target.stamp)
                        .replace("{url}", url),
                );
            }
            let url = target.url.clone().unwrap_or_else(|| translate("page.copy.new_address", lang).to_string());
            let files: Vec<String> = target.batches.iter().map(|b| format!("`{b}`")).collect();
            let mut copy = translate("page.copy.batches", lang)
                .replace("{page}", page)
                .replace("{url}", &url)
                .replace("{files}", &files.join(", "));
            if !target.existing.is_empty() {
                let docs: Vec<String> = target.existing.iter().map(|d| format!("`{d}`")).collect();
                let existing = translate("page.copy.existing", lang).replace("{docs}", &docs.join(", "));
                copy = format!("{copy} {existing}");
            }
            if record_next {
                let record = translate("page.copy.record", lang)
                    .replace("{spec}", spec)
                    .replace("{record}", &target.record.to_string());
                copy = format!("{copy} {record}");
            }
            out.push(copy);
        }
        if !out.is_empty() {
            out.push(translate("page.copy.no_links", lang).to_string());
        }
        out
    }

    /// Cada página com o que vai para o banco dela: a chave dela, o nome no
    /// catálogo, o template e o que ela declara ao ser publicada.
    fn targets(&self) -> Vec<(&Target, &'static str, &'static str, &'static str, &'static str)> {
        let mut out = vec![(&self.spec, SPEC_PAGE, "page.name.spec", SPEC_TEMPLATE, SPEC_CAPABILITIES)];
        if let Some(project) = &self.project {
            out.push((project, PROJECT_PAGE, "page.name.project", PROJECT_TEMPLATE, PROJECT_CAPABILITIES));
        }
        out
    }
}

/// Para os testes: as escritas dos lotes da página `page` (`spec` ou
/// `project`) que a resposta `report` de um passo mandou copiar, na ordem,
/// como a ferramenta do banco as recebe, cada uma com o corpo do documento
/// lido do `file_path` dela em `body`.
#[cfg(test)]
pub(crate) fn sent(root: &Path, report: &Value, page: &str) -> Vec<Value> {
    let batches = report["copy"][page]["batches"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for batch in batches {
        let path = root.join(batch.as_str().unwrap_or_default());
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let writes: Vec<Value> = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(writes.len() <= BATCH_MAX, "{} has {} writes", path.display(), writes.len());
        for mut write in writes {
            if let Some(file) = write["file_path"].as_str() {
                let body = std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
                write["body"] = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{file}: {e}"));
            }
            out.push(write);
        }
    }
    out
}

/// Para os testes: os números dos itens que os lotes da página da spec, na
/// resposta `report`, mandam gravar (`set`), juntando os itens de cada faixa
/// escrita, na ordem em que aparecem nela.
#[cfg(test)]
pub(crate) fn sent_items(root: &Path, report: &Value) -> Vec<u64> {
    sent(root, report, SPEC_PAGE)
        .iter()
        .filter(|w| w["collection"] == json!(RANGES) && w["op"] == json!("set"))
        .flat_map(|w| w["body"]["items"].as_array().cloned().unwrap_or_default())
        .filter_map(|item| item["id"].as_u64())
        .collect()
}

/// Para os testes: a frase da ordem que copia os lotes da página da spec
/// `spec`, que a resposta `report` de um passo preparou, para o banco no
/// endereço `url`, como o catálogo em `lang` a monta. Sem documento já
/// existente no banco: ver [`batches_order_with`] para nomear os que já
/// existem.
#[cfg(test)]
pub(crate) fn batches_order(report: &Value, spec: &str, url: &str, lang: Locale) -> String {
    batches_order_with(report, spec, url, &[], lang)
}

/// Como [`batches_order`], nomeando em `existing` (`{coleção}/{doc_id}`) os
/// documentos que a ordem diz já existirem no banco, a pedir a versão de
/// cada um antes de trocar.
#[cfg(test)]
pub(crate) fn batches_order_with(report: &Value, spec: &str, url: &str, existing: &[&str], lang: Locale) -> String {
    let batches = report["copy"][SPEC_PAGE]["batches"].as_array().cloned().unwrap_or_default();
    let files: Vec<String> = batches.iter().map(|b| format!("`{}`", b.as_str().unwrap_or_default())).collect();
    let mut copy = translate("page.copy.batches", lang)
        .replace("{page}", translate("page.name.spec", lang))
        .replace("{url}", url)
        .replace("{files}", &files.join(", "));
    if !existing.is_empty() {
        let docs: Vec<String> = existing.iter().map(|d| format!("`{d}`")).collect();
        let existing = translate("page.copy.existing", lang).replace("{docs}", &docs.join(", "));
        copy = format!("{copy} {existing}");
    }
    let record = translate("page.copy.record", lang)
        .replace("{spec}", spec)
        .replace("{record}", &report["copy"][SPEC_PAGE]["record"].to_string());
    format!("{copy} {record}")
}

/// Para os testes: a frase que diz que a página `page` (`spec` ou `project`)
/// publicada inteira por uma versão antiga fica parada, como o catálogo em
/// `lang` a monta.
#[cfg(test)]
pub(crate) fn old_page_order(page: &str, lang: Locale) -> String {
    let name = if page == PROJECT_PAGE { "page.name.project" } else { "page.name.spec" };
    translate("page.copy.old_page", lang).replace("{page}", translate(name, lang))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use mustard_core::domain::model::contract::{HookInput, Outcome, Trigger};
    use mustard_core::domain::spec_state::SpecState as _;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::{round_for, RoundOpts};
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};
    use crate::shared::spec_state::DiskSpecState;

    const SPEC_URL: &str = "https://claude.ai/code/artifact/spec-x";
    const PROJECT_URL: &str = "https://claude.ai/code/artifact/projeto";

    fn git(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Grava pelo `run write`, o comando que a conversa usa; a fala do
    /// usuário, que só um gancho grava, vai pela mesma gravação dos ganchos.
    fn write(root: &Path, event_type: &str, mut draft: Value) -> Value {
        if event_type == "task" {
            let map = draft.as_object_mut().expect("a tarefa é um objeto");
            map.entry("files").or_insert_with(|| json!([]));
            map.entry("depends_on").or_insert_with(|| json!([]));
        }
        let out = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            event_type: event_type.into(),
            json: draft.to_string(),
        });
        assert_eq!(out["ok"], json!(true), "{event_type}: {out}");
        out
    }

    /// O carimbo do molde da página `page` que o programa rodando monta, como
    /// a publicação o grava.
    fn stamp_of(page: &str) -> String {
        let template =
            if page == PROJECT_PAGE { project_page_template(Locale::PtBr) } else { spec_page_template(Locale::PtBr) };
        template_stamp(&template).expect("the stamp").to_string()
    }

    fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {report}"))
    }

    /// Um evento do Claude Code, pelo mesmo despachante que o `mustard-rt on`
    /// usa, na sessão `s1` do projeto em `root`.
    fn hook_event(root: &Path, event: &str, tool: Option<&str>, tool_input: Value, raw: Value) -> Outcome {
        let input = HookInput {
            hook_event_name: Some(event.to_string()),
            tool_name: tool.map(str::to_string),
            tool_input,
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            raw,
            ..HookInput::default()
        };
        crate::dispatch::run_event(Trigger::from_event_name(event), &input)
    }

    /// Um passo do fluxo pelo despacho do `mustard-rt run`: grava a chamada.
    fn flow_step(root: &Path) {
        crate::commands::flow::cli::dispatch(crate::commands::flow::cli::FlowCmd::Resume {
            spec: Some("x".to_string()),
            root: root.to_path_buf(),
        });
    }

    /// A rodada, o marco que mais se repete, pela porta do comando.
    fn round(root: &Path) -> Value {
        let out = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, None);
        assert_eq!(out["ok"], json!(true), "{out}");
        out
    }

    fn log(root: &Path) -> SpecLog {
        DiskSpecState::new(root).log("x").expect("the spec")
    }

    /// O `next` de `report` por extenso: a rodada leva só a linha curta que
    /// manda ler o arquivo, e a ordem por extenso mora nele — na mesma pasta
    /// dos lotes, que o próximo marco reconstrói do zero. Leia logo depois
    /// da rodada que gerou `report`, antes de outra rodada rodar.
    fn full_next(root: &Path, report: &Value) -> String {
        let next = report["next"].as_str().unwrap_or_default().to_string();
        let file = root.join(".claude").join("spec").join("x").join("copy").join("next.md");
        std::fs::read_to_string(&file).map_or(next.clone(), |order| format!("{order} {next}"))
    }

    /// Um projeto no git, com os arquivos `files`, na branch da spec `x`
    /// aberta, com a fala do usuário e um critério gravados: devolve os
    /// números dos dois.
    fn project_with(files: &[&str]) -> (tempfile::TempDir, u64, u64) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        for file in files {
            std::fs::write(root.join(file), "fn um() {}\n").unwrap();
        }
        git(root, &["init", "-q"]);
        git(root, &["config", "core.autocrlf", "false"]);
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "semente"]);
        for (key, value) in [("user.email", "t@t"), ("user.name", "t"), ("commit.gpgsign", "false")] {
            git(root, &["config", key, value]);
        }
        git(root, &["checkout", "-q", "-b", "feature/x"]);
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = id_of(&write(root, "message", json!({"author": "user", "text": "o objetivo"})));
        let crit = id_of(&write(root, "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test", "form": "ubiquitous",
                "origin": said})));
        (dir, said, crit)
    }

    /// Um projeto no git, na branch da spec `x`, com a spec aprovada: uma
    /// onda com uma tarefa num arquivo que o git conhece.
    fn approved_project() -> tempfile::TempDir {
        let (dir, said, crit) = project_with(&["src/a.rs"]);
        let root = dir.path();
        write(root, "wave", json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "A suíte passa.",
            "origin": said}));
        write(root, "task", json!({"wave": 1, "text": "Tarefa da onda 1.", "files": [{"path": "src/a.rs"}],
            "origin": said}));
        crate::shared::spec_state::approve_in(&root.join(".claude/spec/x"));
        dir
    }

    /// O que a conversa faz com a ordem de um marco: publica cada página que
    /// a ordem manda publicar e grava o endereço, com o carimbo do molde,
    /// copia os lotes e grava cada cópia feita com o `--json` que a ordem
    /// traz.
    fn follow(root: &Path, report: &Value) {
        let milestone = "round";
        for page in report["publish"].as_array().cloned().unwrap_or_default() {
            let page = page.as_str().unwrap_or_default();
            let url = if page == SPEC_PAGE { SPEC_URL } else { PROJECT_URL };
            write(root, "publish", json!({"page": page, "milestone": milestone, "ok": true, "template": true,
                "stamp": stamp_of(page), "url": url}));
        }
        for page in ["spec", "project"] {
            if !report["copy"][page].is_null() {
                write(root, "copy", report["copy"][page]["record"].clone());
            }
        }
    }

    /// Um marco chega depois de itens novos no `spec.ndjson`, gravados pelos
    /// ganchos e pelos comandos de verdade: a fala do usuário e o texto que
    /// os ganchos colocam, um comando que a trava barra, a resposta, uma
    /// decisão, uma anotação com uma senha e a chamada de um passo do fluxo.
    /// Os itens desta spec pequena cabem todos na mesma faixa (0-99), então a
    /// cópia do segundo marco reenvia a faixa inteira, com os itens de antes
    /// e os novos juntos — é a faixa que muda, não cada item. Ficam fora dela
    /// os registros internos e a anotação com a senha, que o marco manda
    /// expurgar. A cópia grava o número até onde foi, e a cópia seguinte
    /// começa dele. Nenhum marco escreve `spec.md`, `spec.html` nem
    /// `project.html`.
    #[test]
    fn the_copy_carries_only_the_items_after_the_last_copied_number() {
        let dir = approved_project();
        let root = dir.path();

        // O primeiro marco: as páginas ainda não têm endereço, e a cópia leva
        // a spec inteira.
        let first = round(root);
        assert_eq!(first["publish"], json!(["spec", "project"]), "{first}");
        let last = first["copy"]["spec"]["record"]["last"].as_u64().expect("the number copied");
        assert_eq!(last, log(root).max_id(), "the copy goes up to the last item");
        assert_eq!(sent_items(root, &first).first(), Some(&1), "the whole spec: {first}");
        follow(root, &first);

        // Itens novos, pelos ganchos e pelos comandos.
        let prompt = json!({"prompt": "Anote a senha do banco e siga."});
        assert!(!hook_event(root, "UserPromptSubmit", None, Value::Null, prompt).is_blocking());
        let barred = json!({"command": "rm -rf /"});
        assert!(hook_event(root, "PreToolUse", Some("Bash"), barred, Value::Null).is_blocking());
        let reply = json!({"last_assistant_message": "Anotei a decisão e deixei a senha fora."});
        assert!(!hook_event(root, "Stop", None, Value::Null, reply).is_blocking());
        let said = log(root).visible().into_iter().filter(|e| e.event_type == "message").map(|e| e.id).next_back();
        let decision = id_of(&write(root, "decision", json!({"text": "A cópia leva só o que é novo.", "why": "w",
            "keys": ["cópia"], "applies_to": {"files": ["**"]}, "origin": said})));
        let secret = write(root, "note", json!({"text": "A senha do banco: S3nh4F0rte2024", "keys": ["banco"],
            "origin": said}));
        flow_step(root);

        let second = round(root);
        let after = log(root);
        let new: Vec<&SpecEvent> = after.events.iter().filter(|e| e.id > last).collect();
        for internal in LEFT_OUT {
            assert!(new.iter().any(|e| e.event_type == *internal), "{internal} was recorded after the copy");
        }
        let items = sent_items(root, &second);
        // A faixa trocada vai inteira: o item já copiado no primeiro marco
        // volta junto com os novos.
        assert!(items.contains(&last), "the touched range resends the earlier items too: {items:?}");
        assert!(items.contains(&decision), "{items:?}");
        // Os registros internos e o item com segredo nunca entram na faixa.
        for internal in new.iter().filter(|e| LEFT_OUT.contains(&e.event_type.as_str())) {
            assert!(!items.contains(&internal.id), "{} was copied: {items:?}", internal.id);
        }
        assert!(!items.contains(&id_of(&secret)), "the secret item stays out of the range: {items:?}");
        let bodies = sent(root, &second, "spec");
        assert!(bodies.iter().all(|w| !w.to_string().contains("S3nh4F0rte2024")), "the secret never goes");
        let computed = bodies.iter().find(|w| w["collection"] == json!("computed")).expect("the computed item");
        assert_eq!(computed["body"]["waves"], json!({"1": "running"}), "{computed}");
        assert_eq!(second["withheld"], json!([secret["code"]]), "{second}");
        let next = full_next(root, &second);
        assert!(next.contains("write purge") && next.contains(SPEC_URL), "{next}");
        assert!(second.get("publish").is_none(), "both pages have their address: {second}");
        assert!(second["copy"].get("project").is_none(), "the phase did not change: {second}");

        // A cópia feita grava o número, e a seguinte começa dele: o registro
        // da cópia vira item na mesma faixa, e é o mais novo dela.
        let recorded = id_of(&write(root, "copy", second["copy"]["spec"]["record"].clone()));
        flow_step(root);
        let third = round(root);
        assert_eq!(sent_items(root, &third).last(), Some(&recorded), "the newest item in the touched range");

        for page in [".claude/spec/x/spec.md", ".claude/spec/x/spec.html", ".claude/spec/project.html"] {
            assert!(!root.join(page).exists(), "{page} is no longer written");
        }
    }

    /// O trecho com cara de segredo pode morar em qualquer campo do item, não
    /// só em `text`: aqui ele mora no `why` de uma decisão, com o `text`
    /// limpo. O item fica fora da cópia até o expurgo, do mesmo jeito que um
    /// segredo no texto.
    #[test]
    fn a_secret_outside_the_text_field_is_withheld_too() {
        let dir = approved_project();
        let root = dir.path();
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        let decision = write(root, "decision", json!({"text": "A regra não guarda nada.",
            "why": "A senha do banco: S3nh4F0rte2024", "keys": ["banco"], "applies_to": {"files": ["**"]},
            "origin": said}));

        let first = round(root);
        let bodies = sent(root, &first, "spec");
        assert!(
            bodies.iter().all(|w| !w.to_string().contains("S3nh4F0rte2024")),
            "the secret in `why` never goes: {bodies:?}",
        );
        assert_eq!(first["withheld"], json!([decision["code"]]), "{first}");
    }

    /// A página republicada — um `publish` novo do template, depois de uma
    /// cópia já gravada — reinicia a contagem: a cópia seguinte volta a levar
    /// a spec inteira, como se a página tivesse acabado de nascer, mesmo com
    /// uma cópia anterior gravada para o link antigo.
    #[test]
    fn a_republished_page_restarts_the_copy_from_zero() {
        const REPUBLISHED_URL: &str = "https://claude.ai/code/artifact/spec-x-2";
        let dir = approved_project();
        let root = dir.path();
        let first = round(root);
        follow(root, &first);
        assert_eq!(sent_items(root, &first).first(), Some(&1), "the first copy: {first}");

        write(root, "publish",
            json!({"page": "spec", "milestone": "round", "ok": true, "template": true, "stamp": stamp_of("spec"), "url": REPUBLISHED_URL}));

        let second = round(root);
        assert_eq!(sent_items(root, &second).first(), Some(&1), "the whole spec goes again: {second}");
        let next = full_next(root, &second);
        assert!(!next.contains("if_version"), "the new address has an empty database: {next}");
    }

    /// A república no mesmo endereço não zera a contagem: o banco continua
    /// no mesmo endereço, e a cópia seguinte leva só o que falta, nomeando
    /// entre os que já existem a faixa tocada e o documento calculado, senão
    /// o banco recusa a troca sem versão.
    #[test]
    fn the_copy_after_a_same_address_republish_sends_only_what_is_missing() {
        let dir = approved_project();
        let root = dir.path();
        let lang = Locale::PtBr;
        let first = round(root);
        follow(root, &first);
        let copied = first["copy"]["spec"]["record"]["last"].as_u64().expect("the number copied");

        write(root, "publish", json!({"page": "spec", "milestone": "round", "ok": true, "template": true,
            "stamp": stamp_of("spec"), "url": SPEC_URL}));

        let second = round(root);
        let items = sent_items(root, &second);
        assert!(items.iter().any(|id| *id > copied), "what came after the copy goes: {items:?}");
        let touched = format!("{RANGES}/{}", range_start(copied + 1));
        let next2 = full_next(root, &second);
        let expected2 = batches_order_with(&second, "x", SPEC_URL, &[touched.as_str(), COMPUTED], lang);
        assert!(next2.contains(&expected2), "the documents already there are pinned: {next2}");
    }

    /// Um expurgo gravado depois da última cópia troca de novo a faixa que
    /// ele tocou: a versão limpa do item substitui a do banco, dentro da
    /// faixa reenviada. O item que ainda guarda um trecho com cara de segredo
    /// depois do expurgo some da lista de itens da faixa.
    #[test]
    fn a_purge_after_the_copy_sends_the_clean_item_again_or_takes_it_out() {
        let dir = approved_project();
        let root = dir.path();
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        let clean = id_of(&write(root, "note",
            json!({"text": "O código do cofre é azul-marinho-42.", "keys": ["cofre"], "origin": said})));
        let held = id_of(&write(root, "note",
            json!({"text": "Cofre azul-marinho-42, e a senha: S3nh4F0rte2024.", "keys": ["cofre"], "origin": said})));

        let first = round(root);
        let copied = sent_items(root, &first);
        assert!(copied.contains(&clean) && !copied.contains(&held), "{copied:?}");
        follow(root, &first);
        let purge =
            json!({"targets": [clean, held], "reason": "client_data", "excerpt": "azul-marinho-42", "origin": said});
        write(root, "purge", purge);

        let second = round(root);
        let writes = sent(root, &second, "spec");
        let range = writes.iter().find(|w| w["collection"] == json!(RANGES)).expect("the touched range");
        assert_eq!(range["op"], json!("set"), "{range}");
        let items = range["body"]["items"].as_array().cloned().unwrap_or_default();
        let again = items.iter().find(|i| i["id"].as_u64() == Some(clean)).expect("the purged item goes again");
        assert_eq!(again["text"], json!("O código do cofre é …."), "{again}");
        assert!(!items.iter().any(|i| i["id"].as_u64() == Some(held)), "the item with a secret left leaves the range");
        assert!(!writes.iter().any(|w| w.to_string().contains("azul-marinho")), "{writes:?}");
    }

    /// Uma cópia que só tira um item do meio (um expurgo, sem onda, pedido
    /// nem rtk mudando) muda mesmo assim o `last` do documento calculado: é
    /// por ele que a página aberta nota que há algo novo, mesmo sem item novo
    /// nem mudança de onda para a escuta pegar.
    #[test]
    fn a_middle_only_purge_still_moves_the_last_copied_item() {
        let dir = approved_project();
        let root = dir.path();
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        let clean = id_of(&write(root, "note",
            json!({"text": "O código do cofre é azul-marinho-42.", "keys": ["cofre"], "origin": said})));
        let held = id_of(&write(root, "note",
            json!({"text": "Cofre azul-marinho-42, e a senha: S3nh4F0rte2024.", "keys": ["cofre"], "origin": said})));

        let first = round(root);
        follow(root, &first);
        let first_last = first["copy"]["spec"]["record"]["last"].as_u64().expect("the first last");

        write(root, "purge", json!({"targets": [clean, held], "reason": "client_data",
            "excerpt": "azul-marinho-42", "origin": said}));

        let second = round(root);
        let writes = sent(root, &second, "spec");
        let computed = writes.iter().find(|w| w["collection"] == json!("computed")).expect("the computed item");
        let second_last = computed["body"]["last"].as_u64().expect("the last copied item");
        assert!(second_last > first_last, "a copy with only a purge still moves `last`: {first_last} -> {second_last}");
        assert_eq!(computed["body"]["waves"], json!({"1": "running"}), "no wave changed: {computed}");
    }

    /// O banco recusa a troca de um documento já existente sem a versão
    /// dele: a partir da segunda cópia, a ordem nomeia os documentos que já
    /// estão lá — a faixa tocada e o documento calculado — para ler a versão
    /// de cada um antes de trocar. Na primeira cópia nada existe ainda, e a
    /// ordem não fala em versão; um expurgo que troca de novo a faixa de um
    /// item já copiado também pede a versão dela.
    #[test]
    fn the_copy_order_pins_the_documents_it_overwrites() {
        let dir = approved_project();
        let root = dir.path();
        let lang = Locale::PtBr;

        // Primeira cópia: nada existe ainda no banco.
        let first = round(root);
        let next1 = full_next(root, &first);
        assert_eq!(sent_items(root, &first).first(), Some(&1), "the whole spec: {first}");
        assert!(!next1.contains("if_version"), "the first copy has nothing to overwrite: {next1}");
        follow(root, &first);

        // Segunda cópia: um item novo troca de novo a mesma faixa, e o
        // documento calculado já existe desde a primeira cópia.
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        let added = id_of(&write(root, "note", json!({"text": "Nota nova.", "keys": ["k"], "origin": said})));
        let second = round(root);
        let next2 = full_next(root, &second);
        let touched = format!("{RANGES}/{}", range_start(added));
        let expected2 = batches_order_with(&second, "x", SPEC_URL, &[touched.as_str(), COMPUTED], lang);
        assert!(next2.contains(&expected2), "the touched range and the computed document are pinned: {next2}");
        follow(root, &second);

        // Um expurgo troca de novo a faixa de um item que já estava no
        // banco.
        let purge = json!({"targets": [added], "reason": "client_data", "excerpt": "nova", "origin": said});
        write(root, "purge", purge);
        let third = round(root);
        let next3 = full_next(root, &third);
        let expected3 = batches_order_with(&third, "x", SPEC_URL, &[touched.as_str(), COMPUTED], lang);
        assert!(next3.contains(&expected3), "the range a purge trades again is pinned too: {next3}");
    }

    /// A linha da spec vai para o banco da página do projeto só quando a fase
    /// dela muda: a primeira rodada leva a spec de aprovada para em execução,
    /// e a linha vai; a rodada seguinte não muda a fase, e a linha não vai.
    #[test]
    fn the_project_row_is_copied_only_when_the_phase_changes() {
        let dir = approved_project();
        let root = dir.path();
        // O marco da aprovação copiou a linha na fase de então.
        write(root, "publish",
            json!({"page": "project", "milestone": "approval", "ok": true, "template": true, "stamp": stamp_of("project"), "url": PROJECT_URL}));
        write(root, "copy", json!({"page": "project", "phase": "approved"}));

        let first = round(root);
        assert_eq!(first["publish"], json!(["spec"]), "the project page has its address: {first}");
        let rows = sent(root, &first, "project");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((&rows[0]["doc_id"], &rows[0]["body"]["phase"]), (&json!("x"), &json!("running")));
        assert_eq!(first["copy"]["project"]["record"], json!({"page": "project", "phase": "running"}));
        let next = full_next(root, &first);
        assert!(next.contains(PROJECT_URL) && next.contains(r#"'{"page":"project","phase":"running"}'"#), "{next}");
        follow(root, &first);

        let second = round(root);
        assert!(second["copy"].get("project").is_none(), "the phase did not change: {second}");
    }

    /// A linha da spec copiada antes para a página do projeto já está no
    /// banco daquele endereço, e o banco recusa trocá-la sem a versão: a
    /// cópia seguinte para o mesmo endereço nomeia o documento dela entre os
    /// que já existem. A primeira cópia, para um endereço novo com o banco
    /// vazio, não nomeia nada.
    #[test]
    fn a_linha_do_projeto_ja_copiada_pede_a_versao_antes_de_trocar() {
        let dir = approved_project();
        let root = dir.path();
        let lang = Locale::PtBr;
        let named = translate("page.copy.existing", lang).replace("{docs}", &format!("`{SPECS}/x`"));

        // O marco da aprovação publica a página do projeto num endereço novo
        // e copia a linha da spec na fase de então.
        let approval = prepare(root, "x", lang).expect("the approval copy");
        let project = approval.project.as_ref().expect("the project row goes");
        assert!(project.url.is_none() && project.existing.is_empty(), "a new address has nothing yet");
        let first = approval.order("x", Some("approval"), lang).join(" ");
        assert!(!first.contains(&named), "the first copy names nothing: {first}");
        write(root, "publish", json!({"page": "project", "milestone": "approval", "ok": true, "template": true,
            "stamp": stamp_of("project"), "url": PROJECT_URL}));
        write(root, "copy", project.record.clone());

        // A rodada muda a fase, e a linha vai de novo para o mesmo endereço.
        let second = round(root);
        assert_eq!(second["copy"]["project"]["record"], json!({"page": "project", "phase": "running"}), "{second}");
        let next = full_next(root, &second);
        assert!(next.contains(PROJECT_URL) && next.contains(&named), "the row already there is named: {next}");
    }

    /// Outra spec publica a página do projeto num endereço novo e copia
    /// todas as linhas do índice, a desta spec junto: a primeira cópia desta
    /// spec para esse endereço já acha a linha dela no banco e a nomeia.
    #[test]
    fn a_linha_levada_pela_copia_de_outra_spec_tambem_pede_a_versao() {
        let dir = approved_project();
        let root = dir.path();
        let lang = Locale::PtBr;
        let named = translate("page.copy.existing", lang).replace("{docs}", &format!("`{SPECS}/x`"));
        let other = root.join(".claude/spec/y/spec.ndjson");
        let put = |event_type: &str, fields: Value| {
            store::write(&other, event_type, fields.as_object().cloned().unwrap(), &[]).unwrap();
        };
        put("state", json!({"phase": "survey"}));
        put("publish", json!({"page": "project", "milestone": "round", "ok": true, "template": true,
            "stamp": stamp_of(PROJECT_PAGE), "url": PROJECT_URL}));
        put("copy", json!({"page": "project", "phase": "survey"}));

        let first = round(root);
        assert!(!first["publish"].as_array().is_some_and(|p| p.contains(&json!("project"))), "{first}");
        assert_eq!(first["copy"]["project"]["record"], json!({"page": "project", "phase": "running"}), "{first}");
        let next = full_next(root, &first);
        assert!(next.contains(PROJECT_URL) && next.contains(&named), "the row the other spec carried is named: {next}");
    }

    /// A cópia de uma spec longa vai em lotes de até 50 escritas, na ordem
    /// das faixas, e o documento das coisas calculadas vai no último; ver
    /// [`a_long_spec_fits_the_page_database`], que já precisa de uma spec
    /// grande o bastante para tocar mais de 50 faixas.
    #[test]
    fn a_long_copy_goes_in_batches_of_fifty() {
        let dir = approved_project();
        let root = dir.path();
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        for n in 0..60 {
            write(root, "note", json!({"text": format!("Nota {n}."), "keys": ["nota"], "origin": said}));
        }
        let first = round(root);
        // 60 notas cabem numa faixa só: um lote, com a faixa e o documento
        // das coisas calculadas.
        let batches = first["copy"]["spec"]["batches"].as_array().cloned().unwrap_or_default();
        assert_eq!(batches, [json!(".claude/spec/x/copy/spec-1.json")], "{batches:?}");
        let writes = sent(root, &first, "spec");
        assert_eq!(writes.len(), 2, "the range and the computed document: {writes:?}");
        assert_eq!(writes.last().map(|w| w["collection"].clone()), Some(json!("computed")));
    }

    const OLD_URL: &str = "https://claude.ai/code/artifact/pagina-antiga";

    /// Um projeto com duas specs cuja página do projeto uma versão antiga do
    /// Mustard publicou inteira, sem banco: a publicação gravada sem a marca
    /// do template deixa na linha do projeto do índice os mesmos bytes que a
    /// versão antiga gravava. O primeiro marco, uma rodada, manda publicar o
    /// template do projeto num link novo, dizendo que a antiga fica parada, e
    /// copiar para ele as linhas das duas specs. Nada vai para a página
    /// antiga: o endereço dela não aparece na resposta nem nos lotes.
    /// Publicado o template, a linha do projeto guarda a marca, e o marco
    /// seguinte não publica de novo.
    #[test]
    fn the_old_project_page_gets_the_template_in_a_new_link() {
        let dir = approved_project();
        let root = dir.path();
        let lang = Locale::PtBr;
        let other = root.join(".claude/spec/y/spec.ndjson");
        store::write(&other, "state", json!({"phase": "survey"}).as_object().cloned().unwrap(), &[]).unwrap();
        // A página da spec já é o template, com a cópia gravada: só a do
        // projeto é antiga.
        write(root, "publish", json!({"page": "spec", "milestone": "approval", "ok": true, "template": true, "stamp": stamp_of("spec"),
            "url": SPEC_URL}));
        write(root, "copy", json!({"page": "spec", "last": log(root).max_id()}));
        // A versão antiga publicou a página do projeto inteira.
        write(root, "publish", json!({"page": "project", "milestone": "approval", "ok": true, "url": OLD_URL}));
        let index = || std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap();
        let old_line = mustard_core::domain::spec_index::project_line(Some(OLD_URL));
        assert!(index().starts_with(&format!("{old_line}\n")), "the bytes the old version wrote: {}", index());

        let first = round(root);
        assert_eq!(first["publish"], json!(["project"]), "the old page is not the template: {first}");
        assert_eq!(first["copy"]["project"]["published"], json!(false), "{first}");
        let rows: Vec<Value> = sent(root, &first, "project").iter().map(|w| w["doc_id"].clone()).collect();
        assert_eq!(rows, [json!("x"), json!("y")], "every row goes to the new link: {first}");
        let next = full_next(root, &first);
        let publish = translate("page.copy.publish", lang)
            .replace("{page}", translate("page.name.project", lang))
            .replace("{template}", PROJECT_TEMPLATE)
            .replace("{capabilities}", PROJECT_CAPABILITIES)
            .replace("{spec}", "x")
            .replace("{key}", PROJECT_PAGE)
            .replace("{milestone}", "round")
            .replace("{stamp}", &stamp_of(PROJECT_PAGE));
        let old = old_page_order(PROJECT_PAGE, lang);
        let files: Vec<String> = first["copy"]["project"]["batches"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|b| format!("`{}`", b.as_str().unwrap_or_default()))
            .collect();
        let copy = translate("page.copy.batches", lang)
            .replace("{page}", translate("page.name.project", lang))
            .replace("{url}", translate("page.copy.new_address", lang))
            .replace("{files}", &files.join(", "))
            .replace("{spec}", "x")
            .replace("{record}", &first["copy"]["project"]["record"].to_string());
        let at = |sentence: &str| next.find(sentence).unwrap_or_else(|| panic!("missing «{sentence}» in {next}"));
        assert!(at(&publish) < at(&old) && at(&old) < at(&copy), "publish, keep the old one still, then copy: {next}");
        assert!(!next.contains(&old_page_order(SPEC_PAGE, lang)), "the spec page is the template: {next}");
        assert!(root.join(PROJECT_TEMPLATE).is_file(), "the template to publish is on disk");
        let batches = sent(root, &first, "project");
        assert!(!first.to_string().contains(OLD_URL), "nothing goes to the old page: {first}");
        assert!(!format!("{batches:?}").contains(OLD_URL), "nothing goes to the old page: {batches:?}");

        // A conversa publica o template no link novo e grava a cópia.
        write(root, "publish", json!({"page": "project", "milestone": "round", "ok": true, "template": true, "stamp": stamp_of("project"),
            "url": PROJECT_URL}));
        write(root, "copy", first["copy"]["project"]["record"].clone());
        let second = round(root);
        assert!(second.get("publish").is_none(), "the template is published once: {second}");
        assert!(second["copy"].get("project").is_none(), "the rows are already there: {second}");
        assert!(!second.to_string().contains(OLD_URL), "{second}");
        let page = mustard_core::domain::spec_index::project_page(&index());
        assert_eq!(page.map(|p| (p.url, p.template)), Some((PROJECT_URL.to_string(), true)), "{}", index());
        assert_eq!(mustard_core::io::spec_index::project_page_url(root).as_deref(), Some(PROJECT_URL));
    }

    /// Uma spec aprovada por uma versão antiga do Mustard, que publicou a
    /// página inteira dela, chega ao primeiro marco desta versão, uma
    /// rodada, ainda sem a página nova: a rodada segue e despacha a onda, e a
    /// ordem manda publicar o template num link novo, deixando a página
    /// antiga parada, e copiar a spec inteira. Quem copia é o orquestrador,
    /// na própria conversa: a frase dos lotes vai solta, sem agente e sem
    /// texto a despachar entre « e ». Publicado o template, sem a cópia
    /// gravada, o marco seguinte não publica de novo e manda a spec inteira
    /// outra vez, no mesmo link e do mesmo jeito; gravada a cópia, a seguinte
    /// leva só o que veio depois.
    #[test]
    fn a_primeira_copia_fica_com_o_orquestrador_sem_agente() {
        let (dir, said, crit) = project_with(&["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs"]);
        let root = dir.path();
        let wave = |n: u64, depends: Option<u64>| {
            let mut draft = json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit],
                "done_when": "A suíte passa.", "origin": said});
            if let Some(on) = depends {
                draft["depends_on"] = json!([on]);
            }
            write(root, "wave", draft);
        };
        let task = |n: u64, k: u64, file: &str| {
            write(root, "task", json!({"wave": n, "text": format!("Tarefa {k} da onda {n}."),
                "files": [{"path": file}], "origin": said}))
        };
        wave(1, None);
        task(1, 1, "src/a.rs");
        wave(2, Some(1));
        task(2, 1, "src/b.rs");
        task(2, 2, "src/b.rs");
        wave(3, Some(1));
        task(3, 1, "src/c.rs");
        task(3, 2, "src/c.rs");
        task(3, 3, "src/c.rs");
        wave(4, Some(1));
        task(4, 1, "src/d.rs");
        task(4, 2, "src/d.rs");
        crate::shared::spec_state::approve_in(&root.join(".claude/spec/x"));
        // A versão antiga publicou a página inteira na aprovação.
        write(root, "publish", json!({"page": "spec", "milestone": "approval", "ok": true, "url": OLD_URL}));
        let lang = Locale::PtBr;

        let first = round(root);
        let dispatched: Vec<u64> =
            first["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(dispatched, [1], "{first}");
        let next = full_next(root, &first);
        // O template nasce num link novo, e a página antiga fica parada.
        assert_eq!(first["publish"], json!(["spec", "project"]), "the old page is not the template: {first}");
        let record = format!(
            r#"'{{"page":"spec","milestone":"round","ok":true,"template":true,"stamp":"{}","url":"…"}}'"#,
            stamp_of(SPEC_PAGE)
        );
        assert!(next.contains(&record), "{next}");
        assert!(next.contains(&old_page_order(SPEC_PAGE, lang)), "{next}");
        assert!(!next.contains(&old_page_order(PROJECT_PAGE, lang)), "the project page is new: {next}");
        assert!(!first.to_string().contains(OLD_URL), "the old page is never touched: {first}");
        // A primeira cópia leva a spec inteira, e a conversa a copia ela
        // mesma: a frase dos lotes vai uma vez, solta, sem agente.
        assert_eq!(sent_items(root, &first).first(), Some(&1), "the whole spec: {first}");
        let copy = batches_order(&first, "x", translate("page.copy.new_address", lang), lang);
        let yourself = "Copie você mesmo, nesta conversa e sem agente,";
        assert!(copy.starts_with(yourself), "the batches order says who copies: {copy}");
        assert_eq!(next.matches(copy.as_str()).count(), 1, "the conversation copies it once: {next}");
        assert!(!next.contains('«'), "no text goes to an agent: {next}");
        assert!(first["copy"]["spec"].get("first").is_none(), "no first-copy mark for an agent: {first}");
        assert_eq!(translate("page.copy.agent", lang), "<missing-key>", "the agent text is gone");
        assert!(first.get("migration").is_none(), "the old-note migration is gone: {first}");

        // A conversa publica o template e não grava a cópia.
        write(root, "publish", json!({"page": "spec", "milestone": "round", "ok": true, "template": true, "stamp": stamp_of("spec"), "url": SPEC_URL}));
        write(root, "publish",
            json!({"page": "project", "milestone": "round", "ok": true, "template": true, "stamp": stamp_of("project"), "url": PROJECT_URL}));
        let second = round(root);
        let next = full_next(root, &second);
        assert!(second.get("publish").is_none(), "the link does not change: {second}");
        assert_eq!(sent_items(root, &second).first(), Some(&1), "the whole spec again: {second}");
        let copy = batches_order(&second, "x", SPEC_URL, lang);
        assert!(next.contains(&copy) && !next.contains('«'), "the conversation copies it again: {next}");
        assert!(second.get("migration").is_none(), "the old-note migration is gone: {second}");
        let rows = mustard_core::io::spec_index::read_rows(root);
        assert_eq!(rows[0].url.as_deref(), Some(SPEC_URL), "the status line shows the new link: {rows:?}");

        // A conversa grava a cópia: a seguinte leva só o que veio depois, e a
        // faixa tocada leva o registro da cópia como item mais novo.
        let recorded = id_of(&write(root, "copy", second["copy"]["spec"]["record"].clone()));
        let third = round(root);
        let next = full_next(root, &third);
        assert!(!next.contains('«'), "{next}");
        // A faixa do registro da cópia e o documento calculado já existem no
        // banco desde a primeira cópia, que a conversa gravou.
        let touched = format!("{RANGES}/{}", range_start(recorded));
        let expected = batches_order_with(&third, "x", SPEC_URL, &[touched.as_str(), COMPUTED], lang);
        assert!(next.contains(&expected), "{next}");
        assert_eq!(sent_items(root, &third).last(), Some(&recorded), "{third}");
    }

    /// Acrescenta `count` notas cruas ao arquivo de eventos da spec `x`, a
    /// partir do número seguinte ao último, direto no arquivo — não pelo
    /// gravador, que releria o arquivo inteiro a cada nota e custaria caro
    /// numa spec de milhares de itens. Devolve o número da nota de índice
    /// `mark_index` (a partir de 0), para o teste tocar um item no meio.
    fn append_notes(root: &Path, count: u64, mark_index: u64) -> u64 {
        let path = root.join(".claude/spec/x/spec.ndjson");
        let start = log(root).max_id() + 1;
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        let mut marked = 0;
        for i in 0..count {
            let id = start + i;
            if i == mark_index {
                marked = id;
            }
            let line = json!({"v": 1, "id": id, "at": "2026-09-19T00:00:00-03:00", "type": "note",
                "author": "assistant", "code": format!("MSTD-NOTE-{id:04}"), "keys": ["k"],
                "text": format!("Nota {i}.")});
            content.push_str(&line.to_string());
            content.push('\n');
        }
        mustard_core::io::fs::write_atomic(&path, content.as_bytes()).expect("bulk append");
        marked
    }

    /// Uma spec longa — mais de 5.000 itens, o bastante para estourar o teto
    /// de um documento por item — cabe no banco: os itens vão em documentos
    /// por faixa de números, bem abaixo do teto de 5.000 documentos, e
    /// nenhum passa do tamanho que faz uma faixa se partir. Um item novo
    /// troca só a faixa dele; um expurgo no meio troca só a faixa do item
    /// expurgado. Juntando as três cópias, a página mostra todos os itens, na
    /// ordem, como antes — sem um documento por item.
    #[test]
    fn a_long_spec_fits_the_page_database() {
        let dir = approved_project();
        let root = dir.path();
        let middle = append_notes(root, 5_200, 2_500);

        let first = round(root);
        let writes1 = sent(root, &first, "spec");
        let range_docs1: Vec<&Value> =
            writes1.iter().filter(|w| w["collection"] == json!(RANGES) && w["op"] == json!("set")).collect();
        assert!(range_docs1.len() < 5_000, "{} documents for 5.200+ items", range_docs1.len());
        for doc in &range_docs1 {
            let size = doc["body"].to_string().len();
            assert!(size <= RANGE_MAX_BYTES, "{}: {size} bytes", doc["doc_id"]);
        }
        let batch_files = first["copy"]["spec"]["batches"].as_array().cloned().unwrap_or_default();
        assert!(batch_files.len() >= 2, "more than 50 writes split into several batch files: {batch_files:?}");
        follow(root, &first);

        // Um item novo troca só a faixa dele.
        let said = log(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id);
        let added = id_of(&write(root, "note", json!({"text": "Nota nova.", "keys": ["k"], "origin": said})));
        let second = round(root);
        let writes2 = sent(root, &second, "spec");
        let range_docs2: Vec<&Value> = writes2.iter().filter(|w| w["collection"] == json!(RANGES)).collect();
        assert_eq!(range_docs2.len(), 1, "{range_docs2:?}");
        assert_eq!(range_docs2[0]["doc_id"], json!(range_start(added).to_string()));
        follow(root, &second);

        // O expurgo no meio troca a faixa do item expurgado, e o próprio
        // registro do expurgo é um item novo, que troca a faixa dele — a
        // mais nova, distante da do item no meio de uma spec deste tamanho.
        let purged = id_of(&write(
            root,
            "purge",
            json!({"targets": [middle], "reason": "client_data", "excerpt": "2500", "origin": said}),
        ));
        let third = round(root);
        let writes3 = sent(root, &third, "spec");
        let range_docs3: Vec<&Value> = writes3.iter().filter(|w| w["collection"] == json!(RANGES)).collect();
        assert_eq!(range_docs3.len(), 2, "{range_docs3:?}");
        let target_range =
            range_docs3.iter().find(|w| w["doc_id"] == json!(range_start(middle).to_string())).expect("{range_docs3:?}");
        assert!(
            range_docs3.iter().any(|w| w["doc_id"] == json!(range_start(purged).to_string())),
            "the purge record's own range: {range_docs3:?}"
        );
        let purged_item = target_range["body"]["items"]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["id"].as_u64() == Some(middle)))
            .expect("the purged item stays, redacted");
        assert!(purged_item["text"].as_str().unwrap_or_default().contains('…'), "{purged_item}");

        // Juntando as três cópias, o banco mostra todos os itens, na ordem,
        // como uma leitura fresca do arquivo mostraria.
        let mut database: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
        for writes in [&writes1, &writes2, &writes3] {
            for w in writes.iter().filter(|w| w["collection"] == json!(RANGES)) {
                let doc_id = w["doc_id"].as_str().unwrap_or_default().to_string();
                match w["op"].as_str() {
                    Some("set") => {
                        database.insert(doc_id, w["body"].clone());
                    }
                    Some("delete") => {
                        database.remove(&doc_id);
                    }
                    _ => {}
                }
            }
        }
        let mut got: Vec<u64> = database
            .values()
            .flat_map(|doc| doc["items"].as_array().cloned().unwrap_or_default())
            .filter_map(|item| item["id"].as_u64())
            .collect();
        got.sort_unstable();
        let final_log = log(root);
        let starts: BTreeSet<u64> = final_log.events.iter().map(|e| range_start(e.id)).collect();
        let expected: Vec<u64> = starts.iter().flat_map(|&s| range_items(&final_log, s)).map(|(id, _)| id).collect();
        assert_eq!(got, expected, "the page shows every item, in order, as before");
    }

    /// Um modelo velho já instalado no projeto — de antes do carimbo, sem
    /// catálogo nenhum — é reescrito pelo modelo de agora antes do primeiro
    /// marco publicar a página: o passo confere o carimbo contra o do molde
    /// que o binário monta, e a diferença manda gravar o modelo fresco, não o
    /// deixar como estava.
    #[test]
    fn an_old_installed_template_is_rewritten_before_the_first_publish() {
        let dir = approved_project();
        let root = dir.path();
        let path = root.join(SPEC_TEMPLATE);
        std::fs::create_dir_all(path.parent().expect("a pasta do modelo")).unwrap();
        std::fs::write(&path, "<!doctype html><html>modelo velho, sem selo</html>").unwrap();

        let first = round(root);
        assert_eq!(first["publish"], json!(["spec", "project"]), "{first}");

        let installed = std::fs::read_to_string(&path).unwrap();
        assert_eq!(installed, spec_page_template(Locale::PtBr), "o modelo velho foi trocado pelo de agora");
    }

    /// O modelo instalado com a mesma versão e outro conteúdo — o molde do
    /// programa instalado, lido por um programa compilado no meio de uma
    /// obra — é reescrito; o modelo com o carimbo de agora fica como está.
    #[test]
    fn the_installed_template_is_compared_by_the_whole_stamp() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = root.join(SPEC_TEMPLATE);
        std::fs::create_dir_all(path.parent().expect("a pasta do modelo")).unwrap();
        let built = spec_page_template(Locale::PtBr);

        let same_version = format!("<!-- mustard: {} -->\nmolde de ontem", mustard_core::harness_version());
        std::fs::write(&path, &same_version).unwrap();
        ensure_template(root, SPEC_TEMPLATE, &built).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), built, "a mesma versão com outro conteúdo é trocada");

        let current = format!("<!-- mustard: {} -->\nmolde já em dia", template_stamp(&built).expect("the stamp"));
        std::fs::write(&path, &current).unwrap();
        ensure_template(root, SPEC_TEMPLATE, &built).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), current, "o mesmo carimbo fica como está");
    }

    /// A página já publicada com um molde velho é publicada de novo no mesmo
    /// endereço, antes do lote de cópia: sem carimbo, com o carimbo de outra
    /// versão e com o carimbo da mesma versão e outro conteúdo, as três. O
    /// molde novo é escrito no projeto, e a ordem manda gravar a publicação
    /// com o carimbo. Com o carimbo de agora, nada é publicado de novo. O
    /// mesmo vale para a página do projeto.
    #[test]
    fn a_pagina_publicada_com_molde_antigo_e_publicada_de_novo_no_mesmo_endereco() {
        let lang = Locale::PtBr;
        // O molde da mesma versão com outro conteúdo: o mesmo template
        // montado com o catálogo em outro idioma.
        let other_content = |page: &str| {
            let template =
                if page == PROJECT_PAGE { project_page_template(Locale::EnUs) } else { spec_page_template(Locale::EnUs) };
            let stamp = template_stamp(&template).expect("the stamp").to_string();
            assert!(stamp.starts_with(mustard_core::harness_version().as_str()), "the same version: {stamp}");
            stamp
        };
        let other_version = "0.0.1 0123456789abcdef".to_string();
        // O carimbo velho de cada caso, o da página da spec e o da do projeto.
        let cases: [(&str, Option<(String, String)>); 3] = [
            ("sem carimbo", None),
            ("outra versão", Some((other_version.clone(), other_version))),
            ("outro conteúdo", Some((other_content(SPEC_PAGE), other_content(PROJECT_PAGE)))),
        ];
        for (case, old) in cases {
            let dir = approved_project();
            let root = dir.path();
            for (page, url) in [(SPEC_PAGE, SPEC_URL), (PROJECT_PAGE, PROJECT_URL)] {
                let mut publish =
                    json!({"page": page, "milestone": "approval", "ok": true, "template": true, "url": url});
                if let Some((spec, project)) = &old {
                    publish["stamp"] = json!(if page == SPEC_PAGE { spec } else { project });
                }
                write(root, "publish", publish);
            }
            write(root, "copy", json!({"page": "spec", "last": log(root).max_id()}));
            write(root, "copy", json!({"page": "project", "phase": "approved"}));
            std::fs::create_dir_all(root.join(SPEC_TEMPLATE).parent().expect("a pasta")).unwrap();
            for template in [SPEC_TEMPLATE, PROJECT_TEMPLATE] {
                std::fs::write(root.join(template), "<!-- mustard: 0.0.1 -->\nmolde velho").unwrap();
            }

            let first = round(root);
            assert_eq!(first["publish"], json!(["spec", "project"]), "{case}: {first}");
            let next = full_next(root, &first);
            for (page, url, template, capabilities, name) in [
                (SPEC_PAGE, SPEC_URL, SPEC_TEMPLATE, SPEC_CAPABILITIES, "page.name.spec"),
                (PROJECT_PAGE, PROJECT_URL, PROJECT_TEMPLATE, PROJECT_CAPABILITIES, "page.name.project"),
            ] {
                let republish = translate("page.copy.republish", lang)
                    .replace("{page}", translate(name, lang))
                    .replace("{template}", template)
                    .replace("{capabilities}", capabilities)
                    .replace("{spec}", "x")
                    .replace("{key}", page)
                    .replace("{milestone}", "round")
                    .replace("{stamp}", &stamp_of(page))
                    .replace("{url}", url);
                let copy = translate("page.copy.batches", lang)
                    .replace("{page}", translate(name, lang))
                    .replace("{url}", url);
                let copy = copy.split("{files}").next().unwrap_or_default();
                let at = |s: &str| next.find(s).unwrap_or_else(|| panic!("{case}: missing «{s}» in {next}"));
                assert!(at(&republish) < at(copy), "{case}: publish again before the batches: {next}");
                let installed = std::fs::read_to_string(root.join(template)).unwrap();
                assert_eq!(template_stamp(&installed), Some(stamp_of(page).as_str()), "{case}: the new template");
            }
            assert!(!next.contains(translate("page.copy.new_address", lang)), "{case}: the same address: {next}");
            assert!(next.contains("if_version"), "{case}: the database is still there: {next}");

            // A conversa publica de novo com o carimbo: o marco seguinte não
            // publica mais.
            follow(root, &first);
            let second = round(root);
            assert!(second.get("publish").is_none(), "{case}: the same stamp publishes nothing: {second}");
            let next = full_next(root, &second);
            assert!(!next.contains("write publish"), "{case}: {next}");
        }
    }

    /// A página do projeto publicada de novo noutra spec, com o carimbo de
    /// agora, vale para esta: o carimbo da página do projeto é o da última
    /// publicação dela em qualquer spec do projeto.
    #[test]
    fn the_project_page_stamp_recorded_in_another_spec_counts() {
        let dir = approved_project();
        let root = dir.path();
        for page in [SPEC_PAGE, PROJECT_PAGE] {
            let url = if page == SPEC_PAGE { SPEC_URL } else { PROJECT_URL };
            write(root, "publish", json!({"page": page, "milestone": "approval", "ok": true, "template": true,
                "url": url, "stamp": if page == SPEC_PAGE { stamp_of(page) } else { "0.0.1 0123456789abcdef".into() }}));
        }
        write(root, "copy", json!({"page": "spec", "last": log(root).max_id()}));
        write(root, "copy", json!({"page": "project", "phase": "approved"}));
        let other = root.join(".claude/spec/y/spec.ndjson");
        store::write(&other, "state", json!({"phase": "survey"}).as_object().cloned().unwrap(), &[]).unwrap();
        // A publicação da outra spec vem depois, numa hora bem à frente: no
        // mesmo segundo, a desta spec valeria.
        let again = json!({"v": 1, "id": 2, "at": "2099-01-01T00:00:00-03:00", "type": "publish",
            "author": "assistant", "code": "MSTD-PUB-0001", "page": "project", "milestone": "round", "ok": true,
            "template": true, "url": PROJECT_URL, "stamp": stamp_of(PROJECT_PAGE)});
        let mut content = std::fs::read_to_string(&other).unwrap();
        content.push_str(&format!("{again}\n"));
        std::fs::write(&other, content).unwrap();

        let first = round(root);
        assert!(first.get("publish").is_none(), "the other spec already published the new template: {first}");
    }

    /// A linha do gasto soma os tokens de toda onda enviada e toma o maior
    /// gasto de quem despachou entre as ondas, sem comparar a soma com
    /// régua nenhuma por arquivo tocado.
    #[test]
    fn the_spend_line_sums_tokens_without_a_file_ruler() {
        let log = mustard_core::domain::spec_events::parse_log(
            "{\"v\":1,\"id\":1,\"at\":\"2026-09-19T09:00:00-03:00\",\"type\":\"send\",\"author\":\"binary\",\
             \"wave\":1,\"role\":\"wave\",\"text\":\"t\",\"lines\":1,\"chars\":1,\"items\":[],\
             \"mustard\":\"0.2.1\",\"tokens\":1000000,\"caller_tokens\":200000}\n\
             {\"v\":1,\"id\":2,\"at\":\"2026-09-19T09:01:00-03:00\",\"type\":\"send\",\"author\":\"binary\",\
             \"wave\":2,\"role\":\"wave\",\"text\":\"t\",\"lines\":1,\"chars\":1,\"items\":[],\
             \"mustard\":\"0.2.1\",\"tokens\":500000,\"caller_tokens\":900000}\n\
             {\"v\":1,\"id\":3,\"at\":\"2026-09-19T09:02:00-03:00\",\"type\":\"delivered\",\"author\":\"assistant\",\
             \"wave\":1,\"text\":\"d\",\"files\":[\"src/a.rs\"]}\n\
             {\"v\":1,\"id\":4,\"at\":\"2026-09-19T09:03:00-03:00\",\"type\":\"delivered\",\"author\":\"assistant\",\
             \"wave\":2,\"text\":\"d\",\"files\":[\"src/a.rs\",\"src/b.rs\"]}\n",
        );
        // Tokens de onda: 1_000_000 + 500_000 = 1_500_000. Tokens de quem
        // despachou: máximo entre 200_000 e 900_000 = 900_000. Total:
        // 2_400_000. Nenhuma régua de tokens por arquivo entra na conta: a
        // linha não traz "régua", "×" nem os vereditos "barata"/"cara".
        let line = spend_line(&log, Locale::PtBr).expect("the spend line");
        assert!(line.contains("1500000"), "{line}");
        assert!(line.contains("900000"), "{line}");
        assert!(line.contains("2400000"), "{line}");
        assert!(!line.contains("égua"), "{line}");
        assert!(!line.contains("barata"), "{line}");
        assert!(!line.contains("cara"), "{line}");
    }

    /// A linha do gasto continua aparecendo mesmo sem nenhum arquivo
    /// entregue ainda: sem régua por arquivo, não há mais divisão por zero
    /// arquivo a evitar, e o que importa é ter algum token registrado.
    #[test]
    fn the_spend_line_shows_up_without_a_delivered_file() {
        let log = mustard_core::domain::spec_events::parse_log(
            "{\"v\":1,\"id\":1,\"at\":\"2026-09-19T09:00:00-03:00\",\"type\":\"send\",\"author\":\"binary\",\
             \"wave\":1,\"role\":\"wave\",\"text\":\"t\",\"lines\":1,\"chars\":1,\"items\":[],\
             \"mustard\":\"0.2.1\",\"tokens\":1000000}\n",
        );
        let line = spend_line(&log, Locale::PtBr).expect("the spend line");
        assert!(line.contains("1000000"), "{line}");
    }

    /// A linha do gasto também mostra os turnos por tarefa: uma rodada com
    /// 3 tarefas e 24 turnos aparece como 8 turnos por tarefa — o texto
    /// inteiro do pedaço, não só o algarismo 8 (que passaria com qualquer
    /// número que tivesse um oito, como os próprios 1000 tokens da onda).
    #[test]
    fn a_linha_de_gasto_mostra_turnos_por_tarefa() {
        let log = mustard_core::domain::spec_events::parse_log(
            "{\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"wave\",\"n\":1,\"text\":\"Onda 1.\"}\n\
             {\"v\":1,\"id\":2,\"at\":\"t\",\"type\":\"task\",\"wave\":1,\"text\":\"Tarefa 1.\"}\n\
             {\"v\":1,\"id\":3,\"at\":\"t\",\"type\":\"task\",\"wave\":1,\"text\":\"Tarefa 2.\"}\n\
             {\"v\":1,\"id\":4,\"at\":\"t\",\"type\":\"task\",\"wave\":1,\"text\":\"Tarefa 3.\"}\n\
             {\"v\":1,\"id\":5,\"at\":\"t\",\"type\":\"send\",\"wave\":1,\"role\":\"wave\",\"text\":\"t\",\
             \"lines\":1,\"chars\":1,\"items\":[],\"mustard\":\"0.2.1\",\"tokens\":1000,\"steps\":24}\n",
        );
        let line = spend_line(&log, Locale::PtBr).expect("the spend line");
        assert_eq!(
            line,
            "Gasto total: 1000 tokens de onda + 0 tokens de quem despachou = 1000 tokens \
             (8 turnos por tarefa).",
            "{line}"
        );
    }

    /// Sem nenhum token registrado ainda, a linha do gasto fica de fora.
    #[test]
    fn the_spend_line_stays_out_without_any_token() {
        let log = mustard_core::domain::spec_events::parse_log("");
        assert_eq!(spend_line(&log, Locale::PtBr), None);
    }

    /// O marco de verdade também leva a linha do gasto: depois do envio da
    /// onda (com o consumo dela) e da entrega (com o arquivo tocado), o
    /// documento das coisas calculadas que a cópia grava traz `spend`
    /// preenchida, pelo mesmo caminho que a página lê.
    #[test]
    fn a_real_round_carries_the_spend_line_to_the_computed_document() {
        let dir = approved_project();
        let root = dir.path();
        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave": 1, "role": "wave", "text": "t",
            "lines": 1, "chars": 1, "items": [1], "mustard": "0.2.1", "tokens": 1_000_000, "caller_tokens": 200_000}));
        crate::shared::spec_state::seed_event(root, "x", "delivered", json!({"wave": 1, "text": "d", "files": ["src/a.rs"]}));

        let report = round(root);
        let bodies = sent(root, &report, "spec");
        let computed = bodies.iter().find(|w| w["collection"] == json!("computed")).expect("the computed item");
        let spend = computed["body"]["spend"].as_str().unwrap_or_default();
        assert!(spend.contains("1000000"), "{spend}");
        assert!(spend.contains("200000"), "{spend}");
        assert!(spend.contains("1200000"), "{spend}");
    }

    /// O documento das coisas calculadas leva também os números do gasto,
    /// cada um no seu campo, pelo mesmo caminho que a página lê: os tokens
    /// das ondas, os de quem despachou, o total e os turnos por tarefa — os
    /// mesmos da frase. Sem nenhum token, o campo vem vazio.
    #[test]
    fn a_real_round_carries_the_spend_numbers_to_the_computed_document() {
        let dir = approved_project();
        let root = dir.path();
        let computed_of = |root: &std::path::Path| {
            let report = round(root);
            let bodies = sent(root, &report, "spec");
            bodies.iter().find(|w| w["collection"] == json!("computed")).expect("the computed item")["body"].clone()
        };
        assert_eq!(computed_of(root)["tokens"], Value::Null, "no token yet");

        crate::shared::spec_state::seed_event(root, "x", "send", json!({"wave": 1, "role": "wave", "text": "t",
            "lines": 1, "chars": 1, "items": [1], "mustard": "0.2.1", "tokens": 900_000, "steps": 40,
            "caller_tokens": 300_000}));
        crate::shared::spec_state::seed_event(root, "x", "delivered", json!({"wave": 1, "text": "d", "files": ["src/a.rs"]}));
        let body = computed_of(root);
        let tokens = &body["tokens"];
        assert_eq!((&tokens["waves"], &tokens["caller"], &tokens["total"]), (&json!(900_000), &json!(300_000), &json!(1_200_000)), "{body}");
        let turns = tokens["turns"].as_u64().expect("the turns per task");
        assert!(turns > 0, "{body}");
        let line = body["spend"].as_str().unwrap_or_default();
        assert!(line.contains(&turns.to_string()), "the same turns as the spend line: {line} / {tokens}");
    }
}
