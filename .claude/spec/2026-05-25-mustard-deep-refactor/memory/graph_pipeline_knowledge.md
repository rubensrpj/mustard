---
name: graph-pipeline-knowledge
description: O graph (.claude/graph/) mostra apenas conhecimento da pipeline Mustard — specs, skills, commands, refs, recipes, convenções — nunca entidades/enums do projeto-alvo (esses vivem no entity-registry.json)
metadata:
  type: principle
  origin_spec: 2026-05-25-mustard-deep-refactor
  origin_wave: wave-3-mixed
---

# Graph é Conhecimento de Pipeline

O vault Obsidian em `.claude/graph/` tem escopo restrito: ele é o **mapa de conhecimento da pipeline do Mustard**, não do projeto-alvo.

## Tipos canônicos de nó permitidos

- `spec.{slug}` — spec ativa/arquivada
- `skill.{name}` — skill foundation OU detectada pelo scan
- `command.{name}` — slash command (`/mustard:X`)
- `ref.{cmd}.{name}` — ref progressivo
- `recipe.{subproject}.{name}` — recipe gerada pelo scan
- `conv.{subproject}.{slug}` — convenção do subprojeto

## Tipos PROIBIDOS no graph

- `{sub}.entity.X` — entidades vivem em `entity-registry.json` (faceta estruturada)
- `{sub}.enum.X` — idem; enums não são conhecimento navegável

## Origem

User explicitou em 2026-05-25 "graph deveria ser apenas das specs e skills e comandos". Os 12 nós `dashboard.entity.*` + 1 `dashboard.enum.specbucket` foram movidos para `~/.mustard-backups/2026-05-25-recipes-graph-rescope/graph-entity-enum-nodes/` e a heurística de geração foi redesenhada em [[wave-3-mixed]] (T3.6) da [[2026-05-25-mustard-deep-refactor]].

A motivação: graph view do Obsidian deve responder perguntas tipo "qual spec usou esta skill?" / "quais commands chamam essa ref?" — não "quais entidades existem no projeto?" (essa pergunta tem registry pra responder).

## Aplica-se a

- Ao gerar nós graph em `apps/rt/src/run/graph_index.rs` ou `wikilink.rs`: filtrar tipos por allowlist; nada de entity/enum.
- Ao revisar `.claude/graph/`: qualquer `.md` com `entity.` ou `enum.` no nome é candidato a remover.

## Status

Active.

## Relacionado

- [[scan_rust_first]] — graph é parte do scan estrutural Rust
