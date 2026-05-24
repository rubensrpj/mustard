# Wave 6 — Pages secondary (ProjectDetail, SpecDetail, Prd, Commands, Settings, Preferences, Home)

## Resumo

Replicar o padrão validado na Wave 5 nas 7 páginas secundárias do dashboard. Cada uma compõe `<PageSurface>` + `<EditorialBand>` (quando faz sentido — `Preferences` e `Home` podem ter aberturas mais discretas), consome primitivas do barril `@/components/page`, importa features via `@/features/{name}`, e zera resíduo visual inline. Mesma regra HARD da Wave 5: pages NÃO inventam visual; só layout estrutural + composição. Este é o fechamento do wave plan: ao fim, `check-pages-no-inline-visual.mjs` passa em TODAS as 11 páginas, `check-pages-imports.mjs` confirma zero imports legacy, e AC-6 + AC-10 do parent finalmente ficam verdes — destravando o CLOSE do wave plan inteiro.

## Network

- Parent: [[2026-05-23-dashboard-design-system]]
- Depende de: [[wave-5-ui]] (padrão `<PageSurface>` + `<EditorialBand>` validado em 4 páginas alto-tráfego; check-pages-no-inline-visual já passa lá)
- Habilita: CLOSE do wave plan parent (com AC-6 e AC-10 finalmente verdes)

## Component Contract

Idêntico ao da Wave 5 — não repetir. Ver `[[wave-5-ui]] § Component Contract` para o padrão `<PageSurface>` + `<EditorialBand>` + composição de primitivas + proibição de inline visual.

**Especificidades por página:**

| Page | Tamanho atual | Herói | Observações |
|---|---|---|---|
| `ProjectDetail.tsx` | 12.4K | `<EditorialBand>` com nome do projeto + status + última atividade | Tabs de detalhe usam `<TabBar>` (shadcn) — sem mudança de comportamento |
| `SpecDetail.tsx` | 5.1K | `<EditorialBand>` com spec name + stage + outcome chips | Já é split-pane; `<PageSurface editorial={false}>` se o split-pane lida com ritmo |
| `Prd.tsx` | 23.1K | `<EditorialBand>` "PRD Builder" + eyebrow contextual | Formulário pesado; manter campos como estão (já em `features/prd`) |
| `Commands.tsx` | 9.4K | `<EditorialBand>` "Commands" | Lista de comandos via `<DataCard><DataRow>` |
| `Settings.tsx` | 8.8K | `<EditorialBand>` "Settings" | Per-project; agrupar campos em `<DataCard>` |
| `Preferences.tsx` | 1.4K | Aberturas discretas — `<EditorialBand>` com `editorial={false}` ou só `<EditorialTitle>` solto | Dashboard-global; pequena |
| `Home.tsx` | 7.8K | `<EditorialBand>` com brand mark + tagline | Página de entrada; pode ter mais respiração |

## Arquivos

- `apps/dashboard/src/pages/ProjectDetail.tsx`
- `apps/dashboard/src/pages/SpecDetail.tsx`
- `apps/dashboard/src/pages/Prd.tsx`
- `apps/dashboard/src/pages/Commands.tsx`
- `apps/dashboard/src/pages/Settings.tsx`
- `apps/dashboard/src/pages/Preferences.tsx`
- `apps/dashboard/src/pages/Home.tsx`

## Informações da Entidade

N/A — refator de páginas.

## Tarefas

### Wave 6 — Pages secondary (ui, model: opus)

Aplique o mesmo padrão da Wave 5 a CADA página da lista, na ordem:

