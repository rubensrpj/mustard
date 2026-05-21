# Enhancement: git-flow-simplify
### Status: completed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-04-01T00:00:00Z

## Summary
Simplificar git flow: remover mapeamento de branch pessoal do `init`, bloquear commits direto em main/dev, wildcard `*→dev` como padrão, merge sempre para dev com sync obrigatório antes.

## Checklist
### templates-impl Agent
- [x] `src/commands/init.ts` — remover pergunta "Personal branch pattern", gerar flow com `"*": "dev"` e `"dev": "main"` automaticamente
- [x] `templates/commands/mustard/git/SKILL.md` — adicionar proteção de branches (main/dev), remover cascade merge automático, sync obrigatório antes de merge, `/git merge` sempre vai pra dev
- [x] `.claude/commands/mustard/git/SKILL.md` — espelhar mudanças do template
- [x] Build/type-check

## Files (~3)
- `src/commands/init.ts` (modify)
- `templates/commands/mustard/git/SKILL.md` (modify)
- `.claude/commands/mustard/git/SKILL.md` (modify)

## Changes Detail

### 1. `src/commands/init.ts`
- Remover pergunta `devPattern` ("Personal branch pattern")
- No modo interativo: perguntar só `production` e `devBranch`
- Gerar flow: `{ "*": "<devBranch>", "<devBranch>": "<production>" }` (wildcard pega qualquer branch)
- No modo `--yes`: mesmo comportamento automático

### 2. `git/SKILL.md` (template + .claude)
- **Branch protection**: recusar commit/push/merge se branch atual é main ou dev
- **Merge simplificado**: `/git merge` sempre faz merge para o parent (dev), sem cascade
- **`/git merge main`** continua explícito: promove dev → main (único caso permitido em main)
- **Sync obrigatório**: antes de qualquer merge, executar sync (rebase do dev)
- Remover conceito de cascade merge (dev_rubens → dev → main automático)
- Remover referências a `dev_*` pattern
