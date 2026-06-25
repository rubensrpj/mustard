---
id: spec.scan-enrich-purpose-recall
---

# Enriquecer o scan com o PROPÓSITO de cada declaração de lógica (na língua do projeto) para destravar o recall do digest

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Critério de Aceitação, recall = quantos dos alvos certos a busca encontra, enrich = passo de enriquecimento) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O scan indexa o **nome** de cada declaração (método/classe/tipo) e o digest casa token-contra-token. Ele sabe *"existe um método chamado `EffectivateAsync`"* — nunca *"esse método efetiva uma previsão de pagamento"*. O significado (o propósito) vive só no **corpo** e na cabeça do dev. Quando a palavra que o usuário busca está numa língua/sinônimo diferente do identificador (ex.: PT "efetivar" vs EN `Effectivate`), o método certo **nunca é encontrado** — e nenhuma camada de juiz conserta o que nunca foi mostrado a ela (o juiz só re-ranqueia o que foi recuperado).

Isto foi **medido** (2026-06-25, conjunto rotulado de 10 recall-holes reais da sialia, corpus 27.224 declarações):

| abordagem | recall do conceito central |
|---|---|
| name-match (o digest hoje) | **0/10** |
| embedding do nome (multilíngue) | 0/10 |
| embedding de assinatura+doc cru | 1/10 |
| **lexical sobre sumário-de-PROPÓSITO gerado por LLM lendo o corpo** | **7/10** (8/10 com ≥2 palavras) |

Conclusão: o único sinal que recupera o significado é um sumário de propósito **derivado do corpo**. A boa notícia é o custo: só **6.781** das 27k declarações são métodos de lógica (o resto é property/const/type/field/enum) → **~$5 de Haiku uma única vez** para a sialia inteira, **~$0,10/commit incremental**, **+~1 MB** no grain model, e **query-time continua 100% determinista e sem custo extra** (a busca só lê os tokens `purpose` pré-computados).

Âncoras (camadas que a feature toca):
- packages/core/src/domain/ast/entity.rs — tipo `Declaration` (kind/name/line/supertypes); ganha `purpose` + `body_hash` (serde aditivo).
- packages/core/src/domain/scan.rs — view `Digest`/`DigestTerm`; expõe `purpose` (aditivo, como `specificity_x1024` foi).
- apps/rt/src/commands/lexicon_enrich.rs — precedente do padrão **render→apply** (determinístico no binário; LLM fora) a espelhar.
- apps/rt/src/commands/agent/concern_judge.rs — precedente render byte-estável (sem IO/clock) a espelhar.
- apps/scan/src/digest.rs — match ladder; passa a indexar tokens de `purpose` (aditivo, tier-tagged).
- apps/cli/src (ProjectConfig) — `language` já existe (fonte única); ganha toggle `enrich.purpose` + modelo.
- apps/cli/templates/skills + .claude/commands/mustard/scan — orquestração que despacha o lote LLM e re-alimenta via `--apply`.

Por que agora: o recall 0/10 é o teto mais fundo do subsistema digest, e a investigação fechou que este é o único lever que funciona — barato e determinista no query-time.

## Usuários/Stakeholders

Todo usuário do `/feature`, `/task`, `/bugfix` que pesquisa pelo digest numa língua diferente dos identificadores do código (PT→EN é o caso mais largo; sinônimo dentro do EN é o caso estreito). O ganho é maior em config **não-EN**.

## Métrica de sucesso

1. Num conjunto rotulado de recall-holes (query na língua do `mustard.json` → método cujo nome diverge), o recall do digest sobe de **0/10** (name-match) para **≥7/10** com o índice de `purpose`.
2. Custo do enrich: primeira varredura completa de um repo do porte da sialia ≤ **~$8** e ≤ **~5 min**; re-scan incremental sobre 1 arquivo ≤ **segundos** e ≤ **$0,01**.
3. Query-time inalterado: o binário continua sem chamada de modelo/rede; latência da busca não aumenta.

## Não-Objetivos

