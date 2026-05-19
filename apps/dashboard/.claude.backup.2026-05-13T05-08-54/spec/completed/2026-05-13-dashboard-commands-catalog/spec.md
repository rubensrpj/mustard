# Feature: dashboard-commands-catalog

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T18:30:00Z
### Lang: pt

## Contexto

O dashboard legado tinha uma aba "Comandos" que funcionava como catálogo navegável de todos os slash-commands `/mustard:*` — principal vetor de onboarding para devs e não-devs entenderem qual comando usar quando. O dashboard Tauri atual expõe pipelines individuais (Specs, Activity, Knowledge, Telemetry) mas não consolida o contrato dos comandos em um lugar acessível dentro do app. Sem essa aba, usuários precisam abrir docs externas para descobrir o que `/mustard:feature` faz versus `/mustard:task`, ou quando aplicar `/mustard:bugfix` no lugar de `/mustard:feature`. Isso quebra o fluxo "tudo num lugar só" que era a proposta de valor do dashboard original e adiciona fricção significativa em onboarding interno e de novos colaboradores.

## Resumo

Portar a aba "Comandos" do dashboard legado para uma rota `/commands` no Tauri. Catálogo declarativo em TS local (`src/data/commands-catalog.ts`) com ≥20 entries, página densa estilo Linear com busca + filtros por categoria, cards expansíveis com explicação dual (simples vs técnico), e integração no Cmd+K para "saltar até" qualquer comando.

## Limites

Arquivos intencionalmente tocados:

- `src/data/commands-catalog.ts` (NOVO)
- `src/pages/Commands.tsx` (NOVO)
- `src/App.tsx` (rota nova)
- `src/components/layout/Sidebar.tsx` (NavLink novo)
- `src/components/CommandPalette.tsx` (Group novo)

Fora do escopo: qualquer mudança em `src-tauri/`, PRD Builder (Wave E em spec separada), ENV editor (Wave F em spec separada), license gating, live updates do catálogo (estático).

## Arquivos (~5)

| Arquivo | Operação | Notas |
|---------|----------|-------|
| `src/data/commands-catalog.ts` | criar | Interface `CommandEntry`, array `COMMANDS` (≥20), `CATEGORIES` |
| `src/pages/Commands.tsx` | criar | Página com search + filtros + cards expansíveis + anchor scroll |
| `src/App.tsx` | modificar | Importar `Commands`, registrar `<Route path="/commands">` |
| `src/components/layout/Sidebar.tsx` | modificar | Importar `Terminal` (lucide), NavLink "Comandos" entre Knowledge e Activity |
| `src/components/CommandPalette.tsx` | modificar | Importar `COMMANDS`, novo `Command.Group heading="Comandos"` com itens "Ver: /mustard:*" → navega `/commands#cmd-{slug}` |

## Component Contract — Commands page

- **Props**: nenhuma (é page top-level montada pelo router).
- **State interno**:
  - `query: string` (input de busca, useState com debounce 300ms via setTimeout, padrão `Knowledge.tsx`)
  - `selectedCategory: string | null` (chip ativo)
  - `expanded: Set<string>` (slugs dos cards abertos, padrão `Knowledge.tsx`)
- **Acessibilidade**: input com `placeholder` claro; cards expansíveis usam `<button type="button">` clicáveis (não `<div onClick>`); chips de categoria são `<button>` com estado visual ativo; cada card tem `id={`cmd-${slug}`}` para deep-linking.
- **Erro**: catálogo é estático em TS; se `COMMANDS.length === 0` (não deve ocorrer em prod), mostra "Nenhum comando catalogado". Se filtro/busca não retorna nada, mostra "Nenhum comando para `{query}`".
- **Hash navigation**: `useEffect` ouvindo `window.location.hash` (e mudanças via `hashchange`) faz `scrollIntoView({ behavior: 'smooth', block: 'start' })` no elemento `#cmd-{slug}` quando montar ou quando hash mudar.

## Tarefas

### UI Agent (Wave 1)

