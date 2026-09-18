//! O andamento: a visão das ondas, o painel de medição (o único grupo aberto
//! da página), as fases e publicações e os commits. O painel sai só dos
//! eventos, mais a economia do rtk nos dias da spec, que chega pronta de quem
//! roda o rtk.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{code_span, join, wave_anchor, Page, RtkDay, WaveState};
use crate::domain::spec_events::{Block, SpecEvent};
use crate::domain::spec_state::original_of;
use crate::domain::survey;
use crate::view::document::{Card, Group, Node, Overview, Table};

impl Page<'_> {
    /// O andamento: a visão das ondas, a medição (o único grupo aberto), as
    /// fases e publicações e os commits.
    pub(super) fn progress(&self) -> Node {
        let mut body: Vec<Node> = self.overview().map(Node::Overview).into_iter().collect();
        let metrics = self.metrics();
        if !metrics.is_empty() {
            body.push(Node::Group(Group {
                anchor: "progress-metrics".to_string(),
                title: self.t("page.group.metrics").to_string(),
                status: None,
                summary: String::new(),
                open: true,
                body: metrics,
            }));
        }
        let title = |key: &str| self.t(key).to_string();
        body.extend(self.group("progress-state".into(), title("page.group.state"), &self.of_block(Block::State)));
        body.extend(self.group("progress-commits".into(), title("page.group.commits"), &self.of_block(Block::Progress)));
        self.section(Block::Progress.name(), self.t("page.block.progress"), body)
    }

    /// A visão das ondas: uma ficha por onda do plano, com o estado dela e o
    /// atalho para o grupo dela, e a conta dos estados. Sem onda, nada.
    fn overview(&self) -> Option<Overview> {
        let cards: Vec<Card> = self
            .wave_names()
            .into_iter()
            .map(|(n, name)| Card {
                target: wave_anchor("waves", n),
                label: n.to_string(),
                status: self.wave_status(n),
                hint: name,
            })
            .collect();
        if cards.is_empty() {
            return None;
        }
        let mut tally: Vec<(WaveState, usize)> = Vec::new();
        for n in cards.iter().filter_map(|card| card.label.parse::<u64>().ok()) {
            let state = self.wave_state(n);
            match tally.iter_mut().find(|(s, _)| *s == state) {
                Some((_, count)) => *count += 1,
                None => tally.push((state, 1)),
            }
        }
        // A conta mais alta vem primeiro; no empate, a que aparece antes.
        tally.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let legend = tally
            .into_iter()
            .map(|(state, count)| {
                let key = if count > 1 { format!("{}.many", state.key()) } else { state.key().to_string() };
                format!("{count} {}", self.t(&key))
            })
            .collect::<Vec<_>>()
            .join(" · ");
        Some(Overview { title: self.t("page.block.waves").to_string(), legend, cards })
    }

    /// O painel: uma linha por medida que tem dado, e depois a medida de cada
    /// onda: o pedido, a entrega e o que a revisão achou dela.
    fn metrics(&self) -> Vec<Node> {
        let count = |events: &[&SpecEvent], field: &str, word: &str| {
            events.iter().filter(|e| e.str_field(field) == Some(word)).count()
        };
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut row = |key: &str, value: String| rows.push(vec![self.t(key).to_string(), value]);

        let injections = self.of_type("injection");
        if !injections.is_empty() {
            let chars: u64 = injections.iter().filter_map(|e| e.int("chars")).sum();
            row(
                "page.metrics.injected",
                self.t("page.metrics.injected.value")
                    .replace("{chars}", &chars.to_string())
                    .replace("{tokens}", &(chars / 4).to_string()),
            );
        }
        let hooks = self.of_type("hook");
        if !hooks.is_empty() {
            let names: BTreeSet<&str> = hooks.iter().filter_map(|e| e.str_field("hook")).collect();
            let hook_value = |events: &[&SpecEvent]| {
                self.t("page.metrics.hooks.value")
                    .replace("{blocks}", &count(events, "action", "block").to_string())
                    .replace("{warns}", &count(events, "action", "warn").to_string())
            };
            let by_hook = names
                .into_iter()
                .map(|name| {
                    let of: Vec<&SpecEvent> =
                        hooks.iter().copied().filter(|e| e.str_field("hook") == Some(name)).collect();
                    format!("{}: {}", code_span(name), hook_value(&of))
                })
                .collect::<Vec<_>>()
                .join("; ");
            row("page.metrics.hooks", format!("{} — {by_hook}", hook_value(&hooks)));
        }
        let calls = self.of_type("call");
        if !calls.is_empty() {
            let mut by_command: BTreeMap<&str, usize> = BTreeMap::new();
            for call in &calls {
                *by_command.entry(call.str_field("command").unwrap_or("?")).or_default() += 1;
            }
            let done = self.of_type("wave").iter().filter_map(|w| w.wave()).filter(|n| self.wave_committed(*n)).count();
            row(
                "page.metrics.calls",
                format!(
                    "{} — {}",
                    self.t("page.metrics.calls.value")
                        .replace("{count}", &calls.len().to_string())
                        .replace("{refused}", &count(&calls, "result", "refused").to_string())
                        .replace("{done}", &done.to_string()),
                    join(by_command.into_iter().map(|(command, n)| format!("{} {n}", code_span(command)))),
                ),
            );
        }
        if let Some(phases) = self.phase_times() {
            row("page.metrics.phases", phases);
        }
        let verdicts = self.of_type("verdict");
        if !verdicts.is_empty() {
            let rejected: BTreeSet<u64> = verdicts
                .iter()
                .filter(|v| v.str_field("result") == Some("rejected"))
                .filter_map(|v| v.wave())
                .collect();
            let mut value = self
                .t("page.metrics.verdicts.value")
                .replace("{approved}", &count(&verdicts, "result", "approved").to_string())
                .replace("{rejected}", &count(&verdicts, "result", "rejected").to_string());
            if !rejected.is_empty() {
                value.push_str(
                    &self
                        .t("page.metrics.rework")
                        .replace("{waves}", &join(rejected.iter().map(u64::to_string))),
                );
            }
            row("page.metrics.verdicts", value);
        }
        // Os pontos contam pela mesma leitura do levantamento: o ponto revisto
        // e fechado pela primeira versão aparece fechado aqui e no `grill`.
        let points = self.of_type("point");
        if !points.is_empty() {
            let open = survey::open_points(self.log).len();
            let closed = survey::closed_points(self.log).len();
            row(
                "page.metrics.points",
                self.t("page.metrics.points.value")
                    .replace("{open}", &open.to_string())
                    .replace("{closed}", &closed.to_string()),
            );
            let reminders: usize = points
                .iter()
                .filter_map(|p| p.fields.get("reminders").and_then(Value::as_array))
                .map(Vec::len)
                .sum();
            row("page.metrics.reminders", self.t("page.metrics.reminders.value").replace("{count}", &reminders.to_string()));
        }
        let sends = self.of_type("send");
        if !sends.is_empty() {
            let largest = sends.iter().filter_map(|e| e.int("lines")).max().unwrap_or(0);
            row(
                "page.metrics.sends",
                self.t("page.metrics.sends.value")
                    .replace("{count}", &sends.len().to_string())
                    .replace("{lines}", &largest.to_string()),
            );
        }
        if let Some(rtk) = self.rtk_savings() {
            row("page.metrics.rtk", rtk);
        }
        if rows.is_empty() {
            return Vec::new();
        }
        let mut out = vec![Node::Table(Table {
            headers: vec![
                self.t("page.metrics.col.measure").to_string(),
                self.t("page.metrics.col.value").to_string(),
            ],
            rows,
        })];
        if let Some(table) = self.size_against_review(&sends, &verdicts) {
            out.push(Node::Heading { level: 3, text: self.t("page.metrics.by_wave").to_string() });
            out.push(table);
        }
        out
    }

    /// Para cada onda enviada ao agente dela, pelo último pedido: as linhas e
    /// os caracteres dele, quantos itens ele deu para o agente ler (contados
    /// no envio, sem registro de cada leitura), o tempo até a entrega que
    /// respondeu a ele, quantas vezes a revisão reprovou a onda e o resultado
    /// da última revisão. O pedido ainda sem entrega fica sem tempo.
    fn size_against_review(&self, sends: &[&SpecEvent], verdicts: &[&SpecEvent]) -> Option<Node> {
        let mut last_send: BTreeMap<u64, &SpecEvent> = BTreeMap::new();
        for send in sends.iter().copied().filter(|s| s.str_field("role") == Some("wave") && s.int("lines").is_some()) {
            if let Some(n) = send.wave() {
                last_send.insert(n, send);
            }
        }
        if last_send.is_empty() {
            return None;
        }
        let deliveries = self.of_type("delivered");
        let none = || "—".to_string();
        let rows = last_send
            .into_iter()
            .map(|(n, send)| {
                let of: Vec<&SpecEvent> = verdicts.iter().copied().filter(|v| v.wave() == Some(n)).collect();
                let rejected = of.iter().filter(|v| v.str_field("result") == Some("rejected")).count();
                let last = of.last().and_then(|v| v.str_field("result")).map_or_else(none, |r| self.value_label(r));
                let delivery = deliveries
                    .iter()
                    .find(|d| d.wave() == Some(n) && d.id > send.id)
                    .and_then(|d| seconds_between(send.at(), d.at()))
                    .map_or_else(none, duration);
                let number = |field: &str| send.int(field).map_or_else(none, |v| v.to_string());
                let items = send.ints("items").len().to_string();
                vec![n.to_string(), number("lines"), number("chars"), items, delivery, rejected.to_string(), last]
            })
            .collect();
        Some(Node::Table(Table {
            headers: ["wave", "lines", "chars", "items", "delivery", "rejected", "last"]
                .iter()
                .map(|column| self.t(&format!("page.metrics.col.{column}")).to_string())
                .collect(),
            rows,
        }))
    }

    /// O tempo que a spec passou em cada fase, na ordem em que as fases
    /// apareceram. Cada fase vai do estado que a abriu ao estado seguinte que
    /// mudou a fase; a fase atual vai até o último evento da spec. Um estado
    /// revisto conta do lugar e da hora da primeira versão dele, como na
    /// leitura do estado.
    fn phase_times(&self) -> Option<String> {
        let mut states: Vec<(u64, &SpecEvent)> = self
            .of_type("state")
            .into_iter()
            .map(|state| (original_of(self.log, state), state))
            .collect();
        states.sort_by_key(|(first, state)| (*first, state.id));
        let mut spans: Vec<(&str, &str)> = Vec::new();
        for (first, state) in states {
            let Some(phase) = state.str_field("phase") else {
                continue;
            };
            if spans.last().is_none_or(|(current, _)| *current != phase) {
                let at = self.log.get(first).map_or_else(|| state.at(), SpecEvent::at);
                spans.push((phase, at));
            }
        }
        let end = self.log.events.last()?.at();
        let mut totals: Vec<(&str, i64)> = Vec::new();
        for (i, (phase, from)) in spans.iter().enumerate() {
            let to = spans.get(i + 1).map_or(end, |(_, at)| *at);
            let secs = seconds_between(from, to)?;
            match totals.iter_mut().find(|(p, _)| p == phase) {
                Some((_, total)) => *total += secs,
                None => totals.push((phase, secs)),
            }
        }
        (!totals.is_empty())
            .then(|| join(totals.into_iter().map(|(phase, secs)| format!("{} {}", self.phase(phase), duration(secs)))))
    }

    /// A economia do rtk nos dias em que a spec teve eventos, somada dos
    /// números que o próprio rtk dá, com o primeiro e o último dia que
    /// contaram. Sem comando nenhum nesses dias, nada.
    fn rtk_savings(&self) -> Option<String> {
        let from = self.log.events.first()?.at().get(..10)?;
        let to = self.log.events.last()?.at().get(..10)?;
        let days: Vec<&RtkDay> =
            self.rtk.iter().filter(|d| d.date.as_str() >= from && d.date.as_str() <= to).collect();
        let commands: u64 = days.iter().map(|d| d.commands).sum();
        if commands == 0 {
            return None;
        }
        let input: u64 = days.iter().map(|d| d.input).sum();
        let saved: u64 = days.iter().map(|d| d.saved).sum();
        let pct = (saved * 100 + input / 2).checked_div(input).unwrap_or(0);
        let counted: Vec<&str> = days.iter().filter(|d| d.commands > 0).map(|d| d.date.as_str()).collect();
        Some(
            self.t("page.metrics.rtk.value")
                .replace("{commands}", &commands.to_string())
                .replace("{saved}", &saved.to_string())
                .replace("{pct}", &pct.to_string())
                .replace("{from}", counted.iter().min().copied().unwrap_or(from))
                .replace("{to}", counted.iter().max().copied().unwrap_or(to)),
        )
    }
}

