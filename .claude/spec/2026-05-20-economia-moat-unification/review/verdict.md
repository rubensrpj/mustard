# Review Verdict — economia-moat-unification

### Timestamp Final: 2026-05-21T06:55:00Z

## Resultado final

| Reviewer | Round 1 | Round 2 | Round 3 |
|----------|---------|---------|---------|
| core | REJECTED (1 CRITICAL) | APPROVED ✅ | — |
| rt | APPROVED (2 W rec.) | APPROVED ✅ | — |
| dashboard | APPROVED-fixes (1 CRITICAL cosm.) | REJECTED (2 partial) | APPROVED ✅ |

**Pipeline APPROVED para avançar pra QA (Wave 10).**

## Fix-loop 1 (round 1 → round 2)

### Core
- ✅ `MultiProjectReader::fan_out` closure agora `Fn(&Connection, &ProjectPath)` — bug `projects[0]` corrigido por design da API (não nos call-sites)
- ✅ `scope_where` reescrito sem tautologias `?N=?N`; Wave scope em `economy_summary` via attribution_cte (filtro real)
- ✅ `economy::sources::time` (NEW) extraído — dedupe 3x dos helpers `now_iso`/`epoch_secs_to_ymdhms` (90 LOC menos)
- ✅ `test_economy_summary_wave_scope_filters_to_wave_only` adicionado (regressão pro tautology bug)
- ✅ 268 tests pass (+3)

### RT
- ✅ `bash_guard.rs:1477` — `SavingsSource::BashGuardBlock` → `RtkRewrite` no site rtk-rewrite
- ✅ `bash_guard.rs:1472` — `estimate_input_tokens(&cmd, &env::var("CLAUDE_MODEL"))` em vez de empty string
- ✅ `util/mod.rs` — `home_dir()` + `encode_cwd()` consolidados; `session_cleanup.rs` + `transcript_watcher.rs` importam do util compartilhado
- ✅ 644 tests pass

### Dashboard
- ✅ `i18n.ts` — `useSyncExternalStore` import + void suppressor removidos
- ✅ `NOTICE.md` — placeholders `<year>`/`<authors>` → `"2023–2025 Anthropic, Inc."`
- ✅ `formatTokens` consolidado em `lib/types/economy.ts` — `ExecutionTrace.tsx` + `BaseRow.tsx` importam de lá
- ✅ `CodeBlock.tsx` — grid layout condicional (`block` quando `showLineNumbers=false`)

## Fix-loop 2 (round 2 → round 3) — só dashboard

### Dashboard
- ✅ `i18n.ts:72-77` — JSDoc de `useLang()` reescrito; zero menção restante de `useSyncExternalStore` no arquivo inteiro
- ✅ `format.ts:10` — `formatTokens` reescrito como `export { formatTokens } from "./types/economy";` (re-export do canônico) — uma única definição em todo o codebase

## Concerns aceitos (vão para CLOSE como debt list)

Lista consolidada das 23 Concerns originais das 8 waves: 22 ACEITAS pelos reviewers, 1 NEED-DISCUSSION:
- W4 3º fallback silencioso (span sem `agent.start` cai na coluna própria) — REVIEW pediu adicionar log/counter de "spans sem atribuição" como tactical-fix futuro. Não bloqueia.

## Tactical Fix Candidates surfacedos (não consumidos nesta wave)

Reviewers identificaram 10 candidates totais. 7 foram resolvidos nos fix-loops; 3 ficam como sugestão futura:
- Core: `eprintln!` → `tracing::warn!` (precisa adicionar `tracing` dep no `mustard-core`)
- RT: refactor dos 5 hooks W2 para usar `economy::store::open_for` (já existe; só falta consumir)
- RT: `transcript_watcher` ganhar PID file pra idempotência

## Próximo passo

QA (Wave 10) — `mustard-rt run qa-run --spec 2026-05-20-economia-moat-unification` para rodar os 67+ Acceptance Criteria literalmente.
