//! A leitura do arquivo: cada linha vira um evento, a linha que não se
//! entende é pulada pelo número, e a leitura mostra só o que não saiu — por
//! bloco, por onda e por passo do fluxo.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::platform::i18n::{Locale, translate};

use super::check::GIVES_BACK_FIELD;
use super::search::found_by;
use super::{Block, BlockQuery, CUT_LINE_TYPE, FORMAT_VERSION, METRIC_TYPES, PURGED_FIELD, render_line, shown_line, type_spec};
use crate::domain::normalize::Languages;

/// Um evento lido do arquivo.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecEvent {
    pub id: u64,
    pub event_type: String,
    /// O número da linha no arquivo, a partir de 1.
    pub line: usize,
    /// A linha inteira, como foi lida.
    pub fields: Map<String, Value>,
}

impl SpecEvent {
    /// A data e a hora do evento.
    #[must_use]
    pub fn at(&self) -> &str {
        self.str_field("at").unwrap_or_default()
    }

    /// Um campo de texto.
    #[must_use]
    pub fn str_field(&self, field: &str) -> Option<&str> {
        self.fields.get(field).and_then(Value::as_str)
    }

    /// Um campo de número.
    #[must_use]
    pub fn int(&self, field: &str) -> Option<u64> {
        self.fields.get(field).and_then(Value::as_u64)
    }

    /// Um campo de lista de números; vazio quando falta.
    #[must_use]
    pub fn ints(&self, field: &str) -> Vec<u64> {
        ints(self.fields.get(field))
    }

    /// Os números que esta versão substitui (`replaces`): um só, no item da
    /// spec e na versão nova de uma lição, ou vários, na lição que junta
    /// outras numa só. Vazio quando o evento não substitui nada.
    #[must_use]
    pub fn replaced(&self) -> Vec<u64> {
        self.int("replaces").map_or_else(|| self.ints("replaces"), |old| vec![old])
    }

    /// `true` na volta que o próprio agente gravou (`returned`), que só a
    /// rodada ou o fechamento assume.
    #[must_use]
    pub fn returned(&self) -> bool {
        self.fields.get("returned") == Some(&Value::Bool(true))
    }

    /// O bloco do tipo; `None` para um tipo que este binário não conhece.
    #[must_use]
    pub fn block(&self) -> Option<Block> {
        type_spec(&self.event_type).map(|t| t.block)
    }

    /// O número da onda a que o evento pertence: `n` na onda, `wave` na
    /// tarefa, no envio, no entregou e no veredito.
    #[must_use]
    pub fn wave(&self) -> Option<u64> {
        wave_of(&self.event_type, |field| self.int(field))
    }

    /// A linha como a leitura mostra, sem o `search`.
    #[must_use]
    pub fn shown(&self) -> String {
        shown_line(&self.fields)
    }
}

pub(super) fn ints(value: Option<&Value>) -> Vec<u64> {
    value.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default()
}

/// O número da onda a que uma linha pertence: `n` na onda, `wave` na tarefa,
/// no envio, no entregou e no veredito. `None` nos outros tipos.
pub(super) fn wave_of(event_type: &str, field: impl Fn(&str) -> Option<u64>) -> Option<u64> {
    match event_type {
        "wave" => field("n"),
        "task" | "send" | "delivered" | "verdict" | "step" | "validation_failure" => field("wave"),
        _ => None,
    }
}

/// Por que uma linha foi pulada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// A linha não é um objeto JSON com `id` e `type`: a máquina desligou no
    /// meio de uma gravação, ou alguém editou o arquivo à mão.
    Unreadable,
    /// A linha repete um número que outra linha acima já usa.
    DuplicateId(u64),
}

/// Uma linha que a leitura pulou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkippedLine {
    /// O número da linha no arquivo, a partir de 1.
    pub line: usize,
    pub reason: SkipReason,
    /// O número que a linha parece ter, para que a próxima gravação nunca o
    /// repita.
    pub id_hint: Option<u64>,
}

impl SkippedLine {
    /// O aviso, no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        match self.reason {
            SkipReason::Unreadable => translate("spec_events.skipped_line", lang).replace("{line}", &self.line.to_string()),
            SkipReason::DuplicateId(id) => {
                translate("spec_events.duplicate_id", lang).replace("{line}", &self.line.to_string()).replace("{id}", &id.to_string())
            }
        }
    }
}

/// Por que um evento some da leitura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hidden {
    /// Um `remove` o tirou; `by` é o número do `remove`.
    Removed { by: u64 },
    /// Uma versão nova, de número `by`, o substitui.
    Replaced { by: u64 },
    /// O expurgo de número `by`, no formato antigo, tirou o texto dele do
    /// arquivo.
    Purged { by: u64 },
    /// É a volta que o próprio agente gravou (`returned`): a entrega da onda
    /// ou o veredito do revisor, que só contam quando a rodada ou o
    /// fechamento grava a versão oficial.
    Returned,
}

/// A primeira remoção que aponta um evento: o número dela e se ela leva a
/// marca de `GIVES_BACK_FIELD`, que faz a versão removida devolver a
/// anterior.
#[derive(Debug, Clone, Copy)]
struct Removal {
    by: u64,
    gives_back: bool,
}

/// Um filtro de remoção: um tipo e um intervalo de horário, na hora local de
/// quem lê. Os dois extremos entram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeFilter {
    pub event_type: String,
    pub from: String,
    pub to: String,
}

impl TimeFilter {
    /// O filtro de um campo `filter`; `None` quando falta alguma parte.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        let get = |k: &str| value.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Self { event_type: get("type")?, from: get("from")?, to: get("to")? })
    }

    /// `true` quando o evento é do tipo e o horário cai no intervalo. Compara
    /// só a parte local do `at`, até a precisão de cada extremo:
    /// `2026-09-11T21:10` pega tudo o que aconteceu nesse minuto.
    #[must_use]
    pub fn matches(&self, event: &SpecEvent) -> bool {
        if event.event_type != self.event_type {
            return false;
        }
        let local = event.at().get(..19).unwrap_or(event.at());
        let cut = |bound: &str| local.get(..bound.len().min(19)).unwrap_or(local).to_string();
        let from = self.from.get(..self.from.len().min(19)).unwrap_or(&self.from);
        let to = self.to.get(..self.to.len().min(19)).unwrap_or(&self.to);
        cut(from).as_str() >= from && cut(to).as_str() <= to
    }
}

/// Um passo do fluxo que lê a spec. Cada passo lê só os blocos dele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Retomar: o estado.
    Resume,
    /// Despachar uma onda: o bloco da onda, os critérios dela, a
    /// especificação, que é do projeto, os itens combinados de que a onda ou
    /// o projeto são donos, o entregou das ondas de que ela depende e, no
    /// conserto, as linhas dele. Nunca a conversa.
    Dispatch { wave: u64 },
    /// Revisar uma onda: o bloco da onda, com o entregou dela, os critérios
    /// dela e, na revisão de um conserto, as linhas dele.
    Review { wave: u64 },
    /// Fechar: o estado e os critérios.
    Close,
    /// Tirar uma dúvida: a conversa, filtrada pelo termo, cortado nas
    /// línguas do projeto.
    Question { term: String, languages: Languages },
}

/// O arquivo lido: os eventos em ordem e as linhas puladas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpecLog {
    pub events: Vec<SpecEvent>,
    pub skipped: Vec<SkippedLine>,
    /// Os ocultos desta leitura, contados uma vez ([`SpecLog::hidden`]).
    memo: Memo,
}

/// O que a leitura esconde e as substituições que valem, contados juntos.
#[derive(Debug)]
struct Derived {
    hidden: BTreeMap<u64, Hidden>,
    replaced_by: BTreeMap<u64, u64>,
    /// A posição de cada evento que a leitura mostra, em ordem.
    visible: Vec<usize>,
    /// As posições dos que a leitura mostra, por bloco.
    by_block: BTreeMap<Block, Vec<usize>>,
    /// As posições das métricas que a leitura mostra.
    metrics: Vec<usize>,
}

/// A forma dos eventos quando a conta foi feita: os eventos que ganham,
/// perdem ou trocam de lugar mudam a forma, e a conta se refaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shape {
    len: usize,
    at: usize,
    first: u64,
    last: u64,
}

impl Shape {
    fn of(events: &[SpecEvent]) -> Self {
        Self { len: events.len(), at: events.as_ptr() as usize, first: events.first().map_or(0, |e| e.id), last: events.last().map_or(0, |e| e.id) }
    }
}

/// A conta guardada de uma leitura. A cópia da leitura começa sem conta, e
/// duas leituras se comparam só pelos eventos.
#[derive(Default)]
struct Memo(std::sync::Mutex<Option<(Shape, std::sync::Arc<Derived>)>>);