- [ ] **(parallel-safe)** Criar `src/data/commands-catalog.ts`:
  - Exportar interface `CommandEntry { cmd: string; syntax: string; category: string; short: string; simples: string; tecnico: string; when: string; notWhen: string; examples: string[]; seeAlso: string[]; }`.
  - Exportar `CATEGORIES: string[] = ['Pipeline', 'QA & Review', 'Sync', 'Knowledge', 'Task', 'Git', 'Stats', 'Maintenance', 'Skill']` (9 categorias).
  - Exportar `COMMANDS: CommandEntry[]` com **mínimo 20 entries** cobrindo (em ordem): `/mustard:feature`, `/mustard:bugfix`, `/mustard:approve`, `/mustard:resume`, `/mustard:complete`, `/mustard:scan`, `/mustard:scan-format`, `/mustard:qa`, `/mustard:review`, `/mustard:status`, `/mustard:stats`, `/mustard:metrics`, `/mustard:knowledge`, `/mustard:task`, `/mustard:git`, `/mustard:maint`, `/mustard:skill`, `/mustard:templates:agent-prompt` (18 obrigatórios) + 2-7 sub-formas/aliases conhecidos (`/mustard:resume --inline`, `/mustard:scan <subproject>`, etc.) para chegar a ≥20.
  - Conteúdo de `simples` em PT-BR plain language (sem jargão de pipeline). Conteúdo de `tecnico` referencia fases (ANALYZE/PLAN/EXECUTE/QA/CLOSE), hooks relevantes, env vars (`MUSTARD_*`) e budget (modelo, retries) quando aplicável.
  - `examples` com mínimo 1 entry por comando. `seeAlso` lista os `cmd` (sem o prefixo `/mustard:` — ex: `['approve', 'resume']`) de comandos relacionados para os chips clicáveis.

- [ ] Criar `src/pages/Commands.tsx`:
  - Estrutura raiz: `<div className="flex flex-col gap-4">` com (a) breadcrumb `Mustard / Comandos` (padrão `Knowledge.tsx`), (b) header `<h1 className="text-base font-medium">Catálogo de comandos</h1>` + counter `(N)` em `font-mono text-muted-foreground/50`, (c) input de busca (replicar markup de `Knowledge.tsx` com ícone `Search` lucide + autoFocus + debounce 300ms), (d) row de chips de categoria (botões com Badge active state usando `bg-primary/10 text-primary` quando ativo), (e) lista de cards.
  - **Card collapsed**: `<button>` largura full, `<div>` com `font-mono` para `cmd`, `Badge variant="secondary"` para `category`, `text-muted-foreground text-[13px]` para `short`. Chevron lucide à esquerda (Right collapsed / Down expanded).
  - **Card expanded**: dois containers em `grid grid-cols-1 md:grid-cols-2 gap-3` para "Explicação simples" / "Detalhes técnicos" (cada um com label `text-[11px] uppercase tracking-wider text-muted-foreground` + corpo). Abaixo, sections "Quando usar" / "Quando NÃO usar" (mesma label style) e "Exemplos" (cada exemplo em `<code className="font-mono text-[12px] bg-muted/40 px-2 py-1 rounded">` + botão `<button>` lucide `Copy` que chama `navigator.clipboard.writeText(example)`). "Ver também": chips clicáveis (Badge) que fazem `document.getElementById('cmd-' + slug)?.scrollIntoView({ behavior: 'smooth', block: 'start' })` E setam `window.location.hash = 'cmd-' + slug`.
  - **Filter logic** (`useMemo`): aplica (a) filtro de categoria se `selectedCategory !== null`, (b) busca se `query.trim().length >= 2`: lowercase contains em `cmd + ' ' + simples + ' ' + tecnico + ' ' + short`. Sem libs externas — manual `includes`.
  - **Anchor**: cada card é `<div id={`cmd-${slug(entry.cmd)}`}>` onde `slug` = `entry.cmd.replace('/mustard:', '').replace(':', '-')` (ex: `templates:agent-prompt` → `templates-agent-prompt`).
  - **Hash navigation `useEffect`**: ao montar e em `hashchange`, ler `window.location.hash`, achar elemento, `scrollIntoView` + auto-expand o card (adicionar slug ao `expanded` Set).

- [ ] Modificar `src/App.tsx`:
  - Adicionar `import { Commands } from "@/pages/Commands";` (ordem alfabética entre `Activity` e `Knowledge` ou no fim — manter consistência com imports existentes).
  - Adicionar `<Route path="/commands" element={<Commands />} />` na lista de rotas (após `/activity`, antes de `/telemetry` para manter agrupamento lógico).

- [ ] Modificar `src/components/layout/Sidebar.tsx`:
  - Adicionar `Terminal` ao import de `lucide-react`.
  - Inserir `<NavLink to="/commands" className={navItemClass}><Terminal className="h-3.5 w-3.5" /> Comandos</NavLink>` entre o NavLink de `Knowledge` e o de `Activity` (per request "entre Activity e Knowledge" — posição entre os dois, ordem atual Knowledge→Activity vira Knowledge→Comandos→Activity).

