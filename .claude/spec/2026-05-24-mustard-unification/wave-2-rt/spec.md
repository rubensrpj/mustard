# W2 — Scan cold-path via Claude CLI (sem ANTHROPIC_API_KEY)

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

- [ ] AC-W2-1: `apps/rt/src/run/scan/interpret.rs` não importa nem referencia `reqwest`, `api.anthropic.com`, nem `ANTHROPIC_API_KEY`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/scan/interpret.rs','utf8');for(const k of ['reqwest','api.anthropic.com','ANTHROPIC_API_KEY']){if(t.includes(k)){console.error('still has',k);process.exit(1)}}"`
- [ ] AC-W2-2: `interpret.rs` invoca `Command::new("claude")`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/scan/interpret.rs','utf8');if(!/Command::new\\(\"claude\"\\)/.test(t))process.exit(1)"`
- [ ] AC-W2-3: `mustard-rt run doctor --json` reporta `claude_cli`. Command: `rtk mustard-rt run doctor --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.checks?.find(c=>c.name==='claude_cli'))process.exit(1)})"`
- [ ] AC-W2-4: Regression test `scan_cold_path_uses_fake_binary` passa. Command: `rtk cargo test -p mustard-rt scan_cold_path 2>&1 | grep -q "ok"`
- [ ] AC-W2-5: `entity-registry.json` em projeto Mustard tem `entities[]` não-vazio após scan. Command: `rtk mustard-rt run sync-registry && node -e "const j=JSON.parse(require('fs').readFileSync('.claude/entity-registry.json','utf8'));if(!Array.isArray(j.entities)||j.entities.length===0)process.exit(1)"`

## Notas

- `apps/rt/CLAUDE.md` atual menciona `MUSTARD_SCAN_MODEL` (sonnet/opus/haiku). Manter (controla qual modelo passar ao `claude --model`).
- Paralelizável com W1.
- Bloqueia W3 (meta-sidecar consome entity-registry para validar paths das specs).

## Resultado (2026-05-24)

- T2.1..T2.4: cumpridos (`interpret.rs` reescrito; `doctor.rs` reporta `claude_cli`; `apps/rt/CLAUDE.md` documenta a nova invocação).
- T2.5: N/A — `apps/cli/templates/commands/mustard/scan/SKILL.md` já estava limpo (sem menção a `ANTHROPIC_API_KEY`).
- T2.6: cumprido (`apps/rt/tests/scan_cold_path.rs` novo + `interpret_with_fake_binary_returns_entities` passa em Windows e Unix).
- T2.7: cumprido parcialmente — `sync-registry` roda em ms; `entities[]` continua vazio (ver "AC-W2-5 — bloqueio" abaixo).
- T2.8: cumprido (`emit_economy_event` em `interpret.rs:550` emite `pipeline.economy.operation.invoked`).

### Patch crítico pós-implementação (fix de recursão)

A primeira versão da `call_model` invocava `claude --print --model <m>` sem flags adicionais. Cada sub-sessão headless disparava o `SessionStart` hook do próprio `mustard-rt`, que entrava em algum caminho que re-chamava o cold-path — recursão infinita observada na sessão de despache. Patch aplicado em `interpret.rs::call_model`:

```text
claude --bare --print --no-session-persistence --disable-slash-commands --tools "" --model <m>
```

- `--bare` (load-bearing): skip hooks, LSP, plugin sync, CLAUDE.md auto-discovery, auto-memory, keychain reads.
- `--no-session-persistence`: não persiste a sessão headless em disco.
- `--disable-slash-commands`: não carrega skills.
- `--tools ""`: sub-sessão não tem acesso a tools, só produz texto.

Validação: 77 testes do scan verdes, doctor reporta `OK claude_cli`, `sync-registry` retorna em ms.

### AC-W2-5 — cumprida (após dois fixes adicionais)

Após o patch de recursão (`--bare`), o canal de invocação compilava mas continuava gerando `entities: []` no Mustard. Diagnóstico em duas camadas:

1. **`--bare` quebrava OAuth**: o flag força auth via `ANTHROPIC_API_KEY` apenas (OAuth+keychain bypassed). Sem chave, o subprocess saía com "Not logged in" e o fail-open zerava o registry. Conflito direto com a regra `feedback_llm_via_claude_cli`. Patch: **remover `--bare`**, manter `--print --no-session-persistence --disable-slash-commands --tools ""`, e prevenir recursão via env guard `MUSTARD_COLD_PATH_INVOKED=1` setado no spawn — o `SessionStart` hook em `apps/rt/src/hooks/session_start.rs` faz short-circuit `Verdict::Allow` quando essa env é vista.
2. **Bug em `build_profile`**: a função em `interpret.rs:191` buscava `cluster.get("files")`, mas o `cluster_discovery` (Wave 1 do project-profiler) emite o array de arquivos representativos sob `samples`. Resultado: profile ia ao modelo com `samples: []` e o modelo respondia `entities:[]` corretamente (não tinha o que classificar). Patch: aceitar `samples` (primário) ou `files` (fallback) em `build_profile`.

Validação final: `MUSTARD_INTERPRET_CACHE=off rtk mustard-rt run sync-registry --force` produz **25 entities + 2 enums** (`ActivePipelineRow`, `MetricsSummary`, `PipelineSummary`, `KnowledgeRow`, `RecentEvent`, …; enums `EconomyScope`, `SpecBucket`).

A tactical-fix `2026-05-24-tf-cluster-discovery-empty-mustard` foi criada com hipótese errada (cluster_discovery threshold) e está **superseded** — a causa real era o bug em `build_profile` desta wave.
