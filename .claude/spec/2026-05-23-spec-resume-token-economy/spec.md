# Token Economy do `/mustard:spec` resume

## Resumo

O fluxo `/mustard:spec` (continuar spec em EXEC) está gastando ~60k tokens só pra começar a primeira wave — antes mesmo do primeiro Edit do agente. Auditoria do resume da spec `2026-05-23-dashboard-design-system` nesta sessão mostrou: ~25k de tokens evitáveis vindo de (a) leitura do ref `resume-flow.md` (323 linhas) p/ rotear uma decisão simples, (b) leitura do ref `agent-prompt.md` (180 linhas) p/ montar um prompt, (c) `spec-extract` redumpando ~200 linhas que o orchestrator já tinha lido, (d) `diff-context --subproject` retornando o stat global de divergência em vez do escopo pedido, (e) 5 chamadas `emit-pipeline` sequenciais. Esta spec move trabalho do LLM (caro) para o `mustard-rt` (gratuito): um único comando `resume-bootstrap` decide modo + caminho operacional + necessidade de diff/slice, um `agent-prompt-render` materializa o prompt server-side, e `diff-context` passa a respeitar o `--subproject`. Os refs viram folha fina (≤80 linhas, só invariantes + tabela de estados). Meta concreta: corte de ≥40% no contexto até o primeiro Task dispatch num resume típico.

## Network

- Parent: — (spec independente; não é tactical-fix de outra)
- Depende de: registry SQLite + event-store já presentes (sem migração)
- Habilita: `/mustard:spec resume` mais barato; reutilizável por `/mustard:feature` e `/mustard:bugfix` que compartilham o template de agent-prompt

## Component Contract

### `mustard-rt run resume-bootstrap --spec <name> [--json]`

Comando único que substitui Steps 0 + 0.5 + 1 + 5 + parte do 2 do `resume-flow.md` atual. Retorna JSON único:

```json
{
  "mode": "continued" | "reanalyzed" | "ask",
  "stage": "Plan" | "Execute" | "Analyze" | "QaReview" | "Close",
  "operationalSpecPath": ".claude/spec/{name}/spec.md" | ".claude/spec/{name}/wave-N-{role}/spec.md",
  "isWavePlan": true | false,
  "currentWave": 5,
  "totalWaves": 6,
  "isStub": false,
  "lastDispatchFailure": null | { "at": "ISO", "ageMs": N, "agentType": "...", "description": "...", "prompt": "..." },
  "needsDiff": true | false,
  "needsContextSlice": true | false,
  "waveModel": "opus" | "sonnet" | null,
  "specSummary": "primeira linha de ## Resumo (≤200 chars)",
  "agentRoles": ["ui"]
}
```

Internamente:
1. Lê `pipeline_state_for_spec` da SQLite.
2. Aplica regra auto-continue: `lastEventAt` ≤10min + `status=in_progress` + sem `lastDispatchFailure` → `mode=continued`.
3. Resolve operational spec: root `spec.md` vs `wave-plan.md` (e nesse caso resolve wave N atual via projection ou via `wave-tree` interno).
4. Detecta stub (header `Stage: Plan` + sem `## Files`/`## Arquivos`/`## Tasks`/`## Tarefas`).
5. Decide `needsDiff` = `true` se houve `pipeline.wave.complete` desde o último `pipeline.resume_mode` (transição de wave força refresh).
6. Decide `needsContextSlice` = mesma regra de `needsDiff`.
7. Lê wave-plan p/ extrair `waveModel` da linha da current wave.
8. **Emite** o evento `pipeline.resume_mode` antes de retornar (LLM não precisa emitir).
9. Lê primeira heading `## Resumo` ou `## Summary` do operational spec, devolve em `specSummary`.

Fail-open: qualquer erro de IO degrada o campo afetado para `null`/`false`, **nunca** falha. Exit code sempre 0.

