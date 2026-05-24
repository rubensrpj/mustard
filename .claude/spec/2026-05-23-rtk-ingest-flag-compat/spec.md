# Tactical Fix: rtk ingest compat com versão atual do rtk

## Contexto

Derivado de [[2026-05-22-economia-didatica-e-economias-reais]].

`packages/core/src/economy/sources/rtk.rs:74` chama `rtk gain --json`. A versão atual do binário `rtk` instalado (`rtk.exe gain --help` confirma) só aceita `--format json` — o flag `--json` retorna exit 2 "unexpected argument". Resultado: ingestor falha silenciosamente toda vez (`eprintln!` + 0 records), e o card "Reescrita de comando shell" no dashboard fica perpetuamente em 0 tok, embora `rtk gain` reporte 409M tokens economizados.

Adicionalmente, mesmo com `--format json`, o rtk atual NÃO expõe lista per-rewrite — só summary e (com `--all`) breakdowns por dia/semana/mês. O parser legado em `sources/rtk.rs:126` espera `parsed.as_array()`, mas a saída atual é `{summary: {...}, daily: [...]}` — `as_array()` retorna None e cai no fail-open silencioso.

## Decisão de design

Trocar o comando para `rtk gain --all --format json` (já validado funcionar nesta versão). Parser passa a aceitar dois shapes:

1. **Legado** (`Vec<{command, saved_tokens, model, ...}>`): se `parsed.is_array()`, mantém comportamento atual — útil se uma versão futura do rtk reexpor per-rewrite.
2. **Atual** (`{summary, daily: [{date, saved_tokens, ...}], weekly, monthly}`): extrai `daily[]` e emite 1 `SavingsRecord` por dia. Granularidade temporal preservada; `saved_tokens` por dia é meaningful pro agregado.

`ts` do record passa a ser a data do bucket (`{date}T12:00:00Z`) em vez de `now_iso()` — preserva linha do tempo real. `model_target` fica `None` (rtk não diz qual modelo).

**Idempotência fora de escopo**: re-rodar `mustard-rt run rtk-gain` ainda duplicaria registros, mas (a) usuário roda manualmente, (b) é problema pré-existente, (c) consertar precisa de UNIQUE constraint ou dedup-on-insert que mexe no schema — escopo maior.

## Arquivos

- `packages/core/src/economy/sources/rtk.rs` — atualizar `RealRtkCommand::run` (flag) + `ingest_with` (parser dual-shape) + tests + doc-comment do módulo

## Tarefas

### Library Agent (core)

- [x] `RealRtkCommand::run`: trocar `["gain", "--json"]` por `["gain", "--all", "--format", "json"]`. Atualizar mensagens de erro.
- [x] `ingest_with`: branch no shape parsado:
  - Se `parsed.is_array()` → comportamento legado intacto (mantém tests existentes verdes)
  - Senão se `parsed.get("daily").and_then(as_array)` → iterar, emitir 1 record por dia com `ts = {date}T12:00:00Z` e `tokens_saved = entry.saved_tokens`
  - Senão → fail-open eprintln + Ok(vec![])
- [x] Atualizar doc-comment do módulo para citar o novo shape
- [x] Novo teste: `ingest_with_parses_summary_plus_daily_shape` com fixture do shape atual; assert que retorna 1 record por entry de `daily` com `saved_tokens > 0`
- [x] Manter os 3 tests existentes verdes (array, runner-fail, not-json)
- [x] `cargo build && cargo test -p mustard-core --lib sources::rtk`

### Execução

- [x] Rodar `mustard-rt run rtk-gain` no cwd do user; confirmar rows_persisted > 0 (eyeballing eprintln output)
- [x] Confirmar que o dashboard, após reload, mostra valor não-zero na linha "Reescrita de comando shell"

## Critérios de Aceitação

- [x] AC-1: build core verde — Command: `cargo build -p mustard-core`
- [x] AC-2: testes core verdes — Command: `cargo test -p mustard-core --lib`
- [x] AC-3: novo shape parseado — Command: `bash -c "grep -q '\"daily\"' packages/core/src/economy/sources/rtk.rs && echo ok"`
- [x] AC-4: flag novo no comando — Command: `bash -c "grep -q '\"--format\"' packages/core/src/economy/sources/rtk.rs && echo ok"`

## Limites

- Não alterar schema de `savings_records`
- Não mexer no writer (apenas no parser/source)
- Não consertar idempotência (escopo maior; documentar como follow-up)
- Compatibilidade legacy preservada — array continua sendo parseado
