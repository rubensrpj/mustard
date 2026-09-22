//! O pedido de uma onda: o texto que o agente dela recebe, montado dos blocos
//! já lidos do arquivo de eventos.
//!
//! Tudo aqui é puro: sem disco, sem relógio e sem descobrir caminho nenhum.
//! Quem lê o arquivo, o banco de lições e os arquivos das skills entrega os
//! blocos prontos em [`Material`] — inclusive as pastas da cópia separada, que
//! a rodada escolhe —; esta função só os escreve, sempre na mesma ordem, então
//! o mesmo material dá sempre os mesmos bytes.
//!
//! O pedido leva a lista, não o texto. Só duas exceções copiam texto de item:
//! a frase do `done_when` da própria onda, que abre o pedido porque é o que
//! ela entrega, e o `<bloco>` de cada arquivo de leitura por tarefa. Fora
//! disso, cada parte traz, numa linha por bloco da spec, os códigos dos itens
//! em sequência, e o pedido mostra uma vez só o comando que lê um item pelo
//! binário, com o caminho do repositório principal quando o agente trabalha
//! numa cópia. O agente lê cada código na ordem, na hora de agir. As tarefas
//! saem na ordem de execução que a onda declara, cada uma com o arquivo dela
//! e o que precisa ler antes; sem essa ordem, na ordem do arquivo. A lista
//! inteira fica no pedido, porque é ela que mostra o escopo todo de uma vez.
//! As lições entram pelo número delas no banco, como todo o resto — `run read
//! lessons --term <número>` devolve o texto — e cada skill entra como
//! recomendação de uma linha. O pedido não tem teto de linhas: o que cresce é
//! sempre lista, nunca texto de item.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;

use crate::domain::lessons::{applies_to, Scope};
use crate::domain::mustard_id;
use crate::domain::project_map::cited_paths;
use crate::domain::search;
use crate::domain::spec_events::{search_field, Block, BlockQuery, Refusal, SpecEvent, SpecLog, Step};
use crate::domain::spec_state::State;
use crate::platform::i18n::{translate, Locale};

/// O teto de turnos de uma onda com uma tarefa só: uma ida e volta do
/// modelo, que pode conter várias chamadas de ferramenta.
pub const SOLO_TASK_TURNS_CAP: u32 = 10;

/// O teto de turnos de uma onda com mais de uma tarefa.
pub const MULTI_TASK_TURNS_CAP: u32 = 15;

/// O teto de turnos da onda com `tasks` tarefas, para o cabeçalho do agente:
/// [`SOLO_TASK_TURNS_CAP`] com uma tarefa só, [`MULTI_TASK_TURNS_CAP`] com
/// mais de uma. O corte é da própria plataforma, pelo campo `maxTurns` do
/// molde do agente — o binário só escolhe o número e o escreve lá; o que
/// sobrou quando ela corta volta para a fila como tarefa nova.
#[must_use]
pub fn requested_turns(tasks: usize) -> u32 {
    if tasks <= 1 {
        SOLO_TASK_TURNS_CAP
    } else {
        MULTI_TASK_TURNS_CAP
    }
}

/// O nome do agente de onda a chamar, pelo número de tarefas do lote:
/// `"wave-solo"` para uma tarefa só, `"wave"` para várias. É o nome do
/// arquivo, sem a extensão, sob `.claude/agents/mustard/` — cada um já traz o
/// teto de turnos certo no próprio `maxTurns`, então escolher o arquivo é o
/// que fixa o corte; o binário não escreve mais o teto em memória.
#[must_use]
pub fn agent_role(tasks: usize) -> &'static str {
    if tasks <= 1 {
        "wave-solo"
    } else {
        "wave"
    }
}

/// O nome do agente que o molde `template` identifica, pelo campo `name` do
/// frontmatter dele: `mustard-wave-solo` vira `"wave-solo"`, `mustard-wave`
/// vira `"wave"`. Sem o campo, ou um nome fora do prefixo `mustard-`, volta
/// `"wave"` — o papel que a plataforma já aceitava antes dos dois moldes. É
/// como o reenvio, que não remonta o pedido, sabe qual dos dois o envio
/// original usou.
#[must_use]
pub fn agent_from_template(template: &str) -> String {
    template
        .lines()
        .find_map(|line| line.trim().strip_prefix("name:"))
        .map(str::trim)
        .and_then(|name| name.strip_prefix("mustard-"))
        .unwrap_or("wave")
        .to_string()
}

/// O modelo pedido para o papel `role` (`wave`, `wave-solo`, `review` ou
/// `skill`) no envio: a onda que implementa sai em Sonnet 5, tarefa única ou
/// várias; a revisão e o agente de teste dedicado, em Opus 5. Quem manda isso
/// é o binário, no próprio pedido — sem escolha explícita, a onda herda o
/// modelo da sessão e a decisão morre em silêncio.
#[must_use]
pub fn requested_model(role: &str) -> &'static str {
    match role {
        "wave" | "wave-solo" => "Sonnet 5",
        _ => "Opus 5",
    }
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

