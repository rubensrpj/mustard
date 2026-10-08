//! Public status snapshot, rendered inside Mustard's shared page layout.
//! Only the publication allowlist reaches this renderer; all values are escaped.

use super::{NavSection, Report, escape};
use serde_json::Value;
use std::fmt::Write as _;

pub(crate) fn public_snapshot(snapshot: &Value) -> String {
    let english = snapshot["language"] == "en-US";
    let t = |pt: &'static str, en: &'static str| if english { en } else { pt };
    let state = |value: &Value| {
        let raw = value.as_str().unwrap_or_default();
        match raw {
            "planned" => t("Planejada", "Planned").to_string(),
            "received" => t("Retorno recebido", "Return received").to_string(),
            "integrated" => t("Integrada", "Integrated").to_string(),
            "committed" => t("Comitada", "Committed").to_string(),
            "repair-required" => t("Conserto necessário", "Repair required").to_string(),
            _ => {
                let lang = if english {
                    mustard_core::platform::i18n::Locale::EnUs
                } else {
                    mustard_core::platform::i18n::Locale::PtBr
                };
                let label =
                    mustard_core::platform::i18n::translate(&format!("page.phase.{raw}"), lang);
                if label == "<missing-key>" {
                    t("Desconhecido", "Unknown").to_string()
                } else {
                    label.to_string()
                }
            }
        }
    };
    let validation = match snapshot["final_validation_valid"].as_bool() {
        Some(true) => t("Válida", "Valid"),
        Some(false) => t("Pendente", "Pending"),
        None => t("Desconhecida", "Unknown"),
    };
    let review = if snapshot["review_approved"] == true {
        t("Aprovada", "Approved")
    } else {
        t("Pendente", "Pending")
    };
    let waves: Vec<&Value> = snapshot["waves"].as_array().into_iter().flatten().collect();
    let done = waves
        .iter()
        .filter(|w| matches!(w["status"].as_str(), Some("committed" | "integrated")))
        .count();
    let percent = if waves.is_empty() {
        0
    } else {
        done * 100 / waves.len()
    };
    let title = format!(
        "{} · {}",
        snapshot["project"].as_str().unwrap_or("Mustard"),
        snapshot["spec"].as_str().unwrap_or_default()
    );
    let raw_date = snapshot["at"].as_str().unwrap_or_default();
    let date = chrono::DateTime::parse_from_rfc3339(raw_date)
        .map(|date| {
            date.with_timezone(&chrono::Utc)
                .format(if english {
                    "%Y-%m-%d · %H:%M UTC"
                } else {
                    "%d/%m/%Y · %H:%M UTC"
                })
                .to_string()
        })
        .unwrap_or_else(|_| raw_date.to_string());
    let mut report = Report::new(
        title,
        format!("{} {date}", t("Retrato gerado em", "Snapshot generated at")),
    )
    .with_lang(if english { "en-US" } else { "pt-BR" })
    .with_kind(t("Acompanhamento", "Status"));
    let mut body = String::new();
    let sections = [
        ("overview", t("Visão geral", "Overview"), 3),
        ("waves", t("Ondas", "Waves"), waves.len()),
    ];
    for (id, label, count) in sections {
        report.nav(NavSection {
            id: id.into(),
            title: label.into(),
            count,
            groups: Vec::new(),
        });
    }
    let _ = write!(
        body,
        "<section id=\"overview\" class=\"block snapshot-overview\" data-crumb=\"{}\"><h2>{}</h2><div class=\"status-grid\">",
        escape(sections[0].1),
        escape(sections[0].1)
    );
    for (label, value) in [
        (t("Etapa", "Stage"), state(&snapshot["phase"])),
        (t("Validação final", "Final validation"), validation.into()),
        (t("Revisão", "Review"), review.into()),
    ] {
        card(&mut body, label, &value);
    }
    let _ = write!(
        body,
        "</div><div class=\"wave-progress\"><span>{}: <b>{done}/{}</b></span><div role=\"progressbar\" aria-label=\"{}\" aria-valuemin=\"0\" aria-valuemax=\"100\" aria-valuenow=\"{percent}\"><span style=\"width:{percent}%\"></span></div></div></section>",
        t("Ondas concluídas", "Completed waves"),
        waves.len(),
        t("Ondas concluídas", "Completed waves")
    );
    let _ = write!(
        body,
        "<section id=\"waves\" class=\"block\" data-crumb=\"{}\"><h2>{}</h2><div class=\"wave-grid\">",
        escape(sections[1].1),
        escape(sections[1].1)
    );
    for wave in &waves {
        let n = wave["wave"]
            .as_u64()
            .map_or_else(|| "?".into(), |n| n.to_string());
        let label = state(&wave["status"]);
        let _ = write!(
            body,
            "<article class=\"wave-card\"><span class=\"wave-number\">{} {}</span><strong>{}</strong></article>",
            t("Onda", "Wave"),
            escape(&n),
            escape(&label)
        );
    }
    if waves.is_empty() {
        let _ = write!(
            body,
            "<p class=\"muted\">{}</p>",
            t("Nenhuma onda planejada.", "No planned waves.")
        );
    }
    body.push_str("</div></section>");
    if let Some(usage) = snapshot.get("consumption") {
        let label = t("Consumo", "Usage");
        report.nav(NavSection {
            id: "usage".into(),
            title: label.into(),
            count: 2,
            groups: Vec::new(),
        });
        let _ = write!(
            body,
            "<section id=\"usage\" class=\"block\" data-crumb=\"{label}\"><h2>{label}</h2><div class=\"status-grid\">"
        );
        for (key, label) in [
            ("wave_tokens", t("Tokens das ondas", "Wave tokens")),
            (
                "conductor_tokens",
                t("Tokens do condutor", "Conductor tokens"),
            ),
        ] {
            let number = usage[key]
                .as_u64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| t("Desconhecido", "Unknown").into());
            card(&mut body, label, &number);
        }
        body.push_str("</div></section>");
    }
    let _ = write!(
        body,
        "<footer>{}</footer>",
        t(
            "Retrato gerado sob pedido explícito. Para atualizar, solicite uma nova publicação.",
            "Snapshot generated on explicit request. Request a new publication to update it."
        )
    );
    report.raw(&body).render()
}

