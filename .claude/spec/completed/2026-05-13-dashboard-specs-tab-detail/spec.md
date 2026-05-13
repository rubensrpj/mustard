# Feature: dashboard-specs-tab-detail

### Status: closed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T03:02:53.000Z
### Lang: pt

## Contexto

Usuários do Mustard Dashboard esperam abrir um projeto e enxergar imediatamente o trabalho em andamento — quais specs estão em ANALYZE/EXECUTE/QA, quais fecharam, e o conteúdo de cada uma com Acceptance Criteria, checklist e arquivos afetados. Hoje a página de detalhe do projeto mostra somente inventário estático (subprojetos, recipes, skills) e um feed bruto de eventos do harness, sem nenhum recorte por spec. O backend Tauri já expõe `dashboard_specs` retornando rows por spec lidas do SQLite, porém nenhuma rota ou componente da UI consome esse endpoint, tornando o dado invisível para o operador. O resultado é que entender o estado das pipelines exige abrir o terminal e ler `spec.md` à mão dentro de `.claude/spec/active/` — anulando o ganho de instalar o dashboard.

## Resumo

Refatorar `ProjectDetail` em duas abas estilo Linear ("Specs" default + "About" preservando o layout atual), introduzir `SpecsList` consumindo `fetchSpecs` com lista densa clicável, criar drill-down `SpecDetail` em `/project/:id/spec/:specName` que lê o markdown bruto via novo comando Rust `dashboard_spec_markdown` (active → completed fallback) e renderiza AC numeradas + Checklist com checkboxes + Affected files. Cmd+K ganha grupo "Specs do projeto atual" quando há `selectedProjectId`.

## Limites

Caminhos intencionalmente tocados:

- `src-tauri/src/lib.rs` — adicionar comando `dashboard_spec_markdown` e registrar no `invoke_handler`
- `src/lib/dashboard.ts` — exportar `fetchSpecMarkdown(repoPath, specName)`
- `src/pages/ProjectDetail.tsx` — refatorar em Tabs (Specs default + About) com sync via search param `?tab=`
- `src/components/SpecsList.tsx` — **NOVO** componente de listagem densa
- `src/pages/SpecDetail.tsx` — **NOVA** página de drill-down
- `src/App.tsx` — adicionar rota `/project/:id/spec/:specName`
- `src/components/CommandPalette.tsx` — adicionar grupo "Specs do projeto atual"

Fora do escopo: edit/criar spec, AggregateView, KnowledgeBrowser, persistência de scroll position cross-route, animations de transição entre tabs, license, CI.

## Arquivos (~7)

| Path | Op | Wave |
|------|----|------|
| `src-tauri/src/lib.rs` | edit | 1 |
| `src/lib/dashboard.ts` | edit (parallel-safe) | 1 |
| `src/components/SpecsList.tsx` | create | 2 |
| `src/pages/SpecDetail.tsx` | create | 2 |
| `src/pages/ProjectDetail.tsx` | edit | 2 |
| `src/App.tsx` | edit | 2 |
| `src/components/CommandPalette.tsx` | edit | 2 |

## Component Contract

### `SpecsList`

- **Props:** `{ project: Project }` (Project tem `id` + `path`).
- **Data:** `useQuery({ queryKey: ['specs', project.path], queryFn: () => fetchSpecs(project.path), staleTime: 30_000 })`.
- **Estados:**
  - Loading: 3 skeleton rows `h-6 bg-muted/40 rounded animate-pulse`.
  - Error: `<p className="text-destructive text-sm">{error.message}</p>`.
  - Empty (data.length === 0): bloco centralizado com lucide `FileText` (opacity reduzida) + texto "Nenhuma spec encontrada. Use /mustard:feature no projeto para começar."
  - Populated: `ul.flex.flex-col.gap-0.5`. Cada `li`: `<StatusDot variant={...} /> <span className="font-mono font-medium">{name}</span> <Badge variant="secondary">{phase}</Badge> <Badge variant="outline" className="text-[10px] py-0">{status}</Badge> <span className="ml-auto text-muted-foreground text-xs">{timestamp}</span>`. Hover `bg-muted/40 cursor-pointer`. Click → `navigate("/project/" + project.id + "/spec/" + encodeURIComponent(spec.name))`.
- **StatusDot mapping** (função interna `specVariant(spec: SpecRow): StatusDotVariant`):
  - `status === 'blocked'` → `'blocked'` (precedência sobre phase).
  - `phase === 'EXECUTE'` → `'active'`.
  - `phase === 'ANALYZE' | 'PLAN' | 'QA'` → `'planning'`.
  - `phase === 'CLOSE'` → `'done'`.
  - default → `'idle'`.
- **Timestamp:** `completed_at ? relativeTime(completed_at) : (started_at ? relativeTime(started_at) : '—')`.

### `SpecDetail`

