---
id: spec.digest-concern-split-por-co
---

# digest concern split por co-ocorrencia: query emite N sub-digests rotulados e a IA analisa apos todas as quebras retornarem

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

digest concern split por co-ocorrencia: query emite N sub-digests rotulados e a IA analisa apos todas as quebras retornarem.

Âncoras (do scan):
- apps/scan/src/digest.rs (query, digest, postings, graph)
- apps/rt/src/commands/lexicon_suggest.rs (query, postings, term, fold)
- packages/core/src/domain/scan.rs (query, digest, term)
- apps/dashboard/src/pages/Knowledge.tsx (rank, postings)
- apps/cli/templates/skills/skill-creator/scripts/run_eval.py (query)
- apps/dashboard/src-tauri/src/economy.rs (postings)
- apps/mcp/src/lib.rs (query)
- apps/scan/src/rank.rs (rank, stratum, diversity)
- apps/rt/src/commands/feature.rs (query, digest, term)
- apps/rt/src/commands/wave/epic_fold.rs (postings, fold)
- packages/core/src/domain/vocabulary/stacks.rs (component, postings, term)
- apps/scan/src/matching.rs (query, postings, fold)

Fatias recorrentes (precedente a espelhar): Report (×7), main+new+run+src (×6), args (×2)

**Por que agora.** O ranqueador do digest soma a pontuação de relevância (BM25F) sobre TODOS os conceitos da consulta numa única recuperação, devolvendo uma lista de âncoras (arquivos-alvo) plana. Quando o `--intent` carrega vários assuntos independentes — chamamos cada um de *concern* (ex.: 1 correção + 2 melhorias num mesmo pedido) — os conceitos do concern mais "denso em termos" dominam o ranking e afogam os demais; o componente compartilhado de nome genérico (ex.: um `DataTable`) não aparece. A avaliação de campo no sialia (2026-06-23, binário atual) mostrou que a vitória (achar reuso no backend) e a derrota (perder o componente genérico) são o MESMO viés. A diversidade-por-projeto (stratum) já mitiga representação por projeto, mas NÃO separa concerns. Esta feature quebra a consulta em concerns determinísticos (componentes conexos por co-ocorrência) e devolve um ranking por concern — num único payload completo, que a inteligência artificial (IA) só analisa depois que TODAS as quebras retornarem (assim ela não foca só no primeiro concern e perde os outros).

## Usuários/Stakeholders

- **O orquestrador / a IA que consome o digest** (`/feature`, `/task`): recebe rankings limpos por concern em vez de uma lista única diluída, então roteia cada concern para o trilho certo (correção vs melhoria) sem caçar à mão o que o ranking afogou.
- **O operador (indireto)**: menos retrabalho de roteamento e menos `Grep` manual para achar o alvo que a lista única escondia.

## Métrica de sucesso

Numa consulta multi-concern GRAVADA (loop de outcome — os eventos `feature.query` em `.claude/.session/*/.events/*.ndjson`), cada concern recupera seu arquivo-alvo no top-5 do SEU sub-digest, sem que o concern denso o afogue — medido A/B (antes: lista única; depois: N sub-digests). Invariante de não-regressão: consulta de concern ÚNICO produz exatamente 1 cluster e o mesmo ranking de hoje (zero regressão nas consultas com match forte).

## Não-Objetivos

- **Hub genérico sem termo nem path** (a classe ABERTA do "Feature B" — surfar o serviço central de nome genérico por conexão de grafo): fora de escopo; depende de re-scan com `deps` persistidas.
- **Chamar IA/modelo para segmentar** os concerns: a quebra é 100% determinística e agnóstica (reusa `postings` + grafo já existentes).
- **Mudar o algoritmo de pontuação BM25F per-conceito**: só PARTICIONAMOS os conceitos e rodamos a pontuação existente por partição.
- **Adivinhar qual concern o usuário "quer"**: devolvemos TODOS, rotulados.

## Critérios de Aceitação

- **AC-1** — Build do pipeline verde
  Command: `cargo build`
- **AC-2** — Suite completa verde (sem regressão)
  Command: `cargo test`
- **AC-3** — Consulta de concern único produz exatamente 1 cluster (sem over-split)
  Command: `cargo test cooccurrence_single_concern_one_cluster`
- **AC-4** — Conceitos disjuntos (sem co-ocorrência nem ponte de grafo) quebram em ≥2 sub-digests
  Command: `cargo test cooccurrence_disjoint_concepts_split`
- **AC-5** — Numa fixture multi-concern, cada concern recupera seu alvo no seu próprio sub-digest
  Command: `cargo test concern_split_target_surfaces_per_concern`
- **AC-6** — O `note()` emitido carrega o contrato "analise apenas após todas as quebras retornarem"
  Command: `cargo test note_emits_all_breaks_first_contract`

<!-- PLAN -->

## Arquivos

- `packages/core/src/domain/scan.rs` — estende `QueryResult` (campos `files`/`files_detail` planos, `digest.rs:154-200`) com o agrupamento por concern: um novo tipo de view (lista de sub-digests rotulados; cada concern carrega seus próprios `files`/`files_detail`/`reason`). Camada: **core (tipos)**.
- `apps/scan/src/digest.rs` — `query()` (`255-462`): após montar os `QConcept` (`382-417`), agrupa os conceitos da query em componentes conexos por co-ocorrência (interseção dos conjuntos de módulos em `Corpus::postings`, `696`; ponte opcional via `Corpus::imports`, `708`) = concerns; pontua cada cluster com o BM25F existente (`bm25f_contribution_x1024` + `stratified_order`) restrito aos seus conceitos; monta os N sub-digests. Camada: **scan (motor)**.
- `apps/scan/src/rank.rs` — helper novo de componentes conexos (union-find sobre os conceitos da query); não há util de connected-components hoje. Camada: **scan (helper)**.
- `apps/rt/src/commands/feature.rs` — `payload()` (`128-224`) emite os N sub-digests rotulados num payload completo; `note()` (`365-394`) ganha a instrução do contrato: "analise apenas após TODAS as quebras retornarem". Camada: **rt (superfície)**.

## Limites

IN: clustering dos conceitos da consulta em concerns por co-ocorrência (interseção de `postings` + ponte por `imports`) dentro do `query()`; a pontuação BM25F existente rodada POR cluster; `QueryResult` passa a carregar os N sub-digests rotulados; `payload()` os emite num payload completo; `note()` instrui "analise só após todas as quebras"; tudo determinístico, sem modelo.
OUT: hub-genérico-sem-termo (Feature B); chamada de modelo; alteração do BM25F per-conceito; inferência de qual concern o usuário quer.

GATE DE PROCESSO (obrigatório antes do commit): medição A/B contra as consultas GRAVADAS (loop de outcome), com o resultado registrado neste spec — esta classe de truque de agregação no digest já resistiu a 4 tentativas e reverts, então NÃO commitar sem a medição. O AC-5 é o proxy hermético (fixture); este gate é a validação sobre as consultas reais.