impl Clone for Memo {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for Memo {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Memo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Memo")
    }
}

/// Lê o conteúdo do arquivo. Nunca falha: a linha que não se entende é
/// pulada, com o número dela em [`SpecLog::skipped`], e o resto é lido. Uma
/// linha em branco não conta.
#[must_use]
pub fn parse_log(content: &str) -> SpecLog {
    let mut log = SpecLog::default();
    let mut seen = BTreeSet::new();
    for (i, raw) in content.split('\n').enumerate() {
        let line = i + 1;
        let raw = raw.trim_end_matches('\r');
        if raw.trim().is_empty() {
            continue;
        }
        let parsed = parse_object(raw).map(upgrade).and_then(|fields| {
            let id = fields.get("id").and_then(Value::as_u64).filter(|n| *n > 0)?;
            let event_type = fields.get("type").and_then(Value::as_str)?.to_string();
            Some(SpecEvent { id, event_type, line, fields })
        });
        match parsed {
            Some(event) if !seen.insert(event.id) => log.skipped.push(SkippedLine { line, reason: SkipReason::DuplicateId(event.id), id_hint: Some(event.id) }),
            Some(event) => log.events.push(event),
            None => log.skipped.push(SkippedLine { line, reason: SkipReason::Unreadable, id_hint: id_hint(raw) }),
        }
    }
    log
}

