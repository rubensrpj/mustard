//! Per-role delivery contracts (`{role_block}`) and the matching
//! tool-restricted `subagent_type`.
//!
//! Each known role (`guards`, `explore`, `plan`, `review`, `qa`, and the
//! `impl` default) gets an explicit contract: what to produce and how to
//! deliver it. This is what makes the rendered prompt self-restricting.

use mustard_core::io::fs as mfs;
use std::path::Path;

/// The epistemic floor for read-only investigative dispatches (the `explore`
/// role). Settle existence by enumeration, never claim absence from sampled
/// reads, and never refute a symptom the user observed at runtime — static
/// reading cannot disprove it. This single definition feeds BOTH the rendered
/// explore contract (in [`build_role_block`]) AND the `subagent_inject` floor
/// that re-asserts it for Explores dispatched OUTSIDE the renderer (ad-hoc /
/// cross-repo), so the discipline never drifts between the two and is never
/// lost to the dispatch route.
pub const EPISTEMIC_FLOOR: &str = "Settle existence/duplication questions by Grep \
     enumeration over the slice FIRST — reading samples never proves absence. Ground \
     every claim in file:line. NEVER assert \"X does not exist\" and never refute a \
     symptom the user observed at runtime — static reading cannot disprove it; say \
     \"not found in the files I read\" instead.";

/// The intentional-`<MEMORY>` contract carried by the roles that PRODUCE
/// knowledge — the `impl` default and `plan`. Read-only investigative roles
/// (`explore`, `review`, `qa`, `guards`) do not carry it. One
/// definition, two arms: `subagent_inject::extract_memory_block` captures the
/// tag without filtering it, relying on this bar (a real choice AND
/// transferable) to keep emission scarce — so the two arms must never state it
/// differently.
const MEMORY_CONTRACT: &str = "MEMORY: when you finish, emit ONE `<MEMORY>one-line \
     decision/lesson + why in ≤2 sentences</MEMORY>` block ONLY if BOTH hold: (a) there was a \
     REAL choice — alternatives existed and you could have gone the other way (not the only \
     option, not the obvious default); AND (b) a future agent on this project would decide \
     WORSE without knowing it. Obvious / a recap of what you did / only-true-for-this-task / \
     context you read / guards / a file list / 'interrupted' → emit NO `<MEMORY>`. \
     Good: `<MEMORY>Chose atomic_md write over direct fs::write — a mid-write crash corrupts \
     the file</MEMORY>`. Bad: `<MEMORY>Fixed the bug in foo.rs</MEMORY>` (a recap).";

