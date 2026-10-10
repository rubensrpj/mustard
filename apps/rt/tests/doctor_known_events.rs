// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! Drift ratchet between the doctor's hook-event set and the shipped
//! `plugin/hooks/hooks.json` manifest.
//!
//! A conferência da fiação valida os comandos `mustard-rt on <evento>` contra
//! o conjunto de eventos que o manifesto registra, inclusive `PreCompact` e
//! `Stop`. `doctor::known_hook_events` deriva o conjunto do manifesto; este
//! teste lê o manifesto uma segunda vez, de forma independente, direto do
//! disco, e falha se os dois discordam para qualquer lado — assim uma
//! derivação desfeita, ou um leitor que deixa de enxergar uma forma que o
//! manifesto usa, vira falha de teste e não um FAIL calado em campo.
//!
//! libtest matches `--exact` against the FULL test path, which in the package's
//! single integration program is
//! `doctor_known_events::known_events_match_shipped_hooks`: a criterion that
//! runs this test with `--exact` cites that whole path.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use mustard_rt::commands::doctor::doctor::known_hook_events;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Locate the shipped hook manifest by walking up from the crate directory.
/// `CARGO_MANIFEST_DIR` is `<repo>/apps/rt`; the manifest is
/// `<repo>/plugin/hooks/hooks.json`.
fn shipped_manifest_path() -> Option<PathBuf> {
    let manifest = manifest_dir::manifest_dir();
    let mut dir = manifest.as_path();
    loop {
        let candidate = dir.join("plugin").join("hooks").join("hooks.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

/// The event names the manifest registers — read straight off disk, so the
/// assertion compares two derivations instead of one value against itself.
fn shipped_events() -> BTreeSet<String> {
    let path = shipped_manifest_path().expect("plugin/hooks/hooks.json must be reachable");
    let text = std::fs::read_to_string(&path).expect("hooks.json must be readable");
    let manifest: serde_json::Value =
        serde_json::from_str(&text).expect("hooks.json must be valid JSON");
    manifest
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .expect("hooks.json must carry a `hooks` object")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn known_events_match_shipped_hooks() {
    let shipped = shipped_events();
    assert!(
        !shipped.is_empty(),
        "the shipped manifest registers no hook event — the fixture, not the doctor, is broken"
    );

    let known = known_hook_events();
    assert!(
        !known.is_empty(),
        "doctor derived an empty event set — the embedded manifest did not parse"
    );

    let missing: Vec<&String> = shipped.difference(&known).collect();
    assert!(
        missing.is_empty(),
        "shipped hook events the doctor would call unknown: {missing:?}"
    );

    let extra: Vec<&String> = known.difference(&shipped).collect();
    assert!(
        extra.is_empty(),
        "hook events the doctor accepts but nothing ships: {extra:?}"
    );
}

/// Every way a session can start is covered by a matcher.
///
/// `SessionStart` fires with one of five sources: `startup`, `resume`, `clear`,
/// `compact`, `fork`. A source no matcher names gets no hook at all, so the
/// window opens with the router absent and nothing says so — which is exactly
/// what `fork` did until this unit.
#[test]
fn sessionstart_matchers_cover_fork() {
    let path = shipped_manifest_path().expect("plugin/hooks/hooks.json must be reachable");
    let text = std::fs::read_to_string(&path).expect("hooks.json must be readable");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

    let matchers: Vec<String> = manifest["hooks"]["SessionStart"]
        .as_array()
        .expect("SessionStart must register at least one entry")
        .iter()
        .filter_map(|e| e.get("matcher").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect();

    for source in ["startup", "resume", "clear", "compact", "fork"] {
        assert!(
            matchers.iter().any(|m| m.split('|').any(|alt| alt.trim() == source)),
            "no SessionStart matcher covers `{source}` — a session started that way \
             opens with no hook at all. Registered: {matchers:?}",
        );
    }
}

/// A raiz do repositório, a partir do manifesto que este arquivo já localiza:
/// `<repo>/plugin/hooks/hooks.json` sobe três pastas até `<repo>`.
fn repo_root() -> PathBuf {
    let manifest = shipped_manifest_path().expect("plugin/hooks/hooks.json must be reachable");
    manifest
        .ancestors()
        .nth(3)
        .expect("plugin/hooks/hooks.json sits three folders under the repo root")
        .to_path_buf()
}

/// O texto de um arquivo do repositório, pelo caminho relativo à raiz.
fn source(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("ler {}: {e}", path.display()))
}

/// As sobras da economia antiga: o teto de linhas do pedido de onda, o teto de
/// três tarefas e três provas por onda, a pausa da conversa aos 200 mil tokens,
/// o teto de turnos do agente, a remoção da cópia pelo acerto do git e a
/// leitura da onda da cópia pelo tamanho da conversa saíram do código em obras
/// anteriores. Esta prova fecha as três portas por onde elas voltam a aparecer
/// para quem lê: um comentário que ainda descreva a regra que saiu; um teste
/// composto que repita prova já existente em teste próprio; e o registro do
/// gancho de compactação, hoje conferido só contra si mesmo — quem tira a
/// entrada `PreCompact` do manifesto tira junto a lista derivada dele, e o
/// aviso de compactar some sem nenhum teste cair.
#[test]
fn leftovers_of_the_old_economy_no_longer_exist() {
    // 1) Nenhum comentário descreve regra que já saiu do código. Cada frase
    // abaixo é a que o arquivo trazia: a volta de qualquer uma derruba aqui.
    for (rel, phrase) in [
        ("apps/rt/src/commands/flow/plan.rs", "acima do teto de linhas"),
        ("apps/rt/src/commands/flow/plan.rs", "mais de três tarefas ou mais de três provas"),
        ("apps/rt/src/hooks/observe/wave_alive_observer.rs", "pausa aos 200 mil"),
        ("apps/rt/src/commands/git_settle.rs", "stays true only when WE removed it"),
        // O teto do lote é fixo: nenhuma rodada o corrige, e o teto de
        // turnos que teria sido corrigido do mesmo jeito saiu.
        ("apps/rt/src/shared/dag.rs", "do jeito que já\n/// corrigiram o teto de turnos"),
        ("apps/rt/src/shared/dag.rs", "as primeiras rodadas de despacho a corrigem"),
        // O gancho do bilhete e a ajuda do plano não conferem mais o tamanho
        // do pedido.
        ("apps/rt/src/hooks/task/subagent_inject.rs", "pedido cabe no teto de linhas"),
        ("apps/rt/src/commands/flow/cli.rs", "cada pedido cabe no teto de linhas"),
        // Ninguém fora do observador lê a onda dona da cópia.
        ("apps/rt/src/hooks/observe/wave_alive_observer.rs", "Também lida\n/// por [`crate::hooks::session::conversation_size`]"),
        // O plano só recusa pelo ponto aberto; a onda não nasce mais pequena,
        // e o teto de turnos do agente saiu.
        ("apps/rt/src/commands/flow/plan.rs", "ponto aberto, pedido grande demais"),
        ("apps/rt/src/commands/flow/plan.rs", "a onda nasce pequena agora"),
        ("apps/rt/src/commands/flow/plan.rs", "antes da onda nascer pequena"),
        ("apps/rt/src/commands/flow/plan.rs", "quem corta o custo\n    /// agora é o teto de turnos do agente"),
        // O pedido da onda tem teto em tokens, não em linhas: a escolha das
        // lições não fala mais do teto que saiu.
        ("packages/core/src/io/wave_prompt.rs", "passaria do teto de linhas"),
    ] {
        assert!(
            !source(rel).contains(phrase),
            "{rel} ainda descreve uma regra que saiu do código: {phrase:?}"
        );
    }

    // 2) Nenhum teste composto repete prova que já existe em teste próprio. O
    // que segurava as cinco réguas de uma vez saiu; as quatro provas próprias
    // que o cobriam continuam, cada uma no arquivo da régua que ela prova.
    assert!(
        !source("apps/rt/src/commands/spec_events/pages/secrets.rs")
            .contains("the_five_old_economy_caps_stay_out_of_the_real_paths"),
        "o teste composto das cinco réguas voltou ao módulo da página"
    );
    for (rel, proof) in [
        (
            "apps/rt/src/commands/spec_events/write.rs",
            "fn a_fourth_task_and_a_fourth_proof_are_recorded_like_the_third",
        ),
        (
            "apps/rt/src/commands/flow/plan.rs",
            "fn a_request_far_past_the_old_line_cap_does_not_block_the_question",
        ),
        (
            "apps/rt/src/commands/flow/round/usage.rs",
            "fn the_round_sums_every_piece_of_the_wave_agent_once",
        ),
        (
            "apps/rt/src/hooks/session/conversation_size.rs",
            "fn compaction_warning_arrives_in_the_hook_and_nobody_else_is_blocked_by_size",
        ),
    ] {
        assert!(
            source(rel).contains(proof),
            "{rel} perdeu a prova própria que substitui o teste composto: {proof:?}"
        );
    }

    // 3) O registro do gancho de compactação tem catraca. O manifesto é lido
    // aqui pelo nome do evento, não pela lista que o doutor deriva dele: a
    // entrada precisa existir, chamar `on PreCompact` e cobrir as duas formas
    // de compactar, a manual e a automática.
    let path = shipped_manifest_path().expect("plugin/hooks/hooks.json must be reachable");
    let text = std::fs::read_to_string(&path).expect("hooks.json must be readable");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("hooks.json must be valid JSON");
    let entries = manifest["hooks"]["PreCompact"]
        .as_array()
        .expect("o manifesto tem de registrar `PreCompact` — sem ele o aviso de compactar nunca chega");
    assert!(
        entries.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .is_some_and(|hooks| hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|c| c.contains("on PreCompact"))
                }))
        }),
        "`PreCompact` está registrado sem chamar `mustard-rt on PreCompact`: {entries:?}"
    );
    let matchers: Vec<String> = entries
        .iter()
        .filter_map(|e| e.get("matcher").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect();
    for source_name in ["manual", "auto"] {
        assert!(
            matchers.iter().any(|m| m.split('|').any(|alt| alt.trim() == source_name)),
            "nenhum matcher de `PreCompact` cobre `{source_name}` — compactar assim não avisa nada. \
             Registrados: {matchers:?}",
        );
    }
}
