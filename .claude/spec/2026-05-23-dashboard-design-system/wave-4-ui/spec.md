# Wave 4 — Folder-per-component + features/ namespace (refator estrutural)

## Resumo

Refator estrutural cirúrgico, sem mudança de UI nem de comportamento, alinhando o dashboard ao padrão React/Tauri 2026 confirmado por pesquisa: cada componente em sua própria pasta com `index.tsx`, lógica de domínio fora de `components/` (que vira só primitivas compartilhadas) e dentro de `src/features/`. Hoje `src/components/` mistura 11 subpastas: 8 de domínio (`specs/`, `workspace/`, `economy/`, `knowledge/`, `prd/`, `telemetry/`, `amend/`, `trace/`) e 3 compartilhadas (`page/`, `layout/`, `ui/`), além de 10 arquivos soltos no root (`AggregateOverview`, `CommandPalette`, `KnowledgeCard`, `LivePipelineCard`, `Markdown`, `SpecSidePanel`, `SpecsList`, `StatusDot`, `WaveNav`, `WorkspaceDigest`). Cada `.tsx` é flat e o barrel `page/index.ts` é único — features de domínio importam diretamente do arquivo. Esta wave (a) move as 8 pastas de domínio para `src/features/`, (b) converte CADA `.tsx` em `Component/index.tsx` (folder-per-component, padrão Robin Wieruch 2026 / FSD shared), (c) realoca os 10 strays para suas features ou para shared, (d) adiciona `@/features/*` ao `tsconfig.json` e ao `vite.config.ts` para alias path, (e) atualiza TODOS os imports em `src/` via codemod determinístico, (f) cria o script faltante `scripts/check-pages-no-inline-visual.mjs` (AC-10 do parent), (g) varre tokens fantasmas (`--color-ok`, `--color-accent-mustard`, `text-red-*`) nos arquivos movidos durante o trânsito — eliminando ~107 ocorrências surfaceadas no review da Wave 3. Zero mudança de prop, zero refit visual, zero novo componente. No fim: 3 pastas em `components/` (page, layout, ui) só com primitivas; 8 pastas em `features/` com lógica de domínio; cada componente em sua pasta com `index.tsx`; tokens fantasmas erradicados; script AC-10 existindo. Waves 5 e 6 consomem `@/features/*` direto sem nova refatoração.

## Network

- Parent: [[2026-05-23-dashboard-design-system]]
- Depende de: [[wave-3-ui]] (Topbar/Sidebar/button já no padrão Binance — o refator estrutural não retoca shell)
- Habilita: [[wave-5-ui]] (high-traffic pages importam `@/features/{specs,workspace,economy,knowledge}` direto); [[wave-6-ui]] (idem para secundárias)

## Component Contract

### Regra de transformação (uma regra, aplicada uniformemente)

Para cada arquivo `apps/dashboard/src/components/{dir}/{Name}.tsx`:

1. Determine o destino:
   - Se `{dir} ∈ {specs, workspace, economy, knowledge, prd, telemetry, amend, trace}` (domínio): destino é `apps/dashboard/src/features/{dir}/{Name}/`
   - Se `{dir} ∈ {page, layout, ui}` (compartilhado): destino é `apps/dashboard/src/components/{dir}/{Name}/`
2. Crie a pasta destino.
3. Mova o `.tsx` para `{destino}/index.tsx` (mantendo conteúdo idêntico). O nome do arquivo vira sempre `index.tsx`; a pasta carrega o nome do componente.
4. Co-located helpers (`spec-graph-layout.ts`, `stage-from-status.ts`, `spec-status.tsx`) vão para a pasta do componente que os consome principal; se forem compartilhados por múltiplos, ficam em `features/{dir}/_shared/{Name}.ts` (underscore = não-componente).
5. `__tests__/` permanece dentro da pasta da feature inteira, NÃO migra para subpastas por componente.
6. Atualize TODAS as referências `@/components/{dir}/{Name}` em `src/**/*.{ts,tsx}` para o novo caminho:
   - domínio: `@/features/{dir}/{Name}`
   - shared: `@/components/{dir}/{Name}` (sufixo idêntico, mas TypeScript resolve `Name` → `Name/index.tsx`)
7. Barrels (`index.ts`) em `features/{dir}/index.ts` e `components/{dir}/index.ts` recebem `export * from './{Name}'` para cada componente — pages podem importar tanto granular (`@/features/specs/SpecCard`) quanto agregado (`@/features/specs`).

