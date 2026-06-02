# Full scope sempre decompõe em ≥1 wave (pai-orquestrador + wave-subagente)

<!-- drafter:tone=didactic -->

<!-- PRD -->

## Contexto

Decisão do usuário (2026-06-02): **todo trabalho independente (escopo Full) tem ao menos uma wave**. A spec-pai vira documento de **orquestração** (sem `## Tarefas`/`## Checklist` próprios); a wave é o **subagente** que executa. Não existe "Full não-decomposto". Light continua sendo spec única, com checklist próprio, executada inline.

A motivação saiu ao fechar [[2026-06-02-spec-scaffold-lifecycle-gate]]: aquela spec era `scope=full` com `total_waves=0`/`isWavePlan=false` — um estado-limbo onde `FullScopeNoWaves` (`packages/core/src/domain/spec/contract.rs:280`) a consideraria inválida, mas ela existia e foi executada por despacho-por-subprojeto sem wave-plan. Esta spec elimina esse limbo no **código/templates do tool** (não em nota de memória nem na spec gerada).

Âncoras (do scan + exploração desta sessão):

- `apps/rt/src/commands/spec/spec_draft.rs:246` — `total_waves: if matches!(scope, Scope::Full) { Some(waves.max(1)) } else { None }`. **Já garante ≥1 para Full** — não é a origem do limbo; serve de invariante a blindar com teste.
- `apps/rt/src/commands/spec/spec_draft.rs:198-202` — comentário: a decomposição Full é "owned by wave-scaffold; meta já registra scope=full + totalWaves + isWavePlan, então um wave-plan é *esperado* antes do wave-scaffold preenchê-lo".
- `apps/rt/src/commands/spec/scope_decompose.rs` — `decide` retorna `decompose: true|false` por sinais (multi-layer `layer_count>=2`, roadmap, history, wide+new `file>10 && new>=2`; senão `single-layer → false`). Decide **N-vs-1 wave**, mas o caller hoje trata `false` como **zero waves**. Esse é o ponto a reinterpretar para Full: `false` = **uma** wave, não zero.
- `apps/rt/src/commands/wave/wave_scaffold.rs` — cria `wave-plan.md` + `wave-N-{role}/` a partir de um `plan.json`. Precisa suportar/garantir o caso de **uma** wave para Full.
- `packages/core/src/domain/spec/contract.rs:280` — `FullScopeNoWaves` (Full exige `total_waves>=1`). **Fica como invariante correto**; idealmente passa a ser exercido no ponto de persistência/aprovação para virar enforcement real (hoje `validate` não roda nesse caminho).
- `.claude/refs/spec/approve-only-flow.md:40` (+ cópia em `apps/cli/templates/.../refs/spec/approve-only-flow.md`) — branch **"Reject decomposition — use single spec"**: hoje faz merge das waves numa spec única e seta `isWavePlan:false`. **Origem do limbo.** Para Full, deve colapsar para **uma** wave (pai-orquestrador + 1 wave), nunca zero.
- `apps/cli/templates/commands/mustard/feature/SKILL.md` §3 — regra de decomposição em prosa: "wave decomposition when file_count ≥ 6 OR layer_count ≥ 3 OR independent_subbehaviors ≥ 3". **Diverge do código** (`scope_decompose` usa `layer>=2` ou `file>10 && new>=2`). Reconciliar e afirmar "Full sempre ≥1 wave; `scope_decompose` decide 1-vs-N, não 0-vs-≥1".

Por que agora: o limbo gera specs Full inconsistentes (meta diz uma coisa, FS outra), confunde o close-gate/dashboard e contradiz o modelo mental pai=orquestrador/wave=subagente.

## Usuários/Stakeholders

Operadores do pipeline `/feature` + `/spec` (quem planeja e executa). Eles ganham um modelo previsível: Full ⇒ sempre pai-orquestrador + ≥1 wave; Light ⇒ spec única inline. Some o estado ambíguo "Full sem wave".

## Métrica de sucesso

- Nenhum caminho do tool produz um Full persistido/aprovado com `total_waves=0` ou sem wave-plan.
- A opção "rejeitar decomposição" em Full passa a significar "uma wave" (não "sem wave").
- `FullScopeNoWaves` deixa de ser uma tensão e passa a ser enforcement exercido de fato.
- Prosa (`feature/SKILL.md`, `approve-only-flow.md`) consistente com o código.

## Não-Objetivos

- NÃO mudar o comportamento de Light (segue spec única + checklist inline).
- NÃO mudar a heurística multi-wave de `scope_decompose::decide` além de reconciliar a prosa e garantir o piso de 1 wave para Full.
- NÃO tocar o dashboard.
- NÃO reescrever specs já geradas em projetos clientes (correção é no tool).

## Critérios de Aceitação

