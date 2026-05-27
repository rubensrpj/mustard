# Drenagem comportamental W4 + W5 (no-sqlite) — pre-W6

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-27T11:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de drenagem (wave-18-rt-followups) que zera a dívida comportamental acumulada nas waves W4A-C + W5A-B antes de entrar em W6 (dashboard). Foram triados 12 itens; 3 entram como Tier A (fix agora), 1 como Tier A de verificação, 8 como Tier B com pointer para wave futura. Justifica o teto de 5 arquivos (vs cap de 5 por sub-spec) porque drenagem multi-item cabe em mudanças cirúrgicas (≤30 LOC cada).

### Triagem (12 itens)

| # | Item (origem) | Tier | Resolução |
|---|---|---|---|
| W4#1 | `amend_finalize::run` não é mais chamado em `SessionEnd` (regression W3B) | **A** | Religar em `apps/rt/src/hooks/session_cleanup.rs` (1 chamada fail-open) |
| W4#2 | W4B cap estourado | n/a | Já registrado, sem ação |
| W4#3 | `mustard-rt run memory feedback --id <i64>` aceita arg mas dispatcher ignora e chama `run_feedback` com `path` vazio (sempre retorna "memory file not found") | **A** | Renomear clap `--id` → `--path` (`PathBuf`), propagar via `DispatchExtras.feedback_path` |
| W4#4 | `spec-extract --measure` perdeu `context_cost_frames` (telemetria SQLite-only) | **B** | Defer wave-economy (W7 territory) |
| W4#5 | `spec-children` perdeu `started_at`/`completed_at`/`reason`/`wave` correlation | **B** | Aceitar como simplificação documentada (já registrado em wave-12-rt/spec.md); follow-up via `.events/*.ndjson` walk se necessidade aparecer |
| W4#6 | wave-15-rt | done | Resolvido em `420bcec` |
| W4#7 | Warnings espúrios `[BOUNDARY WARNING] ... is outside the boundaries declared in spec "2026-05-26-dashboard-i18n-migration"` em quase toda edição: `check_boundaries` em `apps/rt/src/hooks/post_edit.rs` percorre TODOS os specs e pega o primeiro com `## Boundaries`, sem filtrar por stage/outcome ativo — a spec `dashboard-i18n-migration` (`Close + Active + followup_open`) sempre vence | **A** | Filtrar `check_boundaries` para specs com `SpecOutcome::Active` e `Stage ∈ {Analyze, Plan, Execute}` (usa `mustard_core::spec::parse_state`) |
| W5#8 | OTEL attribution (`lookup_attribution` two-tier) sumiu junto com TelemetryStore | **B** | Defer W6 (dashboard) — `SpecRecord.extra` já carrega spec/session_id/tool_use_id; dashboard re-deriva |
| W5#9 | `mcp/get_spec_metrics` retorna `tool_breakdown` e `dispatch_failures_by_phase` vazios | **B** | Defer W7 (core-economy NDJSON) |
| W5#10 | `mcp/get_run_summary` zero até OTEL collector receber tráfego real | **B** | Defer W8B (smoke fixture) |
| W5#11 | `check_subtractions` agora lê `pipeline.telemetry.subtraction` no NDJSON — validar se algum producer legado ainda escreve `mustard.subtraction.applied` na tabela `events` SQLite | **A** | Verificação: nenhum producer ativo em `apps/rt/src/**` ou `packages/core/src/**` escreve `mustard.subtraction.applied` (apenas `apps/dashboard/src-tauri/**` referencia, e essas referências são reader-only contra SQLite legacy a ser apagado em W6). Doc como verificado. |
| W5#12 | 4 tests deletados em W5B p/ reintro em W8B | **B** | Já no plano (W8B smoke fixture) |

### Investigation log

- **W4#1**: `apps/rt/src/hooks/session_cleanup.rs::observe()` (linhas 472-489) chama `ingest_rtk_savings`, `ingest_session_transcript`, `prune_telemetry`, `archive_stale_followups`, `clean_pipeline_states`, `clean_statusline_cache`, `clean_compact_state`, `clean_otel_pid` — sem `amend_finalize::run`. A função `crate::run::amend_finalize::run(&session_id)` é segura para chamar fail-open e foi removida em W3B sem reintrodução.
- **W4#3**: `apps/rt/src/run/mod.rs:354` declara `id: Option<i64>`; `mod.rs:1556-1569` constrói `DispatchExtras` com `id` mas `feedback_path: None`. `apps/rt/src/run/memory.rs:1038-1047` chama `run_feedback` com `path: extras.feedback_path.clone().unwrap_or_default()` — sempre vazio. `run_feedback` (linha 896) faz `if !opts.path.exists() { report["error"] = "memory file not found"; return; }`. Fix: rename arg, drop campo morto `id`, popular `feedback_path` via clap `--path`.
- **W4#7**: `apps/rt/src/hooks/post_edit.rs::check_boundaries` (linha 432) faz `entries.into_iter().filter(|e| e.is_dir)` direto, sem checagem de status; primeira spec com `## Boundaries` ou `## Limites` vence (line 480 — comentário "the first spec with a boundary section wins" confirma). `dashboard-i18n-migration/spec.md` tem `## Limites` (linha 67) e está em `Stage: Close + Outcome: Active`. Fix: ler header via `spec::parse_state`, pular se `outcome != Active` ou `stage ∈ {Close, Review, QA}`.
- **W5#11**: `rtk grep "mustard.subtraction.applied"` retorna 1 hit em `apps/rt/src/run/otel/diagnose.rs:16` (apenas doc-comment dizendo que `pipeline.telemetry.subtraction` é a NDJSON face); e 3 hits em `apps/dashboard/src-tauri/` (legacy SQLite reader, alvo de W6). Zero producers ativos em apps/rt ou packages/core.