/// Uma cópia separada do repositório e a pasta de compilação em que ela
/// compila.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WaveCopy {
    /// A pasta da cópia.
    pub path: String,
    /// A pasta de compilação fixa, que passa de uma cópia para a seguinte.
    /// Toda onda recebe uma, porque ela é também a vaga que conta quantas
    /// ondas rodam juntas; o pedido só a cita quando o projeto é Rust
    /// ([`Execution::rust`]).
    pub build_dir: Option<String>,
}

/// As regras da execução que o pedido leva, lidas do projeto e da rodada.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Execution {
    /// O comando que compila o projeto, quando ele declara um.
    pub build: Option<String>,
    /// O comando que roda os testes do projeto, quando ele declara um.
    pub test: Option<String>,
    /// As outras ondas em andamento, cada uma com os arquivos das tarefas
    /// dela: o arquivo dividido com elas é juntado na volta.
    pub running: Vec<(u64, Vec<String>)>,
    /// O commit da onda, em que a revisão cria a cópia separada; sem ele, a
    /// cópia sai do commit atual.
    pub commit: Option<String>,
    /// O repositório principal: onde a spec mora e onde nada é editado.
    pub root: String,
    /// A cópia que a rodada criou para a onda; sem ela, o pedido não fala de
    /// cópia.
    pub copy: Option<WaveCopy>,
    /// A cópia em que o revisor da onda trabalha.
    pub review: WaveCopy,
    /// O mapa do projeto marca alguma parte dele como `cargo`. Só então o
    /// pedido traz a frase que manda compilar na pasta de compilação da cópia
    /// e cita o Cargo: num projeto Node, por exemplo, ela não serve.
    pub rust: bool,
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
/// como revisar numa cópia separada. Onda reprovada com o conserto já
/// entregue restringe `block`, `agreed` e `own_delivered` a ela: o agente
/// confere só o conserto, não a obra inteira de novo. O número da onda do
/// material não conta aqui.
#[must_use]
pub fn write_final_review(material: &Material, lang: Locale) -> String {
    Writer { material, lang }.final_review_text()
}

// ---------------------------------------------------------------------------
// Os códigos de cada bloco
// ---------------------------------------------------------------------------

/// As linhas de uma lista de itens: uma por bloco da spec, na ordem em que o
/// primeiro item de cada bloco aparece, com o nome do bloco — o que o comando
/// de leitura pede — e os códigos dos itens dele, em sequência. Nem o texto
/// do item nem o comando entram aqui: o comando está uma vez só no pedido.
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
}

/// O dono de cada item combinado, pelo número do item. O item sem dono não
/// entra: nenhuma tarefa de uma onda do plano o cobre, ele não diz uma onda
/// do plano em `waves` e não vale no projeto todo.
///
/// O item do projeto é o que diz, em `applies_to`, que vale no projeto todo:
/// a mesma leitura que acha a lição do projeto todo. A busca por palavras não
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
        }
    }
    out
}

/// Os itens combinados que vão no pedido da onda `wave`, como a montagem os
/// escolhe antes da análise: os de que ela é dona e os do projeto. O item sem
/// dono não entra aqui; só a análise antes do envio ([`dispatch_items`]) pode
/// pô-lo num pedido.
#[must_use]
pub fn agreed_for(log: &SpecLog, wave: u64) -> Vec<&SpecEvent> {
    let owners = owners(log);
    agreed_items(log)
        .into_iter()
        .filter(|item| match owners.get(&item.id) {
            Some(Owner::Project) => true,
            Some(Owner::Waves(waves)) => waves.contains(&wave),
            None => false,
        })
        .collect()
}

/// Os itens combinados sem dono, em ordem de número: os que a análise antes
/// do envio julga para cada onda.
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
    /// Os do projeto todo que as tarefas da onda não fazem.
    pub project: Vec<&'a SpecEvent>,
    /// Os sem dono.
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
/// `wave`; as lições, que moram no banco, quem lê o banco põe à parte.
#[must_use]
pub fn candidates(log: &SpecLog, wave: u64) -> Candidates<'_> {
    let owners = owners(log);
    let done: BTreeSet<u64> = log
        .block(BlockQuery::Wave(wave))
        .into_iter()
        .filter(|e| e.event_type == "task")
        .flat_map(|task| task.ints("covers"))
        .filter_map(|id| log.current(id).map(|e| e.id))
        .collect();
    let mut out = Candidates::default();
    for item in agreed_items(log).into_iter().filter(|item| !done.contains(&item.id)) {
        match owners.get(&item.id) {
            Some(Owner::Project) => out.project.push(item),
            None => out.unowned.push(item),
            Some(Owner::Waves(_)) => {}
        }
    }
    out
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
/// agora: os critérios que elas cobrem (`covers`, sem repetir), o texto
/// delas juntado por espaço e o pronto-quando — a prova de cada critério
/// coberto, ligada por " && ", ou, sem prova nenhuma (o caso do item
/// combinado sem dono, que não tem prova), o próprio texto das tarefas. A
/// formação do lote e a atualização dele depois de uma tarefa sair pela
/// cesta usam esta mesma conta, sobre as tarefas que a leitura de agora
/// mostra, para as duas nunca discordarem.
pub struct BasketFields {
    pub criteria: Vec<u64>,
    pub text: String,
    pub done_when: String,
}

