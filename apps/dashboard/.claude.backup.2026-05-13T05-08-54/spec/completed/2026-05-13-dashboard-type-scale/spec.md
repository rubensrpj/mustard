# Enhancement: dashboard-type-scale

### Status: closed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-13T03:28:01.000Z
### Lang: pt

## Contexto

O dashboard foi construído com tipografia agressivamente compacta — `text-[10px]` para badges, `text-[11px]` para section headings, `text-xs` (12px) para tudo que é secundário — visando densidade "Linear-style". Na prática, comparando lado-a-lado com Linear e Notion reais, nossa UI fica menor do que ambos: o corpo de Linear roda em 14px e o secundário em 13px; o de Notion em 15-16px. Nosso pendor por 10-12px sai do registro "denso e legível" e cai em "apertado", forçando esforço visual desnecessário em listas longas (specs, knowledge, activity feed). O efeito acumula: cada tela do dashboard parece um pouco mais hostil do que deveria.

## Resumo

Aplicar sweep tipográfico no codebase para alinhar com a escala Linear: badges 11px, labels uppercase 12px, body secundário 13px, body principal 14px. Três passes sequenciais (`text-[11px]` → `text-xs`; `text-[10px]` → `text-[11px]`; muted body `text-xs` → `text-[13px]`) em 11 arquivos de UI. Pular primitivos shadcn (badge/button/tooltip) que servem como base.

## Checklist

### Frontend Agent

- [ ] Sweep 1 — `text-[11px]` → `text-xs` em todos os section labels (sidebar headings, ProjectDetail SectionHeading, AggregateOverview headings, contadores). Executar ANTES do sweep 2 para não conflitar com badges promovidos.
- [ ] Sweep 2 — `text-[10px]` → `text-[11px]` em badges, counter labels uppercase, CommandPalette group headings.
- [ ] Sweep 3 — substituir pares `text-muted-foreground text-xs` ↔ `text-xs text-muted-foreground` por equivalente `text-[13px]`. Aplica a breadcrumbs, descrições, timestamps relativos, empty-state body.
- [ ] Validar visualmente patterns remanescentes: `text-xs uppercase tracking-wider` (labels — mantém 12px) e `text-xs` standalone (input placeholder, etc.).
- [ ] Rodar `pnpm exec tsc --noEmit` e garantir zero erros.

## Arquivos (~11)

- `src/pages/Activity.tsx`
- `src/pages/Home.tsx`
- `src/pages/Knowledge.tsx`
- `src/pages/Settings.tsx`
- `src/pages/ProjectDetail.tsx`
- `src/pages/SpecDetail.tsx`
- `src/components/AggregateOverview.tsx`
- `src/components/CommandPalette.tsx`
- `src/components/SpecsList.tsx`
- `src/components/layout/Sidebar.tsx`
- `src/components/layout/Topbar.tsx`

Excluídos (shadcn primitivos, mantém defaults): `src/components/ui/badge.tsx`, `button.tsx`, `tooltip.tsx`.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript type-check passa sem erros — Command: `pnpm exec tsc --noEmit`
- [x] AC-2: Nenhum `text-[10px]` remanescente fora de shadcn primitivos — Command: `node -e "const cp=require('child_process'); const r=cp.spawnSync('node',['-e',\"const fs=require('fs'),path=require('path');function w(d){let n=0;for(const e of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,e.name);if(e.isDirectory())n+=w(p);else if(/\\.tsx?$/.test(e.name)&&!/components.ui.(badge|button|tooltip)\\.tsx/.test(p)){if(fs.readFileSync(p,'utf8').includes('text-[10px]'))n++}}return n}console.log(w('src'))\"]); process.exit(r.stdout.toString().trim()==='0'?0:1)"`
- [x] AC-3: Pelo menos 1 ocorrência de `text-[13px]` introduzida (sinal de que sweep 3 rodou) — Command: `node -e "const cp=require('child_process'); const r=cp.spawnSync('node',['-e',\"const fs=require('fs'),path=require('path');function w(d){let n=0;for(const e of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,e.name);if(e.isDirectory())n+=w(p);else if(/\\.tsx?$/.test(e.name)){const m=fs.readFileSync(p,'utf8').match(/text-\\[13px\\]/g);if(m)n+=m.length}}return n}console.log(w('src'))\"]); process.exit(parseInt(r.stdout.toString().trim())>=5?0:1)"`
