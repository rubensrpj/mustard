//! The line between a gate that BLOCKS and one that WARNS, asserted against the
//! shipped behaviour and the shipped prose.
//!
//! ## The rule these three criteria pin
//!
//! **A gate blocks when it catches debt the author can clear where they stand;
//! it warns when it catches something else.** Both halves matter, and only
//! asserting the first would let the second rot: a project that answers every
//! doubt with `Deny` stops being a harness and becomes an obstacle, and the
//! operator who stated the principle stated it as *flow, and block only where
//! flowing breaks what the tool stands for*.
//!
//! AC-1 is the blocking half, AC-2 the flowing half, AC-3 the same rule applied
//! to a question rather than a write.

use std::path::Path;

/// Repository root — two levels up from `apps/rt`.
fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Read a shipped file, failing loudly when it moved: a ratchet whose source is
/// missing did not run, and reading that as "nothing to check" is the failure
/// mode this whole file exists to prevent.
fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{rel} unreadable at {}: {e}", path.display()))
}

/// AC-1 — a document over its budget is REFUSED with nobody configuring
/// anything.
///
/// Asserted on the default passed at the call site, not on a mode resolved
/// through the environment: the environment is exactly what the criterion says
/// must not be needed. A test that exported `MUSTARD_SPEC_SIZE_MODE=strict`
/// would prove the gate can block when asked — which was already true — instead
/// of proving it blocks when NOT asked, which is the change.
#[test]
fn ac1_documento_acima_do_orcamento_e_recusado_por_padrao() {
    let src = read("apps/rt/src/hooks/write/size_gate.rs");

    for (env_var, label) in [
        ("MUSTARD_SPEC_SIZE_MODE", "spec size"),
        ("MUSTARD_SKILL_SIZE_MODE", "skill size"),
        ("MUSTARD_SKILL_VALIDATE_GATE_MODE", "skill validate"),
    ] {
        // Match the QUOTED literal: the call site spells it with quotes and the
        // module doc-comment does not. Searching the bare name found the comment
        // first and asserted against prose — which would keep passing after
        // someone changed the code and forgot the comment, i.e. exactly backwards.
        let quoted = format!("\"{env_var}\"");
        let at = src
            .find(&quoted)
            .unwrap_or_else(|| panic!("{label} gate no longer resolves {env_var} in code"));
        // The default is `resolve_mode`'s third argument, a short window later.
        let window = &src[at..(at + 220).min(src.len())];
        assert!(
            window.contains("GateMode::Strict"),
            "{label} still defaults to something other than strict — an oversized \
             document would pass with nobody configuring anything: {window}",
        );
    }
}

/// AC-2 — a file the spec never listed WARNS and lets the work through.
///
/// The flowing half, and the one a well-meaning future change is most likely to
/// break: making this strict looks like more rigour and is in fact friction
/// protecting nothing. It fires when a PLAN was incomplete — measured in this
/// repository on 2026-08-18, where an implementer needed `shared/context.rs`
/// that the spec had failed to list. Advisory, it did the right work and
/// reported the deviation; strict, it would have died on a planning error the
/// author could not fix without leaving the work.
#[test]
fn ac2_arquivo_fora_do_plano_avisa_e_deixa_seguir() {
    let src = read("apps/rt/src/hooks/write/boundary_gate.rs");

    let at = src
        .find("MUSTARD_BOUNDARY_MODE")
        .expect("the boundary gate no longer resolves its mode");
    let window = &src[at..(at + 400).min(src.len())];
    assert!(
        !window.contains("GateMode::Strict") && !window.contains("BoundaryMode::Strict"),
        "the boundary gate now blocks by default — a plan that failed to list a \
         file would kill the wave instead of reporting the omission: {window}",
    );

    // …and the reason is written where the next author will read it, so the
    // decision survives as a decision rather than as an accident nobody dares
    // touch.
    let size_src = read("apps/rt/src/hooks/write/size_gate.rs");
    assert!(
        size_src.contains("boundary_gate") && size_src.to_lowercase().contains("advisory"),
        "the blocking gate no longer explains why its neighbour does NOT block, \
         so the line between the two survives only as folklore",
    );
}

/// AC-3 — `/mustard:spec` with no argument, from inside a unit's own branch,
/// goes straight to that unit.
///
/// A prose ratchet, because the picker is prose: the command file IS the
/// implementation. It failed in the field on this repository's own unit —
/// the inviolable rule already said "inside the unit's branch the resume costs
/// nothing", but step 1 said "Empty → render the table" with no exception, so
/// the rule was true about the step AFTER the one that broke it.
#[test]
fn ac3_picker_dentro_da_branch_nao_pergunta() {
    let spec_md = read("plugin/commands/spec.md");

    let empty_rules: Vec<&str> = spec_md
        .lines()
        .filter(|l| l.trim_start().starts_with("- **Empty"))
        .collect();
    assert!(
        empty_rules.len() >= 2,
        "step 1 still has ONE rule for an empty argument — the work-branch case \
         and the base case answer differently and cannot share a line: {empty_rules:?}",
    );

    let branch_rule = empty_rules
        .iter()
        .find(|l| l.contains("work branch"))
        .expect("step 1 never mentions being inside a unit's work branch");
    assert!(
        branch_rule.contains("skip §2") || branch_rule.contains("no table"),
        "the work-branch rule does not say the table is skipped: {branch_rule}",
    );

    // The base case must survive: on an integration base the question is real,
    // and deleting the table there would trade one defect for its mirror.
    let base_rule = empty_rules
        .iter()
        .find(|l| l.contains("integration base"))
        .expect("step 1 no longer keeps the table for an integration base");
    assert!(
        base_rule.contains("§2"),
        "the base case no longer renders the picker, so nothing can name a unit \
         when the caller is not standing in one: {base_rule}",
    );
}
