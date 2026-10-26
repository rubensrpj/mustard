//! `mustard-rt run round [--spec <nome>]` — uma rodada de ondas.
//!
//! É a porta única da execução, e cada rodada é uma chamada só. Sem relatório,
//! a rodada despacha: escolhe as ondas que podem sair juntas, preservando as
//! reservas de arquivos, e prepara no commit atual a vaga de cada uma, a cópia
//! fixa em que ela compila, monta o pedido de cada uma com a cópia dela,
//! grava o envio com o pedido exato como foi injetado e marca a spec como em
//! execução na primeira rodada. Com a entrega que uma onda gravou na spec e
//! que ainda não foi assumida, ela primeiro fecha o que voltou — junta ao
//! repositório principal os arquivos que cada cópia entregou e comita; a
//! cópia fica, para a próxima onda — e só então despacha a rodada seguinte.
//!
//! **A onda nasce quando sai.** Só existe a onda que está rodando; as outras
//! tarefas ficam no backlog, sem número de onda. A cada rodada, a montagem
//! (`backlog`) forma no máximo uma onda por vaga livre, cada uma com as
//! tarefas do mesmo tipo de trabalho, com decisões tipadas do Jev em cache,
//! ou por arquivos e relações atuais do scan quando não há Jev. A onda montada
//! que não saiu por outro motivo, como a falta de cópia, se desfaz, e as
//! tarefas dela voltam ao backlog e entram na conta. A que a rodada segurou à
//! espera da escolha dos itens ou da escolha das skills pelo Jev não se
//! desfaz: segue gravada, com as tarefas dela, e sai com o número que ganhou
//! na rodada em que a decisão chega.
//!
//! **A escolha dos itens do pedido.** Antes de criar a cópia de uma onda
//! pronta, a rodada olha os itens combinados que o pedido dela não leva nem
//! tira por conta própria (`item_choice`): os do projeto todo, sem a marca de
//! toda onda, os ligados aos arquivos que as tarefas dela mexem e os sem
//! ligação com ela. Com algum, uma chamada ao Jev por onda, feita antes das
//! travas da rodada (`dispatch_jev`), diz, por item, se ele governa algo que
//! as tarefas dela mudam ou testam; a onda sai na mesma rodada, sem parada de
//! quem conduz, e a que ficou sem decisão espera a rodada seguinte, só ela. Os itens que as tarefas
//! da onda fazem, os de toda onda (`every_wave`), os de que a onda é dona e as
//! lições vão sempre, sem chamada. O envio gravado leva os itens que ficaram e,
//! à parte, no campo `analysis`, o que saiu e o que entrou, cada um com a
//! chance que o Jev deu. Sem chave, ou com o recibo incerto da reserva do Jev
//! que falhou, o pedido leva o padrão: o projeto todo e os dos arquivos da
//! onda vão, e o sem ligação fica fora.
//!
//! **A entrega mora na spec, e a rodada a assume.** O agente de onda grava a
//! própria entrega com `mustard-rt run write delivered`, e só com o envio da
//! onda aberto: a gravação sai marcada como volta, escondida da leitura e da
//! cópia da página, e já confere o título do commit, o resumo com cara de
//! SHA, o arquivo que o projeto não conhece e a prova nova, sem gravar nada
//! quando uma falha. A entrega traz a onda, o texto, os arquivos, o resumo do
//! commit e, quando é o caso, a prova nova de um critério cujo teste mudou de
//! nome, as ondas que o conserto fecha, a mudança de plano e as sobras. A
//! rodada lê da spec a última volta de cada onda mais nova que a entrega
//! oficial dela, grava o veredito e depois a entrega oficial, com `replaces`
//! apontando as voltas desde o último envio — também na onda que o conserto
//! fecha, o que pede a revisão dela de novo —, grava a versão nova do critério
//! com a prova nova, formata só os arquivos da rodada, faz o commit com a
//! mensagem montada do resumo e grava cada sobra como tarefa da spec, no
//! backlog. O `--report` leva só o que o orquestrador
//! escreve: a `<REVIEWED>`, com que ele aceita a volta que viu pela revisão
//! de onda, sem copiar a impressão, a `<PAUSED>` e a `<REJECTED>`, com que
//! ele reprova a volta de uma onda e dá o motivo, que vai ao agente novo
//! dela. A onda que gravou a entrega não pede linha de uso: a
//! `<USAGE>{"wave":1}</USAGE>` fica para o agente que terminou sem gravá-la.
//! O consumo de
//! cada onda assumida — o modelo, os passos e os tokens do agente dela — e o
//! da conversa principal no ramo da spec a rodada mede nos arquivos de
//! conversa que a plataforma grava, na pasta de configuração dela e na sessão
//! de quem chama, e avisa a onda cujo arquivo não achou. O veredito também
//! mora na spec: o revisor o grava com `mustard-rt run write verdict`, só com
//! pedido de revisão aberto, e a rodada ou o fechamento o assume antes das
//! entregas, com `replaces` para as voltas dele. A linha de consumo sozinha
//! completa a onda que voltou; a de uma onda de lote com envio aberto, sem volta e com o
//! Claude Code dela fechado marca a onda cortada, e as tarefas dela voltam
//! para o backlog.
//!
//! **O que trava.** Uma spec que ainda não foi aprovada; o checkout fora do
//! ramo da spec, em que a rodada não comita nem abre cópia; o git que recusa
//! o commit; um relatório sem
//! nenhuma das linhas que a rodada lê, ou com uma linha de escolha sem campo
//! obrigatório; a linha de entrega ou de veredito no relatório, que manda
//! gravá-la pelo `run write`; a linha de consumo de uma onda com o
//! Claude Code dela aberto e sem volta gravada, e a volta cuja cópia mudou
//! arquivo sem o resumo do commit — as duas pedem que o agente grave a
//! entrega; um arquivo entregue que não está no disco nem no git, nem no
//! repositório principal nem na cópia; o resumo do commit fora do modelo
//! (acima do teto, link do claude.ai, o nome do modelo, assinatura de
//! coautoria ou e-mail de alguém), que a gravação da entrega já recusa; o
//! relatório em que um agente
//! diz que o plano da onda não funciona, que para a rodada e só segue com o
//! "sim" do usuário. O "sim" da mudança de plano é o clique em "Aceitar" na
//! pergunta dela, gravado pela testemunha como na aprovação da spec, e nunca a
//! leitura que o modelo faz de uma frase: a rodada não aceita código nenhum de
//! quem a chama.
//!
//! **O que para sem travar.** A onda reprovada depois da segunda rodada de
//! conserto segura só ela e as ondas que dependem dela: o resto da rodada
//! segue, e a resposta traz a pergunta ao usuário com os vereditos dela. A
//! onda que sai do plano deixa de contar, na rodada e no fechamento. A
//! entrega com um trecho que a junção da cópia não resolve fica de fora, sem
//! nada dela gravado: as outras voltas são juntadas, comitadas e gravadas, e a
//! resposta traz a recusa dela, com os trechos e o comando que a resolve na
//! cópia; só quando não sobra outra volta a recusa é a resposta. Do mesmo
//! jeito fica de fora só a onda que uma conferência da volta recusa: o
//! caminho que nunca entra num commit, a cópia que o git não lê, a gravação
//! que a spec recusaria, o cliente gerado atrás do schema que ela mudou e a
//! conferência depois da onda — a importação nova contra uma regra forte do
//! padrão do projeto e o resto do que a onda tirou, que a onda conserta na
//! mesma cópia, até a pergunta ao usuário depois da segunda rodada de
//! conserto.
//!
//! **As travas.** A rodada segura a trava das cópias da volta ao despacho,
//! inteira: duas rodadas ao mesmo tempo no mesmo checkout fazem cada passo
//! uma depois da outra, nunca soltam a mesma onda duas vezes, e o fechamento,
//! o descarte e a limpeza das cópias esperam por ela. A formação e a montagem
//! das ondas, a escolha dos itens, o preparo das cópias e a montagem dos
//! pedidos rodam só com ela; as perguntas ao Jev desses passos saem antes
//! dela, pela spec gravada, e com ela presa o despacho só lê a decisão
//! gravada (`dispatch_jev`). A trava do passo do git, a mesma da entrega
//! e do veredito, fica para a leitura do repositório, a junção, o commit e o
//! desfazer quando o git recusa, e, no despacho, só para reler a spec,
//! conferir de novo a onda a reenviar ou a relistar e gravar os envios: quem
//! entrega nunca espera o Jev nem o preparo de uma cópia. A ordem é sempre
//! cópias, passada do mapa, git, spec. As voltas são lidas de novo da spec já dentro da trava
//! do git, e por isso a volta que uma rodada assumiu nunca é assumida de novo
//! pela outra. O mapa acompanha o commit em segundo plano: a onda seguinte
//! sai sem esperar a passada, e o pedido não mostra linha do mapa de arquivo
//! que mudou depois dela. O commit sai só no ramo da spec, e a cópia só nasce dele. O
//! commit leva só os arquivos da rodada, por caminho, e a mensagem descreve a
//! mudança, sem nada do Mustard; a recusa do git volta o disco e o índice
//! deles, e nada da onda recusada entra no commit de outra. O arquivo de
//! dentro de um submódulo é comitado no submódulo, na branch de mesmo nome da
//! spec, e o commit do principal leva o ponteiro novo; a cópia da onda traz os
//! submódulos que ela toca.
//!
//! **O que avisa.** O formatador que o projeto declara e que não foi achado
//! sai pelo nome, em vez de a formatação ser pulada em silêncio; a prova nova
//! que sai verde sem rodar teste nenhum sai pelo código do critério; a cópia
//! que não pôde ser criada, cuja onda fica para a rodada seguinte; e a cópia
//! com mudança fora da entrega, que fica no disco em vez de ser apagada.
//! Cada onda que mudou arquivo volta com a linha de tamanho — as linhas
//! postas e tiradas e os arquivos mudados —, que sai na resposta como aviso
//! `wave-size`, e nunca no commit. O tamanho nunca recusa a onda.
//!
//! O painel local acompanha os eventos. Publicação externa só existe sob
//! pedido explícito, pela porta de publicação.

