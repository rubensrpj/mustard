# Wave Plan — Visão Geral redesenhada

### Status: approved
### Phase: PLAN
### Scope: full (wave plan)
### Checkpoint: 2026-05-20T23:05:00Z
### Lang: pt

## PRD (visão única)

A página `Visão Geral` (`Workspace.tsx`) será reorganizada em 5 visualizações navegáveis (specs por status com filtro de período, total de tokens economizados, calendário mensal de atividades, feed de eventos com badges por tipo e atalho clicável pra spec, ranking de arquivos mais usados) sustentadas por um sistema de badges semânticos novo (`success/warning/error/info`, estilo Notion) em `badge.tsx`. Hoje a página é um mosaico raso: heatmap só de "hoje", lista de specs duplicando `/specs`, badges sem semântica, sem visão acumulada de economia nem feed cronológico — o operador precisa abrir três páginas pra montar a foto. O sucesso é o operador abrir a Visão Geral e em ≤3 segundos saber em voz alta: quantas specs por status, quanto economizou no total, qual o dia mais ativo da semana (clicável), qual o último evento e em qual spec.

## Métrica de sucesso

Operador abre `/` e responde sem hover/scroll: (a) contagem por status (b) total de tokens economizados (c) navegação mês/ano + densidade diária (d) último evento + atalho pra spec.

## Não-Objetivos globais (valem para todas as waves)

- Não tocar Sidebar, Topbar, outras pages (Specs, Economia, Knowledge, Settings, Preferences, Home, Commands, ProjectDetail, SpecDetail, Prd).
- Não migrar usos antigos de `Badge variant="tag-*"` — só os componentes novos consomem os semânticos.
- Não persistir filtros entre sessões.
- Não criar pacote compartilhado de badges (fica em `apps/dashboard/src/components/ui/`).
- Não tocar `packages/core`, `apps/cli`, `apps/rt`.

## Tabela de Waves

| Wave | Spec                       | Role       | Modelo | Status   | Depende de                       | Resumo                                                                |
|------|----------------------------|------------|--------|----------|----------------------------------|-----------------------------------------------------------------------|
| 1a   | [[wave-1-backend]]         | general    | opus   | draft    | —                                | 3 comandos Tauri novos em `spec_views.rs` + registro em `main.rs`     |
| 1b   | [[wave-1-badges]]          | frontend   | opus   | draft    | —                                | Variants semânticos `success/warning/error/info` + `status-*` em `badge.tsx` |
| 2    | [[wave-2-data]]            | frontend   | opus   | queued   | [[wave-1-backend]]               | 3 invoke wrappers em `lib/dashboard.ts` + 3 hooks `useWorkspace*`     |
| 3    | [[wave-3-ui]]              | frontend   | opus   | queued   | [[wave-1-badges]], [[wave-2-data]] | 5 componentes `components/workspace/Workspace*.tsx`                   |
| 4    | [[wave-4-integration]]     | frontend   | opus   | queued   | [[wave-3-ui]]                    | Reescrita do `Workspace.tsx` montando as 5 visualizações              |

**Paralelismo:** [[wave-1-backend]] e [[wave-1-badges]] são disparáveis em paralelo (zero acoplamento — backend Rust + frontend badges não se cruzam).

Planos SDD (declarados upfront, executados ao final):

| Plano  | Arquivo                       | Conteúdo                                                                         |
|--------|--------------------------------|----------------------------------------------------------------------------------|
| Review | [[review]] (`review/spec.md`)  | Checklist 7 categorias, reviewer `sonnet`, verdict em `review/verdict.md`        |
| QA     | [[qa]] (`qa/spec.md`)          | Consolida AC do parent + 4 waves + review, runner `qa-run --include-children`, relatório em `qa/report.md` |

## Network

Grafo de dependências (wikilinks Obsidian-style — clicáveis no dashboard quando [[mustard-wave-network-standard]] entregar o parser):

- [[wave-1-backend]] → [[wave-2-data]] → [[wave-3-ui]] → [[wave-4-integration]] → [[review]] → [[qa]]
- [[wave-1-badges]] → [[wave-3-ui]]

Memória compartilhada entre waves: o agente de [[wave-2-data]] recebe no prompt o resumo dos artefatos produzidos por [[wave-1-backend]]; o agente de [[wave-3-ui]] recebe resumos de [[wave-1-badges]] e [[wave-2-data]]; o agente de [[wave-4-integration]] recebe resumo de [[wave-3-ui]]. Hoje isso depende de o operador injetar manualmente (`mustard-rt run memory agent` só grava — read-side injetado vem em [[mustard-wave-network-standard]]).

## Artefatos de verificação (gerados pela pipeline)

Estes arquivos NÃO existem agora — a pipeline cria-os ao concluir cada fase. Ficam ao lado das waves, no mesmo dir-pai, para preservar a uniformidade "uma fase = um arquivo" pedida.

- `qa-report.md` — produzido pela fase QA (Wave 10) após Wave 4. Contém o resultado de cada AC declarado por wave + os AC globais do parent.
- `review-report.md` — produzido por `/mustard:review` em cada subprojeto tocado (`dashboard` é o único aqui). Verdict APPROVED/REJECTED + 7 categorias.
- `close-report.md` — produzido por `/mustard:close` no encerramento. Resumo de specs movidas pra `completed/`, registry sync, métricas finais.

> Observação: a geração automática dos `qa-report.md`/`review-report.md`/`close-report.md` é proposta nova de uniformidade pedida pelo usuário. Hoje QA grava em `.claude/.qa-reports/{spec}.json` e Review inline em `## Concerns`. Adequar a pipeline pra produzir esses arquivos é trabalho da próxima evolução do `mustard-rt` — fora do escopo desta spec. Esta seção documenta a intenção; cada wave pode anexar suas observações de QA manualmente em `qa-report.md` no spec dir até a automação chegar.

## Critérios de Aceitação (globais — somam aos da cada wave)

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-G1: Build do dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-G2: Lint do dashboard passa — Command: `pnpm --filter mustard-dashboard lint`
- [ ] AC-G3: Cargo check passa no crate Tauri — Command: `cargo check -p dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [ ] AC-G4: Todas as 4 waves marcadas `completed` no wave-plan.md (manual check final)

## Limites globais

```
ESCOPO:
  apps/dashboard/src/components/ui/badge.tsx
  apps/dashboard/src/pages/Workspace.tsx
  apps/dashboard/src/components/workspace/Workspace*.tsx
  apps/dashboard/src/hooks/useWorkspace*.ts
  apps/dashboard/src/lib/dashboard.ts
  apps/dashboard/src-tauri/src/spec_views.rs
  apps/dashboard/src-tauri/src/main.rs

OUT-OF-BOUNDS:
  apps/dashboard/src/components/layout/**
  apps/dashboard/src/pages/{Specs,Economia,Knowledge,Settings,Preferences,Home,Commands,ProjectDetail,SpecDetail,Prd}.tsx
  apps/dashboard/src/components/specs/**
  apps/dashboard/src/components/knowledge/**
  packages/core/**
  apps/cli/**
  apps/rt/**
```
