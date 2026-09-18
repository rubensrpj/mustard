//! As ondas e a revisão: um grupo por onda, com o estado que chega pronto de
//! quem lê a rodada, o commit que a fechou, o que o agente dela recebe e o
//! pedido, recolhido; e a revisão e o QA, com a conta dos vereditos de cada
//! onda.

use std::collections::{BTreeMap, BTreeSet};

use super::{code_span, first_paragraph, join, wave_anchor, Page, WaveState};
use crate::domain::spec_events::{Block, SpecEvent};
use crate::view::document::{Group, Node, Status};

impl Page<'_> {
    /// Cada onda do plano, em ordem de número, com o nome dela: a primeira
    /// linha do texto.
    pub(super) fn wave_names(&self) -> BTreeMap<u64, String> {
        self.of_type("wave")
            .into_iter()
            .filter_map(|wave| Some((wave.wave()?, first_paragraph(wave.str_field("text").unwrap_or_default()))))
            .collect()
    }

    /// Um grupo por onda, em ordem de número, com o estado e o nome dela, a
    /// onda, as tarefas, os envios e os entregou dela; as skills vêm no fim.
    ///
    /// A onda leva o estado, o commit que a fechou e o que o agente dela
    /// recebe. O pedido vem recolhido, como o agente o lê: cada envio mostra
    /// o texto exato que foi injetado; a onda ainda não enviada mostra o
    /// pedido que o binário montou, porque é por esta página que a spec é
    /// aprovada, e quem aprova tem de ver cada linha que o agente vai ler, as
    /// instruções fixas incluídas.
    pub(super) fn waves(&self) -> Node {
        let events = self.of_block(Block::Waves);
        let numbers: BTreeSet<u64> = events.iter().filter_map(|e| e.wave()).collect();
        let names = self.wave_names();
        let sends = self.of_type("send");
        let mut groups = Vec::new();
        for n in numbers {
            let mut out = Vec::new();
            let sent: Vec<&SpecEvent> = sends.iter().copied().filter(|s| s.wave() == Some(n)).collect();
            let last_wave_send = sent.iter().rev().find(|s| s.str_field("role") == Some("wave"));
            let prompt = last_wave_send
                .and_then(|s| s.str_field("text"))
                .or_else(|| self.prompts.get(&n).map(String::as_str));
            for event in events.iter().filter(|e| e.wave() == Some(n)) {
                let mut item = self.item(event, true);
                if event.event_type == "wave" {
                    let status = self.wave_status(n);
                    item.fields.push(self.field("page.field.wave_state", status.label.clone()));
                    item.status = Some(status);
                    let commits = self.wave_commits(n);
                    if !commits.is_empty() {
                        item.fields.push(self.field("page.field.wave_commit", commits));
                    }
                    if let Some(parts) = prompt.map(receives).filter(|p| !p.is_empty()) {
                        item.fields.push(self.field("page.field.wave_receives", parts));
                    }
                }
                if event.event_type == "send" {
                    // O texto enviado vem logo abaixo, recolhido.
                    let text = std::mem::take(&mut item.text);
                    let item_code = item.code.clone();
                    out.push(Node::Item(item));
                    if !text.is_empty() {
                        let role = event.str_field("role").map_or_else(String::new, |r| self.value_label(r));
                        out.push(Node::Details {
                            summary: self
                                .t("page.wave.sent")
                                .replace("{role}", &role)
                                .replace("{lines}", &count_lines(&text).to_string()),
                            body: vec![Node::Markdown(text)],
                            owner: Some(item_code),
                        });
                    }
                    continue;
                }
                out.push(Node::Item(item));
            }
            if sent.is_empty()
                && let Some(prompt) = self.prompts.get(&n)
            {
                let heading = self.t("page.wave.prompt").replace("{n}", &n.to_string());
                let lines = self.t("page.wave.prompt.summary").replace("{lines}", &count_lines(prompt).to_string());
                out.push(Node::Details {
                    summary: format!("{heading} · {lines}"),
                    body: vec![Node::Markdown(prompt.clone())],
                    owner: None,
                });
            }
            groups.push(Node::Group(Group {
                anchor: wave_anchor("waves", n),
                title: self.t("page.wave.heading").replace("{n}", &n.to_string()),
                status: names.contains_key(&n).then(|| self.wave_status(n)),
                summary: names.get(&n).cloned().unwrap_or_default(),
                open: false,
                body: out,
            }));
        }
        let skills: Vec<&SpecEvent> = events.iter().copied().filter(|e| e.wave().is_none()).collect();
        groups.extend(self.group("waves-skills".into(), self.t("page.group.skill").into(), &skills));
        self.section(Block::Waves.name(), self.t("page.block.waves"), groups)
    }

    /// A revisão e o QA: um grupo por onda, com a conta dos vereditos.
    pub(super) fn review(&self) -> Node {
        let events = self.of_block(Block::Review);
        let numbers: BTreeSet<Option<u64>> = events.iter().map(|e| e.wave()).collect();
        let body = numbers
            .into_iter()
            .filter_map(|n| {
                let of: Vec<&SpecEvent> = events.iter().copied().filter(|e| e.wave() == n).collect();
                let (anchor, title) = match n {
                    Some(n) => (wave_anchor("review", n), self.t("page.wave.heading").replace("{n}", &n.to_string())),
                    None => ("review-others".to_string(), self.t("page.findings.others").to_string()),
                };
                self.tallied(anchor, title, &of)
            })
            .collect();
        self.section(Block::Review.name(), self.t("page.block.review"), body)
    }

    /// Os commits que fecharam a onda `n`: o `sha` e o código de cada um.
    fn wave_commits(&self, n: u64) -> String {
        join(
            self.of_type("commit")
                .into_iter()
                .filter(|c| c.ints("waves").contains(&n))
                .map(|c| format!("{} ({})", code_span(c.str_field("sha").unwrap_or_default()), self.code(c.id))),
        )
    }

    /// O estado da onda `n`, como chegou de quem lê a rodada.
    pub(super) fn wave_state(&self, n: u64) -> WaveState {
        self.waves.get(&n).copied().unwrap_or(WaveState::Todo)
    }

    pub(super) fn wave_status(&self, n: u64) -> Status {
        let state = self.wave_state(n);
        Status { label: self.t(state.key()).to_string(), tone: state.tone() }
    }

    /// A onda `n` tem commit.
    pub(super) fn wave_committed(&self, n: u64) -> bool {
        self.of_type("commit").iter().any(|c| c.ints("waves").contains(&n))
    }
}