mod after_work;
mod agreed;
mod answer;
mod backlog;
mod checks;
mod evidence;
pub(crate) mod following;
mod scheduling;
pub(crate) use scheduling::snapshot as scheduling_snapshot;
mod commit;
mod commit_title;
mod unused_evidence;
pub(crate) use commit::{final_unused, work_maps};
mod copy_check;
mod dependencies;
pub(crate) use dependencies::prepare as prepare_copy_dependencies;
mod environment;
mod finish_check;
mod fixes;
mod git_ignored;
pub(crate) use git_ignored::all_ignored;
mod handoff;
mod imports_check;
pub(crate) mod dispatch_jev;
pub(crate) mod item_choice;
mod jev_decision;
mod jev_failure;
mod keep;
mod leftovers;
mod lost_commit;
mod map_pass;
mod next_step;
#[cfg(test)]
mod off_path_tests;
#[cfg(test)]
mod skill_cycle_tests;
#[cfg(test)]
mod dispatch_jev_tests;
#[cfg(test)]
mod triage_jev_tests;
mod outdated_request;
pub(crate) use outdated_request::{awaits_verdict, find as outdated_review};
mod planning;
pub(crate) use planning::needs_planning;
pub(crate) mod queue;
mod read_check;
mod rehearsal;
mod rejection;
mod removed_check;
mod report;
mod request_fit;
mod resend;
mod running;
pub(crate) mod review;
mod sent_tasks;
mod size_check;
mod slots;
mod stops;
mod summary_wave;
pub(crate) mod task_release;
mod triage_questions;
mod usage;
pub(crate) mod vacancy;
mod wave_commit;
mod wave_review;
pub(crate) use wave_review::{is_dispatch as is_wave_review_dispatch, is_wave_verdict, wave_review_text};

