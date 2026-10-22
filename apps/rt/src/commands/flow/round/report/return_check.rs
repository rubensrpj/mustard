//! As conferências que a gravação de uma volta faz antes de gravar: a entrega
//! da onda (`run write delivered`) e o veredito do revisor (`run write
//! verdict`), cada uma com a trava do passo do git, e o veredito como a rodada
//! o grava, que a conferência e a rodada leem do mesmo jeito.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::domain::wave_prompt as agreed_prompt;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt;
use mustard_core::platform::i18n::Locale;
use serde_json::{Map, Value, json};

use super::super::agreed::{Unmet, covered_codes, request_agreed, settle_agreed};
use super::super::commit::{git_lock, unknown_file};
use super::super::commit_title::message;
use super::super::finish_check::{proof_older_than_files, unfinished_work};
use super::super::queue::{open_review, open_sends, sends_without_return};
use super::super::read_check::{request_name, unread_items, without_undone};
use super::super::task_release::held_back;
use super::{RoundRefusal, criterion_id, observations, proof_target, wave_report_of};

/// As conferências da volta que só leem a própria volta, feitas na gravação
/// dela (`run write delivered`), antes de gravar: há envio aberto para a
/// onda; a volta se lê como a rodada a assumirá; o resumo do commit cabe no
/// teto; cada arquivo entregue
/// está no disco ou no git; cada prova aponta um item da spec e é uma
/// linha de comando; cada item combinado que o pedido da onda levou tem
/// resposta em `agreed` ([`request_agreed`]); o agente leu o pedido; e a
/// volta não devolve trabalho começado nem dá tarefa por feita sem o passo
/// de término dela ([`unfinished_work`]); e nenhum arquivo entregue mudou
/// depois do último passo de término, o da prova verde
/// ([`proof_older_than_files`]). Passando, a volta ganha
/// `returned` e o autor da onda, e os arquivos ficam relativos ao
/// repositório. A conferência que depende de
/// juntar a cópia fica na rodada. Sem o número da onda, nada aqui é
/// conferido: a gravação recusa pelo campo que falta.
///
/// Devolve, com os avisos da gravação no idioma `lang` (a prova que citou
/// outro item que não um critério), a trava do passo do git, a mesma que a rodada segura da leitura
/// das voltas até a entrega oficial, presa antes de ler a spec: quem grava a
/// segura até a volta estar no arquivo. A volta que chega enquanto a rodada
/// assume a mesma onda espera por ela e é conferida depois, com o envio já
/// fechado pela entrega oficial, e é recusada; a que entra antes a rodada lê
/// e assume. Nenhuma fica depois da entrega oficial para a rodada seguinte
/// assumir de novo.
///
/// O veredito do revisor (`event_type` `verdict`) só prende a trava aqui, pelo
/// mesmo motivo: o fechamento assume o veredito e grava o oficial com ela
/// presa, e o veredito que chegasse depois, com o pedido de revisão já
/// fechado, ficaria parado sem ninguém que o assumisse e a gravação
/// responderia ok. Com a trava, a conferência do veredito
/// ([`check_verdict_return`]) vê o pedido como o fechamento o deixou e recusa
/// a gravação quando ele fechou; o revisor grava de novo.
pub(crate) fn check_return(
    start: &Path,
    spec: &str,
    event_type: &str,
    draft: &mut Map<String, Value>,
    lang: Locale,
) -> Result<(LockedFile, Vec<String>), RoundRefusal> {
    let held = git_lock(&crate::commands::spec_events::project(start).root)?;
    let wave = draft.get("wave").and_then(Value::as_u64).filter(|_| event_type == "delivered");
    let Some(wave) = wave else { return Ok((held, Vec::new())) };
    let (project, log) = spec_log(start, spec)?;
    // O envio que o agente recebeu basta: um envio daquela onda sem volta.
    //
    // A leitura de antes tirava dois grupos — a onda replanejada e a que saiu
    // do plano —, e as duas coisas acontecem ENQUANTO o agente trabalha, sem
    // culpa dele. A entrega dele era recusada por uma mudança que ele não
    // fez, e o trabalho ia embora: foram 93 das 406 recusas de um projeto de
    // verdade, e a causa de duas ondas que nunca saíram. A recusa fica para o
    // caso real: a onda que não tem envio nenhum. Quem avisa que o plano
    // mudou é a rodada, mais abaixo.
    let Some(sent) = sends_without_return(&log).get(&wave).copied() else {
        return Err(RoundRefusal::Refused(Refusal::NoOpenSend { wave }));
    };
    let send_changed = !open_sends(&log).contains_key(&wave);
    observations::check(&project.root,spec,&log,sent,wave,draft)?;
    if let Some(knowledge) = draft.get("knowledge") {
        let copy = wave_prompt::recorded_copy(&log, wave);
        let tree = copy
            .as_ref()
            .map_or(project.root.as_path(), |copy| Path::new(&copy.path));
        mustard_core::io::knowledge::waves::validate(tree, knowledge).map_err(|reason| {
            RoundRefusal::Refused(Refusal::MissingField {
                event_type: "delivered".into(),
                field: format!("knowledge: {reason}"),
            })
        })?;
    }
    let mut report = wave_report_of(&log, draft)?;
    // A mensagem sai pela mesma montagem do commit da rodada ([`message`]): o
    // resumo que passa aqui passa também no commit da onda. A volta que não
    // cita arquivo também tem o resumo conferido: a
    // cópia pode ter mudado arquivo, e aí a rodada comita com ele.
    let cited = report.files.clone();
    if report.files.is_empty() {
        report.files.push(String::from("."));
    }
    message(&project.root, &report, lang)?;
    report.files = cited;
    unknown_file(&project.root, &log, std::slice::from_ref(&report))?;
    // A prova que cita outro item que não um critério entra com aviso: vale
    // para o critério que ele leva, quando leva a um só, e senão fica como
    // dele ([`proof_target`]), como a rodada a assumirá. A entrega guarda a
    // prova como veio. Só o item que a spec não tem recusa.
    let mut warnings = Vec::new();
    // O plano da onda mudou depois de ela sair, ou ela saiu do plano: a
    // entrega entra, e o aviso diz o que mudou.
    if send_changed {
        warnings.push(crate::shared::say::say("round.send_changed", lang, &[("{wave}", &wave.to_string())]));
    }
    for (reference, proof) in &report.proofs {
        let target = proof_target(&log, reference).map_err(RoundRefusal::Refused)?;
        let named = target.named();
        let code = log.codes().get(&named).cloned().unwrap_or_else(|| named.to_string());
        agreed_prompt::proof_rule(&code, proof).map_err(RoundRefusal::Refused)?;
        warnings.extend(target.warning(&log, lang).map(|(_, hint)| hint));
    }
    // A entrega presta conta de cada item combinado que o pedido levou, como
    // o veredito final presta de todo o combinado: faltar algum recusa, com
    // os códigos. O pedido sem item combinado não exige o campo.
    let expected = request_agreed(&log, wave);
    let (_, missing) = settle_agreed(&log, &mut draft.clone(), &expected, &mut BTreeSet::new(), &BTreeSet::new()).map_err(RoundRefusal::Refused)?;
    if !missing.is_empty() {
        return Err(RoundRefusal::Refused(Refusal::DeliveryAgreedMissing { wave, missing }));
    }
    // E de ter lido cada item que o pedido lista: o pedido de antes desta
    // conferência não grava a lista, e a onda dele não é cobrada. A leitura é
    // das tarefas feitas: o que só as devolvidas em `undone` pedem não conta,
    // nem o que só as tarefas ainda não liberadas trazem.
    let undone: Vec<String> = report.undone.iter().map(|(_, code)| code.clone()).collect();
    let not_released = held_back(&log, sent, wave);
    let unread = without_undone(&log, wave, &undone, unread_items(&log, sent, &request_name(Some(wave))));
    let unread: Vec<String> = unread.into_iter().filter(|item| !not_released.contains_key(item)).collect();
    if !unread.is_empty() {
        return Err(RoundRefusal::Refused(Refusal::DeliveryReadMissing { wave, missing: unread }));
    }
    // E de não devolver trabalho começado: a tarefa devolvida como não feita
    // com a cópia mudada depois do último passo de término, e a tarefa dada
    // como feita sem o passo dela, recusam.
    unfinished_work(&log, sent, &report, draft)?;
    // E de não levar ao commit arquivo mudado depois da prova verde.
    proof_older_than_files(&log, sent, &report)?;
    if draft.contains_key("files") {
        draft.insert("files".into(), json!(report.files));
    }
    draft.insert("returned".into(), json!(true));
    draft.insert("author".into(), json!("wave"));
    Ok((held, warnings))
}

