//! O relatório da rodada e as voltas dos agentes: a entrega que cada onda
//! grava na spec e o veredito que o revisor grava, lidos de lá, conferidos
//! antes de qualquer gravação, a entrega juntada da cópia de cada onda, e os
//! dois assumidos pela mesma porta das outras gravações, com o commit no
//! meio. O relatório que o orquestrador passa traz só as linhas dele: o
//! consumo, a pausa e a escolha antes do envio.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::scan::ScanReport;
use mustard_core::domain::spec_events::{Hidden, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, State};
use mustard_core::domain::wave_prompt as agreed_prompt;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::Scan;
use serde_json::{json, Map, Value};

use super::answer::RoundRefusal;
use super::commit::{
    close_copies, commit_draft, commit_message, ensure_builds, ensure_criteria_proofs, format_round_files, git_lock,
    head, join_copies, make_commit, real_changed_files, record_commit, refresh_map, round_repos, unknown_file,
    write_joined, UNMADE_SHA,
};
use super::leftovers::{leftover_task, leftovers_of, open_leftovers, Leftover, LeftoverKind};
use super::queue::{backlog_wave, open_review, open_sends, waves_in_progress, ANALYSIS_LINE};
use super::stops::{change_accepted, replan_code};
use crate::commands::spec_events::write::{record, RecordCheck};

/// A linha da entrega que o agente de onda devolvia colada no relatório: a
/// entrega mora na spec, e a linha que ainda chega é recusada.
const DELIVERED_LINE: &str = "DELIVERED";
/// A linha do veredito que o revisor devolvia colada no relatório: o
/// veredito mora na spec, e a linha que ainda chega é recusada.
const VERDICT_LINE: &str = "VERDICT";
/// A linha da pausa, do agente de onda ou do orquestrador em nome dele.
const PAUSED_LINE: &str = "PAUSED";
/// A linha do consumo de uma onda, que só o orquestrador escreve: a
/// plataforma entrega a ele o total de tokens e o número de idas e voltas do
/// agente quando este termina, e é ele quem os copia para esta linha ao
/// remandar a rodada. O molde do agente de onda nunca pede esse número, e a
/// entrega que o agente grava não é lida para isso: um valor que apareça lá,
/// digitado pelo agente, não vira consumo.
const USAGE_LINE: &str = "USAGE";

/// O consumo de uma onda, que só quem despacha sabe dizer: o modelo que o
/// agente usou de verdade, os passos que deu e os tokens que gastou, e o
/// consumo de quem despacha até esta rodada — a conversa do orquestrador, não
/// a da onda —, todos vindos da linha `USAGE`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Usage {
    pub model_used: Option<String>,
    pub steps: Option<u64>,
    pub tokens: Option<u64>,
    pub caller_steps: Option<u64>,
    pub caller_tokens: Option<u64>,
}

impl Usage {
    /// Os campos informados, com os nomes que o envio grava; vazio sem
    /// nenhum.
    fn fields(&self) -> Map<String, Value> {
        let mut out = Map::new();
        if let Some(model) = &self.model_used {
            out.insert("model_used".into(), json!(model));
        }
        for (key, value) in [
            ("steps", self.steps),
            ("tokens", self.tokens),
            ("caller_steps", self.caller_steps),
            ("caller_tokens", self.caller_tokens),
        ] {
            if let Some(value) = value {
                out.insert(key.into(), json!(value));
            }
        }
        out
    }
}

/// A volta de uma onda, como a rodada a assume.
pub(crate) struct WaveReport {
    pub wave: u64,
    pub delivered: String,
    pub files: Vec<String>,
    /// O resumo do commit, de onde a mensagem é montada.
    pub commit: Option<String>,
    /// As provas novas: o critério, pelo código ou pelo número, e o comando.
    pub proofs: Vec<(Value, String)>,
    /// As ondas que este conserto fecha.
    pub fixes: Vec<u64>,
    pub replan: Option<String>,
    /// As sobras, cada uma com o título, o detalhe e o que ela é: a rodada
    /// as grava quando assume a volta ([`Leftover`]).
    pub leftovers: Vec<Leftover>,
    /// Todas as voltas da onda desde o envio que a despachou, que a entrega
    /// oficial substitui.
    pub returns: Vec<u64>,
    /// O consumo da onda, da linha `USAGE`, nunca da volta que o agente grava.
    pub usage: Usage,
}

/// O veredito que o revisor gravou, como a rodada o assume: os campos dele,
/// com a onda à parte. O veredito final vem sem onda, a não ser o reprovado
/// que aponta a onda a refazer.
pub(crate) struct VerdictReport {
    pub wave: Option<u64>,
    pub fields: Map<String, Value>,
    /// Todas as voltas do revisor desde o pedido de revisão aberto, que o
    /// veredito oficial substitui.
    pub returns: Vec<u64>,
}

/// O que a rodada assume: as voltas gravadas na spec e as linhas do
/// relatório. O fechamento assume a última rodada pela mesma porta.
pub(crate) struct Report {
    pub waves: Vec<WaveReport>,
    pub verdicts: Vec<VerdictReport>,
    /// As ondas que pausaram, pela linha `PAUSED`: a rodada as reenvia com o
    /// mesmo pedido de antes, sem gravar entrega nem veredito nenhum.
    pub paused: Vec<u64>,
    /// As linhas `USAGE`, cada uma com a onda dela. Depois de casadas com as
    /// voltas, ficam só as das ondas que uma rodada anterior já assumiu: cada
    /// uma vira a versão nova do envio da onda.
    pub usage: Vec<(u64, Usage)>,
}

/// O que a rodada fez com um relatório.
pub(crate) struct Taken {
    /// O que foi gravado, na ordem.
    pub recorded: Vec<Value>,
    /// Os arquivos formatados.
    pub formatted: Vec<String>,
    /// Os avisos: formatador não achado, prova nova que não roda teste, cópia
    /// que ficou e a recusa da entrega segurada por conflito.
    pub warnings: Vec<Value>,
    /// O commit feito, quando houve arquivo entregue.
    pub commit: Option<Value>,
    /// As ondas que pausaram, pela linha `PAUSED`.
    pub paused: Vec<u64>,
}

/// Assume o que voltou de uma rodada: a volta que cada onda gravou na spec
/// desde o envio que a despachou e as linhas do texto `raw` que o
/// orquestrador passa. Confere tudo e junta a cópia de cada onda ao
/// repositório principal sem gravar nada, e só então grava a junção, formata
/// os arquivos da rodada e faz o commit; depois grava os vereditos, a entrega
/// oficial de cada onda, com `replaces` para as voltas dela, a versão nova de
/// cada critério com prova nova e o commit, a tarefa de cada sobra que
/// quebra e a pendência de cada sobra que não quebra, e apaga as cópias. O
/// git, que pode recusar, roda antes da primeira gravação na spec, e a
/// recusa dele devolve o disco e o índice do
/// repositório principal ao que eram: a chamada corrigida depois de uma
/// recusa junta e grava tudo uma vez só, e o commit de outra onda nunca leva
/// nada da recusada. A entrega que a junção segura por conflito fica de fora,
/// e a resposta traz a recusa dela; só quando não há mais nada a assumir a
/// recusa é a resposta. A rodada e o fechamento assumem por aqui.
pub(crate) fn take_report(
    start: &Path,
    root: &Path,
    spec: &str,
    raw: Option<&str>,
    log: &SpecLog,
    lang: Locale,
) -> Result<Taken, RoundRefusal> {
    take_report_with_mine(start, root, spec, raw, log, lang, &|root, out| Scan::locate().scan(root, out))
}

/// [`take_report`] com quem relê o mapa depois do commit (`mine`), que um
/// teste escolhe sem instalar a ferramenta do scan de verdade.
pub(crate) fn take_report_with_mine(
    start: &Path,
    root: &Path,
    spec: &str,
    raw: Option<&str>,
    log: &SpecLog,
    lang: Locale,
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>,
) -> Result<Taken, RoundRefusal> {
    let mut report = parse_report(raw.unwrap_or_default())?;
    let nothing = |report: &Report| report.waves.is_empty() && report.verdicts.is_empty() && report.usage.is_empty();
    report.waves = returned_waves(log)?;
    report.verdicts = returned_verdict(log);
    // Sem volta, sem veredito e sem consumo, não há o que juntar nem comitar,
    // e a rodada não espera a trava: a pausa reenvia essas ondas com o pedido
    // de antes, mais adiante, em [`super::answer::run_round_with_mine`].
    if nothing(&report) {
        return Ok(Taken::only_paused(report.paused));
    }
    // A volta mora na spec, e duas rodadas ao mesmo tempo leem a mesma: a
    // trava do passo do git é presa antes de escolher as voltas, e a spec é
    // lida de novo sob ela. A rodada que chega depois de outra já ter
    // assumido uma volta não a vê mais, e nunca junta, comita ou grava a
    // mesma entrega duas vezes.
    let held_lock = git_lock(root)?;
    let path = store::spec_file(root, spec).map_err(RoundRefusal::Refused)?;
    let fresh = store::read(&path).map_err(RoundRefusal::Refused)?.unwrap_or_else(|| log.clone());
    report.waves = returned_waves(&fresh)?;
    report.verdicts = returned_verdict(&fresh);
    let cut = match_usage(&fresh, &mut report)?;
    if nothing(&report) {
        // A onda de lote cortada não entra no commit: as tarefas dela voltam
        // ao backlog soltas, sem a onda que as levou.
        let mut taken = Taken::only_paused(report.paused);
        taken.recorded = return_cut_batches(start, spec, &fresh, &cut).map_err(RoundRefusal::Refused)?;
        return Ok(taken);
    }
    take_returns(start, root, spec, report, &fresh, lang, mine, held_lock, &cut)
}

impl Taken {
    /// Nada assumido: só as ondas que pausaram, que a rodada reenvia.
    fn only_paused(paused: Vec<u64>) -> Self {
        Taken { recorded: Vec::new(), formatted: Vec::new(), warnings: Vec::new(), commit: None, paused }
    }
}

/// O corpo de [`take_report_with_mine`], com a trava do passo do git já
/// presa, as voltas lidas sob ela, o consumo casado com elas e as ondas de
/// lote cortadas (`cut`).
#[allow(clippy::too_many_arguments)]
fn take_returns(
    start: &Path,
    root: &Path,
    spec: &str,
    mut report: Report,
    log: &SpecLog,
    lang: Locale,
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>,
    held_lock: LockedFile,
    cut: &[u64],
) -> Result<Taken, RoundRefusal> {
    // O agente que diz que o plano da onda não funciona para a rodada: a
    // mudança proposta é mostrada, e só o clique do usuário em "Aceitar",
    // gravado pela testemunha, a deixa seguir.
    for wave in &report.waves {
        if let Some(change) = &wave.replan {
            let code = replan_code(wave.wave, change);
            if !change_accepted(log, wave.wave, &code) {
                return Err(RoundRefusal::Replan { wave: wave.wave, change: change.clone(), code });
            }
        }
    }
    // Da leitura das voltas e do repositório à junção, ao commit e ao desfazer
    // quando o git recusa, a trava do passo do git fica presa, uma vez: outra
    // rodada ao mesmo tempo no mesmo checkout espera, e nunca junta sobre o
    // que esta ainda não comitou nem põe a mudança dela no commit desta. A
    // trava solta antes de as cópias serem apagadas, que a pegam de novo.
    // A lista que a entrega cita vira só conferência: o que entra no commit é
    // o que a cópia da onda mudou de fato, pelo `git status` dela — inclusive
    // o arquivo que a entrega não citou. A divergência entre as duas vira
    // aviso, com quantos arquivos mudaram, quantos a onda citou e quais
    // ficaram de fora da citação.
    let mut warnings: Vec<Value> = Vec::new();
    for wave in &mut report.waves {
        let Some(actual) = real_changed_files(root, log, wave.wave) else { continue };
        // Cópia sem diff nenhum (comum nos testes, que escrevem direto na
        // raiz do checkout em vez da cópia da onda) não conta como
        // divergência nem apaga a lista declarada: sem nada de real para
        // comparar, a conferência não tem o que dizer. É também a cópia da
        // onda que só foi conferir: sem arquivo mudado, não há o que comitar.
        if actual.is_empty() {
            continue;
        }
        // A cópia mudou arquivo de verdade: a entrega precisa do resumo do
        // commit, mesmo tendo voltado sem citar arquivo nenhum. É a única
        // conferência da volta que depende da cópia, e por isso fica aqui, e
        // não na gravação: o que a onda mexeu nunca entra no repositório
        // principal sem título de commit, e a resposta pede que o agente
        // grave a entrega de novo.
        if wave.commit.is_none() {
            return Err(RoundRefusal::ReturnNeedsCommit { wave: wave.wave });
        }
        let declared: BTreeSet<&str> = wave.files.iter().map(String::as_str).collect();
        let actual_set: BTreeSet<&str> = actual.iter().map(String::as_str).collect();
        if declared != actual_set {
            let left_out: Vec<String> = actual_set.difference(&declared).map(|s| (*s).to_string()).collect();
            warnings.push(json!({
                "reason": "files-diverged",
                "wave": wave.wave,
                "hint": translate("round.files_diverged", lang)
                    .replace("{wave}", &wave.wave.to_string())
                    .replace("{changed}", &actual.len().to_string())
                    .replace("{declared}", &declared.len().to_string())
                    .replace("{missing}", &left_out.join(", ")),
            }));
        }
        wave.files = actual;
    }
    unknown_file(root, log, &report.waves)?;
    // A junção de cada cópia é decidida antes de qualquer gravação. A entrega
    // com um trecho que ela não resolve fica de fora, com o repositório
    // principal intacto para ela, e o resto do relatório segue.
    let (joined, mut held) = join_copies(root, log, &report.waves)?;
    report.waves.retain(|wave| held.iter().all(|one| one.wave != wave.wave));
    if report.waves.is_empty() && report.verdicts.is_empty() && !held.is_empty() {
        return Err(held.remove(0).refusal(head(root)));
    }
    // A mensagem do commit é montada e conferida junto das outras travas,
    // antes de qualquer gravação: recusá-la depois de gravar o entregou e o
    // veredito faria a chamada seguinte, com a mensagem corrigida, duplicar os
    // dois.
    let message = commit_message(&report.waves, lang)?;
    let mut files: Vec<String> = Vec::new();
    for file in report.waves.iter().flat_map(|w| w.files.iter()) {
        if !files.contains(file) {
            files.push(file.clone());
        }
    }
    let mut waves: Vec<u64> = Vec::new();
    for n in report.waves.iter().flat_map(|w| std::iter::once(w.wave).chain(w.fixes.iter().copied())) {
        if !waves.contains(&n) {
            waves.push(n);
        }
    }
    // Toda gravação que vem depois do git passa antes pela mesma conferência
    // da gravação, contra a spec: a recusa que viesse depois do commit
    // deixaria o commit feito e a chamada corrigida sem nada a comitar. O
    // commit de cada submódulo é gravado como o do principal.
    let repos = round_repos(root, &files);
    let planned: Vec<Map<String, Value>> = match &message {
        Some((title, _)) => std::iter::once(commit_draft(root, UNMADE_SHA, title, &waves, &files))
            .chain(repos.subs.iter().map(|(sub, own)| commit_draft(&root.join(sub), UNMADE_SHA, title, &waves, own)))
            .collect(),
        None => Vec::new(),
    };
    let checked = check_reports(start, root, spec, &report, planned).map_err(RoundRefusal::Refused)?;

    if let Err(refused) = write_joined(root, &joined, true) {
        let _ = write_joined(root, &joined, false);
        return Err(refused);
    }
    // A formatação roda uma vez por rodada, só nos arquivos da rodada.
    let outcome = format_round_files(root, &files);
    for name in outcome.missing {
        warnings.push(json!({
            "reason": "formatter-not-found",
            "hint": translate("round.formatter_missing", lang).replace("{name}", &name),
        }));
    }
    // O repositório principal compila antes do commit, com o mesmo comando
    // que o pedido de cada onda já ensina: não compilou, o disco volta ao que
    // era e nada é comitado.
    if message.is_some()
        && let Err(refusal) = ensure_builds(root)
    {
        let _ = write_joined(root, &joined, false);
        return Err(refusal);
    }
    // A prova de cada critério que as ondas deste relatório cobrem roda antes
    // do commit, uma de cada vez: a que não executa ou não passa recusa com o
    // código do critério, o comando inteiro e a saída de erro, e nada é
    // comitado.
    if message.is_some()
        && let Err(refusal) = ensure_criteria_proofs(root, log, &waves)
    {
        let _ = write_joined(root, &joined, false);
        return Err(refusal);
    }
    // A recusa do git volta o índice e o disco antes de sair, com a trava ainda
    // presa.
    let unit = State::from_log(log).branch.unwrap_or_default();
    let made = match message {
        Some((title, body)) => Some((make_commit(root, &held_lock, &unit, (&title, &body), &repos, &joined)?, title)),
        None => None,
    };
    // A entrega segurada se resolve no commit que já leva as outras.
    let now = head(root);
    for one in held {
        let wave = one.wave;
        let refusal = one.refusal(now.clone());
        warnings.push(json!({ "reason": refusal.reason(), "wave": wave, "hint": refusal.message(lang) }));
    }
    let (mut recorded, proofs) = record_reports(start, spec, checked).map_err(RoundRefusal::Refused)?;
    let commit = match made {
        Some((made, title)) => {
            let mut commit = record_commit(start, root, spec, &made.sha, &title, &waves, &files)?;
            let mut subs: Vec<Value> = Vec::new();
            for (sub, sha) in &made.subs {
                let own = repos.subs.get(sub).map(Vec::as_slice).unwrap_or_default();
                record_commit(start, &root.join(sub), spec, sha, &title, &waves, own)?;
                subs.push(json!({ "path": sub, "sha": sha }));
            }
            if !subs.is_empty() {
                commit["submodules"] = json!(subs);
            }
            Some(commit)
        }
        None => None,
    };
    // A onda de lote cortada não entra no commit: as tarefas dela voltam ao
    // backlog soltas, sem a onda que as levou, ainda sob a trava.
    recorded.extend(return_cut_batches(start, spec, log, cut).map_err(RoundRefusal::Refused)?);
    drop(held_lock);
    // Cada sobra das voltas assumidas que não quebra nada vira pendência, com
    // a entrega oficial já gravada: a cosmética passa ao projeto, e a sem
    // `kind` fica da spec, com a pergunta de destino.
    let (opened, not_opened) = open_leftovers(start, spec, &report.waves, lang);
    recorded.extend(opened);
    warnings.extend(not_opened);
    // O mapa acompanha o commit, antes de a onda seguinte pedir a sugestão de
    // skill e de arquivos parecidos: sem isso, ela apontaria o que este
    // commit acabou de apagar.
    if commit.is_some() {
        refresh_map(root, mine);
    }
    warnings.extend(close_copies(root, log, &report.waves, lang));
    // A linha de consumo é opcional na forma, e nunca em silêncio: sem ela o
    // envio da onda não ganha versão nova, e a página mostra um gasto menor
    // que o real. A entrega fica gravada do mesmo jeito — o consumo não é
    // dela, e recusá-la devolveria o trabalho de uma onda inteira por uma
    // linha que quem despacha escreve —, e a resposta avisa, nomeando a onda.
    for wave in &report.waves {
        if wave.usage.fields().is_empty() {
            warnings.push(json!({
                "reason": "usage-missing",
                "wave": wave.wave,
                "hint": translate("round.usage_missing", lang).replace("{wave}", &wave.wave.to_string()),
            }));
        }
    }
    // A prova nova roda uma vez: a que sai verde sem rodar teste nenhum é
    // avisada agora, antes de o fechamento recusá-la.
    for (code, proof) in proofs {
        if crate::commands::review::qa_run::run_proof(&proof, root).ran_no_test.is_some() {
            warnings.push(json!({
                "reason": "proof-ran-no-test",
                "hint": translate("round.proof_ran_no_test", lang).replace("{code}", &code),
            }));
        }
    }
    Ok(Taken { recorded, formatted: outcome.formatted, warnings, commit, paused: report.paused })
}