**Princípio inegociável:** zero mudança de código DENTRO dos componentes. O `.tsx` é movido bit-a-bit. Imports DENTRO dele que apontavam para componentes da mesma feature passam de `from "./Sibling"` para `from "../Sibling"` (porque agora cada componente está um nível mais fundo). Esta é a ÚNICA edição feita no conteúdo.

### Estado inicial vs final

| Dir | Antes | Depois | Local |
|---|---|---|---|
| `components/specs/` | 20 `.tsx` flat + 2 `.ts` helpers | 20 pastas com `index.tsx` + `_shared/` com helpers | `features/specs/` |
| `components/workspace/` | 13 `.tsx` flat | 13 pastas com `index.tsx` | `features/workspace/` |
| `components/economy/` | 3 `.tsx` | 3 pastas com `index.tsx` | `features/economy/` |
| `components/knowledge/` | 1 `.tsx` (KnowledgeBadge) | 1 pasta + index | `features/knowledge/` |
| `components/prd/` | 4 `.tsx` | 4 pastas + index | `features/prd/` |
| `components/telemetry/` | 11 `.tsx` | 11 pastas + index | `features/telemetry/` |
| `components/amend/` | 2 `.tsx` + `__tests__/` | 2 pastas + index + `__tests__/` preservado | `features/amend/` |
| `components/trace/` | 2 `.tsx` (ExecutionTrace, ToolEventRow) | 2 pastas + index | `features/trace/` |
| `components/page/` | 16 `.tsx` + `README.md` + `index.ts` | 16 pastas + index + README | `components/page/` (sem moverdir) |
| `components/layout/` | 4 `.tsx` (já editados Wave 3) | 4 pastas + index | `components/layout/` |
| `components/ui/` | 14 shadcn `.tsx` (lowercase) | 14 pastas (mantém lowercase) + index | `components/ui/` |
| `components/` root (10 strays) | flat | Cada vai para sua feature ou para shared (mapa abaixo) | — |

### Mapa dos 10 strays no root de `components/`

| Stray | Destino | Razão |
|---|---|---|
| `AggregateOverview.tsx` | `features/workspace/AggregateOverview/index.tsx` | Consome workspace context |
| `CommandPalette.tsx` | `components/layout/CommandPalette/index.tsx` | Shell-global, igual Topbar |
| `KnowledgeCard.tsx` | `features/knowledge/KnowledgeCard/index.tsx` | Knowledge feature |
| `LivePipelineCard.tsx` | `features/workspace/LivePipelineCard/index.tsx` | Workspace feature |
| `Markdown.tsx` | `components/page/Markdown/index.tsx` | Wave 2 era pra ter movido — primitiva shared |
| `SpecSidePanel.tsx` | `features/specs/SpecSidePanel/index.tsx` | Specs feature |
| `SpecsList.tsx` | `features/specs/SpecsList/index.tsx` | Specs feature |
| `StatusDot.tsx` | `components/page/StatusDot/index.tsx` | Wave 2 era pra ter movido — primitiva shared |
| `WaveNav.tsx` | `features/specs/WaveNav/index.tsx` | Spec-wave bound |
| `WorkspaceDigest.tsx` | `features/workspace/WorkspaceDigest/index.tsx` | Workspace feature |

### Codemod (`scripts/refactor-folder-per-component.mjs`)

Script Node 22 idempotente. Estrutura:

```js
// scripts/refactor-folder-per-component.mjs
// Run: node scripts/refactor-folder-per-component.mjs [--dry-run] [--verbose]
//
// Idempotent: if a file is already at its destination, skip.
// Logs every move + import rewrite. Exits non-zero on any I/O error.
//
// Rules: see Wave 4 spec.
```

Operações do script:

1. **Discovery**: walk `apps/dashboard/src/components/**` e enumerate `.tsx` files at depth-1 (root) ou depth-2 (subdir/file.tsx).
2. **Plan**: para cada arquivo descoberto, calcular destino conforme tabela acima. Construir mapa `{from: absPath, to: absPath}`.
3. **Validate**: confirmar que nenhum destino já existe (idempotência: se existe E é igual ao source, skip; se difere, FAIL).
4. **Move via `git mv`** (preserva history). Fallback `fs.rename` se git mv falhar (não é repo / file untracked).
5. **Rewrite imports**: scan `apps/dashboard/src/**/*.{ts,tsx}` (excluding moved files in-flight). Para cada arquivo, aplicar regex de import:
   - `from "@/components/specs/X"` → `from "@/features/specs/X"` (e variantes para 7 outras features)
   - `from "@/components/X"` (root stray) → `from "@/features/{feature}/X"` ou `from "@/components/{shared}/X"` conforme mapa.
   - `from "./SiblingComponent"` em arquivo recém-movido para `Component/index.tsx` → `from "../SiblingComponent"` (siblings de mesma feature, depth ajusta).
