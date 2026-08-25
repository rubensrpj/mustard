---
id: wave.gatilho-medido-enriquecimento.2-docs
---

# wave-2-docs

## Summary

a prosa semeada do roteador ganha a regra que le o sinal, e um teste trava prosa e codigo no mesmo literal

## Network

- Parent: [[spec.gatilho-medido-enriquecimento]]
- Depends on: [[wave.gatilho-medido-enriquecimento.1-backend]]

## Tasks

- [ ] Acrescentar a `packages/core/templates/mustard/orchestrator.md`, na secao `## Locating code`, UMA linha: ao ler o sinal do portao de base no stderr, dizer ao operador em uma frase que o modelo do projeto esta pela metade e oferecer o fluxo `scan` como unidade PROPRIA, despachada so depois que a unidade corrente fechar, em arvore limpa.
- [ ] A linha vai em `orchestrator.md` e NAO em `dispatch.md`. Motivo medido: `dispatch.md` viaja no evento `sessionStart`, que soma 8072 caracteres do proprio arquivo mais o censo (~950) e as advertencias dentro de um teto de 10000; `orchestrator.md` viaja no `userPromptSubmit` com 5927 caracteres e folga larga. Estourar o teto nao corta o texto: ele vira referencia de arquivo e para de estar EM VIGOR.
- [ ] Anexar a impressao digital da versao superseded do seed a `PRIOR_ORCHESTRATOR_FINGERPRINTS` em `packages/core/src/platform/project_seed.rs`. Sem isso o teste `the_fingerprint_catalog_covers_every_history` reprova, e instalacoes existentes preservam a prosa velha achando que e customizacao do operador.
- [ ] Re-semear a copia entregue `.claude/mustard/orchestrator.md` a partir do template — o teste de deriva compara as duas byte a byte.
- [ ] Trocar em `plugin/commands/scan.md` a descricao `Weak fallback only: use when the router did not engage and the model is visibly stale` pelo gatilho medido que o portao agora emite. A descricao e o que faz o fluxo ser descoberto; enquanto ela disser `visivelmente`, o gatilho continua sendo palpite.
- [ ] Acrescentar a `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs` o teste `the_router_prose_names_the_signal_the_gate_emits`, que le a constante do crate e afirma que o MESMO literal aparece na prosa do seed do orquestrador. E o teste que impede as duas metades de divergirem em silencio, exatamente o proposito declarado desse arquivo.

## Files

- `packages/core/templates/mustard/orchestrator.md`
- `packages/core/src/platform/project_seed.rs`
- `.claude/mustard/orchestrator.md`
- `plugin/commands/scan.md`
- `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs`
