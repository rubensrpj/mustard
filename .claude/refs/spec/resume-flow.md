# /mustard:spec — Resume flow (continuar pipeline)

Loaded on demand pelo SKILL Step 5 quando `stage=Execute` (ou `Analyze`/`QaReview`/`Close`). Toda a decisão de modo (`continued` vs `reanalyzed`), resolução de operational spec, detecção de stub, decisão de `needsDiff`/`needsContextSlice`, lookup de `waveModel`, parsing de `lastDispatchFailure` e emissão de `pipeline.resume_mode` foram movidas para `mustard-rt run resume-bootstrap --spec X --json` — este ref **não** repete essa lógica. A construção literal do prompt do agente foi movida para `mustard-rt run agent-prompt-render` (template embedded em `apps/rt/src/run/agent_prompt_template.md`). Este ref guarda só o que o binário não pode decidir sozinho.

## Step 12c — Wave Plan Scope (condicional, só se `isWavePlan === true`)

Quando o JSON do bootstrap indica wave plan, o orquestrador despacha só a **wave atual**, nunca a spec inteira:

1. A spec para esta invocação é `operationalSpecPath` retornado pelo bootstrap (já resolvido para `wave-{currentWave}-*/spec.md`).
2. **Entre waves** (post-dispatch da wave N):
   - Commit estilo `/mustard:git commit` com mensagem `feat(wave-{N}/{role}): {summary}`. Fallback: `git add {files} && git commit -m "..."`.
   - Emita wave completion: `mustard-rt run emit-pipeline --kind pipeline.wave.complete --spec {specName} --payload "{\"wave\":{N},\"duration_ms\":{elapsed}}"`. A projeção deriva `completedWaves` + `currentWave` desses eventos — sem JSON state file.
   - Rode `mustard-rt run wave-tree --spec-dir .claude/spec/{specName}` para mostrar progresso.
   - Cache o diff desta wave: `git diff HEAD~1 HEAD > .claude/.pipeline-states/{specName}.wave-{N-1}.diff.md`. O `agent-prompt-render` da próxima wave injeta esse arquivo automaticamente; orquestrador não passa nada explicitamente.
3. Se `currentWave > totalWaves` → pule remaining wave dispatch, siga para REVIEW + CLOSE no overall wave plan.
4. Se uma wave falha (REJECTED após 2 fix-loops, ou BLOCKED) → ver Escalation Statuses abaixo e `../resume/fix-loop-wave.md`.

## Step 12d — Dependency Precheck (factual gate)

Antes de despachar a wave, rode:

```bash
mustard-rt run dependency-precheck --spec {operationalSpecPath}
```

Parse o JSON. Se `ok: false`:

1. Imprima inline: `BLOCKED — N símbolos ausentes: {comma list of missing.symbol}. Sugestão: criar tactical-fix com {suggested_tactical_fix_files}.`
2. Emita `mustard-rt run emit-pipeline --kind pipeline.dispatch_failure --spec {specName} --payload "{\"reason\":\"dependency-precheck-failed\",\"missing\":{N}}"`.
3. AskUserQuestion: **Criar tactical-fix automaticamente** / **Investigar manualmente** / **Forçar dispatch (override)**.
4. Tactical-fix path: `Skill(mustard:tactical-fix)` com `parent={specName}`, descrição derivada dos missing symbols.
5. Override path: `mustard-rt run emit-pipeline --kind pipeline.precheck_override --spec {specName} --payload "{\"reason\":\"user-override\"}"`, depois proceda com dispatch.

**Skip se `resume-bootstrap` retornou `mode: continued`** — cached trust da sessão prior. Skip também se env `MUSTARD_DEPENDENCY_PRECHECK_MODE=off`.

## Escalation Statuses

Após cada agente retornar, cheque o return value antes de avançar:

| Status | Tratamento |
|--------|------------|
| Internal error (no parseable output, empty return, API error) | Re-despache **sequencialmente** (não parallel), mesmo prompt. Max 1 retry/agent. Ainda falhando → STOP + report |
| `CONCERN` | Record verbatim sob `## Concerns` na spec; continue para next wave. ≥2 CONCERNs na mesma wave → surface juntas antes de avançar |
| `BLOCKED` | Pare imediatamente; AskUserQuestion com blocker exato; NÃO avance |
| `PARTIAL` | Aplique Granular Retry Protocol do último step completado (re-dispatch com `--mode granular`); NÃO restart |
| `DEFERRED` | Note na spec com justification; pergunte se o item é load-bearing antes de CLOSE |
| REJECTED (após REVIEW) | Fix Loop Protocol (max 2 loops): re-dispatch com `--mode fix-loop`, re-rode REVIEW. 2 fails → STOP |
| Wave failure (REJECTED 2× / BLOCKED / build fails repetido) | Só se `isWavePlan`. Update `failedWaves`, escreva `failure.md`, AskUserQuestion: fix manually / re-PLAN wave / abort |

Ver `.claude/pipeline-config.md § Escalation Statuses + Diagnostic Failure Routing` para tabela completa, e `../resume/fix-loop-wave.md` para retry/fix-loop detalhado.

## INVIOLABLE RULES

- Main context **IS** o Pipeline Runner — NUNCA wrap em single Task agent.
- NEVER implementar código diretamente — ALL via Task agents (1 per subproject per wave).
- Wave dispatch: TODOS os agentes da mesma wave em UMA SINGLE message.
- Cada sub-agent lê seu próprio `{subproject}/CLAUDE.md` + auto-loads relevant skills (orquestrador NÃO os lê).
- Atualize spec checkboxes em cada transition; pipeline-state vem dos eventos SQLite.
- ALWAYS use `mustard-rt run agent-prompt-render` para montar prompt — NUNCA construir from scratch nem reutilizar o template literal antigo (foi deletado deste ref).
- ALWAYS use `mustard-rt run resume-bootstrap` para decidir modo/path/diff/slice — NUNCA reimplementar essas regras no SKILL.
- ALWAYS rode QA (Wave 10 — `mustard-rt run qa-run --spec {specName}`) após REVIEW e antes de CLOSE — close-gate bloqueia CLOSE sem `qa.result` event.
- ALWAYS rode dependency-precheck (Step 12d) antes de dispatch — block em `ok: false` a menos que user override.
- Wave plan CLOSE só quando `completedWaves.length === totalWaves`; entre waves apenas `═══ WAVE {N-1} COMPLETE — {role} ═══` + stop.
