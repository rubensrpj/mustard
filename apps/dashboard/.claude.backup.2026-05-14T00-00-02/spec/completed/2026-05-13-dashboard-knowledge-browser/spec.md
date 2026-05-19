# Enhancement: dashboard-knowledge-browser

### Status: closed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-13T03:16:54.000Z
### Lang: pt

## Contexto

O backend Tauri já expõe `dashboard_search_knowledge(repo_path, query, limit)` retornando rows com `id/type/name/description/confidence/source` indexadas no SQLite FTS5 de cada projeto, mas a Sidebar mostra o item "Knowledge — soon" como placeholder desabilitado e nenhuma rota consome o endpoint. O operador que quer recuperar um padrão ou convenção descoberta pelo `/knowledge` (Mustard) precisa abrir o terminal e rodar `node .claude/scripts/harness-views.js`, fugindo do dashboard. Pior: como cada projeto tem o seu próprio knowledge base, ele não tem como buscar a mesma frase em todos os projetos descobertos de uma vez. O resultado é que dois meses de patterns extraídos pelos pipelines ficam acessíveis apenas via CLI — invertendo a promessa de "knowledge cross-project visível".

## Resumo

Ativar o item "Knowledge" da Sidebar como rota real `/knowledge`. Criar página `Knowledge.tsx` com input de busca debounced (300ms) que dispara `useQueries` cross-project chamando `fetchSearchKnowledge` por projeto, agrupando resultados merged por confidence desc com badge do projeto de origem. Adicionar entrada Cmd+K para "Ir para Knowledge".

## Checklist

### Frontend Agent

- [x] Criar `src/hooks/useKnowledgeSearch.ts` exportando `useKnowledgeSearch(projects: Project[], query: string)` que retorna `{ results, loading }`. Internamente usa `useQueries` com `queryKey: ['knowledge-search', p.path, query]`, `queryFn: () => fetchSearchKnowledge(p.path, query, 50)`, `enabled: query.trim().length >= 2`, `staleTime: 60_000`. Flatten results em `{ projectId, projectName, row: KnowledgeRow }`, ordena por `row.confidence` desc.
- [x] Criar `src/pages/Knowledge.tsx`: input controlado com `value={query}` + debounce 300ms via useState/useEffect para `debouncedQuery`. Header "Knowledge cross-project" + contador de resultados. Estados: sem query (texto "Digite ≥2 caracteres para buscar."), loading (skeleton 3 rows), vazio com query (texto "Nenhum resultado para '<query>'."), populated (lista densa). Cada row: Badge type secondary + name (mono font-medium) + project name (text-xs muted) + confidence (font-mono percentual) + description truncada 120 chars. Sem onClick (sem destino interno ainda; futura iteração).
- [x] Editar `src/components/layout/Sidebar.tsx` substituindo o `<div className={disabledItemClass}>` de Knowledge por `<NavLink to="/knowledge" className={navItemClass}>` mantendo o ícone BookOpen. Remover o `<span ... opacity-60>soon</span>`.
- [x] Editar `src/App.tsx` adicionando `<Route path="/knowledge" element={<Knowledge />} />`.
- [x] Editar `src/components/CommandPalette.tsx` adicionando `Command.Item` "Ir para Knowledge" no grupo "Navegar" (entre Home e Settings).
- [x] Rodar `pnpm exec tsc --noEmit` e garantir zero erros.

## Arquivos (~5)

- `src/hooks/useKnowledgeSearch.ts` (NEW)
- `src/pages/Knowledge.tsx` (NEW)
- `src/components/layout/Sidebar.tsx` (edit)
- `src/App.tsx` (edit)
- `src/components/CommandPalette.tsx` (edit)

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript type-check passa sem erros — Command: `pnpm exec tsc --noEmit`
- [x] AC-2: Arquivos novos `useKnowledgeSearch.ts` e `Knowledge.tsx` existem — Command: `node -e "const f=require('fs'); process.exit(f.existsSync('src/hooks/useKnowledgeSearch.ts') && f.existsSync('src/pages/Knowledge.tsx') ? 0 : 1)"`
- [x] AC-3: Rota `/knowledge` registrada em `App.tsx` — Command: `node -e "const s=require('fs').readFileSync('src/App.tsx','utf8'); process.exit(s.includes('path=\"/knowledge\"') && s.includes('<Knowledge') ? 0 : 1)"`
- [x] AC-4: Sidebar Knowledge é NavLink ativa para `/knowledge` — Command: `node -e "const s=require('fs').readFileSync('src/components/layout/Sidebar.tsx','utf8'); process.exit(/<NavLink[^>]*to=\"\\/knowledge\"/.test(s) ? 0 : 1)"`
- [x] AC-5: useKnowledgeSearch usa useQueries + fetchSearchKnowledge — Command: `node -e "const s=require('fs').readFileSync('src/hooks/useKnowledgeSearch.ts','utf8'); process.exit(s.includes('useQueries') && s.includes('fetchSearchKnowledge') ? 0 : 1)"`
