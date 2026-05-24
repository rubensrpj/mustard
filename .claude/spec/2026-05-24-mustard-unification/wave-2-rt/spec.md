# W2 — Scan cold-path via Claude CLI (sem ANTHROPIC_API_KEY)

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: light
### Checkpoint: 2026-05-24T19:30:00Z
### Lang: pt-BR

## Contexto

Após o project-profiler (W2 entregue), o cold-path interpret (`apps/rt/src/run/scan/interpret.rs`) chama o modelo Sonnet via SDK Anthropic, exigindo `ANTHROPIC_API_KEY` no ambiente. Sem chave, cai em "fallback agnóstico vazio" e o `entity-registry.json` sai com `entities: []`. A regra do projeto (já formalizada em `feedback_llm_via_claude_cli`): **todo LLM call passa por subprocess do binário `claude`** (Claude CLI — a assinatura do user já paga).

## Tarefas

- [ ] **T2.1.** Reescrever a invocação do modelo em `apps/rt/src/run/scan/interpret.rs` para `Command::new("claude").args(["--print", "--model", "sonnet"])` com prompt via stdin. Capturar stdout como resposta. Sem `reqwest`, sem `api.anthropic.com`.
- [ ] **T2.2.** Substituir referências a `ANTHROPIC_API_KEY` por probe de `claude` binary no PATH. Se ausente: log warning, retorna vazio (mantém fail-open atual).
- [ ] **T2.3.** Atualizar `apps/rt/src/run/doctor.rs`: trocar check de `ANTHROPIC_API_KEY` por `which claude` / `where claude`. Reportar `claude_cli: { present: bool, path: Option<String> }`.
- [ ] **T2.4.** Atualizar `apps/rt/CLAUDE.md` (guard) para documentar nova invocação. Remover trecho que menciona `ANTHROPIC_API_KEY required`.
- [ ] **T2.5.** Atualizar `apps/cli/templates/commands/mustard/scan/SKILL.md` removendo qualquer menção à API key.
- [ ] **T2.6.** Regression test em `apps/rt/tests/`: injeta fake `claude` binary no PATH (shell script `echo '{"clusters":[{"id":"x","label":"y"}]}'`) e valida que o cold-path consome o stdout dele.
- [ ] **T2.7.** Rodar `mustard-rt run sync-registry` no monorepo Mustard e validar `entities[]` não-vazio.
- [ ] **T2.8.** Emit `pipeline.economy.operation.invoked { operation: "scan-cold-path", duration_ms, tokens_used: 0 }` para alimentar `/economia` (W12). Tokens são 0 do ponto de vista do agente que disparou (vai pela assinatura do `claude` CLI).

## Files

- `apps/rt/src/run/scan/interpret.rs` (reescrita do canal de invocação)
- `apps/rt/src/run/doctor.rs` (check do binário)
- `apps/rt/CLAUDE.md` (atualizar guard)
- `apps/cli/templates/commands/mustard/scan/SKILL.md` (remover menção a API key)
- `apps/rt/tests/scan_cold_path.rs` (novo regression test)

## Critérios de Aceitação

- [ ] **AC-2.1.** `apps/rt/src/run/scan/interpret.rs` não importa nem referencia `reqwest`, `api.anthropic.com`, nem `ANTHROPIC_API_KEY`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/scan/interpret.rs','utf8');for(const k of ['reqwest','api.anthropic.com','ANTHROPIC_API_KEY']){if(t.includes(k)){console.error('still has',k);process.exit(1)}}"`
- [ ] **AC-2.2.** `interpret.rs` invoca `Command::new("claude")`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/scan/interpret.rs','utf8');if(!/Command::new\\(\"claude\"\\)/.test(t))process.exit(1)"`
- [ ] **AC-2.3.** `mustard-rt run doctor --json` reporta `claude_cli`. Command: `rtk mustard-rt run doctor --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.checks?.find(c=>c.name==='claude_cli'))process.exit(1)})"`
- [ ] **AC-2.4.** Regression test `scan_cold_path_uses_fake_binary` passa. Command: `rtk cargo test -p mustard-rt scan_cold_path 2>&1 | grep -q "ok"`
- [ ] **AC-2.5.** `entity-registry.json` em projeto Mustard tem `entities[]` não-vazio após scan. Command: `rtk mustard-rt run sync-registry && node -e "const j=JSON.parse(require('fs').readFileSync('.claude/entity-registry.json','utf8'));if(!Array.isArray(j.entities)||j.entities.length===0)process.exit(1)"`

## Notas

- `apps/rt/CLAUDE.md` atual menciona `MUSTARD_SCAN_MODEL` (sonnet/opus/haiku). Manter (controla qual modelo passar ao `claude --model`).
- Paralelizável com W1.
- Bloqueia W3 (meta-sidecar consome entity-registry para validar paths das specs).
