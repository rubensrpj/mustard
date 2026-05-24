# Tactical Fix: primitives ausentes em components/page

### Stage: Execute
### Outcome: Active
### Flags: 
### Scope: light
### Checkpoint: 2026-05-23T20:30:00Z
### Lang: pt
### Parent: 2026-05-23-dashboard-design-system

## Contexto

Tactical fix derivado de [[2026-05-23-dashboard-design-system]] (Wave 5 BLOCKED).

A Wave 5 (`wave-5-ui`) assumiu que a Wave 2 entregaria 7 primitives em `apps/dashboard/src/components/page/`, mas a Wave 2 explicitamente excluiu esse conjunto do escopo (ver `wave-2-ui/spec.md` linha 57-60: *"Não cria EditorialBand…; Wave 3 ou Wave 4 cria conforme demanda real"*). As Waves 3 e 4 também não criaram. O dispatch da Wave 5 retornou **BLOCKED** com evidência: barril `components/page/index.ts` lista 18 exports, nenhum corresponde aos primitives necessários.

A Wave 6 (`wave-6-ui`, pages secundárias) depende dos mesmos primitives — criar agora desbloqueia ambas em uma única passada.

**Primitives a criar** (uma pasta por primitiva conforme convenção pós-Wave-4):

| Pasta | Exports | Uso |
|---|---|---|
| `EditorialBand/index.tsx` | `EditorialBand`, `EditorialEyebrow`, `EditorialTitle`, `EditorialSubtitle` | Herói de página (slots eyebrow/title/subtitle/actions) |
| `KpiValue/index.tsx` | `KpiValue`, `KpiLabel`, `KpiHint` | Numerics mono tabular dentro de KPICards |
| `KPIRow/index.tsx` | `KPIRow` | Grid wrapper de KPICards (define gap/responsividade) |
| `DeltaText/index.tsx` | `DeltaText` (props: `value: number`, `format: "pct" \| "abs"`, opcional `intent: "auto" \| "success" \| "error" \| "neutral"` — default `auto` deriva intent do sinal) | Deltas verde/vermelho usando tokens `--intent-success`/`--intent-error` |
| `DataRow/index.tsx` | `DataRow` (slots `lead`, `primary`, `meta`, `trailing`) | Linha de lista dentro de `DataCard` |
| `CostBar/index.tsx` | `CostBar`, `BarTrack`, `BarFill` (intent: `primary` \| `accent`) | Barras horizontais (Economia: custo por agente/spec) |
| `LegendSwatch/index.tsx` | `LegendSwatch` (props: `intent`, `label`) | Legenda colorida (swatch + texto) |

**Regras inegociáveis para cada primitive:**

- Zero hex literal no JSX/CSS-in-JS.
- Zero classes Tailwind de cor cru (`text-{cor}-{N}`, `bg-{cor}-{N}`, `border-{cor}-{N}`). Permitido: `bg-card`, `text-foreground`, `text-muted-foreground`, `bg-primary`, `text-primary`, `border-border`, `bg-[--intent-success]`, `text-[--intent-error]` (referenciam tokens semânticos).
- Zero `shadow-*` ou `rounded-*` fora do já-existente em `KPICard`/`DataCard` (consistência).
- Tipografia mono tabular (`font-mono tabular-nums`) em valores numéricos (`KpiValue`, `DeltaText`, `BarFill` label numérico).
- `EditorialBand`: altura ~80px conforme DESIGN.md, tipografia voltage (eyebrow uppercase tracking-wider, title text-2xl|3xl, subtitle muted).
- Cada primitive é stateless (sem `useState`, sem `useEffect`); só layout + composição de slots.

**Pós-criação**: rodar `node scripts/refactor-folder-per-component.mjs` para regenerar `components/page/index.ts` (barrel auto-gerado lista 18 hoje, deve listar 25 depois).

## Tarefas

0. **Antes de tudo**, ler a spec completa em `.claude/spec/2026-05-23-tf-dashboard-page-primitives/spec.md` — a fonte de verdade para a tabela de 7 primitivas, exports nomeados, regras inegociáveis, arquivos, limites e critérios de aceitação está toda lá. As seções referenciadas a seguir (`## Contexto`, `## Critérios de Aceitação`, `## Arquivos`, `## Limites`) ficam nesse arquivo.
1. Para cada uma das 7 primitivas da tabela em `## Contexto`, criar `apps/dashboard/src/components/page/{Nome}/index.tsx` com os exports nomeados indicados.
2. Antes do primeiro Write, ler `apps/dashboard/src/components/page/KPICard/index.tsx` e `apps/dashboard/src/components/page/DataCard/index.tsx` como sibling-reference para confirmar convenções (import order, typed slots, props pattern, classe Tailwind permitida).
3. Honrar as **Regras inegociáveis** acima literalmente — zero hex, zero classes Tailwind cru de cor, stateless, mono tabular onde aplicável.
4. Após criar as 7 pastas, rodar `node apps/dashboard/scripts/refactor-folder-per-component.mjs` (a partir da raiz do repo) para regenerar `apps/dashboard/src/components/page/index.ts`. Confirmar via grep que o barrel passou de 18 para 25 exports.
5. Rodar `pnpm --filter mustard-dashboard build` para validar que tudo compila e o type-check passa.
6. Reportar arquivos criados + qualquer decisão de design não-trivial (ex.: escolha de `cn()` para concat de classes, decisão de slot vs prop para `EditorialBand`).

