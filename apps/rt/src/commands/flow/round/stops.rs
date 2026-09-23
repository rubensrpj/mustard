//! O que para sem travar a rodada: o limite de consertos de cada onda, com a
//! pergunta ao usuário, e o plano que muda — a onda replanejada depois do
//! pedido e a mudança de plano que um agente propõe, que só segue com o
//! clique do usuário.

use std::collections::{BTreeMap, BTreeSet};

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

/// Quantas rodadas de conserto uma onda tem. A reprovação que vem depois da
/// última delas para a onda e as que dependem dela: o problema é de desenho,
/// e vai ao usuário.
pub(super) const MAX_FIX_ROUNDS: usize = 2;

/// As ondas paradas pelo limite de consertos, na resposta da rodada: cada uma
/// com a pergunta ao usuário e os vereditos que a pararam, por inteiro — é com
/// eles que o usuário decide —, e o texto que manda fazer cada pergunta.
pub(super) fn stopped_waves(
    stuck: &BTreeMap<u64, Vec<&SpecEvent>>,
    codes: &BTreeMap<u64, String>,
    lang: Locale,
) -> (Vec<Value>, Vec<String>) {
    let max = MAX_FIX_ROUNDS.to_string();
    stuck
        .iter()
        .map(|(wave, verdicts)| {
            let listed: Vec<(String, &str)> = verdicts
                .iter()
                .map(|v| (codes.get(&v.id).cloned().unwrap_or_else(|| v.id.to_string()), v.str_field("text").unwrap_or_default()))
                .collect();
            let names: Vec<&str> = listed.iter().map(|(code, _)| code.as_str()).collect();
            let asked = translate("round.fix_limit", lang)
                .replace("{wave}", &wave.to_string())
                .replace("{count}", &listed.len().to_string())
                .replace("{max}", &max)
                .replace("{verdicts}", &names.join(", "));
            let question = translate("round.fix_limit.question", lang).replace("{wave}", &wave.to_string()).replace("{max}", &max);
            let verdicts: Vec<Value> = listed.iter().map(|(code, text)| json!({ "code": code, "text": text })).collect();
            (json!({ "wave": wave, "question": question, "verdicts": verdicts }), asked)
        })
        .unzip()
}

/// O código da mudança proposta, que vai na pergunta que a decide: a onda e
/// uma chave do texto da mudança, para que um "sim" nunca sirva para outra.
pub(super) fn replan_code(wave: u64, change: &str) -> String {
    let key = crate::commands::agent::render::prompt_ref::fnv1a64(&[change.trim()]) & 0x00ff_ffff;
    format!("onda-{wave}-{key:06x}")
}

/// A pergunta que decide a mudança que a onda `wave` propõe, no idioma
/// `lang`: em palavras, com o que a onda propõe e o que acontece ao aceitar e
/// ao recusar. O código da mudança nunca entra no enunciado — quem pergunta
/// escreve a frase com as palavras que o usuário entender, e o código viaja
/// no cabeçalho da pergunta.
pub(super) fn change_question(wave: u64, change: &str, lang: Locale) -> String {
    translate("change.question", lang).replace("{wave}", &wave.to_string()).replace("{change}", change.trim())
}

/// O código de mudança que `text` traz, quando traz um: a palavra com a forma
/// que [`replan_code`] escreve. É assim que a testemunha lê o código no
/// cabeçalho da pergunta, sem depender de nada do enunciado.
pub(crate) fn change_code_of(text: &str) -> Option<String> {
    text.split_whitespace().find(|word| is_change_code(word)).map(str::to_string)
}