// O processo que já fechou, que os testes da rodada, os do gancho do despacho
// e os da pasta `tests/` dividem: um arquivo só, trazido pelo caminho.
#[cfg(test)]
#[path = "../../../tests/support/closed_process.rs"]
mod closed_process;

/// O código de mudança que um texto traz: a testemunha dos gestos o lê no
/// cabeçalho da pergunta que decide a mudança. A mudança proposta que ainda
/// espera o clique e as ondas paradas no limite de consertos, o bloco de
/// retomada as conta.
pub(crate) use stops::{append_agent_line, change_accepted, change_code_of, replan_code, swaps_decision, waves_stuck};

pub(crate) use commit::{refresh_map_if_stale, waves_checked_only};
pub(crate) use map_pass::{BACKGROUND_PASS_ENV, refresh_map_while_behind, refresh_now as refresh_map_now};
#[cfg(test)]
pub(crate) use map_pass::map_after_rounds;

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_state::SpecState as _;
use serde_json::{Value, json};

use crate::commands::spec_events;
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};

pub(crate) use answer::{RoundRefusal, agents_refreshed, minutes_since, read_command, wave_dispatch};
pub(crate) use jev_decision::{FINAL_REVIEW, Waiting};
pub(crate) use leftovers::{FinalFindings, record_final_leftovers};
pub(crate) use triage_questions::ask_after_locks;
pub(crate) use fixes::{fix_file, sweep_fixes};
pub(crate) use keep::{Kept, landed_kept};
pub(crate) use queue::{
    awaited_review, backlog_left, new_agent_escalation, open_review, send_revision, sender_fields, tasks_left, waves_awaiting_new_agent, waves_in_progress,
    waves_left_without_tasks, waves_pending_fix,
};
pub(crate) use read_check::request_name;
pub(crate) use rejection::{replaced_by_rejection, replaced_in_file};
#[cfg(test)]
pub(crate) use slots::copies_leave_with_the_test;
pub(crate) use slots::{
    Removal, clean_copy, code_kept_hint, ensure_copy, held_slots, leaving_place, local_file_ignored, local_file_missing,
    remove_single_copy, remove_spec_copies, reset_slot, slot_owner, remove_spec_copies_held, remove_spec_copies_under, spec_copies,
    standing_in_a_copy,
};
#[cfg(test)]
pub(crate) use tests::{deliver_after_refusals, finish_tasks, read_request, read_review, round_with_mine, seed_read, shipped_agent};
#[cfg(test)]
pub(crate) use imports_check::tests::mine_giving;
pub(crate) use usage::snapshot as usage_snapshot;
// Fora da rodada, só os testes do gancho do despacho leem o processo fechado,
// e eles rodam só no Linux.
pub(crate) use finish_check::mark_step_copy;
pub(crate) use queue::wave_states;
pub(crate) use report::{backlog_return, check_return, check_verdict_return, take_report};
#[cfg(all(test, target_os = "linux"))]
pub(crate) use tests::closed_process;
pub(crate) use usage::Caller;

/// As opções de `mustard-rt run round`.
pub struct RoundOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec cuja rodada corre; sem ela, a spec atual.
    pub spec: Option<String>,
    /// O relatório da rodada anterior, em JSON.
    pub report: Option<String>,
}

/// Keep dispatches, reviews, warnings and evidence intact; a wave-size warning
/// becomes a short change line.
pub(crate) fn projection(report: &Value) -> Value {
    let mut out = report.clone();
    if let Some(warnings) = out.get_mut("warnings").and_then(Value::as_array_mut) {
        let changes: Vec<_> = warnings.iter().filter(|warning| warning["reason"] == "wave-size")
            .map(|warning| json!({"wave":warning["wave"],"summary":warning["hint"]})).collect();
        warnings.retain(|warning| warning["reason"] != "wave-size");
        if !changes.is_empty() { out["changes"] = json!(changes); }
    }
    out
}

/// A resposta curta da rodada: a de [`projection`], com as tarefas seguradas
/// agrupadas pelo motivo ([`scheduling::grouped`]) — a frase da vaga cheia
/// sai uma vez, e não uma por tarefa.
pub(crate) fn short(projected: &Value) -> Value {
    let mut out = projected.clone();
    if let Some(held) = out.get("held").and_then(Value::as_array) {
        out["held"] = json!(scheduling::grouped(held));
    }
    out
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    #[test]
    fn compact_round_keeps_actions_and_turns_size_warnings_into_changes() {
        let full = json!({"ok":true,"dispatch":[{"wave":1,"read":"read request-1","agent":"mustard-wave"}],
            "warnings":[{"reason":"proof-missing"},{"reason":"wave-size","wave":1,"hint":"smaller"}],"stopped":[2],"commit":{"sha":"abc"}});
        let compact = projection(&full);
        for field in ["dispatch", "stopped", "commit"] {
            assert_eq!(compact[field], full[field]);
        }
        assert_eq!(compact["warnings"], json!([{"reason":"proof-missing"}]));
        assert_eq!(compact["changes"], json!([{"wave":1,"summary":"smaller"}]));
    }
}

/// O passo que a rodada devolve quando todas as ondas estão entregues e
/// aprovadas.
pub const DONE_STEP: &str = "close";

/// O núcleo testável de [`run`]. A sessão e a pasta de configuração da
/// plataforma vêm do ambiente. Nunca entra em pânico.
pub(crate) fn round_at(opts: &RoundOpts) -> Value {
    let session = session_from_env();
    let config_dir = mustard_core::claude_config_dir();
    round_in(opts, Caller { session: session.as_deref(), config_dir: config_dir.as_deref() })
}

