# Feature: b2-mustard-core-crate

> Spec de backlog (Parte B, item B2). ÉPICO em rascunho grosso — decompõe no ANALYZE. Depende de B1 (concluído). Revisada 2026-05-18: ganha o contrato de hook, a config de enforcement e o módulo `knowledge`.

## Contexto

A migração para Rust (B3-B5) vai portar 37 hooks, 31 scripts e a CLI. Se cada um for portado isoladamente, a lógica compartilhada — leitura/escrita do log de eventos `events.jsonl`, resolução de ambiente do hook (`_lib/hook-env.js`), emissão de métricas (`_lib/metrics-emit.js`), leitura de `pipeline-state`, mensagens de gate (`_lib/gate-message.js`), extração de conhecimento (`_lib/knowledge-extract.js`) — seria reimplementada dezenas de vezes, como já acontece hoje nos 9 `_lib/*.js` duplicados por `require`. Antes de portar qualquer hook é preciso um crate Rust de fundação que concentre esse núcleo e, principalmente, defina o contrato que hooks, scripts e CLI compartilham. O `mustard-core` é essa biblioteca; é o que torna a migração Rust enxuta em vez de caótica, e é o único lugar onde o contrato de hook e o modelo de evento existem.

## Resumo

Criar o crate Rust `packages/core` (`mustard-core`) em três camadas: (1) **`model/`** — tipos `serde` puros, sem efeito colateral: eventos do harness, o contrato de hook (`HookInput`/`Verdict`/`Outcome`) e `pipeline-state`; (2) **`io/`** — infraestrutura com efeito colateral atrás de traits (`EventSink`, `PipelineRepo`) — append-only do `events.jsonl`, r/w de `pipeline-state`, fs atômico; (3) **fundação transversal** — `config` (enforcement modes), `env` (porte de `hook-env.js`), `metrics`, `knowledge` (o que injetar entre agentes) e `error`. É a fundação consumida por B3, B4 e B5.

## Entidades

N/A — biblioteca de infraestrutura.

## Component Contract

N/A.

## Arquitetura

O crate separa **dado** de **comportamento**: `model/` não tem efeito colateral; `io/` expõe traits, nunca structs concretos — consumidores e testes injetam fakes (Dependency Inversion). O contrato de hook vive aqui, não em B3, porque é o seam compartilhado por todos os consumidores:

- `Verdict` — enum que cobre toda decisão possível: `Allow | Deny | Warn | Rewrite | Inject`. Estados ilegais irrepresentáveis.
- trait `Check` — quem pode afetar o resultado (gates/rewriters/injectors): `evaluate(&HookInput, &Ctx) -> Result<Verdict>`.
- trait `Observer` — telemetria pura, nunca bloqueia, não carrega `Verdict` (Interface Segregation).
- `EnforcementConfig` — tabela tipada que substitui os 9 `MUSTARD_*_MODE` espalhados; cada check tem `mode = off|warn|strict`.
- `knowledge` — decide o que é correto injetar entre agentes (ver Preocupações).

## Stack Rust (verificado via crates.io · maio 2026)

- **Edition 2024**, MSRV `1.85` — edition 2024 estável desde Rust 1.85 (fev/2025); stable atual é 1.95. O crate do dashboard ainda usa edition 2021/rust 1.77 — fica desalinhado até bump futuro (fora do escopo de B2).
- **`[workspace.dependencies]`** no `Cargo.toml` raiz — fonte única de versão; cada crate usa `dep = { workspace = true }`.
- **`[workspace.lints]`** — `clippy::pedantic = "warn"`, `clippy::unwrap_used = "deny"`, `unsafe_code = "forbid"`. Caminho crítico de hook não pode dar panic.
- Dependências fixadas (versões reais conferidas):
  - `serde` 1.0.228 + `serde_json` 1 — schema dos eventos e do contrato
  - `thiserror` 2.0.18 — erros tipados do crate `core` (biblioteca)
  - `anyhow` 1.0.102 — erro na borda dos binários (`rt`, `cli`)
  - `clap` 4.6 (feature `derive`) — parsing de subcomando do `rt`/`cli` (B3/B5)
  - `jiff` 0.2 — timestamps RFC-3339 dos eventos (substitui `chrono`; o dashboard pode seguir em `chrono` — não cruzam tipo, só JSON na borda)
  - `insta` 1.47 (dev-dependency) — snapshot tests; é o oráculo de paridade JS↔Rust
- API pública com `#[non_exhaustive]` em `Verdict` e `Error` — adicionar variante não quebra consumidores.
- `HookInput` é leniente (mantém `raw: Value` para campos novos do harness); tipos internos podem ser estritos.

## Arquivos

