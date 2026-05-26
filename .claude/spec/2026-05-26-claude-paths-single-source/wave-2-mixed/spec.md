# W2 — Migrar 33 call-sites rt + reorg físico + ancorar via `workspace_root()`

### Stage: Execute
### Outcome: Active
### Flags:
### Checkpoint: 2026-05-26T00:00:00Z

## Contexto

Esta é a wave de maior LOC mas a mais mecânica. Cada call-site segue um padrão de substituição estável **com dois primitivos** introduzidos em W1:

```rust
// antes
let dir = cwd.join(".claude").join(".qa-reports");

// depois
let root = mustard_core::workspace_root(cwd)?;
let dir = ClaudePaths::for_project(root).for_spec(spec_name)?.qa_report_json_path();
// (ou .qa_report_html_path() / .economy_baselines_path() / .diff_md_path() conforme o caso)
```

Para os 4 caches globais a substituição é direta:

```rust
// antes
let p = claude.join(".knowledge-seen.json");

// depois
let root = mustard_core::workspace_root(cwd)?;
let p = ClaudePaths::for_project(root).knowledge_seen_path();
```

**Crítico**: a entrada de `workspace_root()` é sempre o `cwd` cru (ou `input.cwd` do `HookInput`); a saída é a raiz do workspace. Nunca passar `cwd` direto para `ClaudePaths::for_project()` — sempre via `workspace_root()`. AC-W2.8 trava regressão.

Para hooks que recebem `HookInput` (com campo `cwd`), o ponto único de mudança é `dispatch.rs` resolver `workspace_root(input.cwd)?` uma vez e propagar como `WorkspaceRoot` (newtype) no `Ctx`, em vez de propagar `cwd` cru.

Reorg físico: quando um path muda de localização (ex: `.qa-reports/{spec}.json` → `spec/{spec}/qa-report.json`), a migração inclui mover o arquivo no filesystem do projeto Mustard (não fazer cleanup do legado — projeto está em fase dev [[feedback_no_migration_dev_phase]]).

## Tarefas

- [ ] **T2.1** — Consolidar 4 caches em `.claude/.cache/`. Arquivos:
  - `apps/rt/src/run/scan_orchestrate.rs:489` (`.detect-cache.json`)
  - `apps/rt/src/run/scan_orchestrate.rs:536` (`.scan-dispatch.json`)
  - `apps/rt/src/hooks/knowledge.rs:433` (`.knowledge-seen.json`)
  - `apps/rt/src/hooks/knowledge.rs:672` (`.memory-seen.json`)
  - `apps/rt/src/run/scan_finalize.rs:69,275,284` (leitor de `.scan-dispatch.json`)
  - `apps/rt/src/run/skills.rs:132` (leitor de `.detect-cache.json`)

  Após substituição: `mkdir .claude/.cache/` e mover arquivos existentes manualmente (1x, no commit da wave).

- [ ] **T2.2** — Per-spec: mover `.qa-reports/{spec}.json` → `spec/{spec}/qa-report.json`. Arquivos:
  - `apps/rt/src/run/qa_run.rs:398-411` (writer)
  - `apps/rt/src/run/event_projections.rs:866` (writer)
  - `apps/rt/src/run/metrics.rs:397` (writer)
  - `apps/rt/src/run/verify_pipeline.rs:527` (writer)

  Atenção: writer hoje grava `{spec}.json` e `{spec}.html` no mesmo dir; passar a gravar `qa-report.json` e `qa-report.html` dentro de `spec/{spec}/`.

- [ ] **T2.3** — Per-spec: mover `.pipeline-states/{spec}.{wave}.diff.md` → `spec/{spec}/wave-N-{role}/diff.md`. Arquivos:
  - `apps/rt/src/hooks/post_edit.rs` (writer de diff)
  - `apps/rt/src/run/emit_pipeline.rs` (writer de prompt + warnings)
  - `apps/rt/src/run/diff_context.rs` (writer)

  Atenção: o nome atual codifica `{spec}.{wave}.diff.md` no caminho; passa a usar o dir da wave (`wave-N-{role}/diff.md`). Quando `wave_slug` não está disponível no call-site, a writer recebe `WavePaths` como parâmetro do chamador.

- [ ] **T2.4** — Per-spec: mover `.economy-baselines.json` (global hoje) → `spec/{spec}/economy-baselines.json` (per-spec). Arquivos:
  - `apps/rt/src/run/economy_capture_baseline.rs:61`
  - `apps/rt/src/run/economy_reconcile.rs:45`
  - `apps/rt/src/run/economy_report.rs:30`

  Migração de dados: o arquivo global atual tem entradas chaveadas por `{operation}/{wave}` — agora cada spec carrega seu próprio arquivo com as entradas dela. Para a spec mãe [[2026-05-25-mustard-deep-refactor]] (já completed), copiar entradas relevantes manualmente.

