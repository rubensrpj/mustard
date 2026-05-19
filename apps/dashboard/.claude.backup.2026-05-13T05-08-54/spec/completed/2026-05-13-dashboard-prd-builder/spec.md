# Feature: dashboard-prd-builder

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T19:00:00Z
### Lang: pt

## Contexto

O dashboard legado tinha uma aba "PRD" que servia como gerador interativo de specs no formato Mustard: um formulário layered (tipo, escopo, layers, boundaries, checklist, AC) que rendava em tempo real para um markdown copiável já com o prefixo `/mustard:feature` ou `/mustard:bugfix`. Esse builder reduzia drasticamente o atrito de escrever specs corretamente — devs juniores, PMs e não-devs conseguiam produzir uma spec aderente à convenção Mustard sem decorar o formato (Status/Phase header, ## Summary, ## Boundaries, ## Checklist, ## Acceptance Criteria etc.). O dashboard Tauri atual não tem essa ferramenta, então qualquer spec hoje nasce em editor de texto cru, com altíssima chance de erro de header (qa-run depende de literal `## Acceptance Criteria`, por exemplo) ou de pular seções que o pipeline assume. Sem o builder, o catálogo `/commands` (Wave D) cumpre só metade do onboarding — vê-se o que existe, mas não se pratica.

## Resumo

Portar a aba "PRD" do dashboard legado para uma rota `/prd` no Tauri. Função pura `generatePrdMarkdown` em `src/lib/prd-template.ts`, página com formulário (esquerda) + preview Markdown live (direita, debounced 300ms), persistência de draft em localStorage, dois botões de copiar (markdown puro vs com prefixo `/mustard:{type} {slug}`), toast de confirmação via `sonner`. Reutiliza `src/components/Markdown.tsx` existente para o preview.

## Limites

Arquivos intencionalmente tocados:

- `src/lib/prd-template.ts` (NOVO)
- `src/pages/Prd.tsx` (NOVO)
- `src/App.tsx` (rota + montagem `<Toaster />`)
- `src/components/layout/Sidebar.tsx` (NavLink novo)
- `package.json` + `pnpm-lock.yaml` (instalar `sonner`)

Fora do escopo: qualquer mudança em `src-tauri/`, salvar PRD direto no filesystem (só clipboard), ENV editor (Wave F), edição inline de specs já existentes, importar PRD de markdown.

## Arquivos (~5)

| Arquivo | Operação | Notas |
|---------|----------|-------|
| `src/lib/prd-template.ts` | criar | Função pura `generatePrdMarkdown(input)` + `slugify(text)`, validação throw em campos obrigatórios |
| `src/pages/Prd.tsx` | criar | Form 2 colunas com preview live, 2 botões copy, draft localStorage, layer checkboxes prepopulam boundaries/checklist |
| `src/App.tsx` | modificar | Importar `Prd`, registrar `<Route path="/prd">`, importar e montar `<Toaster />` da sonner |
| `src/components/layout/Sidebar.tsx` | modificar | Importar `FileText` (lucide), NavLink "PRD" |
| `package.json` | modificar | Adicionar `sonner` via `pnpm add sonner` |

## Component Contract — Prd page

- **Props**: nenhuma (page top-level).
- **State interno** (`useState`):
  - `form: PrdForm` — objeto com todos os campos (`type: 'feature'|'bugfix'`, `slug`, `title`, `scope: 'light'|'full'`, `projectId: string|null`, `summary`, `why`, `layers: { backend, frontend, database, design, docs, testes: boolean }`, `boundaries: string`, `checklist: string`, `acceptanceCriteria: { title: string, command: string }[]`, `decisionsNotObvious: string`, `nonGoals: string`)
  - `errors: Record<string, string>` — erros de validação por campo
- **Derivado** (`useMemo`):
  - `slugDerived` — `form.slug.trim() || slugify(form.title)`
  - `markdownPreview` — chama `generatePrdMarkdown({ ...form, slug: slugDerived })` em try/catch; se erro, mostra `(preencha os campos obrigatórios)`
- **Persistência**: `useEffect` debounced 500ms grava `form` em `localStorage['mustard-prd-draft']` (JSON). `useEffect` no mount lê e reidrata. Botão "Limpar" reseta para defaults E remove a key do localStorage.
- **Copiar**:
  - "Copy markdown" → `navigator.clipboard.writeText(markdownPreview)` + `toast.success('Copiado!')`
  - "Copy with /mustard:{type}" → adiciona `/mustard:${form.type} ${slugDerived}\n\n` ao topo + clipboard + toast
- **Validação no copy**: se `summary` ou `boundaries` ou `checklist` vazios, NÃO copia, seta `errors`, `toast.error('Preencha os campos obrigatórios')` e marca os campos com borda `border-destructive`.
- **Acessibilidade**: `<label htmlFor>` em todos os inputs; `<fieldset>` para grupos relacionados; AC list usa `<button>` para add/remove com `aria-label`.

## Tarefas

### UI Agent (Wave 1)

- [ ] Instalar `sonner` via `pnpm add sonner` da raiz do projeto. Confirmar `package.json` e `pnpm-lock.yaml` atualizados.

- [ ] Criar `src/lib/prd-template.ts`:
  - Exportar `interface PrdInput { type: 'feature'|'bugfix'; slug: string; title: string; summary: string; why?: string; scope: 'light'|'full'; boundaries: string[]; checklist: string[]; acceptanceCriteria: { title: string; command: string }[]; decisionsNotObvious?: string[]; nonGoals?: string[]; project?: string; }`.
  - Exportar `slugify(text: string): string` — lowercase, trim, replace `[^a-z0-9-]` com `-`, collapse múltiplos `-`, trim `-` das pontas.
  - Exportar `generatePrdMarkdown(input: PrdInput): string` — gera string contendo (em ordem): `# {Feature|Bugfix}: {slug}`, blank line, `### Status: draft | Phase: PLAN | Scope: {scope}`, `### Checkpoint: {now ISO}`, blank line, `## Summary` + body, `## Boundaries` + bullet list, `## Checklist` + bullet list (cada item como `- [ ] {item}`), `## Acceptance Criteria` (header literal EN, NÃO traduzir — pipeline depende disso) + cada AC como `- [x] AC-{n}: {title} — Command: \`{command}\``, opcionalmente `## Decisões não-óbvias` se array não vazio, opcionalmente `## Non-Goals` se array não vazio. Se `why` presente: insere `## Por quê?` entre Summary e Boundaries.
  - Validação: `throw new Error('campo obrigatório: {field}')` se `slug`, `title`, `summary`, `boundaries.length === 0` ou `checklist.length === 0`. ACs vazias permitidas (warns aceitos depois pelo qa-run).

- [ ] Criar `src/pages/Prd.tsx`:
  - Estrutura raiz: `<div className="flex flex-col gap-4">` com (a) breadcrumb `Mustard / PRD`, (b) header `<h1 className="text-base font-medium">PRD Builder</h1>` + subtítulo `<p className="text-[13px] text-muted-foreground">Gere specs no formato Mustard.</p>`, (c) grid 2 colunas `grid grid-cols-1 lg:grid-cols-2 gap-4`.
  - **Coluna esquerda (Form)** — densidade Linear, todos com `<label className="text-[13px] font-medium">` + input `bg-card border border-border rounded text-sm px-2 py-1.5 focus:border-primary outline-none`:
    - Radio `type`: Feature / Bugfix (estilo segmented).
    - `<input>` slug — placeholder mostra `slugDerived` quando vazio.
    - `<input>` title.
    - Radio `scope`: Light / Full.
    - `<select>` project — opções vindas de `useQuery(['discover', projectsRoot])` igual Sidebar.
    - `<textarea rows={3}>` summary.
    - `<textarea rows={3}>` why (opcional).
    - Layer checkboxes em `<fieldset>`: Backend, Frontend, Database, Design, Docs, Testes. Quando marcadas, prepopular boundaries/checklist com sugestões padrão (ex: marcar "Backend" adiciona `Endpoints in api/...` ao boundaries e `Add endpoint` ao checklist se ainda não existem). NÃO entram no markdown final — só auxiliam.
    - `<textarea rows={3}>` boundaries (multi-line, 1 item por linha — split em `\n`).
    - `<textarea rows={4}>` checklist (mesmo).
    - `acceptanceCriteria` — lista dinâmica: cada row tem `<input>` title curto e `<textarea rows={2} className="font-mono text-xs">` command. Botão `+ Adicionar AC` no fim. Botão `x` lucide `X` por row remove.
    - `<textarea rows={2}>` decisionsNotObvious (opcional, multi-line).
    - `<textarea rows={2}>` nonGoals (opcional, multi-line).
  - **Coluna direita (Preview)**:
    - Header `<div className="text-[11px] uppercase tracking-wider text-muted-foreground">Preview</div>`
    - Container `<div className="border border-border rounded p-4 bg-card overflow-y-auto" style={{ maxHeight: 'calc(100vh - 200px)' }}>` com `<Markdown content={markdownPreview} />`.
    - Update via `useDeferredValue(form)` ou `setTimeout 300ms` (escolher o mais simples — `useDeferredValue` é nativo do React 18+).
  - **Bottom actions** (full width abaixo da grid): `<div className="flex items-center gap-2 pt-2 border-t border-border">` com:
    - Botão primary "Copiar markdown" (`bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm`).
    - Botão primary "Copiar com /mustard:{type}" — texto muda dinamicamente ('feature' | 'bugfix').
    - Botão secondary "Limpar" (`text-muted-foreground hover:text-foreground px-3 py-1.5 rounded text-sm border border-border`) à direita (ml-auto).
  - **Hooks**:
    - `useEffect` montagem: `JSON.parse(localStorage.getItem('mustard-prd-draft') ?? 'null')` e setForm se válido; defensivo (try/catch).
    - `useEffect` em form (debounced 500ms): grava em localStorage.
    - Use `toast` da `sonner` para feedback.

- [ ] Modificar `src/App.tsx`:
  - Adicionar `import { Prd } from "@/pages/Prd";`.
  - Adicionar `import { Toaster } from "sonner";` no topo.
  - Adicionar `<Route path="/prd" element={<Prd />} />` entre `/commands` e `/telemetry`.
  - Montar `<Toaster position="bottom-right" theme="dark" richColors />` dentro do `<HashRouter>` (ao lado de `<CommandPalette />`). Tema: usar `theme={useTheme().theme}` se prático; senão fixo "dark" combina com a app default.

- [ ] Modificar `src/components/layout/Sidebar.tsx`:
  - Adicionar `FileText` ao import de `lucide-react`.
  - Inserir `<NavLink to="/prd" className={navItemClass}><FileText className="h-3.5 w-3.5" /> PRD</NavLink>` entre o NavLink de `Comandos` e o de `Activity` (ordem final: Home → Knowledge → Comandos → PRD → Activity → Telemetry).

### Build & Type-check (Wave 2)

- [ ] Da raiz do projeto: `npx tsc --noEmit` deve passar (exit 0).
- [ ] Da raiz do projeto: `npm run build` deve passar (exit 0).

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript compila sem erros — Command: `npx tsc --noEmit`
- [x] AC-2: `src/lib/prd-template.ts` existe e exporta `generatePrdMarkdown` + `slugify` — Command: `node -e "const t=require('fs').readFileSync('src/lib/prd-template.ts','utf8');process.exit(t.includes('export')&&t.includes('generatePrdMarkdown')&&t.includes('slugify')?0:1)"`
- [x] AC-3: `src/pages/Prd.tsx` existe com export — Command: `node -e "const t=require('fs').readFileSync('src/pages/Prd.tsx','utf8');process.exit(t.includes('export')&&t.includes('Prd')?0:1)"`
- [x] AC-4: Rota `/prd` registrada e Toaster montada em App.tsx — Command: `node -e "const t=require('fs').readFileSync('src/App.tsx','utf8');process.exit(t.includes('/prd')&&t.includes('Toaster')&&t.includes('sonner')?0:1)"`
- [x] AC-5: Sidebar tem NavLink PRD com FileText — Command: `node -e "const t=require('fs').readFileSync('src/components/layout/Sidebar.tsx','utf8');process.exit(t.includes('FileText')&&t.includes('/prd')?0:1)"`
- [x] AC-6: `sonner` está em package.json — Command: `node -e "const p=require('./package.json');const all={...p.dependencies,...p.devDependencies};process.exit(all.sonner?0:1)"`
- [x] AC-7: `generatePrdMarkdown` produz markdown válido com header AC literal EN — Command: `node -e "const ts=require('fs').readFileSync('src/lib/prd-template.ts','utf8');process.exit(ts.includes('## Acceptance Criteria')?0:1)"`
- [x] AC-8: Build Vite passa — Command: `npm run build`

## Não-Objetivos

- ENV editor (planejado para Wave F em spec separada `dashboard-env-editor`)
- Salvar PRD direto no filesystem (só clipboard nesta entrega)
- Importar PRD existente de markdown
- Editor visual de spec já criado/aprovado
- Validação semântica avançada (ex: dependency check entre boundaries e files) — só campos obrigatórios

## Decisões não-óbvias

- **Header `## Acceptance Criteria` é literal EN no markdown gerado**, mesmo que o resto da spec seja PT — `qa-run.js` faz match literal por essa string (descoberto na Wave D quando o QA deu SKIP com header PT). Memória do projeto registra: "qa-run só matcheia `## Acceptance Criteria` literal".
- **Reutiliza `src/components/Markdown.tsx`** existente — já tem renderer completo (code blocks, GFM, tabelas, checkbox), evita reimplementar.
- **`useDeferredValue` em vez de `setTimeout`** para o debounce do preview — nativo do React 18+, sem leak de timer; `setTimeout` 300ms só pra localStorage write (não precisa cancelar entre re-renders).
- **`sonner` é a opção shadcn-friendly** (~16KB gz) e o user pediu explicitamente. Toaster montada no App.tsx (não dentro da Prd) pra ser global se outras páginas precisarem.
- **Layer checkboxes só auxiliam, NÃO entram no markdown** — geram sugestões pra boundaries/checklist via "merge não-destrutivo" (só adiciona se não existir já naquela linha). Mantém o markdown limpo e a UX guiada.
- **Persistência localStorage debounced 500ms** evita escrita em cada keystroke; key `mustard-prd-draft` (singular, sem ID — só 1 draft por vez é o esperado para iteração).