- [ ] AC-1: contract preserva FullScopeNoWaves (Full exige total_waves>=1) — Command: `cargo test -p mustard-core -- contract`
- [ ] AC-2: spec-draft mantém total_waves>=1 para Full, com teste de lock — Command: `cargo test -p mustard-rt -- spec_draft`
- [ ] AC-3: existe enforcement em código que impede um Full chegar a Execute/Close sem >=1 wave (single-wave quando nao multi) — Command: `cargo test -p mustard-rt -- full_requires_wave`
- [ ] AC-4: o approve-flow nao oferece mais "single spec / zero waves" para Full (colapsa para single wave) — Command: `rg -n "single wave|uma wave|colapsa" .claude/refs/spec/approve-only-flow.md`
- [ ] AC-5: feature SKILL afirma "Full sempre >=1 wave" e a heuristica casa com scope_decompose — Command: `rg -n "sempre|>= ?1 wave|1-vs-N" apps/cli/templates/commands/mustard/feature/SKILL.md`
- [ ] AC-6: workspace compila, testa e linta verde — Command: `cargo test && cargo clippy --all-targets`

<!-- PLAN -->

## Entidades

Modelo de escopo/decomposição do pipeline: `Scope` (Light/Full), `total_waves`, `isWavePlan`, e a decisão `scope_decompose` (N-vs-1). Nenhuma entidade de domínio nova — é mudança de invariante/política no tool.

## Arquivos

**Wave 1 — enforcement em código (apps/rt + packages/core):**

- `apps/rt/src/commands/spec/scope_decompose.rs` — deixar explícito que `decide` escolhe **1 vs N** waves; o resultado `decompose:false` para Full significa **uma** wave. Ajustar o caller (ver abaixo) ou expor um helper `floor_one_wave_for_full`.
- `apps/rt/src/commands/spec/spec_draft.rs` — blindar o invariante `total_waves>=1` para Full (`:246`) com teste; garantir que um Full nunca seja escrito com `isWavePlan:false`/`total_waves:0`.
- `apps/rt/src/commands/wave/wave_scaffold.rs` — suportar o caso de **uma** wave (pai-orquestrador + `wave-1-{role}`), idempotente.
- `apps/rt/src/commands/pipeline/resume_bootstrap/post_execute_gate.rs` (ou ponto de aprovação) — **enforcement**: um Full sem wave-plan/`total_waves>=1` não inicia Execute; ou auto-scaffolda 1 wave, ou bloqueia com mensagem. Exercer `contract::validate`/`FullScopeNoWaves` aqui (hoje não roda nesse caminho).
- `packages/core/src/domain/spec/contract.rs` — manter `FullScopeNoWaves`; doc-comment afirmando que é o invariante "Full ⇒ ≥1 wave".

**Wave 2 — prosa (apps/cli templates + .claude refs):**

- `.claude/refs/spec/approve-only-flow.md` (`:40`) **e** `apps/cli/templates/refs/spec/approve-only-flow.md` — branch "Reject decomposition": para Full, colapsa para **uma** wave (pai-orquestrador + 1 wave), nunca `isWavePlan:false`/zero waves. Manter a opção "single spec" só para Light.
- `apps/cli/templates/commands/mustard/feature/SKILL.md` §3 — afirmar "Full sempre ≥1 wave; `scope_decompose` decide 1-vs-N"; reconciliar os números da heurística com o código (`layer>=2` etc.) ou remover os números divergentes e referenciar `scope-decompose`.

## Tarefas

- T1 — Wave 1: reinterpretar `decompose:false` como "1 wave" para Full no caller; helper em `scope_decompose`; blindar `spec_draft` (`total_waves>=1`) com teste.
- T2 — Wave 1: enforcement no ponto de aprovação/persistência (`post_execute_gate` ou equivalente) — Full sem ≥1 wave não avança; exercer `FullScopeNoWaves`. Teste `full_requires_wave`.
- T3 — Wave 1: `wave_scaffold` cobre o caso de uma wave; doc em `contract.rs`.
- T4 — Wave 2: ajustar `approve-only-flow.md` (ref + template) — reject-decomposition vira single-wave para Full.
- T5 — Wave 2: ajustar `feature/SKILL.md` — afirmar a regra e reconciliar a heurística.

## Dependências

Wave 2 (prosa) depende do contrato de Wave 1 estar definido (nome do enforcement, semântica de "single wave"). Wave 1 não depende de Wave 2.

## Limites

IN: `apps/rt` (scope_decompose, spec_draft, wave_scaffold, ponto de aprovação), `packages/core` (contract doc), `apps/cli/templates` (SKILL + ref), `.claude/refs`. Testes que assertam o invariante.

OUT: comportamento de Light; heurística multi-wave em si; dashboard; specs já geradas.

<!-- wikilinks-footer-start -->
- [2026-06-02-spec-scaffold-lifecycle-gate](?) ⚠ unresolved
<!-- wikilinks-footer-end -->