/// [`round_at`] com a sessão recebida, que é como um teste a escolhe, sem a
/// pasta de configuração da plataforma: o consumo das ondas não é medido.
#[cfg(test)]
pub(crate) fn round_for(opts: &RoundOpts, session: Option<&str>) -> Value {
    round_in(opts, Caller { session, config_dir: None })
}

/// [`round_at`] com a sessão e a pasta de configuração da plataforma
/// recebidas (`caller`), que é como o teste do consumo as escolhe.
pub(crate) fn round_in(opts: &RoundOpts, caller: Caller<'_>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let unchecked = spec_without_base_check(opts, &project.root, caller.session);
    match answer::run_round(opts, &project.root, lang, caller) {
        Ok(mut report) => {
            // O lint do projeto na base roda no primeiro despacho de onda,
            // uma vez só: a rodada já passou por todas as conferências dela,
            // e a onda que acabou de sair ainda não tem commit na branch,
            // então o disco é a base. Só avisa quando não pôde gravar.
            let dispatched = report["dispatch"].as_array().is_some_and(|waves| !waves.is_empty());
            if let Some(spec) = unchecked.filter(|_| dispatched)
                && let Some(reason) = super::validation::record_base_check(&project.root, &spec)
            {
                spec_events::pages::push_warning(&mut report, "base-check-not-recorded", &reason);
            }
            report
        }
        Err(refusal) => refusal.to_value(lang),
    }
}

/// A spec desta rodada, quando ela ainda não teve onda enviada nem a
/// conferência da base gravada ([`super::validation::base_check_due`]). Lida
/// antes da rodada: depois dela, o envio que ela grava já esconde que este
/// foi o primeiro. A spec é a do pedido ou, sem ela, a atual da sessão,
/// como a rodada a acha.
fn spec_without_base_check(opts: &RoundOpts, root: &Path, session: Option<&str>) -> Option<String> {
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root)).active(session)?,
    };
    let log = mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root, &spec).ok()?).ok()??;
    super::validation::base_check_due(&log).then_some(spec)
}

/// A spec na fase `phase` pode ter ondas despachadas: está aprovada, ou já em
/// execução. É a mesma pergunta para a rodada e para a fila que ela mostra.
pub(crate) fn can_run(phase: &str) -> bool {
    matches!(phase, "approved" | "running")
}