- `packages/core/Cargo.toml`, `packages/core/src/lib.rs`
- `packages/core/src/model/event.rs` — `HookEvent` + `HarnessEvent` (schema serde do `events.jsonl`)
- `packages/core/src/model/contract.rs` — `HookInput`, `Verdict`, `Outcome`, `Trigger`, traits `Check`/`Observer`
- `packages/core/src/model/pipeline.rs` — `PipelineState`, `Phase`, `Scope`
- `packages/core/src/io/event_store.rs` — trait `EventSink` + I/O append-only/replay do `events.jsonl`
- `packages/core/src/io/pipeline_repo.rs` — trait `PipelineRepo` + r/w de `.pipeline-states/*.json`
- `packages/core/src/io/fs.rs` — escrita atômica, ops fail-open
- `packages/core/src/config.rs` — `EnforcementConfig` (modes por check + `MUSTARD_DISABLED_HOOKS`)
- `packages/core/src/env.rs` — resolução de ambiente (`shouldRun`, `isSelfDelegation`, cwd/sessão)
- `packages/core/src/metrics.rs` — porte de `_lib/metrics-emit.js`
- `packages/core/src/knowledge.rs` — porte de `_lib/knowledge-extract.js` + API de seleção do contexto a injetar entre agentes
- `packages/core/src/error.rs` — `Error` (`thiserror`) + helpers fail-open
- `Cargo.toml` raiz — registrar `packages/core` no workspace; `[workspace.dependencies]` e `[workspace.lints]`

## Limites

- `packages/core/`, `Cargo.toml` raiz
- **Fora dos limites:** os hooks/scripts/CLI em si (consomem o crate em B3-B5); o JS atual permanece intocado até ser portado.

## Tarefas

### Core Agent (Wave 1) — modelo: evento + contrato

- [x] Definir `model/event.rs` a partir de `events.jsonl` real e `_lib/harness-event.js`.
- [x] Definir `model/contract.rs`: `HookInput`, `Verdict`, `Outcome`, `Trigger`, traits `Check`/`Observer`. É o contrato que B3 consome — congelar a API ao fim desta wave.
- [x] Definir `model/pipeline.rs`.

### Core Agent (Wave 2) — io: traits + I/O

- [x] `io/event_store.rs`: trait `EventSink` + append-only/replay do `events.jsonl`.
- [x] `io/pipeline_repo.rs`: trait `PipelineRepo` + r/w de `pipeline-state`.
- [x] `io/fs.rs`: escrita atômica fail-open.

### Core Agent (Wave 3) — fundação transversal

- [x] `config.rs`: `EnforcementConfig` — modos por check, lista de desabilitados; carrega de `mustard.json`/env.
- [x] `env.rs`: porte de `hook-env.js`.
- [x] `metrics.rs`: porte de `metrics-emit.js`.
- [x] `knowledge.rs`: porte de `knowledge-extract.js` + API de seleção de contexto entre agentes.
- [x] `error.rs` + Cargo workspace (`[workspace.dependencies]`, `[workspace.lints]` com `clippy::unwrap_used`).

### Core Agent (Wave 4) — paridade

- [x] Testes `cargo test` cobrindo paridade com o comportamento JS; `hooks/__tests__/` é o oráculo.

## Dependências

- B1 (monorepo) — concluído.
- Pré-requisito de B3, B4 e B5.

## Preocupações

- **Paridade comportamental:** o crate precisa reproduzir exatamente o comportamento dos `_lib/*.js`. Os testes JS existentes (`hooks/__tests__/`) são a referência.
- **Fail-open:** APIs de I/O nunca causam panic — o padrão fail-open dos hooks depende disso. `clippy::unwrap_used` é `deny` no workspace (exceto em módulos de teste).
- **Estabilidade do contrato:** `model/contract.rs` é congelado ao fim da Wave 1 — B3 e B4 dependem dele; mudança tardia propaga para todos os módulos.
- **Injeção entre agentes (`knowledge`):** hoje a passagem de conhecimento entre agentes é heurística (`knowledge.json`, `memory-write.js`). O módulo `knowledge` deve expor uma API explícita — dado o agente destino e a fase, retornar só o contexto relevante, não o dump inteiro. É o que economiza token no pipeline. O critério de "relevante" é decisão de design do ANALYZE e não pode ser hardcode de tecnologia.

## Critérios de Aceitação

- [x] AC-1: O crate existe e está no workspace — Command: `node -e "const fs=require('fs');if(!fs.existsSync('packages/core/Cargo.toml'))process.exit(1)"`
- [x] AC-2: O crate compila e os testes passam — Command: `bash -c 'cargo test -p mustard-core'`
- [x] AC-3: O contrato está exportado — Command: `bash -c 'grep -rl "pub enum Verdict" packages/core/src'`

## Não-Objetivos

- Não portar nenhum hook/script aqui — só a biblioteca compartilhada e o contrato.
- Não remover os `_lib/*.js` ainda — saem quando o último consumidor JS for portado (fim de B3/B4).
- Não decidir a heurística de relevância do `knowledge` — só expor a API; a política fica no ANALYZE.
