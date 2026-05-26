# wave-2-cli — Reescrever CLAUDE.md + sanear pipeline-config.md

### Parent: [[2026-05-26-template-agnostic-audit]]
### Stage: Plan
### Outcome: Active
### Flags:

## Resumo

Reescrever `templates/CLAUDE.md` do zero em tom meta-agnóstico — zero menção a stack do próprio Mustard (cargo, mustard-rt, mustard-cli, pnpm). Sanear `templates/pipeline-config.md` removendo roles fixos (api/mobile/ui/database/library com Flutter→mobile), placeholders `{backend}/{frontend}/{admin}`, e default "DB+Backend Wave 1 / Frontend Wave 2".

## Network

- Parent: [[2026-05-26-template-agnostic-audit]]
- Depends on: [[wave-1-cli]]
- Blocks: [[wave-3-cli]], [[wave-5-mixed]]

## Arquivos

- `apps/cli/templates/CLAUDE.md` (REWRITE)
- `apps/cli/templates/pipeline-config.md` (MODIFY)

## Tarefas

### CLI Agent — CLAUDE.md
- [ ] Ler `.claude/CLAUDE.md` como referência de tom didactic (NÃO copiar — é meta-instrução do orchestrator local)
- [ ] Reescrever `templates/CLAUDE.md` mantendo APENAS: Role do orchestrator, Response Style, Intent Routing, L0 delegation, Spec Layout, ponteiro para `pipeline-config.md`
- [ ] REMOVER: blocos Build, Stack, Commands, vault layout com `entity`, qualquer referência a cargo/mustard-rt/mustard-cli/pnpm
- [ ] Validar: grep por `cargo build|cargo test|mustard-rt run|mustard-cli|pnpm --filter` retorna zero linhas

### CLI Agent — pipeline-config.md
- [ ] Localizar seção "Role Rules" com tabela `api/mobile/ui/database/library` e Flutter→mobile
- [ ] Substituir tabela hardcoded por comentário: "Role Rules são populados por /scan a partir dos subprojetos detectados — não há lista canônica de roles"
- [ ] Remover linha/seção de default "DB+Backend Wave 1 / Frontend Wave 2"
- [ ] Seção "Recipe Engine" (se existir): remover placeholders `{backend}`, `{frontend}`, `{admin}` como canônicos
- [ ] `cargo build -p mustard-cli`
- [ ] Smoke: `cargo run -p mustard-cli -- init` em tmpdir, ler `.claude/CLAUDE.md` gerado, confirmar zero Rust/cargo

## Critérios de Aceitação

- [ ] AC-W2-1: `templates/CLAUDE.md` zero menção a cargo/mustard-rt/mustard-cli/pnpm — Command: `node -e "const c=require('fs').readFileSync('apps/cli/templates/CLAUDE.md','utf8');const hits=['cargo build','cargo test','mustard-rt run','mustard-cli','pnpm --filter'].filter(s=>c.includes(s));process.exit(hits.length===0?0:1)"`
- [ ] AC-W2-2: `templates/pipeline-config.md` sem Flutter/Dart nem `| mobile |` — Command: `node -e "const c=require('fs').readFileSync('apps/cli/templates/pipeline-config.md','utf8');process.exit(c.includes('Flutter/Dart')||/\\|\\s*mobile\\s*\\|/.test(c)?1:0)"`
- [ ] AC-W2-3: `templates/pipeline-config.md` sem placeholders `{backend}/{frontend}/{admin}` — Command: `node -e "const c=require('fs').readFileSync('apps/cli/templates/pipeline-config.md','utf8');const bad=['{backend}','{frontend}','{admin}'].filter(s=>c.includes(s));process.exit(bad.length===0?0:1)"`

## Limites

- REWRITE: `templates/CLAUDE.md`
- MODIFY: `templates/pipeline-config.md`
- FORA: `.claude/CLAUDE.md`, `templates/refs/`, `templates/commands/`
