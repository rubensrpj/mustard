//! O pedido de uma onda: o texto que o agente dela recebe, montado dos blocos
//! já lidos do arquivo de eventos.
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem descobrir caminho nenhum.
//! Quem lê o arquivo, o banco de lições e os arquivos das skills entrega os
//! blocos prontos em [`Material`] — inclusive as pastas da cópia separada, que
//! a rodada escolhe —; esta função só os escreve, sempre na mesma ordem, então
//! o mesmo material dá sempre os mesmos bytes.
//!
//! O pedido de uma onda leva, de cada item, o código e o título; de cada
//! tarefa, também a parte do agente — arquivos, comandos e o que testar —,
//! porque é ela que abre o trabalho. O porquê de cada item fica na spec: o
//! pedido diz uma vez só como ler, com o caminho do repositório principal
//! quando o agente trabalha numa cópia — em caso de dúvida, o comando que lê
//! o pedido inteiro, com o texto completo de cada item, e o que lê um item
//! pelo código. Abre pela frase do `done_when` da própria onda, que é o que
//! ela entrega. O item gravado antes de o título existir sai com a primeira
//! frase do texto no lugar dele, e a tarefa sem a parte do agente, com o
//! texto dela. As tarefas saem na ordem de execução que a onda declara, cada
//! uma com os arquivos dela e o que precisa ler antes; sem essa ordem, na
//! ordem do arquivo. A lista inteira fica no pedido, porque é ela que mostra
//! o escopo todo de uma vez. As lições entram pelo número delas no banco —
//! `run read lessons --term <número>` devolve o texto — e cada skill entra
//! como recomendação de uma linha. O pedido da revisão final continua só com
//! os códigos, numa linha por bloco da spec, e manda ler cada um na ordem. O
//! pedido não tem teto de linhas.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;

use crate::domain::lessons::{applies_to, same_file, text_only, tied_to_wave, Scope};
use crate::domain::mustard_id;
use crate::domain::project_map::cited_paths;
use crate::domain::spec_events::{type_spec, Block, BlockQuery, Refusal, SpecEvent, SpecLog, Step, TASK_TITLE_MAX};
use crate::domain::spec_index::{cut, title_of};
use crate::domain::spec_state::State;
use crate::platform::i18n::{translate, Locale};

/// O modelo pedido no envio, seja qual for o papel (`wave`, `review` ou
/// `skill`): todo agente do Mustard sai em Opus, pelo apelido que
/// a plataforma resolve sempre para a versão mais nova. Quem manda isso é o
/// binário, no próprio pedido — sem escolha explícita, o agente herda o
/// modelo da sessão e a decisão morre em silêncio.
#[must_use]
pub fn requested_model(_role: &str) -> &'static str {
    "Opus"
}

/// A skill que uma tarefa da onda nomeia, recomendada no pedido. O texto dela
/// não entra: a skill mora num arquivo do projeto, e o agente da onda o lê.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// O nome pelo qual a tarefa a chama.
    pub name: String,
    /// Quando usar, na própria descrição da skill.
    pub when: String,
    /// O caminho do arquivo, a partir da raiz do projeto.
    pub path: String,
    /// `true` quando um dos exemplos que a skill usa mudou depois dela: o
    /// pedido a marca como a revisar.
    pub stale: bool,
}

/// Uma cópia separada do repositório: a vaga fixa em que a onda trabalha e
/// compila. A compilação mora dentro dela e passa de uma onda para a
/// seguinte, então o preparo só roda de novo quando precisa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WaveCopy {
    /// A pasta da cópia.
    pub path: String,
    /// O que mudou desde o último uso da vaga, quando ela é reaproveitada: o
    /// pedido lista os arquivos e manda preparar só se um deles declara
    /// dependências. `None` na vaga nova, que sempre prepara.
    pub reused: Option<Reuse>,
}

/// A vaga reaproveitada: o commit em que ela estava e os arquivos que
/// mudaram de lá até o commit em que a onda começa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reuse {
    /// O commit em que a vaga estava antes de ser zerada.
    pub since: String,
    /// Os arquivos que mudaram desde esse commit, na ordem do git.
    pub changed: Vec<String>,
}

/// Quantos arquivos mudados o pedido lista na vaga reaproveitada. Acima
/// disso, ele diz quantos faltam e o comando que lista todos: a lista
/// inteira de uma vaga parada por muitos commits encheria o pedido.
const REUSED_FILES_SHOWN: usize = 40;

/// As regras da execução que o pedido leva, lidas do projeto e da rodada.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Execution {
    /// O comando que compila o projeto, quando ele declara um.
    pub build: Option<String>,
    /// O comando que roda os testes do projeto, quando ele declara um.
    pub test: Option<String>,
    /// O comando de preparo que o projeto declara (`prepareCommand`), como
    /// `npm ci`: os dois pedidos mandam rodá-lo dentro da cópia antes de
    /// compilar. Sem ele, nenhum dos dois fala de preparo.
    pub prepare: Option<String>,
    /// Os arquivos locais que o git ignora e a cópia precisa (`localFiles`),
    /// relativos à raiz e já sem o caminho que sairia do projeto: o pedido
    /// do revisor os lista, a copiar pelo conteúdo. Vazia, ele não fala deles.
    pub local_files: Vec<String>,
    /// As outras ondas em andamento, cada uma com os arquivos das tarefas
    /// dela: o arquivo dividido com elas é juntado na volta.
    pub running: Vec<(u64, Vec<String>)>,
    /// O commit em que o fechamento cria a cópia do revisor final; sem ele, o
    /// pedido do revisor diz que ela sai do commit atual. O pedido da onda
    /// não o cita.
    pub commit: Option<String>,
    /// O repositório principal: onde a spec mora e onde nada é editado.
    pub root: String,
    /// A cópia separada em que o agente trabalha: a que a rodada criou para
    /// a onda, ou a do revisor final, que o fechamento cria. Sem ela, o
    /// pedido da onda não fala de cópia.
    pub copy: Option<WaveCopy>,
}

/// Os blocos já lidos de que o pedido de uma onda é feito.
#[derive(Debug, Default)]
pub struct Material<'a> {
    /// O nome da spec.
    pub spec: String,
    /// O número da onda.
    pub wave: u64,
    /// O bloco da onda: a onda, as tarefas dela e as skills que elas nomeiam.
    pub block: Vec<&'a SpecEvent>,
    /// Os critérios que a onda aponta.
    pub criteria: Vec<&'a SpecEvent>,
    /// A especificação: contexto e preocupações.
    pub specification: Vec<&'a SpecEvent>,
    /// Os itens combinados de que esta onda ou o projeto são donos.
    pub agreed: Vec<&'a SpecEvent>,
    /// O entregou das ondas de que esta depende.
    pub delivered: Vec<&'a SpecEvent>,
    /// As linhas do conserto ([`fix_lines`]); vazio fora de um conserto.
    pub fix: Vec<&'a SpecEvent>,
    /// O que esta onda entregou depois da última revisão: o que o revisor
    /// confere.
    pub own_delivered: Vec<&'a SpecEvent>,
    /// As regras da execução.
    pub execution: Execution,
    /// As lições que valem para os arquivos, o subprojeto ou a skill da onda.
    pub lessons: Vec<&'a SpecEvent>,
    /// As skills nomeadas pelas tarefas, na ordem dos nomes.
    pub skills: Vec<Skill>,
    /// Os arquivos de leitura que a escolha do orquestrador confirmou para
    /// cada tarefa, pelo código dela: o mapa sugeriu, e ele manteve. As
    /// skills confirmadas entram por [`Material::skills`], não aqui.
    pub task_reads: Vec<(String, Vec<String>)>,
    /// Os arquivos de teste que o mapa do projeto conhece para cada arquivo
    /// que uma tarefa cita, pelo caminho dele: a linha da tarefa os lista
    /// logo abaixo do arquivo. Um arquivo que o mapa não conhece, ou sem
    /// teste conhecido, fica de fora — a linha continua como antes, sem
    /// inventar nada.
    pub file_tests: BTreeMap<String, Vec<String>>,
    /// Os commits da rodada: as mudanças que já entraram na branch, que o
    /// agente de teste dedicado confere no pedido da revisão final.
    pub changes: Vec<&'a SpecEvent>,
    /// O que mudou desde o veredito final que reprovou
    /// ([`since_last_verdict`]): com ele, o pedido da revisão final é o da
    /// revisão de volta. Vazio na primeira revisão.
    pub since_verdict: Vec<&'a SpecEvent>,
    /// O código de cada evento, para o pedido citar item por código.
    pub codes: BTreeMap<u64, String>,
}

/// O pedido montado e o tamanho dele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// O texto inteiro do pedido.
    pub text: String,
    /// Quantas linhas ele tem.
    pub lines: usize,
}

/// O teto de tokens do pedido de uma onda: acima dele, quem despacha recusa
/// e diz o tamanho medido e o teto, para dividir o lote em dois. O teto é
/// de despachar, não de montar — [`build`] e [`write`] continuam escrevendo
/// o pedido inteiro, do tamanho que for, porque é esse texto que a página
/// mostra antes da aprovação; ninguém corta linha para caber.
pub const WAVE_REQUEST_TOKEN_CAP: u64 = 25_000;

/// Uma estimativa do tamanho de `text` em tokens: perto de um token a cada
/// quatro caracteres, a mesma conta grosseira usada para orçar prompt de
/// modelo sem o tokenizador dele à mão. Erra para cima com texto técnico
/// cheio de pontuação — o bastante para um teto de segurança, não para
/// cobrar por token de verdade.
#[must_use]
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// O pedido da onda `wave`, medido em `tokens` tokens (de
/// [`estimate_tokens`]), passa do teto de [`WAVE_REQUEST_TOKEN_CAP`]? `None`
/// quando cabe; a mensagem, pronta para a recusa, diz o tamanho medido e o
/// teto.
#[must_use]
pub fn token_cap_message(wave: u64, tokens: u64, lang: Locale) -> Option<String> {
    if tokens <= WAVE_REQUEST_TOKEN_CAP {
        return None;
    }
    Some(
        translate("wave_prompt.token_cap", lang)
            .replace("{wave}", &wave.to_string())
            .replace("{tokens}", &tokens.to_string())
            .replace("{cap}", &WAVE_REQUEST_TOKEN_CAP.to_string()),
    )
}

/// Monta o pedido da onda a partir do material já lido. Sem teto de linhas:
/// o pedido sai inteiro, do tamanho que a onda pedir. O teto de tokens
/// ([`token_cap_message`]) é conferido à parte, por quem decide despachar,
/// depois de medir este texto.
#[must_use]
pub fn build(material: &Material, lang: Locale) -> Prompt {
    let text = write(material, lang);
    let lines = count_lines(&text);
    Prompt { text, lines }
}

/// O texto do pedido, sem medir nem recusar: a página mostra mesmo o pedido
/// grande demais, que é justamente o que precisa ser visto antes da aprovação.
#[must_use]
pub fn write(material: &Material, lang: Locale) -> String {
    Writer { material, lang }.text()
}

/// O texto do pedido do agente de teste dedicado, que o fechamento pede a
/// toda obra, mesmo a de uma onda só: as ondas e as tarefas delas (`block`),
/// as emendas gravadas para elas (`agreed`), o que cada onda entregou por
/// último (`own_delivered`), os critérios, os commits da branch (`changes`) e
/// como revisar numa cópia separada. Depois de um veredito final que reprovou
/// (`since_verdict`), o pedido lista o que mudou desde ele e manda conferir
/// isso e o encaixe no resto, repetindo a conclusão anterior para os outros
/// itens acordados — é o único jeito de a revisão de volta dizer o que
/// conferir, reprove o veredito uma onda ou a obra. As linhas do conserto
/// (`fix`) são do pedido da onda e não entram aqui. Nenhum pedido manda rodar
/// a suíte inteira: ela passou no fechamento, no commit da cópia. O número da
/// onda do material não conta aqui.
#[must_use]
pub fn write_final_review(material: &Material, lang: Locale) -> String {
    Writer { material, lang }.final_review_text()
}

// ---------------------------------------------------------------------------
// As linhas de item
// ---------------------------------------------------------------------------

/// O título de um item no pedido: o `title` gravado ou, no item gravado antes
/// de o título existir, o que a leitura da spec já tira dele — o trecho em
/// negrito do começo ou a primeira frase do texto —, cortado em
/// [`TASK_TITLE_MAX`] caracteres. `None` sem título nem texto, como o
/// critério antigo, que só tem `when` e `then`.
fn item_title(item: &SpecEvent) -> Option<String> {
    let own = item.str_field("title").map(str::trim).filter(|title| !title.is_empty());
    let title = own.map(str::to_string).or_else(|| title_of(item))?;
    Some(cut(title.trim(), TASK_TITLE_MAX))
}

/// A linha de um item do pedido da onda, a mesma que a página da spec lê:
/// o bloco — o que o comando de leitura pede —, o código e o título; sem
/// título, só o bloco e o código.
fn item_line(material: &Material, item: &SpecEvent) -> String {
    let code = material.codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string());
    let block = item.block().map_or("waves", Block::name);
    match item_title(item) {
        Some(title) => format!("- `{block}` {code}: {title}"),
        None => format!("- `{block}` {code}"),
    }
}

/// A parte do agente de um item, logo abaixo da linha dele e recuada dois
/// espaços: o `agent` gravado ou, no item gravado antes dele, o texto
/// inteiro. Só o tipo que descreve o trabalho tem essa parte na forma dele;
/// a onda, os relatos dos agentes e o critério, que não a têm, seguem numa
/// linha só. As linhas em branco do corpo ficam de fora.
fn agent_part(out: &mut String, item: &SpecEvent) {
    let has_part = type_spec(&item.event_type).is_some_and(|spec| spec.fields.iter().any(|f| f.name == "agent"));
    if !has_part {
        return;
    }
    let own = item.str_field("agent").filter(|agent| !agent.trim().is_empty());
    let body = own.or_else(|| item.str_field("text")).unwrap_or_default();
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let _ = writeln!(out, "  {}", line.trim_end());
    }
}

/// As linhas de uma lista de itens: uma por bloco da spec, na ordem em que o
/// primeiro item de cada bloco aparece, com o nome do bloco — o que o comando
/// de leitura pede — e os códigos dos itens dele, em sequência. É a forma do
/// pedido da revisão final e da entrega das ondas anteriores; nem o texto do
/// item nem o comando entram aqui.
fn codes_by_block(material: &Material, items: &[&SpecEvent]) -> Vec<String> {
    let mut blocks: Vec<(&str, Vec<String>)> = Vec::new();
    for item in items {
        let code = material.codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string());
        let block = item.block().map_or("waves", Block::name);
        match blocks.iter_mut().find(|(name, _)| *name == block) {
            Some((_, codes)) => codes.push(code),
            None => blocks.push((block, vec![code])),
        }
    }
    blocks.into_iter().map(|(block, codes)| format!("- `{block}`: {}", codes.join(", "))).collect()
}

/// Quantas linhas um texto tem; a última conta mesmo sem quebra no fim.
#[must_use]
pub fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.lines().count()
}

// ---------------------------------------------------------------------------
// O dono de cada item combinado
// ---------------------------------------------------------------------------

/// De quem é um item combinado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// Do projeto: a regra que vale sempre, e vai para todo pedido.
    Project,
    /// Das ondas do plano que o têm: as das tarefas que o cobrem e as que ele
    /// diz no campo `waves`.
    Waves(BTreeSet<u64>),
    /// Dos arquivos que ele diz em `applies_to`: vai no pedido da onda em que
    /// algum arquivo das tarefas casa um desses padrões. É o dono do backlog,
    /// em que o número da onda só existe quando o lote sai.
    Files(Vec<String>),
}

/// O dono de cada item combinado, pelo número do item. O item sem dono não
/// entra: nenhuma tarefa de uma onda do plano o cobre, ele não diz uma onda
/// do plano em `waves`, não diz arquivos em `applies_to` e não vale no
/// projeto todo.
///
/// O item do projeto é o que diz, em `applies_to`, que vale no projeto todo:
/// a mesma leitura que acha a lição do projeto todo. O que diz arquivos, sem
/// o curinga, e não tem onda dona, é dos arquivos. A busca por palavras não
/// decide dono nenhum.
#[must_use]
pub fn owners(log: &SpecLog) -> BTreeMap<u64, Owner> {
    let planned = log.planned_waves();
    let items = agreed_items(log);
    let shown: BTreeSet<u64> = items.iter().map(|item| item.id).collect();
    let replaced_by: BTreeMap<u64, u64> =
        log.events.iter().filter_map(|e| e.int("replaces").map(|old| (old, e.id))).collect();
    let newest = |mut id: u64| {
        for _ in 0..=log.events.len() {
            match replaced_by.get(&id) {
                Some(next) => id = *next,
                None => break,
            }
        }
        id
    };
    let mut covered: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for task in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "task") {
        let Some(n) = task.wave().filter(|n| planned.contains(n)) else { continue };
        for id in task.ints("covers").into_iter().map(newest).filter(|id| shown.contains(id)) {
            covered.entry(id).or_default().insert(n);
        }
    }
    let whole_project = Scope::default();
    let mut out: BTreeMap<u64, Owner> = BTreeMap::new();
    for item in items {
        if applies_to(item, &whole_project) {
            out.insert(item.id, Owner::Project);
            continue;
        }
        let mut waves = covered.remove(&item.id).unwrap_or_default();
        waves.extend(item.ints("waves").into_iter().filter(|n| planned.contains(n)));
        if !waves.is_empty() {
            out.insert(item.id, Owner::Waves(waves));
            continue;
        }
        let files = applies_to_files(item);
        if !files.is_empty() {
            out.insert(item.id, Owner::Files(files));
        }
    }
    out
}

