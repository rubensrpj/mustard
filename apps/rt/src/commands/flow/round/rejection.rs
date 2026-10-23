//! A reprovação de quem conduz a obra: a linha `REJECTED` do relatório, com a
//! onda e o motivo. A rodada grava a reprovação na spec, presa à volta que ela
//! reprova, segura essa volta fora do commit pelo mesmo caminho da volta que
//! ela mesma recusa ([`HeldReturn`]) e leva o motivo ao agente novo da onda,
//! pelo arquivo do trecho de conserto que o gancho do despacho lê. Enquanto a
//! volta reprovada for a última da onda, cada rodada segue segurando-a; a
//! volta nova desfaz a reprovação, e a rodada a assume como qualquer outra.
//! Ela só pode vir do agente novo: toda chamada do agente que a reprovação
//! tirou da onda é recusada ([`replaced_in_file`]), gravar na spec inclusive.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use mustard_core::domain::spec_events::{parse_log, SpecLog};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::domain::wave_prompt::wave_title;
use mustard_core::io::fs::lock::read_shared;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

use super::answer::{without_final_period, RoundRefusal};
use super::fixes::fix_file;
use super::queue::{agent_launched, rejected_send, send_revision, waves_awaiting_new_agent, without_agent};
use super::report::{line_object, tagged};
use super::stops::HeldReturn;
use super::usage::{measure_usage, usage_missing, Caller, Usage};
use crate::commands::spec_events::write::{record, record_locked};

/// A linha com que quem conduz a obra reprova a volta de uma onda, com o
/// motivo: `<REJECTED>{"wave":1,"reason":"…"}</REJECTED>`. Ela também marca
/// que o agente terminou, como a `USAGE`: o consumo dele é medido junto, sem a
/// outra linha.
const REJECTED_LINE: &str = "REJECTED";

/// O campo do envio da onda que guarda a reprovação de quem conduz a obra:
/// a volta reprovada e o motivo.
const REJECTED_FIELD: &str = "rejected";

/// Cada linha `REJECTED` do relatório `raw`, com a onda e o motivo, na ordem.
/// A linha sem a onda, sem o motivo ou com o motivo em branco é recusada pelo
/// campo que falta.
pub(super) fn rejected_lines(raw: &str) -> Result<Vec<(u64, String)>, RoundRefusal> {
    let mut rejected = Vec::new();
    for body in tagged(raw, REJECTED_LINE) {
        let (wave, fields) = line_object(body, REJECTED_LINE)?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: REJECTED_LINE, field: "wave" })?;
        let reason = fields.get("reason").and_then(Value::as_str).map(str::trim).filter(|said| !said.is_empty());
        let reason = reason.ok_or(RoundRefusal::LineField { line: REJECTED_LINE, field: "reason" })?;
        rejected.push((wave, reason.to_string()));
    }
    Ok(rejected)
}

/// A reprovação que quem conduz a obra gravou para a onda `wave`, lida do
/// envio mais novo dela: o número da volta reprovada e o motivo. A versão do
/// envio que o despacho de um agente novo grava depois a leva junto.
pub(super) fn rejection_of(log: &SpecLog, wave: u64) -> Option<(u64, String)> {
    let sent = log.get(*log.last_by_wave("send").get(&wave)?)?;
    let rejected = sent.fields.get(REJECTED_FIELD)?;
    Some((rejected.get("delivered")?.as_u64()?, rejected.get("reason")?.as_str()?.to_string()))
}

/// Se o agente da onda `wave` cuja conversa começou em `started` é um que a
/// reprovação de quem conduz a obra tirou dela: a conversa dele começou antes
/// da volta reprovada, então foi ele, ou um agente ainda mais antigo, quem a
/// fez. O agente novo nasce depois da reprovação e segue dono da onda, também
/// quando a rodada recusar a volta dele. Nada sem reprovação gravada para a
/// onda, nem quando a hora da volta reprovada não se lê.
pub(crate) fn replaced_by_rejection(log: &SpecLog, wave: u64, started: DateTime<Utc>) -> bool {
    let Some((back, _)) = rejection_of(log, wave) else { return false };
    let returned = log.get(back).and_then(|event| DateTime::parse_from_rfc3339(event.at()).ok());
    returned.is_some_and(|returned| started < returned.with_timezone(&Utc))
}

