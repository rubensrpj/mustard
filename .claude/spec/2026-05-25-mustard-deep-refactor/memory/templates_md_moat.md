---
name: templates-md-moat
description: Arquivos .md em apps/cli/templates/ são o moat do Mustard — devem ser enxutos, sem refs legadas (bun/JS), e ditam qualidade do scan + entity-registry
metadata:
  type: principle
  origin_spec: 2026-05-25-mustard-deep-refactor
  origin_wave: wave-6-cli
---

# Templates .md São o Moat

Os arquivos `.md` em `apps/cli/templates/` (templates copiados para `.claude/` do projeto-alvo) são o moat real do Mustard. Eles alimentam:

- O `/scan` (qualidade dos `.md` gerados depende do estilo dos templates)
- O `entity-registry.json` (parser/heurísticas leem dos templates)
- A janela de contexto da IA (templates inchados = sessão cara desde o boot)

## Regra

Sempre que tocar em templates, conferir 3 coisas:

1. Tamanho real (linhas) vs equivalente cru no payload
2. Referências legadas (`bun`, `JavaScript`, `node scripts/`, `npm run`, `.mjs`)
3. Coerência com a arquitetura Rust atual (sem hooks JS, sem comandos externos npm)

## Origem

User explicitou em 2026-05-25 que isso é "o moat do mustard"; templates inchados influenciam negativamente scan + entity-registry, que são a base de tudo. Formalizado em [[2026-05-25-mustard-deep-refactor]] com tratamento concreto em [[wave-6-cli]] (cortes nos 12 commands + pipeline-config + refs grandes + sweep refs antigas).

## Aplica-se a

- Ao iniciar qualquer wave que toque `templates/`, gerar um diff de linhas (antes/depois) e auditar refs legadas.
- W6 corta `commands/mustard/*/SKILL.md` (total ≤800), `pipeline-config.md` (489→200), `refs/scan/scan-protocol.md` (368→180), `refs/git/merge-protocol.md` (277→150).
- "Otimizar" template ≠ apenas encurtar; é também desligar de tecnologias eliminadas (bun foi eliminado em spec anterior `eliminate-bun`).

## Status

Active.

## Relacionado

- [[scan_rust_first]] — templates alimentam scan
- [[no_hardcoded_stack_patterns]] — templates não devem catalogar stacks
