# wave-3-ui: lapidador + redesign da página PRD Builder

### Parent: [[2026-05-20-dashboard-prd-ai-lapidator]]
### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full
### Lang: pt
### Checkpoint: 2026-05-20T00:00:00Z

## PRD

## Contexto

A página `apps/dashboard/src/pages/Prd.tsx` (476 linhas) hoje é um form completo de 10+ campos onde o usuário preenche TUDO manualmente — incluindo dados que o sistema já conhece (entidades do `entity-registry.json`, paths do projeto, slug derivável do título). Boundaries e checklist são textareas livres com uma linha por item, AC já é lista estruturada com card+input. Layout é form 2-coluna com preview lateral.

Esta wave faz **duas mudanças em conjunto** (Wave 3 expandida conforme decisão de PLAN):

1. **Integração do lapidador IA** — textarea "Intenção livre" no topo da página, botão "Lapidar com IA" com spinner em background invisível, handler que distribui o JSON da Wave 2 nos campos do form e mostra banner de confronto.

2. **Redesign incremental + redução de digitação manual** — slug some (auto-derivado do título), escopo vira `auto` por default (IA decide, com toggle de override exposto após lapidate), entity picker novo com **multi-seleção via checkboxes filtráveis** carregado do `entity-registry.json`, boundaries e checklist viram **listas editáveis com botão "+ Adicionar"** (mesmo padrão do AC atual, em vez de textarea), pre-populate de paths sugeridos quando user seleciona projeto, redesign visual com hero section pra intenção, layout reorganizado em grupos colapsáveis (`CollapsibleGroup` já existe em `components/page/`).

Resultado: usuário típico escreve 1-3 linhas na intenção, clica Lapidar, revisa entidades pré-marcadas + boundaries + AC, ajusta o que faltar (raramente vai precisar criar do zero), copia. Form 100% manual continua disponível pra quem quiser ignorar o lapidador.

## Métrica de sucesso

Usuário escreve "adicionar refresh token no login" na textarea, clica "Lapidar com IA", **sem janela de terminal abrindo no Windows**, em ≤30s vê: escopo inferido pelo IA, entidades pré-marcadas no picker, boundaries sugeridos como linhas editáveis, AC sugeridas como cards, banner discreto com `entitiesMissing/pathsMissing`. Se `claude` não estiver no PATH, botão fica desabilitado com tooltip "Claude CLI não encontrado — instale via claude.ai/cli".

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc -b`
- [ ] AC-2: build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-3: arquivo Prd.tsx referencia lapidatePrd e checkClaudeAvailable — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src/pages/Prd.tsx','utf8');['lapidatePrd','checkClaudeAvailable'].forEach(s=>{if(!f.includes(s)){console.error('missing:',s);process.exit(1)}})"`
- [ ] AC-4: wrapper API existe e expõe lapidatePrd + checkClaudeAvailable — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src/api/prd.ts','utf8');['export','lapidatePrd','checkClaudeAvailable','invoke'].forEach(s=>{if(!f.includes(s)){console.error('missing:',s);process.exit(1)}})"`
- [ ] AC-5: tipos compartilhados existem e exportam LapidatedPrd — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src/lib/types/prd.ts','utf8');if(!f.includes('LapidatedPrd')||!f.includes('export'))process.exit(1)"`
- [ ] AC-6: slug é derivado do título (sem campo input manual) — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src/pages/Prd.tsx','utf8');if(/id=['\"]prd-slug['\"]/.test(f)){console.error('slug input ainda existe');process.exit(1)}"`
- [ ] AC-7: EntityPicker importado e usado em Prd.tsx — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src/pages/Prd.tsx','utf8');if(!f.includes('EntityPicker'))process.exit(1)"`
- [ ] AC-8: lint passa — Command: `pnpm --filter mustard-dashboard lint`

## Plano

## Arquivos