### Files (5)

- `apps/rt/src/hooks/session_cleanup.rs` (MODIFY) — religar `amend_finalize::run`
- `apps/rt/src/hooks/post_edit.rs` (MODIFY) — filtrar `check_boundaries` por active+non-terminal
- `apps/rt/src/run/mod.rs` (MODIFY) — clap `--id` → `--path` para `memory feedback`
- `.claude/spec/2026-05-26-no-sqlite-git-source-of-truth/wave-18-rt-followups/spec.md` (CREATE) — esta spec
- `.claude/spec/2026-05-26-no-sqlite-git-source-of-truth/wave-18-rt-followups/meta.json` (CREATE) — header twin

## Critérios de Aceitação

- [x] AC-18-1: `cargo build -p mustard-rt` passa. Command: `cargo build -p mustard-rt`
- [x] AC-18-2: `cargo test -p mustard-rt --no-run` passa. Command: `cargo test -p mustard-rt --no-run`
- [x] AC-18-3: `apps/rt/src/hooks/session_cleanup.rs` referencia `amend_finalize` na função `observe`. Command: `bash -c "grep -n 'amend_finalize' apps/rt/src/hooks/session_cleanup.rs | grep -v '^[0-9]*://'"`
- [x] AC-18-4: clap `memory feedback` aceita `--path` (struct field `path: Option<PathBuf>` com `#[arg(long)]`) e NÃO declara `id: Option<i64>`. Command: `bash -c "grep -qE 'path: Option<PathBuf>' apps/rt/src/run/mod.rs && ! grep -qE 'id: Option<i64>' apps/rt/src/run/mod.rs"`
- [x] AC-18-5: `check_boundaries` em `post_edit.rs` consulta `spec::parse_state`. Command: `bash -c "grep -nE 'parse_state|SpecOutcome|Stage::' apps/rt/src/hooks/post_edit.rs"`
- [x] AC-18-6: zero producers ativos de `mustard.subtraction.applied` em `apps/rt/src/**` ou `packages/core/src/**` (doc-comments excluídos). Command: `bash -c "count=$(grep -rE 'event.*=.*\"mustard\\.subtraction\\.applied\"|emit.*\"mustard\\.subtraction\\.applied\"' apps/rt/src packages/core/src 2>/dev/null | grep -v '^.*://' | wc -l); test \"$count\" = \"0\""`

## Plano

## Arquivos

- `apps/rt/src/hooks/session_cleanup.rs`
- `apps/rt/src/hooks/post_edit.rs`
- `apps/rt/src/run/mod.rs`
- `.claude/spec/2026-05-26-no-sqlite-git-source-of-truth/wave-18-rt-followups/spec.md`
- `.claude/spec/2026-05-26-no-sqlite-git-source-of-truth/wave-18-rt-followups/meta.json`

## Tarefas

1. `session_cleanup.rs::observe` — após `clean_otel_pid(&claude);`, adicionar `if let Some(sid) = input.session_id.as_deref() { let _ = crate::run::amend_finalize::run(sid); }` (fail-open: erro de janela amend não pode abortar SessionEnd cleanup).
2. `post_edit.rs::check_boundaries` — antes do `let Ok(content) = fs::read_to_string(&spec_file) else { continue; };`, ler `spec.md` via `mustard_core::spec::parse_state(&content)` (precisa carregar o content antes); se `state.outcome != SpecOutcome::Active` ou `state.stage` ∈ {`Close`, `Review`, `QA`}, `continue`. Mantém legacy specs sem header como pass-through (parse_state None = não filtra).
3. `run/mod.rs` — em `RunCmd::Memory`, renomear o campo `id: Option<i64>` para `path: Option<PathBuf>`; clap arg `#[arg(long)] path: Option<PathBuf>` (substitui `#[arg(long)] id: Option<i64>` e atualiza comentário de `feedback only — target memory file path`). No `match` (linha 1556-1569), passar `feedback_path: path` em vez de `id`. Remover o campo morto `id` de `DispatchExtras` em `memory.rs` se houver — manter assinatura compat se for cross-arquivo (preferir clean rename).
4. Build: `rtk cargo build -p mustard-rt`.
5. Tests: `rtk cargo test -p mustard-rt --no-run`.

## Dependências

Depende de W4A-C + W5A-B (todos commitados em `dev_rubens`).

## Limites

- ≤5 arquivos (3 MODIFY + 2 CREATE) — dentro do cap padrão
- Sem stubs SQLite
- Sem behavior change para code paths não-listados aqui
- Drenagem Tier A: 3 fixes + 1 verificação documentada
- Tier B (8 itens): cada um aponta para wave futura específica
- Commit message: `chore(wave-18/rt-followups): drain W4+W5 behavioral debt`