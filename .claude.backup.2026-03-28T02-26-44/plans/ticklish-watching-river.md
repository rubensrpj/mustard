# Plano: PR-Only + `/mustard:review`

## Context

O Mustard já suporta PR via flag `pr.enabled` no `mustard.json`, mas também permite merge direto. O usuário quer padronizar em PR-only e integrar o skill nativo `code-review` da Claude para revisar PRs. Isso simplifica o fluxo, adiciona audit trail, e habilita review automatizado de PRs.

## Mudanças

### 1. Simplificar `mustard.json` — remover `pr.enabled`, PR é sempre on

**Arquivo:** `mustard.json` (raiz do projeto)

```json
// DE:
{ "git": { "flow": {...}, "pr": { "enabled": false, "provider": "github" }, "submodules": false } }

// PARA:
{ "git": { "flow": {...}, "provider": "github", "submodules": false } }
```

### 2. Atualizar `MustardConfig` e `generateMustardJson()`

**Arquivo:** `src/commands/init.ts` (linhas 166-276)

- Interface `MustardConfig` (L166-172): trocar `pr: { enabled, provider }` por `provider: string`
- Auto-config (L205-211): `pr: { enabled: true, provider: 'github' }` → `provider: 'github'`
- Interactive prompts (L214-254):
  - Remover prompt `prEnabled` (L241-244)
  - Remover `when: (a) => a.prEnabled` do prompt `provider` (L252)
  - Provider sempre perguntado
- Config output (L264-270): `pr: { enabled: ..., provider: ... }` → `provider: answers.provider || 'github'`

### 3. Atualizar git SKILL.md — remover merge direto

**Arquivo:** `templates/commands/mustard/git/SKILL.md`

- L16: `"Promote current → parent (PR if enabled, direct merge if not)"` → `"Promote current → parent (creates PR)"`
- L23-37: Config example — remover `pr: { enabled, provider }`, usar `"provider": "github"`
- L168: Remover `"Behavior depends on pr.enabled"`
- L174-201: Manter Step 2a (PR mode) como único caminho
- **L203-217: REMOVER** Step 2b inteiro (direct merge)
- Adicionar após criação do PR: `> PR created: {url}. Run /review to review it.`
- L70-78 (Step 0): Ler `git.provider` com fallback para `git.pr.provider` (retrocompatibilidade)
- L235-237 (deploy cascade): Simplificar mensagem para "PR created" ao invés de "Merged/PR created"

### 4. Criar `/mustard:review`

**Novo arquivo:** `templates/commands/mustard/review/SKILL.md`

Estrutura:
- Trigger: `/review [pr-number-or-url]`
- Sem argumento: auto-detecta PR da branch atual via `gh pr view --json number,url`
- Com argumento numérico: usa como PR number
- Com URL: extrai número
- Invoca `Skill({ skill: "code-review", args: "<pr>" })`
- Erro se nenhum PR encontrado: sugerir `/git merge` primeiro
- Fallback se skill não disponível: rodar `/task review` local

### 5. Atualizar documentação

**Arquivo:** `CLAUDE.md` (raiz)

- Seção Git (L157-163): Atualizar descrição do merge
- Adicionar entrada: `/mustard:review [number|url]` — Review PR via Claude code-review

## Retrocompatibilidade

Projetos com `mustard.json` antigo (`pr.enabled`):
- git SKILL.md lê `git.provider` com fallback para `git.pr.provider`
- `pr.enabled` é simplesmente ignorado (PR é sempre on)
- `mustard update` regenera SKILL.md mas preserva `mustard.json` do usuário

## Arquivos a Modificar

| Arquivo | Ação |
|---------|------|
| `src/commands/init.ts` | Modificar interface + prompts |
| `templates/commands/mustard/git/SKILL.md` | Remover merge direto, adicionar sugestão review |
| `templates/commands/mustard/review/SKILL.md` | **Criar** novo comando |
| `mustard.json` | Atualizar para novo schema |
| `CLAUDE.md` | Atualizar docs |

## Verificação

1. `npm run build` — compilar sem erros
2. `npm test` — testes passam
3. Testar `mustard init` em diretório temporário — verificar novo schema do `mustard.json`
4. Verificar que o SKILL.md do git não tem referências a merge direto
5. Verificar que `review/SKILL.md` existe em templates