### `mustard-rt run agent-prompt-render --spec <name> --wave <N> --role <ui|backend|...> --subproject <path> [--mode first|granular|fix-loop] [--retry-context-file <path>]`

Materializa o prompt completo do agente substituindo `{placeholders}` no template embedded via `include_str!`. Reads:
- `{subproject}/CLAUDE.md` p/ `{guards_summary}` (extrai só a section `## Guards`).
- `{subproject}/.claude/agents/{role}-impl.md` p/ decidir se `{role_block}` vai vazio.
- Wave-plan p/ `waveModel`.
- Spec da wave atual p/ `{task_steps}` (recorta `## Tarefas` da wave).
- Cached context-slice + diff em `.claude/.pipeline-states/{spec}.{context-md,wave-N-1.diff}.md` (se existirem).
- Cross-wave memory via `memory cross-wave` (in-process, sem subprocess).

Output: stdout = prompt string final, pronta p/ Task tool. Stderr = warnings sobre placeholders deixados vazios (graceful degrade).

### `mustard-rt run diff-context` — fix do escopo de subproject

Quando `--subproject <path>` é passado, **todos** os trechos da saída precisam respeitar o filtro: changes since divergence, commit log, staged/unstaged files. Hoje o "since divergence" sai global. Fix: aplicar `git_scoped` em todas as chamadas, não só algumas. Adicionar regression test.

## Arquivos

- `apps/rt/src/run/resume_bootstrap.rs` (NEW)
- `apps/rt/src/run/agent_prompt_render.rs` (NEW)
- `apps/rt/src/run/diff_context.rs` (modify)
- `apps/rt/src/run/mod.rs` (registrar 2 novos `RunCmd` variants)
- `.claude/commands/mustard/spec/SKILL.md` (Step 2 + Step 5 routing usam novos cmds)
- `apps/cli/templates/commands/mustard/spec/SKILL.md` (mirror)
- `.claude/refs/spec/resume-flow.md` (slim ≤80 linhas — só invariantes + tabela de estados)
- `apps/cli/templates/refs/spec/resume-flow.md` (mirror)
- `.claude/refs/agent-prompt/agent-prompt.md` (slim ≤60 linhas — só placeholders + retry modes + nota de prefix cache)
- `apps/cli/templates/refs/agent-prompt/agent-prompt.md` (mirror)
- `apps/rt/src/run/agent_prompt_template.md` (NEW — template embedded via `include_str!` se ainda não existir como asset)

## Informações da Entidade

N/A — adição de comandos `mustard-rt run` + edição de docs/SKILL. Sem entidade nova.

## Tarefas

### Wave 1 — rt commands + SKILL update (general, model: opus)

#### `apps/rt/src/run/resume_bootstrap.rs`

- [x] Criar módulo seguindo `rt-run-subcommand-pattern`: struct `ResumeBootstrap` com `clap::Args`, função `run(spec: &str, json: bool) -> ()`.
- [x] Carregar `pipeline_state_for_spec(spec)` (reuso direto, já `pub` em `event_projections`).
- [x] Detectar wave-plan via `wave_tree::load_wave_tree(spec_dir)` (helper interno; expor se preciso).
- [x] Detectar stub: ler primeiras 30 linhas do operational spec, regex `Stage:\s*Plan` + ausência de `^##\s+(Files|Arquivos|Tasks|Tarefas)\b`.
- [x] Compute `needsDiff` / `needsContextSlice`: query `events WHERE spec=? AND kind='pipeline.wave.complete' AND ts > (SELECT MAX(ts) FROM events WHERE kind='pipeline.resume_mode' AND spec=?)`.
- [x] Ler wave-plan.md p/ pegar `waveModel` da coluna "Modelo" da row da current wave (parse markdown table).
- [x] Ler `## Resumo` ou `## Summary` heading do operational spec → primeira linha não-vazia.
- [x] Detectar `lastDispatchFailure`: query SQLite p/ último `pipeline.dispatch_failure` sem `cleared:true` posterior; compute `ageMs`.
- [x] Emitir `pipeline.resume_mode` ANTES de retornar (idempotente — se já emitido nos últimos 10s p/ esta spec, skip).
- [x] Output: stdout = pretty JSON se `--json`, senão tabela texto compacta.
- [x] Fail-open em TODO erro de IO — campo vira `null`/`false`, nunca panic.

