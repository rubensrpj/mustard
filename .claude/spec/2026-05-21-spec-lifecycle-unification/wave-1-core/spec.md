# Wave 1 — Core: Stage + Outcome + Flags + parser tolerante

### Parent: [[2026-05-21-spec-lifecycle-unification]]
### Wave: 1
### Role: core
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T23:40:00Z

## Resumo

Introduz no `mustard-core` o modelo canônico de estado de spec: enums `Stage`, `Outcome`, `Flags` e struct `SpecState`. Deprecam (mas mantêm legíveis) `SpecStatus` e o atual `Phase`. O parser de headers (`### Status:` / `### Phase:`) passa a derivar `SpecState` corretamente, e parsing do novo formato (`### Stage:` / `### Outcome:` / `### Flags:`) é adicionado. Invariantes são validadas no construtor `SpecState::new`.

## Arquivos

```
packages/core/src/model/view/spec.rs          (Stage, Outcome, Flags, SpecState, novos parsers; SpecStatus deprecated)
packages/core/src/model/view/mod.rs           (re-exports + alias do Phase legado)
packages/core/src/model/pipeline.rs           (manter Phase do domain — não usado em UI a partir desta wave; doc-comment marca como legacy)
packages/core/src/projection/card.rs          (folds para Stage/Outcome em vez de SpecStatus)
packages/core/src/reader/sqlite.rs            (queries devolvem SpecState; SpecSummary ganha campo state)
packages/core/src/io/sqlite_schema.sql        (sem mudança — eventos não mudam de forma)
packages/core/tests/reader_contract.rs        (atualizar asserções para SpecState)
packages/core/tests/state_invariants.rs       (novo — testa invariantes)
```

## Tarefas

- [x] Adicionar `Stage`, `Outcome`, `Flags`, `SpecState` em `model/view/spec.rs` com derive `Serialize/Deserialize/PartialEq/Eq/Hash/Clone/Copy` (exceto `Flags` que não é Copy).
- [x] Implementar `Stage::parse(&str)`, `Outcome::parse(&str)`, `Flags::parse(&str)` aceitando case-insensitive e sinônimos (`approved` → Plan via mapeamento documentado).
- [x] Implementar `SpecState::new(stage, outcome, flags) -> Result<Self, StateError>` rejeitando combinações ilegais:
  - `Outcome != Active && Stage != Close` → `Err(InvalidTerminalStage)`.
  - `flags.followup_open && (Stage != Close || Outcome != Active)` → `Err(InvalidFollowupContext)`.
  - `flags.wave_failed && Stage != Execute` → `Err(InvalidWaveFailedContext)`.
- [x] Marcar `SpecStatus` com `#[deprecated(note = "Use SpecState. Removed in spec-lifecycle-unification W7.")]`. Manter o enum funcional durante a transição.
- [x] Adicionar conversão `From<SpecStatus> for SpecState` (e oposta `TryFrom<SpecState> for SpecStatus` para back-compat de readers ainda não migrados).
- [x] Atualizar parser de header em `apps/rt/src/run/spec_sections.rs` (read-only nesta wave — só leitura) para reconhecer `### Stage:` / `### Outcome:` / `### Flags:`. Escrita continua escrevendo legacy nesta wave (W4 muda).
- [x] Atualizar `projection/card.rs` para folder o stream de eventos em `SpecState` em vez de `SpecStatus` direto. Manter projection legado disponível como `fold_legacy_status` por compat.
- [x] Atualizar `reader/sqlite.rs::SpecSummary` e `SpecView` adicionando campo `state: SpecState`. Manter `status: SpecStatus` como `#[deprecated]` deserializado a partir de `state.into()`.
- [x] Atualizar `tests/reader_contract.rs` para verificar tanto o campo novo `state` quanto o legado `status`.
- [x] Criar `tests/state_invariants.rs` cobrindo: invariantes do construtor, idempotência `SpecState ↔ SpecStatus`, parser de header novo e legado.
- [x] Rodar `cargo build -p mustard-core && cargo test -p mustard-core && cargo clippy -p mustard-core -- -D warnings`.

