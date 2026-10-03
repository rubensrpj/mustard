//! `conversation_size` — o bloco de retomada antes de compactar.
//!
//! Um gancho só, [`PrecompactNotice`], no evento `PreCompact`: antes de toda
//! compactação — manual (`/compact`) ou automática, a que o próprio Claude
//! Code dispara sozinho quando a conversa cresce —, a conversa recebe o
//! bloco de retomada ([`resume_block`](crate::commands::flow::resume::resume_block)),
//! o mesmo que o início da sessão coloca sozinho depois do resumo: ninguém
//! precisa colá-lo. Sem controle de "já avisado": o próprio `PreCompact` já é
//! o degrau, então cada compactação merece o bloco de novo, e não há como ele
//! ficar velho.
//!
//! O degrau de 200 mil tokens que media a conversa por conta própria foi
//! retirado, dos dois lugares onde ele agia: a pausa ao agente de onda e a
//! recusa à chamada de quem conduz. Quem cuida do tamanho agora é a
//! compactação automática do Claude Code; este gancho não mede nada, só
//! injeta o bloco sempre que uma compactação vai acontecer, para que a
//! retomada nunca dependa de guardar o número certo no meio do caminho.
//!
//! `None` (e o gancho deixa passar, [`Verdict::Allow`]) sem spec atual, sem
//! arquivo de eventos legível ou com a spec já terminada: sem o que retomar,
//! não há bloco.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;
use mustard_core::platform::i18n::Locale;
use mustard_core::translate;

/// O valor de compactação que esta versão instalada do Mustard recomenda à
/// máquina que ainda não escolheu o seu — verificado contra o binário do
/// Claude Code 2.1.278, que ainda honra `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`
/// (`docs/2026-07-25-revisao-portoes-pipeline-ondas.md`, seção 9). Máquina
/// com valor próprio não o recebe: a escolha dela vale.
const RECOMMENDED_AUTOCOMPACT_PCT: &str = "15";

/// O aviso, antes de compactar: o bloco de retomada, que volta sozinho
/// depois do resumo. Dispara em toda compactação, manual ou automática, sem
/// controle de "já avisado" — o próprio `PreCompact` é o degrau.
pub struct PrecompactNotice;

impl Check for PrecompactNotice {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreCompact) {
            return Ok(Verdict::Allow);
        }
        let root = ctx.workspace_root.clone().unwrap_or_else(|| PathBuf::from(ctx.project_dir_or_cwd(input)));
        let Some(context) = precompact_text(&root, input.session_id.as_deref()) else {
            return Ok(Verdict::Allow);
        };
        Ok(Verdict::Inject { context })
    }
}

/// A linha do valor de compactação no aviso. Com valor escolhido na máquina
/// (a variável `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, presente e não vazia), ela
/// só informa esse valor: não cita o recomendado nem manda trocar. Sem valor,
/// recomenda o [`RECOMMENDED_AUTOCOMPACT_PCT`] e diz onde ajustá-lo.
/// `machine` chega como parâmetro, e não por `std::env::var` direto aqui
/// dentro, para o teste poder variá-lo sem `std::env::set_var` — `unsafe` no
/// Rust 2024 e vedado neste crate.
fn autocompact_line(machine: Option<&str>, lang: Locale) -> String {
    match machine {
        Some(value) if !value.is_empty() => {
            translate("conversation_size.autocompact_set", lang).replace("{machine}", value)
        }
        _ => translate("conversation_size.autocompact", lang).replace("{installed}", RECOMMENDED_AUTOCOMPACT_PCT),
    }
}

