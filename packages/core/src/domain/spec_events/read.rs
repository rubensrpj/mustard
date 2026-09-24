//! A leitura do arquivo: cada linha vira um evento, a linha que não se
//! entende é pulada pelo número, e a leitura mostra só o que não saiu — por
//! bloco, por onda e por passo do fluxo.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::platform::i18n::{translate, Locale};

use super::check::GIVES_BACK_FIELD;
use super::search::{found_by, roots};
use super::{shown_line, type_spec, Block, BlockQuery, METRIC_TYPES, PURGED_FIELD};

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

    /// `true` quando o evento tem todas as raízes do termo, somando as do
    /// `search` e as do código do item (`code`, o que a leitura dá a ele): o
    /// `search` não guarda o código, e sem ele um item não seria achado pelo
    /// próprio código que a página mostra. `None` olha só o `search`.
    #[must_use]
    pub fn matches(&self, terms: &[String], code: Option<&str>) -> bool {
        if terms.is_empty() {
            return true;
        }
        let code_roots = code.map(|c| roots([c])).unwrap_or_default();
        let mut words: BTreeSet<&str> = self.str_field("search").unwrap_or_default().split(' ').collect();
        words.extend(code_roots.iter().map(String::as_str));
        terms.iter().all(|t| words.contains(t.as_str()))
    }

    /// A linha como a leitura mostra, sem o `search`.
    #[must_use]
    pub fn shown(&self) -> String {
        shown_line(&self.fields)
    }
}

pub(super) fn ints(value: Option<&Value>) -> Vec<u64> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