fn card(body: &mut String, label: &str, value: &str) {
    let _ = write!(
        body,
        "<article class=\"status-card\"><span>{}</span><strong>{}</strong></article>",
        escape(label),
        escape(value)
    );
}

/// Machine expense snapshots use the same static, responsive layout. All
/// data is native/allowlisted; no external database or JavaScript SDK.
pub(crate) fn public_spend_snapshot(snapshot: &Value) -> String {
    let english = snapshot["language"] == "en-US";
    let t = |pt, en| if english { en } else { pt };
    let title = t("Consumo por dia", "Daily usage");
    let mut report = Report::new(
        title,
        format!(
            "{} {}",
            t("Retrato gerado em", "Snapshot generated at"),
            snapshot["at"].as_str().unwrap_or_default()
        ),
    )
    .with_lang(if english { "en-US" } else { "pt-BR" })
    .with_kind(t("Consumo", "Usage"));
    report.nav(NavSection {
        id: "overview".into(),
        title: t("Resumo", "Summary").into(),
        count: 6,
        groups: Vec::new(),
    });
    let rows = snapshot["rows"].as_array().cloned().unwrap_or_default();
    report.nav(NavSection {
        id: "days".into(),
        title: t("Dias e projetos", "Days and projects").into(),
        count: rows.len(),
        groups: Vec::new(),
    });
    let mut body = format!(
        "<section id=\"overview\" class=\"block\"><h2>{}</h2><div class=\"status-grid\">",
        t("Resumo", "Summary")
    );
    let summary = &snapshot["summary"];
    for (label, value) in [
        (
            t("Tokens de hoje · parcial", "Today's tokens · partial"),
            summary["today"]["tokens"].as_u64().map(|n| n.to_string()),
        ),
        (
            t("Tokens de ontem", "Yesterday's tokens"),
            summary["yesterday"]["tokens"]
                .as_u64()
                .map(|n| n.to_string()),
        ),
        (
            t("Média de tokens · 3 dias", "Average tokens · 3 days"),
            summary["last_3"]["tokens"].as_u64().map(|n| n.to_string()),
        ),
        (
            t("Média de tokens · 7 dias", "Average tokens · 7 days"),
            summary["last_7"]["tokens"].as_u64().map(|n| n.to_string()),
        ),
        (
            t("Jev registrado · hoje", "Recorded Jev · today"),
            summary["today"]["jev_cost_micro_usd"]
                .as_u64()
                .map(|n| format!("US$ {:.4}", n as f64 / 1_000_000.0)),
        ),
        (
            t("Previsão mensal · Jev", "Monthly forecast · Jev"),
            summary["forecast"]["micro_usd"]
                .as_u64()
                .map(|n| format!("US$ {:.4}", n as f64 / 1_000_000.0)),
        ),
    ] {
        card(
            &mut body,
            label,
            &value.unwrap_or_else(|| t("Desconhecido", "Unknown").into()),
        );
    }
    let _ = write!(
        body,
        "</div><p class=\"muted\">{} {} {}.</p></section>",
        t(
            "Médias consideram dias fechados com pelo menos",
            "Averages include closed days with at least"
        ),
        summary["min_actions"].as_u64().unwrap_or_default(),
        t("ações", "actions")
    );
    let _ = write!(
        body,
        "<section id=\"days\" class=\"block\"><h2>{}</h2><div class=\"table\"><table><thead><tr>",
        t("Dias e projetos", "Days and projects")
    );
    for label in [
        t("Dia", "Day"),
        t("Projeto", "Project"),
        "Tokens",
        t("Ações", "Actions"),
        t("Procuras", "Searches"),
        "Jev · US$",
        t("Estado", "State"),
    ] {
        let _ = write!(body, "<th>{}</th>", escape(label));
    }
    body.push_str("</tr></thead><tbody>");
    for row in rows.iter().rev() {
        body.push_str("<tr>");
        let values = [
            row["day"].as_str().unwrap_or_default().to_string(),
            row["project"].as_str().unwrap_or_default().to_string(),
            row["tokens"].as_u64().unwrap_or_default().to_string(),
            row["actions"].as_u64().unwrap_or_default().to_string(),
            row["code_searches"]
                .as_u64()
                .unwrap_or_default()
                .to_string(),
            format!(
                "{:.4}",
                row["jev_cost_micro_usd"].as_u64().unwrap_or_default() as f64 / 1_000_000.0
            ),
            if row["partial"] == true {
                t("Parcial", "Partial")
            } else {
                t("Fechado", "Closed")
            }
            .to_string(),
        ];
        for value in values {
            let _ = write!(body, "<td>{}</td>", escape(&value));
        }
        body.push_str("</tr>");
    }
    let _ = write!(
        body,
        "</tbody></table></div></section><footer>{}</footer>",
        t(
            "Retrato publicado sob pedido explícito. Custo exibido corresponde ao Jev registrado; tokens não equivalem ao custo total do modelo.",
            "Snapshot published on explicit request. Displayed cost is recorded Jev usage; tokens are not total model cost."
        )
    );
    report.raw(&body).render()
}