#### `apps/rt/src/run/agent_prompt_render.rs`

- [x] Criar módulo. Template embedded: `include_str!("agent_prompt_template.md")`.
- [x] Args: `spec`, `wave: Option<u32>`, `role`, `subproject: PathBuf`, `mode: First|Granular|FixLoop`, `retry_context_file: Option<PathBuf>`.
- [x] Resolver operational spec via mesma lógica do `resume_bootstrap` (extrair em helper compartilhado em `wave_lib` ou novo `resume_lib`).
- [x] Ler `{subproject}/CLAUDE.md` → extrair section `## Guards` (regex `^## Guards\n([\s\S]*?)(?=\n## |\z)`).
- [x] Checar existência de `{subproject}/.claude/agents/{role}-impl.md` → se existe, `{role_block}` = "".
- [x] Ler `{spec}/wave-plan.md` p/ `waveModel` (mesma regex do bootstrap; reusar helper).
- [x] Ler `## Tarefas` / `## Tasks` da operational spec → `{task_steps}` via `spec_extract` interno.
- [x] Cached files: `.claude/.pipeline-states/{spec}.context-md.md` → `{context_md}`; `.claude/.pipeline-states/{spec}.wave-{N-1}.diff.md` → injetar como bloco no prompt.
- [x] Cross-wave memory: chamar `memory_cross_wave::collect(spec, wave)` (refatorar se hoje só tem CLI wrapper).
- [x] Substituir placeholders em ordem: PREFIX-STABLE primeiro (cache-friendly), VARIABLE depois. Validar zero `{placeholder}` remanescente; warning stderr p/ cada não-substituído.
- [x] Output: prompt completo em stdout. Sem framing JSON — string raw pronta p/ Task tool.

#### `apps/rt/src/run/diff_context.rs` (modify)

- [x] Auditar TODAS as chamadas `git(&cwd, &[...])` no módulo; trocar pelas variantes `git_scoped` que respeitam `--subproject` quando `sub_path.is_some()`.
- [x] Foco: a section "Commits since divergence" e o "Changed files since divergence" hoje não passam pelo scoped path. Aplicar `-- <subproject>` no `git log ... <base>..HEAD` e no `git diff --stat <base>..HEAD`.
- [x] Adicionar teste regression em `apps/rt/tests/` (ou inline `#[cfg(test)]`): mock cwd com 2 subdirs, commit em ambos, verificar que com `--subproject sub1` o output não menciona `sub2/`.

#### `apps/rt/src/run/mod.rs`

- [x] Registrar `mod resume_bootstrap;` e `mod agent_prompt_render;`.
- [x] Adicionar variants em `RunCmd`: `ResumeBootstrap { spec, json }` e `AgentPromptRender { spec, wave, role, subproject, mode, retry_context_file }`.
- [x] Wire em `main.rs` dispatch (ou wherever `RunCmd::Variant => module::run(...)` mora).

#### `apps/rt/src/run/agent_prompt_template.md` (NEW)

- [x] Conteúdo: o template hoje em `.claude/refs/agent-prompt/agent-prompt.md` (Dispatch Template + Minimal Retry Template), sem o material expositivo sobre prompt cache + skill loading — só os 2 templates literais com `{placeholders}`.

#### SKILL update — `.claude/commands/mustard/spec/SKILL.md` + mirror em `apps/cli/templates/commands/mustard/spec/SKILL.md`

