# Tactical Fix — SpecCard polish (6 itens: unificar + 5 visuais)

### Parent: [[2026-05-21-dashboard-i18n-and-phase-unify]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-21T18:30:00Z
### Lang: pt

## PRD

## Contexto

Em uso real do dashboard após o phase-unify, o `<PipelineTimeline>` ainda renderiza visualmente DIFERENTE entre rota `/specs` (cards da Lista, com `variant="compact"`, circles `h-7`) e rota Detalhes (com `variant="default"`, circles `h-10`). O usuário pediu explicitamente "mesmo componente em todas as rotas" — variants de tamanho NÃO atendem; precisa ser **um único render idêntico**. Além disso, os labels das fases ainda em EN ("Analyze/Plan/Execute/QA/Close") mesmo com `lang=pt` em Preferences; o canto superior direito tem duas badges sobrepostas (status pill + phase chip) — a phase chip é redundante com o `PipelineTimeline` mostrado logo abaixo; o status pill vem cinza/sem cor; há um traço `—` separando os badges do botão "Detalhes" que polui; o botão "Detalhes" é discreto demais; o canto inferior esquerdo (ACs/arquivos/tools) oculta métricas quando o valor é null em vez de mostrar `0` ou `—`. Não há temporizador (duração) visível na posição esperada (canto inferior direito).

## Métrica de sucesso

Recarregar o app em PT: labels das fases viram "Analisar / Planejar / Executar / Revisar / QA / Fechar". Só um badge no canto superior direito (status com cor). Sem traço entre badge e botão. Botão "Detalhes" mais proeminente. Canto inferior esquerdo mostra sempre: ondas, ACs, arquivos, tools, modelo (cada com fallback `0` ou `—`). Canto inferior direito mostra a duração da pipeline.

## Acceptance Criteria

- [x] AC-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: PipelineTimeline NÃO tem mais prop variant (render único) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/telemetry/PipelineTimeline.tsx','utf8');process.exit(s.includes('variant')?1:0)"`
- [x] AC-3: PhaseStation NÃO tem mais prop variant — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/telemetry/PhaseStation.tsx','utf8');process.exit(s.includes('variant')?1:0)"`
- [x] AC-4: PhaseStation labels usam t() — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/telemetry/PhaseStation.tsx','utf8');process.exit(s.includes('phase.analyze')||s.includes('useT')?0:1)"`
- [x] AC-5: SpecCard NÃO renderiza PhaseChip — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(s.includes('<PhaseChip')?1:0)"`

## Plano

## Summary

Cinco edits visuais em `SpecCard.tsx` + um em `PhaseStation.tsx`. Single Light scope, dispatched together.

## Checklist

### dashboard-impl Agent

- [x] **(0) Unificar PipelineTimeline — remover prop `variant`.** Em `PipelineTimeline.tsx` e `PhaseStation.tsx`: DELETAR o tipo `variant?: "compact" | "default"` e toda lógica `isCompact`. Manter SÓ o tamanho `default` (circle `h-10 w-10`, icon `h-5 w-5`, label `text-[12px] font-medium`, ring `ring-2`, gap `gap-3`, padding `px-4`, conector `h-0.5` top-[20px] left-8 right-8, min-w `min-w-[56px]`). Os call sites em `SpecCard.tsx` e `SpecDetailDashboard.tsx` removem a prop `variant`. Resultado: render idêntico nas duas rotas.

- [x] **(1) i18n nas labels de fase.** Em `apps/dashboard/src/components/telemetry/PhaseStation.tsx`, importar `useT` de `@/lib/i18n` e trocar o label literal por `t('phase.{key}')`. As chaves já existem no catálogo (Wave 2 do spec parent adicionou `phase.analyze`, `phase.plan`, etc.). Se o componente recebe `label` como prop, mantenha mas só renderize quando vier explícito — caso contrário, derivar de `phase` via `t()`.
- [x] **(2) Duração no canto inferior direito do SpecCard.** Em `apps/dashboard/src/components/specs/SpecCard.tsx`, RETIRAR o `<span title="Duração">{formatDuration(data.duration_ms)}</span>` do cluster superior direito. Adicionar no rodapé (linha de quantitativos no bottom) à direita: `<span className="ml-auto text-[11px] text-muted-foreground tabular-nums" title="Duração da pipeline">{formatDuration(data.duration_ms)}</span>` — note que já existe um `ml-auto` no `model`; mover o `model` pra antes da duração e a duração vira o último elemento da linha (sempre à direita).
- [x] **(3) Remover PhaseChip.** Em `SpecCard.tsx`, deletar o `<PhaseChip phase={data.phase} />` do cluster superior direito. O PipelineTimeline já mostra as fases logo abaixo, então o chip é redundante. Manter o `<StatusPill status={data.status} />`.
- [x] **(4a) StatusPill com cor.** Confirmar que `<StatusPill>` em `apps/dashboard/src/components/specs/spec-status.tsx` tem cor por status. Se o status `planning` retorna `bg-muted`/cinza, trocar pra `bg-violet-500/15 text-violet-400`. Mapa de cores recomendado:
  - `planning` → violet
  - `implementing` / `in_progress` → green (combina com execute)
  - `reviewing` → amber
  - `qa` → emerald
  - `blocked` / `wave-failed` → red
  - `completed` / `closed` → slate
  - `closed-followup` → cyan
  - `cancelled` → red-muted
  - `abandoned` / `no-events` → muted/border-dashed
- [x] **(4b) Remover traço `—` antes do botão "Detalhes".** Em `SpecCard.tsx`, procurar e remover qualquer `—` (em dash literal ou span dashed) entre o cluster de badges e o botão "Detalhes".
- [x] **(4c) Melhorar botão "Detalhes".** Estilo atual provavelmente é ghost/discrete. Trocar pra:
  - `className="inline-flex items-center gap-1 h-7 px-2.5 rounded-md bg-card border border-border hover:bg-muted/60 hover:border-foreground/20 transition-colors text-[12px] font-medium"`
  - Ícone `Maximize2` à esquerda + texto "Detalhes" + setinha `ArrowUpRight` à direita (lucide).
- [x] **(5) Quantitativos completos com fallback.** Em `SpecCard.tsx`, a linha de quantitativos (`<div className="flex items-center gap-4 ...">`) deve SEMPRE renderizar TODOS os campos, com fallback `0` ou `—`:
  - `ondas {current_wave ?? "—"}/{total_waves ?? "—"}` — sempre (não condicional)
  - `ACs {ac_passed}/{ac_total}` — sempre (já estava)
  - `arquivos {files_touched}` — sempre (já estava)
  - `tools {tools_used}` — sempre (já estava)
  - `modelo {model ?? "—"}` — sempre (era condicional, vira incondicional)
  - `duração {formatDuration(duration_ms)}` (do item 2) — última à direita, sempre

  Apresentação melhor das siglas: label minúsculo + valor em destaque. Ex.: `<span title="Ondas"><span className="text-muted-foreground/60">ondas</span> <span className="text-foreground/70 font-medium">{current_wave ?? "—"}/{total_waves ?? "—"}</span></span>` — já existe esse padrão; só aplicar em TODOS os campos.

- [x] Build verde: `pnpm --filter mustard-dashboard build`.

## Files (~3)

- `apps/dashboard/src/components/telemetry/PhaseStation.tsx`
- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/spec-status.tsx`