/// Os padrões de arquivo que um item diz em `applies_to`, sem os vazios.
fn applies_to_files(item: &SpecEvent) -> Vec<String> {
    item.fields
        .get("applies_to")
        .and_then(|at| at.get("files"))
        .and_then(Value::as_array)
        .map(|files| {
            files.iter().filter_map(Value::as_str).map(str::trim).filter(|f| !f.is_empty()).map(str::to_string).collect()
        })
        .unwrap_or_default()
}

/// Os itens combinados que vão no pedido da onda `wave`, como a montagem os
/// escolhe antes da análise: os de que ela é dona, os dos arquivos que as
/// tarefas dela tocam e os do projeto. O item dos arquivos casa pela mesma
/// leitura de `applies_to` que acha a lição. O item sem dono não entra aqui;
/// só a análise antes do envio ([`dispatch_items`]) pode pô-lo num pedido.
/// A onda só de texto ([`text_only`]) não leva o item do projeto, pela mesma
/// leitura que tira dela a lição do projeto todo; o item do projeto que as
/// tarefas dela fazem vai mesmo assim, porque é o trabalho dela.
#[must_use]
pub fn agreed_for(log: &SpecLog, wave: u64) -> Vec<&SpecEvent> {
    let owners = owners(log);
    let files = wave_files(log, wave);
    let text = text_only(&files);
    let done = done_by(log, wave);
    let touched = Scope { files, ..Scope::default() };
    agreed_items(log)
        .into_iter()
        .filter(|item| match owners.get(&item.id) {
            Some(Owner::Project) => !text || done.contains(&item.id),
            Some(Owner::Waves(waves)) => waves.contains(&wave),
            Some(Owner::Files(_)) => applies_to(item, &touched),
            None => false,
        })
        .collect()
}

/// Os itens combinados sem dono, em ordem de número. A análise antes do
/// envio julga, para cada onda, só os que servem a ela ([`candidates`]).
#[must_use]
pub fn unowned(log: &SpecLog) -> Vec<&SpecEvent> {
    let owners = owners(log);
    agreed_items(log).into_iter().filter(|item| !owners.contains_key(&item.id)).collect()
}

/// Todo o combinado vigente da spec, dono ou não de onda: a lista que a
/// revisão final precisa responder, item por item, mesmo numa rodada de
/// conserto.
#[must_use]
pub fn all_agreed(log: &SpecLog) -> Vec<&SpecEvent> {
    agreed_items(log)
}

// ---------------------------------------------------------------------------
// A escolha do pedido antes do envio
// ---------------------------------------------------------------------------

/// Os candidatos que o orquestrador julga antes de uma onda sair: os itens
/// combinados do projeto todo, que o pedido leva, os sem dono, que ele não
/// leva, e as lições do banco que casam com a onda, que ele leva. Os itens
/// que as tarefas da onda fazem ficam fora dos grupos: vão sempre, sem
/// escolha.
#[derive(Debug, Default)]
pub struct Candidates<'a> {
    /// Os do projeto todo que as tarefas da onda não fazem; vazio na onda
    /// só de texto.
    pub project: Vec<&'a SpecEvent>,
    /// Os sem dono que servem à onda, pelas palavras-chave ou pelo arquivo
    /// citado.
    pub unowned: Vec<&'a SpecEvent>,
    /// As lições do banco que casam com a onda. Elas vêm do banco, fora da
    /// spec, e o número de cada uma é o do banco, não o de um item.
    pub lessons: Vec<&'a SpecEvent>,
}

impl Candidates<'_> {
    /// `true` quando não há nada a julgar: a onda sai sem escolha.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.project.is_empty() && self.unowned.is_empty() && self.lessons.is_empty()
    }

    /// Os números dos itens da spec dos dois grupos.
    #[must_use]
    pub fn ids(&self) -> BTreeSet<u64> {
        self.project.iter().chain(&self.unowned).map(|item| item.id).collect()
    }

    /// Os números das lições, no banco.
    #[must_use]
    pub fn lesson_ids(&self) -> BTreeSet<u64> {
        self.lessons.iter().map(|lesson| lesson.id).collect()
    }
}

/// Os dois grupos de itens da spec que o orquestrador julga para a onda
/// `wave`; as lições, que moram no banco, quem lê o banco põe à parte. A
/// onda só de texto ([`text_only`]) não tem o grupo do projeto: o pedido
/// dela não leva item do projeto, e não há o que julgar. Do grupo sem dono
/// fica só o item que serve à onda, pela escolha que as lições usam
/// ([`tied_to_wave`]): as palavras-chave dele casam com o texto das tarefas
/// dela, ou o texto dele cita um arquivo que ela mexe. O que não serve a
/// onda nenhuma não é candidato de nenhuma, e continua na lista da revisão
/// final ([`all_agreed`]).
#[must_use]
pub fn candidates(log: &SpecLog, wave: u64) -> Candidates<'_> {
    let owners = owners(log);
    let done = done_by(log, wave);
    let files = wave_files(log, wave);
    let text = text_only(&files);
    let mut out = Candidates::default();
    let mut unowned: Vec<&SpecEvent> = Vec::new();
    for item in agreed_items(log).into_iter().filter(|item| !done.contains(&item.id)) {
        match owners.get(&item.id) {
            Some(Owner::Project) if !text => out.project.push(item),
            None => unowned.push(item),
            Some(Owner::Project | Owner::Waves(_) | Owner::Files(_)) => {}
        }
    }
    out.unowned = tied_to_wave(unowned, &tasks_text(log, wave), &files);
    out
}

/// Os itens que as tarefas da onda `wave` fazem, na versão vigente.
fn done_by(log: &SpecLog, wave: u64) -> BTreeSet<u64> {
    log.block(BlockQuery::Wave(wave))
        .into_iter()
        .filter(|e| e.event_type == "task")
        .flat_map(|task| task.ints("covers"))
        .filter_map(|id| log.current(id).map(|e| e.id))
        .collect()
}

/// A escolha do orquestrador antes do envio de uma onda, como o envio a grava
/// no campo `analysis`: os itens julgados, os do projeto todo que saíram, os
/// sem dono que entraram e, à parte, as lições julgadas e as que saíram, cada
/// uma com o motivo numa frase.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    /// Os itens dos dois grupos que a escolha julgou.
    pub judged: BTreeSet<u64>,
    /// Os itens do projeto todo que saíram do pedido, com o motivo.
    pub removed: Vec<(u64, String)>,
    /// Os itens sem dono que entraram no pedido, com o motivo.
    pub added: Vec<(u64, String)>,
    /// As lições que a escolha julgou, pelo número no banco.
    pub judged_lessons: BTreeSet<u64>,
    /// As lições que saíram do pedido, com o motivo.
    pub removed_lessons: Vec<(u64, String)>,
    /// A escolha da skill e dos arquivos parecidos, por tarefa.
    pub tasks: Vec<TaskChoice>,
}

/// O que o orquestrador decidiu, para uma tarefa, entre a skill e os arquivos
/// parecidos que a rodada sugeriu antes do envio: as skills que ele confirmou
/// ou trocou, os arquivos que ele manteve como leitura, e se ele marcou que
/// nenhuma skill serve e vale nascer uma nova.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskChoice {
    /// O número da tarefa, na spec.
    pub task: u64,
    /// As skills confirmadas; a tarefa pode apontar mais de uma.
    pub skills: Vec<String>,
    /// Os arquivos de leitura confirmados, além dos que a tarefa já cita.
    pub files: Vec<String>,
    /// `true` quando o orquestrador marcou que vale nascer uma skill nova.
    pub new_skill: bool,
}

impl Choice {
    /// A escolha reduzida aos candidatos de agora: sai só o que ainda é do
    /// projeto todo ou lição da onda, entra só o que ainda está sem dono, e o
    /// julgado é o que está entre os candidatos.
    #[must_use]
    pub fn within(&self, found: &Candidates) -> Self {
        let has = |group: &[&SpecEvent], id: u64| group.iter().any(|item| item.id == id);
        Self {
            judged: found.ids(),
            removed: self.removed.iter().filter(|(id, _)| has(&found.project, *id)).cloned().collect(),
            added: self.added.iter().filter(|(id, _)| has(&found.unowned, *id)).cloned().collect(),
            judged_lessons: found.lesson_ids(),
            removed_lessons: self.removed_lessons.iter().filter(|(id, _)| has(&found.lessons, *id)).cloned().collect(),
            tasks: self.tasks.clone(),
        }
    }

    /// `true` quando a escolha julgou cada candidato de agora.
    #[must_use]
    pub fn covers(&self, found: &Candidates) -> bool {
        found.ids().is_subset(&self.judged) && found.lesson_ids().is_subset(&self.judged_lessons)
    }

    /// `true` quando a escolha tirou do pedido a lição `id`.
    #[must_use]
    pub fn removes_lesson(&self, id: u64) -> bool {
        self.removed_lessons.iter().any(|(had, _)| *had == id)
    }

    /// O campo `analysis` do envio.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let listed = |key: &str, items: &[(u64, String)]| -> Vec<Value> {
            items.iter().map(|(n, why)| serde_json::json!({ key: n, "why": why })).collect()
        };
        let tasks: Vec<Value> = self
            .tasks
            .iter()
            .map(|t| serde_json::json!({ "task": t.task, "skills": t.skills, "files": t.files, "new_skill": t.new_skill }))
            .collect();
        serde_json::json!({
            "judged": self.judged,
            "removed": listed("item", &self.removed),
            "added": listed("item", &self.added),
            "judged_lessons": self.judged_lessons,
            "removed_lessons": listed("lesson", &self.removed_lessons),
            "tasks": tasks,
        })
    }

    /// A escolha gravada num envio; `None` quando o campo não tem a forma
    /// de [`Choice::to_value`]. O envio gravado antes de as lições entrarem
    /// na escolha não traz os dois campos delas, e vale sem lição julgada; o
    /// gravado antes da escolha por tarefa não traz `tasks`, e vale sem
    /// nenhuma tarefa julgada.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        let listed = |key: &str, id: &str| -> Option<Vec<(u64, String)>> {
            value.get(key)?.as_array()?.iter().map(|entry| {
                Some((entry.get(id)?.as_u64()?, entry.get("why")?.as_str()?.to_string()))
            }).collect()
        };
        let numbers = |key: &str| -> Option<BTreeSet<u64>> {
            value.get(key)?.as_array()?.iter().map(Value::as_u64).collect()
        };
        let lessons = value.get("judged_lessons").is_some() || value.get("removed_lessons").is_some();
        let strings = |entry: &Value, key: &str| -> Vec<String> {
            entry.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
                .iter().filter_map(Value::as_str).map(str::to_string).collect()
        };
        let tasks: Vec<TaskChoice> = value
            .get("tasks")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| {
                Some(TaskChoice {
                    task: entry.get("task")?.as_u64()?,
                    skills: strings(entry, "skills"),
                    files: strings(entry, "files"),
                    new_skill: entry.get("new_skill").and_then(Value::as_bool).unwrap_or(false),
                })
            })
            .collect();
        Some(Self {
            judged: numbers("judged")?,
            removed: listed("removed", "item")?,
            added: listed("added", "item")?,
            judged_lessons: if lessons { numbers("judged_lessons")? } else { BTreeSet::new() },
            removed_lessons: if lessons { listed("removed_lessons", "lesson")? } else { Vec::new() },
            tasks,
        })
    }
}

/// A escolha gravada no envio mais novo da onda `wave`, quando ele tem uma.
#[must_use]
pub fn recorded_choice(log: &SpecLog, wave: u64) -> Option<Choice> {
    let sent = log.last_by_wave("send").get(&wave).and_then(|id| log.get(*id))?;
    Choice::from_value(sent.fields.get("analysis")?)
}

/// A escolha que vale para o pedido da onda `wave`: a dada (`fresh`) ou, sem
/// ela, a gravada no envio mais novo da onda.
#[must_use]
pub fn choice_for(log: &SpecLog, wave: u64, fresh: Option<&Choice>) -> Option<Choice> {
    fresh.cloned().or_else(|| recorded_choice(log, wave))
}

/// O que o pedido da onda `wave` lê: o que a montagem escolhe
/// ([`Step::Dispatch`]), sem os itens do projeto todo que a escolha tirou e
/// com os sem dono que ela pôs. A escolha é a de [`choice_for`], e vale só
/// dentro dos grupos de agora: o item que as tarefas da onda passaram a fazer
/// vai sempre.
#[must_use]
pub fn dispatch_items<'a>(log: &'a SpecLog, wave: u64, fresh: Option<&Choice>) -> Vec<&'a SpecEvent> {
    let base = log.step(&Step::Dispatch { wave });
    let Some(choice) = choice_for(log, wave, fresh) else { return base };
    let choice = choice.within(&candidates(log, wave));
    let removed: BTreeSet<u64> = choice.removed.iter().map(|(id, _)| *id).collect();
    let mut out: Vec<&SpecEvent> = base.into_iter().filter(|item| !removed.contains(&item.id)).collect();
    for (id, _) in &choice.added {
        if let Some(item) = log.get(*id).filter(|item| !out.iter().any(|had| had.id == item.id)) {
            out.push(item);
        }
    }
    out.sort_by_key(|item| item.id);
    out
}

/// Os campos que o registro de um lote deriva das tarefas que o compõem
/// agora: os critérios que elas cobrem (`covers`, sem repetir), os títulos
/// delas juntados por ponto e vírgula e o pronto-quando — a prova de cada
/// critério coberto, ligada por " && ", ou, sem prova nenhuma (o caso do
/// item combinado sem dono, que não tem prova), os mesmos títulos. O texto
/// inteiro das tarefas não entra: o pronto-quando abre o pedido da onda, e as
/// tarefas já saem nele com o título e a parte do agente. A formação do lote
/// e a atualização dele depois de uma tarefa sair pelo backlog usam esta
/// mesma conta, sobre as tarefas que a leitura de agora mostra, para as duas
/// nunca discordarem.
pub struct BacklogFields {
    pub criteria: Vec<u64>,
    pub text: String,
    pub done_when: String,
}

/// Calcula [`BacklogFields`] a partir das tarefas `tasks` de um lote, lendo em
/// `log` a prova de cada critério que elas cobrem.
#[must_use]
pub fn backlog_fields(log: &SpecLog, tasks: &[&SpecEvent]) -> BacklogFields {
    let mut criteria: BTreeSet<u64> = BTreeSet::new();
    let mut titles: Vec<String> = Vec::new();
    for task in tasks {
        criteria.extend(task.ints("covers"));
        titles.extend(item_title(task));
    }
    let criteria: Vec<u64> = criteria.into_iter().collect();
    let proof =
        criteria.iter().filter_map(|id| log.get(*id)).filter_map(|event| event.str_field("proof")).collect::<Vec<_>>().join(" && ");
    let text = titles.join("; ");
    let done_when = if proof.is_empty() { text.clone() } else { proof };
    BacklogFields { criteria, text, done_when }
}

/// Os executores que o binário reconhece abrindo a prova de um critério,
/// escritos como uma palavra só. A lista é de ferramenta, não de projeto:
/// qualquer pilha que rode teste aparece aqui, e o programa que só existe num
/// projeto entra pela outra porta, a do nome com caminho, ponto ou hífen.
pub const PROOF_COMMANDS: &[&str] = &[
    "bash", "bun", "bundle", "cabal", "cargo", "cmake", "composer", "ctest", "dart", "deno", "docker", "dotnet",
    "echo", "elixir", "env", "flutter", "git", "go", "gradle", "gradlew", "grep", "jest", "just", "make", "mix",
    "mocha", "mvn", "ninja", "node", "npm", "npx", "php", "phpunit", "pnpm", "poetry", "printf", "pytest", "python",
    "python3", "rake", "rg", "rspec", "rtk", "ruby", "rustc", "sbt", "sh", "stack", "swift", "task", "tox", "tsc",
    "uv", "vitest", "yarn", "zig", "zsh",
];

/// `true` quando `token` é uma atribuição de variável de ambiente à frente do
/// comando, como `PATH="..."` ou `CARGO_TARGET_DIR=/tmp/x`: ela abre a linha
/// sem ser o programa que roda.
fn env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else { return false };
    !name.is_empty()
        && !name.contains('/')
        && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `true` quando `proof` abre por um programa, e não por prosa ou pelo nome