6. **Phantom token sweep** (oportunístico — único momento que tocamos esses arquivos):
   - `--color-ok` → `--intent-success`
   - `--color-accent-mustard` → `--primary`
   - `text-red-400` / `text-red-500` → `text-[--intent-error]`
   - `bg-red-400` / `bg-red-500` → `bg-[--intent-error]/15` (ou `/10` conforme contexto — manter o opacity sufixo original quando presente)
   - Aplicar SOMENTE nos arquivos sob `features/**` e `components/{page,layout,ui}/**` recém-movidos. NÃO tocar `style.css`, `pages/**`, hooks, lib.
7. **Barrel emit**: gerar/atualizar `index.ts` em cada `features/{dir}/` e `components/{dir}/` com `export * from "./{Component}"` linha-a-linha.
8. **Report**: imprimir resumo (`X files moved, Y imports rewritten, Z phantom tokens fixed`).

Script NÃO toca: `pages/`, `api/`, `hooks/`, `lib/`, `assets/`, `data/`, `styles/`, `App.tsx`, `main.tsx`, `i18n.ts`, `style.css`, `vite-env.d.ts`.

### Path alias

`apps/dashboard/tsconfig.json`:

```jsonc
{
  "compilerOptions": {
    "paths": {
      "@/*": ["./src/*"],
      "@/features/*": ["./src/features/*"]  // explicit (works without too, but explicit ajuda Vite/ESLint)
    }
  }
}
```

`apps/dashboard/vite.config.ts`: já tem `@: src` resolver. Adicionar `@/features` é opcional pois `@/features/X` resolve para `src/features/X` pelo `@: src` mapping. Confirmar via build.

### `scripts/check-pages-no-inline-visual.mjs` (criação)

Script AST-walk que satisfaz AC-10 do parent. Usa `@typescript-eslint/typescript-estree` (ou `acorn` + `acorn-jsx` para evitar dep nova — provavelmente já está como dep transitiva do Vite). Para cada `apps/dashboard/src/pages/*.tsx`:

1. Parse → AST.
2. Walk JSX:
   - **Fail (a)**: atributo `style={...}` cujo objeto literal contém chave em `["color","background","backgroundColor","border","borderColor","borderRadius","boxShadow"]`.
   - **Fail (b)**: atributo `className` (string literal ou `cn(...)` arg literal) contendo regex `^|\s(text|bg|border|ring|fill|stroke)-(\w+-\d+)(?=\s|$)` exceto whitelist `text-foreground|text-muted-foreground|text-card-foreground|bg-background|bg-card|bg-sidebar|bg-primary|border-border|border-sidebar-border|ring-primary` etc. — lista de tokens permitidos baseada em DESIGN.md.
   - **Fail (c)**: string literal `\\#[0-9a-fA-F]{3,8}` em qualquer expressão.
3. Coletar violações com `{file, line, column, kind, snippet}`. Imprimir e exit non-zero se houver.
4. **Permite**: classes de layout estrutural (`grid`, `flex`, `gap-*`, `w-*`, `h-*`, `max-w-*`, `col-span-*`, `row-*`, `place-*`, `mx-*`, `my-*`, `p-*` SEM `bg-*`).

Script é menor que 250 linhas. Sem novas deps obrigatórias (usa `node:fs` + `acorn` se já presente; caso não, adiciona `acorn-jsx` como devDep — confirmado no parecer pré-execute). Para Wave 4, AC-W4-7 só exige que o script exista e exit 0 quando rodado contra os arquivos atuais (que JÁ violariam suas regras nas pages — então o script começa **falhando contra as pages reais** e Waves 5/6 vão fazer ele passar conforme migrarem). Por isso AC-W4-7 testa apenas a EXISTÊNCIA + executabilidade dele, NÃO pass nas pages.

## Arquivos

