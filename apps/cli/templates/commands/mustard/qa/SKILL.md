---
name: mustard:qa
description: Run QA phase — execute Acceptance Criteria from spec. Use after EXECUTE completes to validate all AC pass before CLOSE. Triggers automatically in pipeline but can be run manually.
---
<!-- mustard:generated -->
# /qa - QA Phase

## Trigger

`/mustard:qa [--spec <name>]`

## Description

Executes the QA phase: reads Acceptance Criteria from the active spec, runs each AC command, and reports pass/fail. Blocks CLOSE if any AC fails.

This is Wave 10 of the Mustard pipeline — the formal Dev/QA contract.

## Action

### Step 1 — Identify spec

If `--spec <name>` provided: use that spec name.
Otherwise: Glob `.claude/spec/*/spec.md`, filter by `Status:` header (skip `completed`/`cancelled`), and pick the most recently modified.

### Step 2 — Validate spec has AC

Check that spec contains `## Acceptance Criteria` section with ≥1 item in format:
```
- [ ] AC-N: description — Command: `cmd`
```

If section missing: inform user:
> "Spec has no Acceptance Criteria section. Add the section before running QA. See Wave 10 spec template."
Stop here.

### Step 3 — Run QA

Emit stage transition to QaReview, then run:
```bash
mustard-rt run emit-pipeline --kind pipeline.stage --spec {specName} --payload "{\"stage\":\"QaReview\"}"
mustard-rt run qa-run --spec {specName}
```

If `mustard-rt` not found: dispatch Task(general-purpose) with QA agent context loaded from `.claude/context/qa/qa.core.md`.

### Step 4 — QA result is emitted automatically

`mustard-rt run qa-run` emits a `qa.result` event into the SQLite store on every run. No pipeline-state JSON write is needed — the close-gate reads the `qa.result` event directly from the event log.

### Step 5 — Branch on result

**Overall = pass:**
- Output QA report
- Emit stage transition to Close:
  ```bash
  mustard-rt run emit-pipeline --kind pipeline.stage --spec {specName} --payload "{\"stage\":\"Close\"}"
  ```
- Output: "QA passed. All criteria met. Run `/mustard:close` or proceed to CLOSE."

**Overall = fail:**
- Output QA report with failing criteria
- Output: "QA failed. Fix the following before re-running /mustard:qa:"
  - List each FAIL criterion with its command
- Track iteration count in memory (no pipeline-state write). After 3 failures: STOP and `AskUserQuestion`: "QA has failed 3 times. Manual intervention required. Review the failing criteria and decide: (a) Fix and retry, (b) Relax the AC in the spec, (c) Abort pipeline."

**Overall = skip (no AC section):**
- Warn user: "No Acceptance Criteria in spec — QA skipped. Consider adding AC before CLOSE."
- Pipeline may proceed (QA is advisory when no AC exists).

### Step 6 — Tactical Fix Discovery após QA Pass (advisory)

Quando o overall é `pass`, antes de avançar para o CLOSE: olhar o retorno do QA agent (ou a saída do `qa-run` quando o agent gerou notas adjacentes) por uma seção `## Tactical Fix Candidates` / `## Candidatos a Tactical Fix`. Cada item é um fix tático que passou nos AC mas merece sub-spec própria — critérios de qualificação em `pipeline-config.md § Tactical Fix Discovery` (≤100 LOC, sem mudança de contrato público, sem decisão pendente, sem nova dependência).

Para cada candidato, o orquestrador imprime uma linha de sugestão:

```
Tactical fix candidate (post-QA): <descrição>
Run: /mustard:tactical-fix <parent-spec> "<descrição>"
```

**Advisory.** NÃO bloqueia o CLOSE, NÃO força fix-loop, NÃO segura a transição de fase. O user decide se cria a sub-spec antes do CLOSE, depois, ou nunca. Se não há seção de candidatos no retorno, pular silenciosamente.

QA `fail` mantém o fluxo do Step 5 (não chega aqui). Tactical-fix é para *fixes adjacentes* descobertos durante QA — nunca para um AC que falhou.

### Step 7 — CLOSE check

Before proceeding to CLOSE (either here or in `/mustard:close`), close-gate will verify `qa.result` event with `overall=pass` exists in harness log.

## Return Format

```
[QA] spec: {spec-name}

- AC-1: ✅ PASS — exit 0 (2.3s)
- AC-2: ❌ FAIL — exit 1 (0.8s) — stderr: {excerpt}

Overall: FAIL (1 of 2 failed)

→ Next: fix AC-2, then run /mustard:qa again
```

## Rules

- NEVER run QA before EXECUTE phase completes
- NEVER modify code during QA — QA is read-only execution
- Maximum 3 QA iterations per pipeline
- close-gate blocks CLOSE without qa.result=pass in events log
- `MUSTARD_QA_GATE_MODE=warn` — allows CLOSE with stderr warning even if QA absent
- `MUSTARD_QA_GATE_MODE=off` — skips QA check entirely in close-gate
