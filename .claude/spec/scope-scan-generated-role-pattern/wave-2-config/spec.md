---
id: wave.scope-scan-generated-role-pattern.2-config
---

# wave-2-config

## Summary

O frontmatter dos comandos passa a usar as chaves que a plataforma honra

## Network

- Parent: [[spec.scope-scan-generated-role-pattern]]
- Depends on: [[wave.scope-scan-generated-role-pattern.1-backend]]

## Tasks

- [ ] Nos seis utilitarios (knowledge, maint, skills, stats, status, upsert): acrescentar disable-model-invocation: true ao frontmatter. Nao mexer em mais nada do arquivo.
- [ ] Em review.md: acrescentar context: fork, agent: (o subagente de review do plugin) e background: false. Registrar em comentario que background: false exige Claude Code v2.1.218 e e ignorado por versoes anteriores.
- [ ] NAO tocar em qa.md, feature.md, bugfix.md, task.md, tactical-fix.md, spec.md, mustard.md, scan.md, close.md, git.md, rehook.md, unhook.md.
- [ ] Criar o teste de ratchet command_frontmatter em apps/rt/tests/: le plugin/commands/*.md e falha se um dos seis utilitarios perder disable-model-invocation, se review perder as tres chaves, ou se qa ganhar qualquer chave de fork. Mesmo padrao de run_command_surface.rs.
- [ ] Prova viva do bloqueio: confirmar que context: fork e honrado num arquivo plano de commands/. Se nao for, PARAR e reportar: o plano B (mover review para plugin/skills/review/SKILL.md com name: review) e decisao do usuario, nao desta onda.

## Files

- `plugin/commands/knowledge.md`
- `plugin/commands/maint.md`
- `plugin/commands/skills.md`
- `plugin/commands/stats.md`
- `plugin/commands/status.md`
- `plugin/commands/upsert.md`
- `plugin/commands/review.md`
