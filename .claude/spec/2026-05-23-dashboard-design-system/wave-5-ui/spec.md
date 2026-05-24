# Wave 5 — Pages high-traffic (Workspace, Specs, Economia, Knowledge)

### Parent: [[2026-05-23-dashboard-design-system]]
### Stage: Close
### Outcome: Completed
### Flags:
### Scope: full (wave 5 of 6)
### Lang: pt
### Checkpoint: 2026-05-23T22:30:00Z

## Resumo

Migrar as 4 páginas de maior tráfego para o padrão Binance consolidado pelas Waves 1-4. Cada página passa a (a) compor `<PageSurface>` em vez de wrapper hand-rolled, (b) abrir com `<EditorialBand>` para o herói (eyebrow + título + subtítulo + ações), (c) consumir primitivas do barril `@/components/page` (KPI numerics, StatPill, DeltaText, DataCard, DataRow), (d) importar features de domínio via `@/features/{specs,workspace,economy,knowledge}` (caminho novo da Wave 4), (e) eliminar QUALQUER hex hardcoded, classe Tailwind de cor (`text-{cor}`, `bg-{cor}`, `border-{cor}`), radius (`rounded-*`), elevação (`shadow-*`) ou `style={{...}}` visual residual nas páginas — só layout estrutural (`grid/flex/gap/w/h/max-w/col-span`) e composição de primitivas. Não é refactor de feature; cada componente de domínio continua onde está (em `features/*`). É refactor de **página** — a página consome, não inventa visual. Métrica concreta: `node scripts/check-pages-no-inline-visual.mjs apps/dashboard/src/pages/{Workspace,Specs,Economia,Knowledge}.tsx` retorna 0 (Wave 4 criou o script; aqui ele finalmente passa nas 4 páginas).

## Network

- Parent: [[2026-05-23-dashboard-design-system]]
- Depende de: [[wave-4-ui]] (`@/features/{specs,workspace,economy,knowledge}` existem; `<PageSurface>`/`<EditorialBand>`/`<DeltaText>`/`<StatPill>` no barril `@/components/page`)
- Habilita: [[wave-6-ui]] (padrão validado nas 4 high-traffic; secundárias replicam)

## Component Contract

Padrão único para cada página da wave:

```tsx
import { PageSurface, EditorialBand, EditorialEyebrow, EditorialTitle, EditorialSubtitle, KPIRow, KpiValue, KpiLabel, StatPill, DeltaText, DataCard, DataRow, EmptyState } from "@/components/page";
import { WorkspaceHealthCard, WorkspaceEventsFeed /* etc — granular */ } from "@/features/workspace";
// ou import * as workspace from "@/features/workspace"; (não preferido — perde tree-shaking facial)

export function Workspace() {
  return (
    <PageSurface>
      <EditorialBand
        eyebrow={<EditorialEyebrow>Workspace</EditorialEyebrow>}
        title={<EditorialTitle>{workspaceName}</EditorialTitle>}
        subtitle={<EditorialSubtitle>{subtitle}</EditorialSubtitle>}
        actions={<Button>Add project</Button>}
      />
      <KPIRow>{/* 4 KPICards */}</KPIRow>
      <section className="grid grid-cols-2 gap-8">
        <WorkspaceHealthCard />
        <WorkspaceEventsFeed />
      </section>
    </PageSurface>
  );
}
```

**Inegociável por página:**
- Wrapper sempre `<PageSurface>` (não `<div className="flex flex-col gap-X">`).
- Abertura sempre `<EditorialBand>` (mesmo que o herói seja só o título — `subtitle` opcional).
- Imports de domínio sempre `@/features/{name}` — granular preferível, agregado tolerado.
- Layout structural via Tailwind cru OK (`grid`, `flex`, `gap-8`, `w-full`, `max-w-screen-2xl`, `col-span-2`).
- **PROIBIDO** dentro de `.tsx` de page: hex literal, `text-{red|green|amber|blue|...}-{N}`, `bg-{cor}-{N}`, `border-{cor}-{N}`, `rounded-{sm|md|lg|...}`, `shadow-{sm|md|lg|...}`, `style={{ color/background/border/borderRadius/boxShadow }}`. Se a página precisa de uma combinação visual não-coberta pelas primitivas, surfaceie como BLOCKED ou cria nova primitiva em `components/page/` em uma sub-spec dedicada — NÃO escapa para classe raw.

