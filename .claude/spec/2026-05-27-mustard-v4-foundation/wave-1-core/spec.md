# Wave 1 — mustard-core-vocabulary (papel: core)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Primeira das 3 primitivas de `mustard-core` que alimentam o gate. Entrega o matcher de vocabulário em 4 camadas (`semantic`, `pattern`, `keyword`, `noise`) sobre `aho-corasick`. Vocabulário lido em runtime de `.claude/vocab/regression.toml` (editável sem recompilar). Suporte a hot-reload por mtime. Promoção entre camadas sempre pergunta (AC-A-14).

## Arquivos tocados

- `packages/core/src/vocabulary/mod.rs` (NOVO) — `VocabularyMatcher`, `VocabLayer`, types públicos
- `packages/core/src/vocabulary/aho.rs` (NOVO) — wrapper SOLID sobre `aho-corasick`, scan + ranking
- `packages/core/Cargo.toml` (ESTENDIDO) — adiciona dependency `aho-corasick = "1.1"`
- `.claude/vocab/regression.toml` (NOVO) — semente do vocabulário inicial (20 termos do PICKUP)
- `packages/core/src/lib.rs` (ESTENDIDO) — re-export do `vocabulary`

## Funções tocadas

### Em `packages/core/src/vocabulary/` (NOVO)
- `vocabulary::VocabularyMatcher::from_layers`
- `vocabulary::VocabularyMatcher::scan`
- `vocabulary::VocabLayer::parse_from_toml`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-11: `vocabulary::scan` casa 10k chars contra 100 termos em <5ms (bench)
- AC-A-13: Vocabulário em 4 camadas é editável via TOML sem recompilar
- AC-A-14: Promoção entre camadas SEMPRE pergunta (AskUserQuestion)

## Tarefas

- [ ] T1.1: Criar `packages/core/src/vocabulary/mod.rs` com `VocabularyMatcher`, `VocabLayer` e types públicos (4 camadas: semantic, pattern, keyword, noise)
- [ ] T1.2: Implementar `vocabulary::VocabularyMatcher::from_layers` e `scan` em `packages/core/src/vocabulary/aho.rs` via wrapper SOLID sobre `aho-corasick`
- [ ] T1.3: Implementar `vocabulary::VocabLayer::parse_from_toml` carregando `.claude/vocab/regression.toml` em runtime com suporte a hot-reload por mtime (AC-A-13)
- [ ] T1.4: Estender `packages/core/Cargo.toml` adicionando `aho-corasick = "1.1"` e re-exportar `vocabulary` em `packages/core/src/lib.rs`
- [ ] T1.5: Criar `.claude/vocab/regression.toml` com a semente inicial (20 termos do PICKUP) distribuídos pelas 4 camadas (AC-A-13)
- [ ] T1.6: Adicionar bench de `scan` (10k chars × 100 termos < 5ms) garantindo o limite (AC-A-11)
- [ ] T1.7: Adicionar guard de promoção entre camadas que dispara `AskUserQuestion` quando o usuário tenta mudar a camada de um termo (AC-A-14)

## Dependências (waves anteriores)

- W0 (fixture + parser de `## Funções tocadas` para auto-validação)