- `apps/dashboard/src/lib/types/prd.ts` (novo, ~35 linhas) — `LapidatedPrd`, `PrdLayers`, `PrdAc`, `PrdConfront`
- `apps/dashboard/src/api/prd.ts` (novo, ~30 linhas) — wrappers `lapidatePrd`, `checkClaudeAvailable` via `invoke()`
- `apps/dashboard/src/components/prd/EntityPicker.tsx` (novo, ~80 linhas) — lista filtrável com checkboxes multi-select, badges das entidades escolhidas, busca por nome
- `apps/dashboard/src/components/prd/EditableList.tsx` (novo, ~60 linhas) — primitiva reutilizável: lista crescente de strings com input por linha + botão "+ Adicionar" + "x" remove (usada por boundaries e checklist)
- `apps/dashboard/src/components/prd/IntentHero.tsx` (novo, ~70 linhas) — hero section com textarea grande de intenção, botão Lapidar com IA, spinner, estado disponibilidade do `claude`, banner de erro/confront
- `apps/dashboard/src/hooks/useEntityRegistry.ts` (novo, ~40 linhas) — hook React Query que lê `entity-registry.json` do projeto ativo via Tauri command existente ou fs direto; expõe `entities: string[]`
- `apps/dashboard/src/pages/Prd.tsx` (edit, ~+150 / -80 linhas) — orquestra novos componentes, remove campo slug, troca toggle de escopo por "auto" default, adiciona pre-populate, layout reorganizado em `CollapsibleGroup`, integra handler do lapidate

## Component Contract

### IntentHero
- **Props:** `onLapidate: (intent: string) => Promise<void>`, `isLapidating: boolean`, `claudeAvailable: boolean | null`, `lapidateError: string | null`, `confront: PrdConfront | null`
- **Estados:** idle | checking-claude | claude-unavailable | ready | lapidating | success-with-confront | error
- **A11y:** `aria-busy={isLapidating}`; banner com `role="alert"`; tooltip no botão desabilitado
- **Microinteração:** spinner 200ms transition; sucesso = highlight verde sutil de 1s nos campos preenchidos

### EntityPicker
- **Props:** `entities: string[]` (do registry), `selected: string[]`, `onChange: (selected: string[]) => void`, `prePicked?: string[]` (entitiesFound do lapidate)
- **Estados:** loading | empty (registry vazio) | filled
- **Interação:** input de busca filtra lista em tempo real; checkbox por entidade; badges das selecionadas no topo com "x" remove; entidades pré-marcadas vêm com badge "sugerida" sutil
- **A11y:** input `aria-label="Buscar entidade"`; checkboxes nativos com labels

### EditableList
- **Props:** `items: string[]`, `onChange: (items: string[]) => void`, `placeholder?: string`, `inputLabel?: string`
- **Estados:** empty (1 input vazio) | filled
- **Interação:** uma linha por item com input + botão "x"; "+ Adicionar" no fim; Enter no input adiciona nova linha
- **A11y:** cada input com `aria-label` indexado; botões com aria-label remover

## Tarefas

### ui Agent (Wave 3)

- [ ] Criar `apps/dashboard/src/lib/types/prd.ts`:
  - `export interface LapidatedPrd { type: 'feature'|'bugfix'; slug: string; title: string; scope: 'light'|'full'; summary: string; why?: string; layers: PrdLayers; boundaries: string[]; checklist: string[]; acceptanceCriteria: PrdAc[]; decisionsNotObvious?: string[]; nonGoals?: string[]; _confront: PrdConfront }`
  - `export interface PrdLayers { backend: boolean; frontend: boolean; database: boolean; design: boolean; docs: boolean; testes: boolean }`
  - `export interface PrdAc { title: string; command: string }`
  - `export interface PrdConfront { entitiesFound: string[]; entitiesMissing: string[]; pathsExist: string[]; pathsMissing: string[] }`
- [ ] Criar `apps/dashboard/src/api/prd.ts`:
  - `export async function lapidatePrd(intent: string, projectPath: string): Promise<LapidatedPrd>` → `invoke('lapidate_prd', { intent, projectPath })`
  - `export async function checkClaudeAvailable(): Promise<boolean>` → `invoke('check_claude_available')`
- [ ] Criar `apps/dashboard/src/components/prd/EditableList.tsx`:
  - Reutilizar visual do bloco de AC atual (card-like com inputs + add/remove)
  - Hook simples: items, addItem(), removeItem(i), updateItem(i, value)
  - Enter no último input adiciona nova linha automaticamente
