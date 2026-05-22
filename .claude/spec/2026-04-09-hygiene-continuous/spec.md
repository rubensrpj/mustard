# Enhancement: hygiene-continuous
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Criar hook `spec-hygiene.js` que roda em SessionStart, varre `.claude/spec/active/`, e classifica cada spec:
- **Auto-move** para `completed/` se `Status: completed|cancelled` E todos `[x]`
- **Warn** (não bloqueante) se `Status: implementing` E todos `[x]` — sugere ao user rodar `/mustard:complete`
- **Silent** caso contrário (spec ainda em progresso normal)

Hoje, essa limpeza só acontece no início de `/mustard:feature` — specs abandonadas ficam em `active/` indefinidamente. Evidência direta: esta sessão começou com 2 specs stale em `active/` (frictionless-permissions, git-flow-simplify).

## Why
Specs stale em `active/` poluem hygiene audits, inflam o contador "N specs em progresso", e confundem `/resume`. Um SessionStart hook resolve sem exigir que user rode comandos — é transparente e roda automaticamente. Também captura casos onde `/complete` foi esquecido após implementação manual.

## Boundaries
- `templates/hooks/spec-hygiene.js` (create)
- `templates/settings.json` — registrar SessionStart
- `.claude/hooks/spec-hygiene.js` — mirror
- `.claude/settings.json` — mirror

## Checklist
### templates-impl Agent
- [x] Ler `templates/hooks/_lib/hook-env.js` — API compartilhada (profile detection, env-based disabling)
- [x] Ler `templates/hooks/subagent-tracker.js` OU outro SessionStart hook existente — reference para stdin/stdout pattern
- [x] Ler `templates/commands/mustard/feature/SKILL.md` seção "Spec Hygiene" — reproduzir a lógica de classificação idêntica
- [x] Criar `templates/hooks/spec-hygiene.js`:
  - Event: SessionStart (ou `matcher: "startup"` conforme convenção Mustard)
  - Varre `.claude/spec/active/*/spec.md` (usar `fs.readdirSync` + `fs.readFileSync`, built-ins only)
  - Para cada spec: parse header (`Status:`, `Phase:`) e checklist (`[x]` vs `[ ]`)
  - Se `Status: completed|cancelled` + todos `[x]` → `fs.renameSync` para `completed/`; também deleta `.claude/.pipeline-states/{name}.json` e `.diff.md` se existirem
  - Se `Status: implementing` + todos `[x]` → log stderr `[hygiene] spec {name} appears done (all tasks checked) but Status=implementing. Run /mustard:complete to finalize.`
  - Silent em todos outros casos
  - Fail-open: try/catch em volta de tudo, exit 0 em erro
  - Respeitar `hook-env.js` profile (ex: `profile === 'minimal'` → skip)
- [x] Registrar em `templates/settings.json` sob SessionStart matcher com timeout ~5000ms
- [x] Espelhar para `.claude/hooks/spec-hygiene.js` e `.claude/settings.json`
- [x] Teste manual: criar spec fake em `active/` com todos `[x]` + Status completed, rodar `rtk node templates/hooks/spec-hygiene.js < empty-payload.json`, confirmar movido; limpar depois
- [x] Build: `rtk npm run build`
- [x] Rodar hook tests: `rtk bun test templates/hooks/__tests__/hooks.test.js`

## Files (~4)
- `templates/hooks/spec-hygiene.js` (create)
- `templates/settings.json` (modify — register hook)
- `.claude/hooks/spec-hygiene.js` (mirror)
- `.claude/settings.json` (mirror)

## Acceptance
- Hook existe, é fail-open, built-ins only
- Registrado no settings.json com SessionStart matcher
- Auto-move funciona em smoke test manual
- Warn funciona para implementing+all-checked
- Silent para specs em progresso normal (parciais)
- Build limpo
- Hooks tests ainda passam (26/26)

## Result

- `templates/hooks/spec-hygiene.js` created (106 lines) — SessionStart hook, fail-open, built-ins only
- `templates/settings.json` + `.claude/settings.json` — registered under `SessionStart/startup`, timeout 5000ms
- `.claude/hooks/spec-hygiene.js` — mirrored
- Smoke test: `test-hygiene` (Status: completed, all [x]) → AUTO-MOVED to `completed/` with stderr log
- Incidental: detected `frictionless-permissions` and `git-flow-simplify` as WARN (implementing+all-checked)
- Bug fixed during impl: checklist regex `(?=^##|\z)` replaced with `(?=\n##\s|$)` for correct JS boundary detection
- Build: PASS (tsc clean)
- Hook tests: 26/26 PASS

## Guards
- NUNCA mover spec se houver `## Concerns` section com `BLOCKED` items
- NUNCA deletar arquivo sem conferir que o move completou com sucesso
- NUNCA falhar a sessão por erro no hygiene (fail-open absoluto)
- Built-ins only — sem npm deps
- Reusar lógica da feature.md "Spec Hygiene" — não divergir