/// As conferências do veredito que o revisor grava (`run write verdict`),
/// feitas antes de gravar, como as da entrega, com a trava do passo do git
/// que [`check_return`] prendeu: há pedido de revisão aberto;
/// cada critério e cada item do combinado citado existe; o veredito final
/// responde pelo combinado que lhe cabe ([`agreed_prompt::review_agreed`]); e o revisor leu, de dentro da cópia
/// dele, cada item que o pedido da revisão lista. Passando, o veredito ganha
/// `returned` e o autor da revisão, e fica como o revisor o escreveu: a
/// rodada o resolve de novo ao assumi-lo.
pub(crate) fn check_verdict_return(start: &Path, spec: &str, draft: &mut Map<String, Value>) -> Result<(), RoundRefusal> {
    let (_, log) = spec_log(start, spec)?;
    // O veredito com a onda de um pedido de revisão de onda aberto responde a
    // ele, e nunca à revisão final: a gravação o marca, e só ele leva a marca.
    let wave = draft.get("wave").and_then(Value::as_u64);
    if draft.get("final") != Some(&Value::Bool(true)) && wave.is_some_and(|wave| super::super::wave_review::open_requests(&log).contains_key(&wave)) {
        settle_verdict(&log, &mut draft.clone()).map_err(RoundRefusal::Refused)?;
        draft.insert("wave_review".into(), json!(true));
        draft.insert("returned".into(), json!(true));
        draft.insert("author".into(), json!("review"));
        return Ok(());
    }
    draft.remove("wave_review");
    let Some(asked) = open_review(&log) else {
        return Err(RoundRefusal::NoOpenReview);
    };
    settle_verdict(&log, &mut draft.clone()).map_err(RoundRefusal::Refused)?;
    // E de ter lido cada item que o pedido da revisão lista: o pedido de antes
    // desta conferência não grava a lista, e o veredito dele não é cobrado.
    let unread = unread_items(&log, asked, &request_name(None));
    if !unread.is_empty() {
        return Err(RoundRefusal::Refused(Refusal::VerdictReadMissing { missing: unread }));
    }
    draft.insert("returned".into(), json!(true));
    draft.insert("author".into(), json!("review"));
    Ok(())
}