/// O que o agente de uma onda recebe, lido do próprio pedido: cada parte, com
/// quantos itens ela traz, na ordem em que o pedido as escreve.
fn receives(prompt: &str) -> String {
    let mut parts: Vec<(String, usize)> = Vec::new();
    for line in prompt.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            parts.push((title.trim().to_string(), 0));
        } else if let Some(item) = line.strip_prefix("- ")
            && let Some((_, count)) = parts.last_mut()
        {
            *count += listed(item);
        }
    }
    join(parts.into_iter().filter(|(_, n)| *n > 0).map(|(title, n)| format!("{title} ({n})")))
}

/// Quantos itens uma linha de lista do pedido traz: a linha de um bloco da
/// spec — o nome do bloco entre crases, dois-pontos e os códigos separados
/// por vírgula — traz um por código; a linha de uma lição, de uma skill ou de
/// uma regra da execução — e a de um item no pedido antigo, já gravado no
/// envio — traz um só.
fn listed(item: &str) -> usize {
    let codes = item
        .strip_prefix('`')
        .and_then(|rest| rest.split_once("`: "))
        .filter(|(block, _)| Block::parse(block).is_some())
        .map(|(_, codes)| codes);
    codes.map_or(1, |codes| codes.split(", ").count())
}

/// Quantas linhas um texto tem; a última conta mesmo sem quebra no fim.
fn count_lines(text: &str) -> usize {
    text.lines().count()
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;
    use crate::domain::spec_events::parse_log;
    use crate::platform::i18n::Locale;
    use crate::view::document::{
        spec_document, spec_page, Document, Item, Node, SpecInputs, Tone, WavePrompts, WaveState, WaveStates,
    };

    fn page_with(content: &str, waves: &WaveStates) -> Document {
        let prompts = WavePrompts::new();
        spec_page("s", &parse_log(content), SpecInputs { prompts: &prompts, rtk: &[], waves }, Locale::PtBr)
    }

    fn field<'a>(item: &'a Item, label: &str) -> Option<&'a str> {
        item.fields.iter().find(|f| f.label == label).map(|f| f.value.as_str())
    }

    /// O estado de cada onda é o que chega pronto, lido pela rodada: a página
    /// não tem regra própria. Uma onda reprovada aparece reprovada mesmo com
    /// um veredito aprovado e um commit antes; a que não chegou está por
    /// fazer. O estado vai para o grupo da onda, para a linha e o campo da
    /// onda e para a visão das ondas, com a conta dos estados.
    #[test]
    fn the_wave_state_is_the_one_the_round_reading_gives() {
        let wave = |id: u64, n: u64, name: &str| {
            line(id, "wave", &format!(",\"n\":{n},\"text\":\"**{name}**\\n\\nO resto.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"))
        };
        let content = [
            line(1, "criterion", ",\"when\":\"w\",\"then\":\"t\",\"proof\":\"p\",\"origin\":1"),
            wave(2, 1, "Um"),
            wave(3, 2, "Dois"),
            wave(4, 3, "Três"),
            wave(5, 4, "Quatro"),
            wave(6, 5, "Cinco"),
            line(7, "commit", ",\"author\":\"binary\",\"sha\":\"abc\",\"title\":\"t\",\"waves\":[1],\"files\":[],\"repo\":\".\""),
            line(8, "verdict", ",\"author\":\"review\",\"wave\":1,\"result\":\"approved\",\"text\":\"x\",\"criteria\":[]"),
        ]
        .concat();
        let states = WaveStates::from([
            (1, WaveState::Rejected),
            (2, WaveState::Running),
            (3, WaveState::Delivered),
            (4, WaveState::Approved),
            (6, WaveState::Approved),
        ]);
        let doc = page_with(&content, &states);
        let waves = section(&doc, "waves");
        type Shown = (String, Option<(String, Tone)>, String);
        let shown: Vec<Shown> = groups(waves)
            .iter()
            .map(|g| (g.anchor.clone(), g.status.clone().map(|s| (s.label, s.tone)), g.summary.clone()))
            .collect();
        let state = |label: &str, tone: Tone| Some((label.to_string(), tone));
        assert_eq!(
            shown,
            [
                ("waves-1".into(), state("reprovada", Tone::Bad), "**Um**".into()),
                ("waves-2".into(), state("em andamento", Tone::Running), "**Dois**".into()),
                ("waves-3".into(), state("entregue", Tone::Done), "**Três**".into()),
                ("waves-4".into(), state("aprovada", Tone::Good), "**Quatro**".into()),
                ("waves-5".into(), state("a fazer", Tone::Todo), "**Cinco**".into()),
            ]
        );
        let one = items(waves).into_iter().find(|i| i.code == "MSTD-WAVE-0001").unwrap();
        assert_eq!(one.status.as_ref().map(|s| s.label.as_str()), Some("reprovada"));
        assert_eq!(one.fields.iter().find(|f| f.label == "Estado da onda").map(|f| f.value.as_str()), Some("reprovada"));

        let Node::Overview(overview) = &section(&doc, "progress").body[0] else { panic!("the overview opens the progress") };
        assert_eq!(overview.title, "Ondas");
        let cards: Vec<(&str, &str, &str, &str)> = overview
            .cards
            .iter()
            .map(|c| (c.label.as_str(), c.status.label.as_str(), c.target.as_str(), c.hint.as_str()))
            .collect();
        assert_eq!(
            cards,
            [
                ("1", "reprovada", "waves-1", "**Um**"),
                ("2", "em andamento", "waves-2", "**Dois**"),
                ("3", "entregue", "waves-3", "**Três**"),
                ("4", "aprovada", "waves-4", "**Quatro**"),
                ("5", "a fazer", "waves-5", "**Cinco**"),
            ]
        );
        assert_eq!(overview.legend, "1 reprovada · 1 em andamento · 1 entregue · 1 aprovada · 1 a fazer");

        let many = WaveStates::from([(1, WaveState::Approved), (2, WaveState::Approved), (3, WaveState::Delivered)]);
        let counted = page_with(&content, &many);
        let Node::Overview(overview) = &section(&counted, "progress").body[0] else { panic!() };
        assert_eq!(overview.legend, "2 aprovadas · 2 a fazer · 1 entregue", "the highest count first, in plural");

        let english = spec_page(
            "s",
            &parse_log(&content),
            SpecInputs { prompts: &WavePrompts::new(), rtk: &[], waves: &states },
            Locale::EnUs,
        );
        let Node::Overview(overview) = &section(&english, "progress").body[0] else { panic!() };
        assert_eq!(overview.cards[0].status.label, "rejected");

        let empty = spec_document("s", &parse_log(""), &WavePrompts::new(), Locale::PtBr);
        assert!(!section(&empty, "progress").body.iter().any(|n| matches!(n, Node::Overview(_))), "no wave, no overview");
    }

    /// Cada onda mostra o estado, o commit que a fechou e o que o agente
    /// recebe, lido do próprio pedido. A onda enviada traz o texto exato do
    /// envio recolhido logo abaixo dele, para ser lido como markdown; a que
    /// ainda não foi enviada traz, recolhido, o pedido que o binário montou.
    #[test]
    fn each_wave_shows_its_state_commit_what_it_receives_and_the_request() {
        let sent = "# s — onda 1\n\n**O que é isto.** A lista.\n\n## Especificação\n\n- MSTD-CTX-0001 (contexto) — `ler`\n\n## Critérios\n\n- MSTD-CRIT-0001 (critério) — `ler`\n- MSTD-CRIT-0002 (critério) — `ler`\n";
        let content = [
            line(1, "message", ",\"author\":\"user\",\"text\":\"combine\""),
            line(2, "wave", ",\"n\":1,\"text\":\"Um.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            line(3, "wave", ",\"n\":2,\"text\":\"Dois.\",\"criteria\":[1],\"done_when\":\"d\",\"origin\":1"),
            line(4, "send", &format!(",\"author\":\"binary\",\"wave\":1,\"role\":\"wave\",\"text\":{},\"lines\":12,\"chars\":200,\"items\":[2],\"mustard\":\"0.2.0\"", serde_json::to_string(sent).unwrap())),
            line(5, "commit", ",\"author\":\"binary\",\"sha\":\"5e0c7a91\",\"title\":\"t\",\"waves\":[1],\"files\":[\"a.rs\"],\"repo\":\".\""),
        ]
        .concat();
        let mut prompts = WavePrompts::new();
        prompts.insert(1, "não aparece: a onda já foi enviada".into());
        prompts.insert(
            2,
            "# s — onda 2\n\n## Combinado\n\n- `agreed`: MSTD-RULE-0001, MSTD-DEC-0001\n\n## Lições\n\n- `cargo`: rode em primeiro plano.\n".into(),
        );
        let states = WaveStates::from([(1, WaveState::Approved)]);
        let doc = spec_page("s", &parse_log(&content), SpecInputs { prompts: &prompts, rtk: &[], waves: &states }, Locale::PtBr);
        let waves = section(&doc, "waves");

        let one = items(waves).into_iter().find(|i| i.code == "MSTD-WAVE-0001").unwrap();
        assert_eq!(field(one, "Estado da onda"), Some("aprovada"));
        assert_eq!(field(one, "Commit"), Some("`5e0c7a91` (MSTD-COMMIT-0001)"));
        assert_eq!(field(one, "Recebe"), Some("Especificação (1), Critérios (2)"), "{one:?}");
        let first = &group(waves, "waves-1").body;
        let send = Node::items(first).into_iter().find(|i| i.code == "MSTD-SEND-0001").unwrap();
        assert!(send.text.is_empty(), "the sent text goes in the collapsed part");
        let position = first.iter().position(|n| matches!(n, Node::Item(i) if i.code == "MSTD-SEND-0001")).unwrap();
        assert_eq!(
            first[position + 1],
            Node::Details {
                summary: "Pedido enviado (agente de onda) · 12 linhas, como o agente o recebeu".into(),
                body: vec![Node::Markdown(sent.trim().into())],
                owner: Some("MSTD-SEND-0001".into()),
            }
        );
        assert!(
            !first.iter().any(|n| matches!(n, Node::Details { body, .. } if body == &[Node::Markdown("não aparece: a onda já foi enviada".into())])),
            "a sent wave does not show the assembled request again"
        );

        let two = items(waves).into_iter().find(|i| i.code == "MSTD-WAVE-0002").unwrap();
        assert_eq!(field(two, "Estado da onda"), Some("a fazer"));
        assert_eq!(field(two, "Commit"), None);
        // O pedido de hoje traz os códigos de cada bloco numa linha: conta um
        // item por código. A linha da lição, mesmo aberta por um nome entre
        // crases, conta um; o pedido antigo do envio, um por linha.
        assert_eq!(field(two, "Recebe"), Some("Combinado (2), Lições (1)"));
        let prompt = group(waves, "waves-2").body.last().unwrap();
        let Node::Details { summary, body, owner } = prompt else { panic!("{prompt:?}") };
        assert_eq!(owner, &None, "the assembled request belongs to no single item");
        assert_eq!(summary, "O pedido da onda 2 · 9 linhas, como o agente as recebe");
        assert_eq!(body, &[Node::Markdown(prompts[&2].clone())]);
    }
}