/// `word` tem a forma de um código de mudança: `onda-<número>-<seis dígitos
/// hexadecimais minúsculos>`, como [`replan_code`] o escreve.
fn is_change_code(word: &str) -> bool {
    let Some((wave, key)) = word.strip_prefix("onda-").and_then(|rest| rest.split_once('-')) else {
        return false;
    };
    !wave.is_empty()
        && wave.bytes().all(|b| b.is_ascii_digit())
        && key.len() == 6
        && key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A mudança de código `code`, proposta pela onda `wave`, foi aceita: o
/// clique mais novo do usuário nela, gravado pela testemunha depois do último
/// pedido da onda, é o "Aceitar". Um clique em "Recusar" depois dele desfaz o
/// "sim"; um clique de antes do pedido não vale para ele.
///
/// Quem diz qual mudança o clique decide é o código que a testemunha guardou
/// ao lado da resposta, nunca a frase mostrada ao usuário: a pergunta escrita
/// com as palavras dele vale igual, e o "sim" de uma mudança nunca serve para
/// outra.
///
/// Só conta a mensagem de autor `user` com a testemunha. O `run write` recusa
/// toda mensagem com a testemunha, de qualquer autor, e recusa rever ou tirar
/// uma delas: só a testemunha grava o clique.
pub(super) fn change_accepted(log: &SpecLog, wave: u64, code: &str) -> bool {
    let langs = [Locale::PtBr, Locale::EnUs];
    let sent = log.last_by_wave("send").get(&wave).copied().unwrap_or(0);
    let last_click = log
        .block(BlockQuery::Block(Block::Conversation))
        .into_iter()
        .filter(|e| e.event_type == "message" && e.id > sent && e.str_field("author") == Some("user"))
        .filter_map(|e| e.fields.get("witness"))
        .filter(|w| w.get("change").and_then(Value::as_str).is_some_and(|clicked| clicked.trim() == code))
        .filter_map(|w| w.get("answer").and_then(Value::as_str))
        .next_back();
    last_click.is_some_and(|answer| langs.iter().any(|lang| translate("change.accept", *lang) == answer.trim()))
}

/// As ondas paradas pelo limite de consertos, cada uma com as reprovações
/// seguidas que a pararam: depois da primeira reprovação vêm no máximo
/// [`MAX_FIX_ROUNDS`] rodadas de conserto, e a reprovação seguinte para. A
/// conta começa na versão mais nova do plano da onda que o usuário pediu
/// depois da última reprovação: a onda que ele replanejou volta à fila com a
/// conta zerada, e o que o orquestrador acrescenta ao plano não zera nada.
pub(super) fn waves_stuck(log: &SpecLog) -> BTreeMap<u64, Vec<&SpecEvent>> {
    let verdicts = log.verdicts_by_wave();
    let reset = last_reset_by_user(log, &verdicts);
    let mut out = BTreeMap::new();
    for (n, verdicts) in verdicts {
        let since = reset.get(&n).copied().unwrap_or(0);
        let mut rejected: Vec<&SpecEvent> = verdicts
            .iter()
            .rev()
            .take_while(|v| v.id > since && v.str_field("result") == Some("rejected"))
            .copied()
            .collect();
        if rejected.len() > MAX_FIX_ROUNDS {
            rejected.reverse();
            out.insert(n, rejected);
        }
    }
    out
}

/// Cada versão do plano, com as ondas que ela mexe: a versão da onda ou de
/// uma tarefa dela. A tarefa que muda de onda conta para as duas — a de onde
/// saiu, pela versão que ela substitui, e a para onde foi.
fn plan_versions(log: &SpecLog) -> Vec<(u64, &SpecEvent)> {
    let mut out = Vec::new();
    for event in log.block(BlockQuery::Block(Block::Waves)) {
        if !matches!(event.event_type.as_str(), "wave" | "task") {
            continue;
        }
        let before = event.int("replaces").and_then(|old| log.get(old)).and_then(SpecEvent::wave);
        let mut waves: Vec<u64> = event.wave().into_iter().chain(before).collect();
        waves.dedup();
        out.extend(waves.into_iter().map(|n| (n, event)));
    }
    out
}

/// O número do evento mais novo do plano de cada onda.
fn last_planned(log: &SpecLog) -> BTreeMap<u64, u64> {
    let mut planned: BTreeMap<u64, u64> = BTreeMap::new();
    for (n, event) in plan_versions(log) {
        let newest = planned.entry(n).or_insert(0);
        *newest = (*newest).max(event.id);
    }
    planned
}

/// O número da versão mais nova do plano de cada onda que zera a conta de
/// consertos: a que nasce (`origin`) de uma mensagem ou de uma decisão do
/// usuário gravada depois da última reprovação da onda anterior à versão.
fn last_reset_by_user(log: &SpecLog, verdicts: &BTreeMap<u64, Vec<&SpecEvent>>) -> BTreeMap<u64, u64> {
    let from_user = |origin: u64| {
        log.get(origin).filter(|o| matches!(o.event_type.as_str(), "message" | "decision") && o.str_field("author") == Some("user"))
    };
    let mut reset: BTreeMap<u64, u64> = BTreeMap::new();
    for (n, event) in plan_versions(log) {
        let Some(asked) = event.int("origin").and_then(from_user) else { continue };
        let last_rejection = verdicts
            .get(&n)
            .into_iter()
            .flatten()
            .filter(|v| v.id < event.id && v.str_field("result") == Some("rejected"))
            .map(|v| v.id)
            .max()
            .unwrap_or(0);
        if asked.id > last_rejection {
            let newest = reset.entry(n).or_insert(0);
            *newest = (*newest).max(event.id);
        }
    }
    reset
}

/// As ondas replanejadas depois do último pedido: a onda, ou uma tarefa dela,
/// ganhou versão nova depois do envio.
pub(super) fn waves_replanned(log: &SpecLog) -> BTreeSet<u64> {
    let last_send = log.last_by_wave("send");
    last_planned(log)
        .into_iter()
        .filter(|(n, planned)| last_send.get(n).is_some_and(|sent| sent < planned))
        .map(|(n, _)| n)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use crate::commands::spec_events::write::WriteOpts;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A onda que ganha versão nova depois do pedido volta para a fila, uma
    /// vez só: o pedido gravado descrevia o plano antigo. A tarefa que muda de
    /// onda replaneja as duas. A onda que já entregou não volta por
    /// replanejamento — só a reprovação a devolve.
    #[test]
    fn a_wave_replanned_after_its_send_goes_out_again_and_only_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A onda 2 é um lote do programa com duas tarefas, em arquivos
        // diferentes: a que muda de onda não a esvazia, porque um lote vazio
        // nunca sai, e não a prende à onda 1 por um arquivo dividido.
        approved_with(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])], |said| {
            write(root, "x", "task", json!({"wave": 2, "text": "Outra tarefa da onda 2.",
                "files": [{"path": "src/c.rs"}], "depends_on": [], "origin": said}));
        });
        let first = round(root, "x", None);
        assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(2), "{first}");

        let path = store::spec_file(root, "x").unwrap();
        let current = |kind: &str, n: u64| -> Value {
            let log = store::read(&path).unwrap().unwrap();
            let event = log
                .visible()
                .into_iter()
                .find(|e| e.event_type == kind && e.wave() == Some(n))
                .unwrap_or_else(|| panic!("sem {kind} da onda {n}"));
            let mut fields = event.fields.clone();
            for key in ["v", "id", "code", "at", "search", "type", "author"] {
                fields.remove(key);
            }
            let id = event.id;
            let mut body = Value::Object(fields);
            body["replaces"] = json!(id);
            body
        };

        // A onda 1 ganha outra versão; a tarefa da onda 2 muda para a 1.
        let mut wave = current("wave", 1);
        wave["done_when"] = json!("A suíte passa e o teste novo também.");
        id_of(&write(root, "x", "wave", wave));
        let mut task = current("task", 2);
        task["wave"] = json!(1);
        id_of(&write(root, "x", "task", task));

        let again = round(root, "x", None);
        let waves: Vec<u64> =
            again["dispatch"].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect();
        assert_eq!(waves, vec![1, 2], "as duas foram replanejadas depois do pedido: {again}");

        let quiet = round(root, "x", None);
        assert_eq!(quiet["dispatch"], json!([]), "o pedido novo já descreve o plano atual: {quiet}");

        // Entregue, a onda não volta por uma versão nova.
        round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        let mut wave = current("wave", 1);
        wave["text"] = json!("Onda 1, texto revisto.");
        id_of(&write(root, "x", "wave", wave));
        let after = round(root, "x", None);
        assert_eq!(after["dispatch"], json!([]), "a onda 1 já entregou: {after}");
    }

    /// O agente que diz que o plano da onda não funciona para a rodada até o
    /// "sim" do usuário, e o "sim" é o clique em "Aceitar" na pergunta da
    /// mudança, gravado pela testemunha. A recusa mostra a mudança e a
    /// pergunta; uma mensagem escrita à mão pelo modelo, com a mesma pergunta
    /// e a mesma resposta, não destrava nada; o clique em "Recusar" também
    /// não; o clique em "Aceitar" destrava, e a rodada grava o que a onda
    /// entregou.
    #[test]
    fn a_wave_that_says_its_plan_does_not_work_stops_the_round_until_the_users_click() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let session = "s-replan";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");

        let change = "A onda 1 precisa da 2 antes.";
        let code = replan_code(1, change);
        let report = line("DELIVERED", json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "replan": change}));
        let stopped = round(root, "x", Some(&report));
        assert_eq!(stopped["reason"], json!("wave-plan-does-not-work"), "{stopped}");
        let question = stopped["question"].as_str().unwrap_or_default().to_string();
        assert_eq!(question, change_question(1, change, Locale::PtBr), "{stopped}");
        assert_eq!(stopped["header"], json!(code), "{stopped}");
        assert_eq!(stopped["options"], json!(["Aceitar", "Recusar"]), "{stopped}");
        let hint = stopped["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(change) && hint.contains(&question) && hint.contains(&code), "{hint}");
        assert_eq!(delivered_count(root), 0);

        // O modelo não escreve o "sim": o `run write` recusa a mensagem com a
        // testemunha, seja do usuário, seja do próprio modelo.
        let by_hand = |body: Value| {
            crate::commands::spec_events::write::write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("x".to_string()),
                event_type: "message".into(),
                json: body.to_string(),
            })
        };
        let witness = json!({ "question": question, "answer": "Aceitar", "change": code });
        let forged = by_hand(json!({ "author": "user", "text": format!("{question}\nAceitar"), "witness": witness }));
        assert_eq!(forged["reason"], json!("user-message-by-hook"), "{forged}");
        let own = by_hand(json!({ "text": format!("{question}\nAceitar"), "witness": witness }));
        assert_eq!(own["reason"], json!("user-message-by-hook"), "{own}");
        let still = round(root, "x", Some(&report));
        assert_eq!(still["reason"], json!("wave-plan-does-not-work"), "a forged yes accepts nothing: {still}");

        click(root, session, &question, &code, "Recusar");
        let refused = round(root, "x", Some(&report));
        assert_eq!(refused["reason"], json!("wave-plan-does-not-work"), "a declined change stays stopped: {refused}");

        // O "sim" de uma mudança nunca serve para outra.
        click(root, session, &question, &replan_code(1, "Outra mudança."), "Aceitar");
        let other = round(root, "x", Some(&report));
        assert_eq!(other["reason"], json!("wave-plan-does-not-work"), "{other}");

        click(root, session, &question, &code, "Aceitar");
        let went = round(root, "x", Some(&report));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "the round records what the wave delivered");
    }

    /// A pergunta que a rodada manda fazer vai em palavras: diz o que a onda
    /// propõe e o que acontece em cada escolha, e não leva o código interno
    /// no enunciado. O código vai no cabeçalho da pergunta, e é ele, guardado
    /// ao lado da resposta, que reconhece o "sim" — a pergunta escrita com as
    /// palavras do usuário vale igual, o código de outra mudança não vale, e
    /// a pergunta sem código nenhum não destrava nada.
    #[test]
    fn a_pergunta_vai_em_palavras_e_o_sim_e_reconhecido_pelo_codigo() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let session = "s-pergunta-em-palavras";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");

        let change = "A onda 1 precisa da onda 2 antes dela.";
        let code = replan_code(1, change);
        let report = line("DELIVERED", json!({"wave": 1, "text": "Parei: o plano não fecha.", "replan": change}));
        let stopped = round(root, "x", Some(&report));
        assert_eq!(stopped["reason"], json!("wave-plan-does-not-work"), "{stopped}");

        // A pergunta pronta é a do usuário: o que muda e o que acontece em
        // cada escolha, sem o código dentro dela.
        let asked = stopped["question"].as_str().unwrap_or_default().to_string();
        assert!(asked.contains(change), "a pergunta diz o que a onda propõe: {asked}");
        assert!(asked.contains('1'), "a pergunta diz de que onda se trata: {asked}");
        assert!(asked.contains("Aceitando") && asked.contains("recusando"), "as duas saídas: {asked}");
        assert!(!asked.contains(&code), "o código nunca vai no enunciado: {asked}");
        assert_eq!(stopped["header"], json!(code), "o código vai no cabeçalho: {stopped}");
        assert_eq!(stopped["options"], json!(["Aceitar", "Recusar"]), "{stopped}");

        // A pergunta que quem despacha reescreve com as palavras do usuário,
        // sem o código em lugar nenhum do texto.
        let mine = "A onda 1 travou e quer a onda 2 antes dela. Posso seguir assim?";

        // O código de outra mudança no cabeçalho não aceita esta.
        click(root, session, mine, &replan_code(1, "Outra mudança."), "Aceitar");
        let other = round(root, "x", Some(&report));
        assert_eq!(other["reason"], json!("wave-plan-does-not-work"), "o sim de outra mudança não vale: {other}");

        // Sem código nenhum no cabeçalho, nada diz qual mudança o clique
        // decide, e a rodada segue parada.
        click(root, session, mine, "Mudança", "Aceitar");
        let blind = round(root, "x", Some(&report));
        assert_eq!(blind["reason"], json!("wave-plan-does-not-work"), "sem código não destrava: {blind}");

        // Com o código no cabeçalho, o "sim" vale, seja qual for a frase.
        click(root, session, mine, &code, "Aceitar");
        let went = round(root, "x", Some(&report));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "a rodada gravou o que a onda entregou: {went}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let clicked = log
            .visible()
            .into_iter()
            .filter_map(|e| e.fields.get("witness").cloned())
            .next_back()
            .expect("o clique gravado");
        assert_eq!(clicked["change"], json!(code), "o código fica ao lado da resposta: {clicked}");
        assert_eq!(clicked["question"], json!(mine), "a frase gravada é a que o usuário leu: {clicked}");
    }

    /// Cada onda tem no máximo duas rodadas de conserto. Depois da terceira
    /// reprovação seguida a rodada não a manda de novo, e a resposta traz a
    /// pergunta ao usuário com os três vereditos e as duas saídas. A parada
    /// segue até o usuário replanejar a onda; a versão nova do plano que não
    /// nasce dele não a destrava, e a que nasce de uma decisão dele a devolve
    /// à fila.
    #[test]
    fn a_wave_rejected_after_its_second_fix_round_is_not_sent_again_and_the_user_is_asked() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        // Cada tentativa volta, e a revisão dela reprova.
        let rejected = |n: usize| {
            let out = round(root, "x", Some(&delivered(root, 1, &format!("Tentativa {n}."), &["src/a.rs"])));
            assert_eq!(out["ok"], json!(true), "{out}");
            round(root, "x", Some(&verdict(1, "rejected", &format!("reprovação {n}"))))
        };
        for n in 1..=2 {
            let fix = rejected(n);
            assert_eq!(waves_in(&fix, "dispatch"), vec![1], "rodada de conserto {n}: {fix}");
        }
        let sends = |root: &Path| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            log.visible().iter().filter(|e| e.event_type == "send").count()
        };
        assert_eq!(sends(root), 3);
        // O conserto que saiu está em andamento e ocupa a única vaga.
        let busy = round(root, "x", None);
        assert_eq!(waves_in(&busy, "dispatch"), Vec::<u64>::new(), "{busy}");
        assert_eq!(waves_in(&busy, "running"), vec![1], "{busy}");

        let stopped = rejected(3);
        assert_eq!(stopped["ok"], json!(true), "a parada não recusa a rodada: {stopped}");
        assert_eq!(waves_in(&stopped, "dispatch"), Vec::<u64>::new(), "{stopped}");
        assert_eq!(sends(root), 3, "nada saiu depois da terceira reprovação");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let verdicts: Vec<Value> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "verdict")
            .map(|e| json!({"code": codes[&e.id], "text": e.str_field("text").unwrap()}))
            .collect();
        assert_eq!(verdicts.len(), 3, "a terceira reprovação foi gravada");
        let question = translate("round.fix_limit.question", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{max}", &MAX_FIX_ROUNDS.to_string());
        assert_eq!(stopped["stopped"], json!([{"wave": 1, "question": question, "verdicts": verdicts}]), "{stopped}");
        // Sem mais nada a fazer, o próximo passo é a pergunta, com os vereditos.
        let codes: Vec<&str> = verdicts.iter().filter_map(|v| v["code"].as_str()).collect();
        let asked = translate("round.fix_limit", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{count}", "3")
            .replace("{max}", &MAX_FIX_ROUNDS.to_string())
            .replace("{verdicts}", &codes.join(", "));
        assert!(stopped["next"].as_str().unwrap_or_default().ends_with(&asked), "{stopped}");
        assert!(stopped.get("command").is_none(), "{stopped}");

        // Parada continua parada.
        let still = round(root, "x", None);
        assert_eq!(waves_in(&still, "stopped"), vec![1], "{still}");
        assert_eq!(sends(root), 3);

        // A versão nova da tarefa que o orquestrador grava, com a origem de
        // antes das reprovações, não destrava a onda.
        replan(root, 1);
        let held = round(root, "x", None);
        assert_eq!(waves_in(&held, "stopped"), vec![1], "{held}");
        assert_eq!(waves_in(&held, "dispatch"), Vec::<u64>::new(), "{held}");

        // O plano que o usuário revê devolve a onda à fila.
        let decided = user_decides(root, "Refazer a tarefa da onda 1.");
        replan_from(root, 1, Some(decided));
        let back = round(root, "x", None);
        assert_eq!(back["ok"], json!(true), "{back}");
        assert_eq!(waves_in(&back, "dispatch"), vec![1], "{back}");
        assert!(back.get("stopped").is_none(), "{back}");
    }

    /// A decisão do usuário de rever o plano, nascida de uma fala dele gravada
    /// agora pelo gancho da entrada. Devolve o número da decisão.
    fn user_decides(root: &Path, text: &str) -> u64 {
        let said = id_of(&write(root, "x", "message", json!({"author": "user", "text": text})));
        id_of(&write(root, "x", "decision", json!({"author": "user", "text": text,
            "why": "o usuário reviu o plano da onda", "keys": ["plano"], "waves": [1], "origin": said})))
    }

    /// A conta de consertos só zera com o usuário. A tarefa que o orquestrador
    /// acrescenta à onda depois de uma reprovação, nascida de uma decisão dele,
    /// sai de novo para a onda, mas não zera a conta: a terceira reprovação
    /// para a onda. A fala do usuário gravada antes da última reprovação também
    /// não zera; a decisão dele gravada depois, sim.
    #[test]
    fn only_a_users_decision_resets_the_fix_count_of_a_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        // O veredito final reprovado, com o combinado vigente atendido: sem
        // a lista `agreed` cobrindo o item que a decisão do orquestrador cria
        // mais adiante, a terceira reprovação seria recusada por faltar item,
        // antes de a conta de consertos entrar em jogo.
        let rejected = |n: usize, agreed: &[&str]| {
            let out = round(root, "x", Some(&delivered(root, 1, &format!("Tentativa {n}."), &["src/a.rs"])));
            assert_eq!(out["ok"], json!(true), "{out}");
            let agreed: Vec<Value> = agreed.iter().map(|item| json!({"item": item, "met": true})).collect();
            let rejected = line("VERDICT", json!({"wave": 1, "result": "rejected", "final": true,
                "text": format!("reprovação {n}"), "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}],
                "agreed": agreed}));
            round(root, "x", Some(&rejected))
        };
        assert_eq!(waves_in(&rejected(1, &[]), "dispatch"), vec![1]);
        // Uma fala do usuário antes da segunda reprovação: a tarefa que nasce
        // dela depois dessa reprovação não zera a conta.
        let early = id_of(&write(root, "x", "message", json!({"author": "user", "text": "Veja o teste."})));
        assert_eq!(waves_in(&rejected(2, &[]), "dispatch"), vec![1]);

        // O orquestrador acrescenta uma tarefa à onda, por decisão dele.
        let own = id_of(&write(root, "x", "decision", json!({"text": "Falta uma tarefa na onda 1.",
            "why": "a revisão apontou", "keys": ["tarefa"], "waves": [1], "origin": early})));
        for origin in [own, early] {
            write(root, "x", "task", json!({"wave": 1, "text": format!("Tarefa acrescentada ({origin})."),
                "files": [{"path": "src/a.rs"}], "depends_on": [], "origin": origin}));
        }
        let again = round(root, "x", None);
        assert_eq!(waves_in(&again, "dispatch"), vec![1], "o plano mudou depois do pedido: {again}");

        let stopped = rejected(3, &["MSTD-DEC-0001"]);
        assert_eq!(waves_in(&stopped, "stopped"), vec![1], "a tarefa do orquestrador não zera a conta: {stopped}");
        assert_eq!(waves_in(&stopped, "dispatch"), Vec::<u64>::new(), "{stopped}");
        assert_eq!(stopped["stopped"][0]["verdicts"].as_array().map(Vec::len), Some(3), "{stopped}");

        let decided = user_decides(root, "Refazer a onda 1 com a tarefa nova.");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(1)).unwrap();
        let mut body = wave.fields.clone();
        for key in ["v", "id", "code", "at", "search", "type", "author"] {
            body.remove(key);
        }
        body.insert("replaces".into(), json!(wave.id));
        body.insert("origin".into(), json!(decided));
        write(root, "x", "wave", Value::Object(body));
        let back = round(root, "x", None);
        assert!(back.get("stopped").is_none(), "a decisão do usuário zera a conta: {back}");
        assert_eq!(waves_in(&back, "dispatch"), vec![1], "{back}");
    }

    /// A história da onda `n` parada pelo limite de consertos: cada tentativa
    /// é um pedido, a entrega e a reprovação dela.
    fn stuck(root: &Path, n: u64, files: &[&str]) {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        for attempt in 0..=MAX_FIX_ROUNDS {
            seed_send(root, n);
            write(root, "x", "delivered", json!({"wave": n, "text": format!("Tentativa {attempt}."), "files": files}));
            crate::shared::spec_state::seed_verdict(root, "x", n, "rejected", crit);
        }
    }

    /// A onda parada pelo limite de consertos segura só ela e as que dependem
    /// dela, direta ou por outra onda: a onda independente sai, e a resposta
    /// traz a pergunta com os vereditos da onda parada antes do resto. Tirada
    /// do plano, a onda parada deixa de contar: não segura mais nada; e a
    /// onda em andamento tirada do plano não ocupa vaga.
    #[test]
    fn a_stuck_wave_holds_only_itself_and_its_dependents_and_stops_counting_out_of_the_plan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(
            root,
            "x",
            &[
                (1, &["src/a.rs"], &[]),
                (2, &["src/b.rs"], &[1]),
                (3, &["src/c.rs"], &[2]),
                (4, &["src/d.rs"], &[]),
                (5, &["src/e.rs"], &[]),
                (6, &["src/f.rs"], &[1]),
            ],
        );
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":3}"#).unwrap();
        stuck(root, 1, &["src/a.rs"]);
        // A 2 entregou antes de a rodada existir: a 3 só espera por ela através da 1.
        write(root, "x", "delivered", json!({"wave": 2, "text": "Saiu antes da rodada.", "files": ["src/b.rs"]}));
        seed_send(root, 4);
        write(root, "x", "delivered", json!({"wave": 4, "text": "Saiu.", "files": ["src/d.rs"]}));

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![5], "a 6 depende da 1, e a 3 depende dela pela 2: {out}");
        assert!(out.get("reviews").is_none(), "{out}");
        assert_eq!(waves_in(&out, "stopped"), vec![1], "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let judged: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict").collect();
        let verdicts: Vec<Value> =
            judged.iter().map(|e| json!({"code": codes[&e.id], "text": e.str_field("text").unwrap()})).collect();
        assert_eq!(out["stopped"][0]["verdicts"], json!(verdicts), "{out}");
        let names: Vec<&str> = judged.iter().map(|e| codes[&e.id].as_str()).collect();
        let asked = translate("round.fix_limit", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{count}", "3")
            .replace("{max}", &MAX_FIX_ROUNDS.to_string())
            .replace("{verdicts}", &names.join(", "));
        let rest = format!("{} {}", translate("round.next", Locale::PtBr), translate("round.report", Locale::PtBr));
        assert!(out["next"].as_str().unwrap_or_default().ends_with(&format!("{asked} {rest}")), "{out}");

        // O usuário tira do plano a onda 1 e a 5, que estava em andamento.
        let targets: Vec<u64> = log
            .visible()
            .into_iter()
            .filter(|e| matches!(e.event_type.as_str(), "wave" | "task") && matches!(e.wave(), Some(1 | 5)))
            .map(|e| e.id)
            .collect();
        write(root, "x", "remove", json!({"targets": targets, "reason": "o usuário tirou as ondas do plano"}));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("stopped").is_none(), "{out}");
        let mut sent = waves_in(&out, "dispatch");
        sent.sort_unstable();
        assert_eq!(sent, vec![3, 6], "{out}");
        assert_eq!(waves_in(&out, "running"), vec![3, 6], "a onda fora do plano não ocupa vaga: {out}");
    }
}
