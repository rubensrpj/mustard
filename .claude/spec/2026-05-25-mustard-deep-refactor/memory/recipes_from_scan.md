---
name: recipes-from-scan
description: Recipes são detectadas/geradas pelo /scan por subprojeto, refletindo stack+convenções reais — nunca conteúdo hardcoded genérico no payload do mustard init
metadata:
  type: principle
  origin_spec: 2026-05-25-mustard-deep-refactor
  origin_wave: wave-3-mixed
---

# Recipes From Scan

Recipes (esqueletos de operação para o orquestrador injetar nos agentes) **NÃO** são conteúdo curado manualmente. Hardcoded recipes ("Schema/migration: add column — Drizzle pgTable / EF DbContext / Prisma schema — find by entity name") são inúteis: genéricas demais, não refletem o projeto real.

## Regra

Recipes vivem em `.claude/recipes/{subproject}/{operation}.json` e são **geradas pelo `/scan`** lendo:

- `entity-registry.json` (entidades reais)
- `commands/patterns.md` (padrões detectados)
- Amostras de código real

A pasta `apps/cli/templates/recipes/` deve estar vazia (ou só ter README explicando o gerador) — `mustard init` NÃO copia recipes "default".

## Origem

User explicitou em 2026-05-25 ao revisar `.claude/recipes/` no raiz do Mustard: "essa pasta não deveria existir, certo?" (no sentido: não desse jeito hardcoded). Os 5 genéricos foram movidos para `~/.mustard-backups/2026-05-25-recipes-graph-rescope/recipes-hardcoded-genericos/` e a feature recipes foi redesenhada em [[wave-3-mixed]] da [[2026-05-25-mustard-deep-refactor]].

## Aplica-se a

- Qualquer JSON em `.claude/recipes/` SEM agrupamento por subprojeto E sem paths reais é candidato a remover/regenerar.
- Subcomando responsável: scan-structural integra geração via `scan/recipes_generator.rs` (W3.T3.4).
- `recipe_match.rs` aceita `--subproject` opcional para filtrar contexto.

## Status

Active.

## Relacionado

- [[scan_rust_first]] — recipes nascem do scan estrutural Rust
- [[no_hardcoded_stack_patterns]] — recipes não viram catálogo prévio
- [[templates_md_moat]] — templates ficam enxutos sem recipes "default"