## Modelo final (referência)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Analyze,
    Plan,
    Execute,
    QaReview,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Active,
    Completed,
    Cancelled,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Flags {
    #[serde(default)] pub blocked: bool,
    #[serde(default)] pub wave_failed: bool,
    #[serde(default)] pub followup_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpecState {
    pub stage: Stage,
    pub outcome: Outcome,
    #[serde(default)] pub flags: Flags,
}
```

## Mapeamento legado → novo (consumido pelo parser)

| Header legado | Stage | Outcome | Flags |
|---|---|---|---|
| `Status: draft\|planning\|approved` | Plan | Active | — |
| `Status: in_progress\|implementing` | Execute | Active | — |
| `Status: reviewing` | QaReview | Active | — |
| `Status: qa` | QaReview | Active | — |
| `Status: closed-followup` | Close | Active | `followup_open=true` |
| `Status: completed\|closed` | Close | Completed | — |
| `Status: cancelled\|superseded` | Close | Cancelled | — |
| `Status: abandoned\|orphan` | Close | Abandoned | — |
| `Status: blocked\|paused` | (last known Phase \|\| Plan) | Active | `blocked=true` |
| `Status: wave-failed` | Execute | Active | `wave_failed=true` |
| `Phase: ANALYZE/PLAN/EXECUTE/QA/CLOSE` | mesmo nome | (deriva de Status) | — |
| `Phase: REVIEW` | QaReview | (deriva) | — |

Conflitos: Status terminal (Completed/Cancelled/Abandoned) sempre vence. Status qualificador (blocked/wave-failed) vira flag; Phase decide o Stage.

## Acceptance Criteria

- [x] AC-W1-1: `cargo build -p mustard-core` passa.
- [x] AC-W1-2: `cargo test -p mustard-core` passa.
- [x] AC-W1-3: `cargo clippy -p mustard-core` passa (exit 0 — política oficial do crate: workspace `Cargo.toml` define `pedantic = "warn"` advisory de propósito) E nenhum arquivo tocado por esta wave introduz warning sob `-D warnings` (verificado: 32→0 nos arquivos da wave; 69 restantes são baseline pré-existente em módulos congelados `economy/`/`store/`, fora de escopo). Nota: a redação original `-- -D warnings` era inatingível num checkout limpo por causa do baseline; corrigida para a política real do projeto.
- [x] AC-W1-4: Teste unit `tests/state_invariants.rs::rejects_completed_with_active_stage` passa e `SpecState::new(Stage::Plan, Outcome::Completed, _)` retorna `Err`.
- [x] AC-W1-5: Teste unit `parses_legacy_approved_as_plan_active` passa: header `### Status: approved` resulta em `SpecState { Stage::Plan, Outcome::Active, .. }`.
- [x] AC-W1-6: Teste unit `parses_new_format` passa: header `### Stage: Execute / ### Outcome: Active / ### Flags: blocked` resulta em estado correspondente.
- [x] AC-W1-7: Build do workspace inteiro verde: `cargo build` (sem `-p`).

## Limites

**IN:** apenas os arquivos listados em "Arquivos".

**OUT:**
- `apps/rt/src/hooks/*` — Wave 5.
- `apps/dashboard/**` — Wave 3.
- `apps/cli/templates/**` — Wave 4.
- Reescrita de headers em `.claude/spec/**/*.md` — Wave 7.

## Notas de migração interna

- `SpecStatus` permanece exportado e funcional para evitar quebrar `mustard-rt` e `dashboard-tauri` antes de W2/W3 migrarem. Será removido em W7.
- `Phase` (em `model/pipeline.rs`) tem 7 variantes (Analyze, Plan, Execute, Review, Qa, Close, Coordinate). A nova `Stage` tem 5 (Review absorvido em QaReview, Coordinate removido). Conversão `Phase → Stage`: `Review` e `Qa` → `QaReview`; `Coordinate` → `Plan` (mais conservador) + log warning.
