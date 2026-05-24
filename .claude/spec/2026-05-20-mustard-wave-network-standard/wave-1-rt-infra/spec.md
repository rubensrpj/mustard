# Wave 1 — mustard-rt infra: wikilink-extract + memory cross-wave + schema

## PRD

## Contexto

Sem quatro subcomandos novos no `mustard-rt` e uma tabela `wikilinks` no `mustard.db`, as outras três waves não têm o que consumir. Esta wave entrega a infraestrutura mínima de rede entre waves:

- `wikilink-extract`: parser markdown que extrai `[[name]]` de spec files e grava no SQLite.
- `memory cross-wave`: devolve resumo de memórias das waves prévias dado spec + wave atual.
- `wave-scaffold`: cria a estrutura padrão SDD (wave-plan.md + wave-N-{role}/spec.md + review/spec.md + qa/spec.md) num dir de spec ainda vazio, dado um plano declarado.
- `metrics wave-status --spec <parent>`: devolve JSON `{parent, waves: [{name, status, tokens_saved, duration_ms, retries, cross_wave_memory_bytes, model}]}` agregando os eventos do SQLite agrupados pelo parent.

Reusa o `SqliteEventStore` em `mustard-core` (não duplica conexão) e o pattern de subcomando padrão (`apps/rt/src/run/*.rs`).

## Métrica de sucesso

- `mustard-rt run wikilink-extract --spec-dir <dir>` faz scan de todos os `.md` do dir e devolve JSON com `{wikilinks: [{from, to, file, line}], orphans: [...]}`.
- `mustard-rt run memory cross-wave --spec <s> --wave N` consulta `events` filtrando `kind = 'agent.memory'` para waves 1..N-1 do `--spec` e devolve markdown agregado pronto pra colar no prompt do agente.
- Tabela `wikilinks` populada quando subcomando rodar (idempotente, REPLACE por `(from, to, file)`).
- `cargo check -p mustard-rt` passa, `cargo test -p mustard-rt` passa (smoke test do parser).

## Não-Objetivos

- Não criar UI de validação de links órfãos (só listar no output JSON).
- Não fazer parse semântico do conteúdo das memórias — agregação concatena texto e ranqueia por `ts DESC`.
- Não tocar logica de eventos existentes — só adiciona leitura.
- Não criar migração quebradora — `CREATE TABLE IF NOT EXISTS wikilinks (...)`.

## Acceptance Criteria

- [ ] AC-1: Cargo check passa — Command: `cargo check -p mustard-rt`
- [ ] AC-2: Cargo test passa — Command: `cargo test -p mustard-rt -- wikilink memory_cross_wave wave_scaffold metrics_wave_status`
- [ ] AC-6: `metrics wave-status` expõe flag `--spec` — Command: `bash -c 'mustard-rt run metrics wave-status --help 2>&1 | grep -q -- "--spec"'`
- [ ] AC-3: `wikilink-extract` produz JSON válido com arrays não-vazios para um diretório de fixture montado a runtime — Command: `bash -c 'tmp=$(mktemp -d); printf "# t\n[[a]] [[b]] [[c]]\n" > "$tmp/spec.md"; out=$(mustard-rt run wikilink-extract --spec-dir "$tmp"); rm -rf "$tmp"; node -e "const j=JSON.parse(process.argv[1]); if(!Array.isArray(j.wikilinks)||j.wikilinks.length<3) throw new Error(\"too few: \"+JSON.stringify(j))" "$out"'`
- [ ] AC-4: `memory cross-wave` expõe as flags `--spec` e `--wave` — Command: `bash -c 'out=$(mustard-rt run memory cross-wave --help 2>&1); echo "$out" | grep -q -- "--spec" && echo "$out" | grep -q -- "--wave"'`
- [ ] AC-5: `wikilink-extract` cria o schema (idempotente) — Command: `bash -c 'tmp=$(mktemp -d); printf "# t\n[[a]]\n" > "$tmp/spec.md"; mustard-rt run wikilink-extract --spec-dir "$tmp" >/dev/null; mustard-rt run db-query --sql "SELECT name FROM sqlite_master WHERE type=\"table\" AND name=\"wikilinks\"" | grep -q wikilinks; rc=$?; rm -rf "$tmp"; exit $rc'`

## Plano

## Arquivos (~6)

```
apps/rt/src/run/wikilink.rs                  (new — subcomando + parser)
apps/rt/src/run/memory_cross_wave.rs         (new — subcomando + consulta)
apps/rt/src/run/wave_scaffold.rs             (new — scaffolda wave-N + review + qa)
apps/rt/src/run/metrics_wave_status.rs       (new — agregação por wave agrupada por parent)
apps/rt/src/run/mod.rs                       (modify — register no clap)
packages/core/src/store/wikilinks.rs         (new — schema + CRUD via SqliteEventStore conn)
```

## Tarefas

### Backend Agent

- [ ] `packages/core/src/store/wikilinks.rs`:
  - Struct `Wikilink { from: String, to: String, file: String, line: u32 }`
  - `fn ensure_schema(conn: &Connection) -> Result<()>` — `CREATE TABLE IF NOT EXISTS wikilinks (from TEXT, to TEXT, file TEXT, line INTEGER, PRIMARY KEY (from, to, file))`
  - `fn upsert_batch(conn: &Connection, links: &[Wikilink]) -> Result<usize>` — `INSERT OR REPLACE`
  - `fn list_for_spec(conn: &Connection, spec_name: &str) -> Result<Vec<Wikilink>>`
