---
id: wave.prove-every-acceptance-criterion-can.2-gate
---

# wave-2-gate

## Summary

The proof is demanded during planning, not at the approval gesture; the approval door keeps a fail-closed backstop and stops reporting a gesture the current run never performed.

## Network

- Parent: [[spec.prove-every-acceptance-criterion-can]]
- Depends on: [[wave.prove-every-acceptance-criterion-can.1-rt]]

## Tasks

- [ ] In `plan_materialize.rs`, compose the wave-1 engine IN-PROCESS (module-qualified, no subprocess) exactly as `analyze-validation` and the dependency-DAG check are already composed, and make an unproven criterion WITHHOLD the PLAN transition and exit 2 — the same shape as the uncovered-criteria coverage gate that file already documents as having no env knob. The point of putting it here is the moment: a refusal that first appears when the user clicks approve lands at the instant of highest expectation, so the Full path must hit it while planning, where fixing a criterion is ordinary work. Add the report under its own key, byte-stable and timestamp-free.
- [ ] In `approve_spec.rs`, add a THIRD approval precondition beside clarify and user-approval: every non-exempt criterion in the spec's own `## Acceptance Criteria` must have a PROVEN record in `<spec>/ac-proof.json` for the exact command it carries today. Read the ledger through the type wave 1 defined — never a second parser for the same file. This is the BACKSTOP (it covers the Light path and a tampered ledger); in a healthy Full run the plan materialisation already refused earlier.
- [ ] Fold the refusal into the EXISTING `unmet_gate_message` so one run names every unmet precondition at once. The remedy must be a line the reader can copy — the `mustard-rt run ac-negative-check --spec <slug>` invocation — and the wording must distinguish a proof never taken from a proof that came back green. Never add a second refusal path.
- [ ] Fail CLOSED, matching the deliberate exception this file already documents at `approve_spec.rs:235`: an absent, unreadable or unparsable ledger refuses. Say so in the doc comment, because the crate-wide rule is the opposite and a reader will assume fail-open.
- [ ] A recorded command that no longer matches the criterion's current command counts as NO proof — that is precisely the hand edit this whole spec exists to close. Make the precondition unconditional: it must NOT consult `MUSTARD_APPROVAL_MODE` or any other switch. A spec with no `## Acceptance Criteria` section at all is unchanged — it has nothing to prove, and this gate must not invent a refusal for it.
- [ ] Correct the approval report while this file is open (field finding, confirmed at `approve_spec.rs:64`): `approvedVia` currently echoes the provenance recorded in the marker, so a spec approved in an earlier session reports the door THAT session used, while the field name reads as how the CURRENT approval happened — a silent wrong answer for anyone auditing who approved what. Separate the marker's provenance from the current run's action, so a run that performed no approval gesture never claims one. Keep the existing degrade-to-silence behaviour for an unreadable marker body.
- [ ] Close the forged approval in `hooks/observe/approval_marker_observer.rs`. REPRODUCED while planning this very spec: `is_affirmative` splits the answer into word tokens and accepts it when ANY token starts with the approval stem. That rule was designed for the SHORT option labels the model authors, but a free-text answer lands in the SAME `tool_response.answers` field — so a long message that merely CONTAINS an approval word (a field report discussing approval, for instance) minted `.approved-by-user`, the one signal the whole gate rests on being unforgeable. The marker then froze the wave layout, silently discarding a plan revision. Require the answer to be EXACTLY one of the option labels the question offered — the offered options are in `tool_input`, authored by the model and echoed by the harness — before the stem test runs at all. Free text must never mint the marker regardless of its words. Keep the observer fail-open (side-effect only, never a verdict) and keep the existing stderr notice that explains a decline, extending it to name this new condition so an operator whose genuine approval was typed as free text is told what to do.
- [ ] Cover the forged path in both directions: a free-text answer carrying an approval word mints NOTHING, while a genuine selection of an offered approval label still mints the marker exactly as today. The second half is what stops the fix from passing by making the recogniser inert.

## Files

- `apps/rt/src/commands/pipeline/plan_materialize.rs`
- `apps/rt/src/commands/spec/approve_spec.rs`
- `apps/rt/src/hooks/observe/approval_marker_observer.rs`
