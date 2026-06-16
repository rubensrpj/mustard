# Spec — Léxico auto-enriquecido (`enrich`)

> Status: PROPOSTA (aguardando aprovação + decisão da bifurcação A/B)
> Origem: avaliações de campo sialia — o gap de vocabulário (`titulo→payable`) é a
> fricção nº1 recorrente. O IDF (commit `5c48a102`, deployado) consertou o
> **ranking** (colisão de framework). Esta spec ataca o **vocabulário**: a ponte
> precisa EXISTIR no léxico, e hoje é curada à mão / reativamente.

## Invariante (a linha que não se cruza)

**A IA mantém o DADO (o overlay de léxico) — o motor determinístico USA o dado, sempre.**
O `digest` jamais chama IA. O `scan` (miner) continua 100% determinístico e sem IA.
O enrich é uma CAMADA separada que produz `.claude/lexicons/<par>.toml`; o digest lê
esse arquivo congelado. (Guard de `apps/scan/CLAUDE.md` preservado: nada de IA em `scan/`.)

## Decisão central — onde a IA roda (A × B)

**Achado do ANALYZE:** o `mustard-rt` hoje é 100% Rust determinístico — **zero** shell-out
de LLM (o `prd-build` é port puro-Rust; o `claude -p` vive só no dashboard).

- **Opção A — `mustard-rt run enrich` shella `claude -p`.** Auto-contido, roda headless
  (com chave). **Custo:** introduz a PRIMEIRA dependência de LLM + chave de API no binário
  `rt` (hoje determinístico), e acopla a um provedor/modelo. Fere o roadmap Codex.

- **Opção B — orquestrador-mediado (RECOMENDADO).** O `rt` prepara a entrada determinística
  e aplica a saída determinística; **o orquestrador (o modelo do harness, já presente) propõe
  as pontes**. O `rt` continua determinístico; agnóstico de provedor (Claude hoje, Codex
  depois); gate natural. É o MESMO split do resto do mustard (rt determinístico + IA no
  orquestrador). **Custo:** precisa de um agente presente — em scan headless puro (CI) o
  enrich é pulado (fail-open usa o overlay commitado). Aceitável.

**Recomendação: B.** É a única que mantém os dois binários determinísticos E agnósticos de
provedor. O resto da spec assume B (A seria uma variação do passo 2).

## Fluxo (Opção B)

```
1. mustard-rt run enrich --check --root <dir>        [rt, DETERMINÍSTICO]
     lê grain.model.json (TermD: term/count/samples)
     diff contra as chaves do overlay atual + seed
     → emite JSON dos termos de domínio NOVOS/sem-ponte (incremental)
     vazio → no-op (custo zero)
        │
        ▼
2. orquestrador (modelo do harness)                  [IA, já presente]
     recebe: vocabulário do código (os termos) + língua do projeto (mustard.json)
     propõe pontes: <palavra-do-usuário> = [<termos-do-código>]
        │
        ▼
3. mustard-rt run enrich --apply <proposals.json>    [rt, DETERMINÍSTICO + GATE]
     valida cada termo-código-alvo CONTRA o modelo real (rejeita alvo inexistente
       = mata alucinação deterministicamente)
     escreve em .claude/lexicons/<par>.toml (alfabético, atômico, preserva comentários)
     → reporta o diff aplicado
```

## Os 4 guarda-corpos (o que torna "sempre padrão" seguro)

1. **Incremental** — `--check` só emite termos NOVOS (diff modelo × overlay). Nada novo → no-op.
2. **Fail-open** — sem orquestrador/headless → enrich pulado, usa overlay commitado; o scan
   NUNCA falha por causa disso.
3. **Cacheado/commitado** — o overlay é dado no repo; o digest é determinístico sobre ele.
4. **Gated** — `--apply` valida que cada alvo EXISTE no modelo (gate determinístico anti-
   alucinação) + o orquestrador/usuário revê o diff. Mesmo princípio do `lexicon-suggest --accept`.

**"Sempre padrão"** = o fluxo de scan/feature do orquestrador roda `--check` automaticamente;
havendo termos novos E modelo disponível, propõe + aplica. Headless → pula (fail-open).

## Reúso (do ANALYZE — não reinventar)

| Peça | Origem | Uso |
|---|---|---|
| `overlay_path()`, `upsert_term()`, `accept_report()` | `rt/commands/lexicon_suggest.rs` | escrita atômica, alfabética, preserva comentários |
| `folded()`, `effective_lexicon()` | `rt/commands/lexicon_suggest.rs` | dedup contra seed+overlay já cobertos |
| `parse_lexicon()` + resolução de par | `scan/matching.rs`, `scan/stemmers.rs` | merge seed+overlay; `project_lexicon()` lê |
| `TermD {term,count,samples}` | `scan/digest.rs` | vocabulário do código = entrada da IA |
| variante `RunCmd` + braço `dispatch()` | `rt/commands/mod.rs` | `Enrich` irmão de `LexiconSuggest` |

O `lexicon-suggest` (reativo: corrige depois de 2 queries correlacionadas) é a SEMENTE; o
`enrich` é a versão PROATIVA (popula antes da 1ª query falhar). Mesmo destino (o overlay).

## Ondas (escopo Full)

- **Onda 1 (rt) — `enrich --check`:** detecção incremental de termo novo (diff modelo × chaves
  do overlay+seed), determinística, emite JSON estruturado. Reusa índice de termos + leitor de overlay.
- **Onda 2 (rt) — `enrich --apply <json>`:** valida alvos contra o modelo (gate), escrita via
  `upsert_term` (atômica/alfabética), reporta diff. Reusa o writer do lexicon-suggest.
- **Onda 3 (prosa/SKILL):** liga o passo do orquestrador — o fluxo scan/feature roda
  check→propor→apply por padrão; contrato do prompt pro modelo; fail-open quando headless.
- **Onda 4 (opcional):** marker de model-hash em `.claude/.harness/` p/ pular quando nada mudou.

## Acceptance Criteria (cada um com comando)

- **AC-1** `enrich --check` num modelo com termo de domínio sem-ponte LISTA ele; num modelo
  totalmente pontificado retorna vazio (no-op).
- **AC-2** `enrich --apply` com ponte p/ termo REAL do modelo escreve no overlay (alfabético,
  atômico); ponte p/ termo INEXISTENTE é rejeitada (gate anti-alucinação).
- **AC-3** após `--apply`, a query na palavra do usuário acerta o arquivo pontificado em UMA
  rodada (e2e: semear modelo → enrich → digest query).
- **AC-4** headless / sem propostas → scan + digest seguem funcionando sobre o overlay
  commitado (fail-open, scan nunca bloqueia).
- **AC-5** `--check` é determinístico e byte-estável (duas execuções idênticas).

## Fora de escopo

- Tradução do código (identificadores ficam como estão; a IA só MAPEIA palavra→termo).
- IA no caminho da query/digest (determinismo do hot path preservado).
- Substituir stoplist/stemmer (Snowball, determinísticos, corretos — ficam).