- [ ] Modificar `src/components/CommandPalette.tsx`:
  - Importar `COMMANDS` de `@/data/commands-catalog` no topo.
  - Adicionar novo `<Command.Group heading="Comandos">` (após o group "Navegar", antes de "Projetos") iterando sobre `COMMANDS` (todos, não só top-N — discoverability completa). Cada item: `<Command.Item key={c.cmd} value={`cmd-${c.cmd}`} onSelect={() => run(() => navigate(`/commands#cmd-${slug(c.cmd)}`))}>Ver: {c.cmd}</Command.Item>` com mesmo className dos outros items. Função `slug` local (mesmo cálculo de `Commands.tsx`).

### Build & Type-check (Wave 2)

- [ ] Da raiz do projeto: `npx tsc --noEmit` deve passar (exit 0).
- [ ] Da raiz do projeto: `npm run build` deve passar (exit 0).

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript compila sem erros — Command: `npx tsc --noEmit`
- [x] AC-2: Catálogo exporta ≥20 entries — Command: `node -e "const t=require('fs').readFileSync('src/data/commands-catalog.ts','utf8');const c=t.split('cmd:').length-1;process.exit(c>=20?0:1)"`
- [x] AC-3: Página Commands.tsx existe com export — Command: `node -e "const t=require('fs').readFileSync('src/pages/Commands.tsx','utf8');process.exit(t.includes('export')&&t.includes('Commands')?0:1)"`
- [x] AC-4: Rota /commands registrada em App.tsx — Command: `node -e "const t=require('fs').readFileSync('src/App.tsx','utf8');process.exit(t.includes('/commands')&&t.includes('Commands')?0:1)"`
- [x] AC-5: Sidebar tem NavLink Comandos com ícone Terminal — Command: `node -e "const t=require('fs').readFileSync('src/components/layout/Sidebar.tsx','utf8');process.exit(t.includes('Terminal')&&t.includes('/commands')?0:1)"`
- [x] AC-6: CommandPalette referencia o catálogo — Command: `node -e "const t=require('fs').readFileSync('src/components/CommandPalette.tsx','utf8');process.exit(t.includes('commands-catalog')&&t.includes('COMMANDS')?0:1)"`
- [x] AC-7: Build Vite passa — Command: `npm run build`

## Não-Objetivos

- PRD Builder (planejado para Wave E em spec separada `dashboard-prd-builder`)
- ENV editor (planejado para Wave F em spec separada `dashboard-env-editor`)
- License gating
- Live updates do catálogo (catálogo é estático em TS local, atualiza por commit)
- Persistência da query/filtros do catálogo entre sessões (não pedido; adicionar depois se útil)
- Tradução automática do conteúdo `simples`/`tecnico` para EN — só PT-BR nesta entrega

## Decisões não-óbvias

- **Catálogo declarativo em TS local** (não JSON fetched, não filesystem walk de `.claude/commands/mustard/*/SKILL.md`): conteúdo é versionado junto com o app, não depende de instalação do Mustard core, e permite type-safety. Trade-off: precisa atualizar manualmente quando comandos novos forem adicionados ao Mustard — aceitável dado que `/mustard:*` muda raramente.
- **Search por `includes` lowercase** (sem Fuse.js): catálogo tem ~25 entries; performance é non-issue, tipo "fuzzy" não traz benefício real. Evita uma dependência nova.
- **Anchor `#cmd-{slug}` + `scrollIntoView`**: permite shareable links (`/commands#cmd-feature`), Cmd+K integration trivial via `location.hash`, e auto-expand do card alvo. Slug derivado de `cmd.replace('/mustard:','').replace(':','-')` para suportar `templates:agent-prompt` virar `templates-agent-prompt`.
- **Cmd+K mostra TODOS os comandos do catálogo** (não só os ≥5 do AC original): cmdk faz fuzzy matching nativo do `value`, então a lista cresce sem ruído — usuário só vê o que digitou. Discoverability completa.
- **Ordem do Sidebar muda para Knowledge → Comandos → Activity → Telemetry**: per request literal "entre Activity e Knowledge". Onboarding mental: catálogo de comandos vem logo após "Knowledge" (conhecimento) e antes de "Activity" (uso).
- **Sem dependência nova `sonner`/toaster** para o feedback de copy: o usuário verá clipboard mudar; toast adiciona dependência por benefício marginal. Pode adicionar em iteração futura se UX feedback pedir.