/// Calcula [`BasketFields`] a partir das tarefas `tasks` de um lote, lendo em
/// `log` a prova de cada critério que elas cobrem.
#[must_use]
pub fn basket_fields(log: &SpecLog, tasks: &[&SpecEvent]) -> BasketFields {
    let mut criteria: BTreeSet<u64> = BTreeSet::new();
    let mut text_parts: Vec<String> = Vec::new();
    for task in tasks {
        criteria.extend(task.ints("covers"));
        if let Some(text) = task.str_field("text") {
            text_parts.push(text.to_string());
        }
    }
    let criteria: Vec<u64> = criteria.into_iter().collect();
    let proof =
        criteria.iter().filter_map(|id| log.get(*id)).filter_map(|event| event.str_field("proof")).collect::<Vec<_>>().join(" && ");
    let done_when = if proof.is_empty() { text_parts.join(" ") } else { proof };
    BasketFields { criteria, text: text_parts.join(" "), done_when }
}

/// A gravação de um item combinado novo depois da aprovação: ele nasce com
/// dono. Olha o arquivo antes e depois da gravação; o item que já existia, a
/// spec ainda não aprovada e a gravação de outro tipo passam.
///
/// A onda que o item diz em `waves` vale mesmo antes de estar no plano: a
/// decisão costuma vir antes da onda que a faz, e a tarefa entra numa onda no
/// replanejamento. Até lá, só a análise antes do envio pode pô-lo num pedido.
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
    let declared = |item: &SpecEvent| item.ints("waves").iter().any(|n| *n > 0);
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