- **Não** colocar IA dentro do binário. O binário só RENDERIZA o lote (determinístico) e APLICA os sumários; a chamada LLM mora na orquestração (igual lexicon-enrich / concern-judge / digest-validate).
- **Não** sumariar property/const/type/field/enum nem getters/setters triviais — só `kind ∈ {method, function}` com corpo não-trivial.
- **Não** usar embedding em query-time. O caminho é **lexical** sobre os tokens do `purpose` (mais barato e mais robusto que o embedding pequeno: 7-8/10 vs @10=2/10).
- **Não** hardcodar língua. O `purpose` sai na língua do `ProjectConfig` (carve-out: código/logs/schema seguem como estão).
- **Não** depender do juiz/auto-heal para criar essas pontes — com recall 0 o método certo nunca é surfado, então a fonte das pontes de verbo é o enrich-por-corpo, não o juiz.

## Critérios de Aceitação

- **AC-1** — Build verde.
  Command: `cargo build`
- **AC-2** — `enrich-purpose --render` emite worklist JSON byte-estável `{lang, items}` SÓ para declarações de lógica (pula property/const/type/field) e SÓ as stale (sem `purpose` ou `body_hash` mudou — incremental), sem nenhuma chamada de modelo/rede no binário; rodar 2× dá saída idêntica.
  Command: `cargo test -p mustard-rt enrich_purpose_render`
- **AC-3** — `enrich-purpose --apply` grava `purpose`+`body_hash` nas declarações (aditivo; campos existentes intactos) e re-aplicar com corpo inalterado é no-op (incremental por hash).
  Command: `cargo test -p mustard-rt enrich_purpose_apply_incremental`
- **AC-4** — Recall (DESACOPLADO): `purpose-search` numa fixture espelhando `efetivar→EffectivateAsync` (nome diverge, e o conceito é raro num corpus >150 decls que estoura o cap) devolve o arquivo certo; o name-match do digest NÃO o acha. Cobre o resgate por trigram do gap do stemmer PT.
  Command: `cargo test -p scan purpose_search`
- **AC-5** — Agnóstico: com `mustard.json language=en` o render pede sumário em EN; com `pt-BR`, em PT — parametrizado pelo `ProjectConfig.language`, zero língua hardcoded.
  Command: `cargo test -p mustard-rt enrich_purpose_language_agnostic`
- **AC-6** — No-AI-no-binário preservado: o grep guard não acha cliente LLM/rede nos comandos novos.
  Command: `bash -c "! grep -rnE \"reqwest|anthropic|api_key|http(s)?://api\" apps/rt/src/commands/enrich_purpose* && echo OK"`

## Checklist

- [ ] T1 — Core: `Declaration` ganha `purpose: Option<String>` + `body_hash: Option<String>` (serde aditivo, sem quebrar leitura antiga); `Digest`/`DigestTerm` expõem `purpose` (espelha o aditivo de `specificity_x1024`).
- [ ] T2 — rt `enrich-purpose --render`: worklist JSON incremental `{lang, items:[{id, body}]}` (só declaração de lógica stale), língua do `ProjectConfig`, byte-estável, sem IO/clock/modelo; filtra kind, corpo trivial, E paths não-código (`.claude/`, tests, build).
- [ ] T3 — rt `enrich-purpose --apply`: lê os sumários do lote, grava `purpose`+`body_hash`, pula declaração cujo hash não mudou (incremental).
- [ ] T4 — Comando standalone `purpose-search` (DESACOPLADO do pipeline de anchors): lookup lexical UNCAPPED `purpose`-token→arquivo via o ladder do scan (com resgate trigram pro gap do stemmer PT), ranqueia por nº de conceitos da query batidos, JSON byte-estável. Pipeline de anchors do digest fica INTOCADO (enfiar purpose no BM25F provou frágil: name-match fraco evicta o purpose anchor pelo cap). Reverter o rescue in-pipeline tentado.
- [ ] T5 — Orquestração: o orquestrador chama `purpose-search` SÓ quando o juiz do digest-validate devolve `centralFound=false` (o miss já é detectado lá); fiação no ref `digest-validate` + `scan-enrich-purpose`. O lote de enrich (render→dispatch Haiku→apply) roda no scan sob toggle `enrich.purpose` (default OFF pelo custo); precedente = lexicon-enrich.
- [ ] T6 — Testes: fixture de recall (efetivar-like) + agnóstico (en/pt) + incremental no-op + guard no-AI.
- [ ] T7 — Ref/doc: `.claude/refs` + template explicando o passo, o contrato render→apply, e a regra de língua (fonte única `ProjectConfig`).