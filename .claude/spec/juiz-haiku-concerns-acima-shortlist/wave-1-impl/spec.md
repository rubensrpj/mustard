---
id: wave.juiz-haiku-concerns-acima-shortlist.1-impl
---

# wave-1-impl

## Resumo

Comando rt concern-judge-render: monta deterministicamente o prompt do juiz (conceitos + anchors por conceito do digest) + parser da resposta

## Rede

- Pai: [[juiz-haiku-concerns-acima-shortlist]]

## Tarefas

- [ ] Criar comando `run concern-judge-render` em apps/rt/src/commands/agent/concern_judge.rs: recebe o intent (+ caminho do modelo), reusa a recuperacao deterministica do digest para obter os conceitos que casaram + os arquivos por conceito (report.terms[].files), e RENDERIZA um prompt de juiz byte-estavel (conceitos + anchors por conceito + contrato de particionar em concerns rotulados). Espelhar agent_prompt_render.rs (montagem deterministica; o julgamento e do LLM).
- [ ] Adicionar o parser da resposta do juiz (JSON de concerns: label + concepts + anchors), tolerante a forma invalida sem panic.
- [ ] Registrar o subcomando em apps/rt/src/commands/mod.rs nos DOIS pontos (variante RunCmd + braco dispatch()), conforme o Guard do rt.
- [ ] Testes: concern_judge_render (render byte-estavel a partir de um fixture do caso sialia) e concern_judge_parse (aceita particao valida; rejeita forma invalida).

## Arquivos

- `apps/rt/src/commands/agent/concern_judge.rs`
- `apps/rt/src/commands/mod.rs`