- **Props:** none (consome `useParams<{ id; specName }>()`).
- **Lookup row:** `queryClient.getQueryData<SpecRow[]>(['specs', project.path])?.find(...)`. Se a query nunca rodou (acesso direto via URL), `useQuery(['specs', project.path], () => fetchSpecs(project.path))` faz fetch on-demand.
- **Markdown:** `useQuery({ queryKey: ['spec-markdown', project.path, specName], queryFn: () => fetchSpecMarkdown(project.path, specName) })`.
- **Estados:**
  - Project não encontrado: mensagem "Projeto não encontrado" + Link Home (mesmo padrão de `ProjectDetail`).
  - Markdown loading: skeleton de 3 blocos.
  - Markdown error: `<p className="text-destructive text-sm">{error.message}</p>` + botão "Voltar para Specs".
  - Populated: breadcrumb + header (nome em `h1 text-base font-medium`, Phase Badge, Status Badge, Started/Completed em `text-xs text-muted-foreground`), depois 3 seções renderizadas a partir do markdown parseado.
- **Botão "Voltar para Specs":** `NavLink` ou `Link` para `/project/${id}?tab=specs` no canto superior direito do header.
- **Parser de seções** (helpers internos puros, sem libs externas):
  - `extractSection(md: string, heading: string): string | null` — encontra `## ${heading}` e retorna conteúdo até próximo `## ` ou EOF.
  - **Acceptance Criteria:** procura linhas que comecem com `- [ ]` ou `- [x]` AC-N. Renderiza `<ol>` numerada. Para cada item, extrair (a) texto antes do `— Command:` (b) bloco de código após. Renderizar `<code className="font-mono text-xs bg-muted px-1 py-0.5 rounded">` para o comando. Se a seção não existir → "Sem AC definidos" em `text-muted-foreground`.
  - **Checklist:** parse linha-a-linha. Headings `### X` viram `<h3 className="text-xs uppercase tracking-wider text-muted-foreground mt-3 mb-1">{label}</h3>`. Linhas `- [ ]` ou `- [x]` viram `<label><input type="checkbox" disabled checked={isChecked} /> {text}</label>`. Outras linhas (texto livre) viram `<p className="text-xs text-muted-foreground">`.
  - **Affected files:** vem do `SpecRow.affected_files` (não do markdown). Render `<ul className="font-mono text-xs">` com bullets. Vazio → "Sem arquivos registrados."

### `ProjectDetail` (refatoração — boundary)

- Mantém: `useParams`, `useStore`, `useProject`, breadcrumb, header com `project.name`.
- Remove: estrutura em sections flat de Subprojects/Recipes/Skills/Eventos do retorno principal.
- Adiciona: `<Tabs value={tab} onValueChange={(v) => setSearchParams({ tab: v }, { replace: true })}>` com `<TabsList>` contendo `<TabsTrigger value="specs">Specs</TabsTrigger>` e `<TabsTrigger value="about">About</TabsTrigger>`.
- `tab` lido de `useSearchParams()` com default `'specs'`. Persistido via `setSearchParams`.
- `<TabsContent value="specs">` → `<SpecsList project={project} />`.
- `<TabsContent value="about">` → reaproveita o JSX atual das 4 seções (Subprojects/Recipes/Skills/Eventos) sem mudanças funcionais.
- Style tabs (override shadcn defaults): `h-8`, `text-xs`, `gap-4`, ativo com `border-b-2 border-indigo-500` (ou `text-primary border-b-2 border-primary` — verificar tokens do tema).

### `CommandPalette` (extensão)

- Adicionar consumo de `selectedProjectId` (já vem do `useStore`, atualmente não usado — usar valor existente).
- Após o grupo "Projetos", se `selectedProjectId` definido e projeto encontrado em cache `['discover', projectsRoot]`, ler specs do cache `['specs', project.path]` via `queryClient.getQueryData<SpecRow[]>(...)`. Não fazer fetch — apenas mostrar se já carregado.
- Renderizar `Command.Group heading="Specs"` com `Command.Item` por spec: `Open spec: {name}` → `navigate("/project/" + project.id + "/spec/" + encodeURIComponent(name))`.
- Se `selectedProjectId` ausente ou cache vazio → não renderiza o grupo (sem placeholder).

## Tarefas

### Backend Agent (Wave 1)

- [ ] Adicionar comando `dashboard_spec_markdown(repo_path: String, spec_name: String) -> Result<String, String>` em `src-tauri/src/lib.rs` — tenta `.claude/spec/active/{spec_name}/spec.md`, fallback `.claude/spec/completed/{spec_name}/spec.md`, retorna conteúdo ou erro `"spec markdown not found: {spec_name}"`.
- [ ] Registrar o comando em `invoke_handler` (linha 411+ do `lib.rs`, dentro do `tauri::generate_handler![...]`).
- [ ] Rodar `cargo check` em `src-tauri/` e garantir build limpo.

### Frontend API Agent (Wave 1, parallel-safe)