- [ ] `ProjectDetail.tsx`: wrapper → `<PageSurface>`; header → `<EditorialBand>` (eyebrow=projeto pai/parent crumb, title=projectName, subtitle=status+activity); imports `@/components/specs/X` → `@/features/specs/X`; primitivas KPI/Delta consumidas; sweep inline visual.
- [ ] `SpecDetail.tsx`: avaliar `<PageSurface editorial={false}>` (split-pane interno cuida do ritmo); herói pode ser `<EditorialBand>` compacto OU `<SectionHeader>` direto; `<SpecCard>`/`<SpecMarkdownViewer>`/`<SpecTabBar>` de `@/features/specs/`.
- [ ] `Prd.tsx`: `<PageSurface>` + `<EditorialBand>` "PRD Builder"; formulário em `<DataCard>` agrupado por seção; `<IntentHero>`/`<EditableList>`/`<EntityPicker>` de `@/features/prd/`.
- [ ] `Commands.tsx`: `<PageSurface>` + `<EditorialBand>` "Commands"; lista em `<DataCard><DataRow>` (cada row = comando: lead=ícone, primary=name, meta=description, trailing=shortcut em `<StatPill>`).
- [ ] `Settings.tsx`: `<PageSurface>` + `<EditorialBand>` "Settings"; grupos de config em `<DataCard>` separados; toggles/inputs já são de `@/components/ui`.
- [ ] `Preferences.tsx`: `<PageSurface>` + título simples (`<EditorialTitle>` solto ou `<EditorialBand editorial={false}>`); curtinha por design.
- [ ] `Home.tsx`: `<PageSurface>` + `<EditorialBand>` com `<BrandMark>` opcionalmente; cards/links de navegação como `<DataCard>` ou `<DataRow>`.

#### Validação final

- [ ] `rtk pnpm --filter mustard-dashboard build` verde.
- [ ] `node scripts/check-pages-no-inline-visual.mjs apps/dashboard/src/pages` (sem args específicos — varre TODAS as 11 páginas) retorna 0.
- [ ] `node scripts/check-pages-imports.mjs apps/dashboard/src/pages` retorna 0.
- [ ] Visual smoke: `rtk pnpm --filter mustard-dashboard dev` → abrir as 7 rotas; confirmar canvas escuro + herói editorial + consistência com as 4 da Wave 5.
- [ ] AC-6 do parent passa (`check-pages-imports` verde nas 11).
- [ ] AC-10 do parent passa (`check-pages-no-inline-visual` verde nas 11).

## Dependências

- Wave 4 entregou: estrutura `features/*` + scripts.
- Wave 5 validou: `<PageSurface>` + `<EditorialBand>` padrão em 4 páginas alto-tráfego.
- Sem nova dependência npm.

## Limites

Editar dentro de:
- `apps/dashboard/src/pages/{ProjectDetail,SpecDetail,Prd,Commands,Settings,Preferences,Home}.tsx`

**Não tocar**:
- As 4 páginas da Wave 5 (Workspace/Specs/Economia/Knowledge) — já feitas, intactas
- `apps/dashboard/src/{features,components}/**` — Waves 2-4 estabilizaram. Se uma página precisar de primitiva nova, surface como `mustard:tactical-fix` sub-spec, NÃO inventa inline.
- `apps/dashboard/src/style.css`, hooks, lib, api
- `apps/dashboard/src-tauri/**`
- Scripts em `scripts/`

## Critérios de Aceitação