- [ ] `apps/rt/src/run/wikilink.rs`:
  - Subcomando `wikilink-extract --spec-dir <dir>`
  - Walk `.md` recursivo do dir; regex `\[\[([a-zA-Z0-9_\-]+)\]\]`
  - Para cada match: deriva `from` do path do arquivo (spec name = nome do dir pai ou nome do arquivo sem .md)
  - Persiste via `wikilinks::upsert_batch`
  - Stdout JSON: `{wikilinks: [...], orphans: [...]}` (orphans = `to` que não corresponde a nenhuma spec existente sob `.claude/spec/{active,completed}/`)
- [ ] `apps/rt/src/run/memory_cross_wave.rs`:
  - Subcomando `memory cross-wave --spec <s> --wave <N>`
  - Detecta nomes das waves 1..N-1 via leitura do `wave-plan.md` (tabela): lê linhas começando com `|` e extrai a coluna `Spec` (a sintaxe `[[wave-X]]`)
  - Para cada wave anterior: `SELECT payload FROM events WHERE kind = 'agent.memory' AND json_extract(payload, '$.pipeline') = ?1` ordenado por `ts DESC` LIMIT 5
  - Concatena em markdown: `## Memórias de waves anteriores\n\n### [[wave-1-...]]\n{summary}\n\n### [[wave-2-...]]\n{summary}\n`
  - Stdout: o markdown pronto (single block, pode incluir no `{cross_wave_memory}` do agent prompt)
- [ ] `apps/rt/src/run/wave_scaffold.rs`:
  - Subcomando `wave-scaffold --spec-dir <dir> --plan <json-file>`
  - `<json-file>` declara: `{waves: [{n, role, summary, depends_on: [...]}, ...], total_waves: N, lang: 'pt'|'en'}`
  - Cria `wave-plan.md` com tabela renderizada (wikilinks `[[wave-N-role]]` automáticos)
  - Para cada wave: cria `wave-N-{role}/spec.md` com skeleton (Status: queued para N>1, draft para N=1) + `Parent:` wikilink + seção `## Network` pré-preenchida
  - Cria `review/spec.md` com checklist padrão (7 categorias) + placeholder `review/verdict.md` documentado
  - Cria `qa/spec.md` com placeholder de consolidação de AC + placeholder `qa/report.md` documentado
  - Stdout: JSON `{created_files: [...], skipped: [...]}` (idempotente — não sobrescreve)
- [ ] `apps/rt/src/run/metrics_wave_status.rs`:
  - Subcomando `metrics wave-status --spec <parent>`
  - Detecta o set de waves do `<parent>` lendo wave-plan.md ou listando subdirs `wave-*-*` em `.claude/spec/active/<parent>/`
  - Para cada wave, consulta `events` filtrando `json_extract(payload, '$.pipeline') = <wave_name>`:
    - `status`: último evento `pipeline.status` para esta wave
    - `tokens_saved`: SUM(payload.saved) dos eventos `token.saved`
    - `duration_ms`: diferença entre primeiro `pipeline.status` e último de cada wave
    - `retries`: COUNT de eventos `retry.attempt`
    - `cross_wave_memory_bytes`: LENGTH do markdown que `memory cross-wave --spec <parent> --wave N` retorna (apenas N>1)
    - `model`: lido da coluna `Modelo` no wave-plan.md
  - Stdout: JSON `{parent, waves: [...]}` ordenado por número da wave
- [ ] `apps/rt/src/run/mod.rs`: registrar `wikilink_extract`, `memory_cross_wave`, `wave_scaffold`, `metrics_wave_status` no clap
- [ ] Testes mínimos:
  - `wikilink::tests::extracts_basic` — texto com 3 `[[name]]` retorna 3 links
  - `memory_cross_wave::tests::reads_prior_waves` — insere events fake, verifica markdown gerado
  - `wave_scaffold::tests::creates_full_layout` — plan JSON → 4 arquivos criados (wave-plan + wave-1 + review + qa) com conteúdo esperado
  - `metrics_wave_status::tests::aggregates_per_wave` — fixtures de events → JSON com tokens/duration/retries esperados por wave
- [ ] `cargo check -p mustard-rt && cargo test -p mustard-rt`

## Dependências

Nenhuma — primeira wave.

## Network

- Parent: [[2026-05-20-mustard-wave-network-standard]]
- Desbloqueia: [[wave-2-skill-template]] (SKILL chama `mustard-rt run memory cross-wave`), [[wave-3-dashboard-graph]] (dashboard chama `mustard-rt run wikilink-extract` via Tauri).
- Grava memória: `{subcommands_added: ['wikilink-extract','memory cross-wave'], schema: 'wikilinks (from,to,file,line)', notes: '...'}`.

## Limites

Em escopo: `apps/rt/src/run/{wikilink,memory_cross_wave,mod}.rs`, `packages/core/src/store/wikilinks.rs`.

Fora de escopo: SKILL, agent prompt, dashboard, outros subcomandos de `mustard-rt`.
