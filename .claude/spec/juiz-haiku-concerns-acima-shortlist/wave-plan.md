---
id: wave.juiz-haiku-concerns-acima-shortlist.plan
---

# Plano de Waves

## Tabela de Waves

| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-impl]] | impl | — | Comando rt concern-judge-render: monta deterministicamente o prompt do juiz (conceitos + anchors por conceito do digest) + parser da resposta |
| 2 | [[wave-2-surface]] | surface | [[wave-1-impl]] | Passo do juiz Haiku no SKILL do /feature: gate multi-concern -> render -> dispatch haiku -> parse -> decompor por concern, com fallback deterministico |

## Critérios de Aceitação
- **AC-1** — Build verde. Command: `cargo build`
- **AC-3** — Render deterministico byte-estavel a partir de um fixture. Command: `cargo test concern_judge_render`
- **AC-4** — Parser aceita particao valida e rejeita invalida sem panic. Command: `cargo test concern_judge_parse`
- **AC-2** — Suite completa verde sem regressao. Command: `cargo test`
- **AC-5** — O SKILL do /feature contem o passo do juiz (render -> dispatch haiku -> parse -> decompor por concern) com fallback deterministico. Command: `grep -q concern-judge-render apps/cli/templates/commands/mustard/feature/SKILL.md`