pub(super) fn parse_object(line: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(line).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Traz uma linha de uma versão anterior do formato para a atual. A linha sem
/// `v` é da primeira versão. Quando o formato mudar, a conversão da versão
/// anterior entra aqui, e as specs antigas continuam sendo lidas; uma linha de
/// versão mais nova que este binário é lida como está.
fn upgrade(line: Map<String, Value>) -> Map<String, Value> {
    line
}

/// O número que uma linha estragada parece ter: o que vem depois de `"id":`.
fn id_hint(raw: &str) -> Option<u64> {
    let at = raw.find("\"id\"")?;
    let rest = raw[at + 4..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// O arquivo `content`, lido como `log`, com cada linha cortada trocada, no
/// lugar, por um registro que se lê. Cortada é a linha que não se entende e
/// começa como um evento, `{"v":1,"id":N` com o número inteiro: o disco cheio
/// parou a gravação no meio dela. O registro é do tipo [`CUT_LINE_TYPE`], com
/// o mesmo número, o `code` e o `at` quando o pedaço os traz por inteiro, o
/// autor `binary` e o pedaço todo em `piece`. Nenhuma outra linha muda, nem um
/// byte, e nenhum evento muda de número. A linha que não começa assim fica
/// como está, e a que tem o número de um evento que se lê também: trocar uma
/// pela outra repetiria o número. `None` quando nenhuma linha pede o conserto.
#[must_use]
pub fn repair_cut_lines(content: &str, log: &SpecLog) -> Option<String> {
    let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
    let mut taken: BTreeSet<u64> = log.events.iter().map(|e| e.id).collect();
    let mut changed = false;
    for skipped in log.skipped.iter().filter(|s| s.reason == SkipReason::Unreadable) {
        let Some(slot) = skipped.line.checked_sub(1).and_then(|i| lines.get_mut(i)) else {
            continue;
        };
        let carriage_return = slot.ends_with('\r');
        let piece = slot.trim_end_matches('\r');
        let Some((id, record)) = cut_record(piece) else {
            continue;
        };
        if !taken.insert(id) {
            continue;
        }
        let mut repaired = render_line(&record);
        if carriage_return {
            repaired.push('\r');
        }
        *slot = repaired;
        changed = true;
    }
    changed.then(|| lines.join("\n"))
}

/// O número e o registro que tomam o lugar da linha cortada `piece`; `None`
/// quando ela não começa como um evento ou quando o número pode estar
/// incompleto — o pedaço que para logo depois dos dígitos pode ter perdido
/// algum, e o número errado repetiria o de outro evento.
fn cut_record(piece: &str) -> Option<(u64, Map<String, Value>)> {
    let rest = piece.strip_prefix(&format!("{{\"v\":{FORMAT_VERSION},\"id\":"))?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits == rest.len() {
        return None;
    }
    let id = rest[..digits].parse::<u64>().ok().filter(|n| *n > 0)?;
    let mut tail = &rest[digits..];
    let mut record = Map::new();
    record.insert("v".into(), Value::from(FORMAT_VERSION));
    record.insert("id".into(), Value::from(id));
    if let Some(code) = leading_string(&mut tail, "code").filter(|code| crate::domain::mustard_id::is_id(code)) {
        record.insert("code".into(), Value::String(code));
    }
    if let Some(at) = leading_string(&mut tail, "at").filter(|at| chrono::DateTime::parse_from_rfc3339(at).is_ok()) {
        record.insert("at".into(), Value::String(at));
    }
    record.insert("type".into(), Value::from(CUT_LINE_TYPE));
    record.insert("author".into(), Value::from("binary"));
    record.insert("piece".into(), Value::String(piece.to_string()));
    Some((id, record))
}

/// O valor de texto do campo `key` quando `tail` começa por ele
/// (`,"key":"valor"`), com o fecho das aspas dentro do pedaço; `tail` avança
/// para depois dele. Um valor com barra ou cortado no meio não conta.
fn leading_string(tail: &mut &str, key: &str) -> Option<String> {
    let after = tail.strip_prefix(&format!(",\"{key}\":\""))?;
    let end = after.find('"')?;
    let value = &after[..end];
    if value.contains('\\') {
        return None;
    }
    *tail = &after[end + 1..];
    Some(value.to_string())
}

impl SpecLog {
    /// Uma leitura só com `events`, sem linha pulada: o recorte de outra
    /// leitura.
    #[must_use]
    pub fn with_events(events: Vec<SpecEvent>) -> Self {
        Self { events, ..Self::default() }
    }

    /// O evento de número `id`, removido ou não.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<&SpecEvent> {
        // Os números crescem com o arquivo: a busca binária acha o evento, e
        // o arquivo editado à mão fora de ordem cai na procura de um a um.
        match self.events.binary_search_by_key(&id, |e| e.id) {
            Ok(at) => self.events.get(at),
            Err(_) => self.events.iter().find(|e| e.id == id),
        }
    }

    /// O maior número do arquivo, contando o que as linhas estragadas parecem
    /// ter. A próxima gravação usa o seguinte, então um número nunca se
    /// repete, nem depois de uma edição à mão.
    #[must_use]
    pub fn max_id(&self) -> u64 {
        let read = self.events.iter().map(|e| e.id);
        let hinted = self.skipped.iter().filter_map(|s| s.id_hint);
        read.chain(hinted).max().unwrap_or(0)
    }

    /// Os eventos que um filtro de remoção pega, entre os gravados antes do
    /// número `before`.
    #[must_use]
    pub fn filter_matches(&self, filter: &TimeFilter, before: u64) -> Vec<u64> {
        self.events.iter().filter(|e| e.id < before && filter.matches(e)).map(|e| e.id).collect()
    }

    /// Os eventos que somem da leitura, cada um com o motivo. O expurgo vence;
    /// depois dele, a remoção de hoje, a que leva a marca de
    /// `GIVES_BACK_FIELD`, lida antes das substituições; por último, a
    /// remoção sem a marca e as substituições que valem, as de
    /// `replacements`, na ordem do arquivo, como eram lidas quando ela foi
    /// gravada: entre as duas, vale a primeira. A volta do agente só fica com
    /// o motivo dela quando nada mais a esconde — é assim que o leitor de
    /// voltas separa a que espera a rodada da que já foi assumida.
    #[must_use]
    pub fn hidden(&self) -> BTreeMap<u64, Hidden> {
        self.derived().hidden.clone()
    }

    /// Os ocultos e as substituições que valem, contados uma vez por leitura:
    /// a conta fica guardada até os eventos mudarem ([`Shape`]).
    fn derived(&self) -> std::sync::Arc<Derived> {
        let shape = Shape::of(&self.events);
        let mut memo = self.memo.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((kept, derived)) = memo.as_ref()
            && *kept == shape
        {
            return derived.clone();
        }
        let derived = std::sync::Arc::new(self.count_hidden());
        *memo = Some((shape, derived.clone()));
        derived
    }

    /// A conta de [`SpecLog::hidden`], feita do zero.
    fn count_hidden(&self) -> Derived {
        #[cfg(test)]
        HIDDEN_RUNS.with(|runs| runs.set(runs.get() + 1));
        let removed = self.removals();
        let replacements = self.replacements(&removed);
        let replaced_by: BTreeMap<u64, u64> = replacements.iter().copied().collect();
        let mut hidden = BTreeMap::new();
        // Só a linha que o formato antigo esvaziou sai da leitura: o expurgo
        // de hoje deixa o item com o resto do texto.
        for event in &self.events {
            if let Some(by) = event.int(PURGED_FIELD) {
                hidden.insert(event.id, Hidden::Purged { by });
            }
        }
        for (id, removal) in removed.iter().filter(|(_, r)| r.gives_back) {
            hidden.entry(*id).or_insert(Hidden::Removed { by: removal.by });
        }
        // Cada motivo com o número do evento que o deu; a ordenação estável
        // deixa a remoção antes da substituição que viesse do mesmo evento,
        // como a leitura antiga fazia.
        let mut in_order: Vec<(u64, u64, Hidden)> =
            removed.iter().filter(|(_, r)| !r.gives_back).map(|(id, r)| (r.by, *id, Hidden::Removed { by: r.by })).collect();
        in_order.extend(replacements.into_iter().map(|(old, by)| (by, old, Hidden::Replaced { by })));
        in_order.sort_by_key(|(by, _, _)| *by);
        for (_, id, why) in in_order {
            hidden.entry(id).or_insert(why);
        }
        for event in self.events.iter().filter(|e| e.returned()) {
            hidden.entry(event.id).or_insert(Hidden::Returned);
        }
        let visible: Vec<usize> = self.events.iter().enumerate().filter(|(_, e)| !hidden.contains_key(&e.id)).map(|(at, _)| at).collect();
        let mut by_block: BTreeMap<Block, Vec<usize>> = BTreeMap::new();
        let mut metrics = Vec::new();
        for &at in &visible {
            let event = &self.events[at];
            if METRIC_TYPES.contains(&event.event_type.as_str()) {
                metrics.push(at);
            }
            if let Some(block) = event.block() {
                by_block.entry(block).or_default().push(at);
            }
        }
        Derived { hidden, replaced_by, visible, by_block, metrics }
    }

    /// Os números que uma remoção tira da leitura, cada um com a primeira
    /// remoção que o aponta ([`Self::removal_targets`]).
    fn removals(&self) -> BTreeMap<u64, Removal> {
        let mut removed = BTreeMap::new();
        for event in self.events.iter().filter(|e| e.event_type == "remove") {
            let gives_back = event.fields.get(GIVES_BACK_FIELD) == Some(&Value::Bool(true));
            for id in self.removal_targets(event) {
                removed.entry(id).or_insert(Removal { by: event.id, gives_back });
            }
        }
        removed
    }

    /// As substituições que valem, como pares (versão antiga, versão que a
    /// substitui), em ordem de número da versão nova.
    ///
    /// A versão de um item da spec removida pela remoção de hoje, a que leva
    /// a marca de `GIVES_BACK_FIELD`, não substitui nada: a que ela
    /// substituiu volta à leitura, com o mesmo código. Remover pelo número
    /// tira só aquela versão; pelo código, que a gravação troca por todas as
    /// versões, tira o item inteiro. A versão removida do meio da cadeia sai
    /// dela, e a seguinte passa a substituir as que ela substituía: o item
    /// segue pela versão mais nova, sem trazer a antiga de volta.
    ///
    /// A remoção sem a marca, gravada antes dela, segue a regra de quando foi
    /// gravada: a versão que ela tirou continua substituindo as anteriores, e
    /// o item sai inteiro. Seguem substituídos pela versão removida também a
    /// volta do agente, que a rodada já assumiu e que não volta a esperar a
    /// rodada, e o evento de tipo sem código — a lição do banco, que só se
    /// retira pelo número e sai inteira, com as versões que ela juntou.
    fn replacements(&self, removed: &BTreeMap<u64, Removal>) -> Vec<(u64, u64)> {
        let gives_back = |id: u64| removed.get(&id).is_some_and(|r| r.gives_back) && self.get(id).is_some_and(|e| type_spec(&e.event_type).is_some());
        let mut pairs = Vec::new();
        for event in &self.events {
            let mut pending = event.replaced();
            if gives_back(event.id) {
                pending.retain(|old| self.get(*old).is_some_and(SpecEvent::returned));
            }
            let mut seen = BTreeSet::new();
            while let Some(old) = pending.pop() {
                if !seen.insert(old) {
                    continue;
                }
                pairs.push((old, event.id));
                if gives_back(old)
                    && let Some(skipped) = self.get(old)
                {
                    pending.extend(skipped.replaced());
                }
            }
        }
        pairs
    }

    /// Os eventos que a leitura mostra, em ordem.
    #[must_use]
    pub fn visible(&self) -> Vec<&SpecEvent> {
        self.derived().visible.iter().map(|at| &self.events[*at]).collect()
    }

    /// A versão vigente de um item: segue as substituições que valem a partir
    /// de `id` e devolve a última, se ela está na leitura. Removida pelo
    /// número, pela remoção de hoje, a versão mais nova de um item da spec, a
    /// vigente é a anterior; pela remoção gravada antes da marca, o item não
    /// tem versão vigente.
    #[must_use]
    pub fn current(&self, id: u64) -> Option<&SpecEvent> {
        let derived = self.derived();
        let replaced_by = &derived.replaced_by;
        let mut id = id;
        for _ in 0..=self.events.len() {
            match replaced_by.get(&id) {
                Some(next) => id = *next,
                None => break,
            }
        }
        self.get(id).filter(|e| !derived.hidden.contains_key(&e.id))
    }

    /// Resolve a batch of references against one immutable event snapshot.
    /// Includes missing older IDs referenced by replacements, with the same
    /// removal, return and cycle rules as `current`. No mutable cache survives
    /// a change to the log.
    #[must_use]
    pub fn current_versions(&self) -> BTreeMap<u64, &SpecEvent> {
        let derived=self.derived();
        let (replaced_by,hidden)=(&derived.replaced_by,&derived.hidden);
        let events:BTreeMap<u64,&SpecEvent>=self.events.iter().map(|event|(event.id,event)).collect();
        let ids:BTreeSet<_>=events.keys().chain(replaced_by.keys()).copied().collect();
        ids.into_iter().filter_map(|original| {
            let mut id=original;
            for _ in 0..=self.events.len() {
                match replaced_by.get(&id) {Some(next)=>id = *next,None=>break}
            }
            events.get(&id).filter(|event|!hidden.contains_key(&event.id)).map(|event|(original,*event))
        }).collect()
    }

    /// As ondas que já têm registro de entrega. O que elas fizeram está
    /// provado pelo código que entrou, e não pelo texto que o descreveu. A
    /// volta que a rodada ainda não assumiu não conta: está fora da leitura.
    #[must_use]
    pub fn delivered_waves(&self) -> BTreeSet<u64> {
        self.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|event| event.event_type == "delivered").filter_map(SpecEvent::wave).collect()
    }

    /// Os resumos que ainda valem, em ordem de número, cada um com as tarefas
    /// que ele acompanha e que não foram entregues, pelo número da versão
    /// vigente de cada uma. Resumo é a entrega — a que a leitura mostra, já
    /// assumida pela rodada — com `undone` não vazio: a onda parou e deixou
    /// tarefas por fazer. Cada tarefa é acompanhada pelo resumo mais novo que
    /// a deixou: quem continuou um resumo e parou de novo escreveu um que o
    /// substitui. A tarefa está entregue quando a onda dela tem entrega, salvo
    /// a onda que a deixou por fazer. O resumo vale enquanto acompanha uma
    /// tarefa não entregue, em quantas ondas ela sair; sem nenhuma, sai da
    /// lista.
    #[must_use]
    pub fn live_summaries(&self) -> Vec<(&SpecEvent, BTreeSet<u64>)> {
        let codes = self.codes();
        let delivered = self.delivered_waves();
        let mut summaries: Vec<&SpecEvent> = self
            .block(BlockQuery::Block(Block::Waves))
            .into_iter()
            .filter(|e| e.event_type == "delivered")
            .filter(|e| e.fields.get("undone").and_then(Value::as_array).is_some_and(|tasks| !tasks.is_empty()))
            .collect();
        // A retired wave cannot integrate or block the work. Its already
        // authorized report can still guide the tasks it left, with its
        // `returned` provenance intact and without becoming a delivery.
        let planned = self.planned_waves();
        for report in self.unassumed_returns().into_iter().filter(|e| e.event_type == "delivered") {
            let Some(wave) = report.wave().filter(|n| !planned.contains(n)) else {
                continue;
            };
            let Some(send) = self.last_dispatch_by_wave().get(&wave).and_then(|id| self.get(*id)) else {
                continue;
            };
            if send.id >= report.id || send.str_field("author") != Some("binary") {
                continue;
            }
            let left = report.fields.get("undone").and_then(Value::as_array);
            if left.is_some_and(|left| {
                !left.is_empty()
                    && left.iter().all(|value| {
                        let task = super::against::current_task(self, &codes, value);
                        task.is_some_and(|task| {
                            send.ints("items")
                                .into_iter()
                                .filter_map(|id| self.get(id))
                                .filter(|item| item.event_type == "task" && item.wave() == Some(wave))
                                .any(|item| self.current(item.id).is_some_and(|now| now.id == task.id))
                        })
                    })
            }) {
                summaries.push(report);
            }
        }
        summaries.sort_by_key(|event| event.id);
        let mut carried: BTreeMap<u64, (u64, Option<u64>)> = BTreeMap::new();
        for summary in &summaries {
            for value in summary.fields.get("undone").and_then(Value::as_array).into_iter().flatten() {
                if let Some(task) = super::against::current_task(self, &codes, value) {
                    carried.insert(task.id, (summary.id, task.wave()));
                }
            }
        }
        summaries
            .into_iter()
            .map(|summary| {
                let pending: BTreeSet<u64> = carried
                    .iter()
                    .filter(|(_, (by, _))| *by == summary.id)
                    .filter(|(_, (_, wave))| wave.is_none_or(|wave| !delivered.contains(&wave) || summary.wave() == Some(wave)))
                    .map(|(task, _)| *task)
                    .collect();
                (summary, pending)
            })
            .filter(|(_, pending)| !pending.is_empty())
            .collect()
    }

    /// As voltas que a rodada ou o fechamento ainda não assumiu, em ordem de
    /// número: a última de cada onda, uma por tipo — a entrega da onda e o
    /// veredito do revisor —, e a do veredito final, que vem sem onda. A
    /// volta é o evento que o próprio agente grava com `returned`, fora da
    /// leitura; a rodada a assume gravando a versão oficial, sem o campo, com
    /// `replaces` para ela. Da versão oficial para trás, nenhuma volta da
    /// mesma onda espera mais, nem a que ela não apontou: só a gravada depois
    /// dela. A volta de onda que saiu do plano continua aqui, como história;
    /// a entrega que ainda espera a rodada se lê em
    /// [`Self::returns_awaiting_round`].
    #[must_use]
    pub fn unassumed_returns(&self) -> Vec<&SpecEvent> {
        let derived = self.derived();
        let hidden = &derived.hidden;
        let mut official: BTreeMap<(&str, Option<u64>), u64> = BTreeMap::new();
        let mut last: BTreeMap<(&str, Option<u64>), &SpecEvent> = BTreeMap::new();
        for event in self.events.iter().filter(|e| matches!(e.event_type.as_str(), "delivered" | "verdict")) {
            let key = (event.event_type.as_str(), event.wave());
            if !event.returned() {
                let newest = official.entry(key).or_insert(event.id);
                *newest = (*newest).max(event.id);
            } else if hidden.get(&event.id) == Some(&Hidden::Returned) && last.get(&key).is_none_or(|kept| kept.id < event.id) {
                last.insert(key, event);
            }
        }
        let mut out: Vec<&SpecEvent> =
            last.into_iter().filter(|(key, event)| official.get(key).is_none_or(|id| *id < event.id)).map(|(_, event)| event).collect();
        out.sort_by_key(|event| event.id);
        out
    }

    /// Os arquivos que os commits desta obra tocaram, de toda onda — a prova
    /// concreta do que as entregas mudaram, e não o que uma tarefa declarava
    /// tocar antes de rodar. É daqui que o caminho de volta (do teste para o
    /// critério que o cobre) parte, no fechamento.
    #[must_use]
    pub fn delivered_files(&self) -> BTreeSet<String> {
        self.block(BlockQuery::Block(Block::Progress))
            .into_iter()
            .filter(|event| event.event_type == "commit")
            .flat_map(|event| event.fields.get("files").and_then(Value::as_array).cloned().unwrap_or_default())
            .filter_map(|file| file.as_str().map(str::to_string))
            .collect()
    }

    /// As ondas do plano: as que a leitura mostra, menos a onda que o binário
    /// formou (autor `binary`) e ficou sem tarefa visível nenhuma — o corte
    /// devolveu as tarefas dela ao backlog, ou a tarefa foi regravada sem
    /// onda antes do envio. Sem tarefa não há o que despachar nem o que
    /// entregar, e o backlog reempacota o que ela perdeu numa onda nova. A
    /// onda combinada à mão fica no plano mesmo sem tarefa, porque o backlog
    /// nunca reempacota o que ela perdeu. O que foi gravado em nome de uma
    /// onda que saiu do plano — o veredito, que o binário não deixa tirar, e
    /// o pedido e a entrega — não conta na rodada, no fechamento nem no
    /// pedido.
    #[must_use]
    pub fn planned_waves(&self) -> BTreeSet<u64> {
        let waves = self.block(BlockQuery::Block(Block::Waves));
        let with_task: BTreeSet<u64> = waves.iter().filter(|e| e.event_type == "task").filter_map(|e| e.wave()).collect();
        waves
            .into_iter()
            .filter(|e| e.event_type == "wave")
            .filter(|e| e.str_field("author") != Some("binary") || e.wave().is_some_and(|n| with_task.contains(&n)))
            .filter_map(SpecEvent::wave)
            .collect()
    }

    /// O maior número de onda gravado no arquivo, com a onda que saiu do
    /// plano por ficar vazia ([`Self::planned_waves`]) e a removida
    /// incluídas; zero sem onda nenhuma. A onda nova nasce depois dele, para
    /// nunca repetir o número de uma onda que já foi gravada: o envio e as
    /// tarefas que ainda trazem o número velho passariam a ser dela.
    #[must_use]
    pub fn last_wave_number(&self) -> u64 {
        self.events.iter().filter(|e| e.event_type == "wave").filter_map(SpecEvent::wave).max().unwrap_or(0)
    }

    /// As tarefas vigentes presas a uma onda que a remoção `removed` tirou da
    /// leitura, lidas no arquivo já com ela: a onda sem entrega e sem outra
    /// versão à mostra, e as tarefas que ainda levam o número dela. A onda
    /// entregue fica de fora: as tarefas dela seguem como estão.
    #[must_use]
    pub fn tasks_of_removed_waves(&self, removed: &[u64]) -> Vec<&SpecEvent> {
        let waves = self.block(BlockQuery::Block(Block::Waves));
        let delivered = self.delivered_waves();
        let gone: BTreeSet<u64> = removed
            .iter()
            .filter_map(|id| self.get(*id))
            .filter(|e| e.event_type == "wave")
            .filter_map(SpecEvent::wave)
            .filter(|n| !delivered.contains(n))
            .filter(|n| !waves.iter().any(|e| e.event_type == "wave" && e.wave() == Some(*n)))
            .collect();
        self.visible().into_iter().filter(|e| e.event_type == "task" && e.wave().is_some_and(|n| gone.contains(&n))).collect()
    }

    /// Os vereditos de cada onda do plano, do mais velho ao mais novo.
    #[must_use]
    pub fn verdicts_by_wave(&self) -> BTreeMap<u64, Vec<&SpecEvent>> {
        let planned = self.planned_waves();
        let mut out: BTreeMap<u64, Vec<&SpecEvent>> = BTreeMap::new();
        for verdict in self.block(BlockQuery::Block(Block::Review)).into_iter().filter(|e| matches!(e.event_type.as_str(), "verdict" | "validation_failure")) {
            if let Some(n) = verdict.wave().filter(|n| planned.contains(n)) {
                out.entry(n).or_default().push(verdict);
            }
        }
        out
    }

    /// As ondas do plano cuja última revisão final reprovou, cada uma com o
    /// número dessa reprovação. A rodada e o fechamento leem daqui; o pedido
    /// da revisão recorta as ondas só pelo veredito final mais novo. Um
    /// veredito sem o campo final não conta: só o veredito final
    /// do agente de teste dedicado pode pôr uma onda em modo de conserto.
    /// Só o resultado `rejected` conta: a marca solta (`released`), que o
    /// fechamento grava depois da reprovação sem prova, é a mais nova da onda
    /// e a tira daqui.
    #[must_use]
    pub fn last_rejected(&self) -> BTreeMap<u64, u64> {
        self.verdicts_by_wave()
            .into_iter()
            .filter_map(|(n, verdicts)| {
                verdicts
                    .last()
                    .filter(|v| v.fields.get("final") == Some(&Value::Bool(true)))
                    .filter(|v| v.str_field("result") == Some("rejected"))
                    .map(|v| (n, v.id))
            })
            .collect()
    }

    /// O número do último evento do tipo `event_type` de cada onda, no bloco
    /// das ondas: o último pedido (`send`) ou a última entrega (`delivered`).
    /// A entrega que chega depois de uma reprovação é o conserto, mesmo quando
    /// veio pela linha de outra onda.
    #[must_use]
    pub fn last_by_wave(&self, event_type: &str) -> BTreeMap<u64, u64> {
        let mut last: BTreeMap<u64, u64> = BTreeMap::new();
        for event in self.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == event_type) {
            if let Some(n) = event.wave() {
                last.insert(n, event.id);
            }
        }
        last
    }

    /// A posição em que o envio `id` despachou a onda: o número do envio
    /// original da cadeia de `replaces`. A versão de um envio que só
    /// acrescenta o consumo não despacha nada de novo — ela substitui o
    /// envio e o mantém no lugar em que a onda saiu —, então a entrega, o
    /// veredito, o clique do usuário e a mudança de plano que chegam depois do
    /// despacho valem depois dele, esteja a versão do consumo gravada antes ou
    /// depois deles. É a única conta de "antes ou depois do envio": quem
    /// compara um número da spec com o envio de uma onda lê daqui, nunca do
    /// número da versão mais nova. O envio que não substitui nada — o pedido
    /// novo e o reenvio — é a própria posição.
    #[must_use]
    pub fn dispatch_position(&self, id: u64) -> u64 {
        chain_root(&self.send_versions(), id)
    }

    /// O campo `field` do envio `id` como vale nele: o da própria versão ou,
    /// sem ele, o da versão anterior mais nova que o tem, pela cadeia de
    /// `replaces` entre envios. A versão nova de um envio não repete o pedido
    /// ([`super::SEND_REQUEST_FIELDS`]); a spec antiga, que o repetia em cada
    /// versão, responde pela própria. Quem lê o pedido de um envio lê daqui.
    #[must_use]
    pub fn send_field(&self, id: u64, field: &str) -> Option<&Value> {
        let mut at = self.get(id).filter(|event| event.event_type == "send")?;
        // A cadeia é finita, mas a spec é um arquivo de texto: o passo conta
        // os saltos para um círculo gravado à mão não prender a leitura.
        for _ in 0..=self.events.len() {
            if let Some(value) = at.fields.get(field) {
                return Some(value);
            }
            at = at.int("replaces").and_then(|older| self.get(older)).filter(|older| older.event_type == "send")?;
        }
        None
    }

    /// Os campos de `event` como valem: no envio, com o pedido que a versão
    /// não repete ([`Self::send_field`]); nos outros, os dele. É o que a
    /// leitura mostra e compara de cada versão.
    #[must_use]
    pub fn whole_fields(&self, event: &SpecEvent) -> Map<String, Value> {
        let mut fields = event.fields.clone();
        if event.event_type == "send" {
            for field in super::SEND_REQUEST_FIELDS {
                if !fields.contains_key(*field)
                    && let Some(value) = self.send_field(event.id, field)
                {
                    fields.insert((*field).to_string(), value.clone());
                }
            }
        }
        fields
    }

    /// O texto do pedido do envio `id` ([`Self::send_field`]).
    #[must_use]
    pub fn send_text(&self, id: u64) -> Option<&str> {
        self.send_field(id, "text").and_then(Value::as_str)
    }

    /// O último envio de cada onda do bloco das ondas, na posição em que ele
    /// despachou a onda ([`Self::dispatch_position`]): o número que a entrega,
    /// o veredito e o plano que vêm depois dele são comparados. A onda sem
    /// envio não aparece.
    #[must_use]
    pub fn last_dispatch_by_wave(&self) -> BTreeMap<u64, u64> {
        let versions = self.send_versions();
        self.last_by_wave("send").into_iter().map(|(n, id)| (n, chain_root(&versions, id))).collect()
    }

    /// Cada versão de envio que substitui outro envio, com o número do que ela
    /// substitui.
    fn send_versions(&self) -> BTreeMap<u64, u64> {
        let sends: BTreeSet<u64> = self.events.iter().filter(|e| e.event_type == "send").map(|e| e.id).collect();
        self.events
            .iter()
            .filter(|e| e.event_type == "send")
            .filter_map(|e| e.int("replaces").filter(|older| sends.contains(older)).map(|older| (e.id, older)))
            .collect()
    }

    /// Um bloco, só com o que a leitura mostra. Uma onda (`wave-2`) traz a
    /// onda, as tarefas, os envios e os entregou dela, e as skills que as
    /// tarefas dela nomeiam.
    #[must_use]
    pub fn block(&self, query: BlockQuery) -> Vec<&SpecEvent> {
        let derived = self.derived();
        let pick = |at: &[usize]| at.iter().map(|at| &self.events[*at]).collect::<Vec<_>>();
        match query {
            BlockQuery::Block(Block::Metrics) => pick(&derived.metrics),
            BlockQuery::Block(block) => derived.by_block.get(&block).map_or_else(Vec::new, |at| pick(at)),
            BlockQuery::Wave(n) => {
                let visible = pick(&derived.visible);
                let skills: BTreeSet<&str> =
                    visible.iter().filter(|e| e.event_type == "task" && e.wave() == Some(n)).filter_map(|e| e.str_field("skill")).collect();
                visible
                    .iter()
                    .copied()
                    .filter(|e| e.block() == Some(Block::Waves) || e.event_type == "validation_failure")
                    .filter(|e| e.wave() == Some(n) || (e.event_type == "skill" && e.str_field("name").is_some_and(|s| skills.contains(s))))
                    .collect()
            }
        }
    }

    /// O que um passo do fluxo lê, em ordem de número, sem repetição.
    #[must_use]
    pub fn step(&self, step: &Step) -> Vec<&SpecEvent> {
        let mut picked: BTreeMap<u64, &SpecEvent> = BTreeMap::new();
        match step {
            Step::Resume => pick(&mut picked, self.block(BlockQuery::Block(Block::State))),
            Step::Close => {
                pick(&mut picked, self.block(BlockQuery::Block(Block::State)));
                pick(&mut picked, self.block(BlockQuery::Block(Block::Criteria)));
            }
            Step::Question { term, languages } => {
                let codes = self.codes();
                pick(&mut picked, found_by(self.block(BlockQuery::Block(Block::Conversation)), term, &codes, languages));
            }
            Step::Review { wave } => {
                pick(&mut picked, self.block(BlockQuery::Wave(*wave)));
                pick(&mut picked, self.wave_criteria(*wave));
                pick(&mut picked, crate::domain::wave_prompt::fix_lines(self, *wave));
            }
            Step::Dispatch { wave } => {
                let own = self.block(BlockQuery::Wave(*wave));
                let owned = crate::domain::wave_prompt::agreed_for(self, *wave);
                let depends: Vec<u64> = own.iter().filter(|e| e.event_type == "wave").flat_map(|e| e.ints("depends_on")).collect();
                let delivered: Vec<&SpecEvent> =
                    depends.into_iter().flat_map(|d| self.block(BlockQuery::Wave(d))).filter(|e| e.event_type == "delivered").collect();
                pick(&mut picked, own);
                pick(&mut picked, self.wave_criteria(*wave));
                pick(&mut picked, self.block(BlockQuery::Block(Block::Specification)));
                pick(&mut picked, owned);
                pick(&mut picked, delivered);
                pick(&mut picked, crate::domain::wave_prompt::fix_lines(self, *wave));
            }
        }
        picked.into_values().collect()
    }
}

