//! A falha do Jev na rodada. A montagem das ondas, a escolha dos itens do
//! pedido, a do procedimento de cada onda, a triagem das voltas e o
//! classificador das atividades seguem sem o Jev quando ele falha, e cada uma grava a chamada com o motivo
//! (`jev:<motivo>`). A rodada lê as chamadas que gravou e diz numa linha, uma
//! vez por sessão, que o Jev falhou e por quê: sem isso a falha ficava só no
//! registro, e a rodada seguia calada.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

use crate::shared::jev::{ASSEMBLY_CALL, EXPERTISE_CALL, ITEMS_CALL, TASK_TRIAGE_CALL, TRIAGE_CALL};

/// Os comandos que a rodada grava quando chama o Jev: a montagem das ondas,
/// a escolha dos itens, a do procedimento, a triagem das voltas e o
/// classificador das atividades.
const ROUND_CALLS: [&str; 5] = [ASSEMBLY_CALL, ITEMS_CALL, EXPERTISE_CALL, TRIAGE_CALL, TASK_TRIAGE_CALL];

/// A marca da sessão que já ouviu o aviso.
const MARK: &str = "round.jev_failed";

/// O motivo da primeira falha do Jev entre as chamadas da rodada gravadas
/// depois do último evento de `on_entry`, a spec como a rodada a leu ao entrar.
fn failure_since(now: &SpecLog, on_entry: &SpecLog) -> Option<String> {
    let since = on_entry.events.iter().map(|event| event.id).max().unwrap_or(0);
    now.visible()
        .into_iter()
        .filter(|event| event.id > since && event.event_type == "call")
        .filter(|event| event.str_field("command").is_some_and(|command| ROUND_CALLS.contains(&command)))
        .find_map(|event| event.str_field("filter")?.strip_prefix("jev:").map(str::to_string))
}

/// O que fazer diante da falha de motivo `reason`: a chave recusada pede
/// outra chave; a falta de crédito, crédito, e não outra chave; a pergunta
/// grande demais nem saiu; o resto (tempo esgotado, serviço ocupado, rede,
/// erro ou resposta ilegível) passa sem mexer na chave nem no crédito.
fn remedy_key(reason: &str) -> &'static str {
    match reason {
        "key_refused" | "missing_key" | "key_in_git" => "round.jev_remedy_key",
        "no_credit" => "round.jev_remedy_credit",
        "too_large" => "round.jev_remedy_size",
        _ => "round.jev_remedy_wait",
    }
}