/// O número da onda a que uma linha pertence: `n` na onda, `wave` na tarefa,
/// no envio, no entregou e no veredito. `None` nos outros tipos.
pub(super) fn wave_of(event_type: &str, field: impl Fn(&str) -> Option<u64>) -> Option<u64> {
    match event_type {
        "wave" => field("n"),
        "task" | "send" | "delivered" | "verdict" | "step" => field("wave"),
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
            SkipReason::Unreadable => {
                translate("spec_events.skipped_line", lang).replace("{line}", &self.line.to_string())
            }
            SkipReason::DuplicateId(id) => translate("spec_events.duplicate_id", lang)
                .replace("{line}", &self.line.to_string())
                .replace("{id}", &id.to_string()),
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
    /// Tirar uma dúvida: a conversa, filtrada pelo termo.
    Question { term: String },
}

/// O arquivo lido: os eventos em ordem e as linhas puladas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpecLog {
    pub events: Vec<SpecEvent>,
    pub skipped: Vec<SkippedLine>,
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
            Some(event) if !seen.insert(event.id) => log.skipped.push(SkippedLine {
                line,
                reason: SkipReason::DuplicateId(event.id),
                id_hint: Some(event.id),
            }),
            Some(event) => log.events.push(event),
            None => log.skipped.push(SkippedLine {
                line,
                reason: SkipReason::Unreadable,
                id_hint: id_hint(raw),
            }),
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

impl SpecLog {
    /// O evento de número `id`, removido ou não.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<&SpecEvent> {
        self.events.iter().find(|e| e.id == id)
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
        let removed = self.removals();
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
        let mut in_order: Vec<(u64, u64, Hidden)> = removed
            .iter()
            .filter(|(_, r)| !r.gives_back)
            .map(|(id, r)| (r.by, *id, Hidden::Removed { by: r.by }))
            .collect();
        in_order.extend(self.replacements(&removed).into_iter().map(|(old, by)| (by, old, Hidden::Replaced { by })));
        in_order.sort_by_key(|(by, _, _)| *by);
        for (_, id, why) in in_order {
            hidden.entry(id).or_insert(why);
        }
        for event in self.events.iter().filter(|e| e.returned()) {
            hidden.entry(event.id).or_insert(Hidden::Returned);
        }
        hidden
    }

    /// Os números que uma remoção tira da leitura, cada um com a primeira
    /// remoção que o aponta: os alvos dela e, com o filtro, cada evento do
    /// tipo e do intervalo gravado antes dela.
    fn removals(&self) -> BTreeMap<u64, Removal> {
        let mut removed = BTreeMap::new();
        for event in self.events.iter().filter(|e| e.event_type == "remove") {
            let mut targets = event.ints("targets");
            if let Some(filter) = event.fields.get("filter").and_then(TimeFilter::from_value) {
                targets.extend(self.filter_matches(&filter, event.id));
            }
            let gives_back = event.fields.get(GIVES_BACK_FIELD) == Some(&Value::Bool(true));
            for id in targets {
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
        let gives_back = |id: u64| {
            removed.get(&id).is_some_and(|r| r.gives_back)
                && self.get(id).is_some_and(|e| type_spec(&e.event_type).is_some())
        };
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
        let hidden = self.hidden();
        self.events.iter().filter(|e| !hidden.contains_key(&e.id)).collect()
    }

    /// A versão vigente de um item: segue as substituições que valem a partir
    /// de `id` e devolve a última, se ela está na leitura. Removida pelo
    /// número, pela remoção de hoje, a versão mais nova de um item da spec, a
    /// vigente é a anterior; pela remoção gravada antes da marca, o item não
    /// tem versão vigente.
    #[must_use]
    pub fn current(&self, id: u64) -> Option<&SpecEvent> {
        let replaced_by: BTreeMap<u64, u64> = self.replacements(&self.removals()).into_iter().collect();
        let mut id = id;
        for _ in 0..=self.events.len() {
            match replaced_by.get(&id) {
                Some(next) => id = *next,
                None => break,
            }
        }
        let hidden = self.hidden();
        self.get(id).filter(|e| !hidden.contains_key(&e.id))
    }

    /// As ondas que já têm registro de entrega. O que elas fizeram está
    /// provado pelo código que entrou, e não pelo texto que o descreveu. A
    /// volta que a rodada ainda não assumiu não conta: está fora da leitura.
    #[must_use]
    pub fn delivered_waves(&self) -> BTreeSet<u64> {
        self.block(BlockQuery::Block(Block::Waves))
            .into_iter()
            .filter(|event| event.event_type == "delivered")
            .filter_map(SpecEvent::wave)
            .collect()
    }

    /// As voltas que a rodada ou o fechamento ainda não assumiu, em ordem de
    /// número: a última de cada onda, uma por tipo — a entrega da onda e o
    /// veredito do revisor —, e a do veredito final, que vem sem onda. A
    /// volta é o evento que o próprio agente grava com `returned`, fora da
    /// leitura; a rodada a assume gravando a versão oficial, sem o campo, com
    /// `replaces` para ela. Da versão oficial para trás, nenhuma volta da
    /// mesma onda espera mais, nem a que ela não apontou: só a gravada depois
    /// dela.
    #[must_use]
    pub fn unassumed_returns(&self) -> Vec<&SpecEvent> {
        let hidden = self.hidden();
        let mut official: BTreeMap<(&str, Option<u64>), u64> = BTreeMap::new();
        let mut last: BTreeMap<(&str, Option<u64>), &SpecEvent> = BTreeMap::new();
        for event in self.events.iter().filter(|e| matches!(e.event_type.as_str(), "delivered" | "verdict")) {
            let key = (event.event_type.as_str(), event.wave());
            if !event.returned() {
                let newest = official.entry(key).or_insert(event.id);
                *newest = (*newest).max(event.id);
            } else if hidden.get(&event.id) == Some(&Hidden::Returned)
                && last.get(&key).is_none_or(|kept| kept.id < event.id)
            {
                last.insert(key, event);
            }
        }
        let mut out: Vec<&SpecEvent> = last
            .into_iter()
            .filter(|(key, event)| official.get(key).is_none_or(|id| *id < event.id))
            .map(|(_, event)| event)
            .collect();
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

    /// As ondas do plano: as que a leitura mostra. O que foi gravado em nome
    /// de uma onda que saiu do plano — o veredito, que o binário não deixa
    /// tirar, e o pedido e a entrega — não conta na rodada, no fechamento nem
    /// no pedido.
    #[must_use]
    pub fn planned_waves(&self) -> BTreeSet<u64> {
        self.block(BlockQuery::Block(Block::Waves))
            .into_iter()
            .filter(|e| e.event_type == "wave")
            .filter_map(SpecEvent::wave)
            .collect()
    }

    /// Os vereditos de cada onda do plano, do mais velho ao mais novo.
    #[must_use]
    pub fn verdicts_by_wave(&self) -> BTreeMap<u64, Vec<&SpecEvent>> {
        let planned = self.planned_waves();
        let mut out: BTreeMap<u64, Vec<&SpecEvent>> = BTreeMap::new();
        for verdict in self.block(BlockQuery::Block(Block::Review)).into_iter().filter(|e| e.event_type == "verdict") {
            if let Some(n) = verdict.wave().filter(|n| planned.contains(n)) {
                out.entry(n).or_default().push(verdict);
            }
        }
        out
    }

    /// As ondas do plano cuja última revisão final reprovou, cada uma com o
    /// número dessa reprovação. A rodada, o fechamento e o pedido leem
    /// daqui. Um veredito sem o campo final não conta: só o veredito final
    /// do agente de teste dedicado pode pôr uma onda em modo de conserto.
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

    /// Um bloco, só com o que a leitura mostra. Uma onda (`wave-2`) traz a
    /// onda, as tarefas, os envios e os entregou dela, e as skills que as
    /// tarefas dela nomeiam.
    #[must_use]
    pub fn block(&self, query: BlockQuery) -> Vec<&SpecEvent> {
        let visible = self.visible();
        match query {
            BlockQuery::Block(Block::Metrics) => visible
                .into_iter()
                .filter(|e| METRIC_TYPES.contains(&e.event_type.as_str()))
                .collect(),
            BlockQuery::Block(block) => visible.into_iter().filter(|e| e.block() == Some(block)).collect(),
            BlockQuery::Wave(n) => {
                let skills: BTreeSet<&str> = visible
                    .iter()
                    .filter(|e| e.event_type == "task" && e.wave() == Some(n))
                    .filter_map(|e| e.str_field("skill"))
                    .collect();
                visible
                    .iter()
                    .copied()
                    .filter(|e| e.block() == Some(Block::Waves))
                    .filter(|e| {
                        e.wave() == Some(n)
                            || (e.event_type == "skill"
                                && e.str_field("name").is_some_and(|s| skills.contains(s)))
                    })
                    .collect()
            }
        }
    }

    /// O que um passo do fluxo lê, em ordem de número, sem repetição.
    #[must_use]
    pub fn step(&self, step: &Step) -> Vec<&SpecEvent> {
        let mut picked: BTreeMap<u64, &SpecEvent> = BTreeMap::new();
        match step {
            Step::Resume => pick(&mut picked,self.block(BlockQuery::Block(Block::State))),
            Step::Close => {
                pick(&mut picked,self.block(BlockQuery::Block(Block::State)));
                pick(&mut picked,self.block(BlockQuery::Block(Block::Criteria)));
            }
            Step::Question { term } => {
                let codes = self.codes();
                pick(
                    &mut picked,
                    found_by(self.block(BlockQuery::Block(Block::Conversation)), term, &codes),
                );
            }
            Step::Review { wave } => {
                pick(&mut picked,self.block(BlockQuery::Wave(*wave)));
                pick(&mut picked,self.wave_criteria(*wave));
                pick(&mut picked, crate::domain::wave_prompt::fix_lines(self, *wave));
            }
            Step::Dispatch { wave } => {
                let own = self.block(BlockQuery::Wave(*wave));
                let owned = crate::domain::wave_prompt::agreed_for(self, *wave);
                let depends: Vec<u64> = own
                    .iter()
                    .filter(|e| e.event_type == "wave")
                    .flat_map(|e| e.ints("depends_on"))
                    .collect();
                let delivered: Vec<&SpecEvent> = depends
                    .into_iter()
                    .flat_map(|d| self.block(BlockQuery::Wave(d)))
                    .filter(|e| e.event_type == "delivered")
                    .collect();
                pick(&mut picked,own);
                pick(&mut picked,self.wave_criteria(*wave));
                pick(&mut picked,self.block(BlockQuery::Block(Block::Specification)));
                pick(&mut picked,owned);
                pick(&mut picked,delivered);
                pick(&mut picked, crate::domain::wave_prompt::fix_lines(self, *wave));
            }
        }
        picked.into_values().collect()
    }

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
    /// na ordem do código: o que a rodada prova, uma vez cada, antes de
    /// comitar o que essas ondas entregaram.
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
mod tests {
    use super::*;

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
        assert_eq!(
            log.delivered_files(),
            BTreeSet::from(["src/a.rs".to_string(), "src/b.rs".to_string(), "src/c.rs".to_string()])
        );
    }

    fn log_of(lines: &[serde_json::Value]) -> SpecLog {
        parse_log(&lines.iter().map(serde_json::Value::to_string).collect::<Vec<_>>().join("\n"))
    }

    /// A volta do agente que a rodada assumiu segue substituída pela entrega
    /// oficial mesmo depois que a oficial é removida, pela remoção de hoje,
    /// com a marca, e pela gravada antes dela: a volta fica fora da leitura
    /// sem o motivo de volta, que é o que a rodada lê como espera, e o leitor
    /// de voltas não a devolve.
    #[test]
    fn a_volta_assumida_nao_volta_a_esperar_quando_a_entrega_oficial_sai() {
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
    fn remover_uma_versao_pelo_numero_tira_so_ela() {
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
}