/// `true` quando o arquivo que um texto cita é o arquivo de uma tarefa: o
/// mesmo caminho ou o fim dele (`spec_events/mod.rs`).
fn same_file(cited: &str, file: &str) -> bool {
    file == cited || file.ends_with(&format!("/{cited}"))
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

/// `true` quando um texto casa com a onda `n`: pela busca por palavras sobre
/// todas as ondas do plano, a nota dessa onda não fica abaixo da média das
/// notas das outras. Ela confere se uma tarefa está na onda certa; quem recebe
/// cada item combinado é o dono dele, não a busca.
///
/// Ter uma raiz em comum com a onda não basta: quase toda tarefa tem uma raiz
/// em comum com quase toda onda, e aí qualquer onda serviria. A nota da onda
/// da tarefa é posta contra as das outras, e a que fica abaixo da média não
/// casa.
///
/// É uma pergunta sobre uma onda só, e a resposta é sim ou não. Não é uma
/// disputa em que uma das ondas vence e todas as outras perdem: essa outra
/// pergunta é a de [`closest_wave`], e serve só para dizer para onde um texto
/// iria. A onda que o plano não tem responde que sim, porque a recusa dela é
/// outra e não sai daqui.
#[must_use]
pub fn matches_wave(log: &SpecLog, n: u64, text: &str) -> bool {
    let docs = wave_docs(log);
    if !docs.iter().any(|(number, _)| *number == n) {
        return true;
    }
    let hits = wave_scores(&docs, text);
    let mine = hits.iter().find(|hit| hit.id == n).map_or(0, |hit| hit.score);
    let others: u64 = hits.iter().filter(|hit| hit.id != n).map(|hit| hit.score).sum();
    let count = u64::try_from(docs.len() - 1).unwrap_or(u64::MAX);
    mine > 0 && mine.saturating_mul(count) >= others
}

/// A onda cujo texto casa mais forte com um texto, entre as do plano.
/// `None` quando ele não casa com onda nenhuma, e aí não há para onde apontar.
#[must_use]
pub fn closest_wave(log: &SpecLog, text: &str) -> Option<u64> {
    wave_scores(&wave_docs(log), text).first().map(|hit| hit.id)
}

/// A nota de cada onda que casa com um texto, da mais forte para a mais fraca,
/// pela mesma busca do recorte dos itens. A onda que não casa fica de fora.
fn wave_scores(docs: &[(u64, String)], text: &str) -> Vec<search::Hit> {
    search::SearchIndex::build(docs.iter().map(|(n, roots)| (*n, roots.as_str())))
        .top(&search::query_terms(text), docs.len())
}

/// O texto de cada onda do plano, reduzido para a busca.
fn wave_docs(log: &SpecLog) -> Vec<(u64, String)> {
    log.block(BlockQuery::Block(Block::Waves))
        .into_iter()
        .filter(|event| event.event_type == "wave")
        .filter_map(|event| Some((event.wave()?, search_field(event.str_field("text"), &[]))))
        .collect()
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
        self.read_example(&mut out, m.execution.copy.is_some());
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
    /// obra: as instruções fixas dele, o exemplo de leitura, o conserto —
    /// quando alguma onda voltou reprovada e já entregou de novo, só ele, sem
    /// pedir a obra inteira outra vez —, as ondas com as tarefas, as emendas
    /// gravadas para elas, o que cada uma entregou, os critérios, os commits
    /// que já entraram na branch e como revisar numa cópia separada.
    fn final_review_text(&self) -> String {
        let m = self.material;
        let mut out = String::new();
        let _ = writeln!(out, "# {}\n", self.t("prompt.final.title").replace("{spec}", &m.spec));
        out.push_str(self.t("prompt.final.fixed"));
        out.push_str("\n\n");
        self.read_example(&mut out, true);
        self.fix(&mut out, "prompt.fix.final");
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

    /// O que a onda entrega: a frase do `done_when` do evento da onda, texto
    /// original — é o único texto de item que este pedido copia, porque é
    /// justamente o que abre o trabalho. Vem como parágrafo de abertura, sem
    /// título próprio, antes dos itens da onda. Sem onda no material, nada.
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
    /// nomeiam. Cada bloco da spec sai em sua própria linha de códigos, como
    /// [`codes_by_block`] os agrupa; lições e skills mantêm a listagem
    /// própria, uma linha por item, porque carregam mais que um código. Nem
    /// o texto do item nem o comando de leitura entram aqui — o comando está
    /// uma vez só no pedido, em [`Self::read_example`].
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
            for line in codes_by_block(m, &m.fix) {
                let _ = writeln!(out, "{line}");
            }
        }
        for line in codes_by_block(m, &wave_only) {
            let _ = writeln!(out, "{line}");
        }
        for line in codes_by_block(m, &m.criteria) {
            let _ = writeln!(out, "{line}");
        }
        for line in codes_by_block(m, &m.specification) {
            let _ = writeln!(out, "{line}");
        }
        for line in codes_by_block(m, &m.agreed) {
            let _ = writeln!(out, "{line}");
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
    /// uma linha por tarefa, com o código, os arquivos que ela cita e — para
    /// a que ganhou leitura obrigatória ou escolhida pelo orquestrador — o
    /// que precisa ler antes, pelo mesmo trecho que [`Self::task_reads`]
    /// calcula. Abaixo da linha, um arquivo que o mapa do projeto conhece os
    /// testes ([`Material::file_tests`]) ganha uma linha própria com eles.
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
            if !files.is_empty() {
                let _ = write!(out, ": {}", files.join(", "));
            }
            if let Some((_, reads)) = self.material.task_reads.iter().find(|(t, _)| *t == code) {
                let parts: Vec<String> = reads.iter().map(|file| self.read_hint(file)).collect();
                let _ = write!(out, " — {}: {}", self.t("prompt.task.read_before"), parts.join(", "));
            }
            let _ = writeln!(out);
            for path in paths {
                let Some(tests) = self.material.file_tests.get(path) else { continue };
                let list = tests.iter().map(|test| format!("`{test}`")).collect::<Vec<_>>().join(", ");
                let line = self.t("prompt.task.tested_by").replace("{file}", path).replace("{tests}", &list);
                let _ = writeln!(out, "  - {line}");
            }
        }
        out.push('\n');
    }

    /// O exemplo único do comando que lê um item, com o nome da spec. Leva o
    /// caminho do repositório principal quando o agente trabalha numa cópia
    /// (`in_copy`): de dentro dela, a spec só se lê por lá. Sem cópia, o
    /// agente roda no próprio repositório principal, e o caminho sobra.
    fn read_example(&self, out: &mut String, in_copy: bool) {
        let root = &self.material.execution.root;
        let flag = if in_copy && !root.is_empty() { format!("--root {root} ") } else { String::new() };
        let line = self.t("prompt.read").replace("{root}", &flag).replace("{spec}", &self.material.spec);
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

    /// As linhas do conserto: o título, o que fazer com elas (`intro`: o do
    /// agente da onda ou o do revisor) e uma linha por bloco da spec, com os
    /// códigos em sequência. Fora de um conserto, nada.
    fn fix(&self, out: &mut String, intro: &str) {
        if self.material.fix.is_empty() {
            return;
        }
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.fix"));
        let _ = writeln!(out, "{}\n", self.t(intro));
        for line in codes_by_block(self.material, &self.material.fix) {
            let _ = writeln!(out, "{line}");
        }
        out.push('\n');
    }

    /// As regras da execução do agente da onda que carregam valor deste
    /// projeto e desta rodada — a cópia separada, a pasta de compilação num
    /// projeto Rust, os comandos do projeto e as outras ondas em andamento,
    /// com os arquivos delas — e, ligadas à cópia, as três frases que dizem
    /// com todas as letras que o agente não comita, que o campo `commit` do
    /// relatório é o título, nunca o código do commit, e que a última
    /// mensagem tem só a linha `<DELIVERED>` e a de gasto. O resto (ler por
    /// trecho, a suíte uma vez no fim…) já mora no molde do agente, e não
    /// repete aqui. De onde ler a spec, o exemplo de leitura já diz.
    fn execution(&self, out: &mut String) {
        let execution = &self.material.execution;
        let running = &execution.running;
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.execution"));
        if let Some(copy) = &execution.copy {
            let line = self.t("prompt.execution.copy").replace("{copy}", &copy.path).replace("{root}", &execution.root);
            let _ = writeln!(out, "- {line}");
            self.build_dir(out, copy);
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

    /// As regras da execução do revisor: criar a cópia que o pedido indica no
    /// commit da onda, compilar na pasta de compilação dela num projeto Rust,
    /// os comandos do projeto com menos processos, não comitar e apagar a
    /// cópia no fim. De onde ler a spec, o exemplo de leitura já diz.
    fn review_execution(&self, out: &mut String) {
        let execution = &self.material.execution;
        let (copy, root) = (&execution.review, &execution.root);
        let commit = execution.commit.as_deref().unwrap_or("HEAD");
        let _ = writeln!(out, "## {}\n", self.t("prompt.part.execution"));
        let line = self.t("prompt.review.copy").replace("{copy}", &copy.path).replace("{root}", root);
        let _ = writeln!(out, "- {}", line.replace("{commit}", commit));
        self.build_dir(out, copy);
        self.commands(out);
        let _ = writeln!(out, "- {}", self.t("prompt.review.jobs"));
        let _ = writeln!(out, "- {}", self.t("prompt.review.cleanup").replace("{copy}", &copy.path));
        out.push('\n');
    }

    /// A pasta de compilação da cópia, quando ela tem uma e o projeto é Rust:
    /// a frase cita o Cargo, e fora dele não serve.
    fn build_dir(&self, out: &mut String, copy: &WaveCopy) {
        if !self.material.execution.rust {
            return;
        }
        if let Some(dir) = &copy.build_dir {
            let _ = writeln!(out, "- {}", self.t("prompt.execution.build_dir").replace("{dir}", dir));
        }
    }

    /// Os comandos de compilar e de testar que o projeto declara, um por
    /// linha; o que ele não declara não aparece.
    fn commands(&self, out: &mut String) {
        let execution = &self.material.execution;
        for (key, command) in [("prompt.execution.build", &execution.build), ("prompt.execution.test", &execution.test)] {
            if let Some(command) = command {
                let _ = writeln!(out, "- {}", self.t(key).replace("{command}", command));
            }
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
    /// escolha de novo só por causa do formato antigo (`MSTD-TASK-0016`,
    /// `MSTD-DEC-0009`).
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

    /// O pedido leva a lista, não o texto: cada parte traz uma linha por
    /// bloco da spec, com o nome do bloco e os códigos dos itens em
    /// sequência, e o comando de leitura aparece uma vez só, no exemplo. As
    /// duas exceções que copiam texto são a frase do `done_when`, que abre o
    /// pedido, e o arquivo de cada tarefa; o resto — o texto da onda, o da
    /// tarefa e o do critério — não é copiado, e nenhum código aparece duas
    /// vezes.
    #[test]
    fn a_request_lists_only_the_codes_of_each_block_and_one_reading_example() {
        let log = log(&[
            (
                "criterion",
                json!({"when": "a onda roda", "then": "a suíte passa", "proof": "cargo test"}),
            ),
            (
                "wave",
                json!({"n": 1, "text": "Primeira onda", "criteria": [1], "done_when": "a onda termina"}),
            ),
            ("task", json!({"wave": 1, "text": "Escrever o motor", "files": [{"path": "src/a.rs"}]})),
            ("task", json!({"wave": 1, "text": "Escrever a página", "files": [{"path": "src/b.rs"}]})),
        ]);
        for lang in [Locale::PtBr, Locale::EnUs] {
            let prompt = build(&material(&log, 1), lang);
            for text in ["Primeira onda", "Escrever o motor", "Escrever a página", "a suíte passa", "cargo test"] {
                assert!(!prompt.text.contains(text), "{text:?} foi copiado: {}", prompt.text);
            }
            assert!(prompt.text.contains("a onda termina"), "o done_when abre o pedido: {}", prompt.text);
            assert_eq!(
                listed(&prompt.text, translate("prompt.part.items", lang)),
                ["- `waves`: MSTD-WAVE-0001", "- `criteria`: MSTD-CRIT-0001"],
                "{}",
                prompt.text
            );
            let tasks = section(&prompt.text, translate("prompt.part.tasks", lang));
            assert!(tasks.contains("MSTD-TASK-0001") && tasks.contains("`src/a.rs`"), "{tasks}");
            assert!(tasks.contains("MSTD-TASK-0002") && tasks.contains("`src/b.rs`"), "{tasks}");
            let example = translate("prompt.read", lang).replace("{root}", "").replace("{spec}", "teste");
            assert!(prompt.text.contains(&example), "{}", prompt.text);
            assert_eq!(prompt.text.matches("mustard-rt run read").count(), 1, "{}", prompt.text);
            assert_eq!(prompt.text.matches("--term").count(), 1, "{}", prompt.text);
            for code in ["MSTD-WAVE-0001", "MSTD-TASK-0001", "MSTD-TASK-0002", "MSTD-CRIT-0001"] {
                assert_eq!(prompt.text.matches(code).count(), 1, "{code}: {}", prompt.text);
            }
            assert!(prompt.lines > 0 && prompt.lines == prompt.text.lines().count());
        }
    }

    /// O critério da montagem, provado de uma vez, não espalhado: o pedido
    /// abre pelo `done_when`, antes das tarefas; as tarefas saem na ordem de
    /// execução que a onda declara (`order`); o cabeçalho diz o modelo da
    /// onda; nenhuma das seis frases de execução que o molde do agente já dá
    /// aparece; e o que sobra da execução é só o desta rodada e deste
    /// projeto — a cópia, a pasta de compilação, os comandos do projeto e a
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
            rust: true,
            running: vec![(9, vec!["src/c.rs".into()])],
            copy: Some(WaveCopy { path: "/copia".into(), build_dir: Some("/build".into()) }),
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
        for kept in ["/copia", "/build", "cargo build", "cargo test", "Onda 9", "`src/c.rs`"] {
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
    /// pedido não corta onda grande, quem corta é o teto de turnos do
    /// próprio agente.
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

    /// Depois da aprovação, o item combinado novo nasce com dono: a onda que
    /// ele diz, o projeto todo ou a tarefa que já cobria a versão antiga. Sem
    /// dono, é recusado; antes da aprovação, passa.
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
            assert!(message.contains("decision") && message.contains("waves") && message.contains("**"), "{message}");
        }
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
    /// que nenhuma resolve. O item do projeto e o que já tem dono ficam fora.
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
                (13, waves(&[1]), OwnerFrom::Files(vec!["src/**".into()])),
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

    /// O item combinado escolhido para a onda entra pelo código, na linha do
    /// bloco dele; o texto e o exemplo dele nunca entram.
    #[test]
    fn every_agreed_item_comes_as_a_line_and_never_as_text() {
        let plan = plan();
        let prompt = build(&with_agreed(&plan, 1), Locale::PtBr);
        for text in ["A barra de status mostra o link", "duas linhas", "A página do relatório", "um motor só"] {
            assert!(!prompt.text.contains(text), "{text:?} foi copiado: {}", prompt.text);
        }
        assert!(
            listed(&prompt.text, translate("prompt.part.items", Locale::PtBr))
                .contains(&"- `agreed`: MSTD-RULE-0003, MSTD-RULE-0005"),
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
                json!({"text": "Nenhuma onda fecha com a suíte vermelha", "keys": ["suíte"],
                       "example": "a onda para", "applies_to": {"files": ["**"]}}),
            ),
            ("wave", json!({"n": 1, "text": "Leitura", "criteria": [], "done_when": "lê"})),
            ("task", json!({"wave": 1, "text": "Escrever o leitor", "files": [{"path": "src/a.rs"}]})),
            ("wave", json!({"n": 2, "text": "Página", "criteria": [], "done_when": "sai"})),
            ("task", json!({"wave": 2, "text": "Gravar a página", "files": [{"path": "src/b.rs"}]})),
        ]);
        for wave in [1, 2] {
            let prompt = build(&with_agreed(&log, wave), Locale::PtBr);
            let agreed = listed(&prompt.text, translate("prompt.part.items", Locale::PtBr));
            assert!(agreed.contains(&"- `agreed`: MSTD-RULE-0001"), "onda {wave}: {}", prompt.text);
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

    /// Ler o item pelo número é parte do trabalho, e quem diz isso é o texto
    /// do agente da onda, que ele carrega uma vez; a parte fixa do pedido não
    /// repete a instrução nem traz a proibição antiga de procurar o resto em
    /// outro arquivo.
    #[test]
    fn the_wave_agent_says_that_reading_the_item_by_its_number_is_part_of_the_work() {
        for (lang, reading, forbidden) in [
            (Locale::PtBr, "Ler o item pelo número é parte do trabalho", "nunca vá procurar o resto em outro arquivo"),
            (Locale::EnUs, "Reading the item by its number is part of the work", "never go looking for the rest in another file"),
        ] {
            let (_, agent) = crate::platform::seeds::agent_texts(lang)[0];
            assert!(agent.contains(reading), "{agent}");
            let fixed = translate("prompt.fixed", lang);
            assert!(!fixed.contains(reading) && !fixed.contains(forbidden), "{fixed}");
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

    /// O pedido do conserto (o da própria onda) e o do agente de teste final
    /// trazem as mesmas linhas do conserto, cada um com o que fazer com elas:
    /// consertar só isso, e olhar só o conserto. O pedido da onda leva essas
    /// linhas dentro dos itens da onda, sem título próprio — junto com o
    /// resto que abre o trabalho; o do agente de teste final mantém o título
    /// próprio de antes, porque é onde ele confere o conserto sozinho, sem
    /// pedir a obra inteira outra vez. Fora de um conserto, nenhum dos dois
    /// pedidos traz a frase de abertura do conserto.
    #[test]
    fn the_final_review_request_is_unchanged() {
        let log = rejected(false);
        let mut m = material(&log, 1);
        m.fix = fix_lines(&log, 1);
        for lang in [Locale::PtBr, Locale::EnUs] {
            let fix_intro = translate("prompt.fix.wave", lang);
            let items_heading = translate("prompt.part.items", lang);
            let fix_heading = translate("prompt.part.fix", lang);
            let wave = write(&m, lang);
            let last = write_final_review(&m, lang);
            let items = section(&wave, items_heading);
            assert!(items.contains(fix_intro), "{wave}");
            for code in ["MSTD-VERD-0001", "MSTD-DELIV-0001", "MSTD-DEC-0003", "MSTD-RULE-0001"] {
                assert!(items.contains(code), "{code}: {wave}");
            }
            assert!(!items.contains("Falta o teste"), "nenhum texto é copiado: {items}");
            assert!(section(&last, fix_heading).contains(translate("prompt.fix.final", lang)), "{last}");
            let fix = section(&last, fix_heading);
            assert_eq!(
                listed(&last, fix_heading),
                [
                    "- `review`: MSTD-VERD-0001",
                    "- `waves`: MSTD-DELIV-0001",
                    "- `agreed`: MSTD-DEC-0003, MSTD-RULE-0001",
                ],
                "{fix}"
            );
            assert!(!fix.contains("Falta o teste"), "nenhum texto é copiado: {fix}");
        }
        let plain = material(&log, 1);
        assert!(!write(&plain, Locale::PtBr).contains(translate("prompt.fix.wave", Locale::PtBr)));
        assert!(section(&write_final_review(&plain, Locale::PtBr), "Conserto").is_empty());
    }

    /// A execução de um pedido montado com a cópia que a rodada criou, num
    /// projeto Rust.
    fn with_copy() -> Execution {
        Execution {
            build: Some("make".into()),
            test: Some("make test".into()),
            running: vec![(2, vec!["src/b.rs".into(), "src/c.rs".into()]), (3, Vec::new())],
            commit: Some("abc1234".into()),
            root: "/repo".into(),
            copy: Some(WaveCopy { path: "/repo/copia-1".into(), build_dir: Some("/repo/target/copias/a".into()) }),
            review: WaveCopy { path: "/repo/revisao-1".into(), build_dir: Some("/repo/target/copias/b".into()) },
            rust: true,
        }
    }

    /// Num projeto sem parte Rust, a cópia recebe a pasta de compilação do
    /// mesmo jeito, porque ela é a vaga das ondas que rodam juntas, mas
    /// nenhum dos dois pedidos a cita nem fala do Cargo; a cópia e o resto
    /// das regras continuam. Num projeto Rust, os dois citam a pasta.
    #[test]
    fn the_build_folder_sentence_is_written_only_for_a_rust_project() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let mut m = material(&log, 1);
        for lang in [Locale::PtBr, Locale::EnUs] {
            let rules = translate("prompt.part.execution", lang);
            for rust in [false, true] {
                m.execution = Execution { rust, ..with_copy() };
                let wave = section(&write(&m, lang), rules).to_string();
                let last = section(&write_final_review(&m, lang), rules).to_string();
                for (text, folder) in [(&wave, "/repo/target/copias/a"), (&last, "/repo/target/copias/b")] {
                    let sentence = translate("prompt.execution.build_dir", lang).replace("{dir}", folder);
                    assert_eq!(text.contains(&sentence), rust, "{lang:?} rust={rust}: {text}");
                    assert_eq!(text.contains("Cargo") || text.contains("target/copias"), rust, "{lang:?} rust={rust}: {text}");
                }
                assert!(wave.contains("`/repo/copia-1`") && last.contains("`/repo/revisao-1`"), "{wave}\n{last}");
                assert!(wave.contains("`make test`") && last.contains("`make test`"), "{wave}\n{last}");
            }
        }
    }

    /// O pedido da onda traz as regras da execução: a cópia separada que a
    /// rodada criou, a pasta de compilação dela, os comandos do projeto, não
    /// comitar e as outras ondas em andamento com os arquivos delas; o
    /// caminho do repositório principal vem só no exemplo de leitura. O da
    /// revisão final diz em que cópia trabalhar, como criá-la no commit mais
    /// novo, onde compilar, compilar com menos processos e apagar a cópia no
    /// fim; sem commit, a cópia sai do atual. Sem cópia, o pedido da onda não
    /// fala de cópia, de pasta de compilação nem do repositório principal.
    #[test]
    fn the_requests_carry_the_execution_rules_the_copy_and_its_build_folder() {
        let log = log(&[("wave", json!({"n": 1, "text": "Onda", "criteria": [], "done_when": "pronto"}))]);
        let mut m = material(&log, 1);
        m.execution = with_copy();
        let t = |key: &str| translate(key, Locale::PtBr);
        let wave = write(&m, Locale::PtBr);
        let rules = section(&wave, t("prompt.part.execution"));
        for line in [
            format!("- {}", t("prompt.execution.copy").replace("{copy}", "/repo/copia-1").replace("{root}", "/repo")),
            format!("- {}", t("prompt.execution.build_dir").replace("{dir}", "/repo/target/copias/a")),
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
        assert!(!rules.contains("worktree") && !rules.contains("revisao-1"), "{rules}");
        // De onde ler a spec, só o exemplo de leitura diz, uma vez.
        let example = t("prompt.read").replace("{root}", "--root /repo ").replace("{spec}", "teste");
        assert!(wave.contains(&example) && !rules.contains("--root"), "{wave}");
        assert_eq!(wave.matches("--root").count(), 1, "{wave}");

        let last = write_final_review(&m, Locale::PtBr);
        assert!(last.contains(&example), "{last}");
        assert_eq!(last.matches("--root").count(), 1, "{last}");
        let rules = section(&last, t("prompt.part.execution"));
        for line in [
            "`git worktree add --detach /repo/revisao-1 abc1234`",
            "`CARGO_TARGET_DIR=/repo/target/copias/b`",
            t("prompt.review.jobs"),
            "`git worktree remove --force /repo/revisao-1`",
            "- Compile com `make`.",
        ] {
            assert!(rules.contains(line), "{line}: {rules}");
        }
        assert!(!rules.contains("Onda 2") && !rules.contains("copia-1"), "a revisão roda na cópia dela: {rules}");
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
        assert!(write_final_review(&m, Locale::PtBr).contains("--detach  HEAD`"));
        assert!(write_final_review(&m, Locale::PtBr).contains(&example), "o revisor trabalha sempre numa cópia");
        let en = write(&Material { execution: with_copy(), ..material(&log, 1) }, Locale::EnUs);
        let rules = section(&en, translate("prompt.part.execution", Locale::EnUs));
        assert!(rules.contains("`/repo/copia-1`") && rules.contains("`/repo/target/copias/a`"), "{rules}");
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

    /// Um texto casa com a onda dele quando a nota dela não fica abaixo da
    /// média das notas das outras. Uma raiz em comum não basta: o texto que
    /// divide uma palavra com a onda dele e casa mais com as outras não casa.
    /// O que não casa com onda nenhuma não tem para onde ir, e a onda que o
    /// plano não tem responde que sim.
    #[test]
    fn a_text_fits_its_wave_only_when_it_scores_at_least_the_average_of_the_others() {
        let log = log(&[
            ("wave", json!({"n": 1, "text": "Leitura do arquivo de eventos", "criteria": [], "done_when": "lê"})),
            ("wave", json!({"n": 2, "text": "Página do relatório", "criteria": [], "done_when": "sai"})),
            ("wave", json!({"n": 3, "text": "Publicação da página do relatório", "criteria": [], "done_when": "sai"})),
        ]);
        let shared = "Gravar a página do relatório ao lado do arquivo";
        let docs = wave_docs(&log);
        let scores = wave_scores(&docs, shared);
        let score = |n: u64| scores.iter().find(|hit| hit.id == n).map_or(0, |hit| hit.score);
        assert!(score(1) > 0, "o texto tem uma raiz em comum com a onda 1: {scores:?}");
        assert!(2 * score(1) < score(2) + score(3), "e casa menos com ela do que com as outras: {scores:?}");
        assert!(!matches_wave(&log, 1, shared), "a raiz em comum não basta");
        assert!(matches_wave(&log, 2, shared));
        assert!(matches_wave(&log, 1, "Ler o arquivo de eventos"));

        assert!(!matches_wave(&log, 1, "Somar dois números"));
        assert_eq!(closest_wave(&log, "Somar dois números"), None);
        assert!(matches_wave(&log, 9, "Somar dois números"), "a onda que o plano não tem responde que sim");

        let alone = self::log(&[("wave", json!({"n": 1, "text": "Leitura do arquivo", "criteria": [], "done_when": "lê"}))]);
        assert!(matches_wave(&alone, 1, shared), "com uma onda só, a raiz em comum basta");
    }
}
