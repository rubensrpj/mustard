# Single source of truth para paths de `.claude/`

### Stage: Close
### Outcome: Completed
### Flags: 
### Checkpoint: 2026-05-26T00:00:00Z

## Contexto

Hoje todo path sob `.claude/` é uma string literal repetida nos call-sites. Auditoria desta sessão revelou:

- **33 arquivos** em `apps/rt/src/` constroem paths à mão (`cwd.join(".claude").join(".qa-reports")`, `claude.join(".knowledge-seen.json")`, etc.).
- **3 arquivos** em `apps/dashboard/src-tauri/` fazem o mesmo no lado do leitor.
- O contrato narrativo em [apps/cli/templates/CLAUDE.md](apps/cli/templates/CLAUDE.md) afirma "todo path em `.claude/` deve ter consumidor declarado" — mas a única forma de auditar é grep manual.
- [claude_dir_prune.rs:81-87](apps/rt/src/run/claude_dir_prune.rs#L81) tem uma constante `DOCUMENTED_DIRS` que duplica a lista, com risco de drift permanente.

**Bug correlato fundido em 2026-05-26**: regressão TS→Rust ([[project_no_bun_rust_only]]) também perdeu a resolução estrutural da **raiz** do workspace. No JS, o payload vivia em `.claude/scripts/` e resolvia raiz via `path.resolve(__dirname, "..", "..")` (`.claude/scripts/sync-detect.js:25`). O binário Rust em `$PATH` não tem essa âncora; usa `cwd` cru. Resultado: hooks rodados de dentro de `apps/rt/` escrevem `mustard.db`, `.pipeline-states/`, eventos NDJSON em `apps/rt/.claude/` em vez da raiz do monorepo. Evidência viva: três cópias de `mustard.db` (em `apps/cli`, `apps/rt`, `apps/dashboard`), fixtures de teste vazadas em `apps/rt/.claude/`, e violação ativa de invariante `.claude/.claude/` em `c:\Atiz\sialia\.claude\.claude\.metrics\`. A spec [[2026-05-26-workspace-root-single-anchor]] foi cancelada e absorvida nesta — a `ClaudePaths` precisa receber a raiz de uma fonte correta (`workspace_root()`), não de `cwd` cru.

Além disso, a localização atual mistura escopos na mesma raiz:

| Escopo | Hoje vive em `.claude/` raiz | Deveria viver em |
|---|---|---|
| Cache global cross-spec | `.detect-cache.json`, `.scan-dispatch.json`, `.knowledge-seen.json`, `.memory-seen.json` | `.claude/.cache/` |
| Artefato per-spec | `.qa-reports/{spec}.json`, `.pipeline-states/{spec}.{wave}.diff.md`, `.economy-baselines.json` | `.claude/spec/{name}/` (ao lado do `.events/` que já está lá) |
| Telemetry cross-spec | `.harness/`, `.metrics/` | continua na raiz (correto) |
| Estado de sessão | `.agent-state/` | continua na raiz (correto) |

Auditoria de critério "isso é cache?" (regenerável sem perder informação) deixa claro que `qa-report.json`, `diff.md`, `economy-baselines.json` **não são cache** — são outputs auditáveis do pipeline e morrem com a spec.

Esta spec entrega:

1. **Struct `ClaudePaths`** como catálogo auditável vivo no código.
2. **Função `workspace_root()`** como walker único que ancora a raiz do workspace (ancestor walk procurando `mustard.json + .claude/` no mesmo diretório, falha alto sem âncora).
3. Migração dos 36 call-sites para consumir os dois primitivos.
4. Reorg físico dos artefatos per-spec para dentro de `spec/{name}/`.
5. Doctor estendido com checks `workspace-leaks` e `i1`.
6. Limpeza retroativa one-shot dos rastros do bug.

## Invariantes

- **I1**: `.claude/.claude/` **não existe** em lugar nenhum do workspace. Walker (W1), funções-folha do harness (W1+W2) e doctor (W3) defendem essa invariante de ângulos diferentes.
- **I2**: Existe **uma única** raiz lógica para escrita do harness por execução, resolvida via `workspace_root()`. Falha alto se não houver âncora ancestral.
- **I3**: O scan é o único módulo autorizado a escrever em `subprojeto/.claude/`, e o subprojeto-alvo é parâmetro **explícito** da função, nunca derivado do cwd.

## Usuários/Stakeholders

- **O próprio Mustard** (33 módulos `apps/rt`, 3 módulos `apps/dashboard/src-tauri`) — passa a depender de `mustard_core::claude_paths::ClaudePaths`.
- **Quem mantém o código** — herda 1 ponto único de mudança em vez de 36 strings literais; `doctor` ganha capacidade nova de comparar filesystem vs catálogo.
- **Rubens** (operador) — `.claude/` raiz fica visualmente honesta: só config versionada + caches verdadeiros + telemetry. Per-spec fica dentro da própria spec.

## Métrica de sucesso

- Zero strings literais `".claude/"` seguidas de `.join(".X")` ou `.join("Y/")` em `apps/rt/src/` exceto em `claude_paths.rs`/`workspace.rs`. Verificado por grep.
- Zero chamadas `ClaudePaths::for_project(` recebendo `cwd` direto (sempre via `workspace_root()`).
- Zero `.claude/` em `apps/*/` ou `packages/*/` no repo Mustard após W4.
- Zero `.claude/.claude/` em todo o workspace (invariante I1).
- `cargo test -p mustard-rt` rodado 10x consecutivos não cria `apps/rt/.claude/`.
- Após migração, `find .claude/ -maxdepth 1 -type d` retorna só: `.cache`, `.harness`, `.metrics`, `.agent-state`, `.obsidian`, `commands`, `skills`, `refs`, `recipes`, `agents`, `agent-memory`, `spec`, `graph`.
- `mustard-rt run doctor` (sem flag) reporta zero divergências, zero vazamentos e zero violações I1.
- `claude_dir_prune` deriva `DOCUMENTED_DIRS` de `ClaudePaths::documented_dirs()` (sem lista hardcoded duplicada).
- Build verde: `cargo build --workspace && cargo clippy --workspace -- -D warnings` + `pnpm --filter mustard-dashboard build`.

## Não-Objetivos

- **Arquivo de configuração externo** (`paths.toml`, env var de override). Decisão: Mustard é opinionado; a árvore é fixa, customização não traz valor proporcional ao custo de matriz de combinações.
- **Migrar arquivos histó​ricos versionados** (`.qa-reports/*.json` com 80 entradas commitadas, `.pipeline-states/*` antigos). Saem via `git rm --cached` separado, fora desta spec.
- **Renomear `.claude/` para outra coisa** (`.mustard/`, etc.). Convenção do Claude Code, fora de escopo.
- **Mexer em `commands/`, `skills/`, `refs/`, `recipes/`, `agents/`** — já têm leitores estáveis catalogados pelo scan. Esta spec só toca paths volatile e per-spec.
- **Migrar `entity-registry.json`, `mustard.json`, `settings.json`, `CLAUDE.md`, `pipeline-config.md`** — config canônica na raiz, paths estáveis há meses; toca só se a struct precisar referenciá-los para `doctor`.

## Critérios de Aceitação

ACs autoritativos vivem em cada `wave-N-{role}/spec.md`. ACs globais agregados:

- [ ] **AC-G1.** `cargo build --workspace && cargo clippy --workspace -- -D warnings` passa após todas as ondas. Command: `rtk cargo build --workspace && rtk cargo clippy --workspace -- -D warnings`
- [ ] **AC-G2.** `pnpm --filter mustard-dashboard build` passa. Command: `rtk pnpm --filter mustard-dashboard build`
- [ ] **AC-G3.** Zero strings literais `.claude` + path volatile em `apps/rt/src/` fora de `claude_paths.rs`. Command: `rtk node -e "const{execSync}=require('child_process');const out=execSync('rtk grep -rn --include=\"*.rs\" \".claude\" apps/rt/src',{encoding:'utf8'});const violations=out.split('\\n').filter(l=>l&&!/claude_paths\\.rs/.test(l)&&/\\.join\\(\"\\./.test(l));if(violations.length>0){console.error(violations.join('\\n'));process.exit(1)}"`
- [ ] **AC-G4.** `mustard-rt run doctor --check claude-paths --format json` retorna `{ok: true, divergences: []}`. Command: `rtk mustard-rt run doctor --check claude-paths --format json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.ok||j.divergences.length>0)process.exit(1)})"`
- [ ] **AC-G5.** `.claude/` raiz não contém nenhum dos paths volatile movidos: `.qa-reports/`, `.pipeline-states/`, `.economy-baselines.json`, `.scan-dispatch.json`. Command: `rtk node -e "const fs=require('fs');for(const p of ['.qa-reports','.pipeline-states','.economy-baselines.json','.scan-dispatch.json']){if(fs.existsSync('.claude/'+p))process.exit(1)}"`
- [ ] **AC-G6.** `apps/cli/templates/CLAUDE.md` referencia `ClaudePaths` como contrato canônico (substitui o parágrafo "todo path em `.claude/`..."). Command: `rtk node -e "const t=require('fs').readFileSync('apps/cli/templates/CLAUDE.md','utf8');if(!/ClaudePaths/.test(t))process.exit(1)"`
- [ ] **AC-G7.** `workspace_root()` exportado por `mustard_core` e usado em todos os call-sites (zero `cwd` cru passado para `ClaudePaths::for_project`). Validado por AC-W2.8.
- [ ] **AC-G8.** `doctor` (sem flag) retorna `ok: true` em `c:\Atiz\mustard` após W4. Command: `rtk mustard-rt run doctor --format json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);const allOk=Object.values(j).every(v=>v&&v.ok!==false);if(!allOk)process.exit(1)})"`
- [ ] **AC-G9.** Zero `.claude/` em `apps/{cli,rt,dashboard}/` no repo Mustard após W4. Command: `rtk powershell -Command "foreach ($p in @('c:\Atiz\mustard\apps\cli\.claude','c:\Atiz\mustard\apps\rt\.claude','c:\Atiz\mustard\apps\dashboard\.claude')) { if (Test-Path $p) { exit 1 } }"`
- [ ] **AC-G10.** Zero `.claude/.claude/` em `c:\Atiz\mustard` após W4 (sialia fora de escopo). Command: `rtk powershell -Command "$a=(Get-ChildItem -Recurse -Force c:\Atiz\mustard -ErrorAction SilentlyContinue|Where-Object FullName -match '\.claude[\\/]\.claude'|Measure-Object).Count;if($a -ne 0){exit 1}"`

## Plano

Ver `wave-plan.md`. Resumo:

| W | Nome | Role | Depende | Status |
|---|------|------|---------|--------|
| 1 | claude-paths-struct + workspace-root | rt | — | 📋 |
| 2 | rt-migrate-callsites + dispatch-via-walker | mixed | 1 | 📋 |
| 3 | dashboard-doctor-contract + leak-checks | mixed | 1, 2 | 📋 |
| 4 | retroactive-cleanup | rt | 1, 2, 3 | 📋 |
