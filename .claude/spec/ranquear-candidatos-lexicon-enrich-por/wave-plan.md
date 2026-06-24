---
id: wave.ranquear-candidatos-lexicon-enrich-por.plan
---

# Plano de Waves — REDESENHO (híbrido determinístico + juiz LLM)

> **Por que mudou.** A métrica `count×idf` das W1/W2 originais foi REFUTADA por simulação em dados reais (sialia): 44% de acertividade pareada domínio-vs-encanamento (pior que aleatório). Testadas ~16 variantes determinísticas — teto **~94%** (proveniência + embedding), nenhuma chega a >98%; o resíduo é ambiguidade semântica genuína (`value`/`service`/`card-como-afixo`). O que ATINGE >98% é o LLM: medido nos mesmos 66 termos, **Haiku cego = 99,69%**, **Sonnet = 99,95%**. O mustard já tem LLM no loop do enrich — o bug era o binário surfar lixo, fazendo o LLM julgar de uma lista ruim. Design final, nativo (min-IA/max-Rust): binário determinístico ESTREITA por proveniência → passo Haiku PONTUA o top-N na orquestração (fora do binário, respeita o guard). Detalhe na memória `project-mustard-domain-vs-plumbing-ranking-ceiling`.

## Tabela de Waves

| Wave | Papel | Depende de | Resumo |
|------|-------|------------|--------|
| 1 | narrow | — | Trocar o re-rank refutado (count×idf) por ranking de PROVENIÊNCIA no `--check`: demover afixos-papel recorrentes (via `digest.roles`, que o scan já minera) para o domínio sobreviver ao cap. Remover o gate `target_too_generic` (fail-open, piso 256 inerte). Manter o primitivo `domain_specificity_x1024` (TF·IDF é estatística de corpus válida; a mineração PT o usa p/ tirar boilerplate). |
| 2 | judge | wave-1-narrow | `lexicon-judge-render`: prompt byte-estável (espelha `concern-judge-render`) que pede ao LLM nota 0–100 domínio-vs-genérico por candidato (o prompt validado a 99,7%) + parse tolerante. Wire no SKILL do scan/enrich: `--check`/`--check-pt` → `judge-render` → Haiku na orquestração → propõe só os de alta nota → `--apply`. Sonnet opcional só na faixa ambígua. Fallback headless = ranking determinístico da W1. |
| 3 | pt-mining | wave-1-narrow | Manter `--check-pt` (já construído): mineração PT→código por co-ocorrência. Passa a alimentar o MESMO passo do juiz (o Haiku julga PT nativamente — acertou `venda`=90). |

## Critérios de Aceitação
- **AC-1** — `lexicon-judge-render` emite o prompt de scoring validado (0–100 domínio-vs-genérico), byte-estável, e `parse_lexicon_scores` tolera prosa/cerca sem panic. Command: `rtk cargo test -p mustard-rt lexicon_judge`
- **AC-2** — `--check` ranqueia por proveniência (afixo-papel recorrente demovido), NÃO por specificity; o gate `target_too_generic` foi removido do `--apply` (sobra só o anti-hallucination). Command: `rtk cargo test -p mustard-rt lexicon_enrich`
- **AC-3** — view `Digest` do core expõe `roles` aditivamente (serde compat com modelo antigo). Command: `rtk cargo test -p mustard-core scan`
- **AC-4** — build limpo, suíte verde, sem dead code da métrica refutada como classificador. Command: `rtk cargo build && rtk cargo test -p mustard-rt --lib`
- **AC-5** — EMPÍRICO no sialia: `--check` traz termos de domínio entre os candidatos (não só plumbing no topo); o passo Haiku sobre esses candidatos pontua domínio acima de genérico (validação do desenho a 99,7%). Command: `mustard-rt run lexicon-enrich --check --root C:/Atiz/sialia` + `mustard-rt run lexicon-judge-render` (dispatch Haiku)
- **AC-6** — determinismo: `--check`, `--check-pt` e `judge-render` byte-idênticos em duas execuções no mesmo modelo. Command: duas execuções, diff vazio