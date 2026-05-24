# Wave 1b — Badges semânticos (success/warning/error/info) em badge.tsx

## PRD

## Contexto

O `Badge` atual (`apps/dashboard/src/components/ui/badge.tsx`) tem variants decorativos (`tag-purple/orange/green`, `destructive`, `outline`, `ghost`) mas nenhum semântico explícito de sucesso/atenção/erro/informação como o Notion expõe. Resultado: cada componente do dashboard escolhe sua paleta de pé, e a Visão Geral redesenhada não consegue comunicar status uniformemente. Esta wave acrescenta variants semânticos sem migrar usos antigos.

## Métrica de sucesso

`<Badge variant="success">`/`"warning"`/`"error"`/`"info"` renderiza com tokens Tailwind 4 corretos em light e dark, e `<Badge variant="status-draft">`/`"status-implementing"`/`"status-awaiting-qa"`/`"status-completed"` reusa a mesma paleta semântica.

## Não-Objetivos

- Não alterar/remover variants existentes (`tag-purple`, `tag-orange`, `tag-green`, `destructive`, `outline`, `ghost`, `link`).
- Não migrar usos antigos — só novos componentes (Wave 3) consomem os semânticos.
- Não tocar `tailwind.config` nem CSS vars — usar utilitários `bg-emerald-*`, `text-amber-*`, etc. nativos do Tailwind 4.

## Acceptance Criteria

- [x] AC-1: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [x] AC-2: 4 variants semânticos primários presentes — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/ui/badge.tsx','utf8');['success','warning','error','info'].forEach(v=>{if(!t.includes('\"'+v+'\"'))throw new Error('missing variant '+v)})"`
- [x] AC-3: 4 status variants presentes — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/ui/badge.tsx','utf8');['status-draft','status-implementing','status-awaiting-qa','status-completed'].forEach(v=>{if(!t.includes('\"'+v+'\"'))throw new Error('missing variant '+v)})"`

## Plano

## Arquivos (~1)

```
apps/dashboard/src/components/ui/badge.tsx   (modify — +8 variants)
```

## Tarefas

### Frontend Foundation Agent

- [x] Acrescentar à `cva` em `badge.tsx`:
  - `success: "bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300"`
  - `warning: "bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300"`
  - `error: "bg-red-100 text-red-700 dark:bg-red-500/15 dark:text-red-300"`
  - `info: "bg-sky-100 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300"`
- [x] Acrescentar os 4 status (reusam classes via concatenação para evitar duplicar paleta):
  - `status-draft` → mesmas classes de `info`
  - `status-implementing` → mesmas classes de `warning`
  - `status-awaiting-qa` → mesmas classes de `warning` com border `border-amber-400/40` para diferenciar de `implementing`
  - `status-completed` → mesmas classes de `success`
- [x] `pnpm --filter mustard-dashboard exec tsc --noEmit`

## Dependências

Nenhuma — primeira wave, paralela à [[wave-1-backend]].

## Network

- Parent: [[2026-05-20-dashboard-visual-overview]]
- Paralela a: [[wave-1-backend]]
- Desbloqueia: [[wave-3-ui]] (consome os 8 variants novos do `badge.tsx`)
- Memória compartilhada: grava `{variants_added: [...], dark_mode_classes: "...", notes: "..."}` para [[wave-3-ui]].

## Limites

Em escopo: `apps/dashboard/src/components/ui/badge.tsx`.

Fora de escopo: tudo mais.