- [ ] Criar `apps/dashboard/src/components/prd/EntityPicker.tsx`:
  - `Input` de busca + filter local
  - Lista scrollável (`ScrollArea`) com checkbox por entidade
  - Badges no topo das selecionadas com "x" remove (`Badge` do shadcn)
  - Entidades em `prePicked` aparecem com indicador sutil "sugerida"
  - Empty state quando registry sem entidades
- [ ] Criar `apps/dashboard/src/hooks/useEntityRegistry.ts`:
  - `useQuery({ queryKey: ['entity-registry', projectPath], queryFn: ... })`
  - Lê via Tauri command existente que devolva o `entity-registry.json` parseado (ou cria um novo `read_entity_registry(project_path)` se faltar — verificar `apps/dashboard/src/lib/dashboard.ts` antes)
  - Devolve `string[]` de nomes de entidades
- [ ] Criar `apps/dashboard/src/components/prd/IntentHero.tsx`:
  - Textarea grande (rows=4) com placeholder "Descreva sua intenção em 1-3 linhas (ex: adicionar refresh token no fluxo de login)"
  - Botão "Lapidar com IA" + ícone `Wand2` (lucide); disabled rules conforme Component Contract
  - Spinner inline (`Loader2`) durante chamada
  - Tooltip explicativo quando desabilitado por `claudeAvailable === false`
  - Banner inline de erro + banner de confront (entitiesMissing/pathsMissing)
- [ ] Editar `apps/dashboard/src/pages/Prd.tsx`:
  - Remover campo Slug (linhas 237-247) — slug agora vem só de `slugify(title)` ou `slugify(form.title || lapidated.slug)`
  - Remover botões de escopo light/full (linhas 261-276) — escopo passa a ser `auto` por default; após lapidate, mostrar valor inferido com toggle de override
  - Adicionar imports: `IntentHero`, `EntityPicker`, `EditableList`, `useEntityRegistry`, `lapidatePrd`, `checkClaudeAvailable`, `LapidatedPrd`, `PrdConfront`, `CollapsibleGroup`
  - State novo: `intent`, `isLapidating`, `lapidateError`, `confront`, `claudeAvailable`, `selectedEntities: string[]`
  - `useEffect` que checa `claudeAvailable` na montagem
  - `useEntityRegistry(projectPath)` para alimentar EntityPicker
  - Handler `handleLapidate(intent)`: busca `projectPath` do projeto ativo, chama `lapidatePrd`, distribui via `setForm(prev => ({...prev, ...mapToForm(result)}))`, set `selectedEntities = result._confront.entitiesFound`, set `confront`
  - Reorganizar coluna form em `CollapsibleGroup`: "Intenção" (sempre aberto, IntentHero), "Identidade" (tipo+título+projeto), "Detalhes" (resumo+why+layers), "Escopo de Mudança" (EntityPicker + boundaries via EditableList), "Plano" (checklist via EditableList), "Critérios" (AC list existente), "Avançado" (decisões+não-goals)
  - Substituir textareas de boundaries e checklist por `EditableList`
  - Adicionar pre-populate: quando `form.projectId` muda, popular boundaries com paths sugeridos via Glob (rota Tauri `suggest_paths_for_project` se existir, ou heurística client-side)
- [ ] Verificar type-check: `pnpm --filter mustard-dashboard exec tsc -b`
- [ ] Verificar build: `pnpm --filter mustard-dashboard build`
- [ ] Verificar lint: `pnpm --filter mustard-dashboard lint`

## Limites

- `apps/dashboard/src/pages/Prd.tsx`
- `apps/dashboard/src/api/prd.ts`
- `apps/dashboard/src/lib/types/prd.ts`
- `apps/dashboard/src/components/prd/EntityPicker.tsx`
- `apps/dashboard/src/components/prd/EditableList.tsx`
- `apps/dashboard/src/components/prd/IntentHero.tsx`
- `apps/dashboard/src/hooks/useEntityRegistry.ts`

NÃO toca rotas/Sidebar/Topbar (página já mapeada), NÃO altera `prd-template.ts` (continua gerando markdown), NÃO mexe em outros componentes do dashboard.

## Dependências

- Wave 2 — `invoke('lapidate_prd')` e `invoke('check_claude_available')` precisam dos commands registrados.
