@.claude/scan-map.md

# Scan

> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/CLAUDE.md](../../.claude/CLAUDE.md)



## Guards

<!-- mustard:guards -->
<!-- facts: kind=cargo; frameworks=toml, clap, serde, serde_json, ignore, petgraph, regex, anyhow, tree-sitter, streaming-iterator, grammar_cs, grammar_ts -->
- Mantenha o miner 100% determinístico e SEM IA: nada de síntese, rede ou heurística não reprodutível — ordene/deduplique tudo e use desempates estáveis (a ordem do `HashMap` muda entre execuções).
- Nunca cite uma linguagem, extensão, nó de gramática ou nome de framework dentro de `src/`: esses dados vivem só em `languages.toml`, `manifests.toml` e `queries/<dir>/*.scm` — adicionar linguagem ou build-system é uma linha de dado, jamais lógica nova.
- As queries `.scm` só falam o vocabulário genérico de captura (`@import`, `@namespace`, `@definition.<kind>`, `@name`, `@supertype`); o motor copia o sufixo de `kind` verbatim, então não ensine nomes de nó específicos da gramática a `extract.rs`.
- Não detecte papéis por nome conhecido (tipo "Controller"/"Repository"): o miner descobre convenção por recorrência (afixos, pastas-papel, clusters por Jaccard) — mantenha-o cego a qualquer arquitetura.
- A extração tree-sitter é tolerante a falha: pattern que não compila é descartado individualmente e linguagem cuja gramática falha é pulada com aviso — nunca dê panic nem aborte o scan inteiro, e preserve o fallback textual agnóstico.
- O produto é o `grain.model.json`; `facts`/`digest`/`spec` projetam a partir do modelo e nunca releem o repositório — não quebre o `preserve_order` do `serde_json` (o ranking de deps segue a ordem do manifesto, não a alfabética).
<!-- /mustard:guards -->