/// A mesma pergunta de [`replaced_by_rejection`], lida do arquivo de eventos
/// em `path`. Quem pergunta é o gancho, a cada chamada de um agente de onda,
/// e ler todos os eventos custa várias vezes o resto do gancho: eles só são
/// lidos quando uma linha do arquivo traz o campo da reprovação num envio da
/// onda `wave`. Sem reprovação dela, o agente das outras ondas não paga essa
/// leitura. Nada sem arquivo legível.
pub(crate) fn replaced_in_file(path: &Path, wave: u64, started: DateTime<Utc>) -> bool {
    let Ok(content) = read_shared(path) else { return false };
    let line_of = |at: usize| {
        let start = content[..at].rfind('\n').map_or(0, |end| end + 1);
        &content[start..content[at..].find('\n').map_or(content.len(), |end| at + end)]
    };
    let marked = content.match_indices(&format!("\"{REJECTED_FIELD}\":")).any(|(at, _)| {
        parse_log(line_of(at)).events.iter().any(|event| event.event_type == "send" && event.wave() == Some(wave))
    });
    marked && replaced_by_rejection(&parse_log(&content), wave, started)
}

/// A volta da onda `wave` que espera a rodada: a última entrega que o agente
/// gravou depois do envio que despachou a onda e que nenhuma rodada assumiu
/// ([`SpecLog::returns_after_dispatch`]).
pub(super) fn pending_return(log: &SpecLog, wave: u64) -> Option<u64> {
    log.returns_after_dispatch().into_iter().filter(|e| e.wave() == Some(wave)).map(|e| e.id).max()
}

/// A recusa que segura a volta `last` da onda `wave`, quando é ela a que quem
/// conduz a obra reprovou: a rodada não a assume enquanto ela for a última da
/// onda. O título da recusa vem depois, em [`keep_rejections`].
pub(super) fn held_rejection(log: &SpecLog, wave: u64, last: u64) -> Option<RoundRefusal> {
    let (_, reason) = rejection_of(log, wave).filter(|(back, _)| *back == last)?;
    Some(RoundRefusal::Rejected { wave, reason, title: None })
}