/// solto de um teste: descontadas as atribuições de ambiente da frente, o
/// primeiro pedaço ou está em [`PROOF_COMMANDS`] ou nomeia um programa pelo
/// caminho, pelo ponto ou pelo hífen — que nome de teste e palavra de frase
/// não trazem.
#[must_use]
pub fn proof_is_command(proof: &str) -> bool {
    let Some(program) = proof.split_whitespace().find(|token| !env_assignment(token)) else { return false };
    PROOF_COMMANDS.contains(&program)
        || program.contains('/')
        || program.contains('.')
        || program.contains('-')
}

/// Confere que a prova que a entrega de uma onda traz para o critério
/// `criterion` é uma linha de comando. O campo guarda o comando que demonstra
/// o critério, e é ele que a rodada e o fechamento rodam; duas provas do
/// mesmo critério viram um comando só, ligado por `&&`, e o nome solto de um
/// teste ali dentro vira um comando que o shell não acha, com saída 127 e uma
/// mensagem que não diz de onde veio. A conferência é na gravação, para o
/// defeito aparecer na rodada que o criou.
///
/// # Errors
///
/// [`Refusal::ProofNotACommand`], com o critério e o texto que veio no lugar
/// do comando.
pub fn proof_rule(criterion: &str, proof: &str) -> Result<(), Refusal> {
    if proof_is_command(proof) {
        return Ok(());
    }
    Err(Refusal::ProofNotACommand { criterion: criterion.to_string(), found: proof.to_string() })
}

/// A gravação de um item combinado novo depois da aprovação: ele nasce com
/// dono. Olha o arquivo antes e depois da gravação; o item que já existia, a
/// spec ainda não aprovada e a gravação de outro tipo passam.
///
/// Com o backlog, o número da onda só existe quando o lote sai: o dono se dá
/// pelos arquivos, em `applies_to`, com os arquivos das tarefas que cobrem ou
/// vão cobrir o item. Vale mesmo antes de a tarefa existir: a decisão costuma
/// vir antes da tarefa que a faz. O item dos arquivos vai no pedido da onda
/// cujas tarefas tocam um deles ([`agreed_for`]), sem passar pela análise
/// antes do envio. A onda dita em `waves`, do plano antigo, continua valendo.
///
/// # Errors
///
/// [`Refusal::OwnerMissing`], com o tipo do item novo sem dono.
pub fn owner_rule(before: &SpecLog, after: &SpecLog) -> Result<(), Refusal> {
    if !State::from_log(before).approved {
        return Ok(());
    }
    let had: BTreeSet<u64> = before.events.iter().map(|e| e.id).collect();
    let owners = owners(after);
    let declared = |item: &SpecEvent| item.ints("waves").iter().any(|n| *n > 0) || !applies_to_files(item).is_empty();
    let orphan = |item: &&SpecEvent| !had.contains(&item.id) && !owners.contains_key(&item.id) && !declared(item);
    match agreed_items(after).into_iter().find(orphan) {
        Some(item) => Err(Refusal::OwnerMissing { event_type: item.event_type.clone() }),
        None => Ok(()),
    }
}

/// Os itens combinados que têm dono: os do bloco do combinado que têm texto.
/// O tipo de trabalho e os pontos do levantamento não têm, e não são itens a
/// implementar.
fn agreed_items(log: &SpecLog) -> Vec<&SpecEvent> {
    log.block(BlockQuery::Block(Block::Agreed))
        .into_iter()
        .filter(|e| e.str_field("text").is_some_and(|t| !t.trim().is_empty()))
        .collect()
}

/// Os caminhos que as tarefas de uma onda declaram, em ordem, sem repetir.
#[must_use]
pub fn wave_files(log: &SpecLog, wave: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for task in log.block(BlockQuery::Wave(wave)).iter().filter(|e| e.event_type == "task") {
        let files = task.fields.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        for file in files {
            let path = file.as_str().or_else(|| file.get("path").and_then(Value::as_str)).unwrap_or_default();
            if !path.is_empty() && !out.iter().any(|seen| seen == path) {
                out.push(path.to_string());
            }
        }
    }
    out
}

/// O texto das tarefas de uma onda, uma por linha: a consulta que escolhe as
/// lições que o pedido da onda e o da revisão levam, e os itens sem dono que
/// a onda julga antes do envio ([`candidates`]).
#[must_use]
pub fn tasks_text(log: &SpecLog, wave: u64) -> String {
    log.block(BlockQuery::Wave(wave))
        .iter()
        .filter(|e| e.event_type == "task")
        .filter_map(|task| task.str_field("text"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// A lista dos itens sem dono, para conferir antes de gravar
// ---------------------------------------------------------------------------

/// De onde veio o dono de um item na lista para conferir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerFrom {
    /// As tarefas das ondas do plano que apontam o item, pelo número: as que
    /// nasceram dele e as que citam o código dele no texto.
    Tasks(Vec<u64>),
    /// O texto do item cita as ondas.
    Cited,
    /// As tarefas das ondas mexem nos arquivos que o item cita no texto ou
    /// diz em `applies_to`; aqui, esses arquivos.
    Files(Vec<String>),
    /// O orquestrador deu o dono, com o motivo.
    Orchestrator(String),
    /// Nenhuma regra achou dono.
    Nothing,
}

/// Um item sem dono na lista para conferir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerLine {
    /// O número do item.
    pub item: u64,
    /// O dono que ele recebe; `None` enquanto nenhuma regra nem o
    /// orquestrador o deu.
    pub owner: Option<Owner>,
    pub from: OwnerFrom,
}

/// O dono que o orquestrador dá a um item sem dono, pelo código do item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GivenOwner {
    pub code: String,
    pub owner: Owner,
    /// Por que esse é o dono: vai para a página, ao lado do item.
    pub why: String,
}

/// A proposta de dono de cada item sem dono, na ordem dos números. Três
/// regras, nesta ordem, e vale a primeira que acha onda do plano:
///
/// 1. as tarefas que apontam o item — a que nasceu dele (`origin` numa versão
///    dele) e a que cita o código dele no texto, como as tarefas citavam o
///    que cobriam antes de existir `covers`;
/// 2. as ondas que o texto do item cita, pelo número ("onda 14", "ondas 14,
///    15 e 17") ou pelo código da onda;
/// 3. as ondas cujas tarefas mexem nos arquivos que o item cita no texto ou
///    diz em `applies_to`.
///
/// O item que nenhuma regra resolve fica sem dono, para o orquestrador
/// classificar. A proposta nunca dá o projeto: isso é escolha de quem
/// classifica.
#[must_use]
pub fn propose_owners(log: &SpecLog) -> Vec<OwnerLine> {
    let planned = log.planned_waves();
    let codes = log.codes();
    let tasks: Vec<&SpecEvent> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "task" && e.wave().is_some_and(|n| planned.contains(&n)))
        .collect();
    let wave_codes: BTreeMap<&str, u64> = log
        .block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|e| e.event_type == "wave")
        .filter_map(|e| Some((codes.get(&e.id)?.as_str(), e.wave()?)))
        .collect();
    let files: BTreeMap<u64, Vec<String>> = planned.iter().map(|n| (*n, wave_files(log, *n))).collect();
    unowned(log)
        .into_iter()
        .map(|item| {
            let proposed = by_tasks(log, item, &tasks, codes.get(&item.id))
                .or_else(|| by_cited(item, &wave_codes, &planned))
                .or_else(|| by_files(item, &files));
            match proposed {
                Some((waves, from)) => OwnerLine { item: item.id, owner: Some(Owner::Waves(waves)), from },
                None => OwnerLine { item: item.id, owner: None, from: OwnerFrom::Nothing },
            }
        })
        .collect()
}

/// A lista para conferir: a proposta de cada item sem dono, com o dono que o
/// orquestrador deu no lugar dela quando deu um.
///
/// # Errors
///
/// O código da primeira linha dada que não serve: não é de um item sem dono,
/// o dono é uma onda fora do plano (ou nenhuma), ou falta o motivo.
pub fn owner_list(log: &SpecLog, given: &[GivenOwner]) -> Result<Vec<OwnerLine>, String> {
    let mut lines = propose_owners(log);
    let codes = log.codes();
    let planned = log.planned_waves();
    for entry in given {
        let valid = !entry.why.trim().is_empty()
            && match &entry.owner {
                Owner::Project => true,
                Owner::Waves(waves) => !waves.is_empty() && waves.is_subset(&planned),
                Owner::Files(files) => files.iter().any(|f| !f.trim().is_empty()),
            };
        let line = lines.iter_mut().find(|line| codes.get(&line.item) == Some(&entry.code));
        match line {
            Some(line) if valid => {
                line.owner = Some(entry.owner.clone());
                line.from = OwnerFrom::Orchestrator(entry.why.trim().to_string());
            }
            _ => return Err(entry.code.clone()),
        }
    }
    Ok(lines)
}

/// A primeira regra: as ondas das tarefas que nasceram do item ou citam o
/// código dele.
fn by_tasks(
    log: &SpecLog,
    item: &SpecEvent,
    tasks: &[&SpecEvent],
    code: Option<&String>,
) -> Option<(BTreeSet<u64>, OwnerFrom)> {
    let mut versions: BTreeSet<u64> = BTreeSet::from([item.id]);
    let mut at = item;
    while let Some(old) = at.int("replaces").and_then(|id| log.get(id)) {
        if !versions.insert(old.id) {
            break;
        }
        at = old;
    }
    let cites = |task: &SpecEvent| {
        let text = task.str_field("text").unwrap_or_default();
        code.is_some_and(|code| mustard_id::find(text).into_iter().any(|(start, end)| &text[start..end] == code))
    };
    let hits: Vec<&SpecEvent> = tasks
        .iter()
        .copied()
        .filter(|task| task.int("origin").is_some_and(|origin| versions.contains(&origin)) || cites(task))
        .collect();
    let waves: BTreeSet<u64> = hits.iter().filter_map(|task| task.wave()).collect();
    (!waves.is_empty()).then(|| (waves, OwnerFrom::Tasks(hits.iter().map(|task| task.id).collect())))
}

/// A segunda regra: as ondas do plano que o texto, o rótulo ou as
/// palavras-chave do item citam.
fn by_cited(
    item: &SpecEvent,
    wave_codes: &BTreeMap<&str, u64>,
    planned: &BTreeSet<u64>,
) -> Option<(BTreeSet<u64>, OwnerFrom)> {
    let mut texts: Vec<&str> = [item.str_field("text"), item.str_field("label")].into_iter().flatten().collect();
    if let Some(keys) = item.fields.get("keys").and_then(Value::as_array) {
        texts.extend(keys.iter().filter_map(Value::as_str));
    }
    let waves: BTreeSet<u64> =
        texts.into_iter().flat_map(|text| cited_waves(text, wave_codes)).filter(|n| planned.contains(n)).collect();
    (!waves.is_empty()).then_some((waves, OwnerFrom::Cited))
}

/// As ondas que um texto cita: o número logo depois da palavra "onda" (ou
/// "wave"), os números da lista logo depois de "ondas" ("ondas 14, 15 e 17")
/// e o código de uma onda. O número separado da palavra por pontuação
/// ("ondas. (4") e o código de outro item não contam.
fn cited_waves(text: &str, wave_codes: &BTreeMap<&str, u64>) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    let mut plain = String::with_capacity(text.len());
    let mut from = 0;
    for (start, end) in mustard_id::find(text) {
        plain.push_str(&text[from..start]);
        plain.push(' ');
        out.extend(wave_codes.get(&text[start..end]));
        from = end;
    }
    plain.push_str(&text[from..]);
    let singular: Vec<String> =
        [Locale::PtBr, Locale::EnUs].iter().map(|lang| translate("page.type.wave", *lang).to_lowercase()).collect();
    let words: Vec<String> = plain.split_whitespace().map(str::to_lowercase).collect();
    for (i, word) in words.iter().enumerate() {
        let bare = word.trim_start_matches(|c: char| !c.is_alphanumeric());
        let one = singular.iter().any(|s| s == bare);
        let many = singular.iter().any(|s| bare.strip_suffix('s') == Some(s.as_str()));
        if !one && !many {
            continue;
        }
        for next in &words[i + 1..] {
            let digits: String = next.chars().take_while(char::is_ascii_digit).collect();
            let Ok(n) = digits.parse::<u64>() else {
                if many && matches!(next.as_str(), "e" | "and") {
                    continue;
                }
                break;
            };
            out.insert(n);
            let rest = &next[digits.len()..];
            if one || !(rest.is_empty() || rest == ",") {
                break;
            }
        }
    }
    out
}

/// A terceira regra: as ondas cujas tarefas mexem nos arquivos que o item
/// cita no texto ou diz em `applies_to`. A pasta citada no texto não casa
/// com arquivo nenhum: uma pasta como `.claude/` casaria com quase toda onda.
fn by_files(item: &SpecEvent, files: &BTreeMap<u64, Vec<String>>) -> Option<(BTreeSet<u64>, OwnerFrom)> {
    let cited = cited_paths(item.str_field("text").unwrap_or_default());
    let declared: Vec<String> = item
        .fields
        .get("applies_to")
        .and_then(|at| at.get("files"))
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let mut waves = BTreeSet::new();
    let mut shared: BTreeSet<String> = BTreeSet::new();
    for (n, wave) in files {
        let mut hit = false;
        for path in &cited {
            if wave.iter().any(|file| same_file(path, file)) {
                shared.insert(path.clone());
                hit = true;
            }
        }
        if applies_to(item, &Scope { files: wave.clone(), ..Scope::default() }) {
            shared.extend(declared.iter().cloned());
            hit = true;
        }
        if hit {
            waves.insert(*n);
        }
    }
    (!waves.is_empty()).then(|| (waves, OwnerFrom::Files(shared.into_iter().collect())))
}

// ---------------------------------------------------------------------------
// O conserto
// ---------------------------------------------------------------------------

/// As linhas do conserto da onda `wave`, quando a última revisão dela
/// reprovou: o veredito que reprovou, a entrega anterior a ele e os itens
/// combinados do pedido da onda gravados depois do último envio anterior ao
/// veredito — ou, sem envio, depois daquela entrega. A onda que não está em
/// conserto não tem linha nenhuma.
///
/// São as mesmas linhas no pedido do conserto e na revisão dele: o conserto
/// ganha um envio novo, e a âncora continua a do envio que a reprovação
/// julgou.
#[must_use]
pub fn fix_lines(log: &SpecLog, wave: u64) -> Vec<&SpecEvent> {
    let Some(verdict) = log
        .verdicts_by_wave()
        .remove(&wave)
        .and_then(|verdicts| verdicts.last().copied())
        .filter(|v| v.str_field("result") == Some("rejected"))
    else {
        return Vec::new();
    };
    let own = log.block(BlockQuery::Wave(wave));
    let last_before = |event_type: &str| {
        own.iter().copied().rfind(|e| e.event_type == event_type && e.id < verdict.id)
    };
    let delivered = last_before("delivered");
    let anchor = last_before("send").or(delivered).map(|e| e.id);
    let mut out = vec![verdict];
    out.extend(delivered);
    if let Some(anchor) = anchor {
        out.extend(agreed_for(log, wave).into_iter().filter(|item| item.id > anchor));
    }
    out
}

// ---------------------------------------------------------------------------
// A revisão de volta
// ---------------------------------------------------------------------------

/// O veredito final mais novo da spec, quando ele reprovou: a revisão pedida
/// depois dele é a de volta. `None` sem veredito final, ou quando o mais novo
/// aprovou — aí a revisão confere a obra inteira, como a primeira.
#[must_use]
pub fn last_final_rejection(log: &SpecLog) -> Option<&SpecEvent> {
    log.block(BlockQuery::Block(Block::Review))
        .into_iter()
        .filter(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&Value::Bool(true)))
        .max_by_key(|e| e.id)
        .filter(|e| e.str_field("result").map(str::trim) == Some("rejected"))
}

