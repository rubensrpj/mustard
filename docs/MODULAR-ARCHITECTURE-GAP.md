# Arquitetura Modular — Análise de Lacunas + Plano Faseado

> Confronto do mustard com os "10 princípios de arquitetura modular" e o curso
> *Desenvolvendo Aplicações Enterprise Modernas* (DDD, hexagonal, fitness
> functions, governança automatizada, comunicação explícita entre módulos).
> Gerado em 2026-05-29 a partir de leitura direta do código.

## Enquadramento

Os 10 princípios e o curso descrevem propriedades de **aplicações em produção**
(runtime). O mustard é a **fábrica dev-time** que constrói essas aplicações — não
é uma delas. Logo a pergunta "o que tem / o que falta" tem dois lados:

- **Lado A — o próprio mustard segue os princípios?** (dívida interna / manutenibilidade)
- **Lado B — o mustard obriga os projetos-alvo a seguirem?** (capacidade de produto)

Princípios de runtime puro (deploy independente, escala individual, independência
de falha em produção) estão **fora do escopo** de uma ferramenta dev-time.

---

## Lado A — O mustard como software (auto-auditoria)

| Princípio | Status | Evidência / ressalva |
|---|---|---|
| 1. Fronteiras | ✅ Bom | `core / cli / rt / dashboard` separados. **Ressalva:** `core` re-exporta `ClaudePaths` — acopla o kernel "agnóstico" ao layout Claude (`packages/core/src/lib.rs:109`). |
| 2. Composibilidade | ✅ Razoável | `cli` e `rt` compõem sobre `core`; dashboard sobre ambos. |
| 3. Independência | ✅ Forte | `core` tem zero deps internas; `cli ⊥ rt`. |
| 4. Isolamento de estado | ⚠️ Parcial | Donos únicos de parser em `core::domain` (config/meta/events). **Mas** `events.jsonl` e `mustard.json` são tocados por `rt` (subprocess) **e** dashboard (crate) sem lock → corrida de escrita. SQLite `mustard.db` é só do dashboard (isolado ✅). |
| 5. Comunicação explícita | ⚠️ Inconsistente | dashboard→`rt` = subprocess (contrato CLI ✅); dashboard→`cli` = crate linkado em compilação (acoplamento forte). Dois estilos p/ o mesmo backend. |
| 6. Substituibilidade | ⚠️ Mista | `rt` é substituível (processo); `cli`/`core` não (linkados na build do dashboard). |
| 7. Deploy independente | ⚠️ Parcial | `rt` via `cargo install` (PATH); mudar `cli` exige rebuild do dashboard. |
| 8. Escala individual | ➖ N/A | CLI/desktop; modelo subprocess do `rt` permite paralelizar workers. |
| 9. Observabilidade | ✅ Forte | `events.jsonl` + projeções SQLite + otel + economy. |
| 10. Independência de falha | ✅ Boa | Hooks degradam em bug próprio; subprocess isola crash do `rt`. **Ponto:** bug em `core` derruba cli+rt+dashboard (raio total da dep compartilhada). |
| **Fitness functions internas** | ❌ Não existe | Há `unsafe_code=forbid` / `unwrap_used=deny` (qualidade), mas **nenhum teste de arquitetura** que reprove import proibido entre crates. Sem `cargo-deny`. |

---

## Lado B — O mustard como ferramenta de governança (garante nos alvos?)

| Princípio / prática | Já tem | Falta |
|---|---|---|
| 1. Fronteiras | `path_gate` + `entity_registry_gate` + `CLAUDE.md` por subprojeto (quem escreve onde). | Fronteira por **regra de import no código** (módulo A não pode importar B). |
| 5. Comunicação explícita | `## Component Contract` no spec (documento); `graph_wirelink_gate`. | Enforcement do *Public API Provider pattern* — nada barra acesso interno direto entre módulos. |
| 9. Observabilidade | Forte, mas **do pipeline** (events/otel/economy). | Observabilidade do alvo em runtime (fora de escopo). |
| Governança automatizada (curso Mód. 6) | `close_gate` (build/type/lint/test+QA), `bash_command_gate`, `model_routing_gate`, `gate_regression_check`. | Gates rodam em **PreToolUse** (quando a IA edita) — não como pre-commit/CI que rode também p/ humanos. |
| Fitness functions (curso Mód. 9) | `architecture.rs` **detecta** o estilo (clean/hexagonal/ddd/layered). | **Detecta, não valida.** Sem motor que reprove violação de camada / ciclo / acoplamento. |
| Isolamento de estado, substituibilidade, deploy, escala | — | Não verificados no alvo (a maioria é runtime). |

**Resumo:** governança de **processo** madura; governança de **arquitetura do
produto** rasa. Mas a infraestrutura para fechar a lacuna já existe (grafo de
entidades, vocabulário de camadas, motor de detecção, padrão de gates).

---

## O achado que une os dois lados

O **mesmo gap** aparece nas duas faces: **fitness functions de dependência**.
O mustard não as oferece aos alvos (só detecta) **nem** as aplica em si mesmo
(nada impede `core` de importar `cli`). E o motor de validação **já está escrito
e testado** (ver abaixo) — falta *reprovar*, não *construir*.

---

## Plano faseado

**Decisões tomadas:** (1) ClaudePaths — **refatorar de vez agora**; (2) gate de
arquitetura — **modo `warn` por padrão**.

