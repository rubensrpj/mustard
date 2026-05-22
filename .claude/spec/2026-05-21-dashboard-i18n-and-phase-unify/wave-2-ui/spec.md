# Wave 2 — i18n audit: t(key) global + sidebar/topbar/specs

### Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-21T18:00:00Z

## Resumo

Auditar e centralizar as strings de UI do caminho da sidebar + rota `/specs` + página Knowledge via `t(key)` exportada de `@/lib/i18n`. Catálogo PT+EN com bootstrap a partir de Preferences (zustand store). Quando o usuário troca idioma em Preferences, todas essas strings flipam. A sidebar para de mostrar "Knowledge" em EN quando idioma é PT.

## Contexto

`src/lib/i18n.ts` e `src/i18n.ts` já existem. Investigar primeiro o que está implementado (catálogo? função t()? hook useTranslation?). Os arquivos `Preferences.tsx` já tem seletor de idioma; `store.ts` (zustand) provavelmente já guarda a preferência. Em vez de criar um sistema novo, ESTENDER o existente:

1. Confirmar shape do catálogo atual em `lib/i18n.ts`.
2. Adicionar chaves faltantes: `sidebar.overview, sidebar.specs, sidebar.economy, sidebar.knowledge, sidebar.commands, sidebar.preferences, sidebar.add_project, route.specs.title, route.specs.subtitle, page.knowledge.title, breadcrumb.workspace, action.add, action.refresh, action.close, empty.no_events, count.acs, count.files, count.tools, phase.analyze, phase.plan, phase.execute, phase.review, phase.qa, phase.close, status.no_data, drawer.pin, drawer.unpin`.
3. Garantir que `t(key)` é exportada do módulo `@/lib/i18n` (renomear se já existir como `translate` ou `i18n.t`).
4. Substituir hard-coded strings nos arquivos do escopo.

## Arquivos

```
apps/dashboard/src/lib/i18n.ts                                — catálogo expandido + função t() exportada
apps/dashboard/src/i18n.ts                                    — bootstrap do idioma (lê preferences)
apps/dashboard/src/components/layout/Sidebar.tsx              — labels via t()
apps/dashboard/src/components/layout/Topbar.tsx               — labels via t()
apps/dashboard/src/pages/Specs.tsx                            — title/subtitle/breadcrumb via t()
apps/dashboard/src/pages/Knowledge.tsx                        — title via t()
apps/dashboard/src/components/page/PageHeader.tsx             — se houver string EN
```

## Tarefas

- [ ] Read `apps/dashboard/src/lib/i18n.ts` e `apps/dashboard/src/i18n.ts` para entender a infra atual. Documentar inline (no spec) o shape e o que falta.
- [ ] **Catálogo.** Em `lib/i18n.ts`, adicionar chaves faltantes (lista no Contexto). Estrutura:
  ```ts
  export const translations = {
    pt: {
      "sidebar.overview": "Visão Geral",
      "sidebar.specs": "Specs",
      "sidebar.economy": "Economia",
      "sidebar.knowledge": "Conhecimento",
      "sidebar.commands": "Comandos",
      "sidebar.preferences": "Preferências",
      "sidebar.add_project": "Adicionar projeto",
      "route.specs.title": "Specs",
      "route.specs.subtitle": "Lista e drill-down por spec",
      "breadcrumb.workspace": "Workspace",
      "action.add": "Adicionar",
      "action.refresh": "Atualizar",
      "action.close": "Fechar",
      "empty.no_events": "Pipeline ainda sem eventos",
      "count.acs": "ACs",
      "count.files": "arquivos",
      "count.tools": "tools",
      "phase.analyze": "Analisar",
      "phase.plan": "Planejar",
      "phase.execute": "Executar",
      "phase.review": "Revisar",
      "phase.qa": "QA",
      "phase.close": "Fechar",
      "drawer.pin": "Fixar painel",
      "drawer.unpin": "Soltar painel",
    },
    en: {
      "sidebar.overview": "Overview",
      "sidebar.specs": "Specs",
      "sidebar.economy": "Economy",
      "sidebar.knowledge": "Knowledge",
      "sidebar.commands": "Commands",
      "sidebar.preferences": "Preferences",
      "sidebar.add_project": "Add project",
      ... (espelha PT)
    },
  } as const;
  ```
  Se o catálogo atual usa outra estrutura, ESTENDER preservando a forma existente.
- [ ] **Função `t(key)` global.** Em `lib/i18n.ts`, garantir `export function t(key: string): string` que olha o idioma atual (de `useStore.getState().lang` ou similar) e devolve `translations[lang]?.[key] ?? translations.pt[key] ?? key`. Fallback PT→EN→chave.
- [ ] **Reactivity.** Se o sistema atual é puro fn (não hook), funciona mas componentes não re-renderizam ao trocar idioma. Solução: hook `useT()` que assina ao slice `lang` do store via `useStore(s => s.lang)`. Sidebar/Topbar/pages usam `useT()`. Outra opção: chamar `t()` direto + ler `lang` do store no mesmo render — também causa re-render. Escolha o que se encaixa no padrão existente.
- [ ] **Sidebar.tsx** — substituir cada label hard-coded por `t('sidebar.X')`. Labels visíveis: "Adicionar projeto", "Visão Geral", "Specs", "Economia", "Knowledge", "Comandos", "Preferências", "Configurações" (se houver). Hover/aria-labels também.
- [ ] **Topbar.tsx** — auditar e substituir.
- [ ] **Specs.tsx** — `<PageHeader title="Specs" subtitle="Lista e drill-down por spec" ...>` vira `<PageHeader title={t('route.specs.title')} subtitle={t('route.specs.subtitle')} breadcrumb={[{label: t('breadcrumb.workspace')}, {label: t('route.specs.title')}]} ...>`.
- [ ] **Knowledge.tsx** — título via `t('sidebar.knowledge')`.
- [ ] **PageHeader.tsx** — se tem strings (botões, ações) — auditar.
- [ ] Bootstrap em `src/i18n.ts`: importar `useStore` zustand, ler `lang` no app mount, sincronizar com `<html lang>` para acessibilidade.
- [ ] Build: `pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W2-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W2-2: `t(key)` exportada de `lib/i18n` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/lib/i18n.ts','utf8');process.exit(/export\\s+(function\\s+t|const\\s+t\\s*=)/.test(s)?0:1)"`
- [ ] AC-W2-3: Catálogo tem chave knowledge em PT e EN — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/lib/i18n.ts','utf8');process.exit(/Conhecimento/.test(s)&&/Knowledge/.test(s)?0:1)"`
- [ ] AC-W2-4: Sidebar usa t() — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/layout/Sidebar.tsx','utf8');process.exit(/\\bt\\(['\"]sidebar\\./.test(s)?0:1)"`
- [ ] AC-W2-5: Sidebar NÃO tem string Knowledge hard-coded — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/layout/Sidebar.tsx','utf8');const stripped=s.replace(/\\/\\*[\\s\\S]*?\\*\\/|\\/\\/.*$/gm,'');process.exit(/['\"]Knowledge['\"]/.test(stripped)?1:0)"`

## Limites

- `apps/dashboard/src/lib/i18n.ts`
- `apps/dashboard/src/i18n.ts`
- `apps/dashboard/src/components/layout/Sidebar.tsx`
- `apps/dashboard/src/components/layout/Topbar.tsx`
- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/pages/Knowledge.tsx`
- `apps/dashboard/src/components/page/PageHeader.tsx`

## Network

- Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
- Paraleliza com [[wave-1-ui]] (zero overlap de arquivos)
