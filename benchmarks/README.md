# Mustard — Benchmark de recall por intenção

> A **métrica-norte** do produto (Fase 0 do roadmap). Prova reproduzível de que o Mustard acha código por **intenção/vocabulário** onde `grep`/match-por-nome falha.

## O que mede

Para uma query em linguagem de domínio (possivelmente em outra língua que o código), dois retrievals deterministas competem em achar o(s) arquivo(s) corretos:

- **name-match** — o digest atual (casa tokens do NOME da declaração). É o baseline (≈ grep estruturado).
- **purpose-search** — casa a query contra o `purpose` (sumário de propósito por método, enriquecido pelo scan). É o fosso.

Recall@k = fração das queries cujo arquivo ground-truth aparece no top-k.

## Formato do conjunto rotulado (`labels.ndjson`)

Uma linha JSON por caso, ground-truth **verificado lendo o código** (não chutado):

```json
{"query": "quitar fatura em aberto", "files": ["src/Billing/InvoiceService.cs"], "lang": "pt-BR", "note": "SettleInvoiceAsync — nome diverge da query"}
```

- `query` — as palavras do usuário (a INTENÇÃO), não termos de código.
- `files` — caminho(s) relativo(s) do(s) arquivo(s) corretos. Um hit = qualquer um deles no top-k.
- `lang` — língua da query (default: `language` do `mustard.json`).
- `note` — por que é um recall-hole (o identificador EN que diverge).

**Disciplina de rotulagem:** cada caso é um recall-hole REAL — a palavra central da query NÃO compartilha token com o nome do identificador (senão o name-match acharia trivialmente). Verificar a função existe e FAZ o que a query diz, lendo corpo/assinatura.

## Runner

```bash
mustard-rt run recall-bench --labels <labels.ndjson> --model .claude/grain.model.json
```

Determinista (sem IA, sem rede): roda name-match + purpose-search por caso, emite JSON byte-estável:

```json
{
  "n": 30,
  "summary": {"nameRecall@1": 0.0, "nameRecall@5": 0.0, "purposeRecall@1": 0.9, "purposeRecall@5": 1.0},
  "cases": [{"query": "...", "files": ["..."], "nameRank": null, "purposeRank": 1}]
}
```

Pré-requisito: o model precisa estar **enriquecido com purposes** (`enrich-purpose --apply`), senão o purpose-search devolve vazio.

## Corpora

- **`fixtures/`** — fixture sintético self-contained (sem dado privado), usado nos testes do runner: reproduzível por qualquer um.
- **Corpora reais** (ex.: sialia) ficam **fora deste repo** — os labels referenciam estrutura de projeto privada. Mede-se em dev; só o runner + o formato + o fixture sintético são versionados aqui.
- **Pitch público** (Fase 0 tardia): um repo OSS (ex.: commerce/CRM) com labels versionados aqui, para a prova ser reproduzível por terceiros.

## Por que isto é pré-requisito de qualquer pitch

Sem um número reproduzível (`name-match X% → purpose-search Y%`), "o Mustard acha melhor" é narrativa. Este runner é o gerador desse número — e a fonte da página Recall do dashboard.