- [ ] AC-W6-1: dashboard build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W6-2: cada uma das 7 páginas secundárias tem `<PageSurface>` no JSX raiz — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/ProjectDetail.tsx','apps/dashboard/src/pages/SpecDetail.tsx','apps/dashboard/src/pages/Prd.tsx','apps/dashboard/src/pages/Commands.tsx','apps/dashboard/src/pages/Settings.tsx','apps/dashboard/src/pages/Preferences.tsx','apps/dashboard/src/pages/Home.tsx'];for(const f of files){const c=fs.readFileSync(f,'utf8');if(!/<PageSurface[\s>]/.test(c)){console.error('missing PageSurface in',f);process.exit(1)}}console.log('ok')"`
- [ ] AC-W6-3: check-pages-no-inline-visual passa nas 7 páginas secundárias — Command: `node scripts/check-pages-no-inline-visual.mjs apps/dashboard/src/pages/ProjectDetail.tsx apps/dashboard/src/pages/SpecDetail.tsx apps/dashboard/src/pages/Prd.tsx apps/dashboard/src/pages/Commands.tsx apps/dashboard/src/pages/Settings.tsx apps/dashboard/src/pages/Preferences.tsx apps/dashboard/src/pages/Home.tsx`
- [ ] AC-W6-4: zero `style={{` com propriedade visual nas 7 páginas — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/ProjectDetail.tsx','apps/dashboard/src/pages/SpecDetail.tsx','apps/dashboard/src/pages/Prd.tsx','apps/dashboard/src/pages/Commands.tsx','apps/dashboard/src/pages/Settings.tsx','apps/dashboard/src/pages/Preferences.tsx','apps/dashboard/src/pages/Home.tsx'];const visual=/style\s*=\s*\{\{[^}]*\b(color|background|backgroundColor|border|borderColor|borderRadius|boxShadow)\s*:/;for(const f of files){const c=fs.readFileSync(f,'utf8');if(visual.test(c)){console.error('inline visual style in',f);process.exit(1)}}console.log('ok')"`
- [ ] AC-W6-5: zero hex literal nas 7 páginas — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/ProjectDetail.tsx','apps/dashboard/src/pages/SpecDetail.tsx','apps/dashboard/src/pages/Prd.tsx','apps/dashboard/src/pages/Commands.tsx','apps/dashboard/src/pages/Settings.tsx','apps/dashboard/src/pages/Preferences.tsx','apps/dashboard/src/pages/Home.tsx'];const hex=/['\"\\\`]#[0-9a-fA-F]{3,8}['\"\\\`]/;for(const f of files){const c=fs.readFileSync(f,'utf8');if(hex.test(c)){console.error('hex literal in',f);process.exit(1)}}console.log('ok')"`
- [ ] AC-W6-6: zero classes Tailwind de cor raw nas 7 páginas — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/ProjectDetail.tsx','apps/dashboard/src/pages/SpecDetail.tsx','apps/dashboard/src/pages/Prd.tsx','apps/dashboard/src/pages/Commands.tsx','apps/dashboard/src/pages/Settings.tsx','apps/dashboard/src/pages/Preferences.tsx','apps/dashboard/src/pages/Home.tsx'];const bad=/\\b(text|bg|border|ring|fill|stroke)-(red|amber|emerald|blue|indigo|violet|fuchsia|pink|cyan|teal|lime|green|yellow|orange|rose|sky|slate|zinc|gray|neutral|stone)-(50|100|200|300|400|500|600|700|800|900|950)\\b/;for(const f of files){const c=fs.readFileSync(f,'utf8');const m=c.match(bad);if(m){console.error('raw color class in',f,':',m[0]);process.exit(1)}}console.log('ok')"`
- [ ] AC-W6-7: AC-6 do parent (check-pages-imports) passa nas 11 páginas — Command: `node scripts/check-pages-imports.mjs apps/dashboard/src/pages`
- [ ] AC-W6-8: AC-10 do parent (check-pages-no-inline-visual) passa nas 11 páginas — Command: `node scripts/check-pages-no-inline-visual.mjs apps/dashboard/src/pages`

## Checklist

- [x] Build verde
- [x] 7 páginas com `<PageSurface>` + `<EditorialBand>` (ou alternativa documentada)
- [x] Zero inline visual em todas
- [x] Imports só `@/features/*` (domínio) e `@/components/{page,layout,ui}` (shared)
- [x] check-pages-no-inline-visual.mjs verde em TODAS as 11 páginas
- [x] check-pages-imports.mjs verde em TODAS as 11 páginas
- [x] AC-6 e AC-10 do parent finalmente passam — destrava CLOSE do wave plan
- [x] Visual smoke OK nas 7 rotas
