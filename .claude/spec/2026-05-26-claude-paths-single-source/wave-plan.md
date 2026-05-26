# Plano de ondas — Single source of truth para paths de `.claude/`

## Contexto

4 waves sequenciais (cada uma destrava a próxima). Não há paralelização possível: os primitivos (W1: struct + walker) são pré-requisito da migração rt (W2), que é pré-requisito da migração dashboard + doctor + contrato (W3), que é pré-requisito da limpeza retroativa (W4 — precisa do doctor pra verificar antes/depois).

## Diagrama de dependências

```
W1 claude-paths-struct + workspace-root (primitivos do core)
  ↓
W2 rt-migrate-callsites + dispatch-via-walker (33 arquivos rt + reorg + dispatch.rs + env.rs)
  ↓
W3 dashboard-doctor-contract + leak-checks (3 arquivos dashboard + 3 doctor checks + CLAUDE.md)
  ↓
W4 retroactive-cleanup (one-shot delete + verificação via doctor)
```

## Tabela de ondas

| # | Spec | Role | Depende de | Resumo |
|---|---|---|---|---|
| 1 | [[wave-1-rt]] | rt | — | **Dois primitivos no core.** `packages/core/src/claude_paths.rs`: struct `ClaudePaths` com a árvore canônica completa. APIs: `for_project(root)`, `for_spec(root, name)`, `for_wave(root, name, wave_slug)`. Métodos `documented_dirs()`, `cache_files()`, `per_spec_artifacts()`, `audit_orphans()`. Guard I1 em `for_project()` (path não pode terminar em `.claude` ou conter `.claude/.claude/`). `packages/core/src/workspace.rs`: função `workspace_root(start_dir) -> Result<PathBuf, WorkspaceError>` com ancestor walk procurando `mustard.json + .claude/` no mesmo diretório, falha alto sem âncora, atravessa `.git/` de submodules, honra override `MUSTARD_WORKSPACE_ROOT`, memoize por processo, rejeita `.claude/.claude/` resolvido. Testes cobrem: defaults, idempotência, `for_spec` rejeita name vazio, `for_wave` rejeita slug malformado, walker resolve da raiz/subprojeto, falha sem âncora, atravessa submodule, rejeita I1, honra env, rejeita env inválido, memoize. Zero call-sites migrados nesta wave. |
| 2 | [[wave-2-mixed]] | mixed | [[1]] | Migra os 33 arquivos `apps/rt/src/` que constroem paths de `.claude/`, **passando sempre via `workspace_root()?`** antes de `ClaudePaths::for_project(root)`. Atualiza `dispatch.rs` (build_ctx resolve walker uma vez e propaga como newtype `WorkspaceRoot` no `Ctx`; fail-open em erro). Atualiza `run/env.rs` (substitui `project_dir()` pelo walker; não fail-open — run subcomandos retornam erro tipado). Consolida 4 caches em `.claude/.cache/`. Move `.qa-reports/{spec}.{json,html}` → `spec/{spec}/qa-report.{json,html}`. Move `.pipeline-states/{spec}.{wave}.diff.md` → `spec/{spec}/wave-N-{role}/diff.md`. Move `.economy-baselines.json` → per-spec. `claude_dir_prune::DOCUMENTED_DIRS` deriva da struct. Helper `test_workspace()` em `apps/rt/tests/common/mod.rs` para testes pararem de poluir `apps/rt/.claude/`. |
| 3 | [[wave-3-mixed]] | mixed | [[1]], [[2]] | Migra os 3 arquivos `apps/dashboard/src-tauri/src/`: `watcher.rs`, `db.rs`, `commands-catalog.ts`. Adiciona **três** checks novos no doctor: `--check claude-paths` (filesystem vs catálogo), `--check workspace-leaks` (`.claude/` em subprojetos do workspace classificado por scan-output vs estado-vivo), `--check i1` (qualquer `.claude/.claude/` aninhado, falha crítica). Default `doctor` (sem flag) agrega os três. Reescreve seção em `apps/cli/templates/CLAUDE.md` apontando para `ClaudePaths` como fonte canônica. Atualiza memória [[feedback_claude_dir_audit]]. |
| 4 | [[wave-4-rt]] | rt | [[1]], [[2]], [[3]] | **Limpeza retroativa one-shot.** Rodar doctor antes (salvar `leaks-before.json`, `i1-before-{mustard,sialia}.json`). Delete `apps/{cli,rt,dashboard}/.claude/` no repo Mustard. Delete `.claude/.claude/` em Mustard e sialia. Limpeza seletiva em `c:\Atiz\sialia\backend\Sialia.Backend\.claude\` (preservar `commands/`, `skills/`, `agents/`, `services.json`; remover `.harness/`, `.agent-state/`, `.agent-memory/`, `.metrics/`, `memory/`, `plans/`). Re-rodar doctor depois (salvar `*-after-*.json`). AC compara antes/depois. |

## Paralelização

Nenhuma — cadeia W1 → W2 → W3 → W4 é estritamente sequencial.

## Cobertura — críticas e pedidos do usuário

| Pedido / crítica desta sessão | Onde resolve |
|---|---|
| 36 paths hardcoded em strings literais | W2 (rt) + W3 (dashboard) |
| `.qa-reports/`, `.pipeline-states/`, `.economy-baselines.json` deveriam ser per-spec | W2 (move físico para `spec/{name}/`) |
| `.detect-cache.json`, `.scan-dispatch.json`, `.knowledge-seen.json`, `.memory-seen.json` espalhados na raiz | W2 (consolida em `.cache/`) |
| `DOCUMENTED_DIRS` duplicado entre `claude_dir_prune.rs` e contrato narrativo | W1 + W2 (struct é fonte; `claude_dir_prune` deriva) |
| Contrato em `CLAUDE.md` impossível de auditar | W3 (doctor check + referência à struct) |
| `.events/` é canal específico, não pasta-guarda-tudo | W1 (struct preserva `.events/` e `.blobs/` como APIs separadas de `qa-report`/`diff`) |
| Cache ≠ output — não misturar | W1 (struct separa `cache_files()` de `per_spec_artifacts()`) |
| Sem arquivo `paths.toml` externo | spec.md Não-Objetivos |
| Sem migração de arquivos versionados antigos | spec.md Não-Objetivos (sai via `git rm --cached` separado) |
| Hooks escrevendo em `apps/*/.claude/` em vez da raiz | W1 (`workspace_root()`) + W2 (`dispatch.rs` propaga newtype) |
| `.claude/.claude/` ativo em sialia | W1 (guard I1 no walker + `for_project`) + W3 (doctor `--check i1`) + W4 (limpeza) |
| Três cópias de `mustard.db` em `apps/{cli,rt,dashboard}/` | W2 (todos passam por `workspace_root()`) + W4 (delete os órfãos) |
| Fixtures de teste do `cargo test` poluindo `apps/rt/.claude/` | W2 (`test_workspace()` helper + `MUSTARD_WORKSPACE_ROOT`) |
| `cwd.join(".claude")` cru espalhado | W2 (substituição mecânica via struct + walker) |
| Sialia.Backend dual-mode (submodule vs standalone) | W1 (walker atravessa `.git/` de submodule; standalone tem própria âncora) |
| Subprojeto vendido (`.git/` próprio) com `.claude/` próprio | Aceito — Claude Code lê em runtime; Mustard ignora exceto via scan explícito (W2 mantém contrato `subproject_dir` no scan) |

## Não-Objetivos (ondas)

- Tocar `commands/`, `skills/`, `refs/`, `recipes/`, `agents/` — leitores estáveis, fora de escopo.
- Mexer em `mustard.json`, `entity-registry.json`, `settings.json`, `pipeline-config.md` — config canônica na raiz, paths já estáveis.
- Renomear `.claude/` para outra raiz — convenção do Claude Code.
- Suporte a override por env var ou TOML — Mustard opinionado.

## Riscos eliminados por design

| Risco | Eliminação |
|---|---|
| Struct e código divergirem (refatorar struct quebra call-site não migrado) | W1 entrega só struct + testes; W2 migra tudo em uma única wave; AC-G3 da spec mãe trava regressão |
| Dashboard ler paths antigos enquanto rt escreve nos novos | W3 depende de W2; ordem garantida |
| `claude_dir_prune` apagar `.cache/` por engano (não está em `DOCUMENTED_DIRS` antigo) | W2 deriva lista da struct; `.cache/` é membro canônico |
| Artefato per-spec ficar órfão quando spec é arquivada | W2 move pra dentro de `spec/{name}/`; arquivar = `rm -rf` do dir resolve |
| Doctor reportar falso positivo (filesystem tem path criado por outra ferramenta) | W3: `doctor --check claude-paths` separa STRICT (deve existir) de OBSERVED (pode existir, não-fatal) |
| Fallback silencioso ao cwd quando não há âncora | W1: `workspace_root()` retorna `AnchorNotFound` — não tem fallback |
| Scan escrevendo em path errado por acidente de cwd | W2: contrato do scan exige `subproject_dir` parâmetro explícito |
| `.claude/.claude/` é impossível por construção | W1: guard em `workspace_root` + guard em `ClaudePaths::for_project` + W3: doctor check `i1` falha crítica |
| Regressão futura (módulo novo faz `cwd.join(".claude")`) | W3: doctor `workspace-leaks` detecta; AC-W2.8 grep trava ao rodar localmente |
| Limpeza acidental durante operação em curso | W4: depende de W3 (mecanismo correto antes de cleanup); doctor only-reports (sem `--fix`); operação manual humana |
