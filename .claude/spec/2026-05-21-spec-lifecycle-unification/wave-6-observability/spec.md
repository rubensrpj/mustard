# Wave 6 — Observability: card "Saúde" no Workspace + badges/filtro inline em Specs

### Parent: [[2026-05-21-spec-lifecycle-unification]]
### Wave: 6
### Role: dashboard
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-22T01:42:00Z

## Resumo

Torna visíveis as ações do hygiene hook (Wave 5) no dashboard, sem criar rota nova. Adiciona um card "Saúde do workspace" na rota `/workspace` (logo abaixo do hero) com contadores clicáveis: `Ativas / Suspeitas / Auto-fechadas hoje / Bloqueadas`. Adiciona badges inline nas linhas de `/specs` para `auto-fechada`, `bloqueada`, `wave-failed`, `órfã suspeita`, `follow-up aberto`. Introduz o filtro `Suspeitas` que estava como placeholder em Wave 3.

## Arquivos

```
apps/dashboard/src-tauri/src/spec_views.rs                  (`workspace_health` command novo)
apps/dashboard/src-tauri/src/lib.rs                         (registrar)
apps/dashboard/src/lib/dashboard.ts                         (fetchWorkspaceHealth wrapper)
apps/dashboard/src/lib/types/specs.ts                       (WorkspaceHealth type)
apps/dashboard/src/components/workspace/WorkspaceHealthCard.tsx  (novo)
apps/dashboard/src/pages/Workspace.tsx                      (inserir o card)
apps/dashboard/src/components/specs/SpecRow.tsx             (renderizar flags como badges)
apps/dashboard/src/components/specs/SpecBadge.tsx           (novo — visual unificado)
apps/dashboard/src/pages/Specs.tsx                          (filtro "Suspeitas" populado)
apps/dashboard/src/i18n.ts                                  (chaves novas)
```

## Tarefas

### Backend Tauri
- [ ] `workspace_health(project_path) -> WorkspaceHealth` em `spec_views.rs`. Agrega contagens executando query no SQLite:
  ```sql
  SELECT
    SUM(state.outcome = 'active') as active,
    SUM(events.kind = 'hygiene.detected' AND events.ts > NOW - 7d) as suspects,
    SUM(events.kind = 'hygiene.autoclose' AND events.ts > NOW - 24h) as autoclose_today,
    SUM(state.flags ? 'blocked') as blocked
  ```
- [ ] Tipo:
  ```ts
  type WorkspaceHealth = {
    active: number;
    suspects: number;          // specs com hygiene.detected recente
    autoclose_today: number;
    blocked: number;
    wave_failed: number;
    followup_open: number;
    last_hygiene_run_at: string | null;
  };
  ```

### Card "Saúde" no Workspace

- [ ] `WorkspaceHealthCard.tsx`: card com 4-6 contadores numéricos clicáveis. Cada click navega para `/specs` com o filtro pré-aplicado (`?filter=suspects`, `?filter=blocked`, etc.).
- [ ] Card é **colapsável**. Default: expandido se `suspects > 0 || autoclose_today > 0 || blocked > 0 || wave_failed > 0`; senão colapsado.
- [ ] Mostra timestamp do último run do hygiene (`last_hygiene_run_at`) discreto, formato "rodou há 4h".
- [ ] Inserir no `Workspace.tsx` logo abaixo do `<WorkspaceHero />`.

### Badges em SpecRow

- [ ] `SpecBadge.tsx`: componente com variantes `auto-closed | blocked | wave-failed | suspect | followup`. Cada uma pinta com cor consistente (verde escuro / amber / rosa / cinza / azul). Tamanho 11px, padding 4px 6px, rounded-sm.
- [ ] Em `SpecRow.tsx`, renderizar badges à direita do nome (antes das colunas de métrica) baseado em `state.flags` + último evento `hygiene.*`:
  - `flags.blocked = true` → badge `blocked`.
  - `flags.wave_failed = true` → badge `wave-failed`.
  - `flags.followup_open = true` → badge `follow-up`.
  - Último `hygiene.detected` há ≤7d e ainda `outcome == Active` → badge `suspect`.
  - Último `hygiene.autoclose` há ≤24h → badge `auto-closed` (na lista `Encerradas`).

### Filtro "Suspeitas" em /specs

- [ ] Em `Specs.tsx`, o pill `Suspeitas` (placeholder em Wave 3) agora filtra para specs com `suspect` badge ativa. Implementação: usar query `workspace_health` + listar nomes; cruzar com a lista principal.

### i18n
- [ ] Chaves:
  - `workspace.health.title` → "Saúde do workspace"
  - `workspace.health.active`, `workspace.health.suspects`, `workspace.health.autoclose_today`, `workspace.health.blocked`, `workspace.health.wave_failed`, `workspace.health.followup_open`
  - `workspace.health.last_run` → "Última verificação há {time}"
  - `specs.badge.blocked`, `specs.badge.wave_failed`, `specs.badge.followup`, `specs.badge.suspect`, `specs.badge.auto_closed`
  - `specs.filter.suspects` → "Suspeitas"

## Layout do card "Saúde" (referência visual)

```
┌─────────────────────────────────────────────────────────────────────┐
│  Saúde do workspace                       última verificação há 4h ✕│
├─────────────────────────────────────────────────────────────────────┤
│   12          3              1                  0            0       │
│  Ativas    Suspeitas    Auto-fechadas       Bloqueadas    Wave failed│
│              ▴                ▴                                       │
└─────────────────────────────────────────────────────────────────────┘
```

Números clicáveis. Indicadores (`▴`) marcam categorias com sinal a ler.

## Acceptance Criteria

- [x] AC-W6-1: `pnpm --filter mustard-dashboard build` passa (tsc -b + vite build, 0 erros). ✅ + `cargo check -p mustard-dashboard` 0 erros.
- [~] AC-W6-2: `pnpm lint` pré-existente-quebrado repo-wide (ESLint v9 sem `eslint.config.js` flat) — mesmo gap das W3. `tsc` é o gate real e passa. Follow-up: adicionar flat config.
- [~] AC-W6-3 (build-verified; visual manual pendente): card "Saúde" inserido abaixo do `<WorkspaceHero/>` com 5 contadores colapsáveis. Render visual em `tauri:dev` não verificado nesta sessão (app Tauri não lançável aqui) — verificação visual manual recomendada.
- [~] AC-W6-4 (build-verified): pill/click "Suspeitas" lê `?filter=suspects` e cruza com a lista de hygiene-suspects. Navegação/visual pendente de check manual.
- [~] AC-W6-5 (condicional): badge `auto-closed`/`suspect` em SpecRow implementado via props `suspectSpecs`/`autoClosedSpecs`. Depende de o hygiene ter fechado tf-skill-mirror (não fechou no QA — modo auto não rodado contra repo vivo). Visual pendente.
- [~] AC-W6-6 (build-verified): `useQuery` com `refetchInterval: 12_000` (igual active-pipelines). Live-update pendente de check manual.

> **Nota QA:** ACs W6-3..6 são visuais/runtime e exigem `pnpm tauri:dev` rodando — não lançável nesta sessão. Build (tsc) + cargo check passam, que é o gate objetivo. Verificação visual contra o workspace real (`C:\Atiz\mustard`) recomendada manualmente (cf. memory: QA do dashboard contra consumidor real).

## Limites

**IN:** apenas os arquivos listados.

**OUT:**
- Drill-down de um evento hygiene específico (página própria) — fora de escopo. Reusa o spec drill-down existente.
- Notificação fora do app (toast/banner ao auto-close) — possível em wave futura.