/// O envio original da cadeia de versões que começa em `id`: segue cada versão
/// até a que ela substitui, até uma que não substitui nada. O limite só
/// protege a leitura de um arquivo editado à mão que feche a cadeia em
/// círculo.
fn chain_root(versions: &BTreeMap<u64, u64>, id: u64) -> u64 {
    let mut at = id;
    for _ in 0..=versions.len() {
        match versions.get(&at) {
            Some(older) => at = *older,
            None => break,
        }
    }
    at
}

/// Junta eventos por número, sem repetição.
fn pick<'a>(picked: &mut BTreeMap<u64, &'a SpecEvent>, events: Vec<&'a SpecEvent>) {
    for event in events {
        picked.insert(event.id, event);
    }
}

impl SpecLog {
    /// Os critérios que a onda `n` aponta, na versão vigente de cada um.
    pub fn wave_criteria(&self, n: u64) -> Vec<&SpecEvent> {
        self.block(BlockQuery::Wave(n))
            .into_iter()
            .filter(|e| e.event_type == "wave")
            .flat_map(|e| e.ints("criteria"))
            .filter_map(|id| self.current(id))
            .collect()
    }

    /// Os critérios que as ondas de `waves` apontam, juntos, sem repetição e
    /// na ordem do código: de onde a rodada parte para provar, uma vez cada,
    /// antes de comitar o que essas ondas entregaram. Ela deixa de fora o
    /// critério que outra tarefa ainda por entregar também cobre.
    #[must_use]
    pub fn criteria_for_waves(&self, waves: &[u64]) -> Vec<&SpecEvent> {
        let mut picked: BTreeMap<u64, &SpecEvent> = BTreeMap::new();
        for wave in waves {
            pick(&mut picked, self.wave_criteria(*wave));
        }
        picked.into_values().collect()
    }
}