/// Build the `{role_block}` — the role cue **plus a per-role delivery contract**.
/// Each known role (`guards`, `explore`, `plan`, `review`, `qa`, and the `impl`
/// default) gets an explicit contract: what to produce and how to deliver it
/// (return text vs. edit, the return-line cap, read-only vs. write). This is what makes the
/// rendered prompt self-restricting — the orchestrator no longer hand-appends the
/// contract, and a read-only role is told (and, via its `subagent_type`, unable)
/// to write. See [`recommended_subagent_type`] for the matching tool-restricted
/// agent per role.
pub(crate) fn build_role_block(role: &str, project: &Path, subproject: &str, spec_lang: &str) -> String {
    match role.trim().to_ascii_lowercase().as_str() {
        "guards" => build_guards_role_block(project, subproject, spec_lang),
        "explore" => {
            // The epistemic discipline lives in one place (EPISTEMIC_FLOOR) so
            // the rendered contract and the `subagent_inject` floor never drift.
            let floor = EPISTEMIC_FLOOR;
            format!(
                "ROLE: explore\n\
                 You map a slice of {subproject} read-only and return a compact briefing. You \
                 write NOTHING — if the task implies a change, report it, do not do it. Start from \
                 the anchors you were given; when the question is about composed behavior, follow \
                 the anchor's references into the files it pulls in (an anchor alone does not show \
                 what those files contribute); never bulk-read. {floor} Deliver: your final message is a \
                 ≤30-line briefing — the pattern to mirror, files to touch, contract wiring — plus \
                 a coverage footer (files read / chains not followed), exempt from the cap. No \
                 file dumps."
            )
        },
        // `plan` dispatches to the built-in `Plan` subagent, which has no
        // Edit/Write. It therefore CANNOT be handed the implementer default
        // (which orders a sibling read "before the first Edit/Write" and caps
        // the return at the impl budget); its contract is to design and return
        // the plan. The stated cap is the one `context_budget_gate` measures
        // for `Role::Plan`, pinned by `plan_role_gets_a_read_only_contract`.
        "plan" => format!(
            "ROLE: plan\n\
             You design the change for {subproject} and return the plan — you do NOT implement \
             it. Read-only: you have no Edit or Write tool, so write NOTHING; a step small \
             enough to just do is still only described. Start from the anchors you were given \
             and read what the plan depends on before naming a step — a step you cannot ground \
             in a file you opened is a guess. Deliver: your final message is a ≤80-line plan — \
             ordered steps, the files each step touches (path + what changes), how each step is \
             verified, and the unknowns that would change the plan. Name the alternative you \
             rejected wherever a real choice existed. Do NOT paste file contents.\n\
             {MEMORY_CONTRACT}"
        ),
        "review" => format!(
            "ROLE: review\n\
             You adversarially verify the implementer's work in {subproject}. You are NOT the \
             implementer. Read-only: report findings, never fix. Stay skeptical — the implementer \
             is not authoritative; if you cannot independently confirm a claim, reject it. Run \
             tests with the feature enabled (code presence is not effectiveness). If the prompt \
             carries a `## CHANGE REQUESTS` section, confirm EACH mid-pipeline request was \
             addressed in the code AND is covered by an Acceptance Criterion — flag any that was \
             silently dropped. MOLD CONTRACT: for every file the wave created or refactored whose \
             kind matches a skill in `## SKILLS` (the subproject's `{{role}}-pattern` molds), read \
             that SKILL.md and verify the file follows it — folder, naming, shape, must/must-not \
             rules; an unjustified deviation is a finding. Deliver: your \
             final message is a ≤60-line verdict — pass/fail per claim, each backed by the command \
             you ran and its real output. End that message with ONE machine-readable line so a \
             SubagentStop hook records the gate result without a human re-reading your prose: \
             `<VERDICT>{{\"verdict\":\"approved\"|\"rejected\",\"critical\":N,\"findings\":[…]}}</VERDICT>`. \
             `verdict` is `rejected` when any blocking finding exists, else `approved` (only those two \
             values); `critical` (N) counts BLOCKING findings ONLY — a violated `## Guards` rule, a \
             violated `{{role}}-pattern` mold, or a correctness defect (never style or nits) — and it \
             MUST equal the number of `\"severity\":\"critical\"` entries in `findings`; `findings` is \
             an array of `{{\"severity\":\"critical\"|\"major\"|\"minor\",\"location\":\"<file>:<line>\",\"summary\":\"<one line>\"}}`. \
             Emit exactly one valid-JSON block on a single line; if you cannot, omit it and the manual \
             review-result path still records the verdict."
        ),
        "qa" => "ROLE: qa\n\
             You run each Acceptance Criterion command in the spec and report pass/fail. You do \
             NOT fix anything. Run the exact `Command:` from each AC and capture its real output. \
             Deliver: per-AC pass/fail + the proving output; overall=pass only if every AC passes.".to_string(),
        _ => format!(
            "ROLE: {role}\n\
             You implement inside {subproject} ONLY — never touch another subproject, the spec, or \
             .claude/. Before the first Edit/Write, read ONE sibling file to match conventions. \
             Every comment you write follows the project locale ({spec_lang}), as the spec prose \
             does; the rest of the code stays English — identifiers, file paths, shell commands, \
             log/error messages, API string constants. Never translate pre-existing comments. \
             Max 3 build \
             attempts, then STOP and report. Deliver: your final message is a ≤40-line report — \
             files changed + non-obvious decisions + blockers. Do NOT paste file contents.\n\
             {MEMORY_CONTRACT}"
        ),
    }
}