/// O envio que despachou a onda `wave` por último: o mais novo dela que não é
/// versão de outro — a versão que só acrescenta o consumo não despacha nada.
/// As voltas da onda contam dele em diante.
fn dispatched_at(log: &SpecLog, wave: u64) -> Option<u64> {
    log.events
        .iter()
        .filter(|e| e.event_type == "send" && e.wave() == Some(wave) && !e.fields.contains_key("replaces"))
        .map(|e| e.id)
        .max()
}

/// As voltas que a rodada assume agora, uma por onda: a última entrega que a
/// onda gravou depois do envio que a despachou e que ninguém assumiu ainda,
/// com todas as voltas dela desde esse envio. A volta de antes do envio —
/// de um envio já superado por um reenvio — não conta.
fn returned_waves(log: &SpecLog) -> Result<Vec<WaveReport>, RoundRefusal> {
    let hidden = log.hidden();
    let mut waves = Vec::new();
    for last in log.unassumed_returns().into_iter().filter(|e| e.event_type == "delivered") {
        let Some(wave) = last.wave() else { continue };
        let since = dispatched_at(log, wave).unwrap_or_default();
        if last.id <= since {
            continue;
        }
        let mut report = wave_report_of(log, &last.fields)?;
        report.returns = log
            .events
            .iter()
            .filter(|e| e.event_type == "delivered" && e.wave() == Some(wave) && e.id > since)
            .filter(|e| hidden.get(&e.id) == Some(&Hidden::Returned))
            .map(|e| e.id)
            .collect();
        waves.push(report);
    }
    Ok(waves)
}

/// O veredito que a rodada ou o fechamento assume agora: a última volta que o
/// revisor gravou depois do pedido de revisão aberto — vale a última —, com
/// todas as voltas dele desde esse pedido. Sem pedido aberto, nada espera.
fn returned_verdict(log: &SpecLog) -> Vec<VerdictReport> {
    let Some(asked) = open_review(log) else { return Vec::new() };
    let hidden = log.hidden();
    let returns: Vec<&SpecEvent> = log
        .events
        .iter()
        .filter(|e| e.event_type == "verdict" && e.id > asked && hidden.get(&e.id) == Some(&Hidden::Returned))
        .collect();
    let Some(last) = returns.last() else { return Vec::new() };
    let skipped = ["v", "id", "code", "at", "type", "search", "author", "returned", "wave"];
    let fields = last.fields.iter().filter(|(key, _)| !skipped.contains(&key.as_str()));
    let fields = fields.map(|(key, value)| (key.clone(), value.clone())).collect();
    vec![VerdictReport { wave: last.wave(), fields, returns: returns.iter().map(|e| e.id).collect() }]
}

/// A volta gravada pela onda, lida como a rodada a assume: a onda, o texto,
/// os arquivos — o caminho absoluto de dentro da cópia da onda vira o
/// relativo ao repositório —, o resumo do commit, as provas, as ondas que o
/// conserto fecha, a mudança de plano e as sobras. Arquivo entregue pede o
/// resumo do commit, a não ser na mudança de plano, e o resumo nunca tem cara
/// de código de commit. O `kind` de uma sobra, quando vem, é `breaks` ou
/// `cosmetic`; outro valor é recusado.
fn wave_report_of(log: &SpecLog, fields: &Map<String, Value>) -> Result<WaveReport, RoundRefusal> {
    let text = |key: &str| {
        fields.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
    };
    let missing =
        |field: &str| RoundRefusal::Refused(Refusal::MissingField { event_type: "delivered".into(), field: field.into() });
    let listed = |key: &str| fields.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let wave = fields.get("wave").and_then(Value::as_u64).ok_or_else(|| missing("wave"))?;
    let delivered = text("text").ok_or_else(|| missing("text"))?;
    // Sem `files`, a entrega volta sem arquivo nenhum: é a onda que só foi
    // conferir, e o texto dela diz o que conferiu. Quem confere se mexeu em
    // arquivo mesmo assim é a rodada, contra a cópia da onda.
    let files: Vec<String> = listed("files")
        .iter()
        .filter_map(Value::as_str)
        .map(|f| f.trim().replace('\\', "/"))
        .filter(|f| !f.is_empty())
        .map(|f| own_copy_relative(log, wave, &f))
        .collect();
    let (replan, commit) = (text("replan"), text("commit"));
    if commit.is_none() && !files.is_empty() && replan.is_none() {
        return Err(missing("commit"));
    }
    // O campo `commit` é o título em palavras, nunca o código do commit: a
    // rodada não depende da boa vontade do agente para não comitar dentro da
    // cópia e devolver o código dele aqui.
    if let Some(summary) = &commit
        && looks_like_commit_sha(summary)
    {
        return Err(RoundRefusal::CommitLooksLikeSha { found: summary.clone() });
    }
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(|t| t.trim().to_string());
    let proofs = listed("proofs")
        .iter()
        .filter_map(|p| Some((p.get("criterion").filter(|c| !c.is_null())?.clone(), field(p, "proof")?)))
        .collect();
    let fixes = listed("fixes").iter().filter_map(Value::as_u64).filter(|n| *n != wave).collect();
    let leftovers = leftovers_of(&listed("leftovers")).map_err(RoundRefusal::Refused)?;
    Ok(WaveReport {
        wave,
        delivered,
        files,
        commit,
        proofs,
        fixes,
        replan,
        leftovers,
        returns: Vec::new(),
        usage: Usage::default(),
    })
}

/// As conferências da volta que só leem a própria volta, feitas na gravação
/// dela (`run write delivered`), antes de gravar: há envio aberto para a
/// onda; a volta se lê como a rodada a assumirá; o título do commit que sairá
/// do resumo cabe no teto e não traz o que nunca leva; cada arquivo entregue
/// está no disco ou no git; cada prova aponta um critério da spec e é uma
/// linha de comando. Passando, a volta ganha `returned` e o autor da onda, e
/// os arquivos ficam relativos ao repositório. A conferência que depende de
/// juntar a cópia fica na rodada. Sem o número da onda, nada aqui é
/// conferido: a gravação recusa pelo campo que falta.
///
/// Devolve a trava do passo do git, a mesma que a rodada segura da leitura
/// das voltas até a entrega oficial, presa antes de ler a spec: quem grava a
/// segura até a volta estar no arquivo. A volta que chega enquanto a rodada
/// assume a mesma onda espera por ela e é conferida depois, com o envio já
/// fechado pela entrega oficial, e é recusada; a que entra antes a rodada lê
/// e assume. Nenhuma fica depois da entrega oficial para a rodada seguinte
/// assumir de novo.
pub(crate) fn check_return(
    start: &Path,
    spec: &str,
    draft: &mut Map<String, Value>,
) -> Result<LockedFile, RoundRefusal> {
    let held = git_lock(&crate::commands::spec_events::project(start).root)?;
    let Some(wave) = draft.get("wave").and_then(Value::as_u64) else { return Ok(held) };
    let (project, log) = spec_log(start, spec)?;
    if !open_sends(&log).contains_key(&wave) {
        return Err(RoundRefusal::Refused(Refusal::NoOpenSend { wave }));
    }
    let mut report = wave_report_of(&log, draft)?;
    // O título sai como a rodada o monta para esta onda sozinha — com mais de
    // uma onda no commit, ela encurta o escopo, e o título nunca cresce. A
    // volta que não cita arquivo também tem o título conferido: a cópia pode
    // ter mudado arquivo, e aí a rodada comita com ele.
    let cited = report.files.clone();
    if report.files.is_empty() {
        report.files.push(String::from("."));
    }
    commit_message(std::slice::from_ref(&report), project.lang)?;
    report.files = cited;
    unknown_file(&project.root, &log, std::slice::from_ref(&report))?;
    for (reference, proof) in &report.proofs {
        let id = criterion_id(&log, reference).map_err(RoundRefusal::Refused)?;
        let code = log.codes().get(&id).cloned().unwrap_or_else(|| id.to_string());
        agreed_prompt::proof_rule(&code, proof).map_err(RoundRefusal::Refused)?;
    }
    if draft.contains_key("files") {
        draft.insert("files".into(), json!(report.files));
    }
    draft.insert("returned".into(), json!(true));
    draft.insert("author".into(), json!("wave"));
    Ok(held)
}

/// As conferências do veredito que o revisor grava (`run write verdict`),
/// feitas antes de gravar, como as da entrega: há pedido de revisão aberto;
/// cada critério e cada item do combinado citado existe; o veredito final
/// responde por todo o combinado vigente. Passando, o veredito ganha
/// `returned` e o autor da revisão, e fica como o revisor o escreveu: a
/// rodada o resolve de novo ao assumi-lo.
pub(crate) fn check_verdict_return(start: &Path, spec: &str, draft: &mut Map<String, Value>) -> Result<(), RoundRefusal> {
    let (_, log) = spec_log(start, spec)?;
    if open_review(&log).is_none() {
        return Err(RoundRefusal::NoOpenReview);
    }
    settle_verdict(&log, &mut draft.clone()).map_err(RoundRefusal::Refused)?;
    draft.insert("returned".into(), json!(true));
    draft.insert("author".into(), json!("review"));
    Ok(())
}

/// O veredito como a rodada o grava: cada critério citado pelo número dele e,
/// no veredito final, cada item do combinado também. A revisão final responde
/// por todo o combinado vigente, item a item, em `agreed`: faltar algum, ou a
/// lista inteira, é veredito malformado, e nada é gravado. O item que vem
/// `met:false` força o resultado a reprovado e vira uma tarefa nova no
/// backlog, cobrindo esse item: são essas tarefas que a função devolve.
fn settle_verdict(log: &SpecLog, draft: &mut Map<String, Value>) -> Result<Vec<Map<String, Value>>, Refusal> {
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
    let (mut answered, mut tasks) = (Vec::new(), Vec::new());
    for item in draft.get_mut("agreed").and_then(Value::as_array_mut).into_iter().flatten() {
        let id = agreed_item_id(log, item.get("item").unwrap_or(&Value::Null))?;
        item["item"] = json!(id);
        answered.push(id);
        if item.get("met").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let text = item.get("text").and_then(Value::as_str).map(str::trim).unwrap_or_default();
        let files = item.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
        let files: Vec<Value> = files.map(|f| json!({ "path": f })).collect();
        let task = json!({ "text": text, "files": files, "depends_on": [], "covers": [id], "author": "review" });
        tasks.extend(task.as_object().cloned());
    }
    let codes = log.codes();
    let missing: Vec<String> = agreed_prompt::all_agreed(log)
        .iter()
        .filter(|item| !answered.contains(&item.id))
        .map(|item| codes.get(&item.id).cloned().unwrap_or_else(|| item.id.to_string()))
        .collect();
    if !missing.is_empty() {
        return Err(Refusal::AgreedItemsMissing { missing });
    }
    if !tasks.is_empty() {
        draft.insert("result".into(), json!("rejected"));
    }
    Ok(tasks)
}

/// O `replaces` do evento oficial que assume as voltas `returns`: o número
/// da única, a lista de todas, ou nada sem volta nenhuma.
fn replaced(returns: &[u64]) -> Option<Value> {
    match returns {
        [] => None,
        [only] => Some(json!(only)),
        _ => Some(json!(returns)),
    }
}

/// O projeto de `start` e o arquivo de eventos da spec `spec`, que a
/// gravação de uma volta confere.
fn spec_log(start: &Path, spec: &str) -> Result<(crate::commands::spec_events::Project, SpecLog), RoundRefusal> {
    let project = crate::commands::spec_events::project(start);
    let path = store::spec_file(&project.root, spec).map_err(RoundRefusal::Refused)?;
    let log = store::read(&path)
        .map_err(RoundRefusal::Refused)?
        .ok_or_else(|| RoundRefusal::Refused(Refusal::NoSpecFile { spec: spec.to_string() }))?;
    Ok((project, log))
}

/// Casa cada linha `USAGE` com a onda dela. A onda com volta nesta rodada
/// recebe o consumo; a que uma rodada anterior já assumiu fica com a linha,
/// que vira a versão nova do envio dela. A onda com envio aberto e sem volta
/// não entra no commit: com o Claude Code dela aberto, a rodada recusa e pede
/// que o agente grave a entrega; com ele fechado, a onda de lote está
/// cortada, e o número dela sai na lista devolvida. A linha de uma onda sem
/// entrega nenhuma não se entende.
fn match_usage(log: &SpecLog, report: &mut Report) -> Result<Vec<u64>, RoundRefusal> {
    let open = open_sends(log);
    let alive = waves_in_progress(log);
    let delivered = log.last_by_wave("delivered");
    let mut cut = Vec::new();
    let mut assumed = Vec::new();
    for (wave, usage) in std::mem::take(&mut report.usage) {
        if let Some(target) = report.waves.iter_mut().find(|w| w.wave == wave) {
            target.usage = usage;
        } else if alive.contains_key(&wave) {
            return Err(RoundRefusal::ReturnMissing { wave });
        } else if open.contains_key(&wave) {
            if backlog_wave(log, wave) {
                cut.push(wave);
            }
        } else if delivered.contains_key(&wave) {
            assumed.push((wave, usage));
        } else {
            return Err(RoundRefusal::BadReport { detail: format!("{USAGE_LINE}: onda {wave} sem entrega gravada") });
        }
    }
    report.usage = assumed;
    Ok(cut)
}

/// Os trechos entre `<tag>` e `</tag>` de `raw`, na ordem.
pub(super) fn tagged<'a>(raw: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        let Some(end) = after.find(&close) else { break };
        out.push(after[..end].trim());
        rest = &after[end + close.len()..];
    }
    out
}

/// O objeto JSON de uma linha, com a onda dela, quando ela traz uma.
fn line_object(body: &str, line: &'static str) -> Result<(Option<u64>, Map<String, Value>), RoundRefusal> {
    let parsed: Value =
        serde_json::from_str(body).map_err(|e| RoundRefusal::BadReport { detail: format!("{line}: {e}") })?;
    let Value::Object(fields) = parsed else {
        return Err(RoundRefusal::BadReport { detail: format!("{line}: {body}") });
    };
    Ok((fields.get("wave").and_then(Value::as_u64), fields))
}

/// As linhas do relatório que o orquestrador passa: o consumo (`USAGE`) e a
/// pausa (`PAUSED`), como vieram; a escolha antes do envio (`ANALYSIS`) é
/// lida à parte, no despacho. A entrega e o veredito não vêm aqui: moram na
/// spec, e a linha `DELIVERED` ou `VERDICT` colada no relatório é recusada. O
/// texto sem nenhuma dessas linhas não se entende. O resto do texto não é
/// lido.
pub(crate) fn parse_report(raw: &str) -> Result<Report, RoundRefusal> {
    if !tagged(raw, DELIVERED_LINE).is_empty() || !tagged(raw, VERDICT_LINE).is_empty() {
        return Err(RoundRefusal::ReturnLine);
    }
    let usage_bodies = tagged(raw, USAGE_LINE);
    let paused_bodies = tagged(raw, PAUSED_LINE);
    let unmarked = usage_bodies.is_empty() && paused_bodies.is_empty();
    if unmarked && !raw.trim().is_empty() && tagged(raw, ANALYSIS_LINE).is_empty() {
        let shown: String = raw.trim().chars().take(80).collect();
        return Err(RoundRefusal::BadReport { detail: shown });
    }
    // O consumo, que só quem despacha sabe: o modelo que a onda usou de
    // verdade, os passos e os tokens dela, e os do orquestrador até esta
    // rodada. Vêm só da linha `USAGE`, que o agente de onda nunca escreve.
    let mut usage = Vec::new();
    for body in usage_bodies {
        let (wave, fields) = line_object(body, USAGE_LINE)?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: USAGE_LINE, field: "wave" })?;
        let as_u64 = |key: &str| fields.get(key).and_then(Value::as_u64);
        let model_used =
            fields.get("model").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
        usage.push((
            wave,
            Usage {
                model_used,
                steps: as_u64("steps"),
                tokens: as_u64("tokens"),
                caller_steps: as_u64("caller_steps"),
                caller_tokens: as_u64("caller_tokens"),
            },
        ));
    }
    let mut paused = Vec::new();
    for body in paused_bodies {
        let (wave, _) = line_object(body, PAUSED_LINE)?;
        let wave = wave.ok_or(RoundRefusal::LineField { line: PAUSED_LINE, field: "wave" })?;
        paused.push(wave);
    }
    Ok(Report { waves: Vec::new(), verdicts: Vec::new(), paused, usage })
}