- [ ] **T2.5** — `claude_dir_prune::DOCUMENTED_DIRS` deriva de `ClaudePaths::documented_dirs()`. Arquivo: `apps/rt/src/run/claude_dir_prune.rs:81-87` substituído por chamada da struct. Adicionar `.cache` à lista canônica.

- [ ] **T2.6** — Janitor `session_start.rs` chama `ClaudePaths::audit_orphans()` no lugar da lógica atual. Arquivo: `apps/rt/src/hooks/session_start.rs`. Preservar comportamento "WARN não bloqueia" ([[2026-05-25-mustard-deep-refactor/wave-2-mixed/spec.md#T2.3]]).

- [ ] **T2.7** — Migrar restantes (hooks, run-faces sem categoria acima):
  - `apps/rt/src/hooks/bash_guard.rs`
  - `apps/rt/src/hooks/close_gate.rs`
  - `apps/rt/src/hooks/model_routing.rs`
  - `apps/rt/src/hooks/path_guard.rs`
  - `apps/rt/src/hooks/pre_compact.rs`
  - `apps/rt/src/hooks/prompt_gate.rs`
  - `apps/rt/src/hooks/session_cleanup.rs`
  - `apps/rt/src/run/agent_prompt_render.rs`
  - `apps/rt/src/run/complete_spec.rs`
  - `apps/rt/src/run/doctor.rs`
  - `apps/rt/src/run/env.rs`
  - `apps/rt/src/run/epic_fold.rs`
  - `apps/rt/src/run/event_route.rs`
  - `apps/rt/src/run/exec_rewave_check.rs`
  - `apps/rt/src/run/mod.rs`
  - `apps/rt/src/run/pipeline_state_ingest.rs`
  - `apps/rt/src/run/pipeline_summary.rs`
  - `apps/rt/src/run/spec_link.rs`

  Cada um: localizar `.join(".X")` ou `.join("Y/")`, substituir por método da struct, rodar `cargo check -p mustard-rt` após cada arquivo (não acumular falhas).

- [ ] **T2.8** — Atualizar `.gitignore` (raiz do repo) para refletir nova árvore:
  - Remover: `.claude/.pipeline-states/`, `.claude/.qa-reports/`
  - Adicionar: `.claude/.cache/`, `**/.claude/spec/*/qa-report.{json,html}`, `**/.claude/spec/*/wave-*/diff.md`, `**/.claude/spec/*/wave-*/prompt.md`, `**/.claude/spec/*/wave-*/warnings.txt`, `**/.claude/spec/*/economy-baselines.json`

- [ ] **T2.9** — Propagar `workspace_root()` no dispatcher dos hooks. Arquivo: `apps/rt/src/dispatch.rs`. Hoje `build_ctx()` (linha 62) usa `input.cwd.clone().unwrap_or_default()` direto. Passa a chamar `workspace_root(&input.cwd.unwrap_or_default())?` antes de construir o `Ctx`. Em caso de erro do walker: o dispatcher é fail-open ([[core-fail-open-error]]) — loga o erro estruturado, retorna no-op JSON, exit 0. Hook não bloqueia user por falha de resolução.

- [ ] **T2.10** — Substituir `env::project_dir()` em `apps/rt/src/run/env.rs:15` por chamada a `workspace_root()`. Não há fallback para `current_dir()` — se falhar, run subcomando retorna erro tipado e exit code != 0 (não fail-open: run subcomandos são síncronos, user precisa ver o erro). Migrar todos os call-sites desta função para receberem `Result<WorkspaceRoot>` em vez de `String`.

- [ ] **T2.11** — Cada caminho construído pelos call-sites migrados passa pela função-folha correspondente (definida em W1: `harness_db_path()`, `pipeline_states_dir()`, etc., dentro da struct `ClaudePaths`). Assert pós-construção contra I1: se o path resultante contiver `.claude/.claude/`, debug `panic!`; release retorna erro tipado. Já coberto por T1.6 — esta tarefa só checa que **nenhum** call-site migrado constrói path manual fora da struct.

## Critérios de Aceitação

- [ ] **AC-W2.1** — Compila. Command: `rtk cargo build -p mustard-rt`
- [ ] **AC-W2.2** — Clippy limpo. Command: `rtk cargo clippy -p mustard-rt -- -D warnings`
- [ ] **AC-W2.3** — Testes verdes. Command: `rtk cargo test -p mustard-rt`
- [ ] **AC-W2.4** — Zero literais de path volatile em `apps/rt/src/` fora de chamadas para `ClaudePaths`. Command: `rtk node -e "const{execSync}=require('child_process');const out=execSync('rtk grep -rn --include=\"*.rs\" \"\\\\.join.\\\"\\\\.\" apps/rt/src',{encoding:'utf8'});const violations=out.split('\\n').filter(l=>l&&!/ClaudePaths|test/.test(l));if(violations.length>0){console.error(violations.join('\\n'));process.exit(1)}"`
- [ ] **AC-W2.5** — `claude_dir_prune::DOCUMENTED_DIRS` não existe mais como constante; derivado da struct. Command: `rtk node -e "const t=require('fs').readFileSync('apps/rt/src/run/claude_dir_prune.rs','utf8');if(/const DOCUMENTED_DIRS/.test(t))process.exit(1)"`
- [ ] **AC-W2.6** — `.claude/.cache/` existe e contém os 4 caches. Command: `rtk node -e "const fs=require('fs');for(const f of ['detect-cache.json','scan-dispatch.json','knowledge-seen.json','memory-seen.json']){if(!fs.existsSync('.claude/.cache/'+f))process.exit(1)}"` (após rodar `mustard-rt run sync-detect` uma vez para popular)
- [ ] **AC-W2.7** — `.claude/.qa-reports/`, `.claude/.pipeline-states/`, `.claude/.economy-baselines.json`, `.claude/.scan-dispatch.json`, `.claude/.detect-cache.json`, `.claude/.knowledge-seen.json`, `.claude/.memory-seen.json` ausentes na raiz. Command: `rtk node -e "const fs=require('fs');for(const p of ['.qa-reports','.pipeline-states','.economy-baselines.json','.scan-dispatch.json','.detect-cache.json','.knowledge-seen.json','.memory-seen.json']){if(fs.existsSync('.claude/'+p))process.exit(1)}"`
- [ ] **AC-W2.8** — Zero chamadas `ClaudePaths::for_project(` recebendo `cwd` direto em `apps/rt/src/` (deve sempre receber resultado de `workspace_root()`). Command: `rtk node -e "const{execSync}=require('child_process');const out=execSync('rtk grep -rn --include=\"*.rs\" \"ClaudePaths::for_project\" apps/rt/src',{encoding:'utf8'});const bad=out.split('\\n').filter(l=>l&&/for_project\\((cwd|input\\.cwd|&cwd|current_dir)/.test(l));if(bad.length>0){console.error(bad.join('\\n'));process.exit(1)}"`
- [ ] **AC-W2.9** — `cargo test -p mustard-rt` rodado 10x consecutivos não cria `apps/rt/.claude/` (testes usam fixture tempdir via `MUSTARD_WORKSPACE_ROOT`). Command: `rtk powershell -Command "Remove-Item -Recurse -Force apps/rt/.claude -ErrorAction SilentlyContinue; 1..10 | ForEach-Object { cargo test -p mustard-rt --quiet }; if (Test-Path apps/rt/.claude) { exit 1 }"`
- [ ] **AC-W2.10** — Run subcomando sem âncora ancestral falha com exit code != 0. Command: `rtk powershell -Command "$tmp = New-TemporaryFile; Remove-Item $tmp; New-Item -Type Directory $tmp | Out-Null; Push-Location $tmp; $env:MUSTARD_WORKSPACE_ROOT=$null; mustard-rt run sync-registry 2>&1 | Out-Null; $ec = $LASTEXITCODE; Pop-Location; Remove-Item -Recurse -Force $tmp; if ($ec -eq 0) { exit 1 }"`

## Limites

`apps/rt/src/**/*.rs` (33 arquivos listados + `dispatch.rs` + `run/env.rs`), `.gitignore` (raiz), `apps/rt/tests/common/mod.rs` (helper de fixture). Pode mexer em `packages/core/src/claude_paths.rs` ou `packages/core/src/workspace.rs` se W1 deixou bug, mas não adicionar API nova (volta pra W1 se precisar).

Helper de fixture novo em `apps/rt/tests/common/mod.rs`: `test_workspace()` cria tempdir com `mustard.json + .claude/` mínimo e seta `MUSTARD_WORKSPACE_ROOT` via guard RAII que limpa no `Drop`. Testes que dependem de cwd real migram para usar o helper — é como AC-W2.9 vira verde.

OUT: `apps/dashboard/`, `apps/cli/`, `templates/`. Migração de dashboard + contrato é W3.

## Role

mixed (predominante rt; toca `.gitignore` que é root)
