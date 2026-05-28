# Scan via AST — segundo consumer real do `mustard_core::ast`

### Stage: Plan
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-28T00:00:00.000Z

## Contexto

Spec A v4 W1.5 entregou `mustard_core::ast::{GrammarLoader, extract_function_signatures}` — agnóstico via `tree_sitter::Loader`, com fallback regex (`extract_via_fallback_regex`) quando grammar local ausente. Hoje **1 único consumer** chama essa API: `gate_regression_check` (W4). Outras duas peças que precisam "extrair declarações de função" mantêm caminhos próprios paralelos:

1. **`apps/rt/src/run/scan/entity_extractor.rs::extract_decls`** — string matching literal de prefixos (`"pub fn"`, `"export class"`, `"def"`, etc.). É o que `/scan` consome para popular `entity-registry.json` e o "Real examples" das SKILLs geradas.
2. **`apps/rt/src/run/sync_registry.rs`** — heurística própria que escaneia Drizzle/EF/Prisma sem AST.

Isso viola três coisas:

- `feedback_rust_solid_reuse_global` — **"sem trait sem ≥2 impls reais"**. Hoje a API agnóstica tem 1 impl real e 2 caminhos paralelos competindo. Adicionar `/scan` como segundo consumer **valida** a abstração.
- `feedback_mustard_agnostic` — o `entity_extractor` ainda lista prefixos por linguagem em tabela hardcoded. AST via Loader é a forma agnóstica canônica.
- **Precisão** — string matching erra overload (Rust generics + lifetimes), decorators (Python `@dataclass`), exports compostos (TS `export default class`). AST captura tudo o que a grammar reconhece.

Esta sub-spec migra `/scan` e `sync-registry` pra consumir `mustard_core::ast::extract_function_signatures`. Comportamento preservado quando grammar ausente (cai pro fallback regex agnóstico do mesmo módulo — mesma garantia que `gate_regression_check`).

Dependências satisfeitas pela Spec A v4 (closed 2026-05-28):
- W1.5: `mustard_core::ast::{GrammarLoader, extract_function_signatures, signature::extract_via_fallback_regex}` — API pública, agnóstica, com fallback explícito.
- W7#1: `stub_detect` agora torna `lang_id` opcional para hosts sem grammar — mesmo padrão aplicável aqui.

## Usuários/Stakeholders

Maintainer único (Rubens). Indireto: TODO usuário rodando `/mustard:scan` ou `mustard-rt run sync-registry` num projeto-alvo — passa a receber `entity-registry.json` baseado em AST quando a grammar está instalada, com a precisão correspondente; sem regressão em hosts sem grammar (cai pro fallback regex).

## Métrica de sucesso

- **`/scan` no próprio Mustard**: `entity-registry.json` resultado tem **0 entries duplicadas ou faltantes** versus a verdade-do-AST (validado contra um snapshot determinístico de `cargo check --message-format=json` em hosts COM grammar).
- **`sync-registry` no projeto-alvo Sialia (.NET + React)**: detecta `DbSet<Entity>` declarations + `pgTable` exports COM a precisão que regex perde (overload, generics aninhados).
- **Zero regressão em hosts SEM grammar**: o output continua não-vazio (fallback regex agnóstico), apenas perde algumas features sintáticas (overload é colapsado).
- **Dedup**: `apps/rt/src/run/scan/entity_extractor.rs` se reduz a um shim sobre `mustard_core::ast` (≤30 LOC) ou some inteiro se o caller migrar direto.

## Não-Objetivos

- **Context7 / docs upstream** — diferido para Spec C (Fase C). Esta sub-spec foca apenas em assinaturas LOCAIS do projeto-alvo via tree-sitter.
- **Refactor do `cluster_discovery.rs`** — ele opera sobre file-tree e file-content, não sobre assinaturas; fora de escopo.
- **Quebra de compatibilidade do `entity-registry.json` v4.0** — schema preservado; só a EXTRAÇÃO muda. Versão bumpa pra v4.1 quando precisar.
- **Migrar specs antigas** — sem usuários em prod.

## Critérios de Aceitação

- [ ] AC-S-1: `mustard_core::ast::extract_function_signatures` é chamado em `entity_extractor::extract_decls` quando o loader tem grammar para a extensão do arquivo; output `Vec<ExtractedDecl>` mantém shape atual.
  Command: `cargo test -p mustard-rt --lib run::scan::entity_extractor::tests::extract_decls_uses_ast_when_grammar_present`
  Função: `mustard_rt::run::scan::entity_extractor::extract_decls`

- [ ] AC-S-2: Quando o loader NÃO tem grammar para a extensão, `extract_decls` cai no `mustard_core::ast::signature::extract_via_fallback_regex` (não no string matching anterior). Output ainda não-vazio sobre fixture Rust sem grammar instalada localmente.
  Command: `cargo test -p mustard-rt --lib run::scan::entity_extractor::tests::extract_decls_fallback_when_grammar_absent`
  Função: `mustard_rt::run::scan::entity_extractor::extract_decls`

- [ ] AC-S-3: `sync_registry` consome a mesma API e produz `entity-registry.json` cujo `_patterns` reflete os mesmos kinds que `entity_extractor` retorna (sem divergência entre os dois consumers).
  Command: `cargo test -p mustard-rt --lib run::sync_registry::tests::sync_registry_kinds_match_entity_extractor`
  Função: `mustard_rt::run::sync_registry::run`