/// O que mudou desde o veredito final que reprovou ([`last_final_rejection`]),
/// na ordem em que o pedido da revisão de volta o lista: o próprio veredito,
/// os commits gravados depois dele, os requisitos acordados gravados ou
/// regravados depois dele, junto com os que ele deu como não atendidos — na
/// versão vigente de cada um —, e os critérios gravados ou regravados depois
/// dele. O que veio antes do veredito fica de fora: a conclusão dele vale
/// para o que não mudou. Vazio sem veredito final reprovado.
#[must_use]
pub fn since_last_verdict(log: &SpecLog) -> Vec<&SpecEvent> {
    let Some(verdict) = last_final_rejection(log) else {
        return Vec::new();
    };
    let codes = log.codes();
    // O item da resposta vem pelo número, como a rodada o grava, ou pelo
    // código, como o revisor o escreve.
    let item_id = |reference: &Value| match reference {
        Value::Number(n) => n.as_u64(),
        Value::String(code) => {
            let code = code.trim();
            code.parse().ok().or_else(|| codes.iter().filter(|(_, c)| c.as_str() == code).map(|(id, _)| *id).max())
        }
        _ => None,
    };
    let unmet: BTreeSet<u64> = verdict
        .fields
        .get("agreed")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|answer| answer.get("met").and_then(Value::as_bool) != Some(true))
        .filter_map(|answer| answer.get("item").and_then(item_id))
        .filter_map(|id| log.current(id).map(|e| e.id))
        .collect();
    let mut out = vec![verdict];
    out.extend(
        log.block(BlockQuery::Block(Block::Progress))
            .into_iter()
            .filter(|e| e.event_type == "commit" && e.id > verdict.id),
    );
    let mut agreed: Vec<&SpecEvent> =
        all_agreed(log).into_iter().filter(|e| e.id > verdict.id || unmet.contains(&e.id)).collect();
    agreed.sort_by_key(|e| e.id);
    out.extend(agreed);
    out.extend(
        log.block(BlockQuery::Block(Block::Criteria))
            .into_iter()
            .filter(|e| e.event_type == "criterion" && e.id > verdict.id),
    );
    out
}

struct Writer<'a> {
    material: &'a Material<'a>,
    lang: Locale,
}