#[cfg(test)]
thread_local! {
    /// Quantas vezes os ocultos foram contados nesta linha de execução.
    static HIDDEN_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_current_versions_match_individual_resolution_across_removals_and_returns() {
        let cases=[
            "{\"id\":1,\"type\":\"task\"}\n{\"id\":2,\"type\":\"task\",\"replaces\":1}\n{\"id\":3,\"type\":\"task\",\"replaces\":2}",
            "{\"id\":3,\"type\":\"task\",\"replaces\":1}\n{\"id\":4,\"type\":\"remove\",\"targets\":[3]}",
            "{\"id\":1,\"type\":\"task\"}\n{\"id\":2,\"type\":\"task\",\"replaces\":1}\n{\"id\":3,\"type\":\"remove\",\"targets\":[2],\"gives_back\":true}",
            "{\"id\":1,\"type\":\"delivered\",\"returned\":true}\n{\"id\":2,\"type\":\"task\"}",
            "{\"id\":1,\"type\":\"task\",\"replaces\":2}\n{\"id\":2,\"type\":\"task\",\"replaces\":1}",
        ];
        for content in cases {
            let log=parse_log(content);
            let versions=log.current_versions();
            for id in 0..6 {
                assert_eq!(versions.get(&id).map(|event|event.id),log.current(id).map(|event|event.id),"reference {id}: {content}");
            }
        }
    }

    #[test]
    fn a_broken_line_is_skipped_by_number_and_the_rest_is_read() {
        let content = "{\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"message\",\"text\":\"a\"}\n\
                       garbage\n\
                       {\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"note\"}\n\
                       {\"v\":1,\"id\":2,\"at\":\"t\",\"type\":\"message\",\"text\":\"b\"}\n\
                       {\"v\":1,\"id\":9,\"at\":\"t\",\"ty";
        let log = parse_log(content);
        assert_eq!(log.events.iter().map(|e| e.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(
            log.skipped.iter().map(|s| (s.line, s.reason)).collect::<Vec<_>>(),
            [(2, SkipReason::Unreadable), (3, SkipReason::DuplicateId(1)), (5, SkipReason::Unreadable)]
        );
        assert_eq!(log.max_id(), 9, "the torn line's number is never reused");
        assert!(log.skipped[0].message(Locale::PtBr).contains("linha 2"));
    }

    /// As ondas entregues são as que têm registro de entrega, e só elas: a
    /// onda que só tem tarefa, e a que só foi enviada, ainda vêm.
    #[test]
    fn the_delivered_waves_are_the_ones_with_a_delivery_record() {
        let content = "{\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"wave\",\"n\":1,\"text\":\"Uma.\"}\n\
                       {\"v\":1,\"id\":2,\"at\":\"t\",\"type\":\"wave\",\"n\":2,\"text\":\"Duas.\"}\n\
                       {\"v\":1,\"id\":3,\"at\":\"t\",\"type\":\"wave\",\"n\":3,\"text\":\"Três.\"}\n\
                       {\"v\":1,\"id\":4,\"at\":\"t\",\"type\":\"task\",\"wave\":2,\"text\":\"Mexer.\"}\n\
                       {\"v\":1,\"id\":5,\"at\":\"t\",\"type\":\"send\",\"wave\":2,\"text\":\"Pedido.\"}\n\
                       {\"v\":1,\"id\":6,\"at\":\"t\",\"type\":\"delivered\",\"wave\":1,\"text\":\"Saiu.\"}\n\
                       {\"v\":1,\"id\":7,\"at\":\"t\",\"type\":\"delivered\",\"wave\":3,\"text\":\"Saiu.\"}\n";
        let log = parse_log(content);
        assert_eq!(log.delivered_waves(), BTreeSet::from([1, 3]));
        // O painel mede o tempo do pedido até a entrega: a leitura dele traz
        // os dois, e não a onda nem a tarefa.
        let panel: Vec<u64> = log.block(BlockQuery::Block(Block::Metrics)).iter().map(|e| e.id).collect();
        assert_eq!(panel, [5, 6, 7]);
    }

    /// A onda que o binário formou e ficou sem tarefa sai do plano; a
    /// combinada à mão sem tarefa fica. O número mais alto conta a onda que
    /// saiu, para a onda nova não repetir o número dela.
    #[test]
    fn a_binary_wave_without_a_task_leaves_the_plan_but_keeps_its_number() {
        let content = "{\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"wave\",\"n\":1,\"text\":\"Uma.\",\"author\":\"assistant\"}\n\
                       {\"v\":1,\"id\":2,\"at\":\"t\",\"type\":\"wave\",\"n\":2,\"text\":\"Duas.\",\"author\":\"binary\"}\n\
                       {\"v\":1,\"id\":3,\"at\":\"t\",\"type\":\"wave\",\"n\":3,\"text\":\"Três.\",\"author\":\"binary\"}\n\
                       {\"v\":1,\"id\":4,\"at\":\"t\",\"type\":\"wave\",\"n\":4,\"text\":\"Quatro.\",\"author\":\"binary\"}\n\
                       {\"v\":1,\"id\":5,\"at\":\"t\",\"type\":\"task\",\"wave\":2,\"text\":\"Mexer.\"}\n\
                       {\"v\":1,\"id\":6,\"at\":\"t\",\"type\":\"task\",\"wave\":4,\"text\":\"Mexer mais.\"}\n\
                       {\"v\":1,\"id\":7,\"at\":\"t\",\"type\":\"task\",\"text\":\"Mexer mais, de volta ao backlog.\",\"replaces\":6}\n";
        let log = parse_log(content);
        assert_eq!(log.planned_waves(), BTreeSet::from([1, 2]));
        assert_eq!(log.last_wave_number(), 4);
        assert_eq!(parse_log("").last_wave_number(), 0);
    }

    /// Só a onda removida sem entrega deixa as tarefas presas ao número de uma
    /// onda que não existe mais; as da onda entregue seguem como estão.
    #[test]
    fn only_a_removed_wave_without_a_delivery_leaves_its_tasks_behind() {
        let content = "{\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"wave\",\"n\":1,\"text\":\"Uma.\"}\n\
                       {\"v\":1,\"id\":2,\"at\":\"t\",\"type\":\"wave\",\"n\":2,\"text\":\"Duas.\"}\n\
                       {\"v\":1,\"id\":3,\"at\":\"t\",\"type\":\"task\",\"wave\":1,\"text\":\"Mexer.\"}\n\
                       {\"v\":1,\"id\":4,\"at\":\"t\",\"type\":\"task\",\"wave\":2,\"text\":\"Mexer mais.\"}\n\
                       {\"v\":1,\"id\":5,\"at\":\"t\",\"type\":\"delivered\",\"wave\":2,\"text\":\"Saiu.\"}\n\
                       {\"v\":1,\"id\":6,\"at\":\"t\",\"type\":\"remove\",\"targets\":[1,2],\"reason\":\"Saíram.\"}\n";
        let log = parse_log(content);
        let left: Vec<u64> = log.tasks_of_removed_waves(&[1, 2]).into_iter().map(|e| e.id).collect();
        assert_eq!(left, [3]);
    }

    /// Os arquivos entregues são os dos eventos `commit`, sem repetir, e não
    /// os de outro tipo de evento — a tarefa que só declara o que uma onda
    /// vai tocar não conta, porque nada garante que ela tocou aquilo de
    /// verdade.
    #[test]
    fn delivered_files_are_the_ones_the_commits_touched() {
        let content = "{\"v\":1,\"id\":1,\"at\":\"t\",\"type\":\"task\",\"wave\":1,\"text\":\"Mexer.\",\
                       \"files\":[{\"path\":\"src/nao-e-commit.rs\"}]}\n\
                       {\"v\":1,\"id\":2,\"at\":\"t\",\"type\":\"commit\",\"sha\":\"a1\",\"title\":\"x\",\
                       \"waves\":[1],\"files\":[\"src/a.rs\",\"src/b.rs\"],\"repo\":\".\"}\n\
                       {\"v\":1,\"id\":3,\"at\":\"t\",\"type\":\"commit\",\"sha\":\"a2\",\"title\":\"y\",\
                       \"waves\":[2],\"files\":[\"src/b.rs\",\"src/c.rs\"],\"repo\":\".\"}\n";
        let log = parse_log(content);
        assert_eq!(log.delivered_files(), BTreeSet::from(["src/a.rs".to_string(), "src/b.rs".to_string(), "src/c.rs".to_string()]));
    }

    fn log_of(lines: &[serde_json::Value]) -> SpecLog {
        parse_log(&lines.iter().map(serde_json::Value::to_string).collect::<Vec<_>>().join("\n"))
    }

    /// Os ocultos se contam uma vez por leitura: o bloco, a leitura visível, a
    /// versão vigente e os ocultos de novo reusam a mesma conta.
    #[test]
    fn hidden_events_are_counted_once_per_read() {
        let log = log_of(&[
            serde_json::json!({"v":1,"id":1,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido."}),
            serde_json::json!({"v":1,"id":2,"at":"t","type":"send","wave":2,"role":"wave","text":"Outro.","replaces":1}),
        ]);
        HIDDEN_RUNS.with(|runs| runs.set(0));
        let _ = log.block(BlockQuery::Block(Block::Waves));
        let _ = log.visible();
        assert_eq!(log.current(1).map(|e| e.id), Some(2));
        assert_eq!(log.hidden().get(&1), Some(&Hidden::Replaced { by: 2 }));
        assert_eq!(log.get(2).map(|e| e.id), Some(2));
        assert_eq!(HIDDEN_RUNS.with(std::cell::Cell::get), 1);
    }

    /// A versão de um envio que só traz o consumo mantém o lugar em que o
    /// envio despachou a onda, gravada antes ou depois da entrega, do
    /// veredito ou de uma mudança de plano; o reenvio, que não substitui
    /// nada, é um despacho novo. Só um envio conta como envio substituído, e
    /// a cadeia fechada em círculo por um arquivo editado à mão termina.
    #[test]
    fn a_send_field_is_read_from_the_newest_version_that_has_it() {
        use serde_json::json;
        let log = log_of(&[
            json!({"v":1,"id":1,"at":"t","type":"wave","n":2,"text":"Duas."}),
            json!({"v":1,"id":2,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido.","read_items":["A"],"copy":"c"}),
            json!({"v":1,"id":3,"at":"t","type":"send","wave":2,"role":"wave","tokens":5,"replaces":2}),
            json!({"v":1,"id":4,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido com mais.","replaces":3}),
            json!({"v":1,"id":5,"at":"t","type":"step","wave":2,"text":"Passo.","replaces":4}),
            // A spec antiga repetia o pedido inteiro em cada versão.
            json!({"v":1,"id":6,"at":"t","type":"send","wave":3,"role":"wave","text":"Velho.","read_items":["B"]}),
            json!({"v":1,"id":7,"at":"t","type":"send","wave":3,"role":"wave","text":"Velho.","read_items":["B"],"tokens":1,"replaces":6}),
        ]);
        assert_eq!(log.send_text(3), Some("Pedido."));
        assert_eq!(log.send_field(3, "read_items"), Some(&json!(["A"])));
        assert_eq!(log.send_text(4), Some("Pedido com mais."));
        assert_eq!(log.send_field(4, "read_items"), Some(&json!(["A"])));
        assert_eq!(log.send_field(4, "tokens"), Some(&json!(5)));
        assert_eq!(log.send_field(4, "analysis"), None);
        assert_eq!(log.send_text(7), Some("Velho."));
        assert_eq!(log.send_field(5, "read_items"), None, "only a send version carries the request");
    }

    #[test]
    fn a_consumption_version_of_a_send_keeps_the_place_where_the_send_dispatched_the_wave() {
        use serde_json::json;
        let log = log_of(&[
            json!({"v":1,"id":1,"at":"t","type":"wave","n":2,"text":"Duas."}),
            json!({"v":1,"id":2,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido."}),
            json!({"v":1,"id":3,"at":"t","type":"delivered","wave":2,"text":"Saiu.","returned":true}),
            json!({"v":1,"id":4,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido.","replaces":2}),
            json!({"v":1,"id":5,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido.","replaces":4}),
        ]);
        assert_eq!(log.last_by_wave("send").get(&2), Some(&5), "a versão mais nova é a que a leitura mostra");
        assert_eq!((log.dispatch_position(2), log.dispatch_position(4), log.dispatch_position(5)), (2, 2, 2));
        assert_eq!(log.last_dispatch_by_wave().get(&2), Some(&2), "a versão do consumo não despacha nada de novo");

        let mut resent = log_of(&[
            json!({"v":1,"id":1,"at":"t","type":"wave","n":2,"text":"Duas."}),
            json!({"v":1,"id":2,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido."}),
            json!({"v":1,"id":3,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido.","replaces":2}),
            json!({"v":1,"id":4,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido.","resends":3}),
            json!({"v":1,"id":5,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido.","replaces":4}),
        ]);
        assert_eq!(resent.last_dispatch_by_wave().get(&2), Some(&4), "o reenvio é um despacho novo");
        assert_eq!(resent.dispatch_position(3), 2);

        // Uma versão que aponta outra coisa que não um envio não substitui envio nenhum.
        resent = log_of(&[
            json!({"v":1,"id":1,"at":"t","type":"wave","n":2,"text":"Duas."}),
            json!({"v":1,"id":2,"at":"t","type":"delivered","wave":2,"text":"Saiu."}),
            json!({"v":1,"id":3,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido.","replaces":2}),
        ]);
        assert_eq!(resent.dispatch_position(3), 3);

        let circle = log_of(&[
            json!({"v":1,"id":1,"at":"t","type":"send","wave":2,"role":"wave","text":"A.","replaces":2}),
            json!({"v":1,"id":2,"at":"t","type":"send","wave":2,"role":"wave","text":"B.","replaces":1}),
        ]);
        assert!([1, 2].contains(&circle.dispatch_position(1)), "a leitura termina");
        assert_eq!(parse_log("").dispatch_position(7), 7);
    }

    /// A volta do agente que a rodada assumiu segue substituída pela entrega
    /// oficial mesmo depois que a oficial é removida, pela remoção de hoje,
    /// com a marca, e pela gravada antes dela: a volta fica fora da leitura
    /// sem o motivo de volta, que é o que a rodada lê como espera, e o leitor
    /// de voltas não a devolve.
    #[test]
    fn an_assumed_return_does_not_wait_again_when_the_official_delivery_comes_out() {
        use serde_json::json;
        for removal in [
            json!({"v":1,"id":4,"at":"t","type":"remove","targets":[3],"reason":"engano","gives_back":true}),
            json!({"v":1,"id":4,"at":"t","type":"remove","targets":[3],"reason":"engano"}),
        ] {
            let log = log_of(&[
                json!({"v":1,"id":1,"at":"t","type":"send","wave":2,"role":"wave","text":"Pedido."}),
                json!({"v":1,"id":2,"at":"t","type":"delivered","wave":2,"text":"Saiu.","returned":true}),
                json!({"v":1,"id":3,"at":"t","type":"delivered","wave":2,"text":"Saiu.","replaces":2}),
                removal.clone(),
            ]);
            let hidden = log.hidden();
            assert_eq!(hidden.get(&2), Some(&Hidden::Replaced { by: 3 }), "a volta segue assumida: {removal}");
            assert_eq!(hidden.get(&3), Some(&Hidden::Removed { by: 4 }), "{removal}");
            assert!(log.unassumed_returns().is_empty(), "nenhuma volta espera a rodada: {removal}");
            assert!(log.delivered_waves().is_empty(), "a entrega oficial saiu da leitura: {removal}");
            assert_eq!(log.current(2).map(|e| e.id), None, "{removal}");
        }
    }

    /// Remover pelo número, com a marca que o binário põe na remoção de hoje,
    /// tira só aquela versão. A versão mais nova removida devolve a anterior,
    /// com o mesmo código; a versão do meio removida sai da cadeia, e o item
    /// segue pela mais nova, sem trazer a antiga de volta.
    #[test]
    fn removing_a_version_by_its_number_removes_only_that_one() {
        use serde_json::json;
        let versions = [
            json!({"v":1,"id":1,"at":"t","type":"note","text":"Primeira.","keys":["k"]}),
            json!({"v":1,"id":2,"at":"t","type":"note","text":"Segunda.","keys":["k"],"replaces":1}),
            json!({"v":1,"id":3,"at":"t","type":"note","text":"Terceira.","keys":["k"],"replaces":2}),
        ];
        let shown = |log: &SpecLog| log.visible().iter().filter(|e| e.event_type == "note").map(|e| e.id).collect::<Vec<_>>();
        let with = |remove: serde_json::Value| {
            let mut lines = versions.to_vec();
            lines.push(remove);
            log_of(&lines)
        };

        let newest = with(json!({"v":1,"id":4,"at":"t","type":"remove","targets":[3],"reason":"r","gives_back":true}));
        assert_eq!(shown(&newest), [2], "a versão anterior volta");
        assert_eq!(newest.codes().get(&2), log_of(&versions).codes().get(&3), "com o mesmo código");
        assert_eq!(newest.current(1).map(|e| e.id), Some(2));
        assert_eq!(newest.hidden().get(&3), Some(&Hidden::Removed { by: 4 }));

        let middle = with(json!({"v":1,"id":4,"at":"t","type":"remove","targets":[2],"reason":"r","gives_back":true}));
        assert_eq!(shown(&middle), [3], "a mais nova segue sozinha");
        assert_eq!(middle.current(1).map(|e| e.id), Some(3));
        assert_eq!(middle.current(2).map(|e| e.id), Some(3));
        assert_eq!(middle.hidden().get(&1), Some(&Hidden::Replaced { by: 3 }));

        let all = with(json!({"v":1,"id":4,"at":"t","type":"remove","targets":[1, 2, 3],"reason":"r","gives_back":true}));
        assert!(shown(&all).is_empty(), "todas as versões saem");
        assert_eq!(all.current(1).map(|e| e.id), None);
    }

    #[test]
    fn lines_the_writer_would_refuse_are_still_read() {
        let content = "{\"id\":1,\"type\":\"rule\",\"text\":\"sem versão e sem keys\"}\n\
                       {\"v\":7,\"id\":2,\"type\":\"future_kind\",\"shape\":\"new\"}\n";
        let log = parse_log(content);
        assert_eq!(log.events.len(), 2);
        assert!(log.skipped.is_empty());
        assert_eq!(log.events[1].block(), None, "an unknown type belongs to no block");
    }

    /// A linha cortada que começa como um evento é trocada no lugar; a linha
    /// com fim `\r\n` guarda o `\r`, o arquivo sem `\n` no fim continua sem, e
    /// duas linhas cortadas com o mesmo número não repetem o número.
    #[test]
    fn repairing_replaces_each_cut_line_in_place_and_never_repeats_a_number() {
        let first = r#"{"v":1,"id":2,"code":"MSTD-RULE-0002","at":"2026-09-12T10:00:00-03:00","type":"ru"#;
        let again = r#"{"v":1,"id":2,"code":"MSTD-RULE-0002","at":"2026-09-12T10:00:00-03:00","type":"r"#;
        let content = format!("{{\"v\":1,\"id\":1,\"type\":\"note\",\"text\":\"um\"}}\r\n{first}\r\n{again}\n{{\"v\":1,\"id\":9,\"type\":\"note\"}}");
        let log = parse_log(&content);
        assert_eq!(log.skipped.len(), 2, "{:?}", log.skipped);

        let repaired = repair_cut_lines(&content, &log).expect("a cut line is repaired");
        let lines: Vec<&str> = repaired.split('\n').collect();
        assert_eq!(lines.len(), 4, "no line appears or goes");
        assert_eq!(lines[0], content.split('\n').next().unwrap(), "the first line is byte-equal");
        assert!(lines[1].ends_with('\r'), "the line ending stays: {:?}", lines[1]);
        assert_eq!(lines[2], again, "the second cut line with the same number is left alone");
        assert_eq!(lines[3], content.split('\n').nth(3).unwrap(), "the last line is byte-equal, still without a newline");

        let again_log = parse_log(&repaired);
        let record = again_log.get(2).expect("the record has the number of the cut line");
        assert_eq!((record.event_type.as_str(), record.line, record.str_field("piece")), (CUT_LINE_TYPE, 2, Some(first)),);
        assert_eq!(again_log.skipped.len(), 1, "only the repeated number stays skipped");
        assert_eq!(repair_cut_lines(&repaired, &again_log), None, "nothing else to repair");
        assert_eq!(repair_cut_lines("", &parse_log("")), None);
    }

    /// A spec em que a onda 1 parou e deixou por fazer as duas tarefas dela,
    /// A e B: o resumo é a entrega 4. `rest` são as linhas que vêm depois, uma
    /// por linha.
    fn stopped_wave_log(rest: &[&str]) -> SpecLog {
        let head = [
            r#"{"v":1,"id":1,"at":"t","type":"wave","n":1,"text":"Uma.","author":"binary"}"#,
            r#"{"v":1,"id":2,"at":"t","type":"task","code":"MSTD-TASK-0001","wave":1,"text":"A."}"#,
            r#"{"v":1,"id":3,"at":"t","type":"task","code":"MSTD-TASK-0002","wave":1,"text":"B."}"#,
            r#"{"v":1,"id":4,"at":"t","type":"delivered","wave":1,"text":"Parei.","undone":["MSTD-TASK-0001","MSTD-TASK-0002"]}"#,
        ];
        parse_log(&head.iter().chain(rest).flat_map(|line| [*line, "\n"]).collect::<String>())
    }

    /// Cada resumo que ainda vale, pelo número da entrega, com as tarefas que
    /// ele acompanha.
    fn live(log: &SpecLog) -> Vec<(u64, Vec<u64>)> {
        log.live_summaries().into_iter().map(|(event, tasks)| (event.id, tasks.into_iter().collect())).collect()
    }

    /// O resumo vale enquanto alguma tarefa que ele deixou não foi entregue,
    /// em quantas ondas ela sair: a onda que parou não entrega o que deixou,
    /// a volta ao backlog o mantém, a onda 2 leva A e entrega, e ele segue só
    /// com B; a onda 3 leva B e entrega, e ele sai da lista.
    #[test]
    fn a_summary_holds_until_every_task_it_left_is_delivered() {
        let back = [r#"{"v":1,"id":5,"at":"t","type":"task","text":"A.","replaces":2}"#, r#"{"v":1,"id":6,"at":"t","type":"task","text":"B.","replaces":3}"#];
        let first = [
            r#"{"v":1,"id":7,"at":"t","type":"wave","n":2,"text":"Duas.","author":"binary","summary":4}"#,
            r#"{"v":1,"id":8,"at":"t","type":"task","text":"A.","wave":2,"replaces":5}"#,
            r#"{"v":1,"id":9,"at":"t","type":"delivered","wave":2,"text":"Saiu."}"#,
        ];
        let second = [
            r#"{"v":1,"id":10,"at":"t","type":"wave","n":3,"text":"Três.","author":"binary","summary":4}"#,
            r#"{"v":1,"id":11,"at":"t","type":"task","text":"B.","wave":3,"replaces":6}"#,
            r#"{"v":1,"id":12,"at":"t","type":"delivered","wave":3,"text":"Saiu."}"#,
        ];
        assert_eq!(live(&stopped_wave_log(&[])), [(4, vec![2, 3])], "the wave that stopped did not deliver them");
        assert_eq!(live(&stopped_wave_log(&back)), [(4, vec![5, 6])], "back in the backlog, the summary holds");
        let after_first: Vec<&str> = back.iter().chain(&first).copied().collect();
        assert_eq!(live(&stopped_wave_log(&after_first)), [(4, vec![6])], "A was delivered, B still waits");
        let after_both: Vec<&str> = after_first.iter().copied().chain(second).collect();
        assert!(live(&stopped_wave_log(&after_both)).is_empty(), "every task it left was delivered");
    }

    /// Quem continuou o resumo e parou de novo escreveu um que o substitui: a
    /// tarefa que a onda 2 deixou por fazer passa ao resumo dela, e o
    /// anterior, sem tarefa por entregar, sai da lista.
    #[test]
    fn a_wave_that_stops_again_replaces_the_summary_it_continued() {
        let log = stopped_wave_log(&[
            r#"{"v":1,"id":5,"at":"t","type":"wave","n":2,"text":"Duas.","author":"binary","summary":4}"#,
            r#"{"v":1,"id":6,"at":"t","type":"task","text":"A.","wave":2,"replaces":2}"#,
            r#"{"v":1,"id":7,"at":"t","type":"task","text":"B.","wave":2,"replaces":3}"#,
            r#"{"v":1,"id":8,"at":"t","type":"delivered","wave":2,"text":"Parei.","undone":["MSTD-TASK-0002"]}"#,
        ]);
        assert_eq!(live(&log), [(8, vec![7])]);
    }
}
