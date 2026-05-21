# Enhancement: git-skill-hardening
### Status: completed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-04-17T00:00:00Z

## Summary
Endurecer `commands/mustard/git/SKILL.md` com auto-stash universal + sentinel, tratamento de paths ephemeral (Claude/RTK), retries em race-condition de checkout, preservação de stashes pré-existentes, `--scope` explícito em `commit`, output compacto em ff-merge, sub-fluxo para ephemerals já tracked, resumo final categorizado, e banimento explícito de operações destrutivas.

## Boundaries
- `templates/commands/mustard/git/SKILL.md` — arquivo-fonte (instalado via `mustard init`)
- `.claude/commands/mustard/git/SKILL.md` — cópia local em sincronia com template

## Checklist
### General Agent
- [x] Adicionar seção **"Ephemeral Paths (Claude/RTK)"** listando os 5 paths conhecidos e política de exclusão via `git rev-parse --git-path info/exclude` (safe em submódulos)
- [x] Adicionar seção **"Auto-stash Protocol"** com formato de sentinel `mustard-git-autostash-<action>-<ts>`, retry até 3x em race `would be overwritten`, e pop seguro por índice via `grep -F` do sentinel exato
- [x] Adicionar seção **"Forbidden Operations"** banindo `rm -f`, `rm -rf`, `git clean -fd`, `git checkout -f`, `git reset --hard`; listar alternativas reversíveis (`git rm --cached`, `info/exclude`, stash)
- [x] Adicionar seção **"Ephemeral Tracked Sub-flow"** (executada ANTES de qualquer `commit --scope=all`): detect via `git ls-files`, append em `info/exclude`, `git rm --cached`, commit dedicado `chore: ignore ephemeral runtime state`
- [x] Reescrever action **`commit`**: adicionar parâmetro `--scope=all|staged|<path-pattern>` (default `all`); quando omitido, mostrar preview `git status --short` categorizado (ephemeral/código/untracked) e perguntar UMA vez via `AskUserQuestion`, memorizando para actions subsequentes da sessão
- [x] Reescrever action **`sync`**: substituir comando pelo novo template auto-stash universal com sentinel e retry-up-to-3
- [x] Reescrever action **`merge`** (feature → dev): aplicar auto-stash universal (estava ausente), usar `--ff-only -q` + `git --no-pager diff --stat HEAD@{1} HEAD | tail -3` para output compacto
- [x] Reescrever action **`merge main`**: substituir sentinel genérico `mustard-git-autostash` pelo formato único com `<action>-<ts>`, pop por índice específico (nunca `stash@{0}` implícito), aplicar `--ff-only -q` + stat compacto
- [x] Adicionar seção **"Final Status Report"** obrigatória ao fim de todas actions de escrita: `git status --short` por repo (parent + submódulos) categorizado em (a) ephemeral ignorado, (b) código real pendente, (c) untracked novo
- [x] Sincronizar `.claude/commands/mustard/git/SKILL.md` com `templates/commands/mustard/git/SKILL.md` (cópias idênticas)
- [x] Build/type-check: n/a (doc-only change); validar via `node --check` N/A; conferir via leitura que não há placeholders faltantes

## Files (~2)
- `templates/commands/mustard/git/SKILL.md` (modify — reescrita de seções + novas seções)
- `.claude/commands/mustard/git/SKILL.md` (modify — espelho do template)