pub(crate) fn public_project_snapshot(snapshot: &Value) -> String {
    let english = snapshot["language"] == "en-US";
    let t = |pt, en| if english { en } else { pt };
    let specs = snapshot["specs"].as_array().cloned().unwrap_or_default();
    let mut report = Report::new(
        snapshot["project"].as_str().unwrap_or("Mustard"),
        format!(
            "{} {}",
            t("Retrato do projeto em", "Project snapshot at"),
            snapshot["at"].as_str().unwrap_or_default()
        ),
    )
    .with_lang(if english { "en-US" } else { "pt-BR" })
    .with_kind(t("Projeto", "Project"));
    report.nav(NavSection {
        id: "overview".into(),
        title: t("Visão geral", "Overview").into(),
        count: specs.len(),
        groups: Vec::new(),
    });
    let done = specs
        .iter()
        .filter_map(|spec| spec["waves"][0].as_u64())
        .sum::<u64>();
    let total = specs
        .iter()
        .filter_map(|spec| spec["waves"][1].as_u64())
        .sum::<u64>();
    let mut body = format!(
        "<section id=\"overview\" class=\"block\"><h2>{}</h2><div class=\"status-grid\">",
        t("Visão geral", "Overview")
    );
    card(&mut body, "Specs", &specs.len().to_string());
    card(
        &mut body,
        t("Ondas concluídas", "Completed waves"),
        &format!("{done}/{total}"),
    );
    body.push_str("</div><div class=\"wave-grid\">");
    for spec in &specs {
        let raw = spec["phase"].as_str().unwrap_or_default();
        let lang = if english {
            mustard_core::platform::i18n::Locale::EnUs
        } else {
            mustard_core::platform::i18n::Locale::PtBr
        };
        let label = mustard_core::platform::i18n::translate(&format!("page.phase.{raw}"), lang);
        let _ = write!(
            body,
            "<article class=\"wave-card\"><span class=\"wave-number\">{}</span><strong>{}</strong></article>",
            escape(spec["name"].as_str().unwrap_or_default()),
            escape(if label == "<missing-key>" {
                t("Desconhecido", "Unknown")
            } else {
                label
            })
        );
    }
    if specs.is_empty() {
        let _ = write!(
            body,
            "<p class=\"muted\">{}</p>",
            t(
                "Nenhuma spec registrada. O projeto pode ser compartilhado sem uma spec aberta.",
                "No registered specs. The project can be shared without an open spec."
            )
        );
    }
    body.push_str("</div></section>");
    if let Some(usage) = snapshot.get("consumption") {
        let _ = write!(
            body,
            "<section class=\"block\"><h2>{}</h2><div class=\"status-grid\">",
            t("Jev do projeto", "Project Jev usage")
        );
        card(
            &mut body,
            t("Chamadas físicas", "Physical requests"),
            &usage["physical_requests"]
                .as_u64()
                .map_or_else(|| t("Desconhecido", "Unknown").into(), |n| n.to_string()),
        );
        card(
            &mut body,
            t("Custo registrado · US$", "Recorded cost · USD"),
            &usage["cost_micro_usd"].as_u64().map_or_else(
                || t("Desconhecido", "Unknown").into(),
                |n| format!("{:.4}", n as f64 / 1_000_000.0),
            ),
        );
        body.push_str("</div></section>");
    }
    let _ = write!(
        body,
        "<footer>{}</footer>",
        t(
            "Retrato gerado sob pedido explícito. Para atualizar, solicite nova publicação.",
            "Snapshot generated on explicit request. Request a new publication to update it."
        )
    );
    report.raw(&body).render()
}

/// The model/user writes only the requested analysis. Layout conversion and
/// publication are native and identical to the standalone `run page` layout.
pub(crate) fn public_report_snapshot(snapshot: &Value) -> String {
    let doc = mustard_core::view::document::Document {
        lang: snapshot["language"].as_str().unwrap_or("pt-BR").to_string(),
        kind: Some(
            if snapshot["language"] == "en-US" {
                "Report"
            } else {
                "Relatório"
            }
            .into(),
        ),
        title: snapshot["title"].as_str().unwrap_or("Mustard").to_string(),
        meta: vec![mustard_core::view::document::Meta::Note(
            snapshot["at"].as_str().unwrap_or_default().to_string(),
        )],
        body: super::markdown::page(snapshot["body"].as_str().unwrap_or_default()),
        footer: None,
    };
    super::Render::Html.render(&doc)
}