/// O que os testes das partes da rodada dividem: a spec aprovada com o plano
/// que o teste pede, a chamada da rodada e as linhas que os agentes devolvem.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use mustard_core::io::spec_events as store;
    use serde_json::{Value, json};

    pub(crate) use super::closed_process::closed_process;
    use super::read_check::unread_items;
    use super::*;
    use crate::commands::spec_events::write::{WriteOpts, record_open, seed_at};

    /// A refusal preserves domain state while retaining execution metrics.
    pub(super) fn without_measurements(text: &str) -> String {
        text.lines().filter(|line| serde_json::from_str::<Value>(line).ok().is_none_or(|event| event["type"] != "stage_run")).collect::<Vec<_>>().join("\n")
    }

    pub(super) fn write(root: &Path, spec: &str, event_type: &str, body: Value) -> Value {
        seed_at(&WriteOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), event_type: event_type.into(), json: body.to_string() })
    }

    pub(super) fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("não gravou: {report}"))
    }

    /// A resposta do usuário à pergunta `question`, feita com o cabeçalho
    /// `header` — onde vai o código da mudança —, dada pela testemunha dos
    /// gestos, como o harness a entrega depois do clique.
    pub(super) fn click(root: &Path, session: &str, question: &str, header: &str, answer: &str) {
        use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger};
        let input = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("AskUserQuestion".to_string()),
            session_id: Some(session.to_string()),
            tool_input: json!({ "questions": [{ "question": question, "header": header,
                "options": [{ "label": "Aceitar" }, { "label": "Recusar" }] }] }),
            raw: json!({ "tool_response": { "answers": { question: answer } } }),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PostToolUse));
        crate::hooks::observe::approval_witness::ApprovalWitness.evaluate(&input, &ctx).expect("never errors");
    }

    pub(super) fn git_at(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Um projeto com arquivos no git e uma spec já aprovada, com o plano que
    /// o teste pedir: uma entrada por onda, com os arquivos das tarefas dela e
    /// as ondas de que ela depende.
    pub(super) fn approved(root: &Path, spec: &str, plan: &[(u64, &[&str], &[u64])]) {
        approved_with(root, spec, plan, |_| {});
    }

    /// [`approved`] com o que o teste grava antes da aprovação (`before`), que
    /// recebe o número da mensagem de origem: é antes dela que o levantamento
    /// grava os itens combinados, inclusive os sem dono.
    pub(super) fn approved_with(root: &Path, spec: &str, plan: &[(u64, &[&str], &[u64])], before: impl FnOnce(u64)) {
        let mut files = vec![("mustard.json", "{}")];
        files.extend(plan.iter().flat_map(|(_, planned, _)| planned.iter().map(|file| (*file, "fn one() {}\n"))));
        crate::shared::test_fixture::seeded_repo(root, &files);
        std::fs::create_dir_all(root.join("src")).unwrap();
        // O checkout fica no ramo da spec, como a abertura da obra o deixa: a
        // rodada só comita e só abre cópia nele.
        git_at(root, &["checkout", "-q", "-B", &format!("feature/{spec}")]);

        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, spec, "message", json!({"author": "user", "text": "o objetivo"})));
        // A prova é um comando que sempre passa, sem exigir um projeto Cargo
        // de verdade na cópia de teste: desde que a rodada roda a prova de
        // cada critério coberto antes de comitar, `cargo test` recusaria
        // todo commit destes testes, que escrevem em pastas soltas, sem
        // `Cargo.toml`.
        let crit = id_of(&write(
            root,
            spec,
            "criterion",
            json!({"when": "a onda roda", "then": "a suíte passa", "proof": "git --version", "form": "ubiquitous",
                "origin": said}),
        ));
        for (n, files, depends) in plan {
            let mut wave = json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit],
                "done_when": "A suíte passa.", "origin": said});
            if !depends.is_empty() {
                wave["depends_on"] = json!(depends);
            }
            write(root, spec, "wave", wave);
            let declared: Vec<Value> = files.iter().map(|f| json!({"path": f})).collect();
            write(
                root,
                spec,
                "task",
                json!({"wave": n, "text": format!("Tarefa da onda {n}."),
                "files": declared, "depends_on": [], "origin": said}),
            );
        }
        before(said);
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        copies_leave_with_the_test(root);
    }

    /// O texto que a instalação escreveria agora no agente `name` do projeto
    /// em `root`, no idioma e com o modelo e o esforço do `mustard.json` dele.
    pub(crate) fn shipped_agent(root: &Path, name: &str) -> String {
        let config = mustard_core::ProjectConfig::load(root);
        let rel = format!("agents/mustard/{name}.md");
        mustard_core::platform::project_seed::project_harness_texts(&config)
            .into_iter()
            .find_map(|(path, body)| (path == rel).then_some(body))
            .unwrap_or_else(|| panic!("a instalação não escreve o agente {name}"))
    }

    /// O mapa do projeto em `root`, como o scan o grava, com uma parte só, na
    /// raiz, do tipo `kind` (`cargo`, `npm`). Chame depois de [`approved`]:
    /// o mapa fica fora do commit, como no projeto de verdade.
    pub(super) fn mapped(root: &Path, kind: &str) {
        let model = json!({"projects": [{"name": "(root)", "dir": "", "kind": kind, "code_files": 1}]});
        mustard_core::io::project_map::write_text(root, &model.to_string()).unwrap();
    }

    /// O projeto em `root` com o submódulo `libs/sub`, clonado de um servidor
    /// em `servers` cuja base é `main`, com o arquivo `lib.txt` e quem comita
    /// configurado no submódulo. Chame antes de [`approved`].
    pub(super) fn with_submodule(root: &Path, servers: &Path) {
        let (server, seed) = (servers.join("sub.git"), servers.join("semente"));
        std::fs::create_dir_all(&seed).unwrap();
        std::fs::create_dir_all(root).unwrap();
        git_at(servers, &["init", "-q", "--bare", "-b", "main", "sub.git"]);
        git_at(&seed, &["init", "-q", "-b", "main"]);
        std::fs::write(seed.join("lib.txt"), "fn one() {}\n").unwrap();
        git_at(&seed, &["add", "-A"]);
        git_at(&seed, &["commit", "-q", "-m", "biblioteca"]);
        git_at(&seed, &["push", "-q", &server.to_string_lossy(), "main"]);
        git_at(root, &["init", "-q"]);
        git_at(root, &["config", "core.autocrlf", "false"]);
        git_at(root, &["config", "core.eol", "lf"]);
        let url = server.to_string_lossy().to_string();
        git_at(root, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &url, "libs/sub"]);
        git_at(root, &["commit", "-q", "-m", "submodulo"]);
        for (key, value) in [("user.email", "t@t"), ("user.name", "t"), ("commit.gpgsign", "false")] {
            git_at(&root.join("libs/sub"), &["config", key, value]);
        }
    }

    /// A saída do git em `dir`, sem as bordas.
    pub(super) fn git_text(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("git");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    pub(super) fn round(root: &Path, spec: &str, report: Option<&str>) -> Value {
        review::test_conductor_review(root, spec);
        launched(root, spec, round_for(&RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: report.map(str::to_string) }, None))
    }

    /// A resposta `out` depois de o condutor lançar o agente de cada onda que
    /// ela despachou e de o agente agir: o envio da onda passa a marcar o
    /// lançamento, como a observação do hospedeiro grava.
    pub(super) fn launched(root: &Path, spec: &str, out: Value) -> Value {
        for wave in waves_in(&out, "dispatch") {
            let Some(log) = store::read(&store::spec_file(root, spec).unwrap()).unwrap() else { continue };
            if let Some(sent) = log.last_by_wave("send").get(&wave) {
                queue::record_launch(root, spec, wave, log.dispatch_position(*sent));
            }
        }
        out
    }

    /// As opções de uma rodada da spec `x`, com o relatório `report`.
    fn round_opts(root: &Path, report: Option<&str>) -> RoundOpts {
        RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: report.map(str::to_string) }
    }

    /// A leitura de entrada de uma rodada da spec `x`, com o relatório
    /// `report`, pelo caminho de [`round`]: o que a rodada lê antes de assumir
    /// qualquer volta e de pegar a trava do passo do git.
    pub(super) fn round_entry(root: &Path, report: Option<&str>) -> Result<answer::Entry, RoundRefusal> {
        let project = spec_events::project(root);
        answer::enter_round(&round_opts(root, report), &project.root, None)
    }

    /// O resto da rodada da spec `x`, com o relatório `report`, a partir da
    /// leitura de entrada `entry` ([`round_entry`]), pelo caminho de
    /// [`round`]: a mesma resposta que ele daria.
    pub(super) fn round_from(root: &Path, report: Option<&str>, entry: Result<answer::Entry, RoundRefusal>) -> Value {
        review::test_conductor_review(root, "x");
        let project = spec_events::project(root);
        let opts = round_opts(root, report);
        let out = entry
            .and_then(|entry| answer::run_entered_round(&opts, &project.root, project.lang, Caller::default(), &answer::scan_mine, entry))
            .unwrap_or_else(|refusal| refusal.to_value(project.lang));
        launched(root, "x", out)
    }

    /// Duas rodadas da spec `x` ao mesmo tempo, com o relatório `report`,
    /// cada uma pelo caminho de [`round`]. As duas fazem a leitura de entrada
    /// antes de qualquer uma seguir: a volta de cada leitura é o sinal de que
    /// ela já aconteceu, sem esperar relógio nenhum. Só então as duas seguem,
    /// cada uma na sua linha de execução, disputando a trava do passo do git.
    /// Devolve a resposta de cada rodada.
    pub(super) fn two_rounds_at_once(root: &Path, report: Option<&str>) -> Vec<Value> {
        let entries = [(); 2].map(|()| round_entry(root, report));
        std::thread::scope(|scope| {
            let rounds = entries.map(|entry| scope.spawn(move || round_from(root, report, entry)));
            rounds.into_iter().map(|r| r.join().unwrap()).collect()
        })
    }

    /// [`round`] com quem relê o mapa depois do commit da rodada (`mine`),
    /// que um teste escolhe sem instalar a ferramenta do scan de verdade.
    pub(crate) fn round_with_mine(
        root: &Path,
        spec: &str,
        report: Option<&str>,
        mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<mustard_core::domain::scan::ScanReport>,
    ) -> Value {
        review::test_conductor_review(root, spec);
        let opts = RoundOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), report: report.map(str::to_string) };
        let project = spec_events::project(&opts.root);
        let out = match answer::run_round_with_mine(&opts, &project.root, project.lang, Caller::default(), mine) {
            Ok(report) => report,
            Err(refusal) => refusal.to_value(project.lang),
        };
        launched(root, spec, out)
    }

    /// O recibo que o `run read` de dentro da cópia grava quando lê `item` do
    /// pedido `request`, na spec `spec`.
    pub(crate) fn seed_read(root: &Path, spec: &str, request: &str, item: &str) {
        let call = json!({"author": "binary", "command": "read", "ms": 0, "result": "ok", "request": request, "items": [item]});
        crate::shared::spec_state::seed_event(root, spec, "call", call);
    }

    /// As leituras que o agente da onda `wave` da spec `spec` faz do pedido
    /// dela: um recibo para cada item da lista do envio aberto que ainda não
    /// foi lido. Sem envio aberto, ou de antes de o envio guardar a lista,
    /// não grava nada.
    pub(crate) fn read_request(root: &Path, spec: &str, wave: u64) {
        let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
        let Some(sent) = queue::open_sends(&log).get(&wave).copied() else {
            return;
        };
        let request = request_name(Some(wave));
        for item in unread_items(&log, sent, &request) {
            seed_read(root, spec, &request, &item);
        }
    }

    /// As leituras que o revisor da spec `spec` faz do pedido da revisão: uma
    /// chamada `read` por item da lista do envio de revisão aberto que ainda
    /// não foi lido. Sem pedido aberto, ou de antes de o envio guardar a lista,
    /// não grava nada.
    pub(crate) fn read_review(root: &Path, spec: &str) {
        let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
        let Some(sent) = open_review(&log) else {
            return;
        };
        let request = request_name(None);
        for item in unread_items(&log, sent, &request) {
            seed_read(root, spec, &request, &item);
        }
    }

    /// Uma linha do fim de um agente, como os textos dele ensinam.
    pub(super) fn line(tag: &str, body: Value) -> String {
        format!("<{tag}>{body}</{tag}>")
    }

    /// Os passos de término que o agente da onda `wave` da spec `spec` grava
    /// antes de entregar: um por tarefa com que a onda saiu, pelo
    /// `run write step`, como o agente o grava — a gravação põe a impressão
    /// da cópia de agora em cada um.
    pub(crate) fn finish_tasks(root: &Path, spec: &str, wave: u64) {
        let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
        let codes = log.codes();
        for task in sent_tasks::sent_tasks(&log, wave) {
            let item = codes.get(&task.id).map_or_else(|| json!(task.id), |code| json!(code));
            let step = crate::commands::spec_events::write::write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some(spec.to_string()),
                event_type: "step".into(),
                json: json!({"wave": wave, "item": item, "text": "A tarefa ficou pronta."}).to_string(),
            });
            assert_eq!(step["ok"], json!(true), "o passo da onda {wave} não gravou: {step}");
        }
    }

    /// A volta de uma onda da spec `spec`, gravada como o agente a grava, pela
    /// gravação `write`. A recusa por item do pedido não lido leva o agente a
    /// ler o pedido e gravar de novo; a recusa por tarefa sem passo de
    /// término, ou por trabalho começado, leva-o a gravar o passo de cada
    /// tarefa e entregar de novo. A resposta é a da última gravação; qualquer
    /// outra recusa volta como veio, sem nada gravado.
    pub(crate) fn deliver_after_refusals(root: &Path, spec: &str, body: &Value, write: &dyn Fn(&Value) -> Value) -> Value {
        let mut out = write(body);
        let Some(wave) = body["wave"].as_u64() else {
            return out;
        };
        for _ in 0..2 {
            match out["reason"].as_str() {
                Some("delivery-read-missing") => read_request(root, spec, wave),
                Some("delivery-done-without-step" | "delivery-started-work-undone") => finish_tasks(root, spec, wave),
                _ => return out,
            }
            out = write(body);
        }
        out
    }

    /// A volta de uma onda da spec `x`, gravada como o agente a grava: pelo
    /// `run write delivered`, com os campos de `body`, depois de ler o pedido
    /// e de gravar o passo de cada tarefa quando a gravação os cobra
    /// ([`deliver_after_refusals`]).
    pub(super) fn returned(root: &Path, body: Value) -> Value {
        deliver_after_refusals(root, "x", &body, &|body| returned_unread(root, body.clone()))
    }

    /// Como [`returned`], sem ler o pedido antes: a gravação do agente que
    /// entrega sem ter lido o que o pedido lista.
    pub(super) fn returned_unread(root: &Path, body: Value) -> Value {
        crate::commands::spec_events::write::write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".to_string()),
            event_type: "delivered".into(),
            json: body.to_string(),
        })
    }

    /// A volta da onda `wave`, com o resumo do commit, gravada pela porta do
    /// agente. Cada arquivo entregue que existe ganha uma linha, para o commit
    /// ter o que levar, e cada item combinado que o pedido da onda levou vem
    /// cumprido em `agreed`, como o texto do agente ensina. Devolve o
    /// relatório que o agente deixa depois de gravar: vazio, porque a entrega
    /// mora na spec.
    pub(super) fn delivered(root: &Path, wave: u64, text: &str, files: &[&str]) -> String {
        for file in files {
            let path = root.join(file);
            if let Ok(before) = std::fs::read_to_string(&path) {
                std::fs::write(&path, format!("{before}// {text}\n")).unwrap();
            }
        }
        let mut body = json!({"wave": wave, "text": text, "files": files, "commit": format!("a entrega {wave} saiu")});
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let agreed: Vec<Value> = agreed::request_agreed(&log, wave).iter().map(|item| json!({"item": item.id, "met": true})).collect();
        if !agreed.is_empty() {
            body["agreed"] = json!(agreed);
        }
        let wrote = returned(root, body);
        assert_eq!(wrote["ok"], json!(true), "a volta da onda {wave} não foi gravada: {wrote}");
        String::new()
    }

    /// O veredito da onda `wave`, com o critério pelo código, gravado como o
    /// revisor o grava: pelo `run write verdict`, com o pedido de revisão
    /// aberto antes, como o fechamento o abre. `final: true`, porque só o
    /// veredito final do agente de teste dedicado pode reprovar ou aprovar
    /// uma onda. Devolve o relatório que o revisor deixa depois de gravar:
    /// vazio, porque o veredito mora na spec.
    pub(super) fn verdict(root: &Path, wave: u64, result: &str, text: &str) -> String {
        seed_review(root);
        let body = json!({"wave": wave, "result": result, "final": true, "text": text,
            "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]});
        let wrote = judged(root, body);
        assert_eq!(wrote["ok"], json!(true), "o veredito da onda {wave} não foi gravado: {wrote}");
        String::new()
    }

    /// A volta do revisor da spec `x`, gravada como ele a grava: pelo `run
    /// write verdict`, com os campos de `body`. Devolve a resposta da
    /// gravação, com a recusa quando ela recusa.
    pub(super) fn judged(root: &Path, body: Value) -> Value {
        crate::commands::spec_events::write::write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".to_string()),
            event_type: "verdict".into(),
            json: body.to_string(),
        })
    }

    /// Um pedido de revisão da spec `x`, gravado sem passar pelo fechamento.
    /// Devolve o número dele.
    pub(super) fn seed_review(root: &Path) -> u64 {
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "send",
            json!({"role": "review", "text": "revise",
            "lines": 1, "chars": 6, "mustard": "0", "author": "binary"}),
        )
    }

    pub(super) fn delivered_count(root: &Path) -> usize {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.visible().iter().filter(|e| e.event_type == "delivered").count()
    }

    /// Todas as entregas escritas no arquivo da spec `x`: as oficiais e as
    /// voltas que os agentes gravaram, assumidas ou não.
    pub(super) fn written_deliveries(root: &Path) -> usize {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.events.iter().filter(|e| e.event_type == "delivered").count()
    }

    /// A linha de exemplo que o texto de um agente ensina a gravar — a que
    /// começa com `start` —, tirada do próprio texto, com os valores de
    /// exemplo trocados por `values`.
    pub(super) fn taught_line(template: &str, start: &str, values: &[(&str, &str)]) -> String {
        let found = template.lines().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no line starting {start}"));
        values.iter().fold(found.to_string(), |line, (from, to)| line.replacen(from, to, 1))
    }

    /// O subject e o corpo do último commit.
    pub(super) fn last_commit(root: &Path) -> (String, String) {
        let out = Command::new("git").args(["log", "-1", "--format=%s%n%b"]).current_dir(root).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let (subject, body) = text.split_once('\n').unwrap_or((&text, ""));
        (subject.to_string(), body.trim().to_string())
    }

    /// A decisão do usuário que a mudança de plano dos testes troca.
    pub(super) const DECISION: &str = "A ordem das ondas que o usuário escolheu.";

    /// A pergunta que quem conduz escreve ao usuário para essa mudança, com as
    /// palavras dele.
    pub(super) const QUESTION: &str = "A onda 1 pede a 2 antes dela, e isso troca a ordem que você escolheu. Posso seguir assim?";

    /// O aviso da resposta `out` com a onda que troca uma decisão do usuário e
    /// espera o clique dele; nulo quando nenhuma espera.
    pub(super) fn change_asked(out: &Value) -> Value {
        let mut warnings = out["warnings"].as_array().into_iter().flatten();
        warnings.find(|w| w["reason"] == json!("wave-plan-does-not-work")).cloned().unwrap_or(Value::Null)
    }

    /// O aviso da resposta `out` com o motivo `reason`, que precisa existir:
    /// o teste que o pede falha, com a resposta inteira, quando ele não saiu.
    pub(super) fn warning_of(out: &Value, reason: &str) -> Value {
        let mut warnings = out["warnings"].as_array().into_iter().flatten();
        warnings.find(|w| w["reason"] == json!(reason)).cloned().unwrap_or_else(|| panic!("no {reason} warning: {out}"))
    }

    /// As ondas de uma resposta da rodada, num campo dela.
    pub(super) fn waves_in(out: &Value, field: &str) -> Vec<u64> {
        out[field].as_array().cloned().unwrap_or_default().iter().filter_map(|d| d["wave"].as_u64()).collect()
    }

    /// O pedido que a resposta `out` despachou para a onda `wave`, lido como
    /// o agente o lê: rodando o comando que a resposta traz no lugar do
    /// pedido. O comando lê o último envio gravado da onda, então a leitura
    /// vem logo depois da rodada que o gravou. Vazio quando a onda não saiu.
    pub(super) fn request_of(out: &Value, wave: u64) -> String {
        let found = out["dispatch"].as_array().into_iter().flatten().find(|d| d["wave"] == json!(wave));
        found.map(|entry| request_by_command(entry, out)).unwrap_or_default()
    }

    /// [`request_of`] da onda na posição `at` da lista despachada.
    pub(super) fn request_at(out: &Value, at: usize) -> String {
        request_by_command(&out["dispatch"][at], out)
    }

    /// O pedido da onda despachada em `at` da spec `spec`, seguido do bloco de
    /// cada tarefa que o envio guarda: tudo o que o agente da onda recebe,
    /// uma tarefa por vez.
    pub(super) fn request_and_blocks_at(root: &Path, spec: &str, out: &Value, at: usize) -> String {
        let mut text = request_at(out, at);
        let wave = out["dispatch"][at]["wave"].as_u64().unwrap_or_else(|| panic!("no wave dispatched at {at}: {out}"));
        let log = store::read(&store::spec_file(root, spec).unwrap()).unwrap().unwrap();
        let sent = log.last_by_wave("send")[&wave];
        for block in log.send_field(sent, "tasks").and_then(Value::as_array).into_iter().flatten() {
            text.push_str("\n\n");
            text.push_str(block["text"].as_str().unwrap_or_default());
        }
        text
    }

    /// Roda o comando `read` de um item despachado — `mustard-rt run read
    /// request-<n> --root <raiz> --spec <spec>` — pela mesma leitura do
    /// comando, e devolve o que ele imprime.
    fn request_by_command(entry: &Value, out: &Value) -> String {
        let command = entry["read"].as_str().unwrap_or_else(|| panic!("the dispatch carries no read command: {out}"));
        crate::commands::spec_events::read::read_by_command(command)
    }

    /// A versão nova da tarefa da onda `n`: o plano da onda muda depois do
    /// pedido dela, com a mesma origem da versão anterior.
    pub(super) fn replan(root: &Path, n: u64) {
        replan_from(root, n, None);
    }

    /// [`replan`] com a origem `origin`, quando ela é dada: a mensagem ou a
    /// decisão de onde a versão nova nasce.
    pub(super) fn replan_from(root: &Path, n: u64, origin: Option<u64>) {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(n)).unwrap_or_else(|| panic!("sem tarefa da onda {n}"));
        let mut fields = task.fields.clone();
        for key in ["v", "id", "code", "at", "search", "type", "author"] {
            fields.remove(key);
        }
        let mut body = Value::Object(fields);
        body["replaces"] = json!(task.id);
        body["text"] = json!(format!("Tarefa da onda {n}, revista."));
        if let Some(origin) = origin {
            body["origin"] = json!(origin);
        }
        id_of(&write(root, "x", "task", body));
    }

    /// Um pedido da onda `n` gravado sem passar pela rodada.
    pub(super) fn seed_send(root: &Path, n: u64) {
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "send",
            json!({"wave": n, "role": "wave",
            "text": "pedido", "lines": 1, "chars": 6, "items": [1], "mustard": "0", "author": "binary"}),
        );
    }

    /// O envio da onda `wave` da spec `x` de um Claude Code que fechou: a
    /// versão nova dele leva um processo que já acabou ([`closed_process`]).
    pub(super) fn orphan_the_send(root: &Path, wave: u64) {
        let (pid, started) = closed_process();
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let extra = json!({"claude_pid": pid, "claude_started": started}).as_object().cloned().unwrap();
        let draft = send_revision(&log, wave, extra).expect("the send of the wave");
        let at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        store::write_at(&path, "send", draft, &[], &at).unwrap();
    }

    /// Nenhum arquivo da rodada passa do teto de linhas de código: a porta e
    /// cada parte da pasta dela, pela medida única do núcleo.
    #[test]
    fn no_file_of_the_round_goes_over_the_code_line_cap() {
        let gate = crate::manifest_dir::manifest_dir().join("src").join("commands").join("flow").join("round.rs");
        assert_eq!(mustard_core::io::fs::files_over_code_line_cap(&gate), Ok(Vec::new()));
    }
    #[test]
    fn two_scoped_skills_are_hashed_and_the_native_send_is_accepted() {
        let dir=tempfile::tempdir().unwrap();let root=dir.path();
        approved(root,"x",&[(1,&["src/a.rs"],&[])]);
        for name in ["database","validation"] {
            let folder=root.join(format!(".claude/skills/{name}"));std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("SKILL.md"),format!("---\nname: {name}\ndescription: Check this project area\npaths: [\"src/a.rs\"]\n---\n\nInspect the existing implementation and test its behavior.\n")).unwrap();
        }
        let result=round(root,"x",None);
        assert_eq!(result["ok"],true,"{result}");
        let log=mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root,"x").unwrap()).unwrap().unwrap();
        let send=log.visible().into_iter().find(|event|event.event_type=="send").unwrap();
        let send=&mustard_core::domain::spec_events::SpecEvent {fields:log.whole_fields(send),..send.clone()};
        assert_eq!(send.fields["skills"].as_array().unwrap().len(),2);
        for skill in send.fields["skills"].as_array().unwrap() {
            let bytes=std::fs::read(skill["path"].as_str().unwrap()).unwrap();
            assert_eq!(skill["sha"],mustard_core::io::observations::digest(&bytes));
        }
        let again=round(root,"x",None);assert_eq!(again["ok"],true,"{again}");
        let log=mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root,"x").unwrap()).unwrap().unwrap();
        assert_eq!(log.visible().into_iter().filter(|event|event.event_type=="send").count(),1,"retry does not dispatch twice");
    }

}
