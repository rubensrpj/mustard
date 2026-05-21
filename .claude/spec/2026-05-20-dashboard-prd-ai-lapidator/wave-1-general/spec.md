# wave-1-general: slash command /mustard:prd

### Parent: [[2026-05-20-dashboard-prd-ai-lapidator]]
### Status: draft
### Phase: PLAN
### Scope: full
### Lang: pt
### Checkpoint: 2026-05-20T00:00:00Z

## PRD

## Contexto

O Mustard tem hoje 18 slash commands em `templates/commands/mustard/` (feature, bugfix, approve, qa, close, etc.). Falta um command leve que estruture uma intenção livre em PRD preenchido — hoje quem quer um PRD bom precisa rodar `/mustard:feature` completo, que dispara análise profunda, registry sync, layer detection e leitura de arquivos. Essa fricção desencoraja iterar ideia barato antes de comprometer com pipeline.

Esta wave cria `/mustard:prd <intent>`, um command **leve** sem análise de código. Recebe a intenção em texto livre, consulta `entity-registry.json` para entidades mencionadas no texto, faz Glob de paths comuns do projeto (sem ler arquivos), **infere o escopo** (light vs full) baseado no número de entidades tocadas e tamanho da intenção, **pré-marca entidades** afetadas, e devolve **JSON estruturado** no shape exato do `PrdForm` do dashboard. O command NÃO cria spec no disco, NÃO faz `Task(Explore)`, NÃO opina sobre se a ideia faz sentido — só estrutura.

A saída é JSON pra facilitar parse pelo Tauri command da Wave 2; humanos podem invocar direto via `claude -p "/mustard:prd intenção" --output-format json` e copiar o resultado.

## Métrica de sucesso

`claude -p "/mustard:prd add login refresh token" --output-format json` retorna JSON válido com todos os campos do `PrdForm` preenchidos (incluindo `scope` inferido e `entitiesFound` pré-marcadas) em ≤10s, sem leitura de código além do `entity-registry.json`.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: arquivos SKILL.md existem nos dois paths — Command: `node -e "const fs=require('fs');if(!fs.existsSync('apps/cli/templates/commands/mustard/prd/SKILL.md')||!fs.existsSync('.claude/commands/mustard/prd/SKILL.md'))process.exit(1)"`
- [ ] AC-2: frontmatter YAML válido na fonte canônica — Command: `node -e "const f=require('fs').readFileSync('apps/cli/templates/commands/mustard/prd/SKILL.md','utf8');if(!f.startsWith('---')||!f.includes('name:')||!f.includes('description:'))process.exit(1)"`
- [ ] AC-3: documento descreve trigger, ação, output JSON, inferência de escopo, pré-marcação de entidades — Command: `node -e "const f=require('fs').readFileSync('apps/cli/templates/commands/mustard/prd/SKILL.md','utf8');['## Trigger','## Action','PrdForm','entity-registry','--output-format json','scope','entitiesFound'].forEach(s=>{if(!f.includes(s)){console.error('missing:',s);process.exit(1)}})"`
- [ ] AC-4: cópia em .claude/ é byte-idêntica à fonte template — Command: `node -e "const fs=require('fs');const a=fs.readFileSync('apps/cli/templates/commands/mustard/prd/SKILL.md');const b=fs.readFileSync('.claude/commands/mustard/prd/SKILL.md');if(!a.equals(b))process.exit(1)"`

## Plano

## Arquivos

- `apps/cli/templates/commands/mustard/prd/SKILL.md` (novo, ~200 linhas) — fonte canônica do command
- `.claude/commands/mustard/prd/SKILL.md` (novo, cópia byte-idêntica) — dispatch real no repo Mustard

## Tarefas

### general Agent (Wave 1)

- [ ] Criar diretório `apps/cli/templates/commands/mustard/prd/`
- [ ] Escrever `SKILL.md` com frontmatter YAML (`name: mustard:prd`, `description:`, etc.) seguindo padrão dos commands existentes (ex.: `apps/cli/templates/commands/mustard/feature/SKILL.md` como referência de estrutura)
- [ ] Documentar trigger: `/mustard:prd <intent>` — argumento é texto livre, vai pro `$ARGUMENTS`
- [ ] Documentar ação em passos numerados:
  1. Receber intent como `$ARGUMENTS`
  2. Grep `entity-registry.json` (NÃO Read full) procurando entidades PascalCase mencionadas no intent — pré-marcar como `_confront.entitiesFound`
  3. Glob de paths comuns do projeto (`src/**/*.{ts,tsx,rs}`, `apps/**/*`) — apenas verificar existência, NÃO Read; popular `_confront.pathsExist`
  4. **Inferir escopo** via heurística: `entitiesFound.length >= 2 OR intent.split(' ').length >= 15 OR intent matches /CRUD|migration|workflow/i` → `scope: 'full'`, caso contrário → `scope: 'light'`. Output sempre inclui o valor inferido; user pode override no dashboard.
  5. Montar JSON estruturado no shape do `PrdForm` (campos: type, slug, title, scope, summary, why, layers{}, boundaries[], checklist[], acceptanceCriteria[{title,command}], decisionsNotObvious[], nonGoals[], _confront{entitiesFound[], entitiesMissing[], pathsExist[], pathsMissing[]})
  6. **Pre-popular boundaries** com paths sugeridos: pra cada `entitiesFound`, derivar paths típicos do projeto via Glob (ex.: entity `User` → `src/**/user*.{ts,tsx,rs}`); pra cada layer marcada, sugerir 1-2 paths convencionais
  7. **Slug auto** via `slugify(title)` — não exigir input
  8. Output em JSON puro no stdout (sem markdown wrapper) para facilitar parse via `--output-format json`
- [ ] Documentar restrições HARD: NÃO Task(Explore), NÃO Read arquivos de código, NÃO opinar sobre viabilidade da ideia, NÃO usar Bash além de `mustard-rt run` se necessário
- [ ] Documentar exemplo de invocação: `claude -p "/mustard:prd add refresh token to login" --output-format json --model claude-sonnet-4-6`
- [ ] Documentar shape exato do JSON output (referenciando interface `LapidatedPrd` da Wave 3)
- [ ] Copiar arquivo gerado para `.claude/commands/mustard/prd/SKILL.md` (cópia byte-idêntica)

## Limites

- `apps/cli/templates/commands/mustard/prd/`
- `.claude/commands/mustard/prd/`

Esta wave NÃO toca código de outros subprojects. Edits fora desses paths são erro.

## Dependências

Nenhuma.
