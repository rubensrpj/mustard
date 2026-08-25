---
id: wave.teto-retentativa.3-plugin
---

# wave-3-plugin

## Summary

o teto de turnos dentro de cada subagente do plugin, declarado no frontmatter

## Network

- Parent: [[spec.teto-retentativa]]

## Tasks

- [ ] declarar `maxTurns` como inteiro positivo no frontmatter dos tres agentes do plugin, ao lado de `model` e `effort`
- [ ] escolher o numero por agente pela natureza do trabalho: os dois autores read-only (`mustard-guards`, `mustard-patterns`) sao curtos; `mustard-review` roda testes e precisa de mais folga

## Files

- `plugin/agents/mustard-guards.md`
- `plugin/agents/mustard-patterns.md`
- `plugin/agents/mustard-review.md`

## Reality Obligations

- **RO-3.1** — confirmar na documentacao oficial do Claude Code que `maxTurns` no frontmatter vale para agente de PLUGIN e nao so para agente de `.claude/agents/` — a medicao local achou a validacao no caminho `plugin_load_agents`, mas o aviso de campo ignorado nomeia so `permissionMode` e `mcpServers`
