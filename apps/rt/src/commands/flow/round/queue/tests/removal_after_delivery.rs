//! A onda que fica sem tarefa pela remoção que o modelo grava depois da
//! entrega do agente dela, com o pedido ainda aberto.

use super::*;

/// Duas ondas saem, uma num arquivo TypeScript e outra num C#. O agente da
/// onda 1 entrega, e a única tarefa dela é removida pelo `run write` antes de
/// a rodada assumir a volta, com o pedido ainda aberto: a remoção passa e diz
/// que a onda 1 saiu do ar. A rodada seguinte não a despacha de novo nem a
/// lista em andamento, e grava a nota que diz por quê; a outra rodada também
/// não a despacha e não grava outra nota.
#[test]
fn a_wave_emptied_by_a_write_after_its_delivery_leaves_the_air_and_is_not_sent_again() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["web/app.ts"], &[]), (2, &["src/Program.cs"], &[])]);
    let first = round(root, "x", None);
    assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
    let log = spec_now(root);
    let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(1)).unwrap().id;
    let code = log.codes().get(&task).cloned().unwrap();
    let send = log.codes().get(&log.last_by_wave("send")[&1]).cloned().unwrap();

    delivered(root, 1, "Saiu.", &["web/app.ts"]);
    let log = spec_now(root);
    assert!(open_sends(&log).contains_key(&1), "o pedido da onda 1 segue aberto depois da entrega");
    assert!(waves_returned(&log).contains(&1), "a volta da onda 1 espera a rodada");
    let removed = write(root, "x", "remove", json!({"targets": [code], "reason": "Já foi feito fora da onda."}));
    assert_eq!(removed["ok"], json!(true), "depois da entrega o run write remove: {removed}");
    assert_eq!(removed["waves_left_without_tasks"], json!([1]), "a remoção diz que a onda 1 saiu do ar: {removed}");

    let notes = || -> Vec<String> {
        spec_now(root)
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "note" && e.str_field("author") == Some("binary"))
            .filter_map(|e| e.str_field("text").map(str::to_string))
            .collect()
    };
    let said = translate("round.wave_left_without_tasks", Locale::PtBr).replace("{wave}", "1").replace("{send}", &send);

    let next = round(root, "x", None);
    assert_eq!(next["ok"], json!(true), "{next}");
    assert!(!waves_in(&next, "dispatch").contains(&1), "a onda 1 não sai de novo: {next}");
    assert_eq!(waves_in(&next, "running"), vec![2], "a onda 1 sai do ar: {next}");
    assert_eq!(notes(), vec![said.clone()], "a rodada grava a nota que diz por quê: {next}");

    let again = round(root, "x", None);
    assert_eq!(again["ok"], json!(true), "{again}");
    assert!(!waves_in(&again, "dispatch").contains(&1), "nem a rodada seguinte despacha a onda 1: {again}");
    assert_eq!(waves_in(&again, "running"), vec![2], "{again}");
    assert_eq!(notes(), vec![said], "a nota sai uma vez só: {again}");

    let log = spec_now(root);
    assert_eq!(waves_in_progress(&log).into_keys().collect::<Vec<_>>(), vec![2]);
    assert_eq!(open_sends(&log).into_keys().collect::<Vec<_>>(), vec![2]);
}