## Critérios de Aceitação

- [ ] AC-TF-1: dashboard build verde — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-TF-2: barril `components/page/index.ts` lista as 7 novas pastas — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('apps/dashboard/src/components/page/index.ts','utf8');const need=['EditorialBand','KpiValue','KPIRow','DeltaText','DataRow','CostBar','LegendSwatch'];for(const n of need){if(!c.includes('./'+n)){console.error('missing barrel entry:',n);process.exit(1)}}console.log('ok')"`
- [ ] AC-TF-3: cada pasta tem `index.tsx` com export nomeado — Command: `node -e "const fs=require('fs');const need=[['EditorialBand',['EditorialBand','EditorialEyebrow','EditorialTitle','EditorialSubtitle']],['KpiValue',['KpiValue','KpiLabel','KpiHint']],['KPIRow',['KPIRow']],['DeltaText',['DeltaText']],['DataRow',['DataRow']],['CostBar',['CostBar','BarTrack','BarFill']],['LegendSwatch',['LegendSwatch']]];const kws=['function','const','interface','type','class'];for(const [folder,exports] of need){const f='apps/dashboard/src/components/page/'+folder+'/index.tsx';if(!fs.existsSync(f)){console.error('missing file:',f);process.exit(1)}const c=fs.readFileSync(f,'utf8');for(const ex of exports){let ok=false;for(const kw of kws){if(c.indexOf('export '+kw+' '+ex)!==-1){ok=true;break;}}if(!ok){console.error('missing export',ex,'in',f);process.exit(1)}}}console.log('ok')"`
- [ ] AC-TF-4: zero hex literal nas 7 novas pastas — Command: `node -e "const fs=require('fs');const folders=['EditorialBand','KpiValue','KPIRow','DeltaText','DataRow','CostBar','LegendSwatch'];const hex=/['\"\\\`]#[0-9a-fA-F]{3,8}['\"\\\`]/;for(const fo of folders){const f='apps/dashboard/src/components/page/'+fo+'/index.tsx';const c=fs.readFileSync(f,'utf8');if(hex.test(c)){console.error('hex literal in',f);process.exit(1)}}console.log('ok')"`
- [ ] AC-TF-5: zero classes Tailwind de cor cru nas 7 novas pastas (whitelist: card, foreground, muted-foreground, primary, border, intent-success, intent-error, intent-warning via `bg-[--...]`/`text-[--...]`) — Command: `node -e "const fs=require('fs');const folders=['EditorialBand','KpiValue','KPIRow','DeltaText','DataRow','CostBar','LegendSwatch'];const bad=/\\b(text|bg|border|ring|fill|stroke)-(red|amber|emerald|blue|indigo|violet|fuchsia|pink|cyan|teal|lime|green|yellow|orange|rose|sky|slate|zinc|gray|neutral|stone)-(50|100|200|300|400|500|600|700|800|900|950)\\b/;for(const fo of folders){const f='apps/dashboard/src/components/page/'+fo+'/index.tsx';const c=fs.readFileSync(f,'utf8');const m=c.match(bad);if(m){console.error('raw color class in',f,':',m[0]);process.exit(1)}}console.log('ok')"`

## Arquivos

- `apps/dashboard/src/components/page/EditorialBand/index.tsx` (novo)
- `apps/dashboard/src/components/page/KpiValue/index.tsx` (novo)
- `apps/dashboard/src/components/page/KPIRow/index.tsx` (novo)
- `apps/dashboard/src/components/page/DeltaText/index.tsx` (novo)
- `apps/dashboard/src/components/page/DataRow/index.tsx` (novo)
- `apps/dashboard/src/components/page/CostBar/index.tsx` (novo)
- `apps/dashboard/src/components/page/LegendSwatch/index.tsx` (novo)
- `apps/dashboard/src/components/page/index.ts` (regenerado pelo codemod — não editar à mão)

## Limites

Editar/criar dentro de:
- `apps/dashboard/src/components/page/{EditorialBand,KpiValue,KPIRow,DeltaText,DataRow,CostBar,LegendSwatch}/index.tsx`
- `apps/dashboard/src/components/page/index.ts` (apenas via `node scripts/refactor-folder-per-component.mjs`)

**Não tocar:**
- Qualquer outro arquivo em `components/page/` (KPICard, DataCard, PageSurface, etc. — já estabilizados pela Wave 2)
- `apps/dashboard/src/{pages,features,layout,ui}/**`
- `apps/dashboard/src/style.css`
- `apps/dashboard/src-tauri/**`
- `apps/dashboard/scripts/**`