/// O texto `summary` tem cara de código de commit: só dígito hexadecimal, do
/// tamanho de um SHA curto (o `git rev-parse --short` mais comum) ao inteiro
/// de quarenta caracteres. O título em palavras que o campo `commit` pede
/// nunca cai nessa faixa — um resumo curto e em português ou inglês sempre
/// traz espaço ou letra fora do alfabeto hexadecimal.
fn looks_like_commit_sha(summary: &str) -> bool {
    let text = summary.trim();
    (7..=40).contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// A versão nova da tarefa `task`, devolvida ao backlog: os mesmos campos dela,
/// tirando a onda que a levou — sem `wave`, ela volta a nascer solta, pronta
/// para o lote que o backlog formar na rodada seguinte.
pub(super) fn backlog_return(task: &SpecEvent) -> Map<String, Value> {
    let mut draft: Map<String, Value> = task
        .fields
        .iter()
        .filter(|(key, _)| !["v", "id", "code", "at", "type", "search", "wave"].contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    draft.insert("replaces".into(), json!(task.id));
    draft
}

/// As tarefas das ondas de lote cortadas `cut` — envio aberto, nenhuma volta
/// e o Claude Code delas já fechado: cada uma ganha uma versão sem a onda que
/// a levou, e volta ao backlog.
fn return_cut_batches(start: &Path, spec: &str, log: &SpecLog, cut: &[u64]) -> Result<Vec<Value>, Refusal> {
    let mut recorded = Vec::new();
    for wave in cut {
        for task in log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(*wave)) {
            let written = record(start, spec, "task", backlog_return(task), PhaseWriter::Binary)?;
            recorded.push(json!({ "wave": wave, "type": "task", "id": written.written.id }));
        }
    }
    Ok(recorded)
}

/// O número do evento que `reference` aponta, dado pelo código que a página
/// mostra ou pelo número, na versão gravada — sem conferir ainda se ele
/// segue vigente nem de que tipo é.
fn reference_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let unknown = || Refusal::UnknownTarget {
        target: mustard_core::domain::spec_events::EventRef::from_value(reference)
            .unwrap_or(mustard_core::domain::spec_events::EventRef::Code(reference.to_string())),
    };
    let codes = log.codes();
    match reference {
        Value::Number(n) => n.as_u64().ok_or_else(unknown),
        Value::String(code) => {
            let code = code.trim();
            match code.parse::<u64>() {
                Ok(n) => Ok(n),
                Err(_) => codes.iter().filter(|(_, c)| c.as_str() == code).map(|(id, _)| *id).max().ok_or_else(unknown),
            }
        }
        _ => Err(unknown()),
    }
}

/// O número do critério `reference`, dado pelo código que a página mostra ou
/// pelo número, na versão mais nova. Um critério que a spec não tem é
/// recusado.
fn criterion_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let unknown = || Refusal::UnknownTarget {
        target: mustard_core::domain::spec_events::EventRef::from_value(reference)
            .unwrap_or(mustard_core::domain::spec_events::EventRef::Code(reference.to_string())),
    };
    let id = reference_id(log, reference)?;
    let current = log.current(id).filter(|e| e.event_type == "criterion").ok_or_else(unknown)?;
    Ok(current.id)
}

/// O número vigente do item combinado `reference`, dado pelo código que a
/// página mostra ou pelo número: ao contrário de [`criterion_id`], vale para
/// qualquer tipo do bloco combinado (decisão, regra, contrato...). Um item
/// que a spec não tem, ou que saiu da leitura, é recusado.
fn agreed_item_id(log: &SpecLog, reference: &Value) -> Result<u64, Refusal> {
    let unknown = || Refusal::UnknownTarget {
        target: mustard_core::domain::spec_events::EventRef::from_value(reference)
            .unwrap_or(mustard_core::domain::spec_events::EventRef::Code(reference.to_string())),
    };
    let id = reference_id(log, reference)?;
    let current = log.current(id).ok_or_else(unknown)?;
    Ok(current.id)
}

/// O que [`record_reports`] devolve: o que foi gravado e, de cada prova nova,
/// o código do critério e o comando.
type RecordedReport = (Vec<Value>, Vec<(String, String)>);

/// O que [`check_reports`] conferiu e [`record_reports`] grava: cada veredito
/// e cada entregou já montado, com a onda, o número de cada critério com
/// prova nova, com o comando, e a versão nova de cada envio com consumo
/// informado na entrega.
struct CheckedReport {
    /// A onda de cada veredito, quando ele aponta uma: a aprovação do agente
    /// de teste dedicado, na obra sem onda nenhuma, não aponta.
    verdicts: Vec<(Option<u64>, Map<String, Value>)>,
    deliveries: Vec<(u64, Map<String, Value>)>,
    proofs: Vec<(u64, String)>,
    /// A versão nova do envio de cada onda cuja entrega trouxe o modelo
    /// usado, os passos, os tokens ou o consumo de quem despacha.
    sends: Vec<(u64, Map<String, Value>)>,
    /// Uma tarefa nova no backlog por item combinado que a revisão final
    /// marcou `met:false`: sem onda, para a rodada seguinte formar o lote.
    agreed_tasks: Vec<Map<String, Value>>,
    /// Uma tarefa nova no backlog por sobra que quebra, com a onda que a
    /// apontou: também sem onda própria, para a rodada seguinte formar o lote.
    leftover_tasks: Vec<(u64, Map<String, Value>)>,
}

/// O caminho como a rodada grava `file` da onda `wave`: quando é o caminho
/// absoluto que começa pela cópia gravada no envio da onda, o caminho
/// relativo ao repositório dentro dela; o resto — o caminho já relativo, um
/// caminho absoluto de fora dessa cópia, ou de uma cópia vizinha cujo nome só
/// começa igual — fica como veio, e segue pelo mesmo crivo do git mais
/// adiante. Vale o caminho gravado, não o que a pasta das cópias daria hoje:
/// a onda enviada antes de a pasta mudar volta da cópia onde nasceu.
pub(super) fn own_copy_relative(log: &SpecLog, wave: u64, file: &str) -> String {
    let Some(copy) = wave_prompt::recorded_copy(log, wave) else { return file.to_string() };
    file.strip_prefix(copy.path.as_str())
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))
        .map(|rest| rest.trim_start_matches('/').to_string())
        .unwrap_or_else(|| file.to_string())
}

/// Monta o que voltou e passa cada gravação que virá — cada veredito, cada
/// entregou, a versão nova de cada critério com prova nova e cada commit de
/// `commits`, na ordem em que serão gravados — pela conferência inteira da
/// gravação, contra a spec, sem gravar nada: a linha sem campo obrigatório
/// nunca deixa gravada a que veio antes dela, e nada é recusado depois do
/// commit. O entregou vai também em cada onda que o conserto fecha, e a
/// sobra que quebra vira tarefa no backlog, pela mesma conferência das
/// tarefas que nascem do veredito.
fn check_reports(
    start: &Path,
    root: &Path,
    spec: &str,
    report: &Report,
    commits: Vec<Map<String, Value>>,
) -> Result<CheckedReport, Refusal> {
    let mut check = RecordCheck::open(start, spec, PhaseWriter::Binary)?;
    // Os critérios citados existem, antes de qualquer gravação.
    let mut verdicts = Vec::new();
    let mut agreed_tasks: Vec<Map<String, Value>> = Vec::new();
    for verdict in &report.verdicts {
        let mut draft = verdict.fields.clone();
        agreed_tasks.extend(settle_verdict(check.log(), &mut draft)?);
        // A revisão final não aponta onda: ela responde pelo combinado
        // inteiro, não por uma onda dele. Só a revisão de uma onda usa a
        // última onda do plano quando o veredito não diz qual.
        let is_final = draft.get("final") == Some(&Value::Bool(true));
        let wave = if is_final { verdict.wave } else { verdict.wave.or_else(|| check.log().planned_waves().last().copied()) };
        if let Some(wave) = wave {
            draft.insert("wave".into(), json!(wave));
        }
        // O veredito oficial aponta em `replaces` todas as voltas do revisor
        // desde o pedido de revisão: é ele que fecha o pedido.
        if let Some(returns) = replaced(&verdict.returns) {
            draft.insert("replaces".into(), returns);
        }
        draft.insert("author".into(), json!("review"));
        check.record("verdict", draft.clone())?;
        verdicts.push((wave, draft));
    }
    for task in &agreed_tasks {
        check.record("task", task.clone())?;
    }
    // A entrega oficial de cada onda aponta em `replaces` todas as voltas
    // dela desde o envio que a despachou: nenhuma volta velha fica esperando
    // outra rodada. A cópia na onda que o conserto fecha não substitui nada.
    let mut deliveries = Vec::new();
    for report in &report.waves {
        for wave in std::iter::once(report.wave).chain(report.fixes.iter().copied()) {
            let mut draft = Map::new();
            draft.insert("wave".into(), json!(wave));
            draft.insert("text".into(), json!(report.delivered));
            let files: Vec<String> = report.files.iter().map(|file| own_copy_relative(check.log(), wave, file)).collect();
            draft.insert("files".into(), json!(files));
            if let Some(replan) = &report.replan {
                draft.insert("replan".into(), json!(replan));
            }
            if wave == report.wave
                && let Some(returns) = replaced(&report.returns)
            {
                draft.insert("replaces".into(), returns);
            }
            draft.insert("author".into(), json!("wave"));
            check.record("delivered", draft.clone())?;
            deliveries.push((wave, draft));
        }
    }
    let mut leftover_tasks = Vec::new();
    for wave in &report.waves {
        for leftover in wave.leftovers.iter().filter(|l| l.kind == Some(LeftoverKind::Breaks)) {
            let task = leftover_task(root, check.log(), wave.wave, leftover);
            check.record("task", task.clone())?;
            leftover_tasks.push((wave.wave, task));
        }
    }
    // O consumo, que só se sabe na volta: quando a linha `USAGE` traz algum
    // dos cinco campos, o envio da onda ganha uma versão nova com eles, sem
    // remontar o resto do que foi enviado — também na onda que uma rodada
    // anterior já assumiu.
    let mut sends = Vec::new();
    let usage = report.waves.iter().map(|w| (w.wave, &w.usage)).chain(report.usage.iter().map(|(n, u)| (*n, u)));
    for (wave, usage) in usage {
        let extra = usage.fields();
        if extra.is_empty() {
            continue;
        }
        if let Some(draft) = super::queue::send_revision(check.log(), wave, extra) {
            check.record("send", draft.clone())?;
            sends.push((wave, draft));
        }
    }
    // Mais de uma prova para o mesmo critério não vira uma versão por prova,
    // em cadeia: junta todas num comando só, ligado por `&&`, na ordem e sem
    // repetir, e o critério ganha uma versão só, mais abaixo. Cada uma passa
    // antes pela regra da prova: o que não é linha de comando recusa aqui,
    // nomeando o critério, em vez de virar um comando que o shell não acha na
    // rodada seguinte.
    let mut proofs: Vec<(u64, String)> = Vec::new();
    for wave in &report.waves {
        for (reference, proof) in &wave.proofs {
            let id = criterion_id(check.log(), reference)?;
            let code = check.log().codes().get(&id).cloned().unwrap_or_else(|| id.to_string());
            agreed_prompt::proof_rule(&code, proof)?;
            match proofs.iter_mut().find(|(existing, _)| *existing == id) {
                Some((_, joined)) if joined.split(" && ").any(|part| part == proof) => {}
                Some((_, joined)) => {
                    joined.push_str(" && ");
                    joined.push_str(proof);
                }
                None => proofs.push((id, proof.clone())),
            }
        }
    }
    for (id, proof) in &proofs {
        if let Some(version) = criterion_version(check.log(), *id, proof) {
            check.record("criterion", version.draft)?;
        }
    }
    for draft in commits {
        check.record("commit", draft)?;
    }
    Ok(CheckedReport { verdicts, deliveries, proofs, sends, agreed_tasks, leftover_tasks })
}

/// A versão nova de um critério com a prova nova.
struct CriterionVersion {
    /// O código do critério que a página mostra.
    code: String,
    /// O número da versão que a nova substitui.
    replaces: u64,
    draft: Map<String, Value>,
}