**Movidos via codemod (sem edição de conteúdo, exceto sibling imports):**
- `apps/dashboard/src/components/specs/*.{tsx,ts}` → `apps/dashboard/src/features/specs/{Name}/index.{tsx,ts}` (22 arquivos)
- `apps/dashboard/src/components/workspace/*.tsx` → `apps/dashboard/src/features/workspace/{Name}/index.tsx` (13)
- `apps/dashboard/src/components/economy/*.tsx` → `apps/dashboard/src/features/economy/{Name}/index.tsx` (3)
- `apps/dashboard/src/components/knowledge/*.tsx` → `apps/dashboard/src/features/knowledge/{Name}/index.tsx` (1)
- `apps/dashboard/src/components/prd/*.tsx` → `apps/dashboard/src/features/prd/{Name}/index.tsx` (4)
- `apps/dashboard/src/components/telemetry/*.tsx` → `apps/dashboard/src/features/telemetry/{Name}/index.tsx` (11)
- `apps/dashboard/src/components/amend/*.tsx` → `apps/dashboard/src/features/amend/{Name}/index.tsx` (2; `__tests__/` preservado em `features/amend/__tests__/`)
- `apps/dashboard/src/components/trace/*.tsx` → `apps/dashboard/src/features/trace/{Name}/index.tsx` (2)
- `apps/dashboard/src/components/page/*.tsx` → `apps/dashboard/src/components/page/{Name}/index.tsx` (16)
- `apps/dashboard/src/components/layout/*.tsx` → `apps/dashboard/src/components/layout/{Name}/index.tsx` (4)
- `apps/dashboard/src/components/ui/*.tsx` → `apps/dashboard/src/components/ui/{name}/index.tsx` (14, lowercase preservado)
- 10 strays de `apps/dashboard/src/components/*.tsx` → destinos do mapa (10)

**Total: ~100 componentes movidos + 10 strays.**

**Editados (imports apenas):**
- `apps/dashboard/src/pages/*.tsx` (11 arquivos — imports `@/components/specs/X` viram `@/features/specs/X` etc.)
- `apps/dashboard/src/App.tsx`, `apps/dashboard/src/main.tsx`
- `apps/dashboard/src/hooks/**`, `apps/dashboard/src/api/**`, `apps/dashboard/src/lib/**` (qualquer import de componente)

**Criados (novos):**
- `scripts/refactor-folder-per-component.mjs` (codemod, ~300 linhas)
- `scripts/check-pages-no-inline-visual.mjs` (AST-walk AC-10, ~250 linhas)
- `apps/dashboard/src/features/{specs,workspace,economy,knowledge,prd,telemetry,amend,trace}/index.ts` (8 barrels)
- `apps/dashboard/src/components/{page,layout,ui}/index.ts` (3 barrels — substituem `page/index.ts` existente)

**Modificados (config):**
- `apps/dashboard/tsconfig.json` (path alias `@/features/*`)
- `apps/dashboard/vite.config.ts` (confirmar resolve.alias; ajustar se necessário)
- `apps/dashboard/package.json` (devDeps: `acorn-jsx` se ausente — investigar primeiro)
- `apps/dashboard/.claude/skills/dashboard-page-primitives/SKILL.md` (atualizar inventário e regras de import)
- `apps/dashboard/.claude/CLAUDE.md` (apontar para nova estrutura)

**Deletados:**
- Pastas vazias `components/{specs,workspace,economy,knowledge,prd,telemetry,amend,trace}/` (após codemod move tudo)
- Strays no root de `components/` (10 arquivos, todos movidos)

## Informações da Entidade

N/A — refator estrutural puro. Nenhuma entidade nova; o `entity-registry.json` será regenerado pelo `sync-registry` no fim do CLOSE da wave.

## Tarefas

### Wave 4 — Refator estrutural (ui, model: opus)

#### Fase A — Preparação (sem mudança de filesystem ainda)

- [ ] Read completo de `apps/dashboard/src/components/{page,layout,ui}/index.ts` (se existir) — entender exports atuais para preservar API.
- [ ] Glob `apps/dashboard/src/components/**/*.tsx` para confirmar contagem total (~110 + 10 strays).
- [ ] Read `apps/dashboard/tsconfig.json` e `apps/dashboard/vite.config.ts` — identificar onde adicionar `@/features` se necessário.
- [ ] Grep `acorn-jsx` em `apps/dashboard/package.json` + `node_modules/.pnpm` — confirmar disponibilidade. Se ausente: `pnpm --filter mustard-dashboard add -D acorn acorn-jsx`.