impl Writer<'_> {
    fn t(&self, key: &str) -> &'static str {
        translate(key, self.lang)
    }

    fn text(&self) -> String {
        let m = self.material;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# {}\n",
            self.t("prompt.title").replace("{spec}", &m.spec).replace("{n}", &m.wave.to_string())
        );
        let _ = writeln!(out, "{}\n", self.t("prompt.model.wave"));
        out.push_str(self.t("prompt.fixed"));
        out.push_str("\n\n");
        self.read_example(&mut out, "prompt.read.wave", m.execution.copy.is_some());
        self.delivers(&mut out);
        self.items(&mut out);
        self.tasks(&mut out);
        self.part(&mut out, "prompt.part.delivered", &m.delivered);
        self.execution(&mut out);
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }

    /// O pedido do agente de teste dedicado, que o fechamento pede a toda
    /// obra: as instruções fixas dele, o que olhar — a obra inteira na
    /// primeira revisão; na de volta, o que mudou desde o veredito que
    /// reprovou e o encaixe disso no resto —, o exemplo de leitura, o que
    /// mudou, as ondas com as tarefas, as emendas gravadas para elas, o que
    /// cada uma entregou, os critérios, os commits que já entraram na branch
    /// e como revisar numa cópia separada. O veredito que reprovou aparece
    /// uma vez só, na parte do que mudou.
    fn final_review_text(&self) -> String {
        let m = self.material;
        let mut out = String::new();
        let _ = writeln!(out, "# {}\n", self.t("prompt.final.title").replace("{spec}", &m.spec));
        out.push_str(self.t("prompt.final.fixed"));
        out.push_str("\n\n");
        let look = if m.since_verdict.is_empty() { "prompt.final.look" } else { "prompt.final.look_again" };
        out.push_str(self.t(look));
        out.push_str("\n\n");
        self.read_example(&mut out, "prompt.read", true);
        self.part(&mut out, "prompt.part.since_verdict", &m.since_verdict);
        self.part(&mut out, "prompt.part.waves", &m.block);
        self.part(&mut out, "prompt.part.agreed", &m.agreed);
        self.part(&mut out, "prompt.part.each_delivered", &m.own_delivered);
        self.part(&mut out, "prompt.part.criteria", &m.criteria);
        self.part(&mut out, "prompt.part.branch_changes", &m.changes);
        self.review_execution(&mut out);
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }

    /// Os itens da onda na ordem de execução que ela declara: primeiro os que
    /// a onda lista, na ordem em que ela os lista, depois o que sobrou, na
    /// ordem do arquivo. A onda que não declara ordem sai como está no
    /// arquivo.
    fn wave_items(&self) -> Vec<&SpecEvent> {
        let order: Vec<u64> = self
            .material
            .block
            .iter()
            .find(|e| e.event_type == "wave")
            .map(|wave| wave.ints("order"))
            .unwrap_or_default();
        let mut out: Vec<&SpecEvent> = Vec::new();
        for id in &order {
            if let Some(event) = self.material.block.iter().copied().find(|e| e.id == *id) {
                out.push(event);
            }
        }
        for event in &self.material.block {
            if !out.iter().any(|had| had.id == event.id) {
                out.push(event);
            }
        }
        out
    }

    /// O evento da própria onda, sozinho — as tarefas saem por [`Self::tasks`].
    fn wave_only(&self) -> Vec<&SpecEvent> {
        self.material.block.iter().copied().filter(|e| e.event_type == "wave").take(1).collect()
    }

    /// O que a onda entrega: a frase do `done_when` do evento da onda, como
    /// foi gravada — a prova dos critérios ou, sem prova, os títulos das
    /// tarefas ([`backlog_fields`]) —, porque é o que abre o trabalho. Vem
    /// como parágrafo de abertura, sem título próprio, antes dos itens da
    /// onda. Sem onda no material, nada.
    fn delivers(&self, out: &mut String) {
        let Some(wave) = self.material.block.iter().copied().find(|e| e.event_type == "wave") else { return };
        let done_when = wave.str_field("done_when").unwrap_or_default().trim();
        if done_when.is_empty() {
            return;
        }
        let _ = writeln!(out, "{done_when}\n");
    }

    /// Os itens da onda, todos sob um único título: o conserto pendente
    /// (quando a onda volta reprovada), a própria onda, os critérios, a
    /// especificação, o combinado, as lições e as skills que as tarefas
    /// nomeiam. Cada item sai numa linha, com o bloco, o código e o título
    /// ([`item_line`]), e logo abaixo dela a parte do agente ([`agent_part`]);
    /// lições e skills mantêm a listagem própria. O porquê do item fica na
    /// spec, e o comando de leitura aparece uma vez só no pedido, em
    /// [`Self::read_example`].
    fn items(&self, out: &mut String) {
        let m = self.material;
        let wave_only = self.wave_only();
        let has_content = !m.fix.is_empty()
            || !wave_only.is_empty()
            || !m.criteria.is_empty()
            || !m.specification.is_empty()
            || !m.agreed.is_empty()
            || !m.lessons.is_empty()
            || !m.skills.is_empty();
        if !has_content {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.items"));
        if !m.fix.is_empty() {
            let _ = writeln!(out, "{}\n", self.t("prompt.fix.wave"));
        }
        for group in [&m.fix, &wave_only, &m.criteria, &m.specification, &m.agreed] {
            for item in group {
                let _ = writeln!(out, "{}", item_line(m, item));
                agent_part(out, item);
            }
        }
        for lesson in &m.lessons {
            let _ = writeln!(out, "- `lessons`: {}", lesson.id);
        }
        if !m.skills.is_empty() {
            let _ = writeln!(out, "{}", self.t("prompt.skill.read"));
            for skill in &m.skills {
                let _ = write!(out, "- **{}**", skill.name);
                if skill.stale {
                    let _ = write!(out, " ({})", self.t("prompt.skill.stale"));
                }
                if !skill.when.trim().is_empty() {
                    let _ = write!(out, " — {}", skill.when.trim());
                }
                let _ = writeln!(out, " — `{}`", skill.path);
            }
        }
        out.push('\n');
    }

    /// As tarefas da onda, na ordem de execução dela ([`Self::wave_items`]):
    /// uma linha por tarefa, com o código, o título ([`item_title`]), os
    /// arquivos que ela cita e — para a que ganhou leitura obrigatória ou
    /// escolhida pelo orquestrador — o que precisa ler antes, pelo mesmo
    /// trecho que [`Self::read_hint`] calcula. Abaixo da linha vem a parte do
    /// agente ([`agent_part`]), como nos itens; o porquê fica na spec.
    /// Depois, um arquivo que o mapa do projeto conhece os testes
    /// ([`Material::file_tests`]) ganha uma linha própria com eles.
    fn tasks(&self, out: &mut String) {
        let tasks: Vec<&SpecEvent> = self.wave_items().into_iter().filter(|e| e.event_type == "task").collect();
        if tasks.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.tasks"));
        for task in tasks {
            let code = self.material.codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
            let paths: Vec<&str> = task
                .fields
                .get("files")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|file| file.get("path").and_then(Value::as_str))
                .collect();
            let files: Vec<String> = paths.iter().map(|path| format!("`{path}`")).collect();
            let _ = write!(out, "- `{code}`");
            if let Some(title) = item_title(task) {
                let _ = write!(out, " {title}");
            }
            if !files.is_empty() {
                let _ = write!(out, ": {}", files.join(", "));
            }
            if let Some((_, reads)) = self.material.task_reads.iter().find(|(t, _)| *t == code) {
                let parts: Vec<String> = reads.iter().map(|file| self.read_hint(file)).collect();
                let _ = write!(out, " — {}: {}", self.t("prompt.task.read_before"), parts.join(", "));
            }
            let _ = writeln!(out);
            agent_part(out, task);
            for path in paths {
                let Some(tests) = self.material.file_tests.get(path) else { continue };
                let list = tests.iter().map(|test| format!("`{test}`")).collect::<Vec<_>>().join(", ");
                let line = self.t("prompt.task.tested_by").replace("{file}", path).replace("{tests}", &list);
                let _ = writeln!(out, "  - {line}");
            }
        }
        out.push('\n');
    }

    /// Como ler, pela chave `key`, com o nome da spec e o número da onda: o
    /// pedido de uma onda diz que, em caso de dúvida, um comando só lê o
    /// texto completo de tudo o que ele lista, e dá o que lê um item pelo
    /// código; o da revisão final lê cada item pelo código. Leva o caminho do repositório
    /// principal quando o agente trabalha numa cópia (`in_copy`): de dentro
    /// dela, a spec só se lê por lá. Sem cópia, o agente roda no próprio
    /// repositório principal, e o caminho sobra.
    fn read_example(&self, out: &mut String, key: &str, in_copy: bool) {
        let root = &self.material.execution.root;
        let flag = if in_copy && !root.is_empty() { format!("--root {root} ") } else { String::new() };
        let line = self
            .t(key)
            .replace("{root}", &flag)
            .replace("{spec}", &self.material.spec)
            .replace("{n}", &self.material.wave.to_string());
        let _ = writeln!(out, "{line}\n");
    }

    /// Uma parte do pedido: o título e uma linha por bloco da spec, com os
    /// códigos dos itens em sequência. A parte sem nenhum item não aparece.
    fn part(&self, out: &mut String, key: &str, events: &[&SpecEvent]) {
        if events.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t(key));
        for line in codes_by_block(self.material, events) {
            let _ = writeln!(out, "{line}");
        }
        out.push('\n');
    }

    /// As regras da execução do agente da onda que carregam valor deste
    /// projeto e desta rodada — a cópia separada, o preparo que o projeto
    /// declara, os comandos do projeto e as outras ondas em andamento,
    /// com os arquivos delas — e, ligadas à cópia, as três frases que dizem
    /// com todas as letras que o agente não comita, que o campo `commit` da
    /// entrega é o título, nunca o código do commit, e que a entrega vai para
    /// a spec por `run write delivered`, sem a rodada ler a última mensagem.
    /// O resto (ler por
    /// trecho, a suíte uma vez no fim…) já mora no molde do agente, e não
    /// repete aqui. De onde ler a spec, o exemplo de leitura já diz.
    fn execution(&self, out: &mut String) {
        let execution = &self.material.execution;
        let running = &execution.running;
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.execution"));
        if let Some(copy) = &execution.copy {
            let line = self.t("prompt.execution.copy").replace("{copy}", &copy.path).replace("{root}", &execution.root);
            let _ = writeln!(out, "- {line}");
            self.prepare(out, Some(copy));
            let _ = writeln!(out, "- {}", self.t("prompt.execution.no_commit"));
            let _ = writeln!(out, "- {}", self.t("prompt.execution.commit_field"));
            let _ = writeln!(out, "- {}", self.t("prompt.execution.report_lines"));
        }
        self.commands(out);
        if !running.is_empty() {
            let _ = writeln!(out, "- {}", self.t("prompt.execution.running"));
        }
        for (wave, files) in running {
            let name = self.t("prompt.execution.wave").replace("{n}", &wave.to_string());
            let files: Vec<String> = files.iter().map(|file| format!("`{file}`")).collect();
            if files.is_empty() {
                let _ = writeln!(out, "  - {name}");
            } else {
                let _ = writeln!(out, "  - {name}: {}", files.join(", "));
            }
        }
        out.push('\n');
    }

    /// As regras da execução do revisor: a cópia que o fechamento já criou no
    /// commit da obra, o preparo que o projeto declara, os arquivos locais
    /// que a cópia recebe pelo conteúdo, o comando de compilar, a suíte que o
    /// fechamento já rodou nesse commit — no lugar da ordem de rodá-la, só os
    /// testes em volta de cada corte —, menos processos, e desfazer cada corte
    /// antes de gravar o veredito — quem apaga a cópia é o fechamento. De onde
    /// ler a spec, o exemplo de leitura já diz.
    fn review_execution(&self, out: &mut String) {
        let execution = &self.material.execution;
        let (copy, root) = (execution.copy.clone().unwrap_or_default(), &execution.root);
        let commit = execution.commit.as_deref().unwrap_or("HEAD");
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.execution"));
        let line = self.t("prompt.review.copy").replace("{copy}", &copy.path).replace("{root}", root);
        let _ = writeln!(out, "- {}", line.replace("{commit}", commit));
        self.prepare(out, None);
        if !execution.local_files.is_empty() {
            let files: Vec<String> = execution.local_files.iter().map(|file| format!("`{file}`")).collect();
            let line = self.t("prompt.review.local_files").replace("{files}", &files.join(", ")).replace("{root}", root);
            let _ = writeln!(out, "- {line}");
        }
        self.build_line(out);
        if let Some(command) = &execution.test {
            let line = self.t("prompt.review.suite").replace("{command}", command).replace("{commit}", commit);
            let _ = writeln!(out, "- {line}");
        }
        let _ = writeln!(out, "- {}", self.t("prompt.review.jobs"));
        let _ = writeln!(out, "- {}", self.t("prompt.review.cleanup").replace("{copy}", &copy.path));
        out.push('\n');
    }

    /// O preparo que o projeto declara, rodado dentro da cópia antes de
    /// compilar, com o arquivo versionado que ele mudar de volta ao commit;
    /// sem comando declarado, nada. O revisor (`copy` sem valor) sempre
    /// prepara. Na onda, a cópia nova também. A reaproveitada já guarda o
    /// preparo anterior: o pedido lista os arquivos que mudaram desde o
    /// último uso dela e manda preparar só se um deles declara dependências.
    /// Quem julga é o agente, pela lista: o binário não sabe quais arquivos
    /// de cada linguagem declaram dependências.
    fn prepare(&self, out: &mut String, copy: Option<&WaveCopy>) {
        let Some(command) = &self.material.execution.prepare else { return };
        let Some(copy) = copy else {
            let _ = writeln!(out, "- {}", self.t("prompt.execution.prepare").replace("{command}", command));
            return;
        };
        let line = match &copy.reused {
            None => self.t("prompt.execution.prepare_new").replace("{command}", command),
            Some(reuse) if reuse.changed.is_empty() => {
                self.t("prompt.execution.prepare_same").replace("{command}", command)
            }
            Some(reuse) => {
                let mut files: Vec<String> =
                    reuse.changed.iter().take(REUSED_FILES_SHOWN).map(|file| format!("`{file}`")).collect();
                let left = reuse.changed.len().saturating_sub(REUSED_FILES_SHOWN);
                if left > 0 {
                    let diff = format!("git diff --name-only {} HEAD", reuse.since);
                    files.push(
                        self.t("prompt.execution.prepare_more").replace("{n}", &left.to_string()).replace("{diff}", &diff),
                    );
                }
                self.t("prompt.execution.prepare_reused")
                    .replace("{command}", command)
                    .replace("{files}", &files.join(", "))
            }
        };
        let _ = writeln!(out, "- {line}");
    }

    /// Os comandos de compilar e de testar que o projeto declara, um por
    /// linha; o que ele não declara não aparece.
    fn commands(&self, out: &mut String) {
        self.build_line(out);
        if let Some(command) = &self.material.execution.test {
            let _ = writeln!(out, "- {}", self.t("prompt.execution.test").replace("{command}", command));
        }
    }

    /// O comando de compilar que o projeto declara; sem ele, nada.
    fn build_line(&self, out: &mut String) {
        if let Some(command) = &self.material.execution.build {
            let _ = writeln!(out, "- {}", self.t("prompt.execution.build").replace("{command}", command));
        }
    }


    /// Um arquivo da leitura por tarefa: `caminho#declaração` manda ler só
    /// aquela declaração, função, estrutura ou constante — nunca chamada de
    /// função quando não é; `caminho#declaração@início-fim[,início-fim…]` —
    /// que [`crate::io::wave_prompt`] monta quando o mapa do projeto conhece
    /// a declaração e a linha em que ela termina — manda ler só essas
    /// linhas, uma faixa por trecho, para o nome que se repete no arquivo;
    /// um caminho sozinho é o arquivo, entre crases, como antes.
    fn read_hint(&self, file: &str) -> String {
        let Some((path, rest)) = file.split_once('#') else { return format!("`{file}`") };
        if path.is_empty() || rest.is_empty() {
            return format!("`{file}`");
        }
        match rest.split_once('@') {
            Some((function, lines)) if !function.is_empty() && !lines.is_empty() => self
                .t("prompt.task_read.function_lines")
                .replace("{function}", function)
                .replace("{path}", path)
                .replace("{lines}", lines),
            _ => self.t("prompt.task_read.function").replace("{function}", rest).replace("{path}", path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{parse_log, render_line, stamp, BlockQuery, SpecLog, Step};
    use serde_json::{json, Value};

    /// Um arquivo de eventos escrito à mão, uma linha por evento.
    fn log(events: &[(&str, Value)]) -> SpecLog {
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let id = i as u64 + 1;
            let mut map = crate::domain::spec_events::normalize(
                body.as_object().cloned().unwrap_or_default(),
                event_type,
            );
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, id, None, "2026-09-15T10:00:00-03:00")));
            content.push('\n');
        }
        parse_log(&content)
    }

    fn material(log: &SpecLog, wave: u64) -> Material<'_> {
        let block = log.block(BlockQuery::Wave(wave));
        let criteria: Vec<&SpecEvent> = log
            .step(&Step::Dispatch { wave })
            .into_iter()
            .filter(|e| e.event_type == "criterion")
            .collect();
        Material {
            spec: "teste".into(),
            wave,
            block,
            criteria,
            codes: log.codes(),
            ..Material::default()
        }
    }

    /// A leitura de um envio antigo, gravado antes de a lição entrar na
    /// escolha: o campo `analysis` dele não tem `judged_lessons` nem
    /// `removed_lessons`, e a leitura vale mesmo assim, com as duas listas de
    /// lição vazias — não é lido como envio quebrado, e a onda não pede a
    /// escolha de novo só por causa do formato antigo.
    #[test]
    fn from_value_reads_an_old_send_without_the_lesson_fields() {
        let old = json!({
            "judged": [1, 2],
            "removed": [{"item": 2, "why": "Fala de outra coisa."}],
            "added": [{"item": 3, "why": "Vale para esta onda."}],
        });
        let choice = Choice::from_value(&old).expect("o envio antigo se lê");
        assert_eq!(choice.judged, BTreeSet::from([1, 2]), "{choice:?}");
        assert_eq!(choice.removed, vec![(2, "Fala de outra coisa.".to_string())], "{choice:?}");
        assert_eq!(choice.added, vec![(3, "Vale para esta onda.".to_string())], "{choice:?}");
        assert!(choice.judged_lessons.is_empty(), "sem o campo, nenhuma lição julgada: {choice:?}");
        assert!(choice.removed_lessons.is_empty(), "sem o campo, nenhuma lição tirada: {choice:?}");
        assert!(choice.tasks.is_empty(), "{choice:?}");
    }

    /// O pedido leva, de cada item, o código e o título, numa linha com o
    /// bloco dele; de cada tarefa, também a parte do agente, embaixo da linha
    /// dela e recuada dois espaços. A onda e o critério, que não têm essa
    /// parte, ficam numa linha só. O porquê da tarefa, o resto do texto da
    /// onda e o `when`, o `then` e a prova do critério não são copiados; o
    /// comando de leitura aparece uma vez só, no exemplo, e nenhum código
    /// aparece duas vezes.
    #[test]
    fn a_request_lists_each_item_by_code_and_title_and_each_task_with_its_agent_part() {
        let log = log(&[
            (
                "criterion",
                json!({"title": "A suíte roda inteira", "when": "a onda roda", "then": "a suíte passa",
                       "proof": "cargo test -p suite"}),
            ),
            (
                "wave",
                json!({"n": 1, "text": "Primeira onda. Ela abre o motor e a página.", "criteria": [1],
                       "done_when": "a onda termina"}),
            ),
            (
                "task",
                json!({"wave": 1, "title": "Escrever o motor", "text": "Hoje não há motor. Depois, ele soma.",
                       "agent": "- src/a.rs: `soma`\n  - teste: `cargo test -p motor`", "files": [{"path": "src/a.rs"}]}),
            ),
            ("task", json!({"wave": 1, "text": "Escrever a página. Ela mostra a soma.", "files": [{"path": "src/b.rs"}]})),
        ]);
        for lang in [Locale::PtBr, Locale::EnUs] {
            let prompt = build(&material(&log, 1), lang);
            for text in ["Ela abre o motor", "Hoje não há motor", "Depois, ele soma", "a suíte passa", "-p suite"] {
                assert!(!prompt.text.contains(text), "{text:?} foi copiado: {}", prompt.text);
            }
            assert!(prompt.text.contains("a onda termina"), "o done_when abre o pedido: {}", prompt.text);
            // A onda e o critério não têm parte do agente: cada um fica numa
            // linha só.
            assert_eq!(
                section(&prompt.text, translate("prompt.part.items", lang)).lines().collect::<Vec<_>>(),
                ["", "- `waves` MSTD-WAVE-0001: Primeira onda.", "- `criteria` MSTD-CRIT-0001: A suíte roda inteira"],
                "{}",
                prompt.text
            );
            let tasks: Vec<&str> = section(&prompt.text, translate("prompt.part.tasks", lang)).lines().collect();
            assert_eq!(
                tasks,
                [
                    "",
                    "- `MSTD-TASK-0001` Escrever o motor: `src/a.rs`",
                    "  - src/a.rs: `soma`",
                    "    - teste: `cargo test -p motor`",
                    "- `MSTD-TASK-0002` Escrever a página.: `src/b.rs`",
                    "  Escrever a página. Ela mostra a soma.",
                ],
                "{}",
                prompt.text
            );
            let example =
                translate("prompt.read.wave", lang).replace("{root}", "").replace("{spec}", "teste").replace("{n}", "1");
            assert!(prompt.text.contains(&example), "{}", prompt.text);
            // O comando que lê o pedido inteiro e o que lê um item pelo
            // código, os dois só na linha de como ler.
            assert_eq!(prompt.text.matches("mustard-rt run read").count(), 2, "{}", prompt.text);
            assert_eq!(prompt.text.matches("--term").count(), 1, "{}", prompt.text);
            for code in ["MSTD-WAVE-0001", "MSTD-TASK-0001", "MSTD-TASK-0002", "MSTD-CRIT-0001"] {
                assert_eq!(prompt.text.matches(code).count(), 1, "{code}: {}", prompt.text);
            }
            assert!(prompt.lines > 0 && prompt.lines == prompt.text.lines().count());
        }
    }

    /// O registro de um lote tira das tarefas o título, nunca o texto
    /// inteiro. Sem prova nos critérios cobertos, o pronto-quando — que abre
    /// o pedido da onda — são os títulos, os mesmos do texto do lote; a
    /// tarefa gravada antes do título entra pela primeira frase. Com prova, o
    /// pronto-quando é a prova, e o texto continua sendo os títulos.
    #[test]
    fn a_backlog_lot_takes_the_task_titles_never_their_whole_text() {
        let log = log(&[
            ("criterion", json!({"title": "A soma passa", "when": "a soma roda", "then": "passa", "proof": "cargo test -p soma"})),
            ("task", json!({"title": "Somar o total", "text": "Hoje o total não soma. Depois, soma.", "agent": "- src/a.rs",
                            "files": [{"path": "src/a.rs"}]})),
            ("task", json!({"text": "Mostrar o total na página. Ela lê a soma.", "files": [{"path": "src/b.rs"}]})),
            ("task", json!({"title": "Conferir a soma", "text": "A soma precisa de teste.", "agent": "- src/c.rs",
                            "files": [{"path": "src/c.rs"}], "covers": [1]})),
        ]);
        let visible = log.visible();
        let (titled, old, covering) = (visible[1], visible[2], visible[3]);

        let unproven = backlog_fields(&log, &[titled, old]);
        assert!(unproven.criteria.is_empty());
        assert_eq!(unproven.text, "Somar o total; Mostrar o total na página.");
        assert_eq!(unproven.done_when, unproven.text, "sem prova, o pronto-quando são os títulos");

        let proven = backlog_fields(&log, &[covering]);
        assert_eq!(proven.criteria, [1]);
        assert_eq!(proven.text, "Conferir a soma");
        assert_eq!(proven.done_when, "cargo test -p soma");

        for fields in [&unproven, &proven] {
            for whole in ["Hoje o total", "Ela lê a soma", "precisa de teste"] {
                assert!(!fields.text.contains(whole) && !fields.done_when.contains(whole), "{whole}");
            }
        }
    }

    /// O critério da montagem, provado de uma vez, não espalhado: o pedido
    /// abre pelo `done_when`, antes das tarefas; as tarefas saem na ordem de
    /// execução que a onda declara (`order`); o cabeçalho diz o modelo da
    /// onda; nenhuma das seis frases de execução que o molde do agente já dá
    /// aparece; e o que sobra da execução é só o desta rodada e deste
    /// projeto — a cópia, os comandos do projeto e a
    /// onda que corre junto.
    #[test]
    fn the_request_opens_by_done_when_states_the_model_and_keeps_only_this_projects_execution() {
        let log = log(&[
            ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "a suíte passa",
                            "order": [3, 2]})),
            ("task", json!({"wave": 1, "text": "Primeiro passo", "files": [{"path": "src/a.rs"}]})),
            ("task", json!({"wave": 1, "text": "Segundo passo", "files": [{"path": "src/b.rs"}]})),
        ]);
        let mut m = material(&log, 1);
        m.execution = Execution {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            root: "/repo".into(),
            running: vec![(9, vec!["src/c.rs".into()])],
            copy: Some(WaveCopy { path: "/copia".into(), reused: None }),
            ..Execution::default()
        };
        let prompt = build(&m, Locale::PtBr);
        let text = &prompt.text;

        // Abre pelo que a onda entrega, antes das tarefas.
        let delivers_at = text.find("a suíte passa").expect("o done_when abre o pedido");
        let tasks_at = text.find(translate("prompt.part.tasks", Locale::PtBr)).expect("as tarefas aparecem");
        assert!(delivers_at < tasks_at, "{text}");

        // Diz o modelo da onda.
        assert!(text.contains(translate("prompt.model.wave", Locale::PtBr)), "{text}");

        // A onda declara `order: [3, 2]`: a tarefa 2 (id 3) vem antes da 1
        // (id 2).
        let second = text.find("MSTD-TASK-0002").expect("a segunda tarefa aparece");
        let first = text.find("MSTD-TASK-0001").expect("a primeira tarefa aparece");
        assert!(second < first, "a ordem da onda não foi respeitada: {text}");

        // Nenhuma das frases que o molde do agente já dá volta a aparecer.
        for phrase in [
            "Não comite e não use `git add`: o commit é da rodada.",
            "Leia por trecho: ache a função",
            "Não releia o arquivo depois de editar",
            "Durante o trabalho, rode só os testes do que mudou.",
            "A suíte inteira roda uma vez no fim, em primeiro plano",
            "Nunca mande compilação ou teste para segundo plano",
        ] {
            assert!(!text.contains(phrase), "{phrase:?} devia ter saído do pedido: {text}");
        }

        // O que sobra da execução é só o desta rodada e deste projeto.
        for kept in ["/copia", "cargo build", "cargo test", "Onda 9", "`src/c.rs`"] {
            assert!(text.contains(kept), "{kept:?} devia continuar no pedido: {text}");
        }
    }

    /// Os itens de uma parte que vêm de blocos diferentes saem numa linha
    /// por bloco, na ordem em que o primeiro de cada um aparece, mesmo quando
    /// os blocos se alternam; a parte com itens de um bloco só tem uma linha.
    #[test]
    fn items_of_two_blocks_in_one_part_come_in_one_line_per_block() {
        let log = log(&[
            ("decision", json!({"text": "Uma decisão", "keys": ["d"], "why": "w"})),
            ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"})),
            ("rule", json!({"text": "Uma regra", "keys": ["r"], "example": "e"})),
            ("delivered", json!({"wave": 1, "text": "Feito", "files": ["src/a.rs"]})),
        ]);
        let visible = log.visible();
        let m = Material { spec: "teste".into(), wave: 1, codes: log.codes(), ..Material::default() };
        let (decision, rule, delivered) = (visible[0], visible[2], visible[3]);
        assert_eq!(
            codes_by_block(&m, &[decision, delivered, rule]),
            ["- `agreed`: MSTD-DEC-0001, MSTD-RULE-0001", "- `waves`: MSTD-DELIV-0001"]
        );
        assert_eq!(codes_by_block(&m, &[rule, decision]), ["- `agreed`: MSTD-RULE-0001, MSTD-DEC-0001"]);
    }

    /// Cada tarefa ganhou linha própria — o arquivo dela e o que precisa ler
    /// antes —, então o número de tarefas soma linhas ao pedido, sem teto: o
    /// pedido não corta onda grande, e nada mais corta.
    #[test]
    fn each_task_adds_one_line_and_the_request_has_no_task_count_cap() {
        const MANY: usize = 6;
        let wave = |tasks: usize| {
            let mut events: Vec<(&str, Value)> =
                vec![("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))];
            for _ in 0..tasks {
                events.push(("task", json!({"wave": 1, "text": "Fazer", "files": [{"path": "src/a.rs"}]})));
            }
            log(&events)
        };
        let (one, many) = (wave(1), wave(MANY));
        let small = build(&material(&one, 1), Locale::PtBr);
        let big = build(&material(&many, 1), Locale::PtBr);
        assert!(big.lines > small.lines, "cada tarefa soma linha: {}", big.text);
        assert!(big.text.contains(&format!("MSTD-TASK-{MANY:04}")), "{}", big.text);
    }

    /// O mesmo material escrito duas vezes dá os mesmos bytes.
    #[test]
    fn the_same_material_always_gives_the_same_bytes() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let first = build(&material(&log, 1), Locale::PtBr);
        let again = build(&material(&log, 1), Locale::PtBr);
        assert_eq!(first, again);
    }

    /// Um banco com `count` lições, uma linha cada no pedido.
    fn lesson_bank(count: usize) -> SpecLog {
        let events: Vec<(&str, Value)> = (1..=count)
            .map(|n| ("lesson", json!({"text": format!("Lição {n} do banco"), "keys": ["banco"], "class": "defect"})))
            .collect();
        log(&events)
    }

    /// Um pedido com centenas de lições — bem além do antigo teto de 500
    /// linhas — sai inteiro, sem recusa nenhuma: o teto não existe mais, e
    /// cada lição continua saindo como uma linha própria.
    #[test]
    fn a_request_far_past_the_old_line_cap_is_never_refused_and_carries_every_lesson() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let bank = lesson_bank(600);
        let mut m = material(&log, 1);
        m.lessons = bank.visible();
        let prompt = build(&m, Locale::PtBr);
        assert!(prompt.lines > 600, "{}", prompt.text);
        let last_id = bank.visible().last().expect("banco com lição").id;
        assert!(prompt.text.contains(&format!("- `lessons`: {last_id}")), "{}", prompt.text);
    }

    /// O pedido da onda 3, medido em 27.412 tokens, passa do teto de 25.000:
    /// a recusa diz os dois números. No teto exato (25.000) ele ainda cabe;
    /// um token a mais (25.001) já passa. O teto não mexe na montagem: é
    /// [`build`]/[`write`] que continuam saindo inteiros, sem linha cortada
    /// — quem decide despachar é que confere esta mensagem à parte.
    #[test]
    fn o_pedido_acima_de_vinte_e_cinco_mil_tokens_e_recusado() {
        let over = token_cap_message(3, 27_412, Locale::PtBr).expect("acima do teto: recusa");
        assert!(over.contains("27412"), "{over}");
        assert!(over.contains("25000"), "{over}");
        assert!(over.contains('3'), "a onda 3: {over}");

        assert!(token_cap_message(3, 25_000, Locale::PtBr).is_none(), "no teto exato, ainda cabe");
        assert!(token_cap_message(3, 25_001, Locale::PtBr).is_some(), "um token a mais já passa do teto");
    }

    /// A estimativa é perto de um token a cada quatro caracteres, sempre
    /// arredondada para cima: um texto que não é múltiplo de quatro não passa
    /// por baixo do teto real.
    #[test]
    fn a_estimativa_de_tokens_conta_perto_de_um_a_cada_quatro_caracteres() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2, "cinco caracteres arredondam para cima");
        assert_eq!(estimate_tokens(&"a".repeat(100_000)), 25_000, "cem mil caracteres batem exatos no teto");
    }

    /// Cada skill nomeada entra no pedido como uma linha — nome, quando usar e
    /// o caminho do arquivo —, sem o texto dela, e a skill cujo exemplo mudou
    /// depois dela sai marcada como a revisar.
    #[test]
    fn a_named_skill_is_recommended_by_path_and_a_stale_one_is_marked() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let mut m = material(&log, 1);
        m.skills = vec![
            Skill {
                name: "add-run-command".into(),
                when: "acrescentar um comando run".into(),
                path: "apps/rt/.claude/skills/add-run-command/SKILL.md".into(),
                stale: false,
            },
            Skill {
                name: "add-hook-rule".into(),
                when: "acrescentar uma regra de gancho".into(),
                path: ".claude/skills/add-hook-rule/SKILL.md".into(),
                stale: true,
            },
        ];
        let prompt = build(&m, Locale::PtBr);
        assert!(
            prompt.text.contains("`apps/rt/.claude/skills/add-run-command/SKILL.md`"),
            "{}",
            prompt.text
        );
        assert!(prompt.text.contains("acrescentar um comando run"), "{}", prompt.text);
        assert!(prompt.text.contains(translate("prompt.skill.read", Locale::PtBr)), "{}", prompt.text);
        let stale = translate("prompt.skill.stale", Locale::PtBr);
        assert!(prompt.text.contains(&format!("**add-hook-rule** ({stale})")), "{}", prompt.text);
        assert!(!prompt.text.contains(&format!("**add-run-command** ({stale})")), "{}", prompt.text);
    }

    /// A lição entra pelo número dela no banco, nunca pelo texto — nem o
    /// original nem o campo de busca —, e o número lido é o mesmo id que
    /// `run read lessons --term <número>` acha.
    #[test]
    fn a_lesson_shows_its_number_never_its_text() {
        let bank = log(&[(
            "lesson",
            json!({"text": "Apagar a pasta quebra o cache", "keys": ["apagar"], "class": "defect"}),
        )]);
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let mut m = material(&log, 1);
        let lesson = bank.visible()[0];
        m.lessons = vec![lesson];
        let prompt = build(&m, Locale::PtBr);
        assert!(!prompt.text.contains("Apagar a pasta quebra o cache"), "{}", prompt.text);
        let search = lesson.str_field("search").unwrap_or_default().to_string();
        assert!(!search.is_empty(), "a linha da lição guarda o campo de busca");
        assert!(!prompt.text.contains(&search), "{}", prompt.text);
        assert!(prompt.text.contains(&format!("- `lessons`: {}", lesson.id)), "{}", prompt.text);
    }

    /// Um plano com duas ondas e cinco regras, para provar o dono de cada
    /// item: uma diz a onda dela, uma não tem dono, uma é coberta por uma
    /// tarefa, uma fala do assunto de uma onda sem ser dela e uma vale no
    /// projeto todo.
    fn plan() -> SpecLog {
        log(&[
            (
                "rule",
                json!({"text": "No máximo 3 tentativas de compilação por onda", "keys": ["tentativas"],
                       "example": "a quarta tentativa para", "waves": [2]}),
            ),
            ("rule", json!({"text": "O commit segue o modelo aprovado", "keys": ["commit"], "example": "título curto"})),
            ("rule", json!({"text": "A barra de status mostra o link", "keys": ["barra"], "example": "duas linhas"})),
            (
                "rule",
                json!({"text": "O leitor do arquivo de eventos nunca lê o arquivo inteiro",
                       "keys": ["leitor"], "example": "um bloco por vez", "waves": [2]}),
            ),
            (
                "rule",
                json!({"text": "A página do relatório sai do mesmo motor", "keys": ["página"],
                       "example": "um motor só", "applies_to": {"files": ["**"]}}),
            ),
            ("wave", json!({"n": 1, "text": "Leitura", "criteria": [], "done_when": "lê"})),
            (
                "task",
                json!({"wave": 1, "text": "Escrever o leitor do arquivo de eventos",
                       "files": [{"path": "src/a.rs"}], "covers": [3]}),
            ),
            ("wave", json!({"n": 2, "text": "Página", "criteria": [], "done_when": "sai"})),
            ("task", json!({"wave": 2, "text": "Gravar a página do relatório", "files": [{"path": "src/b.rs"}]})),
        ])
    }

    fn texts(log: &SpecLog, wave: u64) -> Vec<String> {
        agreed_for(log, wave).iter().map(|e| e.str_field("text").unwrap_or_default().to_string()).collect()
    }

    /// O item que diz a onda dele vai para o pedido dela, e não para o das
    /// outras.
    #[test]
    fn an_item_that_names_its_wave_lands_only_in_that_waves_request() {
        let plan = plan();
        let rule = "No máximo 3 tentativas de compilação por onda".to_string();
        assert!(texts(&plan, 2).contains(&rule), "{:?}", texts(&plan, 2));
        assert!(!texts(&plan, 1).contains(&rule), "{:?}", texts(&plan, 1));
        assert_eq!(owners(&plan).get(&1), Some(&Owner::Waves(BTreeSet::from([2]))));
    }

    /// O item sem dono não vai para onda nenhuma, e é ele que o plano aponta
    /// como sem dono.
    #[test]
    fn an_item_without_owner_goes_to_no_wave_and_is_the_one_listed_as_unowned() {
        let plan = plan();
        let general = "O commit segue o modelo aprovado".to_string();
        assert!(!texts(&plan, 1).contains(&general), "{:?}", texts(&plan, 1));
        assert!(!texts(&plan, 2).contains(&general), "{:?}", texts(&plan, 2));
        let unowned: Vec<u64> = unowned(&plan).iter().map(|e| e.id).collect();
        assert_eq!(unowned, [2]);
        for wave in [1, 2] {
            let prompt = build(&with_agreed(&plan, wave), Locale::PtBr);
            assert!(!prompt.text.contains("MSTD-RULE-0002"), "onda {wave}: {}", prompt.text);
        }
    }

    /// A onda da tarefa que cobre o item é dona dele: o item entra no pedido
    /// dela e fica fora do das outras.
    #[test]
    fn the_wave_of_the_task_that_covers_an_item_owns_it() {
        let plan = plan();
        let picked = "A barra de status mostra o link".to_string();
        assert!(texts(&plan, 1).contains(&picked), "{:?}", texts(&plan, 1));
        assert!(!texts(&plan, 2).contains(&picked), "{:?}", texts(&plan, 2));
        assert_eq!(owners(&plan).get(&3), Some(&Owner::Waves(BTreeSet::from([1]))));
    }

    /// O item que vale no projeto todo é do projeto e vai para toda onda.
    #[test]
    fn an_item_that_holds_for_the_whole_project_belongs_to_the_project() {
        let plan = plan();
        let general = "A página do relatório sai do mesmo motor".to_string();
        assert!(texts(&plan, 1).contains(&general), "{:?}", texts(&plan, 1));
        assert!(texts(&plan, 2).contains(&general), "{:?}", texts(&plan, 2));
        assert_eq!(owners(&plan).get(&5), Some(&Owner::Project));
    }

    /// A onda só de texto não recebe o item do projeto todo: nem no pedido,
    /// nem entre os candidatos que o orquestrador julga. O item do projeto
    /// que a tarefa dela faz vai mesmo assim. Na divisa, um arquivo de
    /// código entre os de texto devolve o item; a onda só em views Razor ou
    /// só na página HTML também o recebe.
    #[test]
    fn onda_so_de_texto_nao_recebe_item_do_projeto_inteiro() {
        let everywhere = json!({"files": ["**"]});
        let wave = |n: u64, files: &[&str], covers: &[u64]| -> [(&'static str, Value); 2] {
            let paths: Vec<Value> = files.iter().map(|path| json!({"path": path})).collect();
            [
                ("wave", json!({"n": n, "text": "Onda", "criteria": [], "done_when": "pronto"})),
                ("task", json!({"wave": n, "text": "Fazer", "files": paths, "covers": covers})),
            ]
        };
        let mut events: Vec<(&str, Value)> = vec![
            ("rule", json!({"text": "O comentário diz o que o código faz", "keys": ["c"], "example": "e", "applies_to": everywhere})),
            ("rule", json!({"text": "O documento diz o comando de hoje", "keys": ["d"], "example": "e", "applies_to": everywhere})),
        ];
        events.extend(wave(1, &["docs/guia.md", "LEIA.txt", ".gitignore"], &[2]));
        events.extend(wave(2, &["docs/guia.md", "src/a.rs"], &[]));
        events.extend(wave(3, &["Views/Home/Index.cshtml"], &[]));
        events.extend(wave(4, &["site/spec.html"], &[]));
        let log = log(&events);
        assert_eq!(owners(&log).get(&1), Some(&Owner::Project));

        assert_eq!(ids(&agreed_for(&log, 1)), [2], "só o item que a tarefa da onda faz");
        let dispatched = ids(&log.step(&Step::Dispatch { wave: 1 }));
        assert!(!dispatched.contains(&1) && dispatched.contains(&2), "{dispatched:?}");
        assert!(candidates(&log, 1).project.is_empty(), "{:?}", ids(&candidates(&log, 1).project));
        for n in 2..=4 {
            assert!(ids(&agreed_for(&log, n)).contains(&1), "a onda {n} recebe o item do projeto");
            assert_eq!(ids(&candidates(&log, n).project), [1, 2], "a onda {n} julga os itens do projeto");
        }
    }

    /// Cada onda julga só o item sem dono que serve a ela: o que tem a
    /// palavra-chave inteira no texto das tarefas dela, ou o que cita um
    /// arquivo que ela mexe. O item que cita um arquivo de nenhuma onda e
    /// cuja palavra-chave nenhuma tarefa diz não é candidato de onda nenhuma,
    /// e continua sem dono na lista da revisão final. Na divisa, a
    /// palavra-chave pela metade não liga o item, e a pasta citada também
    /// não.
    #[test]
    fn item_sem_dono_so_aparece_na_onda_a_que_serve() {
        let log = log(&[
            (
                "rule",
                json!({"text": "A barra de status mostra o link da página", "keys": ["barra de status"], "example": "e"}),
            ),
            (
                "rule",
                json!({"text": "A gravação recusa o evento sem título (`src/gravar.rs`).", "keys": ["recusa sem título"], "example": "e"}),
            ),
            (
                "rule",
                json!({"text": "O texto em inglês segue o molde de `docs/molde.md` e a pasta `src/`.", "keys": ["idioma do molde"], "example": "e"}),
            ),
            (
                "rule",
                json!({"text": "A barra de rolagem some no celular", "keys": ["barra de rolagem"], "example": "e"}),
            ),
            ("wave", json!({"n": 1, "text": "Barra", "criteria": [], "done_when": "pronto"})),
            (
                "task",
                json!({"wave": 1, "text": "A barra de status deixa de mostrar a contagem de linhas",
                       "files": [{"path": "src/status.rs"}]}),
            ),
            ("wave", json!({"n": 2, "text": "Limite", "criteria": [], "done_when": "pronto"})),
            ("task", json!({"wave": 2, "text": "O limite do texto passa a valer", "files": [{"path": "src/gravar.rs"}]})),
        ]);
        assert_eq!(ids(&unowned(&log)), [1, 2, 3, 4], "os quatro continuam sem dono");

        assert_eq!(ids(&candidates(&log, 1).unowned), [1], "a onda 1 casa a palavra-chave do primeiro");
        assert_eq!(ids(&candidates(&log, 2).unowned), [2], "a onda 2 mexe no arquivo que o segundo cita");
        assert!(ids(&all_agreed(&log)).contains(&3), "o que não serve continua na revisão final");
    }

    /// O item também cita o arquivo solto no texto, sem crases e com a
    /// linha, e isso o liga à onda que mexe nesse arquivo. Na divisa, o
    /// caminho solto de um arquivo que a onda não mexe não liga o item, e a
    /// pasta solta também não. A leitura de uma skill continua só entre
    /// crases: o caminho solto não entra nela.
    #[test]
    fn caminho_sem_crases_so_liga_o_item_a_onda_que_mexe_no_arquivo() {
        let log = log(&[
            (
                "limit",
                json!({"text": "O texto da entrega tem o limite de hoje (TETO, src/gravar.rs:269).", "keys": ["teto"], "value": "8000"}),
            ),
            (
                "limit",
                json!({"text": "O título tem o limite de hoje (TITULO, src/titulo.rs:12).", "keys": ["teto"], "value": "60"}),
            ),
            ("limit", json!({"text": "Tudo em src/ tem o mesmo teto.", "keys": ["teto"], "value": "1"})),
            ("wave", json!({"n": 1, "text": "Limite", "criteria": [], "done_when": "pronto"})),
            ("task", json!({"wave": 1, "text": "O limite do texto passa a valer", "files": [{"path": "src/gravar.rs"}]})),
        ]);
        assert_eq!(ids(&unowned(&log)), [1, 2, 3], "os três continuam sem dono");
        assert_eq!(ids(&candidates(&log, 1).unowned), [1], "só o que cita solto o arquivo da onda");
        assert!(cited_paths("O texto da entrega tem o limite de hoje (TETO, src/gravar.rs:269).").is_empty());
    }

    /// A primeira rodada de uma obra real: as ondas 1 a 4 saíram juntas, e
    /// a rodada mostrou os mesmos 25 itens sem dono a cada uma. Só a onda 3
    /// usou algum: o orquestrador pôs seis no pedido dela, e as outras três
    /// não pegaram nenhum. Remontada cada onda com o texto e os arquivos reais
    /// da tarefa dela, cinco dos seis continuam candidatos da onda 3, e a
    /// escolha gravada no envio dela continua pondo os cinco; o limite do
    /// texto da entrega entra porque cita, sem crases, um arquivo que a onda
    /// mexe. Fica de fora só a entrega gravada duas vezes antes de a rodada
    /// assumir: nenhuma palavra-chave dela aparece inteira no texto da
    /// tarefa, e ela não cita arquivo. Cada uma das outras ondas recebe
    /// menos que os 25.
    #[test]
    fn os_itens_que_o_orquestrador_pos_na_onda_continuam_candidatos() {
        let fixture: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/itens-sem-dono-por-onda.json"
        )))
        .unwrap();
        let numbers = |value: &Value| -> BTreeSet<u64> {
            value.as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect()
        };
        let mut content = String::new();
        let mut line = |id: u64, event_type: &str, body: Value| {
            let mut map = crate::domain::spec_events::normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, id, None, "2026-09-23T19:30:00-03:00")));
            content.push('\n');
        };
        let items = fixture["items"].as_array().unwrap();
        for item in items {
            let body = json!({"text": item["text"], "keys": item["keys"]});
            line(item["id"].as_u64().unwrap(), item["type"].as_str().unwrap(), body);
        }
        let waves = fixture["waves"].as_array().unwrap();
        for (i, wave) in waves.iter().enumerate() {
            let n = wave["n"].as_u64().unwrap();
            let paths: Vec<Value> = wave["files"].as_array().unwrap().iter().map(|path| json!({"path": path})).collect();
            line(1000 + 2 * i as u64, "wave", json!({"n": n, "text": "A onda", "criteria": [], "done_when": "passa"}));
            line(1001 + 2 * i as u64, "task", json!({"wave": n, "text": wave["text"], "files": paths}));
        }
        let log = parse_log(&content);
        let all: BTreeSet<u64> = items.iter().map(|item| item["id"].as_u64().unwrap()).collect();
        assert_eq!(all.len(), 25);
        assert_eq!(unowned(&log).iter().map(|e| e.id).collect::<BTreeSet<_>>(), all, "os 25 continuam sem dono");

        let twice: BTreeSet<u64> = items
            .iter()
            .filter(|item| item["text"] == json!("A onda grava a entrega duas vezes antes de a rodada assumir."))
            .map(|item| item["id"].as_u64().unwrap())
            .collect();
        assert_eq!(twice.len(), 1, "a amostra tem a entrega gravada duas vezes");

        let mut shortfalls: Vec<String> = Vec::new();
        for wave in waves {
            let n = wave["n"].as_u64().unwrap();
            let (judged, added) = (numbers(&wave["judged"]), numbers(&wave["added"]));
            assert_eq!(judged, all, "onda {n}: a rodada mostrou os 25");
            let found = candidates(&log, n);
            let got: BTreeSet<u64> = found.unowned.iter().map(|e| e.id).collect();
            let lost: BTreeSet<u64> = added.difference(&got).copied().collect();
            if n == 3 {
                assert_eq!(added.len(), 6, "o orquestrador pôs seis na onda 3");
                if lost != twice {
                    shortfalls.push(format!("onda 3 perdeu {lost:?}, e não só {twice:?}; recebe {got:?}"));
                }
                let recorded = Choice { added: added.iter().map(|id| (*id, "motivo".to_string())).collect(), ..Choice::default() };
                let kept = recorded.within(&found).added.len();
                if kept != 5 {
                    shortfalls.push(format!("a escolha gravada da onda 3 põe {kept} dos seis, e não cinco"));
                }
            } else {
                assert!(added.is_empty() && lost.is_empty(), "onda {n}: o orquestrador não pôs nenhum");
                if got.len() >= judged.len() {
                    shortfalls.push(format!("onda {n} recebe {} de {}", got.len(), judged.len()));
                }
            }
        }
        assert!(shortfalls.is_empty(), "{shortfalls:#?}");
    }

    /// A busca por palavras não decide quem recebe o item: a regra que fala
    /// do leitor, assunto da tarefa da onda 1, é da onda 2 e vai só para ela.
    #[test]
    fn the_word_search_no_longer_decides_who_gets_an_item() {
        let plan = plan();
        let reader = "O leitor do arquivo de eventos nunca lê o arquivo inteiro".to_string();
        assert!(!texts(&plan, 1).contains(&reader), "{:?}", texts(&plan, 1));
        assert!(texts(&plan, 2).contains(&reader), "{:?}", texts(&plan, 2));
    }

    /// A onda que o item diz e que o plano não tem não é dona dele; a tarefa
    /// que cobre a versão antiga de um item é dona da versão nova.
    #[test]
    fn a_wave_missing_from_the_plan_owns_nothing_and_the_owner_follows_the_new_version() {
        let log = log(&[
            ("rule", json!({"text": "Regra da onda que não existe", "keys": ["r"], "example": "e", "waves": [9]})),
            ("decision", json!({"text": "Versão velha", "keys": ["d"], "why": "w"})),
            ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"})),
            ("task", json!({"wave": 1, "text": "Fazer", "files": [{"path": "src/a.rs"}], "covers": [2]})),
            ("decision", json!({"text": "Versão nova", "keys": ["d"], "why": "w", "replaces": 2})),
        ]);
        let owners = owners(&log);
        assert_eq!(owners.get(&1), None, "{owners:?}");
        assert_eq!(owners.get(&5), Some(&Owner::Waves(BTreeSet::from([1]))), "{owners:?}");
        assert_eq!(unowned(&log).iter().map(|e| e.id).collect::<Vec<_>>(), [1]);
    }

    /// Um arquivo com a spec aprovada e, depois, o item que o teste pedir.
    fn approved_then(item: Option<(&str, Value)>) -> SpecLog {
        let mut events: Vec<(&str, Value)> = vec![
            ("decision", json!({"text": "Antiga, sem dono", "keys": ["a"], "why": "w"})),
            ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"})),
            ("task", json!({"wave": 1, "text": "Fazer", "files": [{"path": "src/a.rs"}], "covers": [1]})),
            ("state", json!({"phase": "approved", "author": "binary"})),
        ];
        events.extend(item);
        log(&events)
    }

    /// Depois da aprovação, o item combinado novo nasce com dono: os arquivos
    /// das tarefas que o cobrem, o projeto todo, a onda que ele diz ou a
    /// tarefa que já cobria a versão antiga. Sem dono, é recusado, com o texto
    /// que manda dar o dono pelos arquivos; antes da aprovação, passa.
    #[test]
    fn a_new_agreed_item_after_the_approval_is_born_with_an_owner() {
        let before = approved_then(None);
        let decision = |extra: Value| {
            let mut body = json!({"text": "Nova", "keys": ["n"], "why": "w"});
            body.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
            approved_then(Some(("decision", body)))
        };
        let refused = owner_rule(&before, &decision(json!({}))).unwrap_err();
        assert_eq!(refused.reason(), "owner-missing");
        for lang in [Locale::PtBr, Locale::EnUs] {
            let message = refused.message(lang);
            assert!(message.contains("decision") && message.contains("applies_to") && message.contains("**"), "{message}");
            assert!(!message.contains("waves"), "{message}");
        }
        assert_eq!(owner_rule(&before, &decision(json!({"applies_to": {"files": ["src/a.rs"]}}))), Ok(()));
        assert!(owner_rule(&before, &decision(json!({"applies_to": {"files": []}}))).is_err(), "sem arquivo não é dono");
        assert_eq!(owner_rule(&before, &decision(json!({"waves": [1]}))), Ok(()));
        assert_eq!(owner_rule(&before, &decision(json!({"applies_to": {"files": ["**"]}}))), Ok(()));
        assert_eq!(owner_rule(&before, &decision(json!({"replaces": 1}))), Ok(()), "a tarefa cobre a versão antiga");
        assert_eq!(owner_rule(&before, &decision(json!({"waves": [9]}))), Ok(()), "a onda que ainda vai existir vale");
        assert!(owner_rule(&before, &decision(json!({"waves": [0]}))).is_err(), "onda zero não é dona");

        let mut survey = approved_then(None);
        survey.events.retain(|e| e.event_type != "state");
        let mut added = decision(json!({}));
        added.events.retain(|e| e.event_type != "state");
        assert_eq!(owner_rule(&survey, &added), Ok(()), "antes da aprovação o dono vem do plano");
        let rule = approved_then(Some(("rule", json!({"text": "Sem dono", "keys": ["r"], "example": "e"}))));
        assert!(owner_rule(&before, &rule).is_err(), "vale para todo item combinado");
    }

    /// Um plano de duas ondas com itens sem dono, um para cada caso da
    /// proposta: a tarefa que nasceu da versão velha, a tarefa que cita o
    /// código, a onda citada de três jeitos, o que não conta como citação, os
    /// arquivos citados, o `applies_to` e a ordem entre as regras.
    fn unowned_plan() -> SpecLog {
        let rule = |text: &str| ("rule", json!({"text": text, "keys": ["r"], "example": "e"}));
        let decision = |text: &str| ("decision", json!({"text": text, "keys": ["d"], "why": "w"}));
        log(&[
            decision("Versão velha"),
            ("wave", json!({"n": 1, "text": "Leitura", "criteria": [], "done_when": "lê"})),
            (
                "task",
                json!({"wave": 1, "text": "Escrever o leitor", "files": [{"path": "src/leitor.rs"}], "origin": 1}),
            ),
            ("wave", json!({"n": 2, "text": "Página", "criteria": [], "done_when": "sai"})),
            (
                "task",
                json!({"wave": 2, "text": "Gravar a página (MSTD-DEC-0002)",
                       "files": [{"path": "apps/rt/src/pagina.rs"}]}),
            ),
            ("decision", json!({"text": "Versão nova", "keys": ["d"], "why": "w", "replaces": 1})),
            decision("A página nova sai na onda 1"),
            rule("O conserto da onda 2."),
            rule("Entre as ondas 1 e 2, nada muda."),
            rule("Não contam: as ondas. (1) nem a onda: 2 nem a MSTD-DEC-0001 nem a onda 9; nem a pasta `apps/rt/`."),
            decision("Como diz a MSTD-WAVE-0002."),
            rule("O leitor de `rt/src/pagina.rs` muda."),
            ("rule", json!({"text": "Vale para as fontes.", "keys": ["r"], "example": "e",
                            "applies_to": {"files": ["src/**"]}})),
            rule("Da onda 1, e cita `rt/src/pagina.rs`."),
            ("rule", json!({"text": "Do projeto", "keys": ["r"], "example": "e", "applies_to": {"files": ["**"]}})),
            ("rule", json!({"text": "Já tem dono", "keys": ["r"], "example": "e", "waves": [2]})),
        ])
    }

    fn waves(list: &[u64]) -> Option<Owner> {
        Some(Owner::Waves(list.iter().copied().collect()))
    }

    /// A proposta dá a cada item sem dono as ondas da primeira regra que
    /// acha onda do plano — as tarefas que nasceram dele ou citam o código,
    /// a onda que o texto cita, os arquivos em comum —, e deixa sem dono o
    /// que nenhuma resolve. O item do projeto, o que já tem dono e o que diz
    /// arquivos em `applies_to`, que é dono deles, ficam fora.
    #[test]
    fn the_proposal_gives_each_unowned_item_the_waves_of_the_first_rule_that_finds_one() {
        let log = unowned_plan();
        let got: Vec<(u64, Option<Owner>, OwnerFrom)> =
            propose_owners(&log).into_iter().map(|line| (line.item, line.owner, line.from)).collect();
        assert_eq!(
            got,
            [
                (6, waves(&[1]), OwnerFrom::Tasks(vec![3])),
                (7, waves(&[2]), OwnerFrom::Tasks(vec![5])),
                (8, waves(&[2]), OwnerFrom::Cited),
                (9, waves(&[1, 2]), OwnerFrom::Cited),
                (10, None, OwnerFrom::Nothing),
                (11, waves(&[2]), OwnerFrom::Cited),
                (12, waves(&[2]), OwnerFrom::Files(vec!["rt/src/pagina.rs".into()])),
                (14, waves(&[1]), OwnerFrom::Cited),
            ]
        );
    }

    /// O orquestrador dá o dono do item que a proposta não resolve, ou troca
    /// o dela, sempre com o motivo; a linha que não serve é recusada pelo
    /// código: o item que já tem dono, o código que não existe, a onda fora
    /// do plano, nenhuma onda e o motivo em branco.
    #[test]
    fn the_orchestrator_gives_or_replaces_an_owner_and_a_line_that_does_not_fit_is_refused() {
        let log = unowned_plan();
        let given = |code: &str, owner: Owner, why: &str| GivenOwner { code: code.into(), owner, why: why.into() };
        let lines = owner_list(
            &log,
            &[
                given("MSTD-RULE-0003", Owner::Project, " vale para todo pedido "),
                given("MSTD-RULE-0001", Owner::Waves(BTreeSet::from([1])), "a 1 é que conserta"),
            ],
        )
        .unwrap();
        let of = |id: u64| lines.iter().find(|line| line.item == id).cloned().unwrap();
        assert_eq!(of(10).owner, Some(Owner::Project));
        assert_eq!(of(10).from, OwnerFrom::Orchestrator("vale para todo pedido".into()));
        assert_eq!(of(8).owner, waves(&[1]));
        assert_eq!(of(8).from, OwnerFrom::Orchestrator("a 1 é que conserta".into()));
        assert_eq!(of(9).from, OwnerFrom::Cited, "a proposta do resto fica");

        for (code, owner, why) in [
            ("MSTD-RULE-0008", Owner::Project, "já tem dono"),
            ("MSTD-RULE-0099", Owner::Project, "não existe"),
            ("MSTD-RULE-0003", Owner::Waves(BTreeSet::from([9])), "fora do plano"),
            ("MSTD-RULE-0003", Owner::Waves(BTreeSet::new()), "nenhuma onda"),
            ("MSTD-RULE-0003", Owner::Project, "  "),
        ] {
            assert_eq!(owner_list(&log, &[given(code, owner, why)]), Err(code.to_string()), "{why}");
        }
    }

    /// O material de uma onda com os itens combinados escolhidos para ela.
    fn with_agreed(log: &SpecLog, wave: u64) -> Material<'_> {
        let mut m = material(log, wave);
        m.agreed = agreed_for(log, wave);
        m
    }

    /// O item combinado escolhido para a onda entra numa linha, com o bloco,
    /// o código e o título — no item gravado antes do título, a primeira
    /// frase do texto —, e logo abaixo dela, recuada, a parte do agente; o
    /// item gravado antes dessa parte traz o texto inteiro. O exemplo nunca
    /// entra, nem o texto do item que tem a parte do agente.
    #[test]
    fn every_agreed_item_comes_with_its_title_and_its_agent_part_below() {
        let everywhere = json!({"files": ["**"]});
        let log = log(&[
            (
                "rule",
                json!({"text": "A barra de status mostra o link. Ela cabe em duas linhas.", "keys": ["barra"],
                       "example": "um link só", "applies_to": everywhere}),
            ),
            (
                "rule",
                json!({"title": "O pedido cabe numa leitura", "text": "O agente lê o pedido inteiro. Depois ele começa.",
                       "agent": "- conferir no teste", "keys": ["pedido"], "example": "um pedido curto",
                       "applies_to": everywhere}),
            ),
            ("wave", json!({"n": 1, "text": "Leitura", "criteria": [], "done_when": "lê"})),
            ("task", json!({"wave": 1, "text": "Escrever o leitor", "files": [{"path": "src/a.rs"}]})),
        ]);
        let prompt = build(&with_agreed(&log, 1), Locale::PtBr);
        for text in ["um link só", "O agente lê o pedido", "Depois ele começa", "um pedido curto"] {
            assert!(!prompt.text.contains(text), "{text:?} foi copiado: {}", prompt.text);
        }
        let items: Vec<&str> = section(&prompt.text, translate("prompt.part.items", Locale::PtBr)).lines().collect();
        assert_eq!(
            items,
            [
                "",
                "- `waves` MSTD-WAVE-0001: Leitura",
                "- `agreed` MSTD-RULE-0001: A barra de status mostra o link.",
                "  A barra de status mostra o link. Ela cabe em duas linhas.",
                "- `agreed` MSTD-RULE-0002: O pedido cabe numa leitura",
                "  - conferir no teste",
            ],
            "{}",
            prompt.text
        );
    }

    /// O item marcado como válido para todas as ondas entra na lista de cada
    /// uma delas, sem nenhuma tarefa precisar declará-lo.
    #[test]
    fn the_item_that_holds_for_every_wave_is_listed_in_all_of_them() {
        let log = log(&[
            (
                "rule",
                json!({"title": "Suíte verde para fechar", "text": "Nenhuma onda fecha com a suíte vermelha.",
                       "agent": "- rodar a suíte", "keys": ["suíte"], "example": "a onda para",
                       "applies_to": {"files": ["**"]}}),
            ),
            ("wave", json!({"n": 1, "text": "Leitura", "criteria": [], "done_when": "lê"})),
            ("task", json!({"wave": 1, "text": "Escrever o leitor", "files": [{"path": "src/a.rs"}]})),
            ("wave", json!({"n": 2, "text": "Página", "criteria": [], "done_when": "sai"})),
            ("task", json!({"wave": 2, "text": "Gravar a página", "files": [{"path": "src/b.rs"}]})),
        ]);
        for wave in [1, 2] {
            let prompt = build(&with_agreed(&log, wave), Locale::PtBr);
            let agreed = listed(&prompt.text, translate("prompt.part.items", Locale::PtBr));
            assert!(agreed.contains(&"- `agreed` MSTD-RULE-0001: Suíte verde para fechar"), "onda {wave}: {}", prompt.text);
            assert!(!prompt.text.contains("suíte vermelha"), "onda {wave}: {}", prompt.text);
        }
    }

    /// Os itens da onda saem na ordem de execução que ela declara; o que ela
    /// não lista vem depois, na ordem do arquivo. A onda sem essa ordem sai
    /// como está no arquivo.
    #[test]
    fn the_wave_items_come_in_the_execution_order_the_wave_declares() {
        let events = |order: Value| {
            vec![
                ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto", "order": order})),
                ("task", json!({"wave": 1, "text": "Primeira", "files": [{"path": "src/a.rs"}]})),
                ("task", json!({"wave": 1, "text": "Segunda", "files": [{"path": "src/b.rs"}]})),
                ("task", json!({"wave": 1, "text": "Terceira", "files": [{"path": "src/c.rs"}]})),
            ]
        };
        let codes_in_order = |prompt: &str| -> Vec<String> {
            section(prompt, translate("prompt.part.tasks", Locale::PtBr))
                .lines()
                .filter_map(|line| line.strip_prefix("- `MSTD-TASK-"))
                .filter_map(|line| line.split('`').next())
                .map(str::to_string)
                .collect()
        };

        let declared = log(&events(json!([4, 2])));
        let prompt = build(&material(&declared, 1), Locale::PtBr);
        assert_eq!(codes_in_order(&prompt.text), ["0003", "0001", "0002"], "{}", prompt.text);

        let plain = log(&events(json!([])));
        let prompt = build(&material(&plain, 1), Locale::PtBr);
        assert_eq!(codes_in_order(&prompt.text), ["0001", "0002", "0003"], "{}", prompt.text);
    }

    /// Regra, onda, tarefa e uma lista grande de lições juntas: nada no
    /// pedido obriga a cortar nenhuma parte por causa do tamanho, o pedido
    /// sai com todas elas.
    #[test]
    fn a_request_that_mixes_every_kind_of_content_is_never_cut_for_its_size() {
        let log = log(&[
            ("rule", json!({"text": "Uma regra qualquer", "keys": ["regra"], "example": "exemplo", "waves": [1]})),
            ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"})),
            ("task", json!({"wave": 1, "text": "Fazer", "files": [{"path": "src/a.rs"}]})),
        ]);
        let bank = lesson_bank(600);
        let mut m = with_agreed(&log, 1);
        m.lessons = bank.visible();
        let prompt = build(&m, Locale::PtBr);
        assert!(prompt.text.contains("MSTD-WAVE-0001"), "{}", prompt.text);
        assert!(prompt.text.contains("MSTD-TASK-0001"), "{}", prompt.text);
        assert!(prompt.text.contains("MSTD-RULE-0001"), "{}", prompt.text);
        let last_id = bank.visible().last().expect("banco com lição").id;
        assert!(prompt.text.contains(&format!("- `lessons`: {last_id}")), "{}", prompt.text);
    }

    /// As instruções fixas abrem todo pedido, no idioma do projeto.
    #[test]
    fn every_request_opens_with_the_same_fixed_instructions() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        for lang in [Locale::PtBr, Locale::EnUs] {
            let prompt = build(&material(&log, 1), lang);
            assert!(prompt.text.contains(translate("prompt.fixed", lang)), "{lang:?}");
        }
    }

    /// O texto do agente da onda, que ele carrega uma vez, diz que o pedido
    /// traz o título de cada item e, sob a linha dele, a parte do agente, que o
    /// texto completo se lê só em caso de dúvida e que o item novo que ele
    /// gravar leva as três partes; a ordem antiga de ler tudo antes de começar
    /// saiu, e a parte fixa do pedido não repete nada disso.
    #[test]
    fn the_wave_agent_reads_the_whole_text_only_in_case_of_doubt() {
        for (lang, said, gone) in [
            (
                Locale::PtBr,
                [
                    "o código e o título de cada item",
                    "sob a linha dele, a parte do agente",
                    "na dúvida, o comando que ele dá lê o texto completo",
                    "leva `title`, `text` e `agent`",
                ],
                "rodá-lo antes de começar",
            ),
            (
                Locale::EnUs,
                [
                    "each item's code and title",
                    "under its line, the agent part",
                    "when in doubt, the command it gives reads the whole text",
                    "takes `title`, `text` and `agent`",
                ],
                "running it before you start",
            ),
        ] {
            let (_, agent) = crate::platform::seeds::agent_texts(lang)[0];
            let fixed = translate("prompt.fixed", lang);
            for sentence in said {
                assert!(agent.contains(sentence), "{sentence}: {agent}");
                assert!(!fixed.contains(sentence), "{sentence}: {fixed}");
            }
            assert!(!agent.contains(gone), "{agent}");
        }
    }

    /// O pedido da revisão final traz as instruções fixas dela, todas as
    /// ondas com as tarefas, o que cada uma entregou e os critérios, uma linha
    /// por item, sem texto de item nenhum.
    #[test]
    fn the_final_review_request_lists_every_wave_what_each_delivered_and_the_criteria() {
        let log = log(&[
            ("criterion", json!({"when": "a spec fecha", "then": "passa", "proof": "true"})),
            ("wave", json!({"n": 1, "text": "Primeira onda secreta", "criteria": [1], "done_when": "pronto"})),
            ("task", json!({"wave": 1, "text": "Fazer a primeira", "files": [{"path": "src/a.rs"}]})),
            ("wave", json!({"n": 2, "text": "Segunda onda secreta", "criteria": [1], "done_when": "pronto"})),
            ("delivered", json!({"wave": 1, "text": "Entrega da primeira", "files": ["src/a.rs"]})),
            ("delivered", json!({"wave": 2, "text": "Entrega da segunda", "files": ["src/b.rs"]})),
        ]);
        let visible = log.visible();
        let of = |kind: &str| -> Vec<&SpecEvent> { visible.iter().copied().filter(|e| e.event_type == kind).collect() };
        let mut block = of("wave");
        block.extend(of("task"));
        let material = Material {
            spec: "x".into(),
            block,
            own_delivered: of("delivered"),
            criteria: of("criterion"),
            codes: log.codes(),
            ..Material::default()
        };
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = write_final_review(&material, lang);
            assert!(text.starts_with(&format!("# {}", translate("prompt.final.title", lang).replace("{spec}", "x"))));
            assert!(text.contains(translate("prompt.final.fixed", lang)), "{text}");
            for key in ["prompt.part.waves", "prompt.part.each_delivered", "prompt.part.criteria"] {
                assert!(text.contains(&format!("## {}", translate(key, lang))), "{key}: {text}");
            }
            assert_eq!(
                listed(&text, translate("prompt.part.waves", lang)),
                ["- `waves`: MSTD-WAVE-0001, MSTD-WAVE-0002, MSTD-TASK-0001"],
                "{text}"
            );
            assert_eq!(
                listed(&text, translate("prompt.part.each_delivered", lang)),
                ["- `waves`: MSTD-DELIV-0001, MSTD-DELIV-0002"],
                "{text}"
            );
            assert_eq!(text.matches("mustard-rt run read").count(), 1, "{text}");
            for secret in ["Primeira onda secreta", "Entrega da segunda", "a spec fecha"] {
                assert!(!text.contains(secret), "no item text is copied: {text}");
            }
        }
    }

    /// O trecho de um texto que vai do título `## {heading}` até o título
    /// seguinte.
    fn section<'t>(text: &'t str, heading: &str) -> &'t str {
        let Some((_, rest)) = text.split_once(&format!("## {heading}\n")) else { return "" };
        rest.split("\n## ").next().unwrap_or_default()
    }

    /// As linhas de lista (`- `) de uma parte do pedido.
    fn listed<'t>(text: &'t str, heading: &str) -> Vec<&'t str> {
        section(text, heading).lines().filter(|line| line.starts_with("- ")).collect()
    }

    /// Uma onda que saiu, entregou e foi reprovada, com itens gravados antes e
    /// depois do envio; `fixed` acrescenta o envio e a entrega do conserto e
    /// um item gravado durante ele.
    fn rejected(fixed: bool) -> SpecLog {
        let send = json!({"wave": 1, "role": "wave", "text": "p", "lines": 1, "chars": 1, "items": [1], "mustard": "0"});
        let mut events: Vec<(&str, Value)> = vec![
            ("decision", json!({"text": "Antes do envio", "keys": ["a"], "why": "w"})),
            ("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"})),
            ("task", json!({"wave": 1, "text": "Fazer", "files": [{"path": "src/a.rs"}]})),
            ("wave", json!({"n": 2, "text": "Outra", "criteria": [], "done_when": "pronto"})),
            ("send", send.clone()),
            ("decision", json!({"text": "Da outra onda", "keys": ["b"], "why": "w", "waves": [2]})),
            ("delivered", json!({"wave": 1, "text": "Feito", "files": ["src/a.rs"]})),
            ("verdict", json!({"wave": 1, "result": "rejected", "text": "Falta o teste", "criteria": []})),
            ("decision", json!({"text": "Depois da reprovação", "keys": ["c"], "why": "w", "waves": [1]})),
            ("rule", json!({"text": "Do projeto", "keys": ["d"], "example": "e", "applies_to": {"files": ["**"]}})),
        ];
        if fixed {
            events.push(("send", send));
            events.push(("delivered", json!({"wave": 1, "text": "Conserto", "files": ["src/a.rs"]})));
            events.push(("decision", json!({"text": "Durante o conserto", "keys": ["e"], "why": "w", "waves": [1]})));
        }
        log(&events)
    }

    fn ids(events: &[&SpecEvent]) -> Vec<u64> {
        events.iter().map(|e| e.id).collect()
    }

    /// As linhas do conserto são o veredito que reprovou, a entrega anterior
    /// a ele e os itens do pedido da onda gravados depois do último envio: o
    /// item de antes do envio e o de outra onda ficam fora. O envio do
    /// conserto não muda a âncora, e o item gravado durante o conserto entra.
    /// A onda sem reprovação, ou aprovada depois, não tem linha nenhuma.
    #[test]
    fn the_fix_lines_are_the_verdict_the_previous_delivery_and_the_items_after_the_last_send() {
        assert_eq!(ids(&fix_lines(&rejected(false), 1)), [8, 7, 9, 10]);
        assert!(fix_lines(&rejected(false), 2).is_empty());
        assert_eq!(ids(&fix_lines(&rejected(true), 1)), [8, 7, 9, 10, 13]);

        let mut approved = rejected(true);
        let mut more = log(&[("verdict", json!({"wave": 1, "result": "approved", "text": "ok", "criteria": []}))]);
        more.events[0].id = 14;
        approved.events.extend(more.events);
        assert!(fix_lines(&approved, 1).is_empty());

        let dispatched = ids(&rejected(false).step(&Step::Dispatch { wave: 1 }));
        let reviewed = ids(&rejected(true).step(&Step::Review { wave: 1 }));
        for id in [8, 7, 9, 10] {
            assert!(dispatched.contains(&id) && reviewed.contains(&id), "{id}: {dispatched:?} {reviewed:?}");
        }
    }

    /// O pedido do conserto (o da própria onda) leva as linhas do conserto
    /// dentro dos itens da onda, sem título próprio, junto com o resto que
    /// abre o trabalho, e a frase que manda consertar só isso. O pedido da
    /// revisão final não as repete: o veredito que reprovou aparece uma vez
    /// só, na parte do que mudou, e a entrega e os itens do conserto não
    /// ganham parte à parte. Fora de um conserto, o pedido da onda não traz a
    /// frase de abertura do conserto.
    #[test]
    fn the_fix_lines_go_into_the_wave_items_and_never_into_the_final_review() {
        let log = rejected(false);
        let mut m = material(&log, 1);
        m.fix = fix_lines(&log, 1);
        m.since_verdict = log.get(8).into_iter().collect();
        for lang in [Locale::PtBr, Locale::EnUs] {
            let fix_intro = translate("prompt.fix.wave", lang);
            let items_heading = translate("prompt.part.items", lang);
            let since_heading = translate("prompt.part.since_verdict", lang);
            let wave = write(&m, lang);
            let last = write_final_review(&m, lang);
            let items = section(&wave, items_heading);
            assert!(items.contains(fix_intro), "{wave}");
            for line in [
                "- `review` MSTD-VERD-0001: Falta o teste",
                "- `waves` MSTD-DELIV-0001: Feito",
                "- `agreed` MSTD-DEC-0003: Depois da reprovação",
                "- `agreed` MSTD-RULE-0001: Do projeto",
            ] {
                assert!(items.lines().any(|l| l == line), "{line}: {wave}");
            }
            assert_eq!(last.matches("MSTD-VERD-0001").count(), 1, "o veredito aparece uma vez só: {last}");
            assert_eq!(listed(&last, since_heading), ["- `review`: MSTD-VERD-0001"], "{last}");
            let headings: Vec<&str> = last.lines().filter_map(|line| line.strip_prefix("## ")).collect();
            let expected = [since_heading, translate("prompt.part.waves", lang), translate("prompt.part.execution", lang)];
            assert_eq!(headings, expected, "o conserto não ganha parte à parte: {last}");
            assert!(!last.contains("MSTD-DEC-0003"), "o item gravado depois da reprovação fica no pedido da onda: {last}");
        }
        let plain = material(&log, 1);
        assert!(!write(&plain, Locale::PtBr).contains(translate("prompt.fix.wave", Locale::PtBr)));
    }

    /// A execução de um pedido montado com a cópia que a rodada criou.
    fn with_copy() -> Execution {
        Execution {
            build: Some("make".into()),
            test: Some("make test".into()),
            running: vec![(2, vec!["src/b.rs".into(), "src/c.rs".into()]), (3, Vec::new())],
            root: "/repo".into(),
            copy: Some(WaveCopy { path: "/repo/copia-1".into(), reused: None }),
            ..Execution::default()
        }
    }

    /// A mesma execução no pedido do revisor final: a cópia que o fechamento
    /// criou para ele, no commit mais novo da obra.
    fn with_final_copy() -> Execution {
        Execution {
            commit: Some("abc1234".into()),
            copy: Some(WaveCopy { path: "/repo/revisao-1".into(), reused: None }),
            ..with_copy()
        }
    }

    /// O pedido da onda traz as regras da execução: a cópia separada que a
    /// rodada preparou, os comandos do projeto, não comitar e as outras ondas
    /// em andamento com os arquivos delas; o caminho do repositório principal
    /// vem só no exemplo de leitura. O da revisão final diz em que cópia
    /// trabalhar, como criá-la no commit mais novo, compilar com menos
    /// processos e apagar a cópia no fim; sem commit, a cópia sai do atual.
    /// Sem cópia, o pedido da onda não fala de cópia nem do repositório
    /// principal. Nenhum dos dois cita pasta de compilação à parte.
    #[test]
    fn the_requests_carry_the_execution_rules_and_the_copy() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let mut m = material(&log, 1);
        m.execution = with_copy();
        let t = |key: &str| translate(key, Locale::PtBr);
        let wave = write(&m, Locale::PtBr);
        let rules = section(&wave, t("prompt.part.execution"));
        for line in [
            format!("- {}", t("prompt.execution.copy").replace("{copy}", "/repo/copia-1").replace("{root}", "/repo")),
            format!("- {}", t("prompt.execution.no_commit")),
            format!("- {}", t("prompt.execution.commit_field")),
            format!("- {}", t("prompt.execution.report_lines")),
            "- Compile com `make`.".to_string(),
            "- Teste com `make test`.".to_string(),
            format!("- {}", t("prompt.execution.running")),
            "  - Onda 2: `src/b.rs`, `src/c.rs`".to_string(),
            "  - Onda 3\n".to_string(),
        ] {
            assert!(rules.contains(&line), "{line}: {rules}");
        }
        assert!(!rules.contains("worktree") && !rules.contains("CARGO_TARGET_DIR"), "{rules}");
        // De onde ler a spec, só a linha de como ler diz, uma vez, nos dois
        // comandos dela.
        let wave_example =
            t("prompt.read.wave").replace("{root}", "--root /repo ").replace("{spec}", "teste").replace("{n}", "1");
        assert!(wave.contains(&wave_example) && !rules.contains("--root"), "{wave}");
        assert_eq!(wave.matches("--root").count(), 2, "{wave}");

        let example = t("prompt.read").replace("{root}", "--root /repo ").replace("{spec}", "teste");
        m.execution = with_final_copy();
        let last = write_final_review(&m, Locale::PtBr);
        assert!(last.contains(&example), "{last}");
        assert_eq!(last.matches("--root").count(), 1, "{last}");
        let rules = section(&last, t("prompt.part.execution"));
        for line in [
            "já a criou no commit `abc1234`",
            t("prompt.review.jobs"),
            "recusa começar sobre `/repo/revisao-1` com mudança",
            "- Compile com `make`.",
        ] {
            assert!(rules.contains(line), "{line}: {rules}");
        }
        assert!(!rules.contains("Onda 2"), "a revisão roda na cópia dela: {rules}");
        assert!(!rules.contains("CARGO_TARGET_DIR"), "{rules}");
        assert!(
            !rules.contains(t("prompt.execution.no_commit"))
                && !rules.contains(t("prompt.execution.commit_field"))
                && !rules.contains(t("prompt.execution.report_lines")),
            "o revisor não entrega, e o pedido dele não fala do campo commit nem das duas linhas: {rules}"
        );

        m.execution = Execution { root: "/repo".into(), ..Execution::default() };
        let wave = write(&m, Locale::PtBr);
        let rules = section(&wave, t("prompt.part.execution"));
        assert!(!rules.contains("Compile com") && !rules.contains(t("prompt.execution.running")), "{rules}");
        assert!(
            !rules.contains(t("prompt.execution.no_commit"))
                && !rules.contains(t("prompt.execution.commit_field"))
                && !rules.contains(t("prompt.execution.report_lines")),
            "sem cópia, não há o que comitar nem relatar: {rules}"
        );
        assert!(!rules.contains("CARGO_TARGET_DIR") && !wave.contains("--root"), "{wave}");
        assert!(write_final_review(&m, Locale::PtBr).contains("no commit `HEAD`"));
        assert!(write_final_review(&m, Locale::PtBr).contains(&example), "o revisor trabalha sempre numa cópia");
        let en = write(&Material { execution: with_copy(), ..material(&log, 1) }, Locale::EnUs);
        let rules = section(&en, translate("prompt.part.execution", Locale::EnUs));
        assert!(rules.contains("`/repo/copia-1`") && !rules.contains("target/copias"), "{rules}");
    }

    /// Os textos dos agentes, que cada agente carrega uma vez, exigem o teste
    /// de cada critério nascendo vermelho pelo caminho que o usuário usa, e
    /// não só pela função auxiliar, com a entrega dizendo como a prova foi
    /// feita; o do revisor manda rodar a prova gravada, ler as provas do
    /// vermelho da entrega e cortar onde a onda não cortou, sem repetir os
    /// cortes dela. A parte fixa dos pedidos não repete nada disso.
    #[test]
    fn the_agent_texts_ask_for_the_red_proof_by_the_real_path_and_the_review_skips_the_waves_cuts() {
        for (lang, wave, review) in [
            (
                Locale::PtBr,
                ["nasce vermelho", "o comando ou o evento do gancho", "não só na função auxiliar", "verificação do vermelho (o que foi cortado"],
                ["rode a verificação gravada", "verificação do vermelho que a entrega relata", "onde a onda não cortou", "sem repetir os dela"],
            ),
            (
                Locale::EnUs,
                ["is born red", "the command or the hook event", "not only in the helper function", "red verification (what was cut"],
                ["run its recorded verification", "red verification the delivery reports", "where the wave did not cut", "without repeating its own"],
            ),
        ] {
            let agents = crate::platform::seeds::agent_texts(lang);
            for (said, (agent, key)) in wave.iter().map(|s| (s, (agents[0].1, "prompt.fixed"))).chain(review.iter().map(|s| (s, (agents[1].1, "prompt.final.fixed")))) {
                assert!(agent.contains(said), "{lang:?}: {said}: {agent}");
                assert!(!translate(key, lang).contains(said), "{lang:?} {key} repeats {said}");
            }
        }
    }
}