/// O aviso `jev-failed` da rodada da spec em `path`, com o motivo e o que
/// fazer, quando alguma chamada da rodada ao Jev falhou; uma vez só na sessão
/// `session`, e sempre sem sessão conhecida. Sem Jev na rodada (`asked`
/// falso), nenhuma chamada saiu, e a spec nem é relida.
pub(super) fn warning(root: &Path, path: &Path, on_entry: &SpecLog, asked: bool, session: Option<&str>, lang: Locale) -> Option<Value> {
    if !asked {
        return None;
    }
    let now = store::read(path).ok().flatten()?;
    let failure = failure_since(&now, on_entry)?;
    crate::shared::search_door::first_warning(root, session, MARK).then(|| {
        let hint = translate("round.jev_failed", lang)
            .replace("{reason}", &failure)
            .replace("{remedy}", translate(remedy_key(&failure), lang));
        json!({ "reason": "jev-failed", "failure": failure, "hint": hint })
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use mustard_core::domain::map_filter::FilterError;
    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{Locale, translate};
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use crate::shared::jev::{ITEMS_CALL, TASK_TRIAGE_CALL, record_jev_call};

    use super::super::item_choice::fake;
    use super::super::tests::{approved_with, id_of, write};
    use super::super::{RoundOpts, round_for};

    /// Uma rodada da obra `x` na sessão `session`.
    fn round_in(root: &Path, session: Option<&str>) -> Value {
        super::super::review::test_conductor_review(root, "x");
        round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: None }, session)
    }

    fn jev_warnings(out: &Value) -> Vec<Value> {
        out["warnings"].as_array().into_iter().flatten().filter(|w| w["reason"] == json!("jev-failed")).cloned().collect()
    }

    /// Uma tarefa solta no backlog, chamada `name`, no arquivo `file`, que a
    /// rodada seguinte monta numa onda nova.
    fn loose_task(root: &Path, name: &str, file: &str) {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let first = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
        let (said, crit) = (first("message"), first("criterion"));
        std::fs::write(root.join(file), "export function one() {}\n").unwrap();
        id_of(&write(root, "x", "task", json!({"text": format!("Mexer no código {name}."), "files": [{"path": file}], "depends_on": [], "covers": [crit], "origin": said})));
    }

    /// O Jev sem crédito recusa a escolha dos itens com 402 em cada rodada
    /// que solta uma onda: a primeira rodada da sessão avisa uma vez, com o
    /// motivo `no_credit` e o remédio de crédito, nunca o de trocar a chave;
    /// a seguinte, na mesma sessão, cala, mas grava a chamada; outra sessão
    /// avisa de novo. A sessão nova cuja rodada não chamou o Jev cala: a
    /// falha velha, de outra rodada, não é dela.
    #[test]
    fn a_jev_failure_reaches_the_round_once_per_session_with_its_reason() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[], |said| {
            id_of(&write(
                root,
                "x",
                "rule",
                json!({"title": "A tabela nova tem chave.", "text": "A tabela nova tem chave.", "example": "e", "keys": ["k"],
                "applies_to": {"files": ["**"]}, "origin": said}),
            ));
        });
        let _jev = fake::refusing_with(402);

        loose_task(root, "um", "src/a.ts");
        let first = round_in(root, Some("sessao-1"));
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");
        let warned = jev_warnings(&first);
        assert_eq!(warned.len(), 1, "{first}");
        assert_eq!(warned[0]["failure"], json!("no_credit"), "{first}");
        let hint = warned[0]["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("no_credit"), "{first}");
        assert!(hint.contains(translate("round.jev_remedy_credit", Locale::PtBr)), "{first}");
        assert!(!hint.contains(translate("round.jev_remedy_key", Locale::PtBr)), "no credit is not a key to change: {first}");

        loose_task(root, "dois", "src/b.ts");
        let again = round_in(root, Some("sessao-1"));
        assert_eq!(again["dispatch"].as_array().map(Vec::len), Some(1), "{again}");
        assert!(jev_warnings(&again).is_empty(), "one warning per session: {again}");

        loose_task(root, "três", "src/c.ts");
        let other = round_in(root, Some("sessao-2"));
        assert_eq!(jev_warnings(&other).len(), 1, "another session warns: {other}");

        let quiet = round_in(root, Some("sessao-3"));
        assert_eq!(quiet["dispatch"].as_array().map_or(0, Vec::len), 0, "{quiet}");
        assert!(jev_warnings(&quiet).is_empty(), "an old failure of another round stays silent: {quiet}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let failed = log.visible().into_iter().filter(|e| e.event_type == "call" && e.str_field("filter") == Some("jev:no_credit")).count();
        assert_eq!(failed, 3, "every failed call stays recorded");
    }

    /// A rodada sem Jev não relê a spec para o aviso: nenhuma chamada dela
    /// saiu, e a falha gravada depois da entrada não é dela.
    #[test]
    fn a_round_without_jev_skips_the_failure_reading() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[], |_| {});
        let path = store::spec_file(root, "x").unwrap();
        let on_entry = store::read(&path).unwrap().unwrap();
        let refused = FilterError::Refused { status: 402 };
        record_jev_call(root, "x", ITEMS_CALL, Instant::now(), None, Err(&refused));

        assert!(super::warning(root, &path, &on_entry, true, None, Locale::PtBr).is_some(), "the failure is there to read");
        assert_eq!(super::warning(root, &path, &on_entry, false, None, Locale::PtBr), None);
    }

    /// A falha gravada do classificador das atividades vira o aviso
    /// `jev-failed` da rodada, com o motivo.
    #[test]
    fn task_triage_failure_warns_the_round() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[], |_| {});
        let path = store::spec_file(root, "x").unwrap();
        let on_entry = store::read(&path).unwrap().unwrap();
        record_jev_call(root, "x", TASK_TRIAGE_CALL, Instant::now(), None, Err(&FilterError::Refused { status: 402 }));

        let warned = super::warning(root, &path, &on_entry, true, None, Locale::PtBr).expect("the failure warns");
        assert_eq!(warned["reason"], json!("jev-failed"));
        assert_eq!(warned["failure"], json!("no_credit"));
    }
}