/// Build the `guards` role block — the enrich instruction. Carries the
/// grounded 3-6 line cap, the project locale (the caller reads it from
/// `mustard.json` through [`mustard_core::ProjectConfig::language`]), the
/// pending block's deterministic facts, and the delivery contract (return the
/// lines as text; never write a file — nothing writes them into a `CLAUDE.md`).
fn build_guards_role_block(project: &Path, subproject: &str, spec_lang: &str) -> String {
    let facts = read_guards_facts(project, &project.join(subproject));
    let facts_line = if facts.is_empty() {
        String::new()
    } else {
        format!("\nFacts (deterministic, from scan): {facts}.")
    };
    format!(
        "ROLE: guards\n\
         Write 3-6 lines of Guards (do/don't) GROUNDED in the deterministic facts \
         and the subproject's real code; include ONLY what is NOT auto-inferable \
         from the manifest/tree. Do not RESTATE a fact — author the non-obvious \
         RULE it implies: a `scripts=` codegen step ⇒ \"its output is generated — \
         regenerate via that script, never hand-edit it\"; a detected stack ⇒ the \
         convention that stack enforces here. Write in the project locale \
         ({spec_lang}), in plain words. Be concise; never generic prose. Deliver \
         ONLY the lines as your final message; do NOT write any file — nothing \
         writes them into a CLAUDE.md for you.{facts_line}"
    )
}

/// Read the `<!-- facts: ... -->` payload from a subproject's pending `## Guards`
/// block (the scan's grounding context: `kind=...; frameworks=...`). Empty when
/// the file or the facts line is absent. Shape-mirrors [`super::sections::read_guards_block`]
/// down to resolving its source through [`crate::shared::context::guards_file`],
/// so the enrich pass keeps its grounding under a private install.
fn read_guards_facts(root: &Path, subproject_dir: &Path) -> String {
    let source = crate::shared::context::guards_file(root, subproject_dir);
    let text = mfs::read_to_string(source).unwrap_or_default();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<!-- facts:") {
            return rest.trim_end_matches("-->").trim().to_string();
        }
    }
    String::new()
}

/// The Mustard plugin namespace — the `name` field of
/// `plugin/.claude-plugin/plugin.json`. Claude Code registers plugin-owned
/// agents under it as `mustard:<agent>`, so a `subagent_type` naming a plugin
/// agent MUST carry this prefix or the dispatch silently falls back to
/// `general-purpose`. Single source of truth: [`qualify_plugin_agent`] builds
/// the qualified name from it and a drift test pins it to the manifest `name`.
pub const PLUGIN_NAMESPACE: &str = "mustard";

/// Qualify a plugin-owned agent name with [`PLUGIN_NAMESPACE`]:
/// `mustard-review` → `mustard:mustard-review`. Only plugin-owned agents go
/// through here; built-in harness agent types (`Explore`, `Plan`,
/// `general-purpose`) are not plugin-registered and stay bare.
fn qualify_plugin_agent(name: &str) -> String {
    format!("{PLUGIN_NAMESPACE}:{name}")
}

