# W9 — Context injection optimization

## Contexto

Hoje:

- `SessionStart` injeta top-5 patterns + top-5 decisions + top-5 lessons (~1500 tokens fixos), sem filtrar por spec ativa.
- `SubagentStart` e `UserPromptSubmit` não injetam nada.
- Sub-agentes EXECUTE recebem `CLAUDE.md` inteiro + `entity-registry.json` inteiro.

Alvos:

- Orchestrator inicial `≤25k` tokens.
- Sub-agents Explore/Plan `≤15k`.
- Agents EXECUTE `≤30k`.

## Tarefas

### T9.1 — Reformulação de injeções por evento

| Evento | Hoje | Proposta | Trigger condicional | Custo |
|---|---|---|---|---|
| `SessionStart` | top-5×3 fixo, 1500 chars | scope-by-current-spec: 3 spec-scoped + 2 globais (`spec IS NULL AND confidence >= 0.8`). Cap 1500. | sempre | ~500-1500 tokens |
| `SessionStart` (resume) | nenhum | + Bloco "Pipelines em curso" se `has_active_pipeline`: `{spec} @ wave {N}/{total} — {stage}` + delta tempo. Usa `resume-bootstrap --summary-only` (novo flag). | `has_active_pipeline` | +50-300 tokens |
| `UserPromptSubmit` | só archive | + 1 linha "Pipeline em curso: {spec} @ {stage}" se prompt não inicia `/mustard:*` e há spec ativa | conditional | ~50 tokens |
| `SubagentStart` | só counter | novo hook `subagent_inject`: slice mínimo só se `HookInput.raw.prompt_size < 4000` (Task sem SKILL) | conditional | 0-300 tokens |
| `PostToolUse(Task)` | budget | + observer `auto_capture_summary` (W8 já adicionou) | sempre | 0 (escrita) |
| `PreCompact` | git+pipeline | + até 3 `agent_memory` recentes da spec ativa | `has_active_pipeline` | +200 tokens |
| `SessionEnd` | knowledge | + consolidação: `agent_memory active confidence>=0.7 referenciado 2+x` vira candidate `memory_decisions/lessons` | `has_active_pipeline` | 0 |
| `SubagentStop` | nada | observer: bump `last_used` em memórias citadas no `tool_response` | sempre | 0 |

### T9.2 — Otimização de carga inicial

| Item hoje | Tipo | Como otimizar | Onde |
|---|---|---|---|
| `CLAUDE.md` (root + sub) | markdown ~3-5k linhas | SLICE por seção (`## Guards`, `## Stack`, `## Patterns`) | `agent_prompt_render::read_guards_block` (estender para 2-3 seções) |
| `CONTEXT.md` glossário | markdown ~500 termos | SLICE existente: `context-slice` (cap 250 linhas) | já funciona |
| `entity-registry.json` | JSON ~2k entries | SUMMARY via `knowledge glossary --filter <spec-entities-only>` ou SLICE via `context-resolve` | `apps/rt/src/run/mod.rs` |
| Active pipeline state | JSON ~500 bytes/spec | PARÂMETRO: `resume-bootstrap --json` produz objeto estruturado | já existe |
| Skills index | markdown ~1k linhas | SLICE por role: `recommended_skills` por role já em `guess_recommended_skills` | já funciona |
| Prior wave diff | unified diff ~2k linhas | CACHE já existe `.pipeline-states/{spec}.wave-N.diff.md` | já bom |
| Cross-wave memory | markdown ~10-30 linhas | SUMMARY já existe (cap 5 por wave) | já bom |
| Spec body | markdown 2-10k tokens | PARÂMETRO: já recebe só `## Tasks` via `read_task_steps` | já bom |

### T9.3 — Implementação

- [ ] Modificar `apps/rt/src/hooks/session_start.rs`: scope-by-spec inject; bloco resume condicional.
- [ ] Novo hook `apps/rt/src/hooks/subagent_inject.rs` (observer que injeta condicional).
- [ ] Estender `apps/rt/src/hooks/prompt_gate.rs` (UserPromptSubmit) para injetar 1 linha condicional.
- [ ] Modificar `apps/rt/src/hooks/pre_compact.rs`: adicionar 3 `agent_memory` recentes.
- [ ] Modificar `apps/rt/src/hooks/knowledge.rs` (SessionEnd): consolidação.
- [ ] Novo hook `apps/rt/src/hooks/subagent_stop_bump.rs` (SubagentStop observer).
- [ ] Modificar `apps/rt/src/run/context_slice.rs`: estender para `CLAUDE.md` (heuristically por `## Guards`/`## Stack`/`## Patterns`).
- [ ] Modificar `apps/rt/src/run/agent_prompt_render.rs`: adicionar flag `--budget-tokens N` que trunca placeholders por prioridade.
- [ ] Modificar `apps/rt/src/run/resume_bootstrap.rs`: adicionar flag `--summary-only` + campo `coolness: hot|paused|cooled|cold`.
- [ ] Registrar novos hooks em `apps/rt/src/registry.rs`.
- [ ] Cada injeção emite `pipeline.economy.context.served { role, spec, wave, tokens_served, tokens_full_estimate }` para `/economia`.

## Files

- `apps/rt/src/hooks/session_start.rs`
- `apps/rt/src/hooks/subagent_inject.rs` (novo)
- `apps/rt/src/hooks/prompt_gate.rs`
- `apps/rt/src/hooks/pre_compact.rs`
- `apps/rt/src/hooks/knowledge.rs`
- `apps/rt/src/hooks/subagent_stop_bump.rs` (novo)
- `apps/rt/src/registry.rs`
- `apps/rt/src/run/context_slice.rs`
- `apps/rt/src/run/agent_prompt_render.rs`
- `apps/rt/src/run/resume_bootstrap.rs`

## Critérios de Aceitação

- [ ] AC-W9-1: `SessionStart` em projeto com spec ativa injeta bloco resume. Command: fixture + verify stdout.
- [ ] AC-W9-2: `UserPromptSubmit` injeta 1 linha apenas se prompt não é `/mustard:*` e há spec ativa. Command: fixture.
- [ ] AC-W9-3: Novo hook `subagent_inject` registrado. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/registry.rs','utf8');if(!/subagent_inject/.test(t))process.exit(1)"`
- [ ] AC-W9-4: `context-slice --doc CLAUDE.md --spec X` funciona (não só CONTEXT.md). Command: `rtk mustard-rt run context-slice --doc CLAUDE.md --spec test --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.slice)process.exit(1)})"`
- [ ] AC-W9-5: `agent-prompt-render --budget-tokens 15000` trunca por prioridade. Command: fixture.
- [ ] AC-W9-6: `resume-bootstrap --summary-only --json` retorna objeto com `coolness`. Command: `rtk mustard-rt run resume-bootstrap --summary-only --json --spec test | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!('coolness' in j))process.exit(1)})"`
- [ ] AC-W9-7: Orchestrator inicial em projeto canário ≤25k tokens. Verificável via `context-budget --role main`.
- [ ] AC-W9-8: `pipeline.economy.context.served` aparece em events após session boot. Command: SQL query.

## Notas

- Bloqueia W12 (que conecta os eventos `economy.context.served` ao dashboard).
- W10 (Stop/Notification) pode rodar em paralelo (eventos diferentes).
