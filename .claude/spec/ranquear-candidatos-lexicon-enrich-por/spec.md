---
id: spec.ranquear-candidatos-lexicon-enrich-por
---

# Ranquear candidatos do lexicon-enrich por especificidade de dominio (TF-IDF count x idf) em vez de frequencia, gate de qualidade do alvo no apply, e mineracao de vocabulario PT para alinhar ao codigo

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Ranquear candidatos do lexicon-enrich por especificidade de dominio (TF-IDF count x idf) em vez de frequencia, gate de qualidade do alvo no apply, e mineracao de vocabulario PT para alinhar ao codigo.

Âncoras (do scan):
- apps/rt/src/commands/lexicon_enrich.rs (lexicon, enrich, unbridged, term)
- packages/core/src/domain/ranking.rs (idf, ranking, corpus)
- apps/scan/src/digest.rs (term, idf, digest, corpus)
- apps/dashboard/src/features/workspace/WorkspaceFilesRanking/index.tsx (ranking)
- apps/cli/src/commands/install_grammars.rs (postings)
- apps/dashboard/src-tauri/src/telemetry_agg.rs (postings)
- apps/rt/src/commands/lexicon_suggest.rs (lexicon, bridge, term, postings)
- apps/rt/src/commands/feature.rs (lexicon, term, gate, digest)
- apps/scan/src/matching.rs (lexicon, bridge, postings)
- apps/rt/src/commands/digest_precision.rs (term, digest)
- apps/rt/src/commands/knowledge/recall.rs (ranking, corpus)
- packages/core/src/domain/scan.rs (term, digest)

Fatias recorrentes (precedente a espelhar): terms (×4)

Por que agora.

## Usuários/Stakeholders

Quem se beneficia.

## Métrica de sucesso

Métrica de sucesso.

## Não-Objetivos

O que fica de fora.

## Critérios de Aceitação

- **AC-1** — Pipeline build green
  Command: `cargo build`

<!-- PLAN -->

## Arquivos

Listar arquivos afetados.

## Limites

IN: ...
OUT: ...
