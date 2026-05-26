# Tactical Fix: Refresh stale .claude/ installs apos edicoes em apps/cli/templates/

### Stage: Analyze
### Outcome: Active
### Flags: 
### Scope: light
### Lang: pt-BR
### Checkpoint: 2026-05-26T16:21:15.524Z
### Parent: 2026-05-26-template-agnostic-audit

## Contexto

Tactical fix derivado de [[2026-05-26-template-agnostic-audit]].

Durante o fix-loop da spec parent, descobrimos um padrão recorrente: o próprio repo do Mustard mantém em `.claude/` cópias instaladas dos arquivos de `apps/cli/templates/`. Quando uma wave edita o source em `templates/`, as cópias em `.claude/` ficam stale e disparam falsos positivos em ferramentas como `language-audit` (que escaneia `.claude/refs/` por design — spec parent linha 149). Memória [[feedback_mustard_self_scripts_stale]] já documenta o padrão para `.claude/scripts/`; este fix generaliza para `.claude/refs/`, `.claude/commands/mustard/`, e `.claude/skills/`.

Hoje a única forma de refrescar é (a) rodar `mustard update --force` no próprio repo (efeito colateral: regenera muitos outros arquivos), ou (b) fazer `Copy-Item` manual (bloqueado por auto-mode quando o LLM tenta self-modify). Falta um subcomando dedicado `mustard-rt run refresh-claude` que:

1. Para cada subdir relevante (`refs/`, `commands/mustard/`, `skills/`), compare conteúdo em `apps/cli/templates/<sub>/**` contra `.claude/<sub>/**`.
2. Copie arquivos que diferem **apenas** quando o source é mais novo (mtime) E o destino é cópia limpa (sem edição local detectável via hash).
3. Reporte em JSON: `copied[]`, `skipped[]`, `conflicts[]` (destino com edição local + source diferente — pula com warning).
4. Idempotente: rodar 2x seguidos = `copied: []` na segunda.

## Métrica de sucesso

Após a próxima edição em `apps/cli/templates/refs/` (ou `commands/mustard/`), rodar `mustard-rt run refresh-claude` resolve a discrepância sem efeito colateral em outros arquivos. `language-audit` não dispara falso positivo em `.claude/` cópia stale. Comando ganha permissão durável via `.claude/settings.json` (não requer aprovação a cada uso, já que é seguro por design).

## Critérios de Aceitação

- [ ] AC-1: `mustard-rt run refresh-claude --help` documenta o subcomando — Command: `bash -c 'cargo run -q -p mustard-rt -- run refresh-claude --help | grep -q "Refresh stale"'`
- [ ] AC-2: Rodar sem mudanças no source = `copied: []` no JSON — Command: `bash -c 'cargo run -q -p mustard-rt -- run refresh-claude --format json | node -e "let s=\"\";process.stdin.on(\"data\",c=>s+=c).on(\"end\",()=>process.exit(JSON.parse(s).copied.length===0?0:1))"'`
- [ ] AC-3: Tocar `apps/cli/templates/refs/spec/resume-flow.md` (touch) → rodar `refresh-claude` → `.claude/refs/spec/resume-flow.md` tem o conteúdo do source — Command: `node -e "require('fs').utimesSync('apps/cli/templates/refs/spec/resume-flow.md',new Date(),new Date());require('child_process').execSync('cargo run -q -p mustard-rt -- run refresh-claude');const src=require('fs').readFileSync('apps/cli/templates/refs/spec/resume-flow.md','utf8');const dst=require('fs').readFileSync('.claude/refs/spec/resume-flow.md','utf8');process.exit(src===dst?0:1)"`
- [ ] AC-4: Testes verdes — Command: `cargo test -p mustard-rt refresh_claude`

## Arquivos

- `apps/rt/src/run/refresh_claude.rs` — novo subcomando: walk + diff + copy + JSON output
- `apps/rt/src/run/mod.rs` — registrar `refresh-claude` no dispatch
- `apps/rt/src/run/refresh_claude/tests.rs` (ou inline) — fixtures: source novo + destino stale; source novo + destino com edição local; idempotência
- `.claude/settings.json` — permission durável para `mustard-rt run refresh-claude`