- [x] Step 1 (AUTO-SYNC) permanece.
- [x] Step 2 vira: rodar `rtk mustard-rt run active-specs --format table` (inalterado) — sem mudança.
- [x] Step 5 roteamento PLAN/EXEC: refatorar p/ chamar `mustard-rt run resume-bootstrap --spec X --json` em UM passo; ler o JSON de retorno; routear baseado em `stage` + `mode`. Remover Steps 0/0.5/1 inteiros do ref `resume-flow.md` (movidos pra dentro do binário).
- [x] Step 5 EXECUTE: depois do bootstrap, despachar via `mustard-rt run agent-prompt-render ... | <pass to Task>` — orchestrator NÃO monta mais o prompt manualmente.
- [x] Atualizar a tabela "Estágio detectado" pra mencionar a nova API.

#### Refs slim — `.claude/refs/spec/resume-flow.md` + `.claude/refs/agent-prompt/agent-prompt.md` (+ mirrors)

- [x] `resume-flow.md`: cortar de 323 → ≤80 linhas. Manter só: (a) Step 12c wave plan scope, (b) Step 12d dependency precheck routing, (c) tabela de Escalation Statuses, (d) INVIOLABLE RULES. Steps 0/0.5/1/5/6/Bootstrap apagados (movidos pra `resume-bootstrap` binário).
- [x] `agent-prompt.md`: cortar de 180 → ≤60 linhas. Manter só: (a) explicação do `{placeholders}` (tabela compacta), (b) Retry Modes (3 estados), (c) nota de prompt cache (1 parágrafo). Apagar o template literal duplicado (agora em `agent_prompt_template.md` do binário).

### Wave 1 — Validação

- [x] `cargo build -p mustard-rt` verde.
- [x] `cargo test -p mustard-rt` verde (incl. teste regression novo de diff-context scope).
- [x] Smoke: `cargo run -p mustard-rt -- run resume-bootstrap --spec 2026-05-23-dashboard-design-system --json` retorna JSON válido com `mode`, `stage`, `operationalSpecPath`, `currentWave: 5`, `totalWaves: 6`, `waveModel: "opus"`.
- [x] Smoke: `cargo run -p mustard-rt -- run agent-prompt-render --spec 2026-05-23-dashboard-design-system --wave 5 --role ui --subproject apps/dashboard` retorna prompt string sem `{placeholder}` remanescente; stdout começa com `<!-- PREFIX-STABLE -->`.

## Dependências

- `mustard-core` (já dep do `mustard-rt`).
- `serde_json` p/ pretty JSON output (já dep).
- `rusqlite` p/ query do event-store (já dep).
- Sem nova dep externa.

## Limites

Editar dentro de:
- `apps/rt/src/run/{resume_bootstrap,agent_prompt_render,agent_prompt_template.md,diff_context,mod}.rs`
- `.claude/commands/mustard/spec/SKILL.md` + mirror em `apps/cli/templates/commands/mustard/spec/SKILL.md`
- `.claude/refs/{spec/resume-flow,agent-prompt/agent-prompt}.md` + mirrors em `apps/cli/templates/refs/`

**Não tocar**:
- Outros comandos `mustard-rt run` (não refatorar `sync-registry`, `active-specs`, etc.).
- `apps/dashboard/**`, `apps/cli/src/**` (não-Rust).
- `packages/core/**` (mudanças aqui virariam outra spec).
- Outros refs ou SKILLs (`/feature`, `/bugfix`, etc.) — embora se beneficiem do mesmo `agent-prompt-render`, integrá-los é escopo separado.
- `apps/rt/src/hooks/**` (enforcement face — não relevante).

## Critérios de Aceitação

