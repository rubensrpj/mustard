---
id: wave.matar-prd-standalone-fazer-feature.2-purge-prd
---

# wave-2-purge-prd

## Resumo

Remover o PRD standalone: prd-build (rt), skill mustard:prd, e qualquer exposição em mcp; manter PRD_SECTIONS

## Rede

- Pai: [[matar-prd-standalone-fazer-feature]]

## Tarefas

- [ ] Deletar apps/rt/src/commands/spec/prd_build.rs e desregistrar em spec/mod.rs e commands/mod.rs
- [ ] Deletar o skill apps/cli/templates/commands/mustard/prd/SKILL.md
- [ ] Varrer callers de prd-build/PrdReport (apps/mcp/src/lib.rs e outros) e remover a exposição
- [ ] Confirmar que PRD_SECTIONS em packages/core/src/domain/spec/contract.rs permanece intacto

## Arquivos

- `apps/rt/src/commands/spec/prd_build.rs`
- `apps/rt/src/commands/spec/mod.rs`
- `apps/rt/src/commands/mod.rs`
- `apps/cli/templates/commands/mustard/prd/SKILL.md`
- `apps/mcp/src/lib.rs`
- `packages/core/src/domain/spec/contract.rs`