- [ ] Exportar `fetchSpecMarkdown(repoPath: string, specName: string): Promise<string>` em `src/lib/dashboard.ts` invocando `"dashboard_spec_markdown"` com `{ repoPath, specName }`. Posicionar próximo a `fetchSpecs`.

### Frontend Agent (Wave 2)

- [ ] Criar `src/components/SpecsList.tsx` conforme Component Contract (props, estados, StatusDot mapping, click navigate).
- [ ] Criar `src/pages/SpecDetail.tsx` conforme Component Contract (parsing de AC/Checklist/Affected files, breadcrumb, botão voltar).
- [ ] Refatorar `src/pages/ProjectDetail.tsx` em Tabs (Specs default + About preservando layout atual). Sync via `useSearchParams` com default `?tab=specs`.
- [ ] Adicionar rota `<Route path="/project/:id/spec/:specName" element={<SpecDetail />} />` em `src/App.tsx`.
- [ ] Estender `src/components/CommandPalette.tsx` com grupo "Specs" (consome cache, só renderiza se `selectedProjectId` + projeto + cache de specs presentes).
- [ ] Rodar `pnpm tsc --noEmit` e garantir zero erros.

## Dependências

- Wave 2 depende de Wave 1 em runtime (FE invoca `dashboard_spec_markdown`), mas type-check da Wave 2 não depende do build Rust — `tsc --noEmit` passa mesmo sem cargo. Marcamos Wave 1 frontend-api como `parallel-safe`.
- Reaproveita componentes existentes: `StatusDot` (`src/components/StatusDot.tsx`), `Badge` (`src/components/ui/badge.tsx`), `Tabs` (`src/components/ui/tabs.tsx`), `ScrollArea`, `Separator`, helper `relativeTime` (`src/lib/time.ts`), `queryClient` (`src/lib/query-client.ts`).
- Não introduz nova dependência npm. Não introduz nova crate Rust.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript type-check passa sem erros — Command: `pnpm exec tsc --noEmit`
- [x] AC-2: Rust crate compila — Command: `cargo check --manifest-path src-tauri/Cargo.toml`
- [x] AC-3: Comando Rust `dashboard_spec_markdown` está registrado em `invoke_handler` — Command: `node -e "const s=require('fs').readFileSync('src-tauri/src/lib.rs','utf8'); process.exit(s.includes('fn dashboard_spec_markdown') && s.match(/invoke_handler[\s\S]*dashboard_spec_markdown/) ? 0 : 1)"`
- [x] AC-4: `fetchSpecMarkdown` exportado em `src/lib/dashboard.ts` — Command: `node -e "process.exit(require('fs').readFileSync('src/lib/dashboard.ts','utf8').includes('export function fetchSpecMarkdown') ? 0 : 1)"`
- [x] AC-5: Arquivos novos `SpecsList.tsx` e `SpecDetail.tsx` existem — Command: `node -e "const f=require('fs'); process.exit(f.existsSync('src/components/SpecsList.tsx') && f.existsSync('src/pages/SpecDetail.tsx') ? 0 : 1)"`
- [x] AC-6: Rota `/project/:id/spec/:specName` registrada em `App.tsx` — Command: `node -e "process.exit(require('fs').readFileSync('src/App.tsx','utf8').includes('/spec/:specName') ? 0 : 1)"`
- [x] AC-7: `ProjectDetail` usa componentes Tabs do shadcn — Command: `node -e "const s=require('fs').readFileSync('src/pages/ProjectDetail.tsx','utf8'); process.exit(s.includes('TabsList') && s.includes('TabsTrigger') && s.includes('TabsContent') ? 0 : 1)"`
- [x] AC-8: `CommandPalette` lê specs do cache do React Query — Command: `node -e "const s=require('fs').readFileSync('src/components/CommandPalette.tsx','utf8'); process.exit(s.includes(\"queryClient.getQueryData\") && s.match(/\\['specs',/) ? 0 : 1)"`

## Preocupações

- WARN layer-gap: validator não reconhece `.rs` como Backend extension. Falso positivo — `src-tauri/src/lib.rs` é o backend Tauri/Rust.
- WARN layer-gap: validator não reconhece `.tsx` como Frontend extension da forma esperada. Falso positivo — todos arquivos sob `src/` são Frontend.
- WARN task-count: Frontend API Agent tem 1 task (mínimo do validator é 2). Aceitável: a tarefa é atômica (exportar 1 função wrapping `invoke`); decompor em 2 seria artificial.

## Não-Objetivos

- Editor/criação de spec (apenas leitura).
- Markdown rendering geral (usar parser simples line-by-line, sem `react-markdown` ou similar).
- AggregateView cross-project (Tier 2.4 — próxima wave).
- KnowledgeBrowser com FTS5 (Tier 2.5 — próxima wave).
- Sync de scroll position entre tabs / cross-route.
- Animations de transição entre tabs.
- Suporte a `spec-references/{section}.md` (progressive disclosure) — escopo limitado a `spec.md` único.
- Renderização de wave plans / multi-spec parents.