#### Fase B — Scripts (criar antes de mover)

- [ ] Criar `scripts/refactor-folder-per-component.mjs` conforme contrato acima. Suportar `--dry-run` (imprime plano, não escreve).
- [ ] Criar `scripts/check-pages-no-inline-visual.mjs` conforme contrato acima.
- [ ] `node scripts/refactor-folder-per-component.mjs --dry-run` — inspecionar plano. Deve listar ~110 moves + ~300 import rewrites (estimativa: cada page importa 5-10 componentes; 11 pages).
- [ ] `node scripts/check-pages-no-inline-visual.mjs apps/dashboard/src/pages` — confirmar que o script EXECUTA (pode retornar non-zero contra pages atuais; isso é OK para Wave 4 e esperado).

#### Fase C — Path alias

- [ ] `apps/dashboard/tsconfig.json`: adicionar `"@/features/*": ["./src/features/*"]` no objeto `paths`.
- [ ] `apps/dashboard/vite.config.ts`: confirmar que `resolve.alias["@"]` aponta para `src` (já funciona transitivamente para `@/features`). Adicionar entrada explícita só se Vite reclamar no build.

#### Fase D — Codemod real

- [ ] `node scripts/refactor-folder-per-component.mjs` (sem `--dry-run`). Capturar stdout (lista de moves + rewrites).
- [ ] `rtk pnpm --filter mustard-dashboard build` — verde imediato (sem este passo a wave reverte).

#### Fase E — Pós-codemod

- [ ] Atualizar `apps/dashboard/.claude/skills/dashboard-page-primitives/SKILL.md`:
  - Inventário aponta para `components/page/{Name}/index.tsx`.
  - Regra "import sempre `@/components/page`, nunca arquivo individual" continua válida (barrel re-exporta).
  - Adicionar seção "Domain features" listando `features/{specs,workspace,economy,knowledge,prd,telemetry,amend,trace}` com regra "pages importam de `@/features/{name}` (granular ou agregado)".
- [ ] Atualizar `apps/dashboard/.claude/CLAUDE.md`:
  - Documentar regra "folder-per-component em todo `components/**` e `features/**`".
  - Listar features/ como onde lógica de domínio mora.
- [ ] `rtk git status` — confirmar mudanças (~120 moves + ~50 import edits + 5 novos scripts/configs).
- [ ] `rtk pnpm --filter mustard-dashboard build` — re-rodar (sanity).
- [ ] `node scripts/check-pages-imports.mjs apps/dashboard/src/pages` — AC-6 do parent. Provavelmente AINDA falhará porque pages podem importar `@/components/{specs,workspace,...}` legacy — mas estes paths não existem mais, então pages só importam de `@/features/*` agora. O script de check-pages-imports talvez precise atualizar (CRITICAL pro spec do parent: confirmar). Se o script atual de Wave 1 só procura `@/components/ds`, ele já passa. Confirmar no fonte e atualizar se necessário.

## Dependências

- Wave 3 entregou layout shell estável — `AppShell`, `Sidebar`, `Topbar`, `SplitDetail`, `button` ficam onde estão (movem para subpastas, sem retoque de conteúdo).
- Novos pacotes: `acorn` + `acorn-jsx` como devDeps se ausentes (confirmar via Read de `pnpm-lock.yaml`; se já transitive, sem add).
- Nenhuma dependência runtime nova.

## Limites

Editar dentro de:
- `apps/dashboard/src/components/**/*` (move + create folders + create barrels)
- `apps/dashboard/src/features/**/*` (criação de pastas)
- `apps/dashboard/src/pages/*.tsx` (apenas import paths)
- `apps/dashboard/src/{App.tsx,main.tsx}` (apenas import paths se necessário)
- `apps/dashboard/{tsconfig.json,vite.config.ts}` (path alias)
- `apps/dashboard/package.json` (devDeps de codemod, se necessário)
- `apps/dashboard/.claude/{CLAUDE.md,skills/dashboard-page-primitives/SKILL.md}`
- `scripts/refactor-folder-per-component.mjs` (novo)
- `scripts/check-pages-no-inline-visual.mjs` (novo)

