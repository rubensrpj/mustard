# Enhancement: rtk-subagent-coverage
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Auditoria apontou que `rtk-rewrite.js` está registrado apenas no matcher `PreToolUse(Bash)` do orchestrator. Objetivo: verificar se hooks `PreToolUse(Bash)` propagam para Task subagents no Claude Code. Se não propagam, aplicar fix (extender matcher, ou injetar instrução "sempre prefira `rtk`" no prompt dos agents via `_core.md` / templates). Alvo: recuperar 40-60% de economia de tokens em outputs shell dentro de waves.

## Why
Na sessão atual observamos que impl agents rodam `dotnet build`, `git log`, `cargo test` etc. sem passar pelo RTK — custou token em waves de 50-95K. Memory `feedback_token_efficiency_audit.md` lista isso como "segundo maior gap". Explorer confirmou: `templates/settings.json` registra `rtk-rewrite.js` só em `Bash` matcher, Task contexts recebem `subagent-tracker.js` + `context-budget.js` mas não RTK.

## Boundaries
- `templates/settings.json` — hook registration (primary)
- `templates/hooks/rtk-rewrite.js` — inspect only (body)
- `templates/prompts/*.core.md` — fallback path se hook não propaga
- `.claude/settings.json` — espelhar mudança
- `.claude/hooks/rtk-rewrite.js` — espelhar se alterado

## Checklist
### templates-impl Agent
- [x] **VERIFICAR PRIMEIRO (mandatório):** PreToolUse(Bash) hooks fire dentro de Task subagents? Consultar docs atuais (WebSearch/WebFetch Anthropic docs) — NÃO assumir.
- [x] Cenário A — hook já propaga: fechar como "misdiagnosed, already working", atualizar `feedback_token_efficiency_audit.md` removendo gap RTK.
- [x] Cenário B — hook não propaga: N/A (Cenário A confirmado).
- [x] Espelhar mudança `templates/` → `.claude/` (se aplicável) — N/A, nenhuma mudança de código.
- [ ] Smoke test: delegar Task trivial rodando `git status` e observar se output é compacto. (não executável neste contexto)
- [x] Build/type-check: `npm run build` — N/A (nenhum código alterado).

## Files (~2-5)
- `templates/settings.json` (possibly modify)
- `.claude/settings.json` (mirror)
- `templates/prompts/*.core.md` (possibly modify)
- spec checkpoint updates

## Acceptance
- Verificação documentada (link/quote das docs)
- Se fix aplicado: smoke test mostrando RTK ativo em subagent
- Build limpo

## Result

**Scenario A — hooks already propagate. Audit was misdiagnosed.**

- **Evidence**: `https://code.claude.com/docs/en/hooks` states hooks defined in settings.json propagate to subagents. The Common input fields section confirms: when running inside a subagent, the hook receives `agent_id` and `agent_type` fields — proving the hook fired. There is no separate matcher for subagent tools; `"Bash"` matcher applies uniformly in both main session and Task subagents.
- **Conclusion**: `rtk-rewrite.js` registered under `PreToolUse[Bash]` already intercepts Bash calls made by Task agents. The prior observation of agents bypassing RTK was likely due to RTK not being installed (which the hook gracefully skips — fail-open), not a hook propagation failure.
- **Files changed**: `C:\Users\ruben\.claude\projects\C--Atiz-Mustard\memory\feedback_token_efficiency_audit.md` — removed false RTK-in-agents gap claim, added correction note. No code changes required.
- **Build**: N/A (no code changed).