/// Grava cada reprovação que o relatório trouxe (`rejected`, a onda e o
/// motivo), presa à volta que ela reprova: a versão nova do envio da onda,
/// com a volta e o motivo em `rejected`, o consumo do agente medido nos
/// arquivos de conversa de `caller`, como na linha `USAGE`, e sem o Claude
/// Code nem o lançamento do envio ([`without_agent`]) — a reprovação diz que
/// o agente terminou, e a onda passa a esperar um agente novo, com o Claude
/// Code que mandou a onda aberto ou fechado. A onda sem volta à espera da
/// rodada e a onda cuja volta já entrou num commit são recusadas antes de
/// qualquer gravação. A reprovação repetida da mesma volta com o mesmo
/// motivo não grava nada; com outro motivo, troca só o motivo, e o agente
/// novo que já saiu segue dono da cópia. Quem chama segura a trava do passo
/// do git e leu `log`, do arquivo `path`, sob ela: com algo gravado, `log` é
/// lido de novo. Devolve o que foi gravado e o aviso de cada onda cujo
/// consumo não foi achado.
pub(super) fn record_rejections(
    start: &Path,
    spec: &str,
    path: &Path,
    log: &mut SpecLog,
    rejected: &[(u64, String)],
    caller: Caller<'_>,
    lang: Locale,
) -> Result<(Vec<Value>, Vec<Value>), RoundRefusal> {
    // A última linha de cada onda vale: duas linhas da mesma onda gravariam
    // duas versões a partir da mesma leitura.
    let rejected: BTreeMap<u64, &str> = rejected.iter().map(|(wave, reason)| (*wave, reason.trim())).collect();
    if let Some(wave) = rejected.keys().find(|wave| pending_return(log, **wave).is_none()) {
        let commit = log.visible().into_iter().rfind(|e| e.event_type == "commit" && e.ints("waves").contains(wave));
        let sha = commit.map(|commit| commit.str_field("sha").unwrap_or_default().chars().take(7).collect());
        return Err(match sha {
            Some(sha) => RoundRefusal::RejectedCommitted { wave: *wave, sha },
            None => RoundRefusal::RejectedWithoutReturn { wave: *wave },
        });
    }
    let (mut recorded, mut warnings) = (Vec::new(), Vec::new());
    for (wave, reason) in rejected {
        let Some(back) = pending_return(log, wave) else { continue };
        let before = rejection_of(log, wave).filter(|(id, _)| *id == back);
        if before.as_ref().is_some_and(|(_, said)| said.trim() == reason) {
            continue;
        }
        let mut usage = Usage::default();
        measure_usage(&mustard_core::io::spec_events::spec_root(start), log, caller, std::iter::once((wave, &mut usage)));
        if usage.tokens.is_none() {
            warnings.push(usage_missing(wave, lang));
        }
        let mut extra = usage.fields();
        extra.insert(REJECTED_FIELD.into(), json!({ "delivered": back, "reason": reason }));
        let Some(mut draft) = send_revision(log, wave, extra) else { continue };
        // A primeira reprovação da volta tira a onda do agente que a fez: sem
        // Claude Code nem lançamento no envio, ela espera um agente novo.
        if before.is_none() {
            without_agent(&mut draft);
        }
        let written = record(start, spec, "send", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
    }
    if !recorded.is_empty()
        && let Some(fresh) = store::read(path).map_err(RoundRefusal::Refused)?
    {
        *log = fresh;
    }
    Ok((recorded, warnings))
}

/// Devolve à espera de um agente novo cada onda reprovada cujo envio mais
/// novo, já lido na entrada desta rodada (`entry`), traz o Claude Code do
/// despacho do agente novo e nenhum lançamento: o gancho grava o despacho
/// antes de o agente existir, e o agente que nunca começou não segura a onda.
/// A versão nova do envio sai sem o agente ([`without_agent`]), e o despacho
/// seguinte a toma. O despacho gravado depois da entrada desta rodada é de
/// agora e fica. O envio que já marca o lançamento também fica, mesmo quando
/// a marca veio herdada de uma reprovação gravada por uma versão antiga, que
/// só tirava o Claude Code do agente: a marca não diz se o lançamento é do
/// agente novo ou do antigo, e soltar a onda de um agente que trabalha poria
/// dois na mesma cópia. A leitura do envio, a conferência de que ninguém
/// lançou o agente e a gravação da versão nova correm com a trava do arquivo
/// de eventos presa, como em [`super::queue::record_launch`], sobre o arquivo
/// como está agora e não sobre `log`: o lançamento que chega depois de quem
/// chamou ler `log` é visto, e a onda não perde o agente que começou. Quem
/// chama segura a trava do passo do git; com algo gravado, `log` passa a ser
/// o arquivo como a gravação o deixou.
pub(super) fn release_unstarted(start: &Path, spec: &str, path: &Path, log: &mut SpecLog, entry: &SpecLog) -> Result<Vec<Value>, RoundRefusal> {
    let held = store::with_locked_writer(path, |locked| -> Result<Vec<Value>, RoundRefusal> {
        let mut recorded = Vec::new();
        let waves: Vec<u64> = locked.log().last_by_wave("send").into_keys().collect();
        for wave in waves {
            let Some(sent) = rejected_send(locked.log(), wave) else { continue };
            let claimed = locked.log().get(sent).is_some_and(|event| event.int("claude_pid").is_some());
            if !claimed || agent_launched(locked.log(), sent) || entry.get(sent).is_none() {
                continue;
            }
            let Some(mut draft) = send_revision(locked.log(), wave, serde_json::Map::new()) else { continue };
            without_agent(&mut draft);
            let written = record_locked(locked, start, spec, "send", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
            recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
        }
        if !recorded.is_empty() {
            *log = locked.log().clone();
        }
        Ok(recorded)
    })
    .map_err(RoundRefusal::Refused)?;
    held.transpose().map(Option::unwrap_or_default)
}

/// Grava, para cada volta que quem conduz a obra reprovou (`held`), o motivo
/// dele no arquivo do trecho de conserto da volta ([`fix_file`]), onde o
/// gancho do despacho do agente novo o lê, e dá à recusa de cada uma o título
/// da onda quando ela espera esse agente ([`waves_awaiting_new_agent`]): é o
/// título que a resposta manda despachar. O arquivo é conferido a cada rodada
/// que segura a volta: o motivo mora na spec, e o arquivo só o leva ao agente.
/// A falha de gravação deixa a onda sem o agente novo, e a resposta diz que
/// ele já trabalha.
pub(super) fn keep_rejections(root: &Path, spec: &str, log: &SpecLog, held: &mut [HeldReturn], lang: Locale) {
    for one in held.iter() {
        let RoundRefusal::Rejected { wave, reason, .. } = &one.refusal else { continue };
        let Some(file) = fix_file(root, spec, log, *wave) else { continue };
        let text = translate("round.rejected_fix", lang).replace("{wave}", &wave.to_string()).replace("{reason}", reason.trim());
        if std::fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
            let _ = file.parent().map(std::fs::create_dir_all);
            let _ = std::fs::write(&file, text);
        }
    }
    let awaiting = waves_awaiting_new_agent(root, spec, log);
    for one in held.iter_mut() {
        if let RoundRefusal::Rejected { wave, title, .. } = &mut one.refusal {
            *title = awaiting.contains_key(wave).then(|| wave_title(spec, *wave, lang));
        }
    }
}

/// A frase da volta reprovada, no idioma `lang`: com o título (`title`), a
/// onda espera o agente novo, e a frase manda despachá-lo por ele; sem ele, o
/// agente novo já saiu e trabalha na cópia.
pub(super) fn rejected_message(wave: u64, reason: &str, title: Option<&str>, lang: Locale) -> String {
    let key = if title.is_some() { "round.rejected" } else { "round.rejected_working" };
    translate(key, lang)
        .replace("{wave}", &wave.to_string())
        .replace("{title}", title.unwrap_or_default())
        .replace("{reason}", &without_final_period(reason))
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::SpecEvent;
    use tempfile::tempdir;

    use super::super::commit::copy_of;
    use super::super::queue::{launch_fields, record_launch, sender_fields};
    use super::super::tests::{approved, returned, round};
    use super::*;

    fn log_of(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// Os envios da onda 1 na spec de `root`, do mais antigo ao mais novo.
    fn sends(root: &Path) -> Vec<SpecEvent> {
        log_of(root).visible().into_iter().filter(|e| e.event_type == "send" && e.wave() == Some(1)).cloned().collect()
    }

    /// A onda 1 voltou, quem conduz a obra reprovou a volta, e o despacho do
    /// agente novo gravou o hospedeiro dele no envio, junto de `extra`. Nenhuma
    /// chamada do agente novo foi vista ainda.
    fn rejected_and_dispatched(root: &Path, extra: serde_json::Map<String, Value>) {
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let copy = copy_of(&log_of(root), 1).unwrap();
        std::fs::write(copy.join("src/a.rs"), "fn changed() {}\n").unwrap();
        let out = returned(root, json!({"wave":1,"text":"Concluída.","files":["src/a.rs"],"commit":"Altera comportamento"}));
        assert_eq!(out["ok"], true, "{out}");
        round(root, "x", Some(r#"<REJECTED>{"wave":1,"reason":"Falta o teste do caso vazio."}</REJECTED>"#));
        let log = log_of(root);
        assert!(rejection_of(&log, 1).is_some(), "the wave went back: {:?}", log.last_by_wave("send"));
        let mut fields = sender_fields(Some((4242, 1)));
        fields.extend(extra);
        let draft = send_revision(&log, 1, fields).unwrap();
        record(root, "x", "send", draft, PhaseWriter::Binary).unwrap();
    }

    /// O agente novo faz a primeira chamada depois de a rodada ler a spec e
    /// antes de a soltura ser gravada: a chamada grava o lançamento no
    /// arquivo, e uma soltura feita pela leitura antiga tiraria a onda de um
    /// agente que já trabalha. A onda fica com o agente que tem, e nada é
    /// gravado.
    #[test]
    fn a_launch_that_lands_after_the_reading_keeps_the_agent_of_the_rejected_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        rejected_and_dispatched(root, serde_json::Map::new());
        let path = store::spec_file(root, "x").unwrap();
        let entry = log_of(root);
        let mut read = entry.clone();
        let sent = *read.last_by_wave("send").get(&1).unwrap();
        assert!(!agent_launched(&read, sent), "no agent had started when the round read the spec");

        assert!(record_launch(root, "x", 1, read.dispatch_position(sent)), "the first call of the new agent records the launch");
        let written = sends(root);
        let Ok(released) = release_unstarted(root, "x", &path, &mut read, &entry) else { panic!("the release was refused") };

        assert_eq!(released, Vec::<Value>::new(), "the agent started: the wave is not released");
        let now = sends(root);
        assert_eq!(now.len(), written.len(), "nothing was written after the launch");
        let newest = now.last().unwrap();
        assert_eq!(newest.int("claude_pid"), Some(4242), "{:?}", newest.fields);
        assert!(agent_launched(&log_of(root), newest.id), "{:?}", newest.fields);
    }

    /// A reprovação gravada por uma versão antiga tirava só o hospedeiro do
    /// envio, e a marca de lançamento do agente reprovado seguia para as
    /// versões depois dela. Com o hospedeiro do agente novo gravado por cima,
    /// a marca não se distingue do lançamento do agente novo, e a onda não é
    /// solta: um segundo agente numa cópia que já tem um é pior que uma onda
    /// que espera o aviso de silêncio.
    #[test]
    fn a_launch_mark_inherited_from_an_older_rejection_keeps_the_wave_with_its_agent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        rejected_and_dispatched(root, launch_fields());
        let path = store::spec_file(root, "x").unwrap();
        let entry = log_of(root);
        let mut read = entry.clone();
        let sent = *read.last_by_wave("send").get(&1).unwrap();
        assert_eq!((read.get(sent).unwrap().int("claude_pid"), agent_launched(&read, sent)), (Some(4242), true));
        let written = sends(root);

        let Ok(released) = release_unstarted(root, "x", &path, &mut read, &entry) else { panic!("the release was refused") };

        assert_eq!(released, Vec::<Value>::new(), "the wave is not released");
        assert_eq!(sends(root).len(), written.len(), "nothing was written");
    }
}