## Arquivos

- `apps/dashboard/src/pages/Workspace.tsx`
- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/pages/Economia.tsx`
- `apps/dashboard/src/pages/Knowledge.tsx`

## Informações da Entidade

N/A — refactor de páginas, sem entidade nova.

## Tarefas

### Wave 5 — Pages high-traffic (ui, model: opus)

#### Workspace.tsx (5.3K hoje)

- [ ] Read arquivo atual; mapear wrappers hand-rolled (`<div className="flex flex-col gap-X">` etc.) para substituição por `<PageSurface>`.
- [ ] Substituir wrapper raiz por `<PageSurface>`.
- [ ] Identificar herói (título + subtítulo + CTAs). Migrar para `<EditorialBand>` com slots `eyebrow`/`title`/`subtitle`/`actions`.
- [ ] KPI grid (se houver) → `<KPIRow>` envolvendo `<KPICard>` (já existente, agora composto via slots `label`/`value`/`hint`).
- [ ] Deltas numéricos → `<DeltaText value={n} format="pct|abs" />`.
- [ ] Listas tipo "by status" / "events feed" → `<DataCard>` envolvendo `<DataRow>` (compostos por `lead`/`primary`/`meta`/`trailing`).
- [ ] Imports `@/components/workspace/X` → `@/features/workspace/X` (provavelmente já foi feito pelo codemod da Wave 4 — confirmar via Grep).
- [ ] Eliminar QUALQUER `text-red-*`, `bg-amber-*`, `border-emerald-*`, hex hardcoded, `style={{...}}` visual.
- [ ] Substituir classes Tailwind de cor remanescentes por composição (e.g., status colorido vira `<StatusDot status="ok|warn|error" />` que internamente mapeia para `--intent-success`/`--intent-warning`/`--intent-error`).

#### Specs.tsx (21.0K hoje)

- [ ] `<PageSurface>` no root; `<EditorialBand>` no herói (header "Specs").
- [ ] Filtros / scope bar permanecem (já em `features/specs/` ou agora `features/economy/ScopeBar`); apenas trocar import path.
- [ ] Lista de specs → consumir `<SpecsList>` (movida para `features/specs/SpecsList`).
- [ ] Cards de spec → `<SpecCard>` (`features/specs/SpecCard`) — sem alteração interna.
- [ ] Phantom token sweep IN-PAGE: trocar qualquer `text-red-*`, `bg-amber-*` no JSX da page por composição via primitiva.
- [ ] Eliminar classes Tailwind de cor cru; deltas via `<DeltaText>`.

#### Economia.tsx (30.3K hoje — maior página)

- [ ] `<PageSurface>` + `<EditorialBand>` no header.
- [ ] KPI numerics em `<KpiValue>` mono tabular; labels em `<KpiLabel>`; hints em `<KpiHint>`.
- [ ] Deltas de custo em `<DeltaText format="abs" />` (verde/vermelho via tokens `--intent-success`/`--intent-error`).
- [ ] Pílulas numéricas → `<StatPill>` (já renomeado de MetricsPill na Wave 2).
- [ ] Barras horizontais (custo por agente/spec) → `<CostBar>` (label + `<BarTrack><BarFill intent="primary|accent" /></BarTrack>` + valor mono).
- [ ] Legendas → `<LegendSwatch>` (Wave 2 entregou).
- [ ] Tabelas → `<DataCard><DataRow>` em vez de `<table>` cru OU `<table>` mantida desde que tds usem só classes de layout (grid/flex/gap).
- [ ] Sweep phantom tokens; deletar TODO `style={{ color/background/border }}`.

#### Knowledge.tsx (18.3K hoje)

- [ ] `<PageSurface>` + `<EditorialBand>`.
- [ ] Cards de knowledge → `<KnowledgeCard>` (movida para `features/knowledge/KnowledgeCard` pela Wave 4) consumida pela page.
- [ ] Badges → `<KnowledgeBadge>` (`features/knowledge/KnowledgeBadge`) ou `<PhaseChip>`/`<EventChip>` shared se aplicável.
- [ ] Tabelas/listas → `<DataCard><DataRow>`.
- [ ] Sweep phantom tokens; deletar inline visual.

#### Validação

- [ ] `rtk pnpm --filter mustard-dashboard build` verde.
- [ ] `node scripts/check-pages-no-inline-visual.mjs apps/dashboard/src/pages/Workspace.tsx apps/dashboard/src/pages/Specs.tsx apps/dashboard/src/pages/Economia.tsx apps/dashboard/src/pages/Knowledge.tsx` retorna 0.
- [ ] `node scripts/check-pages-imports.mjs apps/dashboard/src/pages` retorna 0 (nenhuma das 4 importa do barril deletado `@/components/ds`).
- [ ] Visual smoke: `rtk pnpm --filter mustard-dashboard dev` → abrir cada uma das 4 rotas; confirmar canvas escuro, herói editorial 80px, KPIs em mono, deltas verde/vermelho Binance, CTA amarelo se houver.

## Dependências

- Wave 4 entregou: `@/features/{workspace,specs,economy,knowledge}` (caminhos novos), `scripts/check-pages-no-inline-visual.mjs`, phantom tokens já eliminados nos arquivos de `features/*` e `components/{page,layout,ui}/*`.
- Wave 2 entregou as primitivas `<PageSurface>`, `<EditorialBand>`, `<KpiValue>`, `<StatPill>`, `<DeltaText>`, `<DataRow>`, `<CostBar>`, `<LegendSwatch>` (todas em `@/components/page` — barrel re-export inclui o conteúdo de cada pasta após Wave 4).
- Sem nova dependência npm.

## Limites

Editar dentro de:
- `apps/dashboard/src/pages/{Workspace,Specs,Economia,Knowledge}.tsx`

**Não tocar**:
- Qualquer outra página (`ProjectDetail`, `SpecDetail`, `Prd`, `Commands`, `Settings`, `Preferences`, `Home`) — Wave 6
- `apps/dashboard/src/{features,components}/**` — Wave 4 estabilizou; aqui só consome. Se precisar de primitiva nova, surface como sub-spec tactical-fix em `<EditorialBand>` etc., NÃO criar inline.
- `apps/dashboard/src/style.css`, `apps/dashboard/src/{api,hooks,lib,data}/**`
- `apps/dashboard/src-tauri/**`
- Scripts em `scripts/`

## Critérios de Aceitação

- [ ] AC-W5-1: dashboard build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W5-2: cada uma das 4 páginas tem `<PageSurface>` no JSX raiz — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/Workspace.tsx','apps/dashboard/src/pages/Specs.tsx','apps/dashboard/src/pages/Economia.tsx','apps/dashboard/src/pages/Knowledge.tsx'];for(const f of files){const c=fs.readFileSync(f,'utf8');if(!/<PageSurface[\s>]/.test(c)){console.error('missing PageSurface in',f);process.exit(1)}}console.log('ok')"`
- [ ] AC-W5-3: cada uma das 4 páginas usa `<EditorialBand>` para o herói — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/Workspace.tsx','apps/dashboard/src/pages/Specs.tsx','apps/dashboard/src/pages/Economia.tsx','apps/dashboard/src/pages/Knowledge.tsx'];for(const f of files){const c=fs.readFileSync(f,'utf8');if(!/<EditorialBand[\s>]/.test(c)){console.error('missing EditorialBand in',f);process.exit(1)}}console.log('ok')"`
- [ ] AC-W5-4: `check-pages-no-inline-visual.mjs` passa nas 4 páginas-alvo da Wave 5 (Workspace/Specs/Economia/Knowledge); demais páginas ficam fora deste critério (cobertas pela Wave 6) — Command: `node -e "const{execSync}=require('child_process');let o='';try{o=execSync('node scripts/check-pages-no-inline-visual.mjs apps/dashboard/src/pages',{encoding:'utf8'})}catch(e){o=(e.stdout||'')+(e.stderr||'')}const t=['Workspace.tsx','Specs.tsx','Economia.tsx','Knowledge.tsx'];for(const p of t){if(o.includes(p)){console.error('violation in target page:',p);process.exit(1)}}console.log('ok')"`
- [ ] AC-W5-5: zero `style={{` com propriedade visual em qualquer das 4 páginas — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/Workspace.tsx','apps/dashboard/src/pages/Specs.tsx','apps/dashboard/src/pages/Economia.tsx','apps/dashboard/src/pages/Knowledge.tsx'];const visual=/style\s*=\s*\{\{[^}]*\b(color|background|backgroundColor|border|borderColor|borderRadius|boxShadow)\s*:/;for(const f of files){const c=fs.readFileSync(f,'utf8');if(visual.test(c)){console.error('inline visual style in',f);process.exit(1)}}console.log('ok')"`
- [ ] AC-W5-6: zero hex literal `#[0-9a-f]{3,8}` em string nas 4 páginas — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/Workspace.tsx','apps/dashboard/src/pages/Specs.tsx','apps/dashboard/src/pages/Economia.tsx','apps/dashboard/src/pages/Knowledge.tsx'];const hex=/['\"\\\`]#[0-9a-fA-F]{3,8}['\"\\\`]/;for(const f of files){const c=fs.readFileSync(f,'utf8');if(hex.test(c)){console.error('hex literal in',f);process.exit(1)}}console.log('ok')"`
- [ ] AC-W5-7: zero classes Tailwind de cor (`text-red-`, `bg-amber-`, `border-emerald-`, etc., exceto whitelist) nas 4 páginas — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/Workspace.tsx','apps/dashboard/src/pages/Specs.tsx','apps/dashboard/src/pages/Economia.tsx','apps/dashboard/src/pages/Knowledge.tsx'];const bad=/\\b(text|bg|border|ring|fill|stroke)-(red|amber|emerald|blue|indigo|violet|fuchsia|pink|cyan|teal|lime|green|yellow|orange|rose|sky|slate|zinc|gray|neutral|stone)-(50|100|200|300|400|500|600|700|800|900|950)\\b/;for(const f of files){const c=fs.readFileSync(f,'utf8');const m=c.match(bad);if(m){console.error('raw color class in',f,':',m[0]);process.exit(1)}}console.log('ok')"`
- [ ] AC-W5-8: zero imports antigos `@/components/{specs|workspace|economy|knowledge}/` nas 4 páginas (todos via `@/features/*`) — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/Workspace.tsx','apps/dashboard/src/pages/Specs.tsx','apps/dashboard/src/pages/Economia.tsx','apps/dashboard/src/pages/Knowledge.tsx'];const bad=/@\/components\/(specs|workspace|economy|knowledge)\//;for(const f of files){const c=fs.readFileSync(f,'utf8');if(bad.test(c)){console.error('legacy import in',f);process.exit(1)}}console.log('ok')"`

## Checklist

- [ ] Build passa
- [ ] Cada uma das 4 páginas: `<PageSurface>` + `<EditorialBand>` + composição de primitivas
- [ ] Zero inline visual, zero hex, zero classes Tailwind de cor cru
- [ ] Imports só de `@/features/*` (domínio) e `@/components/{page,layout,ui}` (shared)
- [ ] Visual smoke OK nas 4 rotas
