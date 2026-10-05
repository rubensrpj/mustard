//! O que para sem travar a rodada: o limite de consertos de cada onda, com a
//! pergunta ao usuário; a volta recusada por uma conferência dela, que segura
//! só a própria onda; e o plano que muda — a onda replanejada depois do
//! pedido e a mudança de plano que um agente propõe. A mudança que não troca
//! decisão do usuário segue e fica registrada; a que troca só segue com o
//! clique do usuário e, enquanto espera, segura só a onda dela. As tarefas
//! que a onda não fez voltam ao backlog quando a rodada assume a volta, menos
//! a que outra onda já levou, que fica com ela.

use std::collections::{BTreeMap, BTreeSet};

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::answer::{without_final_period, RoundRefusal};
use super::report::{backlog_return, PlanChange, WaveReport};
use super::sent_tasks::sent_tasks;

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
pub(crate) fn replan_code(wave: u64, change: &str) -> String {
    let key = crate::commands::agent::render::prompt_ref::fnv1a64(&[change.trim()]) & 0x00ff_ffff;
    format!("onda-{wave}-{key:06x}")
}

/// A volta `event` diz que a mudança de plano dela troca uma decisão do
/// usuário: só essa mudança espera o clique.
pub(crate) fn swaps_decision(event: &SpecEvent) -> bool {
    event.str_field("changes_decision").is_some_and(|decision| !decision.trim().is_empty())
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
pub(crate) fn change_accepted(log: &SpecLog, wave: u64, code: &str) -> bool {
    let langs = [Locale::PtBr, Locale::EnUs];
    let sent = log.last_dispatch_by_wave().get(&wave).copied().unwrap_or(0);
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

/// A volta de uma onda que a rodada deixa fora do commit, com a recusa que a
/// segura: a que troca uma decisão do usuário e ainda espera o clique dele, ou a que
/// uma conferência da própria volta recusou — sem o título do commit, com
/// tarefa de outra onda em `undone`, com a mudança de plano sem `undone`, com
/// arquivo que não existe. A recusa segura só a onda dela: a volta fica na
/// spec sem ninguém a assumir, a cópia fica como está, e o envio aberto
/// segura a vaga e os arquivos dela — o despacho não oferece onda que divida
/// arquivo com ela nem a que dependa dela. O resto da rodada segue. A rodada
/// seguinte a assume como qualquer outra, depois do clique ou da volta
/// gravada de novo.
pub(crate) struct HeldReturn {
    pub wave: u64,
    pub refusal: RoundRefusal,
}

impl HeldReturn {
    /// O aviso da resposta da rodada: o motivo, a onda e a mensagem da recusa,
    /// no idioma `lang`, com o cabeçalho e as opções quando é a mudança de
    /// plano, como a recusa os mostra.
    pub(super) fn warning(&self, lang: Locale) -> Value {
        let mut warning = self.refusal.to_value(lang);
        if let Some(fields) = warning.as_object_mut() {
            fields.remove("ok");
            fields.insert("wave".into(), json!(self.wave));
        }
        warning
    }

    /// A frase do próximo passo, no idioma `lang`: a mudança de plano já diz
    /// a onda e o que perguntar; a volta recusada diz que só ela ficou fora
    /// da rodada, com o que falta para ela entrar.
    pub(super) fn next_line(&self, lang: Locale) -> String {
        let hint = self.refusal.message(lang);
        match self.refusal {
            RoundRefusal::Replan { .. } => hint,
            _ => translate("round.held_return", lang).replace("{wave}", &self.wave.to_string()).replace("{hint}", &hint),
        }
    }
}

/// Tira das voltas `waves` as que trocam uma decisão do usuário sem o "Aceitar"
/// dele gravado em `log`, e devolve cada uma com a recusa que manda perguntar
/// ([`HeldReturn`]). A mudança de plano que não troca decisão nenhuma não é
/// segurada. Depois do clique, a rodada seguinte assume a volta como qualquer
/// outra.
pub(super) fn hold_waiting_changes(log: &SpecLog, waves: &mut Vec<WaveReport>) -> Vec<HeldReturn> {
    hold_refused(waves, |wave| {
        let Some(PlanChange { change, decision: Some(_) }) = &wave.replan else { return Ok(()) };
        let code = replan_code(wave.wave, change);
        if change_accepted(log, wave.wave, &code) {
            return Ok(());
        }
        let tasks = wave.undone.iter().map(|(_, code)| code.clone()).collect();
        Err(RoundRefusal::Replan { wave: wave.wave, code, tasks })
    })
}

/// Tira das voltas `waves` cada uma que a conferência `check` recusa, e
/// devolve cada uma com a recusa dela ([`HeldReturn`]): a recusa de uma volta
/// segura só a onda dela, e as outras seguem.
pub(super) fn hold_refused(
    waves: &mut Vec<WaveReport>,
    mut check: impl FnMut(&WaveReport) -> Result<(), RoundRefusal>,
) -> Vec<HeldReturn> {
    let mut held = Vec::new();
    waves.retain(|wave| match check(wave) {
        Ok(()) => true,
        Err(refusal) => {
            held.push(HeldReturn { wave: wave.wave, refusal });
            false
        }
    });
    held
}

/// As ondas paradas pelo limite de consertos, cada uma com as reprovações
/// seguidas que a pararam: depois da primeira reprovação vêm no máximo
/// [`MAX_FIX_ROUNDS`] rodadas de conserto, e a reprovação seguinte para. A
/// conta começa na versão mais nova do plano da onda que o usuário pediu
/// depois da última reprovação: a onda que ele replanejou volta à fila com a
/// conta zerada, e o que o orquestrador acrescenta ao plano não zera nada.
pub(crate) fn waves_stuck(log: &SpecLog) -> BTreeMap<u64, Vec<&SpecEvent>> {
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
    let last_send = log.last_dispatch_by_wave();
    last_planned(log)
        .into_iter()
        .filter(|(n, planned)| last_send.get(n).is_some_and(|sent| sent < planned))
        .map(|(n, _)| n)
        .collect()
}

/// O que a volta da onda `wave` diz não ter feito (`undone`): as tarefas que
/// voltam ao backlog, cada uma pelo número da versão vigente e pelo código, e
/// se alguma das citadas já foi para outra onda. Cada código citado aponta
/// uma tarefa com que a onda saiu ([`sent_tasks`]), regravada ou não depois
/// do envio; o que não aponta recusa a volta, com essas tarefas. Volta ao
/// backlog só a tarefa cuja versão vigente leva a própria onda ou nenhuma. A
/// que leva outra onda conta para assumir a volta, mas fica com a onda que a
/// levou, na mesma versão: devolvê-la à fila faria um agente refazer o que
/// essa onda entrega ou já entregou. Com a mudança de plano (`replan`), a lista
/// é obrigatória, vazia quando a onda fez todas: sem ela, a rodada daria por
/// feitas as tarefas que o agente não fez. Sem a mudança, a ausência quer
/// dizer que a onda fez todas.
pub(super) fn undone_of(
    log: &SpecLog,
    wave: u64,
    fields: &Map<String, Value>,
    replan: bool,
) -> Result<(Vec<(u64, String)>, bool), RoundRefusal> {
    let codes = log.codes();
    let code_of = |task: &SpecEvent| codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
    let own = sent_tasks(log, wave);
    let tasks = || own.iter().map(|task| code_of(task)).collect::<Vec<_>>();
    let Some(cited) = fields.get("undone").and_then(Value::as_array) else {
        return if replan { Err(RoundRefusal::ReplanNeedsUndone { wave, tasks: tasks() }) } else { Ok((Vec::new(), false)) };
    };
    let mut undone: Vec<(u64, String)> = Vec::new();
    let mut taken_elsewhere = false;
    for value in cited {
        let said = value.as_str().map_or_else(|| value.to_string(), |code| code.trim().to_string());
        let Some(task) = own.iter().find(|task| code_of(task) == said) else {
            return Err(RoundRefusal::UndoneNotInWave { wave, code: said, tasks: tasks() });
        };
        if task.wave().is_some_and(|other| other != wave) {
            taken_elsewhere = true;
        } else if !undone.iter().any(|(id, _)| *id == task.id) {
            undone.push((task.id, said));
        }
    }
    Ok((undone, taken_elsewhere))
}

/// A versão nova da tarefa `task`, que a onda `wave` não fez: a mesma volta
/// ao backlog de [`backlog_return`]. Com a mudança de plano aceita
/// (`change`), a parte do agente ganha uma linha com ela, no idioma `lang`;
/// o título e a parte do usuário ficam como estão.
fn undone_return(task: &SpecEvent, wave: u64, change: Option<&str>, lang: Locale) -> Map<String, Value> {
    let mut draft = backlog_return(task);
    if let Some(change) = change {
        let line =
            translate("round.returned_change", lang).replace("{wave}", &wave.to_string()).replace("{change}", change.trim());
        append_agent_line(&mut draft, &line);
    }
    draft
}

/// Acrescenta a linha `line` ao fim da parte do agente da versão `draft` de
/// uma tarefa: como item da lista, quando a parte termina numa; como
/// parágrafo novo, quando termina em texto corrido; sozinha, quando a parte
/// está vazia ou falta.
pub(super) fn append_agent_line(draft: &mut Map<String, Value>, line: &str) {
    let before = draft.get("agent").and_then(Value::as_str).map(str::trim_end).filter(|text| !text.is_empty());
    let agent = match before {
        Some(text) if text.lines().last().is_some_and(|last| last.trim_start().starts_with("- ")) => {
            format!("{text}\n- {line}")
        }
        Some(text) => format!("{text}\n\n{line}"),
        None => line.to_string(),
    };
    draft.insert("agent".into(), json!(agent));
}

/// A versão nova de cada tarefa que as voltas `waves` não fizeram, com a
/// onda que a devolveu, lida em `log`: a volta ao backlog de
/// [`undone_return`].
pub(super) fn undone_returns(log: &SpecLog, waves: &[WaveReport], lang: Locale) -> Vec<(u64, Map<String, Value>)> {
    let mut out = Vec::new();
    for wave in waves {
        for task in wave.undone.iter().filter_map(|(id, _)| log.get(*id)) {
            let change = wave.replan.as_ref().map(|plan| plan.change.as_str());
            out.push((wave.wave, undone_return(task, wave.wave, change, lang)));
        }
    }
    out
}

/// O aviso da resposta da rodada de que as tarefas que a volta `wave` não
/// fez voltaram ao backlog, com os códigos delas, no idioma `lang`: a
/// mudança aceita pode pedir uma decisão nova ou a tarefa reescrita antes da
/// rodada seguinte. `None` quando a onda fez todas.
pub(super) fn tasks_returned(wave: &WaveReport, lang: Locale) -> Option<Value> {
    let codes: Vec<&str> = wave.undone.iter().map(|(_, code)| code.as_str()).collect();
    (!codes.is_empty()).then(|| {
        let hint = translate("round.tasks_returned", lang)
            .replace("{wave}", &wave.wave.to_string())
            .replace("{tasks}", &codes.join(", "));
        json!({ "reason": "tasks-returned", "wave": wave.wave, "tasks": codes, "hint": hint })
    })
}

/// O aviso da resposta da rodada de que a onda mudou o plano sem trocar
/// decisão do usuário: a rodada seguiu sem perguntar, e a mudança vai na
/// entrega, entre as decisões que o assistente tomou sozinho. `None` quando a
/// onda não muda o plano ou quando a mudança troca uma decisão, que o usuário
/// decidiu.
pub(super) fn plan_changed_alone(wave: &WaveReport, lang: Locale) -> Option<Value> {
    let PlanChange { change, decision: None } = wave.replan.as_ref()? else { return None };
    let hint = translate("round.replan_recorded", lang)
        .replace("{wave}", &wave.wave.to_string())
        .replace("{change}", &without_final_period(change));
    Some(json!({ "reason": "plan-changed", "wave": wave.wave, "hint": hint }))
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

    /// O agente que diz que o plano da onda não funciona segura a onda dele
    /// até o "sim" do usuário, e o "sim" é o clique em "Aceitar" na pergunta
    /// da mudança, gravado pela testemunha. A rodada não recusa: o aviso dela
    /// manda fazer a pergunta, com o cabeçalho e as opções; uma mensagem
    /// escrita à mão pelo modelo, com a mesma pergunta e a mesma resposta, não
    /// destrava nada; o clique em "Recusar" também não; o clique em "Aceitar"
    /// destrava, e a rodada grava o que a onda entregou.
    #[test]
    fn a_plan_change_that_swaps_a_user_decision_waits_for_the_users_click() {
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
        let back = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "replan": change,
            "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, back)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "a mudança segura só a onda dela: {out}");
        let stopped = change_asked(&out);
        assert_eq!(stopped["wave"], json!(1), "{out}");
        let question = QUESTION;
        assert_eq!(stopped["header"], json!(code), "{stopped}");
        assert_eq!(stopped["options"], json!(["Aceitar", "Recusar"]), "{stopped}");
        let hint = stopped["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&code), "o cabeçalho que a pergunta leva vai no aviso: {hint}");
        assert!(out["next"].as_str().unwrap_or_default().contains(hint), "o próximo passo manda perguntar: {out}");
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
        let still = round(root, "x", None);
        assert_eq!(change_asked(&still)["wave"], json!(1), "a forged yes accepts nothing: {still}");

        click(root, session, question, &code, "Recusar");
        let refused = round(root, "x", None);
        assert_eq!(change_asked(&refused)["wave"], json!(1), "a declined change stays waiting: {refused}");

        // O "sim" de uma mudança nunca serve para outra.
        click(root, session, question, &replan_code(1, "Outra mudança."), "Aceitar");
        let other = round(root, "x", None);
        assert_eq!(change_asked(&other)["wave"], json!(1), "{other}");
        assert_eq!(delivered_count(root), 0);

        click(root, session, question, &code, "Aceitar");
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert!(change_asked(&went).is_null(), "{went}");
        assert_eq!(delivered_count(root), 1, "the round records what the wave delivered");
    }

    /// A rodada manda fazer a pergunta e não a escreve: o aviso traz o
    /// cabeçalho e as opções. O código vai no cabeçalho da pergunta, e é ele,
    /// guardado ao lado da resposta, que reconhece o "sim" — a pergunta
    /// escrita com as palavras do usuário vale igual, o código de outra
    /// mudança não vale, e a pergunta sem código nenhum não destrava nada.
    #[test]
    fn question_goes_in_words_and_the_yes_is_recognised_by_the_code() {
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
        let back = json!({"wave": 1, "text": "Parei: o plano não fecha.", "replan": change,
            "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, back)["ok"], json!(true));
        let stopped = change_asked(&round(root, "x", None));
        assert_eq!(stopped["wave"], json!(1), "{stopped}");

        // O enunciado é de quem conduz: o aviso não traz pergunta pronta, e o
        // código vai no cabeçalho.
        assert!(stopped.get("question").is_none(), "o aviso não escreve a pergunta: {stopped}");
        assert_eq!(stopped["header"], json!(code), "o código vai no cabeçalho: {stopped}");
        assert_eq!(stopped["options"], json!(["Aceitar", "Recusar"]), "{stopped}");

        // A pergunta que quem despacha reescreve com as palavras do usuário,
        // sem o código em lugar nenhum do texto.
        let mine = "A onda 1 travou e quer a onda 2 antes dela. Posso seguir assim?";

        // O código de outra mudança no cabeçalho não aceita esta.
        click(root, session, mine, &replan_code(1, "Outra mudança."), "Aceitar");
        let other = round(root, "x", None);
        assert_eq!(change_asked(&other)["wave"], json!(1), "o sim de outra mudança não vale: {other}");

        // Sem código nenhum no cabeçalho, nada diz qual mudança o clique
        // decide, e a rodada segue parada.
        click(root, session, mine, "Mudança", "Aceitar");
        let blind = round(root, "x", None);
        assert_eq!(change_asked(&blind)["wave"], json!(1), "sem código não destrava: {blind}");

        // Com o código no cabeçalho, o "sim" vale, seja qual for a frase.
        click(root, session, mine, &code, "Aceitar");
        let went = round(root, "x", None);
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

    /// A mudança de plano que não troca decisão do usuário não pede clique: a
    /// rodada assume a volta, comita o que a onda entregou, grava a mudança na
    /// entrega e manda contá-la entre as decisões que o assistente tomou
    /// sozinho. A decisão só em branco também não troca nada.
    #[test]
    fn a_plan_change_that_swaps_no_decision_goes_on_without_a_click_and_is_told_as_decided_alone() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        for decision in [None, Some("   ")] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            round(root, "x", None);
            std::fs::write(copy_of(root, 1).join("src/a.rs"), "fn one() {}\n// a onda 1 mudou\n").unwrap();
            let change = "Dividir a soma em duas funções.";
            let mut back = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "commit": "a onda 1 mudou",
                "replan": change, "undone": []});
            if let Some(decision) = decision {
                back["changes_decision"] = json!(decision);
            }
            assert_eq!(returned(root, back)["ok"], json!(true));

            let out = round(root, "x", None);
            assert_eq!(out["ok"], json!(true), "{out}");
            assert!(change_asked(&out).is_null(), "sem decisão trocada não há clique a esperar: {out}");
            assert_eq!(official_deliveries(root), BTreeSet::from([1]), "{out}");
            assert_eq!(last_commit_files(root), "src/a.rs", "{out}");
            let noted = warning_of(&out, "plan-changed");
            assert_eq!(noted["wave"], json!(1), "{noted}");
            let hint = noted["hint"].as_str().unwrap_or_default();
            assert!(hint.contains("Dividir a soma em duas funções") && hint.contains("O que eu decidi sozinho"), "{hint}");
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let recorded = log.visible().into_iter().find(|e| e.event_type == "delivered").expect("a entrega");
            assert_eq!(recorded.str_field("replan"), Some(change), "a mudança fica gravada na entrega");
            assert!(!swaps_decision(recorded), "{:?}", recorded.fields);
        }
    }

    /// O aviso que manda perguntar não leva texto nenhum do agente — nem a
    /// mudança, nem a decisão que ela troca, nem o que a volta disse: quem
    /// conduz lê a volta e escreve a pergunta com as palavras do usuário, pelo
    /// estilo de resposta.
    #[test]
    fn the_stop_for_a_swapped_decision_carries_none_of_the_agents_text() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let (change, decision, said) =
            ("Mover fold_totals para src/zz_marca.rs e ajustar 37 chamadas", "Manter tudo em um arquivo só", "Parei no laboratório 1177");
        let back = json!({"wave": 1, "text": said, "replan": change, "changes_decision": decision, "undone": []});
        assert_eq!(returned(root, back)["ok"], json!(true));

        let out = round(root, "x", None);
        let stopped = change_asked(&out);
        assert_eq!(stopped["wave"], json!(1), "{out}");
        let shown = format!("{stopped} {}", out["next"]);
        for agent_text in [change, decision, said, "fold_totals", "zz_marca"] {
            assert!(!shown.contains(agent_text), "o aviso copiou o texto do agente ({agent_text}): {shown}");
        }
        assert!(!shown.contains("Pergunta pronta") && stopped.get("question").is_none(), "{shown}");
        let hint = stopped["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("três frases curtas") && hint.contains("no sim e no não"), "a regra da pergunta: {hint}");
    }

    /// A cópia gravada no último envio da onda `n` da spec `x`.
    fn copy_of(root: &Path, n: u64) -> std::path::PathBuf {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = mustard_core::io::wave_prompt::recorded_copy(&log, n).unwrap_or_else(|| panic!("a onda {n} sem cópia"));
        std::path::PathBuf::from(copy.path)
    }

    /// Os arquivos que o último commit de `root` mudou.
    fn last_commit_files(root: &Path) -> String {
        git_text(root, &["show", "--name-only", "--format=", "HEAD"])
    }

    /// As ondas com a entrega oficial gravada na spec `x`.
    fn official_deliveries(root: &Path) -> BTreeSet<u64> {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap().delivered_waves()
    }

    /// Duas ondas voltam na mesma rodada, e uma pede novo plano: a outra é
    /// conferida, comitada e gravada, e o despacho segue com a onda que só
    /// dependia dela. A que pede novo plano fica fora do commit, com a cópia
    /// como está e o aviso que manda perguntar; a onda que divide arquivo com
    /// ela e a que depende dela não saem. Depois do clique em "Aceitar", a
    /// rodada seguinte a comita, e as duas que ela segurava saem.
    #[test]
    fn a_wave_asking_for_a_new_plan_holds_only_itself_and_the_next_round_commits_it_after_the_click() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(
            root,
            "x",
            &[
                (1, &["src/a.rs"], &[]),
                (2, &["src/b.rs"], &[]),
                (3, &["src/a.rs"], &[]),
                (4, &["src/c.rs"], &[2]),
                (5, &["src/d.rs"], &[1]),
            ],
        );
        let mut first = waves_in(&round(root, "x", None), "dispatch");
        first.sort_unstable();
        assert_eq!(first, vec![1, 2], "a onda 3 divide o arquivo da 1");
        let (one, two) = (copy_of(root, 1), copy_of(root, 2));
        std::fs::write(one.join("src/a.rs"), "fn one() {}\n// a onda 1 mudou\n").unwrap();
        std::fs::write(two.join("src/b.rs"), "fn one() {}\n// a onda 2 mudou\n").unwrap();
        let change = "A onda 1 precisa de outra tarefa antes.";
        let code = replan_code(1, change);
        let asks = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "commit": "a onda 1 mudou",
            "replan": change, "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));
        let done = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "a onda 2 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));

        let held = round(root, "x", None);
        assert_eq!(held["ok"], json!(true), "a mudança não segura a outra volta: {held}");
        assert_eq!(last_commit_files(root), "src/b.rs", "só a onda 2 entra no commit: {held}");
        assert_eq!(official_deliveries(root), BTreeSet::from([2]), "{held}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n", "{held}");
        assert_eq!(std::fs::read_to_string(one.join("src/a.rs")).unwrap(), "fn one() {}\n// a onda 1 mudou\n");
        let asked = change_asked(&held);
        assert_eq!(asked["wave"], json!(1), "{held}");
        assert_eq!(asked["header"], json!(code), "{asked}");
        let question = QUESTION;
        let hint = asked["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&code), "o cabeçalho da pergunta vai no aviso: {hint}");
        assert!(held["next"].as_str().unwrap_or_default().contains(hint), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), vec![4], "a 3 divide arquivo com a 1, e a 5 depende dela: {held}");
        assert_eq!(waves_in(&held, "running"), vec![4], "a onda que espera o clique não está em andamento: {held}");

        let session = "s-segura-so-ela";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        click(root, session, question, &code, "Aceitar");
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert!(change_asked(&went).is_null(), "{went}");
        assert_eq!(last_commit_files(root), "src/a.rs", "a rodada seguinte comita a onda 1: {went}");
        assert_eq!(official_deliveries(root), BTreeSet::from([1, 2]), "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n// a onda 1 mudou\n");
        let mut sent = waves_in(&went, "dispatch");
        sent.sort_unstable();
        assert_eq!(sent, vec![3, 5], "as ondas que ela segurava saem: {went}");
    }

    /// A onda que pede novo plano já voltou e não é órfã, mesmo com o Claude
    /// Code que a mandou fechado: a cópia dela fica com o que ela entregou, a
    /// rodada não a manda de novo, e ela segue segurando o arquivo dela. Com o
    /// clique, a rodada comita o que ficou na cópia.
    #[test]
    fn a_wave_waiting_for_the_click_keeps_its_copy_when_its_sender_is_gone() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);

        // O Claude Code que mandou a onda 1 fechou: a versão nova do envio
        // leva um processo que já acabou. O programa que nasce e acaba é o
        // próprio executável do teste listando os testes, que toda máquina
        // tem, no lugar de um `true` que o Windows não traz.
        let mut gone = std::process::Command::new(std::env::current_exe().expect("o executável do teste"))
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("o processo de mentira");
        let pid = gone.id();
        gone.wait().expect("o processo acabou");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let extra = json!({"claude_pid": pid, "claude_started": 1}).as_object().cloned().unwrap();
        let draft = super::super::queue::send_revision(&log, 1, extra).expect("o envio da onda 1");
        let at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        store::write_at(&path, "send", draft, &[], &at).unwrap();

        let one = copy_of(root, 1);
        std::fs::write(one.join("src/a.rs"), "fn one() {}\n// a onda 1 mudou\n").unwrap();
        let change = "A onda 1 precisa de outra tarefa antes.";
        let asks = json!({"wave": 1, "text": "Parei.", "files": ["src/a.rs"], "commit": "a onda 1 mudou",
            "replan": change, "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));

        let held = round(root, "x", None);
        assert_eq!(held["ok"], json!(true), "{held}");
        assert_eq!(change_asked(&held)["wave"], json!(1), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), Vec::<u64>::new(), "nem reenvio, nem a onda do mesmo arquivo: {held}");
        assert_eq!(std::fs::read_to_string(one.join("src/a.rs")).unwrap(), "fn one() {}\n// a onda 1 mudou\n");

        let session = "s-copia-fica";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        click(root, session, QUESTION, &replan_code(1, change), "Aceitar");
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(last_commit_files(root), "src/a.rs", "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n// a onda 1 mudou\n");
    }

    /// O fechamento depende de todas as ondas: a que pede novo plano sem o
    /// clique o segura, com a pergunta que a decide, depois de assumir e
    /// comitar a outra volta.
    #[test]
    fn the_close_takes_the_other_return_and_is_held_by_the_wave_waiting_for_the_click() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(copy_of(root, 2).join("src/b.rs"), "fn one() {}\n// a onda 2 mudou\n").unwrap();
        let change = "A onda 1 precisa de outra tarefa antes.";
        let asks = json!({"wave": 1, "text": "Parei.", "replan": change, "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));
        let done = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "a onda 2 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));

        let opts = crate::commands::flow::close::CloseOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            report: None,
            ..Default::default()
        };
        let closed = crate::commands::flow::close::close_for(&opts, None);
        assert_eq!(closed["reason"], json!("wave-plan-does-not-work"), "{closed}");
        assert_eq!(closed["header"], json!(replan_code(1, change)), "{closed}");
        assert!(closed.get("question").is_none(), "{closed}");
        assert_eq!(official_deliveries(root), BTreeSet::from([2]), "{closed}");
        assert_eq!(last_commit_files(root), "src/b.rs", "{closed}");
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
            round(root, "x", Some(&verdict(root, 1, "rejected", &format!("reprovação {n}"))))
        };
        for n in 1..=2 {
            let fix = rejected(n);
            assert_eq!(waves_in(&fix, "dispatch"), vec![1], "rodada de conserto {n}: {fix}");
        }
        // Os envios de onda: o pedido de revisão que cada veredito pede não
        // conta.
        let sends = |root: &Path| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            log.visible().iter().filter(|e| e.event_type == "send" && e.wave().is_some()).count()
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
            seed_review(root);
            let wrote = judged(root, json!({"wave": 1, "result": "rejected", "final": true,
                "text": format!("reprovação {n}"), "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}],
                "agreed": agreed}));
            assert_eq!(wrote["ok"], json!(true), "{wrote}");
            round(root, "x", None)
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

    /// O aviso da resposta `out` com o motivo `reason`, ou nulo.
    fn held_warning(out: &Value, reason: &str) -> Value {
        let mut warnings = out["warnings"].as_array().into_iter().flatten();
        warnings.find(|w| w["reason"] == json!(reason)).cloned().unwrap_or(Value::Null)
    }

    /// A frase do próximo passo da volta da onda `wave` que ficou de fora,
    /// com a mensagem `hint` da recusa dela.
    fn held_line(wave: u64, hint: &Value) -> String {
        translate("round.held_return", Locale::PtBr)
            .replace("{wave}", &wave.to_string())
            .replace("{hint}", hint.as_str().unwrap_or_default())
    }

    /// Duas ondas voltam na mesma rodada, e a cópia de uma mudou arquivo sem
    /// o título do commit na volta: a outra é conferida, comitada e gravada, e
    /// o despacho segue com a onda que só dependia dela. A recusada fica fora
    /// do commit, com a cópia como está e o aviso e o próximo passo pedindo a
    /// volta de novo; ela não está em andamento, e a onda que depende dela não
    /// sai. Gravada de novo com o título, a rodada seguinte a comita, e a onda
    /// que dependia dela sai.
    #[test]
    fn a_return_without_the_commit_title_holds_only_its_wave_and_goes_in_once_recorded_again() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(
            root,
            "x",
            &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[2]), (4, &["src/d.rs"], &[1])],
        );
        let mut first = waves_in(&round(root, "x", None), "dispatch");
        first.sort_unstable();
        assert_eq!(first, vec![1, 2], "a 3 e a 4 esperam as dependências");
        let (one, two) = (copy_of(root, 1), copy_of(root, 2));
        let changed = "fn one() {}\n// a onda 1 mudou\n";
        std::fs::write(one.join("src/a.rs"), changed).unwrap();
        std::fs::write(two.join("src/b.rs"), "fn one() {}\n// a onda 2 mudou\n").unwrap();
        assert_eq!(returned(root, json!({"wave": 1, "text": "Mexi e não contei."}))["ok"], json!(true));
        let done = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "a onda 2 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));

        let held = round(root, "x", None);
        assert_eq!(held["ok"], json!(true), "a volta recusada não segura a outra: {held}");
        assert_eq!(last_commit_files(root), "src/b.rs", "só a onda 2 entra no commit: {held}");
        assert_eq!(official_deliveries(root), BTreeSet::from([2]), "{held}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n", "{held}");
        assert_eq!(std::fs::read_to_string(one.join("src/a.rs")).unwrap(), changed, "a cópia fica como está");
        let warning = held_warning(&held, "round-return-needs-commit");
        assert_eq!(warning["wave"], json!(1), "{held}");
        let line = held_line(1, &warning["hint"]);
        assert!(held["next"].as_str().unwrap_or_default().contains(&line), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), vec![3], "a 4 depende da 1: {held}");
        assert_eq!(waves_in(&held, "running"), vec![3], "a onda recusada não está em andamento: {held}");

        let again = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, again)["ok"], json!(true));
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert!(held_warning(&went, "round-return-needs-commit").is_null(), "{went}");
        assert_eq!(last_commit_files(root), "src/a.rs", "a volta gravada de novo entra: {went}");
        assert_eq!(official_deliveries(root), BTreeSet::from([1, 2]), "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), changed, "{went}");
        assert_eq!(waves_in(&went, "dispatch"), vec![4], "a onda que dependia dela sai: {went}");
    }

    /// A volta que a gravação não conferiu — de um binário antigo, ou de um
    /// plano que mudou depois dela — e que a rodada recusa ao lê-la segura só
    /// a própria onda, mesmo sozinha: a mudança de plano sem `undone`, a
    /// tarefa de outra onda em `undone` e o arquivo que não está no disco nem
    /// no git. A rodada responde sem recusar, com o aviso e o próximo passo da
    /// volta recusada; nada dela é gravado nem comitado, e a outra onda segue
    /// em andamento.
    #[test]
    fn a_return_the_round_refuses_on_reading_holds_only_its_wave_even_alone() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        for case in ["replan-needs-undone", "undone-not-in-wave", "round-file-unknown"] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
            assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2], "{case}");
            let path = store::spec_file(root, "x").unwrap();
            let log = store::read(&path).unwrap().unwrap();
            let other = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(2)).unwrap().id;
            let other = log.codes()[&other].clone();
            let mut body = match case {
                "replan-needs-undone" => json!({"wave": 1, "text": "Parei.", "replan": "Dividir a onda."}),
                "undone-not-in-wave" => json!({"wave": 1, "text": "Parei.", "undone": [other]}),
                _ => json!({"wave": 1, "text": "Saiu.", "files": ["src/nao_existe.rs"], "commit": "a onda 1 saiu"}),
            };
            body["returned"] = json!(true);
            body["author"] = json!("wave");
            store::write(&path, "delivered", body.as_object().cloned().unwrap(), &[]).unwrap();
            let head = git_text(root, &["rev-parse", "HEAD"]);

            let out = round(root, "x", None);
            assert_eq!(out["ok"], json!(true), "{case}: a recusa não vira a resposta: {out}");
            let warning = held_warning(&out, case);
            assert_eq!(warning["wave"], json!(1), "{case}: {out}");
            let line = held_line(1, &warning["hint"]);
            assert!(out["next"].as_str().unwrap_or_default().contains(&line), "{case}: {out}");
            assert_eq!(official_deliveries(root), BTreeSet::new(), "{case}: nada dela foi gravado: {out}");
            assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "{case}: nada foi comitado: {out}");
            assert_eq!(waves_in(&out, "running"), vec![2], "{case}: só a onda 2 segue em andamento: {out}");
        }
    }

    /// O pedido que o gancho monta para a onda `n` da spec `x` pelo bilhete,
    /// como o despacho de um agente o leva.
    fn hook_request(root: &Path, n: u64) -> String {
        use crate::hooks::task::subagent_inject::{SubagentInject, TICKET};
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "prompt": format!("{TICKET} x {n}"), "subagent_type": "general-purpose",
                "description": "onda" }),
            ..HookInput::default()
        };
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        ctx.config = mustard_core::ProjectConfig::load(root);
        match SubagentInject.evaluate(&input, &ctx).expect("never errors") {
            Verdict::Rewrite { tool_input, .. } => tool_input["prompt"].as_str().unwrap_or_default().to_string(),
            other => panic!("the dispatch is rewritten, got {other:?}"),
        }
    }

    /// O despacho de `prompt` ao agente `agent`, como o Claude Code o manda,
    /// pelo gancho do despacho.
    fn hook_dispatch(root: &Path, agent: &str, prompt: &str) -> mustard_core::domain::model::contract::Verdict {
        use crate::hooks::task::subagent_inject::SubagentInject;
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger};
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Agent".to_string()),
            tool_input: json!({ "prompt": prompt, "subagent_type": agent, "description": "onda" }),
            ..HookInput::default()
        };
        let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
        ctx.config = mustard_core::ProjectConfig::load(root);
        SubagentInject.evaluate(&input, &ctx).expect("never errors")
    }

    /// O despacho da onda em andamento que traz, além do título e do comando
    /// de leitura que a rodada devolveu, um aviso do condutor sai só com o
    /// título e o comando, com o recado ao condutor de que o texto a mais
    /// saiu; o título segue na primeira linha. O despacho que já é esse texto
    /// passa como veio. O título da onda que ainda espera a outra é barrado.
    #[test]
    fn a_wave_dispatch_goes_out_with_only_the_title_and_the_read_command() {
        use mustard_core::domain::model::contract::Verdict;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let read = out["dispatch"][0]["read"].as_str().unwrap_or_default().to_string();
        let title = mustard_core::domain::wave_prompt::wave_title("x", 1, Locale::PtBr);
        let exact = format!("{title}\n\n{read}");

        let extra = format!("{exact}\n\nAviso do condutor: rode a suíte inteira antes de entregar.");
        match hook_dispatch(root, "mustard-wave", &extra) {
            Verdict::Rewrite { tool_input, note } => {
                assert_eq!(tool_input["prompt"], json!(exact), "só o título e o comando");
                assert_eq!(tool_input["subagent_type"], json!("mustard-wave"));
                assert_eq!(tool_input["description"], json!("onda"));
                let said = translate("subagent.dispatch_replaced", Locale::PtBr).replace("{wave}", "1");
                assert_eq!(note, Some(said));
            }
            other => panic!("the dispatch is rewritten, got {other:?}"),
        }
        assert_eq!(hook_dispatch(root, "mustard-wave", &exact), Verdict::Allow);
        assert_eq!(hook_dispatch(root, "mustard-wave", &format!("{exact}\n")), Verdict::Allow);

        let waiting = mustard_core::domain::wave_prompt::wave_title("x", 2, Locale::PtBr);
        let refused = translate("subagent.wave_not_running", Locale::PtBr).replace("{spec}", "x").replace("{wave}", "2");
        assert_eq!(hook_dispatch(root, "mustard-wave", &format!("{waiting}\n\n{read}")), Verdict::Deny { reason: refused });
    }

    /// A onda que pede novo plano e espera o clique já voltou: nem a página
    /// nem o pedido das outras ondas a mostram em andamento. Na página, ela
    /// não está em andamento, e a onda que saiu junto está; o pedido da onda
    /// que saiu — o que a rodada gravou e o que o gancho monta pelo bilhete,
    /// iguais — não a lista entre as ondas em andamento.
    #[test]
    fn the_page_and_the_other_requests_do_not_show_the_wave_waiting_for_the_click_in_progress() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[]), (3, &["src/c.rs"], &[2])]);
        round(root, "x", None);
        std::fs::write(copy_of(root, 2).join("src/b.rs"), "fn one() {}\n// a onda 2 mudou\n").unwrap();
        let asks = json!({"wave": 1, "text": "Parei.", "replan": "A onda 1 precisa de outra tarefa antes.",
            "changes_decision": DECISION, "undone": []});
        assert_eq!(returned(root, asks)["ok"], json!(true));
        let done = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "a onda 2 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));

        let held = round(root, "x", None);
        assert_eq!(change_asked(&held)["wave"], json!(1), "{held}");
        assert_eq!(waves_in(&held, "dispatch"), vec![3], "{held}");

        let bodies = crate::commands::spec_events::pages::copy::sent(root, &held, "spec");
        let computed = bodies.iter().find(|w| w["collection"] == json!("computed")).expect("the computed item");
        assert_eq!(computed["body"]["waves"]["1"], json!("todo"), "a onda que espera o clique: {computed}");
        assert_eq!(computed["body"]["waves"]["3"], json!("running"), "{computed}");

        let recorded = request_of(&held, 3);
        let hooked = hook_request(root, 3);
        assert_eq!(hooked, recorded, "o gancho monta o mesmo pedido que a rodada gravou");
        let label = translate("prompt.execution.running", Locale::PtBr);
        assert!(!hooked.contains(label), "nenhuma outra onda em andamento: {hooked}");
    }

    /// A tarefa que a rodada devolve ao backlog duas vezes, sem mudança de
    /// plano, volta com a parte do agente que tinha: nenhuma linha se acumula
    /// a cada volta, nem a do resumo da onda que parou.
    #[test]
    fn a_task_returned_twice_without_a_plan_change_keeps_its_agent_part() {
        use crate::commands::flow::round::queue::{backlog_project, spec_now};

        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let agent = "Mexer em `src/a.rs`.\n- ler a função antes";
        let mut current = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Mexer no código de um.", "agent": agent, "files": [{"path": "src/a.rs"}],
                "depends_on": [], "covers": [crit], "origin": said}),
        ));
        let path = store::spec_file(root, "x").unwrap();
        for wave in [1, 2] {
            let log = spec_now(root);
            let draft = undone_return(log.current(current).expect("the task"), wave, None, Locale::PtBr);
            assert_eq!(draft.get("agent"), Some(&json!(agent)), "return {wave}: {draft:?}");
            current = store::write(&path, "task", draft, &[]).unwrap().id;
        }
    }
}