/// Os segundos entre duas horas gravadas com o fuso; `None` se uma delas não
/// se lê.
fn seconds_between(from: &str, to: &str) -> Option<i64> {
    let from = chrono::DateTime::parse_from_rfc3339(from).ok()?;
    let to = chrono::DateTime::parse_from_rfc3339(to).ok()?;
    Some((to - from).num_seconds().max(0))
}

/// Uma duração curta de ler: "< 1 min", "35 min", "2 h 05 min",
/// "3 d 4 h".
fn duration(secs: i64) -> String {
    let minutes = secs / 60;
    match minutes {
        0 => "< 1 min".to_string(),
        1..=59 => format!("{minutes} min"),
        60..=1439 => format!("{} h {:02} min", minutes / 60, minutes % 60),
        _ => format!("{} d {} h", minutes / 1440, minutes % 1440 / 60),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::tests::*;
    use crate::domain::spec_events::parse_log;
    use crate::platform::i18n::Locale;
    use crate::view::document::{spec_document, spec_page, Group, Node, RtkDay, SpecInputs, WavePrompts, WaveStates};

    /// O painel sai dos eventos: o texto colocado, os bloqueios de cada
    /// gancho, as chamadas dos passos contra as ondas prontas, o tempo de cada
    /// fase, o retrabalho, os lembretes, a medida de cada onda (o pedido, os
    /// itens que ele deu para ler, o tempo até a entrega e a revisão) e a
    /// economia do rtk nos dias da spec. Ele é o grupo aberto do andamento.
    #[test]
    fn the_panel_measures_hooks_steps_phases_rework_reminders_sizes_and_rtk() {
        let at = |id: u64, when: &str, event_type: &str, extra: &str| {
            format!("{{\"v\":1,\"id\":{id},\"at\":\"{when}\",\"type\":\"{event_type}\",\"author\":\"binary\"{extra}}}\n")
        };
        let content = [
            at(1, "2026-09-11T08:00:00-03:00", "state", ",\"phase\":\"survey\""),
            at(2, "2026-09-11T08:00:30-03:00", "injection", ",\"hook\":\"session_start_inject\",\"chars\":400,\"text\":\"t\""),
            at(3, "2026-09-11T08:01:00-03:00", "hook", ",\"hook\":\"command_guard\",\"action\":\"block\",\"tool\":\"Bash\",\"reason\":\"r\""),
            at(4, "2026-09-11T08:02:00-03:00", "hook", ",\"hook\":\"command_guard\",\"action\":\"block\",\"tool\":\"Bash\",\"reason\":\"r\""),
            at(5, "2026-09-11T08:03:00-03:00", "hook", ",\"hook\":\"write_gate\",\"action\":\"warn\",\"tool\":\"Edit\",\"reason\":\"r\""),
            at(6, "2026-09-11T08:04:00-03:00", "point", ",\"author\":\"assistant\",\"block\":\"b\",\"gap\":\"g\",\"from\":\"gap\",\"status\":\"open\",\"facts\":[{\"text\":\"f\",\"source\":\"a.rs:1\"}],\"reminders\":[1,2],\"origin\":1"),
            at(7, "2026-09-11T08:05:00-03:00", "call", ",\"command\":\"grill\",\"ms\":5,\"result\":\"ok\""),
            at(8, "2026-09-11T10:05:00-03:00", "state", ",\"phase\":\"plan\""),
            at(9, "2026-09-11T10:06:00-03:00", "call", ",\"command\":\"plan\",\"ms\":5,\"result\":\"refused\""),
            at(10, "2026-09-11T10:35:00-03:00", "call", ",\"command\":\"plan\",\"ms\":5,\"result\":\"ok\""),
            at(11, "2026-09-11T10:35:00-03:00", "wave", ",\"author\":\"assistant\",\"n\":1,\"text\":\"w\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            at(12, "2026-09-11T10:36:00-03:00", "state", ",\"author\":\"user\",\"phase\":\"approved\",\"witness\":{\"question\":\"q\",\"answer\":\"a\"}"),
            at(13, "2026-09-11T10:36:00-03:00", "state", ",\"phase\":\"running\""),
            at(14, "2026-09-11T10:37:00-03:00", "send", ",\"wave\":1,\"role\":\"wave\",\"text\":\"p\",\"lines\":300,\"chars\":9,\"items\":[11],\"mustard\":\"0\""),
            at(15, "2026-09-11T11:07:00-03:00", "delivered", ",\"author\":\"wave\",\"wave\":1,\"text\":\"e\",\"files\":[]"),
            at(16, "2026-09-12T11:00:00-03:00", "verdict", ",\"author\":\"review\",\"wave\":1,\"result\":\"rejected\",\"text\":\"x\",\"criteria\":[]"),
            at(17, "2026-09-12T11:30:00-03:00", "send", ",\"wave\":1,\"role\":\"wave\",\"text\":\"p\",\"lines\":320,\"chars\":12000,\"items\":[11,1,6],\"mustard\":\"0\""),
            at(18, "2026-09-12T11:50:00-03:00", "delivered", ",\"author\":\"wave\",\"wave\":1,\"text\":\"e\",\"files\":[]"),
            at(19, "2026-09-12T12:00:00-03:00", "commit", ",\"sha\":\"abc\",\"title\":\"t\",\"waves\":[1],\"files\":[],\"repo\":\".\""),
            at(20, "2026-09-12T13:00:00-03:00", "verdict", ",\"author\":\"review\",\"wave\":1,\"result\":\"approved\",\"text\":\"x\",\"criteria\":[]"),
            at(21, "2026-09-13T12:36:00-03:00", "call", ",\"command\":\"round\",\"ms\":5,\"result\":\"ok\""),
            at(22, "2026-09-13T12:36:00-03:00", "send", ",\"wave\":2,\"role\":\"wave\",\"text\":\"p\",\"lines\":5,\"chars\":40,\"items\":[11],\"mustard\":\"0\""),
        ]
        .concat();
        let day = |date: &str, commands: u64, input: u64, saved: u64| RtkDay { date: date.into(), commands, input, saved };
        let rtk = [
            day("2026-09-10", 99, 9_999, 9_999),
            day("2026-09-11", 3, 1_000, 300),
            day("2026-09-13", 1, 1_000, 100),
            day("2026-09-14", 99, 9_999, 9_999),
        ];
        let prompts = WavePrompts::new();
        let waves = WaveStates::new();
        let panel = |rtk: &[RtkDay]| -> Group {
            let doc = spec_page("s", &parse_log(&content), SpecInputs { prompts: &prompts, rtk, waves: &waves }, Locale::PtBr);
            group(section(&doc, "progress"), "progress-metrics").clone()
        };
        let metrics = panel(&rtk);
        assert!(metrics.open, "the measurement opens with the page");
        let Node::Table(table) = &metrics.body[0] else { panic!("{metrics:?}") };
        let rows: BTreeMap<&str, &str> = table.rows.iter().map(|r| (r[0].as_str(), r[1].as_str())).collect();
        assert_eq!(rows["Texto colocado pelos ganchos"], "400 caracteres, cerca de 100 tokens");
        assert_eq!(
            rows["Bloqueios por gancho"],
            "2 bloqueios, 1 avisos — `command_guard`: 2 bloqueios, 0 avisos; `write_gate`: 0 bloqueios, 1 avisos"
        );
        assert_eq!(
            rows["Passos do fluxo contra trabalho"],
            "4 chamadas, 1 recusadas, para 1 ondas prontas — `grill` 1, `plan` 2, `round` 1"
        );
        assert_eq!(rows["Tempo por fase"], "levantamento 2 h 05 min, plano 31 min, aprovada < 1 min, em execução 2 d 2 h");
        assert_eq!(rows["Revisões"], "1 aprovadas, 1 reprovadas; voltaram da revisão as ondas 1");
        assert_eq!(rows["Lembretes que apareceram"], "2 mensagens antigas lembradas nos pontos");
        assert_eq!(rows["Pedidos enviados aos agentes"], "3, o maior com 320 linhas");
        assert_eq!(rows["Economia do rtk"], "4 comandos, 400 tokens a menos na saída (20%), de 2026-09-11 a 2026-09-13");
        assert_eq!(metrics.body[1], Node::Heading { level: 3, text: "Medida por onda".into() });
        let Node::Table(by_wave) = &metrics.body[2] else { panic!("{metrics:?}") };
        assert_eq!(
            by_wave.headers,
            [
                "Onda",
                "Linhas do pedido",
                "Caracteres do pedido",
                "Itens lidos",
                "Tempo até a entrega",
                "Reprovações",
                "Última revisão"
            ]
        );
        // Cada onda sai pelo último pedido: o tamanho dele, os itens que ele
        // deu para ler e o tempo até a entrega que respondeu a ele. O pedido
        // ainda sem entrega fica sem tempo.
        assert_eq!(
            by_wave.rows,
            [["1", "320", "12000", "3", "20 min", "1", "aprovada"], ["2", "5", "40", "1", "—", "0", "—"]]
        );

        // Sem comando do rtk nos dias da spec, a linha não aparece.
        let quiet = panel(&[day("2026-09-10", 5, 10, 1)]);
        let Node::Table(table) = &quiet.body[0] else { panic!() };
        assert!(table.rows.iter().all(|r| r[0] != "Economia do rtk"), "{table:?}");
    }

    /// Um estado revisto depois conta do lugar e da hora da primeira versão:
    /// corrigir a branch do primeiro estado não muda o tempo das fases.
    #[test]
    fn a_revised_state_keeps_the_time_of_its_first_version() {
        let at = |id: u64, when: &str, extra: &str| {
            format!("{{\"v\":1,\"id\":{id},\"at\":\"2026-09-11T{when}:00-03:00\",\"type\":\"state\",\"author\":\"binary\"{extra}}}\n")
        };
        let content = [
            at(1, "08:00", ",\"phase\":\"survey\""),
            at(2, "09:00", ",\"phase\":\"plan\""),
            at(3, "09:30", ",\"phase\":\"survey\",\"branch\":\"feature/x\",\"replaces\":1"),
        ]
        .concat();
        let doc = spec_document("s", &parse_log(&content), &WavePrompts::new(), Locale::PtBr);
        let metrics = group(section(&doc, "progress"), "progress-metrics");
        let Node::Table(table) = &metrics.body[0] else { panic!() };
        let phases = table.rows.iter().find(|r| r[0] == "Tempo por fase").map(|r| r[1].as_str());
        assert_eq!(phases, Some("levantamento 1 h 00 min, plano 30 min"));
    }
}