- [x] AC-1: build verde — Command: `cargo build -p mustard-rt`
- [x] AC-2: testes passam — Command: `cargo test -p mustard-rt`
- [x] AC-3: `resume-bootstrap` retorna JSON válido c/ todas as chaves obrigatórias — Command: `node -e "const{execSync}=require('child_process');const j=JSON.parse(execSync('cargo run -q -p mustard-rt -- run resume-bootstrap --spec 2026-05-23-dashboard-design-system --json',{encoding:'utf8'}));const req=['mode','stage','operationalSpecPath','isWavePlan','currentWave','totalWaves','isStub','needsDiff','needsContextSlice','specSummary'];for(const k of req){if(!(k in j)){console.error('missing key',k);process.exit(1)}}console.log('ok')"`
- [x] AC-4: `agent-prompt-render` produz prompt sem `{placeholder}` remanescente — Command: `node -e "const{execSync}=require('child_process');const p=execSync('cargo run -q -p mustard-rt -- run agent-prompt-render --spec 2026-05-23-dashboard-design-system --wave 5 --role ui --subproject apps/dashboard',{encoding:'utf8'});if(/\\{[a-z_]+\\}/.test(p)){console.error('unfilled placeholder');process.exit(1)}if(!p.includes('PREFIX-STABLE')){console.error('missing prefix marker');process.exit(1)}console.log('ok')"`
- [x] AC-5: `diff-context --subproject apps/dashboard` não menciona caminhos fora de `apps/dashboard/` na section "Changed files since divergence" — Command: `node -e "const{execSync}=require('child_process');const o=execSync('cargo run -q -p mustard-rt -- run diff-context --subproject apps/dashboard',{encoding:'utf8',maxBuffer:50*1024*1024});const i=o.indexOf('Changed files since divergence');if(i<0){console.log('ok');process.exit(0)}const j=o.indexOf('##',i+5);const sec=j>0?o.slice(i,j):o.slice(i);if(sec.includes('apps/cli/')||sec.includes('apps/rt/')||sec.includes('packages/core/')){console.error('subproject scope leak');process.exit(1)}console.log('ok')"`
- [x] AC-6: ref `resume-flow.md` ≤80 linhas — Command: `node -e "const fs=require('fs');const n=fs.readFileSync('.claude/refs/spec/resume-flow.md','utf8').split('\\n').length;if(n>80){console.error('resume-flow.md has',n,'lines');process.exit(1)}console.log('ok')"`
- [x] AC-7: ref `agent-prompt.md` ≤60 linhas — Command: `node -e "const fs=require('fs');const n=fs.readFileSync('.claude/refs/agent-prompt/agent-prompt.md','utf8').split('\\n').length;if(n>60){console.error('agent-prompt.md has',n,'lines');process.exit(1)}console.log('ok')"`
- [x] AC-8: SKILL spec menciona `resume-bootstrap` em Step 5 — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('.claude/commands/mustard/spec/SKILL.md','utf8');if(!c.includes('resume-bootstrap')){console.error('SKILL did not adopt resume-bootstrap');process.exit(1)}console.log('ok')"`
- [x] AC-9: mirrors em `apps/cli/templates/` casam com os de `.claude/` — Command: `node -e "const fs=require('fs');const pairs=[['.claude/commands/mustard/spec/SKILL.md','apps/cli/templates/commands/mustard/spec/SKILL.md'],['.claude/refs/spec/resume-flow.md','apps/cli/templates/refs/spec/resume-flow.md'],['.claude/refs/agent-prompt/agent-prompt.md','apps/cli/templates/refs/agent-prompt/agent-prompt.md']];for(const[a,b]of pairs){if(fs.readFileSync(a,'utf8')!==fs.readFileSync(b,'utf8')){console.error('mirror drift',a,'vs',b);process.exit(1)}}console.log('ok')"`

## Checklist

- [x] `mustard-rt run resume-bootstrap` implementado e testado
- [x] `mustard-rt run agent-prompt-render` implementado e testado
- [x] `diff-context` respeita `--subproject` em todas as sections
- [x] SKILL `/mustard:spec` chama `resume-bootstrap` em vez dos 4-5 comandos antigos
- [x] Refs `resume-flow.md` e `agent-prompt.md` cortados (≤80 e ≤60 linhas)
- [x] Mirrors em `apps/cli/templates/` atualizados em sincronia
- [x] Build + testes verdes