### Decisão de arquitetura — "tirar ClaudePaths do core"

Dentro de `packages/core/src` só **3 módulos** chamam `ClaudePaths`
(`domain/config.rs`, `domain/entity_registry.rs`, `view/projection/mod.rs`); os
~440 usos restantes vivem em `apps/rt`/`apps/dashboard` — **adaptadores Claude
legítimos**. Portanto: **mover a definição** de `ClaudePaths`/`SpecPaths`/`WavePaths`
para um crate novo `packages/claude-paths`, desacoplar os 3 consumidores do `core`,
e trocar o caminho do `import` nos ~111 arquivos dos adaptadores (mecânico).
**Não** introduzir trait + dynamic-dispatch nos 533 sites (indireção sem ganho).
Crate novo (não mover p/ `apps/rt`) preserva a independência `cli ⊥ rt`.

### Fase A — Arrumar a casa

**A1. Fitness function de dependência entre crates** *(baixo)*
Novo `packages/core/tests/crate_boundaries.rs` (convenção: ver
`packages/core/tests/state_invariants.rs`). Regra declarativa de arestas
permitidas; falha se `core`→`cli`/`rt`/`dashboard`, `cli`↔`rt` se cruzam, ou
alguém importa `dashboard`. Opcional CI: `deny.toml` + `cargo-deny`.

**A2. Tirar `ClaudePaths` do `core`** *(alto — épico)*
- Criar `packages/claude-paths`; mover `packages/core/src/io/claude_paths.rs`
  (e o `doctor --check claude-paths`).
- `core`: remover `pub use` (`lib.rs:109`) + `pub mod claude_paths` (`io/mod.rs:3`);
  desacoplar: `config.rs:249` → `root.join("mustard.json")` direto;
  `entity_registry.rs` (`:58`,`:253`) e `projection/mod.rs:84` → receber o path
  do artefato **por parâmetro** do adaptador.
- Adaptadores: dep em `claude-paths`; `use mustard_core::ClaudePaths` →
  `use claude_paths::ClaudePaths` nos ~111 arquivos (validar com `cargo check`).
- A1 passa a guardar `core ⊥ claude-paths`.

**A3. Serializar escrita de estado** *(médio)*
- `apps/rt/src/shared/events/writer_ndjson.rs` (`write_event_inner:205`):
  serializar append por arquivo de spec (eliminar corrida entre hooks paralelos).
- `mustard.json`: escrita atômica `tmp + rename` em `apps/cli/src/commands/init.rs:333`
  e `update.rs:129`.

### Fase B — Governar arquitetura dos alvos (modo `warn`)

Motor **já existe e testado** em `packages/core/src/domain/vocabulary/architecture.rs`:
`LayerRole`, `LayerEdge`, `inward_rank()`, e `detect_architecture()` que já calcula
`solid_adherence=Some(false)` quando camada central importa externa (teste
`solid_violation_when_domain_imports_infrastructure`). Extração de imports existe:
`import_edges.scm` p/ 7 linguagens (`packages/core/src/domain/ast/queries.rs:59`).

**B1. Preservar o role do import — conserto central** *(médio)*
`apps/rt/src/commands/scan/structural_extract.rs:153-189`: `ref_tail` reduz
`crate::infrastructure::PgRepo` → `PgRepo`, **descartando a camada**. Por isso as
`LayerEdge` quase nunca se formam (testes usam refs artificiais como `&["domain"]`).
Conserto: classificar o role de cada segmento do import path no momento da extração
e preservar no `EntityInfo`.

**B2. Montar `LayerEdge` reais + persistir violações** *(baixo — liga os fios)*
`apps/rt/src/commands/scan/architecture.rs:218-233`: alimentar `LayerEdge` com os
roles de B1; persistir a lista de violações no `entity-registry`/`patternsOverlay`.

**B3. Gate de arquitetura no REVIEW** *(médio)*
Novo check (ex.: `apps/rt/src/commands/review/architecture_fitness.rs`) que computa
violações novas do diff e emite evento (ex.: `review.architecture`), seguindo o
padrão de `review_dispatch.rs`. Controle `MUSTARD_ARCH_GATE_MODE = warn (default)
| strict | off`. Em `warn`: surface no banner, não bloqueia. Regras via
`.claude/vocab/architecture.toml` (override já suportado).

### Sequência
`A1 → A2 → A3 → B1 → B2 → B3`. A1 protege A2; a Fase A inteira precede B
("pratique o que prega" antes de impor aos alvos).

### Verificação
- **A1:** `cargo test -p mustard-core --test crate_boundaries`; injetar dep
  proibida temporária e ver falhar.
- **A2:** `cargo build/test --workspace` verdes; `Grep ClaudePaths|claude_paths`
  em `packages/core/src` → zero; A1 verde com `core ⊥ claude-paths`.
- **A3:** teste de concorrência (N threads em `write_event`) sem linha
  intercalada; `mustard.json` válido sob escrita interrompida.
- **B:** fixtures *clean* (infra→domain) e *dirty* (domain→infra). `/scan` no dirty
  marca violação no registry; `/review` num diff com violação mostra banner `warn`
  (e bloqueia só com `MUSTARD_ARCH_GATE_MODE=strict`). Unitários p/ B1 (role
  preservado) e B2 (`LayerEdge` a partir de import real).