- [ ] AC-S-4: Tabela literal de prefixos (`"pub fn"`, `"export class"`, `"def"`, ...) em `entity_extractor.rs` é **removida** ou reduzida ao mapeamento `kind → display label` (sem regex/string matching de fonte). `grep -E '"pub fn"|"export class"|"def"' apps/rt/src/run/scan/entity_extractor.rs` retorna apenas linhas em comentário/doc-string ou em `#[test]`.
  Command: `cargo test -p mustard-rt --lib run::scan::entity_extractor::tests::no_literal_lang_prefixes_in_production_path`
  Função: `mustard_rt::run::scan::entity_extractor::extract_decls`

- [ ] AC-S-5: `/mustard:scan` rodado contra o próprio Mustard (`cargo run -p mustard-rt -- run scan-orchestrate`) completa sem erro e gera `entity-registry.json` com ≥1 entity Rust por crate (`mustard-cli`, `mustard-rt`, `mustard-core`, `mustard-dashboard` quando aplicável).
  Command: `bash -c 'cd /c/Atiz/mustard && cargo run -p mustard-rt -- run scan-orchestrate >/dev/null 2>&1 && test -s .claude/entity-registry.json'`
  Função: `mustard_rt::run::scan_orchestrate::run`

## Arquivos

- `apps/rt/src/run/scan/entity_extractor.rs` (REESCRITO — passa a delegar ao `mustard_core::ast`)
- `apps/rt/src/run/sync_registry.rs` (ESTENDIDO — consome mesma API)
- `packages/core/src/ast/signature.rs` (potencial: expor helper `extract_decls` se a granularidade pedir — opcional)
- `apps/rt/src/run/scan/mod.rs` (limpeza de imports após dedup)

## Funções tocadas

- `mustard_rt::run::scan::entity_extractor::extract_decls` — REESCRITO
- `mustard_rt::run::sync_registry::run` — MODIFICADO
- `mustard_core::ast::extract_function_signatures` — CONSUMIDO (zero mudança, validação por uso real)
- `mustard_core::ast::signature::extract_via_fallback_regex` — CONSUMIDO (zero mudança, validação por uso real)

## Tarefas

3 waves (light scope):

- **W1 (rt)** — Migrar `entity_extractor::extract_decls` pra delegar ao `mustard_core::ast`. Tests novos cobrindo AC-S-1, AC-S-2, AC-S-4. Tabela literal de prefixos some.
- **W2 (rt)** — `sync_registry` consome mesma API. Test AC-S-3 garante zero divergência entre os 2 consumers.
- **W3 (qa)** — QA + CLOSE. Rodar `/mustard:scan` no próprio Mustard (AC-S-5). Auto-dogfood: o próprio `entity-registry.json` do Mustard fica AST-correct.

Dependências: W1 → W2 (W2 reusa primitivos chamados em W1) → W3.

Paralelismo: W1 e W2 são sequenciais por dependência conceitual mas os arquivos são disjuntos — podem ser feitos em commits separados ou na mesma sessão sem conflito.

## Dependências

- Spec A v4 (`2026-05-27-mustard-v4-foundation`) — **CLOSED 2026-05-28**.

## Limites

- **NÃO** muda assinatura pública de `mustard_core::ast::extract_function_signatures`.
- **NÃO** introduz dependência nova (`tree-sitter`, `tree-sitter-loader` já são deps transitivas via `mustard-core`).
- **NÃO** muda shape do `entity-registry.json` — só a EXTRAÇÃO.

## Riscos / Decisões

1. **Performance**: AST é mais caro que string matching. Mitigação: o scan já cacheia via SHA-256 (`.claude/.scan-cache.json`); reuso intacto. Se hot path degradar mensurável, expor `extract_decls_fast` que pula AST e cai no fallback regex sempre.
2. **Grammar ausente em hosts comuns**: a maioria dos hosts de dev não tem grammar instalada. O fallback regex de `mustard_core::ast` deve produzir output comparável ao string matching atual; AC-S-2 garante.
3. **Schema `entity-registry.json` v4.0 vs v4.1**: se a precisão extra do AST quebrar dashboards que esperam shape antigo, bumpar pra v4.1 com flag de migração. Decisão protegida via AC-S-3 (kinds devem bater entre os 2 consumers).
4. **`entity_extractor.rs` pode sumir**: se após o refactor sobra ≤30 LOC, deletar o arquivo e mover o shim pro `mod.rs`. Decisão em W1 quando o LOC final for medido.

## Notas

Scaffold materializado em 2026-05-28 após finding do `/scan` rodado no próprio Mustard durante a Spec A v4 close — `Task(general-purpose)` agentes despachados pelo /scan não tiveram acesso ao `extract_function_signatures` (não há `mustard-rt run extract-signatures` exposto), então a "Real examples" das SKILLs geradas veio de extração textual. Este finding bate exatamente com o follow-up do agent do rt: *"scan subsystem em apps/rt/CLAUDE.md afirma 'no per-language scanner' mas src/run/scan/ tem 10 arquivos — claim de '100% agnóstico' pode precisar de re-verificação"*.

Aguardando aprovação do user para materializar o PLAN (waves + ACs detalhados por wave) via `mustard-rt run wave-scaffold`.