/// O aviso antes de compactar: o bloco de retomada da spec atual, dizendo
/// que ele volta sozinho depois do resumo, e o valor de compactação. `None`
/// sem spec atual, sem arquivo de eventos ou com a spec já terminada.
fn precompact_text(root: &Path, session: Option<&str>) -> Option<String> {
    let block = crate::commands::flow::resume::current_block(root, session)?;
    let lang = crate::commands::spec_events::project(root).lang;
    let machine = std::env::var("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE").ok();
    let autocompact = autocompact_line(machine.as_deref(), lang);
    Some(
        translate("conversation_size.precompact", lang)
            .replace("{block}", &block)
            .replace("{autocompact}", &autocompact),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um projeto instalado, com a spec `spec` aprovada e o checkout parado
    /// na branch dela — o mesmo que a chamada de quem conduz vê.
    fn open_project(spec: &str) -> tempfile::TempDir {
        open_project_in(spec, Locale::PtBr)
    }

    /// [`open_project`] com os textos no idioma `lang`.
    fn open_project_in(spec: &str, lang: Locale) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let text = if lang == Locale::EnUs { "en-US" } else { "pt-BR" };
        std::fs::write(root.join("mustard.json"), format!(r#"{{"language":{{"text":"{text}"}}}}"#)).unwrap();
        crate::shared::spec_state::stand_on_spec_branch(root, spec);
        crate::commands::spec_events::write::record_open(root, spec, &format!("feature/{spec}"), "dev")
            .expect("open");
        crate::shared::spec_state::approve_in(&root.join(".claude").join("spec").join(spec));
        dir
    }

    /// Grava a onda 1 da spec `spec`, em `root`, em andamento — o mesmo
    /// pedido que a rodada grava, com o pid deste processo, que segue vivo
    /// durante o teste, para que `waves_in_progress` a conte como rodando, e
    /// a cópia dela em [`copy_of`]. Devolve o número do critério.
    fn seed_running_wave(root: &Path, spec: &str) -> u64 {
        let said =
            crate::shared::spec_state::seed_event(root, spec, "message", serde_json::json!({"author": "user", "text": "o plano"}));
        let crit = crate::shared::spec_state::seed_event(
            root,
            spec,
            "criterion",
            serde_json::json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}),
        );
        crate::shared::spec_state::seed_event(
            root,
            spec,
            "wave",
            serde_json::json!({"n": 1, "text": "Onda 1.", "criteria": [crit], "done_when": "x", "origin": said}),
        );
        crate::shared::spec_state::seed_event(root, spec, "state", serde_json::json!({"phase": "running", "author": "binary"}));
        let (pid, started) = crate::commands::flow::stuck::this_process();
        crate::shared::spec_state::seed_event(
            root,
            spec,
            "send",
            serde_json::json!({"wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit], "mustard": "0", "author": "binary", "copy": copy_of(root, 1),
                "claude_pid": pid, "claude_started": started}),
        );
        crit
    }

    /// A vaga da onda `wave`, como a rodada a grava no envio: a onda n na
    /// n-ésima vaga.
    fn copy_of(root: &Path, wave: u64) -> String {
        let slot = usize::try_from(wave).unwrap() - 1;
        mustard_core::io::wave_prompt::shown(&mustard_core::io::wave_prompt::slot_path(root, "x", slot))
    }

    /// A chamada de ferramenta de quem conduz, com a transcrição `transcript`.
    fn conductor_call(root: &Path, transcript: &Path) -> HookInput {
        HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Bash".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            agent_id: None,
            tool_input: serde_json::json!({ "command": "ls" }),
            raw: serde_json::json!({ "transcript_path": transcript.to_string_lossy() }),
            ..HookInput::default()
        }
    }

    /// Numa máquina que escolheu `25`, o aviso só informa esse valor: não
    /// cita o `15` que a versão instalada recomenda nem manda ajustar. Numa
    /// máquina sem valor (a variável ausente ou vazia), o aviso recomenda o
    /// `15` e diz onde ajustá-lo. Nos dois idiomas.
    #[test]
    fn warning_respects_the_machine_value_and_only_recommends_when_it_has_none() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let set = autocompact_line(Some("25"), lang);
            assert!(set.contains("25"), "{lang:?}: {set}");
            assert!(!set.contains(RECOMMENDED_AUTOCOMPACT_PCT), "{lang:?}: cites the recommended value: {set}");
            for order in ["settings.json", "ajuste", "ponha", "set ", "reload", "recarregue"] {
                assert!(!set.contains(order), "{lang:?}: tells the machine to change its value ({order}): {set}");
            }

            for unset in [autocompact_line(None, lang), autocompact_line(Some(""), lang)] {
                assert!(unset.contains(RECOMMENDED_AUTOCOMPACT_PCT), "{lang:?}: {unset}");
                assert!(unset.contains("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"), "{lang:?}: {unset}");
                assert!(unset.contains("~/.claude/settings.json"), "{lang:?}: {unset}");
            }
        }
    }

    /// O aviso de compactar chega pelo gancho de `PreCompact`, com o bloco de
    /// retomada — spec, fase e o próximo passo —, com onda rodando ou sem: a
    /// linha das ondas em andamento é uma parte do bloco, não um substituto.
    /// E nenhuma chamada de ferramenta é mais recusada por tamanho: o mesmo
    /// registro inteiro, com uma transcrição de 200 mil tokens (o antigo
    /// degrau), deixa passar um `PreToolUse` comum.
    #[test]
    fn compaction_warning_arrives_in_the_hook_and_nobody_else_is_blocked_by_size() {
        let dir = open_project("x");
        let root = dir.path();

        // O caminho de verdade: o evento `PreCompact` inteiro, pelo
        // despachante e pelo registro — não a função auxiliar direto — é
        // quem precisa injetar o bloco.
        let precompact = HookInput {
            hook_event_name: Some("PreCompact".to_string()),
            session_id: Some("s1".to_string()),
            cwd: Some(root.to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        let outcome = crate::dispatch::run_event(Some(Trigger::PreCompact), &precompact);
        let Verdict::Inject { context } = outcome.verdict else {
            panic!("antes de compactar, o bloco de retomada é injetado");
        };
        assert!(context.contains('x'), "o bloco traz a spec: {context}");
        assert!(context.contains("fase"), "o bloco traz a fase: {context}");

        // Com uma onda em andamento, o bloco continua saindo, com a onda
        // citada.
        seed_running_wave(root, "x");
        let outcome = crate::dispatch::run_event(Some(Trigger::PreCompact), &precompact);
        let Verdict::Inject { context } = outcome.verdict else {
            panic!("com onda em andamento, o bloco continua saindo");
        };
        assert!(context.contains("fase"), "o bloco não vira só a frase das ondas no ar: {context}");
        assert!(context.contains('1'), "o bloco cita a onda em andamento: {context}");

        // Uma transcrição de 200 mil tokens — o antigo degrau — não recusa
        // mais nenhuma chamada de ferramenta de quem conduz: o mesmo
        // despachante, no `PreToolUse`, deixa passar.
        let transcript = root.join("t.jsonl");
        std::fs::write(
            &transcript,
            serde_json::json!({
                "message": {"usage": {
                    "input_tokens": 200_000, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0,
                }}
            })
            .to_string(),
        )
        .unwrap();
        let outcome = crate::dispatch::run_event(Some(Trigger::PreToolUse), &conductor_call(root, &transcript));
        assert_eq!(outcome.verdict, Verdict::Allow, "sem degrau de tokens, nada barra mais a chamada");
    }

    /// Grava na spec `x`, em `root`, o evento `event_type` com `body` e
    /// devolve o número dele.
    fn seed(root: &Path, event_type: &str, body: serde_json::Value) -> u64 {
        crate::shared::spec_state::seed_event(root, "x", event_type, body)
    }

    /// Grava a onda `n` da spec `x` no plano, com o critério `crit`.
    fn plan_wave(root: &Path, n: u64, crit: u64, said: u64) {
        seed(
            root,
            "wave",
            serde_json::json!({"n": n, "text": format!("Onda {n}."), "criteria": [crit], "done_when": "x",
                "origin": said}),
        );
    }

    /// Grava a volta da onda `n` pedindo mudança de plano, à espera da rodada.
    fn seed_replan_return(root: &Path, n: u64) {
        seed(
            root,
            "delivered",
            serde_json::json!({"wave": n, "text": "O plano não fecha.", "replan": format!("Dividir a onda {n}."), "changes_decision": "A ordem escolhida.",
                "returned": true, "author": "wave"}),
        );
    }

    /// Grava a última rodada que deu certo e, depois dela, `count` decisões;
    /// devolve o código de cada uma, na ordem.
    fn seed_decisions_after_round(root: &Path, said: u64, count: usize) -> Vec<String> {
        seed(root, "call", serde_json::json!({"command": "round", "ms": 3, "result": "ok", "author": "binary"}));
        let ids: Vec<u64> = (0..count)
            .map(|at| {
                seed(
                    root,
                    "decision",
                    serde_json::json!({"text": format!("Decisão {at}."), "keys": ["retomada"],
                        "why": "o usuário pediu", "origin": said}),
                )
            })
            .collect();
        let codes = read_log(root).codes();
        ids.iter().map(|id| codes[id].clone()).collect()
    }

    /// O arquivo de eventos da spec `x`, em `root`.
    fn read_log(root: &Path) -> mustard_core::domain::spec_events::SpecLog {
        mustard_core::io::spec_events::read(&mustard_core::io::spec_events::spec_file(root, "x").unwrap())
            .unwrap()
            .unwrap()
    }

    /// O texto que o início da sessão depois da compactação e o aviso antes
    /// de compactar injetam em `root`, pelo evento do gancho, nessa ordem.
    fn hook_contexts(root: &Path, lang: Locale) -> (String, String) {
        let event = |name: &str, trigger: Trigger, raw: serde_json::Value| {
            let input = HookInput {
                hook_event_name: Some(name.to_string()),
                session_id: Some("s1".to_string()),
                cwd: Some(root.to_string_lossy().into_owned()),
                raw,
                ..HookInput::default()
            };
            match crate::dispatch::run_event(Some(trigger), &input).verdict {
                Verdict::Inject { context } => context,
                other => panic!("{lang:?}: {name} injects nothing: {other:?}"),
            }
        };
        (
            event("SessionStart", Trigger::SessionStart, serde_json::json!({"source": "compact"})),
            event("PreCompact", Trigger::PreCompact, serde_json::json!({})),
        )
    }

    /// O que o bloco em `context` mostra na vaga `slot` do modelo do bloco em
    /// `lang`: o trecho entre o rótulo que antecede a vaga e o texto que a
    /// segue até a vaga seguinte.
    fn slot_value<'a>(context: &'a str, lang: Locale, slot: &str) -> &'a str {
        let template = translate("conversation_size.block", lang);
        let at = template.find(slot).expect("the slot is in the template");
        let before = &template[..at];
        let label = &before[before.rfind('}').map_or(0, |end| end + 1)..];
        let after = &template[at + slot.len()..];
        let tail = &after[..after.find('{').unwrap_or(after.len())];
        let start = context.find(label).unwrap_or_else(|| panic!("{lang:?}: no {slot} label: {context}")) + label.len();
        let len = context[start..].find(tail).unwrap_or_else(|| panic!("{lang:?}: no end of {slot}: {context}"));
        &context[start..start + len]
    }

    /// A lista `items` cortada depois de `kept` itens, como o bloco a mostra:
    /// os primeiros e quantos ficaram de fora.
    fn cut_list(items: &[String], kept: usize, lang: Locale) -> String {
        let mut shown = items[..kept].to_vec();
        if kept < items.len() {
            shown.push(translate("conversation_size.more", lang).replace("{count}", &(items.len() - kept).to_string()));
        }
        shown.join(", ")
    }

    /// A lista `items` que o bloco `block` mostra na vaga `slot` cortada no
    /// teto: guarda os primeiros itens e diz quantos ficaram de fora, e um item
    /// a mais já não caberia. Devolve quantos ficaram.
    fn assert_cut_at_the_cap(block: &str, lang: Locale, slot: &str, items: &[String]) -> usize {
        use crate::hooks::session::session_start_inject::MAX_BYTES;
        let shown = slot_value(block, lang, slot);
        let kept = (0..items.len())
            .find(|kept| shown == cut_list(items, *kept, lang))
            .unwrap_or_else(|| panic!("{lang:?}: {slot} is not the first items and how many were left out: {shown}"));
        let one_more = block.replacen(shown, &cut_list(items, kept + 1, lang), 1);
        assert!(block.len() <= MAX_BYTES, "{lang:?}: {} bytes", block.len());
        assert!(one_more.len() > MAX_BYTES, "{lang:?}: {slot} keeps {kept}, but {} bytes still fit", one_more.len());
        kept
    }

    /// Depois da compactação, o início da sessão coloca sozinho o bloco de
    /// retomada, pelo evento do gancho: quantas ondas foram entregues, a onda em andamento
    /// com a pasta da cópia dela, a onda cuja volta espera a rodada pedindo
    /// mudança de plano ainda sem o clique, a onda parada no limite de
    /// consertos, e o código da decisão gravada depois da última rodada — e
    /// não o da gravada antes dela. O aviso antes de compactar traz o mesmo
    /// bloco e não pede para colá-lo. Nos dois idiomas, e dentro do teto do
    /// início da sessão.
    #[test]
    fn after_compaction_the_session_start_brings_the_work_block() {
        use crate::hooks::session::session_start_inject::MAX_BYTES;
        use serde_json::json;

        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let crit = seed_running_wave(root, "x");
            let said = seed(root, "message", json!({"author": "user", "text": "mais ondas"}));
            plan_wave(root, 2, crit, said);
            let (pid, started) = crate::commands::flow::stuck::this_process();
            seed(root, "send", json!({"wave": 2, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [crit], "mustard": "0", "author": "binary", "copy": copy_of(root, 2),
                "claude_pid": pid, "claude_started": started}));
            seed_replan_return(root, 2);
            // A onda 5 também pede mudança de plano, mas não troca decisão do
            // usuário: não espera clique nenhum, e o bloco a lista sem aviso.
            plan_wave(root, 5, crit, said);
            seed(root, "delivered", json!({"wave": 5, "text": "Parei.", "replan": "Trocar o nome da função.",
                "returned": true, "author": "wave"}));
            // A onda 3 entregue, e a 4 reprovada uma vez e depois de cada uma
            // das duas rodadas de conserto: parada no limite.
            plan_wave(root, 3, crit, said);
            seed(root, "delivered", json!({"wave": 3, "text": "Pronta.", "files": ["src/tres.rs"], "author": "wave"}));
            plan_wave(root, 6, crit, said);
            seed(root, "delivered", json!({"wave": 6, "text": "Pronta.", "files": ["src/seis.rs"], "author": "wave"}));
            plan_wave(root, 4, crit, said);
            for _ in 0..3 {
                crate::shared::spec_state::seed_verdict(root, "x", 4, "rejected", crit);
            }
            let before = seed(root, "decision", json!({"text": "Antes da rodada.", "keys": ["retomada"],
                "why": "o usuário pediu", "origin": said}));
            let after = seed_decisions_after_round(root, said, 1);
            let before = read_log(root).codes()[&before].clone();

            let (started, precompact) = hook_contexts(root, lang);

            let running = translate("conversation_size.copy", lang)
                .replace("{wave}", "1")
                .replace("{copy}", &copy_of(root, 1));
            let replan = translate("conversation_size.replan", lang).replace("{wave}", "2");
            for (moment, context) in [("session start", &started), ("precompact", &precompact)] {
                assert_eq!(slot_value(context, lang, "{delivered}"), "2", "{lang:?} {moment}: how many waves were delivered");
                assert_eq!(slot_value(context, lang, "{running}"), running, "{lang:?} {moment}: the wave in flight");
                assert_eq!(
                    slot_value(context, lang, "{returned}"),
                    format!("{replan}, 5"),
                    "{lang:?} {moment}: the return that swaps a decision asks for the click, the other does not"
                );
                assert_eq!(slot_value(context, lang, "{stuck}"), "4", "{lang:?} {moment}: the wave at the fix limit");
                assert_eq!(slot_value(context, lang, "{recorded}"), after[0], "{lang:?} {moment}: after the round");
                assert!(!context.contains(&before), "{lang:?} {moment}: the decision before the round: {context}");
                assert!(context.contains("mustard-rt run round --spec x"), "{lang:?} {moment}: {context}");
            }
            let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
            assert!(started.contains(&block) && precompact.contains(&block), "{lang:?}: the same block");
            assert!(block.len() <= MAX_BYTES, "{lang:?}: {} bytes", block.len());
            for paste in ["cole", "colar", "paste"] {
                assert!(!precompact.contains(paste), "{lang:?}: the notice still asks to paste: {precompact}");
            }
        }
    }

    /// Numa obra com muitas ondas entregues, o bloco de retomada diz só
    /// quantas foram, e não o número de cada uma: o passo seguinte vem das que
    /// faltam e do próximo comando, e a lista das entregues crescia com a obra
    /// e ocupava mais da metade do bloco. O início da sessão depois da
    /// compactação e o aviso de compactar trazem o mesmo bloco curto. Em
    /// português; o inglês do mesmo bloco já é provado em
    /// `after_compaction_the_session_start_brings_the_work_block`.
    #[test]
    fn work_block_counts_the_delivered_waves_instead_of_listing_each_one() {
        use serde_json::json;

        let lang = Locale::PtBr;
        let dir = open_project_in("x", lang);
        let root = dir.path();
        let crit = seed_running_wave(root, "x");
        let said = seed(root, "message", json!({"author": "user", "text": "obra longa"}));
        let mut one_delivered = 0;
        for n in 2..=12 {
            plan_wave(root, n, crit, said);
            seed(root, "delivered", json!({"wave": n, "text": "Pronta.", "files": ["src/a.rs"], "author": "wave"}));
            if n == 2 {
                one_delivered = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block").len();
            }
        }

        let (started, precompact) = hook_contexts(root, lang);
        for (moment, context) in [("session start", &started), ("precompact", &precompact)] {
            assert_eq!(slot_value(context, lang, "{delivered}"), "11", "{moment}: the count: {context}");
            assert!(!context.contains("2, 3, 4"), "{moment}: the delivered waves are listed: {context}");
        }
        let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
        assert!(block.len() <= one_delivered + 1, "the block grew with the delivered waves: {one_delivered} -> {} bytes: {block}", block.len());
    }

    /// Com mais códigos gravados depois da última rodada do que cabem no teto
    /// do início da sessão, o bloco que o gancho injeta mostra os primeiros
    /// que cabem — um a mais já não caberia — e quantos ficaram de fora; o
    /// resto do bloco fica inteiro. Nos dois idiomas.
    #[test]
    fn after_compaction_the_session_start_brings_the_work_block_with_the_codes_cut_at_the_cap() {
        use serde_json::json;

        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            seed_running_wave(root, "x");
            let said = seed(root, "message", json!({"author": "user", "text": "muitas decisões"}));
            let codes = seed_decisions_after_round(root, said, 250);

            let (started, precompact) = hook_contexts(root, lang);
            let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
            assert!(started.contains(&block) && precompact.contains(&block), "{lang:?}: the same block");
            let kept = assert_cut_at_the_cap(&block, lang, "{recorded}", &codes);
            assert!(kept > 0, "{lang:?}: the codes that fit are shown: {block}");
            let running = translate("conversation_size.copy", lang)
                .replace("{wave}", "1")
                .replace("{copy}", &copy_of(root, 1));
            assert_eq!(slot_value(&block, lang, "{running}"), running, "{lang:?}: the wave in flight stays whole");
        }
    }

    /// Quando nem a lista de códigos vazia faz o bloco caber, as outras
    /// partes também encolhem até ele caber no teto: os códigos saem todos
    /// primeiro, e das voltas à espera da rodada ficam as primeiras que cabem —
    /// uma a mais já não caberia — e quantas ficaram de fora. A onda em
    /// andamento, que cede por último, fica inteira. Nos dois idiomas.
    #[test]
    fn after_compaction_the_session_start_brings_the_work_block_cutting_the_other_parts_until_it_fits() {
        use serde_json::json;

        for lang in [Locale::PtBr, Locale::EnUs] {
            let dir = open_project_in("x", lang);
            let root = dir.path();
            let crit = seed_running_wave(root, "x");
            let said = seed(root, "message", json!({"author": "user", "text": "muitas voltas"}));
            let waves: Vec<u64> = (2..62).collect();
            for n in &waves {
                plan_wave(root, *n, crit, said);
                seed_replan_return(root, *n);
            }
            let codes = seed_decisions_after_round(root, said, 3);

            let (started, precompact) = hook_contexts(root, lang);
            let block = crate::commands::flow::resume::current_block(root, Some("s1")).expect("the block");
            assert!(started.contains(&block) && precompact.contains(&block), "{lang:?}: the same block");
            assert_eq!(slot_value(&block, lang, "{recorded}"), cut_list(&codes, 0, lang), "{lang:?}: {block}");
            let returns: Vec<String> = waves
                .iter()
                .map(|n| translate("conversation_size.replan", lang).replace("{wave}", &n.to_string()))
                .collect();
            let kept = assert_cut_at_the_cap(&block, lang, "{returned}", &returns);
            assert!(kept > 0, "{lang:?}: the returns that fit are shown: {block}");
            let running = translate("conversation_size.copy", lang)
                .replace("{wave}", "1")
                .replace("{copy}", &copy_of(root, 1));
            assert_eq!(slot_value(&block, lang, "{running}"), running, "{lang:?}: the wave in flight stays whole");
            assert!(block.contains("mustard-rt run round --spec x"), "{lang:?}: the next command: {block}");
        }
    }
}