/// Map a pipeline role to the `subagent_type` the orchestrator should dispatch.
///
/// Read-only roles resolve to **tool-restricted** agents so they physically
/// cannot write: `explore` → the built-in `Explore` (no Edit/Write), `plan` →
/// the built-in `Plan` (no Edit/Write), `review`/`qa` → `mustard:mustard-review`
/// (Read/Grep/Glob/Bash — Bash for tests only) and `guards` →
/// `mustard:mustard-guards` (Read/Grep/Glob only). The
/// plugin-owned agents carry the [`PLUGIN_NAMESPACE`] prefix (built-ins stay
/// bare) — without it Claude Code cannot resolve the plugin agent and silently
/// falls back to `general-purpose`. Writing roles (`impl` and any other) stay
/// `general-purpose`: they need Edit/Write and rely on the per-role contract +
/// the `write_gate` hook instead. Emitted by `dispatch-plan` so the
/// orchestrator never picks the agent by hand.
#[must_use]
pub fn recommended_subagent_type(role: &str) -> String {
    match role.trim().to_ascii_lowercase().as_str() {
        "explore" => "Explore".to_string(),
        "plan" => "Plan".to_string(),
        "review" | "qa" => qualify_plugin_agent("mustard-review"),
        "guards" => qualify_plugin_agent("mustard-guards"),
        _ => "general-purpose".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Plant a workspace anchor so `ClaudePaths::for_project` accepts the temp dir.
    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

    #[test]
    fn build_role_block_emits_role_cue_and_contract() {
        // Every role block starts with its `ROLE:` cue and now carries a
        // delivery contract (no longer a bare marker). The read-only roles state
        // their write-restriction in prose; their subagent_type enforces it.
        let dir = tempdir().unwrap();
        let impl_block = build_role_block("impl", dir.path(), "api", "en-US");
        assert!(impl_block.starts_with("ROLE: impl"), "cue missing: {impl_block}");
        assert!(impl_block.contains("implement inside api ONLY"), "scope missing: {impl_block}");
        let review_block = build_role_block("review", dir.path(), "api", "en-US");
        assert!(review_block.starts_with("ROLE: review"));
        assert!(review_block.contains("never fix"), "review write-restriction missing");
        let explore_block = build_role_block("explore", dir.path(), "api", "en-US");
        assert!(explore_block.starts_with("ROLE: explore"));
        assert!(explore_block.contains("write NOTHING"), "explore write-restriction missing");
    }

    /// Producing roles (impl and plan, which share [`MEMORY_CONTRACT`] from
    /// their two arms) carry the intentional-`<MEMORY>` instruction; read-only
    /// roles (explore/review/qa/guards) do NOT — they are not knowledge
    /// producers, so the contract stays off their prompt.
    #[test]
    fn producing_roles_carry_memory_contract_readonly_do_not() {
        let dir = tempdir().unwrap();
        let impl_block = build_role_block("impl", dir.path(), "api", "en-US");
        assert!(impl_block.contains("<MEMORY>"), "impl must carry MEMORY contract: {impl_block}");
        // The sharpened contract: an operational two-part test (a real choice AND
        // transferable) rather than the vague "non-obvious decision".
        assert!(
            impl_block.contains("REAL choice") && impl_block.contains("decide WORSE"),
            "MEMORY contract must carry the operational (a)+(b) test: {impl_block}"
        );
        // `plan` has its own arm now, and it carries the SAME const → also present.
        let plan_block = build_role_block("plan", dir.path(), "api", "en-US");
        assert!(plan_block.contains("<MEMORY>"), "plan must carry MEMORY contract: {plan_block}");
        // Read-only roles must NOT carry it.
        for role in ["explore", "review", "qa"] {
            let block = build_role_block(role, dir.path(), "api", "en-US");
            assert!(
                !block.contains("<MEMORY>"),
                "read-only role {role} must not carry the MEMORY contract: {block}"
            );
        }
    }

    /// `recommended_subagent_type("plan")` dispatches to the built-in `Plan`
    /// agent, which has no Edit/Write. While `plan` fell into the implementer
    /// default it was told to read a sibling file "before the first Edit/Write"
    /// and to report changed files — an order the agent physically cannot
    /// carry out. Its own arm states the planning contract instead, and the cap
    /// it announces is the one `context_budget_gate` actually measures for
    /// `Role::Plan`: both sides are read from the code (the block's text, the
    /// budget function), never a literal repeated in two places.
    #[test]
    fn plan_role_gets_a_read_only_contract() {
        use crate::hooks::task::context_budget_gate::{output_budget, Role};

        let dir = tempdir().unwrap();
        let block = build_role_block("plan", dir.path(), "apps/rt", "en-US");
        assert!(block.starts_with("ROLE: plan"), "cue missing: {block}");

        // The implementer's write instructions must not reach a read-only agent.
        let impl_block = build_role_block("impl", dir.path(), "apps/rt", "en-US");
        assert!(
            impl_block.contains("Edit/Write"),
            "the implementer contract must still carry the sibling-read rule: {impl_block}"
        );
        assert!(
            !block.contains("Edit/Write"),
            "plan must not carry the implementer's Edit/Write instruction: {block}"
        );
        assert!(
            !block.contains("You implement inside"),
            "plan must not be handed the implementer contract: {block}"
        );

        // The announced cap IS the gate's budget for this role — one number,
        // two readers. The implementer's cap must not be the one announced.
        let cap = output_budget(Role::Plan);
        assert!(
            block.contains(&format!("≤{cap}-line")),
            "plan must announce the budget the gate measures (≤{cap}-line): {block}"
        );
        let impl_cap = output_budget(Role::General);
        assert_ne!(cap, impl_cap, "the two budgets must stay distinct for this test to bite");
        assert!(
            !block.contains(&format!("≤{impl_cap}-line")),
            "plan must not announce the implementer cap (≤{impl_cap}-line): {block}"
        );
    }

    #[test]
    fn explore_role_block_carries_epistemic_contract() {
        // Field defect: an Explore read sliced anchors and confidently returned
        // "no duplication" — refuting a symptom the user had observed at
        // runtime (the duplicate lived in a referenced file, invisible to
        // sliced anchor reads). The contract must route existence questions to Grep
        // enumeration, demand file:line evidence, forbid unqualified negative
        // verdicts, and keep the coverage footer outside the return cap.
        let dir = tempdir().unwrap();
        let block = build_role_block("explore", dir.path(), "api", "en-US");
        assert!(block.contains("never proves absence"), "grep-first rule missing: {block}");
        assert!(block.contains("file:line"), "evidence rule missing: {block}");
        assert!(
            block.contains("not found in the files I read"),
            "qualified-negative form missing: {block}"
        );
        assert!(block.contains("never refute a symptom"), "symptom rule missing: {block}");
        assert!(block.contains("coverage footer"), "coverage footer missing: {block}");
    }

    #[test]
    fn review_role_block_carries_verdict_contract() {
        // The rendered review block must
        // instruct the agent to end with a machine-readable `<VERDICT>` line so the
        // SubagentStop hook records the gate result without a human reading prose.
        // It must name the JSON shape (verdict approved|rejected, critical N,
        // findings array) and that `critical` counts BLOCKING (Guard / mold /
        // correctness) findings only. Mirrors the plugin `mustard-review.md` contract.
        let dir = tempdir().unwrap();
        let block = build_role_block("review", dir.path(), "apps/rt", "en-US");
        assert!(block.contains("<VERDICT>"), "VERDICT block missing: {block}");
        assert!(block.contains("\"critical\":N"), "critical field missing: {block}");
        assert!(block.contains("\"findings\""), "findings field missing: {block}");
        assert!(block.contains("approved") && block.contains("rejected"), "verdict values missing: {block}");
        assert!(block.contains("BLOCKING findings ONLY"), "blocking-only rule missing: {block}");
        // Read-only role → still no MEMORY contract (that guard must not regress).
        assert!(!block.contains("<MEMORY>"), "review must not carry MEMORY: {block}");
    }

    #[test]
    fn guards_role_block_carries_delivery_contract() {
        // The guards block must tell the agent to return the lines as text and
        // never write a file — the missing rule that let an agent self-write.
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let block = build_role_block("guards", dir.path(), "apps/rt", "pt-BR");
        assert!(block.contains("final message"), "delivery contract missing: {block}");
        assert!(!block.contains("scan-guards-apply"), "names a command that no longer exists: {block}");
        assert!(block.contains("do NOT write any file"), "write-restriction missing: {block}");
    }

    #[test]
    fn recommended_subagent_type_locks_read_only_roles() {
        // Read-only roles map to tool-restricted agents; writing roles stay
        // general-purpose. Case/whitespace-insensitive.
        assert_eq!(recommended_subagent_type("explore"), "Explore");
        assert_eq!(recommended_subagent_type("plan"), "Plan");
        assert_eq!(recommended_subagent_type("review"), "mustard:mustard-review");
        assert_eq!(recommended_subagent_type("qa"), "mustard:mustard-review");
        assert_eq!(recommended_subagent_type(" Guards "), "mustard:mustard-guards");
        assert_eq!(recommended_subagent_type("impl"), "general-purpose");
        assert_eq!(recommended_subagent_type("backend"), "general-purpose");
    }

    #[test]
    fn guards_prompt_lang_carries_locale_and_facts() {
        // The `guards` role drives the enrich step: its block must name the
        // project locale and surface the pending block's deterministic facts
        // so the agent stays grounded. An old `tone` key changes nothing.
        let dir = tempdir().unwrap();
        anchor(dir.path());
        std::fs::write(
            dir.path().join("mustard.json"),
            br#"{"language":{"text":"pt-BR"},"tone":"technical"}"#,
        )
        .unwrap();
        // A subproject CLAUDE.md with a pending Guards block carrying facts.
        let sub = dir.path().join("apps").join("rt");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("CLAUDE.md"),
            "# Rt\n\n## Guards\n\n<!-- mustard:guards pending -->\n\
             <!-- facts: kind=rust; frameworks=serde, clap -->\n<!-- /mustard:guards -->\n",
        )
        .unwrap();

        let block = build_role_block("guards", dir.path(), "apps/rt", "pt-BR");
        assert!(block.starts_with("ROLE: guards"), "role marker missing: {block}");
        // The locale is surfaced; the old tone key is not read.
        assert!(block.contains("pt-BR"), "locale missing: {block}");
        assert!(!block.contains("technical"), "the tone key is read by nobody: {block}");
        // Grounding facts from the pending block are surfaced.
        assert!(block.contains("kind=rust"), "kind fact missing: {block}");
        assert!(block.contains("serde, clap"), "framework facts missing: {block}");
        // The cap (3-6 lines) is named so the agent stays concise.
        assert!(block.contains("3-6"), "line cap not stated: {block}");

        // A non-guards role gets its own contract (no longer a bare marker).
        let backend = build_role_block("backend", dir.path(), "apps/rt", "pt-BR");
        assert!(backend.starts_with("ROLE: backend"), "role marker missing: {backend}");
        assert!(backend.contains("apps/rt"), "subproject scope missing: {backend}");
    }

    #[test]
    fn guards_prompt_lang_specless_derives_locale_from_mustard_json() {
        // The `/scan` enrich path runs spec-less: `run` is invoked with no
        // `--spec`. The locale comes from `mustard.json` `language.text` through
        // `ProjectConfig::language`, the one reader of the language — never an
        // ad-hoc parse. This test pins that source feeding into the guards
        // block (locale + the grounded 3-6 line instruction).
        let dir = tempdir().unwrap();
        anchor(dir.path());
        std::fs::write(dir.path().join("mustard.json"), br#"{"language":{"text":"pt-BR"}}"#).unwrap();
        let sub = dir.path().join("apps").join("rt");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("CLAUDE.md"),
            "# Rt\n\n## Guards\n\n<!-- mustard:guards pending -->\n\
             <!-- facts: kind=rust; frameworks=serde, clap -->\n<!-- /mustard:guards -->\n",
        )
        .unwrap();

        // Mirror the locale derivation in `run`: the narrative locale is
        // `ProjectConfig::load(..).language().text_or_default()`.
        let spec_lang = mustard_core::ProjectConfig::load(dir.path())
            .language()
            .text_or_default()
            .as_str()
            .to_string();
        assert_eq!(spec_lang, "pt-BR", "the locale must come from mustard.json language.text");

        // That derived locale flows into the guards block: locale + the
        // capped, grounded instruction.
        let block = build_role_block("guards", dir.path(), "apps/rt", &spec_lang);
        assert!(block.starts_with("ROLE: guards"), "role marker missing: {block}");
        assert!(block.contains("pt-BR"), "locale missing: {block}");
        assert!(block.contains("kind=rust"), "kind fact missing: {block}");
        assert!(block.contains("3-6"), "line cap not stated: {block}");

        // A project with no language declared falls back to the default
        // locale of Mustard's messages (never a panic on the spec-less path).
        let bare = tempdir().unwrap();
        anchor(bare.path()); // anchor writes `{}` mustard.json.
        let default_lang = mustard_core::ProjectConfig::load(bare.path())
            .language()
            .text_or_default()
            .as_str()
            .to_string();
        assert_eq!(default_lang, "pt-BR", "the default locale is the messages' pt-BR");
    }

    /// The resolver qualifies plugin-owned agents with [`PLUGIN_NAMESPACE`]
    /// (`mustard:mustard-*`) while built-in harness agent types stay bare. A
    /// bare `mustard-review` would not resolve as a Claude Code plugin agent —
    /// the dispatch would silently fall back to `general-purpose`.
    #[test]
    fn recommended_subagent_type_namespaces_plugin_agents_only() {
        let ns = format!("{PLUGIN_NAMESPACE}:");
        // Plugin-owned roles → qualified under the plugin namespace.
        assert_eq!(recommended_subagent_type("review"), format!("{ns}mustard-review"));
        assert_eq!(recommended_subagent_type("qa"), format!("{ns}mustard-review"));
        assert_eq!(recommended_subagent_type("guards"), format!("{ns}mustard-guards"));
        // Built-in harness types are not plugin-owned → stay bare (no `<ns>:`).
        for t in [
            recommended_subagent_type("explore"),
            recommended_subagent_type("plan"),
            recommended_subagent_type("impl"),
        ] {
            assert!(!t.contains(':'), "built-in agent type must stay bare: {t}");
        }
    }

    /// Drift guard: [`PLUGIN_NAMESPACE`] must equal the `name` field of the
    /// plugin manifest (`plugin/.claude-plugin/plugin.json`). If the manifest is
    /// renamed, the qualified `subagent_type`s (`mustard:mustard-*`) would name a
    /// namespace Claude Code does not know and every plugin dispatch would
    /// silently fall back to `general-purpose` — so pin the two together. Reads
    /// outside the crate fail open (skip) per this codebase's test convention.
    #[test]
    fn plugin_namespace_matches_manifest_name() {
        // apps/rt -> apps -> workspace root, then the committed manifest.
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let Some(workspace) = manifest_dir.parent().and_then(Path::parent) else {
            eprintln!("[skip] cannot resolve workspace root from CARGO_MANIFEST_DIR");
            return;
        };
        let manifest = workspace
            .join("plugin")
            .join(".claude-plugin")
            .join("plugin.json");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            eprintln!(
                "[skip] plugin manifest not found at {} — drift guard skipped",
                manifest.display()
            );
            return;
        };
        let json: serde_json::Value =
            serde_json::from_str(&text).expect("plugin.json must be valid JSON");
        let name = json
            .get("name")
            .and_then(serde_json::Value::as_str)
            .expect("plugin.json must carry a string `name`");
        assert_eq!(
            name, PLUGIN_NAMESPACE,
            "PLUGIN_NAMESPACE ({PLUGIN_NAMESPACE}) drifted from plugin.json#name ({name})"
        );
    }

}
