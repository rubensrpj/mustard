# wave-3-cli — Mover stack-templates e reclassificar artifacts opt-in

### Parent: [[2026-05-26-template-agnostic-audit]]
### Stage: Plan
### Outcome: Active
### Flags:

## Resumo

Mover `templates/refs/stack-templates/` (`fe-craft-check.md` + `browser-debug.md`) para `templates-extras/refs/stack-templates/` — mesmo padrão opt-in do `hallmark`. Reclassificar `skill:react-best-practices` em `.artifacts.json` como opt-in. `mustard init` para de instalar artefatos UI-específicos por default.

## Network

- Parent: [[2026-05-26-template-agnostic-audit]]
- Depends on: [[wave-2-cli]]

## Arquivos

- `apps/cli/templates/refs/stack-templates/fe-craft-check.md` (MOVE → `templates-extras/refs/stack-templates/fe-craft-check.md`)
- `apps/cli/templates/refs/stack-templates/browser-debug.md` (MOVE → `templates-extras/refs/stack-templates/browser-debug.md`)
- `apps/cli/templates/refs/stack-templates/` (RMDIR se vazio)
- `apps/cli/templates/.artifacts.json` (MODIFY)
- `apps/cli/templates-extras/.artifacts.json` (CREATE ou MODIFY)
- `apps/cli/src/{commands/init.rs,fs_ops.rs}` (CHECK copy logic)

## Tarefas

### CLI Agent
- [ ] mkdir -p `apps/cli/templates-extras/refs/stack-templates/`
- [ ] git mv dos 2 arquivos para o novo local
- [ ] rmdir `templates/refs/stack-templates/` se vazia
- [ ] Auditar `templates/.artifacts.json` por `skill:react-best-practices`; mover para `templates-extras/.artifacts.json` (criar se não existir)
- [ ] Inspecionar copy logic do `mustard init`: confirmar skip de `templates-extras/` por default
- [ ] `cargo build -p mustard-cli`
- [ ] Smoke: `cargo run -p mustard-cli -- init` em tmpdir — confirmar ausência de `.claude/refs/stack-templates/` e ausência de `react-best-practices` no instalado

## Critérios de Aceitação

- [ ] AC-W3-1: `templates/refs/stack-templates/` não existe — Command: `node -e "process.exit(require('fs').existsSync('apps/cli/templates/refs/stack-templates')?1:0)"`
- [ ] AC-W3-2: `templates-extras/refs/stack-templates/` contém os 2 arquivos — Command: `node -e "const p='apps/cli/templates-extras/refs/stack-templates';const fs=require('fs');process.exit(fs.existsSync(p)&&fs.readdirSync(p).includes('fe-craft-check.md')&&fs.readdirSync(p).includes('browser-debug.md')?0:1)"`
- [ ] AC-W3-3: `templates/.artifacts.json` não tem `react-best-practices` — Command: `node -e "const c=require('fs').readFileSync('apps/cli/templates/.artifacts.json','utf8');process.exit(c.includes('react-best-practices')?1:0)"`

## Limites

- MOVE: 2 arquivos
- MODIFY: `templates/.artifacts.json`, `templates-extras/.artifacts.json`
- CHECK: copy logic em `apps/cli/src/`
- FORA: outras skills, outros refs