**Não tocar**:
- `apps/dashboard/src/{api,hooks,lib,data,assets,styles}/**` (sem alteração estrutural; imports rewriting OK mas só se necessário)
- `apps/dashboard/src/style.css`, `i18n.ts`
- `apps/dashboard/src-tauri/**`
- Qualquer arquivo fora de `apps/dashboard/` exceto `scripts/refactor-folder-per-component.mjs` e `scripts/check-pages-no-inline-visual.mjs`

## Critérios de Aceitação

- [x] AC-W4-1: dashboard build passa após refator — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-W4-2: zero arquivos `.tsx` flat em `apps/dashboard/src/components/specs|workspace|economy|knowledge|prd|telemetry|amend|trace/` (todos viraram pasta) — Command: `node -e "const fs=require('fs');const p=require('path');const dirs=['specs','workspace','economy','knowledge','prd','telemetry','amend','trace'];let leak=0;for(const d of dirs){const root=p.join('apps/dashboard/src/components',d);if(fs.existsSync(root)){const entries=fs.readdirSync(root,{withFileTypes:true});for(const e of entries){if(e.isFile()&&e.name.endsWith('.tsx')){console.error('leak:',p.join(root,e.name));leak++}}}}if(leak)process.exit(1);console.log('ok')"`
- [x] AC-W4-3: features/ existe com 8 subpastas e cada componente é uma pasta com `index.tsx` — Command: `node -e "const fs=require('fs');const p=require('path');const dirs=['specs','workspace','economy','knowledge','prd','telemetry','amend','trace'];for(const d of dirs){const root=p.join('apps/dashboard/src/features',d);if(!fs.existsSync(root)){console.error('missing feature dir:',root);process.exit(1)}const entries=fs.readdirSync(root,{withFileTypes:true});const compFolders=entries.filter(e=>e.isDirectory()&&!e.name.startsWith('_')&&e.name!=='__tests__');if(!compFolders.length){console.error('no component folders in',root);process.exit(2)}for(const cf of compFolders){const idx=p.join(root,cf.name,'index.tsx');if(!fs.existsSync(idx)){console.error('missing index.tsx in',cf.name);process.exit(3)}}}console.log('ok')"`
- [x] AC-W4-4: shared `components/{page,layout,ui}` cada componente em pasta com `index.tsx` (lowercase preservado p/ shadcn ui) — Command: `node -e "const fs=require('fs');const p=require('path');const dirs=['page','layout','ui'];for(const d of dirs){const root=p.join('apps/dashboard/src/components',d);const entries=fs.readdirSync(root,{withFileTypes:true});const flat=entries.filter(e=>e.isFile()&&e.name.endsWith('.tsx'));if(flat.length){console.error('flat .tsx in',root,':',flat.map(f=>f.name).join(','));process.exit(1)}const folders=entries.filter(e=>e.isDirectory());for(const f of folders){const idx=p.join(root,f.name,'index.tsx');if(!fs.existsSync(idx)){console.error('missing index.tsx in',f.name);process.exit(2)}}}console.log('ok')"`
- [x] AC-W4-5: 10 strays no root de `components/` foram realocados e o root só contém subpastas — Command: `node -e "const fs=require('fs');const entries=fs.readdirSync('apps/dashboard/src/components',{withFileTypes:true});const stray=entries.filter(e=>e.isFile()&&(e.name.endsWith('.tsx')||e.name.endsWith('.ts')));if(stray.length){console.error('strays remain:',stray.map(s=>s.name).join(','));process.exit(1)}console.log('ok')"`
- [x] AC-W4-6: zero imports antigos `@/components/{specs|workspace|economy|knowledge|prd|telemetry|amend|trace}/` em qualquer arquivo `src/**` — Command: `node -e "const fs=require('fs');const p=require('path');const needles=['specs','workspace','economy','knowledge','prd','telemetry','amend','trace'].map(d=>'@/components/'+d+'/');const root='apps/dashboard/src';const exts=['.tsx','.ts','.jsx','.js','.mjs','.cjs'];const hits=[];function walk(d){for(const e of fs.readdirSync(d,{withFileTypes:true})){if(e.name==='node_modules'||e.name==='.git'||e.name==='dist')continue;const f=p.join(d,e.name);if(e.isDirectory())walk(f);else if(exts.some(x=>e.name.endsWith(x))){const c=fs.readFileSync(f,'utf8');if(needles.some(n=>c.includes(n)))hits.push(f)}}}walk(root);if(hits.length){console.error('legacy imports remain:\\n'+hits.join('\\n'));process.exit(1)}console.log('ok')"`
- [x] AC-W4-7: `scripts/check-pages-no-inline-visual.mjs` existe e é executável — Command: `node -e "const fs=require('fs');if(!fs.existsSync('scripts/check-pages-no-inline-visual.mjs'))process.exit(1);const c=fs.readFileSync('scripts/check-pages-no-inline-visual.mjs','utf8');if(c.length<400)process.exit(2);if(!/process\.exit/.test(c))process.exit(3);console.log('ok')"`
- [x] AC-W4-8: zero referências a tokens fantasmas `--color-ok` e `--color-accent-mustard` em `apps/dashboard/src/{features,components}/` (escope da wave: features/+components/; pages/ residual em Wave 5/6) — Command: `node -e "const fs=require('fs');const p=require('path');const pa=/--color-ok\\b/;const pb=/--color-accent-mustard\\b/;const roots=['apps/dashboard/src/features','apps/dashboard/src/components'];const exts=['.tsx','.ts','.jsx','.js','.mjs','.cjs','.css'];const hits={ok:[],mustard:[]};function walk(d){if(!fs.existsSync(d))return;for(const e of fs.readdirSync(d,{withFileTypes:true})){if(e.name==='node_modules'||e.name==='.git'||e.name==='dist')continue;const f=p.join(d,e.name);if(e.isDirectory())walk(f);else if(exts.some(x=>e.name.endsWith(x))){const c=fs.readFileSync(f,'utf8');if(pa.test(c))hits.ok.push(f);if(pb.test(c))hits.mustard.push(f)}}}for(const r of roots)walk(r);if(hits.ok.length||hits.mustard.length){console.error('phantom tokens remain:\\n--color-ok:\\n'+hits.ok.join('\\n')+'\\n--color-accent-mustard:\\n'+hits.mustard.join('\\n'));process.exit(1)}console.log('ok')"`
- [x] AC-W4-9: zero `text-red-(400|500|600|700)` ou `bg-red-(400|500|600|700)` Tailwind raw em `apps/dashboard/src/{features,components}/` — Command: `node -e "const fs=require('fs');const p=require('path');const pat=/(text|bg)-red-(400|500|600|700)\\b/;const roots=['apps/dashboard/src/features','apps/dashboard/src/components'];const exts=['.tsx','.ts','.jsx','.js','.mjs','.cjs'];const hits=[];function walk(d){if(!fs.existsSync(d))return;for(const e of fs.readdirSync(d,{withFileTypes:true})){if(e.name==='node_modules'||e.name==='.git'||e.name==='dist')continue;const f=p.join(d,e.name);if(e.isDirectory())walk(f);else if(exts.some(x=>e.name.endsWith(x))){if(pat.test(fs.readFileSync(f,'utf8')))hits.push(f)}}}for(const r of roots)walk(r);if(hits.length){console.error('raw red classes remain:\\n'+hits.join('\\n'));process.exit(1)}console.log('ok')"`
- [x] AC-W4-10: codemod script é idempotente (re-rodar produz zero alterações) — Command: `node -e "const {execSync}=require('child_process');const before=execSync('git status --short',{encoding:'utf8'});execSync('node scripts/refactor-folder-per-component.mjs',{encoding:'utf8'});const after=execSync('git status --short',{encoding:'utf8'});if(before!==after){console.error('codemod is not idempotent:\\nbefore=\\n'+before+'\\nafter=\\n'+after);process.exit(1)}console.log('ok')"`

## Checklist

- [x] `pnpm --filter mustard-dashboard build` verde
- [x] `node scripts/refactor-folder-per-component.mjs` idempotente
- [x] 8 dirs em `features/`, cada um com N pastas `Component/index.tsx`
- [x] 3 dirs em `components/` (page, layout, ui) também folder-per-component
- [x] 10 strays do root de `components/` realocados
- [x] Zero imports `@/components/{specs|workspace|...}` em src
- [x] Zero `--color-ok`, `--color-accent-mustard`, `text-red-400` em src (features+components; pages residual em W5/W6)
- [x] `scripts/check-pages-no-inline-visual.mjs` criado e executável
- [x] `tsconfig.json` com `@/features/*` path
- [x] SKILL.md e CLAUDE.md do dashboard apontam para a nova estrutura
- [x] `git log` mostra moves via `git mv` (history preservada)