/// O veredito como a rodada o grava: cada critério citado pelo número dele e,
/// no veredito final, cada item do combinado também. A revisão final responde
/// pelo combinado que o pedido listou e pelo que nenhuma tarefa terminada
/// cobre ([`agreed_prompt::review_agreed`]), item a item, em `agreed`: faltar algum, ou a
/// lista inteira, é veredito malformado, e nada é gravado. O item que não
/// vem `met:true` força o resultado a reprovado, também quando nenhuma tarefa
/// nasce, e volta, se nenhuma tarefa ainda por entregar já o cobre, para o
/// classificador lhe dar destino
/// ([`place_review_unmet`](super::super::agreed::place_review_unmet)): são
/// esses itens que a função devolve.
pub(in crate::commands::flow::round) fn settle_verdict(log: &SpecLog, draft: &mut Map<String, Value>) -> Result<Vec<Unmet>, Refusal> {
    if let Some(Value::Array(criteria)) = draft.get_mut("criteria") {
        for item in criteria.iter_mut() {
            if let Some(reference) = item.get("criterion").cloned() {
                item["criterion"] = json!(criterion_id(log, &reference)?);
            }
        }
    }
    if draft.get("final") != Some(&Value::Bool(true)) {
        return Ok(Vec::new());
    }
    let mut covered = covered_codes(log, &BTreeSet::new());
    let (items, missing) = settle_agreed(log, draft, &agreed_prompt::review_agreed(log), &mut covered, &BTreeSet::new())?;
    if !missing.is_empty() {
        return Err(Refusal::AgreedItemsMissing { missing });
    }
    let unmet = draft.get("agreed").and_then(Value::as_array).into_iter().flatten().any(|item| item.get("met").and_then(Value::as_bool) != Some(true));
    if unmet {
        draft.insert("result".into(), json!("rejected"));
    }
    Ok(items)
}

/// O projeto de `start` e o arquivo de eventos da spec `spec`, que a
/// gravação de uma volta confere.
fn spec_log(start: &Path, spec: &str) -> Result<(crate::commands::spec_events::Project, SpecLog), RoundRefusal> {
    let project = crate::commands::spec_events::project(start);
    let path = store::spec_file(&project.root, spec).map_err(RoundRefusal::Refused)?;
    let log = store::read(&path).map_err(RoundRefusal::Refused)?.ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.to_string() }))?;
    Ok((project, log))
}