/// A versão nova do critério `id`, com a prova `proof` e o mesmo resto da
/// versão mais nova dele no arquivo `log`. `None` quando o critério saiu da
/// leitura.
fn criterion_version(log: &SpecLog, id: u64, proof: &str) -> Option<CriterionVersion> {
    let criterion = log.current(id)?;
    let mut draft: Map<String, Value> = criterion
        .fields
        .iter()
        .filter(|(key, _)| !["v", "id", "code", "at", "type", "search", "author", "replaces"].contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    draft.insert("proof".into(), json!(proof));
    draft.insert("replaces".into(), json!(criterion.id));
    draft.insert("author".into(), json!("wave"));
    let code = log.codes().get(&criterion.id).cloned().unwrap_or_else(|| criterion.id.to_string());
    Some(CriterionVersion { code, replaces: criterion.id, draft })
}

/// Grava o que [`check_reports`] conferiu, pela mesma porta de gravação das
/// outras: primeiro os vereditos, que julgam entregas já gravadas; depois o
/// entregou de cada onda, a tarefa de cada sobra que quebra e a versão nova
/// de cada critério com prova nova.
/// Devolve o que foi gravado e, de cada prova nova, o código do critério e o
/// comando.
fn record_reports(start: &Path, spec: &str, checked: CheckedReport) -> Result<RecordedReport, Refusal> {
    let CheckedReport { verdicts, deliveries, proofs, sends, agreed_tasks, leftover_tasks } = checked;
    let path = store::spec_file(&crate::commands::spec_events::project(start).root, spec)?;
    let read = || store::read(&path)?.ok_or_else(|| Refusal::NoSpecFile { spec: spec.to_string() });
    let mut recorded = Vec::new();
    for (wave, draft) in verdicts {
        let written = record(start, spec, "verdict", draft, PhaseWriter::Binary)?;
        let mut entry = json!({ "type": "verdict", "id": written.written.id });
        if let Some(wave) = wave {
            entry["wave"] = json!(wave);
        }
        recorded.push(entry);
    }
    for draft in agreed_tasks {
        let written = record(start, spec, "task", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "type": "task", "id": written.written.id }));
    }
    for (wave, draft) in deliveries {
        let written = record(start, spec, "delivered", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": wave, "type": "delivered", "id": written.written.id }));
    }
    for (wave, draft) in leftover_tasks {
        let written = record(start, spec, "task", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": wave, "type": "task", "id": written.written.id }));
    }
    for (wave, draft) in sends {
        let written = record(start, spec, "send", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "wave": wave, "type": "send", "id": written.written.id }));
    }
    let mut ran = Vec::new();
    for (id, proof) in proofs {
        let Some(CriterionVersion { code, replaces, draft }) = criterion_version(&read()?, id, &proof) else {
            continue;
        };
        let written = record(start, spec, "criterion", draft, PhaseWriter::Binary)?;
        recorded.push(json!({ "type": "criterion", "id": written.written.id, "replaces": replaces }));
        ran.push((code, proof));
    }
    Ok((recorded, ran))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use mustard_core::domain::spec_events::SpecEvent;
    use tempfile::tempdir;

    use crate::commands::event::pending::{pending_at, PendingOpts};
    use crate::commands::flow::round::queue::{dispatch_backlog, waves_to_redo};

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A rodada grava o que cada onda entregou, sem pedir revisão nenhuma
    /// dela, e grava o veredito de quem julgar, sob a mesma porta.
    #[test]
    fn what_came_back_becomes_the_delivered_and_the_verdict_of_the_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let out = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("reviews").is_none(), "a rodada não pede revisão nenhuma: {out}");

        let out = round(root, "x", Some(&verdict(root, 1, "approved", "passou")));
        assert!(out.get("reviews").is_none(), "{out}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 1);
        let judged: Vec<&SpecEvent> = log.visible().into_iter().filter(|e| e.event_type == "verdict").collect();
        assert_eq!(judged.len(), 1);
        let criterion = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).unwrap();
        assert_eq!(judged[0].fields["criteria"][0]["criterion"], json!(criterion), "the code became the number");
    }

    /// A volta que o agente grava cita o caminho absoluto dentro da cópia da
    /// própria onda: a gravação o troca pelo caminho relativo ao repositório,
    /// e o conteúdo da cópia entra no principal quando a rodada assume a
    /// volta.
    #[test]
    fn the_round_takes_the_delivery_as_the_agents_return_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let one = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, one)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(delivered_count(root), 1, "{out}");

        // Caminho absoluto dentro da própria cópia da onda: vira caminho
        // relativo ao repositório, e o conteúdo dela entra no principal.
        let copy = wave_prompt::copy_path(root, "x", 2, false);
        std::fs::write(copy.join("src/b.rs"), "fn dois() {}\n// A dobra saiu.\n").unwrap();
        let abs = copy.join("src/b.rs").to_string_lossy().replace('\\', "/");
        let two = json!({"wave": 2, "text": "A dobra saiu.", "files": [abs], "commit": "a onda 2 saiu"});
        assert_eq!(returned(root, two)["ok"], json!(true));
        let out2 = round(root, "x", None);
        assert_eq!(out2["ok"], json!(true), "{out2}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let delivered2 = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "delivered" && e.wave() == Some(2))
            .expect("a entrega da onda 2");
        let files: Vec<String> = delivered2
            .fields
            .get("files")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect();
        assert_eq!(files, vec!["src/b.rs".to_string()], "o caminho gravado é o do repositório: {files:?}");
        let content = std::fs::read_to_string(root.join("src/b.rs")).unwrap();
        assert!(content.contains("A dobra saiu."), "o conteúdo da cópia entrou no repositório principal: {content}");
    }

    /// Quando a entrega traz mais de uma prova para o mesmo critério, elas
    /// ficam todas: um comando só, ligado por `&&`, na ordem e sem repetir, e
    /// o critério ganha uma versão só (não uma em cadeia por prova).
    #[test]
    fn several_proofs_of_one_criterion_all_stay() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let spec_before = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap();
        let criteria_before = spec_before.lines().filter(|l| l.contains("\"type\":\"criterion\"")).count();

        let path = root.join("src/a.rs");
        let file_before = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{file_before}// Saiu.\n")).unwrap();
        let body = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu",
            "proofs": [
                {"criterion": "MSTD-CRIT-0001", "proof": "cargo test a"},
                {"criterion": "MSTD-CRIT-0001", "proof": "cargo test b"},
                {"criterion": "MSTD-CRIT-0001", "proof": "cargo test a"},
                {"criterion": "MSTD-CRIT-0001", "proof": "cargo test c"},
            ]});
        assert_eq!(returned(root, body)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");

        let raw = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap();
        let criteria_after = raw.lines().filter(|l| l.contains("\"type\":\"criterion\"")).count();
        assert_eq!(criteria_after, criteria_before + 1, "one version only, not one per proof: {raw}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let current = log.visible().into_iter().find(|e| e.event_type == "criterion").expect("the criterion");
        assert_eq!(current.str_field("proof"), Some("cargo test a && cargo test b && cargo test c"), "{raw}");
    }

    /// A onda que só foi conferir volta sem arquivo nenhum: a rodada grava a
    /// entrega e segue, sem recusar por falta de arquivo e sem parar para
    /// perguntar nada ao usuário, e nenhum commit sai dessa onda. A proteção
    /// que a exigência da lista de arquivos fazia continua de pé noutro
    /// lugar: a rodada não assume a volta da onda cuja cópia mudou arquivo de
    /// verdade enquanto ela não trouxer o título do commit, e pede ao agente
    /// que grave de novo.
    #[test]
    fn a_onda_que_so_conferiu_e_gravada_sem_pergunta() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);
        let head_before = git_text(root, &["rev-parse", "HEAD"]);

        let checked = json!({"wave": 1,
            "text": "Nada a mudar: o conserto já tinha sido entregue por outra onda. \
                     Rodei a prova do critério e a suíte inteira, e as duas passaram."});
        assert_eq!(returned(root, checked)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("question").is_none(), "nada a perguntar ao usuário: {out}");
        assert!(out.get("commit").is_none(), "a onda que só conferiu não comita: {out}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head_before, "nenhum commit novo: {out}");
        assert_eq!(delivered_count(root), 1, "a entrega foi gravada: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let recorded = log.visible().into_iter().find(|e| e.event_type == "delivered").expect("a entrega");
        assert_eq!(recorded.fields.get("files").and_then(Value::as_array).map_or(0, Vec::len), 0, "{:?}", recorded.fields);

        // A cópia que mudou arquivo de verdade continua pedindo o título do
        // commit, mesmo sem citar arquivo nenhum na entrega.
        let copy = wave_prompt::copy_path(root, "x", 2, false);
        std::fs::write(copy.join("src/b.rs"), "fn um() {}\nfn dois() {}\n").unwrap();
        let hidden = json!({"wave": 2, "text": "Mexi no arquivo e não contei."});
        assert_eq!(returned(root, hidden)["ok"], json!(true));
        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("round-return-needs-commit"), "{refused}");
        assert_eq!(delivered_count(root), 1, "nada foi gravado: {refused}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head_before, "nada foi comitado: {refused}");
    }

    /// Uma onda que volta com pedido de replanejamento e sem arquivo nenhum
    /// tem a entrega gravada, depois do sim do usuário.
    #[test]
    fn a_replan_without_files_is_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let change = "o plano não serve mais";
        let with_replan = json!({"wave": 1, "text": "Parei sem mexer em arquivo.", "replan": change});
        assert_eq!(returned(root, with_replan)["ok"], json!(true));
        let stopped = round(root, "x", None);
        assert_eq!(stopped["reason"], json!("wave-plan-does-not-work"), "{stopped}");
        let session = "s-replan-sem-arquivo";
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), session, "x");
        let question = stopped["question"].as_str().unwrap_or_default().to_string();
        assert_eq!(question, super::super::stops::change_question(1, change, Locale::PtBr), "{stopped}");
        click(root, session, &question, &replan_code(1, change), "Aceitar");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(delivered_count(root), 1, "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let delivered = log.visible().into_iter().find(|e| e.event_type == "delivered").expect("the delivery");
        let files = delivered.fields.get("files").and_then(Value::as_array).map_or(0, Vec::len);
        assert_eq!(files, 0, "{:?}", delivered.fields);
        assert_eq!(delivered.str_field("replan"), Some("o plano não serve mais"));
    }

    /// A volta acima do teto de caracteres é recusada na gravação, e nada é
    /// gravado.
    #[test]
    fn a_delivered_report_over_the_character_cap_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let long = "a".repeat(mustard_core::domain::spec_events::DELIVERED_MAX_CHARS + 1);
        let body = json!({"wave": 1, "text": long, "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        let refused = returned(root, body);
        assert_eq!(refused["reason"], json!("delivered-too-long"), "{refused}");
        assert_eq!(written_deliveries(root), 0, "{refused}");
    }

    /// Quando o campo `commit` da volta chega com cara de código de commit —
    /// hexadecimal, do tamanho de um SHA curto —, a gravação recusa antes de
    /// escrever qualquer coisa, em vez de aceitar em silêncio o código que um
    /// agente comitou dentro da cópia. Corrigido o título, a entrega sai
    /// gravada uma vez.
    #[test]
    fn a_rodada_recusa_o_codigo_de_commit_no_lugar_do_titulo() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let sha_like =
            json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "9f8c3b1a2d4e5f60718293a4b5c6d7e8f9012345"});
        let refused = returned(root, sha_like);
        assert_eq!(refused["reason"], json!("commit-looks-like-sha"), "{refused}");
        assert_eq!(written_deliveries(root), 0, "nada foi gravado");

        let with_title = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "onda 1 fecha a soma"});
        assert_eq!(returned(root, with_title)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(delivered_count(root), 1, "{out}");
    }

    /// A gravação aceita a volta com os campos exatamente como a linha de
    /// exemplo do texto do agente de onda os ensina, nos dois idiomas, e a
    /// rodada a assume: o commit sai com o título e o corpo montados do
    /// resumo. O veredito entra com os campos da linha de exemplo do texto do
    /// revisor, gravado por ele. O próximo passo da rodada ensina as duas
    /// gravações e pede só o consumo, sem nenhuma linha colada.
    #[test]
    fn the_round_takes_the_closing_lines_exactly_as_the_agent_texts_teach_and_commits() {
        for (lang, answer) in [(Locale::PtBr, "Entreguei a soma."), (Locale::EnUs, "I delivered the sum.")] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            approved(root, "x", &[(1, &["src/a.rs"], &[])]);
            let config = format!(r#"{{"language":{{"text":"{}"}}}}"#, lang.as_str());
            std::fs::write(root.join("mustard.json"), config).unwrap();
            let first = round(root, "x", None);
            let taught = translate("round.report", lang);
            assert!(first["next"].as_str().unwrap_or_default().contains(taught), "{first}");
            for said in ["run write delivered", "run write verdict", "<USAGE>", "`commit`"] {
                assert!(taught.contains(said), "{said}: {taught}");
            }
            assert!(!taught.contains("<DELIVERED>") && !taught.contains("<VERDICT>"), "{taught}");

            std::fs::write(root.join("src/a.rs"), "fn um() {}\nfn soma() {}\n").unwrap();
            let (wave_text, review_text) = mustard_core::agent_texts(lang)
                .iter()
                .fold((String::new(), String::new()), |(w, r), (name, body)| match *name {
                    "wave" => ((*body).to_string(), r),
                    "review" => (w, (*body).to_string()),
                    _ => (w, r),
                });
            let example = if lang == Locale::PtBr {
                [("<a entrega>", "A soma saiu, com o teste."), ("caminho/do/arquivo.rs", "src/a.rs"), ("<o resumo do commit>", "a soma sai")]
            } else {
                [("<the delivery>", "The sum is out, with its test."), ("path/to/file.rs", "src/a.rs"), ("<the commit summary>", "the sum ships")]
            };
            assert!(wave_text.contains("run write delivered"), "{lang:?}: {wave_text}");
            let line = taught_line(&wave_text, "{\"wave\"", &example);
            let body: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{lang:?}: {e}: {line}"));
            assert_eq!(returned(root, body)["ok"], json!(true), "{lang:?}: {answer}");
            let back = round(root, "x", None);
            assert_eq!(back["ok"], json!(true), "{lang:?}: {back}");
            let (subject, body) = last_commit(root);
            let (title, summary) = if lang == Locale::PtBr {
                ("feat(onda-1): a soma sai", "- onda 1: a soma sai")
            } else {
                ("feat(wave-1): the sum ships", "- wave 1: the sum ships")
            };
            assert_eq!(subject, title, "{lang:?}");
            assert_eq!(body, summary, "{lang:?}");
            assert_eq!(back["commit"]["title"], json!(title), "{back}");
            let shown = Command::new("git").args(["show", "--name-only", "--format=", "HEAD"]).current_dir(root).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&shown.stdout).trim(), "src/a.rs");

            assert!(review_text.contains("run write verdict"), "{lang:?}: {review_text}");
            let line = taught_line(&review_text, "{\"final\"", &[]);
            let body: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{lang:?}: {e}: {line}"));
            seed_review(root);
            assert_eq!(judged(root, body)["ok"], json!(true), "{lang:?}: {line}");
            let judged = round(root, "x", None);
            assert_eq!(judged["ok"], json!(true), "{lang:?}: {judged}");
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let visible = log.visible();
            let delivered = visible.iter().find(|e| e.event_type == "delivered").expect("the delivery");
            assert_eq!(delivered.str_field("text"), Some(example[0].1));
            assert_eq!(delivered.wave(), Some(1));
            let commit = visible.iter().find(|e| e.event_type == "commit").expect("the commit");
            assert_eq!(commit.str_field("title"), Some(title));
            let verdict = visible.iter().find(|e| e.event_type == "verdict").expect("the verdict");
            assert_eq!(verdict.str_field("result"), Some("approved"));
            let criterion = visible.iter().find(|e| e.event_type == "criterion").map(|e| e.id);
            assert_eq!(verdict.fields["criteria"][0]["criterion"].as_u64(), criterion);
            assert_eq!(judged["command"], json!("mustard-rt run close --spec x"), "{judged}");
        }
    }

    /// O texto corrido, sem linha nenhuma, a rodada recusa e não grava nada;
    /// a volta sem o resumo do commit, a gravação recusa; o veredito sem a
    /// onda ou com um critério que a spec não tem também.
    #[test]
    fn a_report_without_the_closing_line_or_its_fields_is_refused_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        seed_review(root);
        let lines_before = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();

        let missing = round(root, "x", Some("Entreguei a soma, e os arquivos mudaram."));
        assert_eq!(missing["reason"], json!("round-bad-report"), "{missing}");

        let without = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"]});
        let refused = returned(root, without);
        assert_eq!(refused["reason"], json!("missing-field"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains("commit"), "{refused}");

        let no_wave = json!({"result": "approved", "text": "passou", "criteria": []});
        assert_eq!(judged(root, no_wave)["reason"], json!("missing-field"));

        let unknown = json!({"wave": 1, "result": "approved", "text": "passou",
            "criteria": [{"criterion": "MSTD-CRIT-0099", "tests_rule": true}]});
        let refused = judged(root, unknown);
        assert_eq!(refused["ok"], json!(false), "{refused}");

        let lines_after = std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        assert_eq!(lines_after, lines_before, "nothing was recorded");
    }

    /// A onda de lote — formada pelo binário a partir do backlog — cujo Claude
    /// Code fecha no meio do trabalho sem gravar a entrega: quando o
    /// orquestrador passa só a linha de consumo dela, a rodada vê o envio
    /// aberto, nenhuma volta e o processo morto, e devolve ao backlog todas as
    /// tarefas que a onda levava, sem separar nenhuma e sem gravar entrega.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_onda_cortada_sem_volta_devolve_as_tarefas_ao_backlog() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
        let said = log.visible().into_iter().find(|e| e.event_type == "message").unwrap().id;
        let t1 = write(
            root,
            "x",
            "task",
            json!({"text": "Tarefa um.", "files": [{"path": "src/b.rs"}], "depends_on": [],
                "covers": [crit], "origin": said}),
        );
        let t2 = write(
            root,
            "x",
            "task",
            json!({"text": "Tarefa dois.", "files": [{"path": "src/c.rs"}], "depends_on": [],
                "covers": [crit], "origin": said}),
        );
        let t3 = write(
            root,
            "x",
            "task",
            json!({"text": "Tarefa três.", "files": [{"path": "src/d.rs"}], "depends_on": [],
                "covers": [crit], "origin": said}),
        );
        let (id1, id2, id3) = (id_of(&t1), id_of(&t2), id_of(&t3));

        let log = store::read(&path).unwrap().unwrap();
        let formed = dispatch_backlog(root, "x", &log).expect("formou o lote");
        assert_eq!(formed, vec![2], "as três tarefas soltas viram junto a mesma onda de lote: {formed:?}");

        let out = round(root, "x", None);
        assert!(waves_in(&out, "dispatch").contains(&2), "a onda de lote sai como qualquer outra: {out}");

        // O Claude Code que a levou fecha no meio do trabalho: o pedido
        // continua aberto, mas o processo por trás dele já morreu.
        let log = store::read(&path).unwrap().unwrap();
        let sent = log.visible().into_iter().rfind(|e| e.wave() == Some(2) && e.event_type == "send").unwrap();
        let mut draft = sent.fields.clone();
        for key in ["v", "id", "code", "at", "type", "search"] {
            draft.remove(key);
        }
        drop(log);
        let mut dead = Command::new("true").spawn().expect("spawn the fixture process");
        let dead_pid = dead.id();
        dead.wait().expect("reap the fixture process");
        draft.insert("claude_pid".into(), json!(dead_pid));
        draft.insert("claude_started".into(), json!(1));
        store::write_at(&path, "send", draft, &[], &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string())
            .unwrap();

        // Só a linha de consumo chega, sem volta nenhuma gravada pela onda.
        let usage = line("USAGE", json!({"wave": 2, "model": "claude-opus", "steps": 3, "tokens": 900}));
        let cut = round(root, "x", Some(&usage));
        assert_eq!(cut["ok"], json!(true), "o corte da onda de lote não recusa a rodada: {cut}");
        assert!(
            !waves_in(&cut, "dispatch").contains(&2),
            "a onda que o próprio corte acabou de esvaziar não sai de novo na mesma rodada: {cut}"
        );

        let after = store::read(&path).unwrap().unwrap();
        for (label, id) in [("um", id1), ("dois", id2), ("três", id3)] {
            assert!(
                after.current(id).unwrap().wave().is_none(),
                "a tarefa {label} ganha versão nova sem onda e volta ao backlog, sem separar nenhuma: {:?}",
                after.current(id).unwrap().fields
            );
        }
        assert!(
            after.visible().iter().all(|e| e.event_type != "delivered" || e.wave() != Some(2)),
            "nenhuma entrega da onda cortada foi gravada"
        );

        // A onda 2 ficou sem tarefa nenhuma: a rodada seguinte não pode
        // despachá-la de novo, nem como pedido fresco nem como reenvio do
        // pedido antigo — não há mais o que entregar por ela.
        let sends_before = after.visible().iter().filter(|e| e.event_type == "send" && e.wave() == Some(2)).count();
        let again = round(root, "x", None);
        assert!(
            !waves_in(&again, "dispatch").contains(&2) && !waves_in(&again, "resend").contains(&2),
            "a onda esvaziada pelo corte não sai de novo, nem fresca nem reenviada: {again}"
        );
        let after_again = store::read(&path).unwrap().unwrap();
        let sends_after =
            after_again.visible().iter().filter(|e| e.event_type == "send" && e.wave() == Some(2)).count();
        assert_eq!(sends_before, sends_after, "nenhum pedido novo foi gravado para a onda esvaziada");
    }

    /// A mesma onda de lote, mas com o Claude Code ainda aberto por trás do
    /// pedido — o processo deste próprio teste: sem processo morto, não há
    /// corte a reconhecer. A linha de consumo sem volta gravada é recusada,
    /// com o pedido de que o agente grave a entrega, sem comitar e sem
    /// devolver tarefa nenhuma ao backlog.
    #[test]
    fn uma_onda_de_lote_ainda_viva_sem_volta_pede_a_entrega_e_nao_devolve_tarefa() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let crit = log.visible().into_iter().find(|e| e.event_type == "criterion").unwrap().id;
        let said = log.visible().into_iter().find(|e| e.event_type == "message").unwrap().id;
        let t1 = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Tarefa solta.", "files": [{"path": "src/b.rs"}], "depends_on": [],
                "covers": [crit], "origin": said}),
        ));
        let log = store::read(&path).unwrap().unwrap();
        let formed = dispatch_backlog(root, "x", &log).expect("formou o lote");
        assert_eq!(formed, vec![2], "o lote do backlog virou a onda 2: {formed:?}");

        let out = round(root, "x", None);
        assert!(waves_in(&out, "dispatch").contains(&2), "a onda de lote sai como qualquer outra: {out}");

        let lines_before = std::fs::read_to_string(&path).unwrap().lines().count();
        let usage = line("USAGE", json!({"wave": 2, "model": "claude-opus", "steps": 3, "tokens": 900}));
        let refused = round(root, "x", Some(&usage));
        assert_eq!(refused["reason"], json!("round-return-missing"), "{refused}");
        let asked = translate("spec_events.return_missing", Locale::PtBr).replace("{wave}", "2");
        assert_eq!(refused["hint"], json!(asked), "{refused}");

        let lines_after = std::fs::read_to_string(&path).unwrap().lines().count();
        assert_eq!(lines_after, lines_before, "nada foi gravado sem o corte de verdade");
        let after = store::read(&path).unwrap().unwrap();
        assert_eq!(after.current(t1).unwrap().wave(), Some(2), "a tarefa segue com a onda, sem processo morto");
    }

    /// Tudo é conferido antes da primeira gravação. O caminho que não está no
    /// disco nem no git é recusado na gravação da volta, sozinho e junto de um
    /// caminho certo, e a volta corrigida é assumida uma vez só. O veredito
    /// sem resultado também é recusado na gravação, sem deixar nada gravado.
    #[test]
    fn a_wrong_path_or_a_line_missing_a_field_is_refused_before_anything_is_recorded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        let before = spec_lines();

        for files in [&["src/nao_existe.rs"][..], &["src/a.rs", "src/nao_existe.rs"][..]] {
            let wrong = json!({"wave": 1, "text": "Saiu.", "files": files, "commit": "a onda 1 saiu"});
            let refused = returned(root, wrong);
            assert_eq!(refused["reason"], json!("round-file-unknown"), "{files:?}: {refused}");
            let expected = translate("round.file_unknown", Locale::PtBr)
                .replace("{file}", "src/nao_existe.rs")
                .replace("{wave}", "1");
            assert_eq!(refused["hint"], json!(expected), "{refused}");
            assert_eq!(spec_lines(), before, "{files:?}: nothing was recorded");
        }

        seed_review(root);
        let before = spec_lines();
        let no_result = json!({"wave": 1, "text": "sem resultado",
            "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]});
        let refused = judged(root, no_result);
        assert_eq!(refused["reason"], json!("missing-field"), "{refused}");
        assert_eq!(spec_lines(), before, "the verdict without a result was not recorded");

        let went = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "the corrected line records the delivery once");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(log.visible().iter().all(|e| e.event_type != "verdict"), "no verdict was left behind");
    }

    /// Um pedido da onda `n` gravado sem passar pela rodada, com a cópia e a
    /// pasta de compilação já dela, e um Claude Code vivo por trás — o
    /// processo do próprio teste, que segue aberto até o fim dele: assim a
    /// trava por arquivo não confunde este pedido, já em andamento, com um
    /// que ainda espera a vaga do arquivo, nem a limpeza de órfã mexe nele.
    fn seed_send_with_copy(root: &Path, n: u64, copy: &str, build_dir: &str) {
        let (claude_pid, claude_started) = crate::commands::flow::stuck::sender_process();
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "send",
            json!({"wave": n, "role": "wave", "text": "pedido", "lines": 1, "chars": 6, "items": [1],
                "mustard": "0", "author": "binary", "copy": copy, "build_dir": build_dir,
                "claude_pid": claude_pid, "claude_started": claude_started}),
        );
    }

    /// A rodada despacha a onda 1; a 2, que declara o mesmo arquivo, espera a
    /// vaga do arquivo. A cópia da 2 já existia, de um pedido anterior à
    /// trava por arquivo — outra vaga de compilação, ainda em andamento —, e
    /// a entrega dela segue passando pela mesma fusão. A entrega da primeira
    /// é juntada ao repositório principal — o arquivo novo inclusive —,
    /// comitada, e a cópia dela é apagada; a cópia da segunda, que a trava
    /// não tocou, segue intacta. A da segunda, com um trecho que conflita com
    /// a primeira, é recusada sem gravar nada, com a lista dos trechos e a
    /// cópia em que se resolve; resolvido o conflito na cópia, a mesma
    /// entrega é juntada e comitada uma vez só, e a cópia some.
    #[test]
    fn two_waves_on_the_same_file_are_merged_and_a_conflict_is_refused_until_resolved() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        // Um projeto Rust: só nele o pedido cita a pasta de compilação.
        mapped(root, "cargo");
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "a onda 2 divide arquivo com a 1 e espera: {out}");
        let copy = |wave: u64| mustard_core::io::wave_prompt::copy_path(root, "x", wave, false);
        let shown = |wave: u64| mustard_core::io::wave_prompt::shown(&copy(wave));
        let prompt = out["dispatch"][0]["prompt"].as_str().unwrap_or_default();
        assert!(prompt.contains(&format!("`{}`", shown(1))), "{prompt}");
        let folder = |prompt: &str| prompt.split("CARGO_TARGET_DIR=").nth(1).and_then(|rest| rest.split('`').next()).map(str::to_string);
        let folder1 = folder(prompt);
        assert!(folder1.is_some(), "{prompt}");

        let head = || {
            let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        // A cópia da onda 2 já existia, de um pedido anterior à trava por
        // arquivo, ainda em aberto — sobre o mesmo commit que a da 1.
        git_at(root, &["worktree", "add", "--detach", &copy(2).to_string_lossy(), &head()]);
        let folder2 = format!("{}/b", mustard_core::io::wave_prompt::shown(&root.join("target").join("copias")));
        seed_send_with_copy(root, 2, &copy(2).to_string_lossy(), &folder2);

        // Cada agente trabalha na sua cópia.
        std::fs::write(copy(1).join("src/a.rs"), "fn um() {}\n// onda 1\n").unwrap();
        std::fs::write(copy(1).join("src/novo.rs"), "fn novo() {}\n").unwrap();
        std::fs::write(copy(2).join("src/a.rs"), "fn um() {}\n// onda 2\n").unwrap();
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        let commits_of = |wave: u64| {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            log.visible().iter().filter(|e| e.event_type == "commit" && e.ints("waves").contains(&wave)).count()
        };

        let first = json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/a.rs", "src/novo.rs"],
            "commit": "a onda 1 sai"});
        assert_eq!(returned(root, first)["ok"], json!(true));
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn um() {}\n// onda 1\n");
        assert_eq!(std::fs::read_to_string(root.join("src/novo.rs")).unwrap(), "fn novo() {}\n");
        assert_eq!(last_commit(root).0, "feat(onda-1): a onda 1 sai");
        let shown_files = Command::new("git").args(["show", "--name-only", "--format=", "HEAD"]).current_dir(root).output();
        let shown_files = String::from_utf8_lossy(&shown_files.unwrap().stdout).to_string();
        assert_eq!(shown_files.lines().collect::<Vec<_>>(), ["src/a.rs", "src/novo.rs"], "{went}");
        assert!(!copy(1).exists(), "the first copy is gone after the commit: {went}");
        // Só o aviso da onda que entregou sem linha de consumo, de outro
        // assunto: a junção das duas ondas não tem o que avisar.
        let warned: Vec<Value> = went["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|w| w["reason"] != json!("usage-missing"))
            .collect();
        assert!(warned.is_empty(), "{went}");
        // A onda 2 já tem pedido aberto: a rodada não despacha nada de novo,
        // e a cópia dela, com o trecho que ainda não entregou, segue como
        // estava.
        assert!(waves_in(&went, "dispatch").is_empty(), "{went}");
        assert_eq!(std::fs::read_to_string(copy(2).join("src/a.rs")).unwrap(), "fn um() {}\n// onda 2\n", "{went}");

        let second = json!({"wave": 2, "text": "A onda 2 saiu.", "files": ["src/a.rs"], "commit": "a onda 2 sai"});
        assert_eq!(returned(root, second)["ok"], json!(true));
        let (seed, before) = (head(), spec_lines());
        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("round-merge-conflict"), "{refused}");
        let expected = translate("round.merge_conflict", Locale::PtBr)
            .replace("{wave}", "2")
            .replace("{conflicts}", "src/a.rs:2")
            .replace("{copy}", &shown(2))
            .replace("{head}", &seed);
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        assert_eq!((head(), spec_lines()), (seed.clone(), before), "nothing was committed or recorded: {refused}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn um() {}\n// onda 1\n");
        assert!(copy(2).exists(), "the conflicting copy stays to be resolved");

        // Resolvido como a recusa ensina: a cópia vai ao commit atual e o
        // trecho marcado é acertado.
        git_at(&copy(2), &["checkout", "-q", "--merge", "--detach", &seed]);
        assert!(std::fs::read_to_string(copy(2).join("src/a.rs")).unwrap().contains("<<<<<<<"));
        std::fs::write(copy(2).join("src/a.rs"), "fn um() {}\n// onda 1\n// onda 2\n").unwrap();
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn um() {}\n// onda 1\n// onda 2\n");
        assert_eq!(last_commit(root).0, "feat(onda-2): a onda 2 sai");
        assert_eq!((commits_of(1), commits_of(2), delivered_count(root)), (1, 1, 2), "each delivery went in once");
        assert!(!copy(2).exists(), "the second copy is gone after the commit: {went}");
    }

    /// O texto de `rev` no repositório: o título do commit e as linhas que ele
    /// acrescentou.
    fn commit_at(root: &Path, rev: &str) -> (String, Vec<String>) {
        let out = Command::new("git").args(["show", "--unified=0", "--format=%s", rev]).current_dir(root).output();
        let text = String::from_utf8_lossy(&out.unwrap().stdout).to_string();
        let subject = text.lines().next().unwrap_or_default().to_string();
        let added = text.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).map(str::to_string).collect();
        (subject, added)
    }

    /// Um gancho de commit que sempre recusa, no checkout `root`. Devolve o
    /// arquivo dele, para o teste tirá-lo depois.
    fn refusing_hook(root: &Path) -> std::path::PathBuf {
        let hooks = root.join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);
        hook
    }

    /// Duas rodadas da spec `x` soltas ao mesmo tempo, sem relatório,
    /// enquanto outro passo do git segura a trava: devolve o arquivo
    /// principal (`main_file`) como estava enquanto a trava seguia presa, e
    /// a resposta de cada rodada, depois de a trava soltar.
    fn two_rounds_at_once(root: &Path, main_file: &dyn Fn() -> String) -> (String, Vec<Value>) {
        let before = main_file();
        std::thread::scope(|scope| {
            let Ok(lock) = git_lock(root) else { panic!("the git lock") };
            let rounds = [scope.spawn(|| round(root, "x", None)), scope.spawn(|| round(root, "x", None))];
            for _ in 0..150 {
                if main_file() != before {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let while_held = main_file();
            drop(lock);
            (while_held, rounds.into_iter().map(|r| r.join().unwrap()).collect())
        })
    }

    /// O número de commits do HEAD para trás no repositório `dir`.
    fn commit_count(dir: &Path) -> u64 {
        git_text(dir, &["rev-list", "--count", "HEAD"]).parse().unwrap_or_default()
    }

    /// Duas rodadas ao mesmo tempo, com as voltas de duas ondas que mexeram
    /// no mesmo arquivo, em trechos diferentes, já gravadas na spec — a cópia
    /// da 2 já existia, de um pedido anterior à trava por arquivo, na mesma
    /// base da 1.
    ///
    /// Com o gancho do commit recusando, as duas leem as voltas, juntam, veem
    /// o git recusar e desfazem, uma depois da outra: nada é comitado nem
    /// gravado, e as cópias ficam. Sem o gancho, enquanto outro passo do git
    /// segura a trava, nenhuma lê nem junta nada; solta a trava, uma lê as
    /// duas voltas, junta, comita e grava, e a outra, que lê a spec de novo
    /// sob a trava, não acha volta a assumir: um commit só, com as duas
    /// ondas, cada entrega gravada uma vez e as duas cópias somem.
    #[test]
    fn two_rounds_at_the_same_time_assume_each_return_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1], "a onda 2 espera a vaga do arquivo");
        let copy = |wave: u64| mustard_core::io::wave_prompt::copy_path(root, "x", wave, false);
        let seed = git_text(root, &["rev-parse", "HEAD"]);
        git_at(root, &["worktree", "add", "--detach", &copy(2).to_string_lossy(), &seed]);
        let folder2 = format!("{}/b", mustard_core::io::wave_prompt::shown(&root.join("target").join("copias")));
        seed_send_with_copy(root, 2, &copy(2).to_string_lossy(), &folder2);
        std::fs::write(copy(1).join("src/a.rs"), "// onda 1\nfn um() {}\n").unwrap();
        std::fs::write(copy(2).join("src/a.rs"), "fn um() {}\n// onda 2\n").unwrap();
        for wave in [1, 2] {
            let body = json!({"wave": wave, "text": format!("A onda {wave} saiu."), "files": ["src/a.rs"],
                "commit": format!("a onda {wave} sai")});
            assert_eq!(returned(root, body)["ok"], json!(true));
        }
        let main_file = || std::fs::read_to_string(root.join("src/a.rs")).unwrap();

        let hook = refusing_hook(root);
        let (_, refused) = two_rounds_at_once(root, &main_file);
        for out in &refused {
            assert_eq!(out["reason"], json!("git-refused"), "{refused:?}");
        }
        assert_eq!(main_file(), "fn um() {}\n", "each refused round put the file back: {refused:?}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), seed, "nothing was committed: {refused:?}");
        assert_eq!(delivered_count(root), 0, "nothing was recorded: {refused:?}");
        assert!(copy(1).exists() && copy(2).exists(), "the copies stay for the next round: {refused:?}");

        std::fs::remove_file(&hook).unwrap();
        let (while_held, outs) = two_rounds_at_once(root, &main_file);
        assert_eq!(while_held, "fn um() {}\n", "no round joined while another git step held the lock");
        for out in &outs {
            assert_eq!(out["ok"], json!(true), "{outs:?}");
        }
        assert_eq!(outs.iter().filter(|out| out.get("commit").is_some()).count(), 1, "one round commits: {outs:?}");
        assert_eq!(main_file(), "// onda 1\nfn um() {}\n// onda 2\n", "{outs:?}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD~1"]), seed, "one commit only: {outs:?}");
        assert_eq!(
            commit_at(root, "HEAD"),
            ("feat(ondas-1-2): a onda 1 sai".to_string(), vec!["+// onda 1".to_string(), "+// onda 2".to_string()]),
            "{outs:?}"
        );
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let mut delivered: Vec<u64> =
            log.visible().iter().filter(|e| e.event_type == "delivered").filter_map(|e| e.wave()).collect();
        delivered.sort_unstable();
        let commits: Vec<Vec<u64>> =
            log.visible().iter().filter(|e| e.event_type == "commit").map(|e| e.ints("waves")).collect();
        assert_eq!((delivered, commits), (vec![1, 2], vec![vec![1, 2]]), "each once: {outs:?}");
        assert!(!copy(1).exists() && !copy(2).exists(), "both copies are gone: {outs:?}");
    }

    /// Duas rodadas ao mesmo tempo, com as voltas de duas ondas que mexeram
    /// no mesmo arquivo de um submódulo, em trechos diferentes, já gravadas
    /// na spec — a cópia da 2 já existia, de um pedido anterior à trava por
    /// arquivo, na mesma base da 1, com o submódulo dela já na branch da
    /// unidade. Enquanto outro passo do git segura a trava, nada é juntado;
    /// solta a trava, uma rodada junta as duas, comita no submódulo e comita
    /// o ponteiro no principal, e a outra não acha volta a assumir. O arquivo
    /// termina com as duas mudanças, num commit só em cada repositório, e as
    /// cópias somem, com as dos submódulos.
    #[test]
    fn two_rounds_at_the_same_time_on_the_same_submodule_file_commit_once() {
        let dir = tempdir().unwrap();
        let root = &dir.path().join("principal");
        with_submodule(root, dir.path());
        approved(root, "x", &[(1, &["libs/sub/lib.txt"], &[]), (2, &["libs/sub/lib.txt"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1], "a onda 2 espera a vaga do arquivo");
        let copy = |wave: u64| mustard_core::io::wave_prompt::copy_path(root, "x", wave, false);
        git_at(root, &["worktree", "add", "--detach", &copy(2).to_string_lossy(), &git_text(root, &["rev-parse", "HEAD"])]);
        let unit = {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            State::from_log(&log).branch.unwrap_or_default()
        };
        let sub = root.join("libs/sub");
        crate::commands::git_settle::enter_unit_branch(&sub, &unit).unwrap();
        git_at(&sub, &["worktree", "add", "--detach", &copy(2).join("libs/sub").to_string_lossy(), "HEAD"]);
        let folder2 = format!("{}/b", mustard_core::io::wave_prompt::shown(&root.join("target").join("copias")));
        seed_send_with_copy(root, 2, &copy(2).to_string_lossy(), &folder2);
        std::fs::write(copy(1).join("libs/sub/lib.txt"), "// onda 1\nfn um() {}\n").unwrap();
        std::fs::write(copy(2).join("libs/sub/lib.txt"), "fn um() {}\n// onda 2\n").unwrap();
        for wave in [1, 2] {
            let body = json!({"wave": wave, "text": format!("A onda {wave} saiu."), "files": ["libs/sub/lib.txt"],
                "commit": format!("a onda {wave} sai")});
            assert_eq!(returned(root, body)["ok"], json!(true));
        }
        let main_file = || std::fs::read_to_string(sub.join("lib.txt")).unwrap();
        let (main_before, sub_before) = (commit_count(root), commit_count(&sub));

        let (while_held, outs) = two_rounds_at_once(root, &main_file);
        assert_eq!(while_held, "fn um() {}\n", "no round joined while another git step held the lock");
        for out in &outs {
            assert_eq!(out["ok"], json!(true), "{outs:?}");
        }
        assert_eq!(main_file(), "// onda 1\nfn um() {}\n// onda 2\n", "{outs:?}");
        assert_eq!((commit_count(root), commit_count(&sub)), (main_before + 1, sub_before + 1), "{outs:?}");
        assert_eq!(
            commit_at(&sub, "HEAD"),
            ("feat(ondas-1-2): a onda 1 sai".to_string(), vec!["+// onda 1".to_string(), "+// onda 2".to_string()]),
            "{outs:?}"
        );
        assert_eq!(git_text(root, &["rev-parse", "HEAD:libs/sub"]), git_text(&sub, &["rev-parse", "HEAD"]));
        assert_eq!(git_text(root, &["show", "--name-only", "--format=", "HEAD"]), "libs/sub", "{outs:?}");
        assert!(!copy(1).exists() && !copy(2).exists(), "both copies are gone: {outs:?}");
        assert_eq!(git_text(&sub, &["worktree", "list", "--porcelain"]).matches("worktree ").count(), 1);
    }

    /// Duas voltas, a primeira com um trecho que conflita com o repositório
    /// principal: a segunda é juntada, comitada e gravada, e a cópia dela
    /// some. A em conflito fica de fora — nada dela é juntado, nem o arquivo
    /// que não conflitava, nem gravado —, e a resposta traz a recusa dela, com
    /// os trechos e o comando que a leva ao commit que já tem a outra.
    /// Resolvida, a volta dela, que segue na spec, é juntada e comitada uma
    /// vez só.
    #[test]
    fn a_conflict_holds_only_the_delivery_of_its_wave() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/c.rs", "src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let copy = |wave: u64| mustard_core::io::wave_prompt::copy_path(root, "x", wave, false);
        let read = |path: &Path| std::fs::read_to_string(path).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// principal\n").unwrap();
        git_at(root, &["commit", "-q", "-am", "outra mudança"]);
        std::fs::write(copy(1).join("src/c.rs"), "fn um() {}\n// onda 1 sem conflito\n").unwrap();
        std::fs::write(copy(1).join("src/a.rs"), "fn um() {}\n// onda 1\n").unwrap();
        std::fs::write(copy(2).join("src/b.rs"), "fn um() {}\n// onda 2\n").unwrap();
        let first = json!({"wave": 1, "text": "A onda 1 saiu.", "files": ["src/c.rs", "src/a.rs"],
            "commit": "a onda 1 sai"});
        let second = json!({"wave": 2, "text": "A onda 2 saiu.", "files": ["src/b.rs"], "commit": "a onda 2 sai"});
        assert_eq!(returned(root, first)["ok"], json!(true));
        assert_eq!(returned(root, second)["ok"], json!(true));

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(read(&root.join("src/b.rs")), "fn um() {}\n// onda 2\n");
        assert_eq!(commit_at(root, "HEAD"), ("feat(onda-2): a onda 2 sai".to_string(), vec!["+// onda 2".to_string()]));
        assert_eq!(read(&root.join("src/a.rs")), "fn um() {}\n// principal\n", "nothing of the held wave was joined");
        assert_eq!(read(&root.join("src/c.rs")), "fn um() {}\n", "not even its file without conflict");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let delivered: Vec<Option<u64>> =
            log.visible().iter().filter(|e| e.event_type == "delivered").map(|e| e.wave()).collect();
        assert_eq!(delivered, [Some(2)], "only the other delivery was recorded: {out}");
        assert!(!copy(2).exists(), "the other copy is gone: {out}");
        assert!(copy(1).exists(), "the conflicting copy stays to be resolved");
        let head = String::from_utf8_lossy(
            &Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().unwrap().stdout,
        )
        .trim()
        .to_string();
        let expected = translate("round.merge_conflict", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{conflicts}", "src/a.rs:2")
            .replace("{copy}", &mustard_core::io::wave_prompt::shown(&copy(1)))
            .replace("{head}", &head);
        let warned: Vec<Value> = out["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|w| w["reason"] != json!("usage-missing"))
            .collect();
        assert_eq!(json!(warned), json!([{"reason": "round-merge-conflict", "wave": 1, "hint": expected}]), "{out}");
        assert_eq!(waves_in(&out, "running"), vec![1], "the held wave is still out: {out}");

        git_at(&copy(1), &["checkout", "-q", "--merge", "--detach", &head]);
        std::fs::write(copy(1).join("src/a.rs"), "fn um() {}\n// principal\n// onda 1\n").unwrap();
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(read(&root.join("src/a.rs")), "fn um() {}\n// principal\n// onda 1\n");
        assert_eq!(read(&root.join("src/c.rs")), "fn um() {}\n// onda 1 sem conflito\n");
        assert_eq!(last_commit(root).0, "feat(onda-1): a onda 1 sai");
        assert_eq!(delivered_count(root), 2, "each delivery went in once");
        assert!(!copy(1).exists(), "the resolved copy is gone: {went}");
    }

    /// A junção que o git recusa depois de gravada volta o repositório
    /// principal ao que era: a chamada corrigida junta de novo, sem conflito,
    /// e grava a entrega uma vez só.
    #[cfg(unix)]
    #[test]
    fn a_merge_the_commit_refuses_leaves_the_main_repository_as_it_was() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let copy = mustard_core::io::wave_prompt::copy_path(root, "x", 1, false);
        std::fs::write(root.join("src/a.rs"), "// principal\nfn um() {}\n").unwrap();
        git_at(root, &["commit", "-q", "-am", "outra mudança"]);
        std::fs::write(copy.join("src/a.rs"), "fn um() {}\n// onda 1\n").unwrap();
        let hooks = root.join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);

        let report = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"], "commit": "a onda 1 sai"});
        assert_eq!(returned(root, report)["ok"], json!(true));
        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("git-refused"), "{refused}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "// principal\nfn um() {}\n");
        assert_eq!(delivered_count(root), 0);

        std::fs::remove_file(&hook).unwrap();
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "// principal\nfn um() {}\n// onda 1\n");
        assert_eq!(delivered_count(root), 1);
    }

    /// Duas ondas, cada uma na sua cópia e no seu arquivo, a primeira com um
    /// arquivo novo. O gancho do commit recusa a entrega da primeira: enquanto
    /// ele roda, nada da rodada está preparado no checkout, e depois da
    /// recusa o disco e o índice voltam ao que eram — o arquivo mudado volta,
    /// o novo some e o git não guarda registro de nenhum dos dois. Sem a
    /// recusa, a volta da primeira, que segue na spec, é juntada e comitada
    /// uma vez, com os seus dois arquivos; a segunda, gravada depois, é
    /// comitada só com o arquivo dela, e cada entrega é gravada uma vez.
    #[cfg(unix)]
    #[test]
    fn two_waves_one_refused_by_git_leave_nothing_staged_and_each_commit_carries_only_its_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let copy = |wave: u64| mustard_core::io::wave_prompt::copy_path(root, "x", wave, false);
        std::fs::write(copy(1).join("src/a.rs"), "fn um() {}\n// onda 1\n").unwrap();
        std::fs::write(copy(1).join("src/novo.rs"), "fn novo() {}\n").unwrap();
        std::fs::write(copy(2).join("src/b.rs"), "fn um() {}\n// onda 2\n").unwrap();

        // A cada commit, o gancho anota que rodou e o que está preparado no
        // índice do checkout, e recusa enquanto o sinal de recusa existir.
        let hooks = tempdir().unwrap();
        let (staged, refuse) = (hooks.path().join("preparado"), hooks.path().join("recusa"));
        let hook = hooks.path().join("pre-commit");
        let script = format!(
            "#!/bin/sh\necho commit >> '{0}'\nenv -u GIT_INDEX_FILE git diff --cached --name-only >> '{0}'\n\
             if [ -f '{1}' ]; then echo 'o gancho recusou' >&2; exit 1; fi\n",
            staged.display(),
            refuse.display()
        );
        std::fs::write(&hook, script).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_at(root, &["config", "core.hooksPath", &hooks.path().to_string_lossy()]);
        std::fs::write(&refuse, b"").unwrap();

        let pending = || {
            let out = Command::new("git").args(["status", "--porcelain", "--", "src"]).current_dir(root).output();
            String::from_utf8_lossy(&out.unwrap().stdout).to_string()
        };
        let committed = || {
            let out = Command::new("git").args(["show", "--name-only", "--format=%s", "HEAD"]).current_dir(root).output();
            let text = String::from_utf8_lossy(&out.unwrap().stdout).to_string();
            text.lines().filter(|line| !line.is_empty()).map(str::to_string).collect::<Vec<_>>()
        };
        let report = |wave: u64, files: &[&str]| {
            let body = json!({"wave": wave, "text": format!("A onda {wave} saiu."), "files": files,
                "commit": format!("a onda {wave} sai")});
            assert_eq!(returned(root, body)["ok"], json!(true));
        };
        report(1, &["src/a.rs", "src/novo.rs"]);

        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("git-refused"), "{refused}");
        assert!(refused["hint"].as_str().unwrap_or_default().contains("o gancho recusou"), "{refused}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn um() {}\n");
        assert!(!root.join("src/novo.rs").exists(), "the new file left the disk");
        assert_eq!(pending(), "", "the disk and the index are back: {refused}");
        assert_eq!(delivered_count(root), 0);

        std::fs::remove_file(&refuse).unwrap();
        let first = round(root, "x", None);
        assert_eq!(first["ok"], json!(true), "{first}");
        assert_eq!(committed(), ["feat(onda-1): a onda 1 sai", "src/a.rs", "src/novo.rs"], "{first}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn um() {}\n// onda 1\n");
        assert_eq!(pending(), "", "{first}");

        report(2, &["src/b.rs"]);
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(committed(), ["feat(onda-2): a onda 2 sai", "src/b.rs"], "{went}");
        assert_eq!(pending(), "", "{went}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let commits: Vec<Vec<u64>> = log.visible().iter().filter(|e| e.event_type == "commit").map(|e| e.ints("waves")).collect();
        assert_eq!(commits, [vec![1], vec![2]], "each wave committed once: {went}");
        assert_eq!(delivered_count(root), 2, "each delivery recorded once");
        assert!(!copy(1).exists() && !copy(2).exists(), "both copies are gone: {went}");
        let seen = std::fs::read_to_string(&staged).unwrap_or_default();
        assert_eq!(seen, "commit\ncommit\ncommit\n", "nothing of the round was staged while a commit ran");
    }

    /// O conserto que diz as ondas que fecha grava a entrega também nelas,
    /// sem mandá-las refazer nem pedir revisão nenhuma; o commit é de
    /// conserto e leva as ondas consertadas.
    #[test]
    fn a_fix_records_the_delivery_on_the_waves_it_closes_and_asks_their_review_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":1}"#).unwrap();
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let back = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&back, "dispatch"), vec![2], "{back}");
        // A onda 1 é reprovada com a única vaga ocupada pela 2: ela espera na
        // fila, e quem entrega o conserto dela é a 2.
        let rejected = round(root, "x", Some(&verdict(root, 1, "rejected", "faltou o commit")));
        assert_eq!(waves_in(&rejected, "dispatch"), Vec::<u64>::new(), "{rejected}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(waves_to_redo(&log).contains(&1), "the rejected wave waits in the queue");

        std::fs::write(root.join("src/b.rs"), "fn um() {}\nfn conserto() {}\n").unwrap();
        let fix = json!({"wave": 2, "text": "Consertei a onda 1.", "files": ["src/b.rs"],
            "commit": "o commit sai do resumo", "fixes": [1]});
        assert_eq!(returned(root, fix)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(out.get("reviews").is_none(), "{out}");
        assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "the fixed wave is not redone: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(!waves_to_redo(&log).contains(&1), "wave 1's fix already delivered: {out}");
        let (subject, body) = last_commit(root);
        assert_eq!(subject, "fix(onda-2): o commit sai do resumo");
        assert_eq!(body, "- onda 2: o commit sai do resumo (conserta: onda 1)");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let visible = log.visible();
        let fixed_delivery = visible.iter().rfind(|e| e.event_type == "delivered" && e.wave() == Some(1)).unwrap();
        assert_eq!(fixed_delivery.str_field("text"), Some("Consertei a onda 1."));
        let commit = visible.iter().rfind(|e| e.event_type == "commit").unwrap();
        assert_eq!(commit.ints("waves"), vec![2, 1]);
    }

    /// A prova nova de um critério cujo teste mudou de nome vira a versão nova
    /// do critério, com o mesmo resto; a prova nova que sai verde sem rodar
    /// teste nenhum é avisada pelo código do critério. A segunda prova vem da
    /// onda seguinte, que mexe no mesmo arquivo.
    #[test]
    fn a_new_proof_becomes_the_criterions_new_version_and_one_that_runs_no_test_is_warned() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/lib.rs"], &[]), (2, &["src/lib.rs"], &[1])]);
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        round(root, "x", None);
        std::fs::write(
            root.join("src/lib.rs"),
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_nova() { assert_eq!(1 + 1, 2); }\n}\n",
        )
        .unwrap();
        let warned = |out: &Value, reason: &str| {
            out["warnings"]
                .as_array()
                .map(|list| list.iter().any(|w| w["reason"] == json!(reason)))
                .unwrap_or(false)
        };
        let proof = |name: &str| format!("cargo test --lib -- tests::{name} --exact");
        let report = |wave: u64, name: &str, summary: &str| {
            let body = json!({"wave": wave, "text": "O teste mudou de nome.", "files": ["src/lib.rs"],
                "commit": summary, "proofs": [{"criterion": "MSTD-CRIT-0001", "proof": proof(name)}]});
            assert_eq!(returned(root, body)["ok"], json!(true));
        };
        report(1, "soma_nova", "o teste muda de nome");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        // O aviso da onda sem linha de consumo é de outro assunto e sai
        // junto: aqui se olha o da prova que não rodou teste.
        assert!(!warned(&out, "proof-ran-no-test"), "the right name runs a test: {out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let visible = log.visible();
        let criteria: Vec<&&SpecEvent> = visible.iter().filter(|e| e.event_type == "criterion").collect();
        assert_eq!(criteria.len(), 1, "the new version replaces the old one");
        assert_eq!(criteria[0].str_field("proof"), Some(proof("soma_nova").as_str()));
        assert_eq!(criteria[0].str_field("when"), Some("a onda roda"));
        assert!(criteria[0].int("replaces").is_some());
        assert_eq!(log.codes()[&criteria[0].id], "MSTD-CRIT-0001");

        std::fs::write(root.join("src/lib.rs"), "#[cfg(test)]\nmod tests {\n    #[test]\n    fn soma_nova() {}\n}\n").unwrap();
        report(2, "soma", "a prova errada");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let expected = translate("round.proof_ran_no_test", Locale::PtBr).replace("{code}", "MSTD-CRIT-0001");
        let rest: Vec<Value> = out["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|w| w["reason"] != json!("usage-missing"))
            .collect();
        assert_eq!(json!(rest), json!([{"reason": "proof-ran-no-test", "hint": expected}]), "{out}");
    }

    /// A rodada avisa a prova verde que não rodou teste: a prova nova de uma
    /// volta roda uma vez depois do commit, a que roda teste não avisa nada,
    /// e a que sai verde sem rodar teste nenhum é avisada pelo código do
    /// critério — sem cargo nenhum envolvido, só o shell. A segunda prova vem
    /// da onda seguinte, que mexe no mesmo arquivo.
    #[test]
    fn a_rodada_avisa_a_prova_verde_que_nao_rodou_teste() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/lib.rs"], &[]), (2, &["src/lib.rs"], &[1])]);
        round(root, "x", None);

        let report = |wave: u64, proof: &str, summary: &str| {
            std::fs::write(root.join("src/lib.rs"), format!("// {proof}\n")).unwrap();
            let body = json!({"wave": wave, "text": "A prova muda.", "files": ["src/lib.rs"],
                "commit": summary, "proofs": [{"criterion": "MSTD-CRIT-0001", "proof": proof}]});
            assert_eq!(returned(root, body)["ok"], json!(true));
        };

        let warned = |out: &Value, reason: &str| {
            out["warnings"]
                .as_array()
                .map(|list| list.iter().any(|w| w["reason"] == json!(reason)))
                .unwrap_or(false)
        };
        report(1, "echo running 1 test", "a prova roda teste");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(!warned(&out, "proof-ran-no-test"), "a prova que roda teste não avisa: {out}");

        report(2, "echo running 0 tests", "a prova não roda teste");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let expected = translate("round.proof_ran_no_test", Locale::PtBr).replace("{code}", "MSTD-CRIT-0001");
        let rest: Vec<Value> = out["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|w| w["reason"] != json!("usage-missing"))
            .collect();
        assert_eq!(json!(rest), json!([{"reason": "proof-ran-no-test", "hint": expected}]), "{out}");
    }

    /// A conferência antes do git é a da gravação inteira, contra a spec de
    /// agora, e não só a que a volta passou ao ser gravada: o veredito final
    /// gravado antes de uma regra nova do projeto todo não responde mais por
    /// todo o combinado, e é recusado antes do commit, sem commit e sem nada
    /// gravado; o veredito gravado de novo, com a regra, faz o commit e grava
    /// a entrega uma vez só.
    #[test]
    fn a_line_the_spec_would_refuse_is_refused_before_the_commit() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let head = || {
            let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        delivered(root, 1, "A soma saiu.", &["src/a.rs"]);
        seed_review(root);
        let judged_with = |agreed: Value| {
            judged(root, json!({"result": "approved", "final": true, "text": "passou", "agreed": agreed,
                "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]}))
        };
        assert_eq!(judged_with(json!([]))["ok"], json!(true), "sem item combinado, o veredito entra");
        let said = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
            .visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        write(root, "x", "rule", json!({"text": "Vale sempre: o nome é curto.", "example": "e", "keys": ["k"],
            "applies_to": {"files": ["**"]}, "origin": said}));
        let (seed, before) = (head(), spec_lines());

        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("agreed-items-missing"), "{refused}");
        assert_eq!(head(), seed, "nothing was committed: {refused}");
        assert_eq!(spec_lines(), before, "nothing was recorded: {refused}");

        let again = judged_with(json!([{"item": "MSTD-RULE-0001", "met": true}]));
        assert_eq!(again["ok"], json!(true), "{again}");
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_ne!(head(), seed, "the corrected call commits: {went}");
        assert_eq!(delivered_count(root), 1, "the corrected call records the delivery once");
    }

    /// Todo agente do Mustard sai em Opus: a onda de várias tarefas e a de
    /// uma só saem com o modelo pedido no campo `model` do envio gravado, e o
    /// pedido que o agente recebe diz o mesmo na linha do modelo, nos dois
    /// idiomas. Nem o envio nem o pedido voltam a falar de Sonnet.
    #[test]
    fn a_onda_que_implementa_sai_em_opus() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            // A onda 1 leva duas tarefas e a onda 2 leva uma só: as duas saem
            // no mesmo despacho, ao mesmo agente de onda.
            approved_with(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])], |said| {
                write(
                    root,
                    "x",
                    "task",
                    json!({"wave": 1, "text": "A segunda tarefa da onda 1.", "files": [{"path": "src/a.rs"}],
                        "depends_on": [], "origin": said}),
                );
            });
            let config = format!(r#"{{"language":{{"text":"{}"}}}}"#, lang.as_str());
            std::fs::write(root.join("mustard.json"), config).unwrap();

            let out = round(root, "x", None);
            assert_eq!(waves_in(&out, "dispatch"), vec![1, 2], "{out}");
            let said = translate("prompt.model.wave", lang);
            assert!(said.contains("Opus") && !said.contains("Sonnet"), "the model line still names Sonnet: {said}");
            for at in 0..2 {
                let prompt = out["dispatch"][at]["prompt"].as_str().unwrap_or_default();
                assert!(prompt.contains(said), "the {lang:?} request does not say the model: {prompt}");
                assert!(!prompt.contains("Sonnet"), "the {lang:?} request still names Sonnet: {prompt}");
            }

            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            let sends: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "send").collect();
            assert_eq!(sends.len(), 2, "both waves were dispatched: {sends:?}");
            for sent in &sends {
                assert_eq!(sent.str_field("model"), Some("Opus"), "the send carries the requested model: {sent:?}");
            }
            let agents: Vec<_> = (0..2).map(|at| out["dispatch"][at]["agent"].as_str().unwrap_or_default()).collect();
            assert_eq!(agents, vec!["wave", "wave"], "every wave goes to the wave agent, whatever its size: {out}");
        }
    }

    /// O envio da onda guarda o nome do agente e o modelo pedido, na hora do
    /// despacho; quando a rodada assume a volta da onda com a linha `USAGE`
    /// que só o orquestrador escreve, com o modelo usado, os passos, os tokens
    /// e o consumo de quem despacha, o envio ganha uma versão nova com esses
    /// cinco campos, apontando para o envio original e mantendo o agente e o
    /// modelo que já estavam lá.
    #[test]
    fn a_rounds_usage_line_records_a_new_version_of_the_waves_send() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert_eq!(sent.str_field("agent"), Some("wave"), "the send carries the agent's name");
        assert_eq!(sent.str_field("model"), Some("Opus"), "the send carries the requested model");

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        // A volta é só o que o agente grava, sem número nenhum de consumo:
        // quem sabe o consumo é o orquestrador, numa linha à parte.
        let delivery = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, delivery)["ok"], json!(true));
        let usage = line("USAGE", json!({"wave": 1, "model": "Opus", "steps": 42, "tokens": 123_456,
            "caller_steps": 7, "caller_tokens": 89_000}));
        let out = round(root, "x", Some(&usage));
        assert_eq!(out["ok"], json!(true), "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let revised = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert_eq!(revised.replaced(), vec![sent.id], "the new version points to the original send");
        assert_eq!(revised.str_field("model_used"), Some("Opus"), "{revised:?}");
        assert_eq!(revised.int("steps"), Some(42), "{revised:?}");
        assert_eq!(revised.int("tokens"), Some(123_456), "{revised:?}");
        assert_eq!(revised.int("caller_steps"), Some(7), "{revised:?}");
        assert_eq!(revised.int("caller_tokens"), Some(89_000), "{revised:?}");
        assert_eq!(revised.str_field("agent"), Some("wave"), "keeps what was already there");
        assert_eq!(revised.str_field("model"), Some("Opus"), "keeps what was already there");
    }

    /// Um número de consumo que o próprio agente escreve dentro da volta — sem
    /// a linha `USAGE` do orquestrador — não vira consumo nenhum: a gravação
    /// recusa o campo que a entrega não tem, nada é escrito, e o envio da onda
    /// não ganha versão nova, porque só a linha que o orquestrador escreve
    /// conta.
    #[test]
    fn a_number_typed_by_the_agent_inside_delivered_is_not_accepted_as_usage() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::create_dir_all(root.join(".claude/agents/mustard")).unwrap();
        std::fs::write(root.join(".claude/agents/mustard/wave.md"), "molde da onda").unwrap();
        round(root, "x", None);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        // O agente grava os mesmos nomes de campo, mas dentro da própria
        // volta, sem a linha `USAGE`: a gravação os recusa.
        let delivery = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a onda 1 saiu", "model": "Opus", "steps": 42, "tokens": 123_456});
        let refused = returned(root, delivery);
        assert_eq!(refused["reason"], json!("unknown-field"), "{refused}");
        assert_eq!(written_deliveries(root), 0, "{refused}");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sends: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "send" && e.wave() == Some(1)).collect();
        assert_eq!(sends.len(), 1, "no new version of the send was recorded: {sends:?}");
        assert_eq!(sends[0].id, sent.id, "the send is still the original one");
        assert!(sends[0].str_field("model_used").is_none(), "{sends:?}");
        assert!(sends[0].int("tokens").is_none(), "{sends:?}");
    }

    /// A rodada lê o consumo só da linha própria: assumida a volta da onda sem
    /// a linha `USAGE` do orquestrador, o envio da onda não ganha versão nova
    /// nenhuma.
    #[test]
    fn o_consumo_vem_so_da_linha_propria() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::create_dir_all(root.join(".claude/agents/mustard")).unwrap();
        std::fs::write(root.join(".claude/agents/mustard/wave.md"), "molde da onda").unwrap();
        round(root, "x", None);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let delivery = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, delivery)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(delivered_count(root), 1, "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sends: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "send" && e.wave() == Some(1)).collect();
        assert_eq!(sends.len(), 1, "sem a linha própria, o envio não ganha versão nova: {sends:?}");
        assert_eq!(sends[0].id, sent.id, "o envio segue o mesmo de antes");
        assert!(sends[0].int("tokens").is_none(), "sem a linha, consumo nenhum: {sends:?}");
        assert!(sends[0].int("steps").is_none(), "sem a linha, consumo nenhum: {sends:?}");
    }

    /// A linha de consumo pode faltar — a entrega é gravada do mesmo jeito,
    /// porque o consumo não é dela —, mas nunca em silêncio: a resposta da
    /// rodada avisa, nomeando a onda, que o gasto daquela onda não entrou na
    /// página. Com a linha, aviso nenhum.
    #[test]
    fn a_entrega_sem_linha_de_consumo_avisa_na_resposta() {
        let sem = tempdir().unwrap();
        let root = sem.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let out = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(true), "a entrega não é recusada por falta de consumo: {out}");
        assert_eq!(delivered_count(root), 1, "a entrega foi gravada: {out}");
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        let silent: Vec<u64> = warnings
            .iter()
            .filter(|w| w["reason"] == json!("usage-missing"))
            .filter_map(|w| w["wave"].as_u64())
            .collect();
        assert_eq!(silent, vec![1], "o aviso nomeia a onda que ficou sem consumo: {out}");
        let hint = warnings
            .iter()
            .find(|w| w["reason"] == json!("usage-missing"))
            .and_then(|w| w["hint"].as_str())
            .unwrap_or_default()
            .to_string();
        assert!(hint.contains(" 1 "), "o aviso diz de qual onda fala: {hint}");

        // A mesma entrega com a linha de consumo: o gasto entra na página e
        // não há o que avisar.
        let com = tempdir().unwrap();
        let root = com.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let delivery = delivered(root, 1, "A soma saiu.", &["src/a.rs"]);
        let usage = line("USAGE", json!({"wave": 1, "model": "Opus", "steps": 42, "tokens": 123_456,
            "caller_steps": 7, "caller_tokens": 89_000}));
        let out = round(root, "x", Some(&format!("{delivery}\n{usage}")));
        assert_eq!(out["ok"], json!(true), "{out}");
        let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.iter().all(|w| w["reason"] != json!("usage-missing")), "com a linha, aviso nenhum: {out}");
    }

    /// Duas provas do mesmo critério viram um comando só, ligado por `&&`:
    /// o nome solto de um teste ali dentro daria um comando que o shell não
    /// acha, e que só estouraria na rodada seguinte. A gravação da volta
    /// recusa na hora, nomeando o critério e dizendo que o campo é uma linha
    /// de comando, e nada é gravado nem comitado. Com as duas provas escritas
    /// como comando, a junção sai e o critério ganha uma versão só.
    #[test]
    fn duas_provas_do_mesmo_criterio_nao_viram_comando_invalido() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let head_before = git_text(root, &["rev-parse", "HEAD"]);

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let body = |proofs: Value| {
            json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu",
                "proofs": proofs})
        };
        let names = body(json!([
            {"criterion": "MSTD-CRIT-0001", "proof": "a_soma_sai_certa"},
            {"criterion": "MSTD-CRIT-0001", "proof": "a_dobra_sai_certa"},
        ]));
        let refused = returned(root, names);
        assert_eq!(refused["reason"], json!("proof-not-a-command"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default().to_string();
        assert!(hint.contains("MSTD-CRIT-0001"), "a recusa nomeia o critério: {hint}");
        assert!(hint.contains("a_soma_sai_certa"), "a recusa mostra o texto que veio no lugar: {hint}");
        assert_eq!(written_deliveries(root), 0, "nada foi gravado: {refused}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head_before, "nada foi comitado: {refused}");

        // As mesmas duas provas escritas como comando passam, e o critério
        // fica com uma linha de comando só.
        let commands = body(json!([
            {"criterion": "MSTD-CRIT-0001", "proof": "cargo test -p x a_soma_sai_certa"},
            {"criterion": "MSTD-CRIT-0001", "proof": "cargo test -p x a_dobra_sai_certa"},
        ]));
        assert_eq!(returned(root, commands)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let current = log.visible().into_iter().find(|e| e.event_type == "criterion").expect("o critério");
        assert_eq!(
            current.str_field("proof"),
            Some("cargo test -p x a_soma_sai_certa && cargo test -p x a_dobra_sai_certa"),
            "as duas provas viram um comando só"
        );
    }

    /// A versão nova do critério que a onda `wave` do log em `root` cobre,
    /// com o comando `proof` no lugar do antigo — como um conserto de código
    /// quebraria uma prova que antes passava, ou como ela já nasceria
    /// mal-escrita. Devolve o código dela.
    fn reprove_wave_criterion(root: &Path, wave: u64, proof: &str) -> String {
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let said = log.visible().into_iter().find(|e| e.event_type == "message").unwrap().id;
        let crit = log.wave_criteria(wave)[0];
        let code = log.codes().get(&crit.id).cloned().unwrap_or_default();
        let mut fields = crit.fields.clone();
        for key in ["v", "id", "code", "at", "type", "search", "author", "replaces"] {
            fields.remove(key);
        }
        let mut body = Value::Object(fields);
        body["proof"] = json!(proof);
        body["replaces"] = json!(crit.id);
        body["origin"] = json!(said);
        write(root, "x", "criterion", body);
        code
    }

    /// A rodada roda a prova de cada critério que as ondas do relatório
    /// cobrem antes de comitar: a que falha recusa a entrega, nomeando o
    /// critério, o comando inteiro e a saída de erro, e nada é comitado nem
    /// gravado — nem a entrega da onda, nem o commit no repositório
    /// principal.
    #[test]
    fn a_prova_do_criterio_roda_na_volta_da_onda() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        // A prova do critério que a onda cobre passa a falhar — como uma
        // mudança de código quebraria uma prova que antes passava.
        let code = reprove_wave_criterion(root, 1, "git --nao-existe-esta-opcao");
        let head_before = git_text(root, &["rev-parse", "HEAD"]);

        let out = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("round-criterion-proof-failed"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&code), "a recusa nomeia o critério: {hint}");
        assert!(hint.contains("git --nao-existe-esta-opcao"), "a recusa nomeia o comando: {hint}");
        assert!(hint.contains("nao-existe-esta-opcao"), "a recusa traz a saída de erro: {hint}");

        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head_before, "nada foi comitado: {out}");
        assert_eq!(delivered_count(root), 0, "nada da entrega foi gravado: {out}");
    }

    /// A prova que não executa por estar mal-escrita — um comando do cargo
    /// com vários nomes de teste em sequência, sem o separador `--`, que o
    /// cargo recusa antes de rodar teste nenhum — recusa na volta da mesma
    /// onda, e não só horas depois no fechamento: o texto traz o critério, o
    /// comando inteiro e a saída de erro do cargo.
    #[test]
    fn a_prova_mal_escrita_e_recusada_na_volta_da_onda() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"prova\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn soma(a: u32, b: u32) -> u32 { a + b }\n").unwrap();
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "cargo"]);
        round(root, "x", None);

        // Um comando de teste com quatro nomes em sequência, sem o `--`: o
        // cargo recusa o argumento antes de rodar teste nenhum.
        let bad = "cargo test soma_1 soma_2 soma_3 soma_4";
        let code = reprove_wave_criterion(root, 1, bad);
        let head_before = git_text(root, &["rev-parse", "HEAD"]);

        let out = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("round-criterion-proof-failed"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&code), "a recusa nomeia o critério: {hint}");
        assert!(hint.contains(bad), "a recusa nomeia o comando inteiro: {hint}");
        assert!(hint.contains("unexpected argument") || hint.contains("soma_2"), "a saída de erro vem inteira: {hint}");

        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head_before, "nada foi comitado: {out}");
        assert_eq!(delivered_count(root), 0, "nada da entrega foi gravado: {out}");
    }

    /// As linhas do arquivo da spec `x`: a gravação recusada não escreve
    /// nenhuma.
    fn spec_lines(root: &Path) -> usize {
        std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count()
    }

    /// O agente só grava a entrega da onda com envio aberto: a onda que ainda
    /// não saiu é recusada com o texto combinado, sem escrever nada; a que
    /// saiu grava a volta, fora da leitura, e a resposta traz só o número, o
    /// tipo e o código. Depois que a rodada assume a volta, o envio fecha, e
    /// a mesma onda não grava outra.
    #[test]
    fn o_agente_so_grava_a_entrega_com_envio_aberto() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1], "a onda 2 espera a 1");
        let before = spec_lines(root);

        let early = returned(root, json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "b sai"}));
        assert_eq!(early["reason"], json!("no-open-send"), "{early}");
        assert_eq!(early["hint"], json!("Não há envio aberto para a onda 2. Nada foi gravado."), "{early}");
        assert_eq!(spec_lines(root), before, "nada foi gravado: {early}");

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let wrote = returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai"}));
        let id = wrote["id"].as_u64().unwrap_or_else(|| panic!("{wrote}"));
        assert_eq!(wrote, json!({"ok": true, "spec": "x", "id": id, "type": "delivered", "code": "MSTD-DELIV-0001"}));
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let back = log.get(id).expect("a volta gravada");
        assert!(back.returned(), "{:?}", back.fields);
        assert_eq!(back.str_field("author"), Some("wave"), "{:?}", back.fields);
        assert_eq!(delivered_count(root), 0, "a volta fica fora da leitura até a rodada assumir");

        assert_eq!(round(root, "x", None)["ok"], json!(true));
        assert_eq!(delivered_count(root), 1);
        let after = spec_lines(root);
        let again = returned(root, json!({"wave": 1, "text": "De novo.", "files": ["src/a.rs"], "commit": "a soma sai"}));
        assert_eq!(again["reason"], json!("no-open-send"), "o envio fechou com a entrega oficial: {again}");
        assert_eq!(spec_lines(root), after, "{again}");
    }

    /// A volta que a onda grava enquanto a rodada assume a mesma onda — com
    /// as voltas já lidas e a entrega oficial ainda por gravar — espera a
    /// trava do passo do git, que a rodada segura, e é conferida depois: o
    /// envio já fechou com a entrega oficial, e a gravação é recusada sem
    /// escrever nada. Nenhuma volta fica esperando, e a rodada seguinte não
    /// grava uma segunda entrega da onda.
    #[cfg(unix)]
    #[test]
    fn a_volta_gravada_durante_a_rodada_nao_gera_segunda_entrega() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let first = id_of(&returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai"})));
        // O gancho do commit da rodada marca que começou e espera a soltura:
        // é o trecho entre a leitura das voltas e a entrega oficial.
        let hooks = root.join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let (started, release) = (hooks.join("comecou"), hooks.join("solta"));
        let hook = hooks.join("pre-commit");
        let script = format!(
            "#!/bin/sh\ntouch '{}'\nn=0\nwhile [ ! -f '{}' ] && [ $n -lt 600 ]; do sleep 0.05; n=$((n+1)); done\n",
            started.display(),
            release.display()
        );
        std::fs::write(&hook, script).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);

        let (out, late, written_while_held) = std::thread::scope(|scope| {
            let going = scope.spawn(|| round(root, "x", None));
            for _ in 0..3000 {
                if started.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(started.exists(), "o gancho do commit não começou");
            let (wrote_tx, wrote_rx) = mpsc::channel();
            let late = scope.spawn(move || {
                let wrote = returned(root, json!({"wave": 1, "text": "Conferi de novo.", "commit": "a soma sai"}));
                let _ = wrote_tx.send(());
                wrote
            });
            let written_while_held = wrote_rx.recv_timeout(Duration::from_millis(1500)).is_ok();
            std::fs::write(&release, b"").unwrap();
            (going.join().unwrap(), late.join().unwrap(), written_while_held)
        });
        assert_eq!(out["ok"], json!(true), "{out}");
        let head = git_text(root, &["rev-parse", "HEAD"]);
        let next = round(root, "x", None);
        assert_eq!(delivered_count(root), 1, "a rodada seguinte não grava outra entrega da onda: {next}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "nem comita de novo: {next}");
        assert_eq!(late["reason"], json!("no-open-send"), "a volta é conferida depois da entrega oficial: {late}");
        assert!(!written_while_held, "a volta espera a rodada soltar a trava: {late}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        assert!(log.unassumed_returns().is_empty(), "nenhuma volta espera outra rodada");
        let official = log.visible().into_iter().find(|e| e.event_type == "delivered").expect("a entrega oficial");
        assert_eq!(official.replaced(), vec![first], "{:?}", official.fields);
    }

    /// O título do commit que a rodada montará da volta é conferido na
    /// gravação dela: com o escopo de uma onda, o resumo que leva o título a
    /// 61 caracteres é recusado e nada é gravado; o que o leva a 60 entra.
    #[test]
    fn a_volta_com_titulo_longo_e_recusada_na_gravacao() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let before = spec_lines(root);
        // `feat(onda-1): ` tem 14 caracteres.
        let body = |summary: String| json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": summary});

        let long = returned(root, body("a".repeat(47)));
        assert_eq!(long["reason"], json!("commit-too-long"), "{long}");
        assert!(long["hint"].as_str().unwrap_or_default().contains("60"), "{long}");
        assert_eq!(spec_lines(root), before, "nada foi gravado: {long}");

        let fits = returned(root, body("a".repeat(46)));
        assert_eq!(fits["ok"], json!(true), "{fits}");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(last_commit(root).0.chars().count(), 60, "{out}");
    }

    /// Sem volta gravada e com o Claude Code da onda aberto, a linha de
    /// consumo não comita nada e pede que o agente grave a entrega. Com duas
    /// voltas gravadas, a rodada assume a última: grava a entrega oficial,
    /// sem a marca da volta, com `replaces` para as duas, comita com o resumo
    /// dela e grava o consumo na versão nova do envio.
    #[test]
    fn a_rodada_assume_a_entrega_gravada_e_comita_com_o_resumo_dela() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let seed = git_text(root, &["rev-parse", "HEAD"]);
        let usage = line("USAGE", json!({"wave": 1, "model": "Opus", "steps": 3, "tokens": 900}));

        let before = spec_lines(root);
        let asked = round(root, "x", Some(&usage));
        assert_eq!(asked["reason"], json!("round-return-missing"), "{asked}");
        let hint = translate("spec_events.return_missing", Locale::PtBr).replace("{wave}", "1");
        assert_eq!(asked["hint"], json!(hint), "{asked}");
        assert_eq!((git_text(root, &["rev-parse", "HEAD"]), spec_lines(root)), (seed.clone(), before), "{asked}");

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let first = id_of(&returned(root, json!({"wave": 1, "text": "Primeira volta.", "files": ["src/a.rs"],
            "commit": "a primeira sai"})));
        let last = id_of(&returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai"})));
        let out = round(root, "x", Some(&usage));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(last_commit(root).0, "feat(onda-1): a soma sai", "{out}");
        assert_eq!(out["commit"]["title"], json!("feat(onda-1): a soma sai"), "{out}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD~1"]), seed, "um commit só: {out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let visible = log.visible();
        let official: Vec<_> = visible.iter().filter(|e| e.event_type == "delivered").collect();
        assert_eq!(official.len(), 1, "{official:?}");
        assert_eq!(official[0].str_field("text"), Some("A soma saiu."), "a última volta conta");
        assert!(!official[0].returned(), "{:?}", official[0].fields);
        assert_eq!(official[0].replaced(), vec![first, last], "{:?}", official[0].fields);
        assert!(log.unassumed_returns().is_empty(), "nenhuma volta espera mais");
        let sent = visible.iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert_eq!((sent.int("steps"), sent.int("tokens")), (Some(3), Some(900)), "{:?}", sent.fields);
    }

    /// A volta que a onda grava nunca vai para a cópia da página: a rodada
    /// que a assume manda copiar a entrega oficial, e a volta fica de fora.
    #[test]
    fn a_volta_fica_fora_da_copia_da_pagina() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let back = id_of(&returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai"})));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let official = log.visible().into_iter().find(|e| e.event_type == "delivered").expect("a entrega oficial").id;
        let items = crate::commands::spec_events::pages::copy::sent_items(root, &out);
        assert!(items.contains(&official), "a entrega oficial vai para a cópia: {items:?}");
        assert!(!items.contains(&back), "a volta fica de fora: {items:?}");
    }

    /// A volta gravada antes de um reenvio da onda não conta: a rodada assume
    /// a última volta depois do último envio, e a entrega oficial substitui
    /// só as voltas desse envio.
    #[test]
    fn a_volta_depois_do_reenvio_e_a_que_conta() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// Antes do reenvio.\n").unwrap();
        let old = id_of(&returned(root, json!({"wave": 1, "text": "Antes do reenvio.", "files": ["src/a.rs"],
            "commit": "a volta velha"})));
        seed_send(root, 1);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// Depois do reenvio.\n").unwrap();
        let new = id_of(&returned(root, json!({"wave": 1, "text": "Depois do reenvio.", "files": ["src/a.rs"],
            "commit": "a volta nova"})));

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(last_commit(root).0, "feat(onda-1): a volta nova", "{out}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let official = log.visible().into_iter().find(|e| e.event_type == "delivered").expect("a entrega oficial");
        assert_eq!(official.str_field("text"), Some("Depois do reenvio."));
        assert_eq!(official.replaced(), vec![new], "{:?}", official.fields);
        assert!(!official.replaced().contains(&old));
        assert!(log.unassumed_returns().is_empty(), "a volta velha não espera mais");
    }

    /// A linha de consumo de uma onda que uma rodada anterior já assumiu, sem
    /// volta nova, completa o envio dela: a versão nova do envio ganha o
    /// consumo, e nada mais é juntado, comitado nem gravado como entrega.
    #[test]
    fn a_linha_de_consumo_sozinha_completa_a_onda_ja_assumida() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let out = round(root, "x", Some(&delivered(root, 1, "A soma saiu.", &["src/a.rs"])));
        assert_eq!(out["ok"], json!(true), "{out}");
        let head = git_text(root, &["rev-parse", "HEAD"]);

        let usage = line("USAGE", json!({"wave": 1, "model": "Opus", "steps": 5, "tokens": 1_200,
            "caller_steps": 2, "caller_tokens": 300}));
        let completed = round(root, "x", Some(&usage));
        assert_eq!(completed["ok"], json!(true), "{completed}");
        assert!(completed.get("commit").is_none(), "{completed}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "nada foi comitado: {completed}");
        assert_eq!(delivered_count(root), 1, "nenhuma entrega nova: {completed}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).unwrap();
        assert_eq!(sent.str_field("model_used"), Some("Opus"), "{:?}", sent.fields);
        assert_eq!((sent.int("steps"), sent.int("tokens")), (Some(5), Some(1_200)), "{:?}", sent.fields);
        assert_eq!((sent.int("caller_steps"), sent.int("caller_tokens")), (Some(2), Some(300)), "{:?}", sent.fields);
        assert_eq!(sent.replaced().len(), 1, "a versão nova aponta o envio anterior: {:?}", sent.fields);
    }

    /// A linha da entrega colada no relatório é recusada com o texto
    /// combinado, junto de qualquer outra linha, e nada é gravado nem
    /// comitado: a entrega mora na spec.
    #[test]
    fn a_linha_de_entrega_no_relatorio_e_recusada() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let (head, before) = (git_text(root, &["rev-parse", "HEAD"]), spec_lines(root));

        let pasted = line("DELIVERED", json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai"}));
        let usage = line("USAGE", json!({"wave": 1, "model": "Opus", "steps": 3, "tokens": 900}));
        let refused = round(root, "x", Some(&format!("{pasted}\n{usage}")));
        assert_eq!(refused["reason"], json!("round-return-line"), "{refused}");
        assert_eq!(
            refused["hint"],
            json!("A entrega e o veredito moram na spec: o agente os grava com mustard-rt run write. O relatório \
                   leva só as linhas USAGE, PAUSED e ANALYSIS."),
            "{refused}"
        );
        assert_eq!((git_text(root, &["rev-parse", "HEAD"]), spec_lines(root)), (head, before), "{refused}");
    }

    /// A linha do veredito colada no relatório é recusada com o texto
    /// combinado, na rodada e no fechamento, junto de qualquer outra linha, e
    /// nada é gravado nem comitado: o veredito mora na spec, e é o revisor
    /// quem o grava.
    #[test]
    fn a_linha_de_veredito_no_relatorio_e_recusada() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        delivered(root, 1, "A soma saiu.", &["src/a.rs"]);
        seed_review(root);
        let (head, before) = (git_text(root, &["rev-parse", "HEAD"]), spec_lines(root));

        let pasted = line("VERDICT", json!({"final": true, "result": "approved", "text": "Sem achados."}));
        let usage = line("USAGE", json!({"wave": 1, "model": "Opus", "steps": 3, "tokens": 900}));
        let expected = json!("A entrega e o veredito moram na spec: o agente os grava com mustard-rt run write. O \
                              relatório leva só as linhas USAGE, PAUSED e ANALYSIS.");
        let refused = round(root, "x", Some(&format!("{pasted}\n{usage}")));
        assert_eq!(refused["reason"], json!("round-return-line"), "{refused}");
        assert_eq!(refused["hint"], expected, "{refused}");
        let closing = crate::commands::flow::close::close_at(&crate::commands::flow::close::CloseOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            report: Some(pasted),
            ..Default::default()
        });
        assert_eq!(closing["reason"], json!("round-return-line"), "{closing}");
        assert_eq!(closing["hint"], expected, "{closing}");
        assert_eq!((git_text(root, &["rev-parse", "HEAD"]), spec_lines(root)), (head, before), "{refused}");
    }

    /// Cada sobra da volta assumida vira pendência da spec, pela porta do
    /// `pending --add`, ligada à spec do checkout; a sobra com o título de
    /// uma pendência aberta devolve a aberta, sem duplicar. A sobra sem
    /// título é recusada na gravação, com o texto combinado, e nada é
    /// gravado.
    #[test]
    fn a_sobra_relatada_pela_onda_vira_pendencia_da_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        git_at(root, &["checkout", "-q", "-b", "feature/x"]);
        round(root, "x", None);
        let add = |title: &str| {
            pending_at(&PendingOpts {
                root: root.to_path_buf(),
                add: true,
                title: Some(title.to_string()),
                detail: Some("Já estava aberta.".to_string()),
                ..PendingOpts::default()
            })
        };
        let open = add("A busca ignora acento");
        let open_id = open["id"].as_str().unwrap_or_else(|| panic!("{open}")).to_string();

        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();
        let before = spec_lines(root);
        let untitled = returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai", "leftovers": [{"detail": "Sem título."}]}));
        assert_eq!(untitled["reason"], json!("leftover-field-missing"), "{untitled}");
        assert_eq!(
            untitled["hint"],
            json!("Falta o campo title numa sobra (leftovers) da entrega. Nada foi gravado."),
            "{untitled}"
        );
        assert_eq!(spec_lines(root), before, "nada foi gravado: {untitled}");

        let wrote = returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai", "leftovers": [
                {"title": "O log não gira", "detail": "O arquivo de log cresce sem limite."},
                {"title": "a busca ignora ACENTO", "detail": "Achei de novo."},
            ]}));
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let opened: Vec<&Value> =
            out["recorded"].as_array().map(|all| all.iter().filter(|r| r["type"] == json!("pending")).collect()).unwrap_or_default();
        assert_eq!(opened.len(), 2, "{out}");
        let new_id = opened[0]["id"].as_str().unwrap_or_default().to_string();
        assert_ne!(new_id, open_id, "{out}");
        assert!(opened[0]["question"].as_str().unwrap_or_default().contains("O log não gira"), "{out}");
        assert_eq!((opened[1]["id"].as_str(), opened[1]["open"].as_bool()), (Some(open_id.as_str()), Some(true)), "{out}");

        let listed = pending_at(&PendingOpts { root: root.to_path_buf(), ..PendingOpts::default() });
        let titles: Vec<String> = listed["open"]
            .as_array()
            .map(|all| all.iter().filter_map(|i| i["title"].as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert_eq!(titles, ["A busca ignora acento", "O log não gira"], "{listed}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let number: u64 = new_id.trim_start_matches("P-").parse().unwrap();
        assert!(
            log.visible().iter().any(|e| e.event_type == "deferred" && e.int("pending") == Some(number)),
            "a pendência nova fica ligada à spec"
        );
    }

    /// A volta de uma onda com três sobras, assumida pela rodada: a que
    /// quebra vira tarefa da spec no backlog, com o arquivo que o detalhe
    /// cita e que existe, sem pergunta; a cosmética vira pendência do
    /// projeto, na lista de depois, com a onda que a apontou e sem pergunta;
    /// a sem `kind` vira pendência da spec com a pergunta de destino. A volta
    /// com um `kind` de outro valor é recusada na gravação, e nada é gravado.
    #[test]
    fn a_breaking_leftover_becomes_a_task_and_a_cosmetic_one_goes_to_the_later_list_without_a_question() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/c.rs"), "fn tres() {}\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        git_at(root, &["checkout", "-q", "-b", "feature/x"]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\n// A soma saiu.\n").unwrap();

        let before = spec_lines(root);
        let odd = returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai", "leftovers": [
                {"title": "O log não gira", "detail": "O arquivo de log cresce sem limite."},
                {"title": "Depois eu vejo", "detail": "Talvez.", "kind": "later"},
            ]}));
        assert_eq!(odd["reason"], json!("invalid-value"), "{odd}");
        let hint = odd["hint"].as_str().unwrap_or_default();
        for said in ["leftovers[2].kind", "breaks", "cosmetic"] {
            assert!(hint.contains(said), "a recusa não diz `{said}`: {odd}");
        }
        assert_eq!(spec_lines(root), before, "nada foi gravado: {odd}");

        let breaks = "Sem o índice, `src/c.rs` para de ler o arquivo; `src/nao_existe.rs` também.";
        let wrote = returned(root, json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai", "leftovers": [
                {"title": "A leitura para sem o índice", "detail": breaks, "kind": "breaks"},
                {"title": "O nome da variável confunde", "detail": "Um nome mais claro.", "kind": "cosmetic"},
                {"title": "O log não gira", "detail": "O arquivo de log cresce sem limite."},
            ]}));
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let recorded = |kind: &str| -> Vec<Value> {
            out["recorded"].as_array().into_iter().flatten().filter(|r| r["type"] == json!(kind)).cloned().collect()
        };

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let tasks = recorded("task");
        assert_eq!(tasks.len(), 1, "{out}");
        assert_eq!(tasks[0]["wave"], json!(1), "{out}");
        let task = log.get(tasks[0]["id"].as_u64().unwrap()).unwrap_or_else(|| panic!("{out}"));
        assert_eq!(task.wave(), None, "a tarefa fica no backlog, sem onda: {:?}", task.fields);
        assert_eq!(task.fields["title"], json!("A leitura para sem o índice"), "{:?}", task.fields);
        assert_eq!(task.fields["text"], json!(breaks), "{:?}", task.fields);
        assert_eq!(task.fields["files"], json!([{"path": "src/c.rs"}]), "{:?}", task.fields);
        assert_eq!(task.fields["depends_on"], json!([]), "{:?}", task.fields);
        assert_eq!(task.fields["author"], json!("wave"), "{:?}", task.fields);

        let pendings = recorded("pending");
        assert_eq!(pendings.len(), 2, "{out}");
        let (cosmetic, unsorted) = (&pendings[0], &pendings[1]);
        assert!(cosmetic.get("question").is_none(), "a cosmética não pergunta: {out}");
        assert_eq!(cosmetic["owner"], json!("project"), "{out}");
        assert!(unsorted["question"].as_str().unwrap_or_default().contains("O log não gira"), "{out}");

        let listed = pending_at(&PendingOpts { root: root.to_path_buf(), ..PendingOpts::default() });
        let item = |title: &str| {
            listed["open"].as_array().into_iter().flatten().find(|i| i["title"] == json!(title)).cloned()
        };
        assert!(item("A leitura para sem o índice").is_none(), "a que quebra não vira pendência: {listed}");
        let later = item("O nome da variável confunde").unwrap_or_else(|| panic!("{listed}"));
        assert_eq!(later["owner"], json!("project"), "{listed}");
        assert_eq!(later["later"], json!("cosmética, apontada pela onda 1"), "{listed}");
        let asked: Vec<String> = crate::commands::event::pending::open_pending_born_in(root, "x")
            .into_iter()
            .map(|p| p.title)
            .collect();
        assert_eq!(asked, ["O log não gira"], "só a sem `kind` espera a pergunta de destino: {listed}");
    }
}
