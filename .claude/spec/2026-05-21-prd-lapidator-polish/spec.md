# Polish do PRD Lapidator

### Parent: 2026-05-20-dashboard-prd-ai-lapidator
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-21T00:00:00Z
### Lang: pt

## PRD

## Contexto

Os reviews da spec pai (`2026-05-20-dashboard-prd-ai-lapidator`) aprovaram a entrega mas surgiram 3 *tactical fix candidates* — todos de polish, nenhum bloqueante. Esta sub-spec agrupa os três num único passe leve: extrair o banner âmbar de entidades/paths faltantes em componente isolado, mover a lógica do lapidador de `Prd.tsx` para um hook dedicado, e tirar o slug do modelo Claude do `prd_lapidator.rs` para uma constante (evita recompilar Rust quando o modelo mudar). Todos os três fixes preservam o comportamento atual — é refactor cosmético, não muda contrato Rust↔TS nem fluxo do usuário.

## Métrica de sucesso

Build verde nos três alvos (`tsc -b`, `vite build`, `cargo build`), `Prd.tsx` reduz em pelo menos 40 LOC, e mudar a string do modelo no Rust passa a ser uma edição de uma única linha (a constante).

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: novo arquivo `src/components/prd/ConfrontBanner.tsx` existe e é importado por `IntentHero.tsx` — Command: `node -e "const fs=require('fs');if(!fs.existsSync('apps/dashboard/src/components/prd/ConfrontBanner.tsx'))process.exit(1);if(!fs.readFileSync('apps/dashboard/src/components/prd/IntentHero.tsx','utf8').includes('ConfrontBanner'))process.exit(1)"`
- [ ] AC-2: novo arquivo `src/hooks/useLapidator.ts` existe e é importado por `Prd.tsx` — Command: `node -e "const fs=require('fs');if(!fs.existsSync('apps/dashboard/src/hooks/useLapidator.ts'))process.exit(1);if(!fs.readFileSync('apps/dashboard/src/pages/Prd.tsx','utf8').includes('useLapidator'))process.exit(1)"`
- [ ] AC-3: constante de modelo extraída em `prd_lapidator.rs` — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src-tauri/src/prd_lapidator.rs','utf8');if(!/const\s+CLAUDE_MODEL\s*:\s*&str\s*=\s*\"claude-sonnet-4-6\"/.test(f))process.exit(1)"`
- [ ] AC-4: tsc passa — Command: `pnpm --filter mustard-dashboard exec tsc -b`
- [ ] AC-5: vite build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-6: cargo build passa — Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`

## Plano

## Arquivos

- `apps/dashboard/src/components/prd/ConfrontBanner.tsx` (novo, ~50 LOC) — extrai o JSX âmbar de `IntentHero.tsx` linhas 92-122
- `apps/dashboard/src/components/prd/IntentHero.tsx` (edit, ~-30 LOC) — passa a importar e renderizar `<ConfrontBanner confront={confront} />`
- `apps/dashboard/src/hooks/useLapidator.ts` (novo, ~80 LOC) — encapsula state (`intent`, `isLapidating`, `lapidateError`, `confront`, `claudeAvailable`, `selectedEntities`) + `useEffect` que checa `checkClaudeAvailable` + função `lapidate` que aceita o caller-provided `applyToForm` callback
- `apps/dashboard/src/pages/Prd.tsx` (edit, ~-60 LOC) — substitui os 6 useState + useEffect + handleLapidate por `const { intent, setIntent, ... } = useLapidator(projectPath)`
- `apps/dashboard/src-tauri/src/prd_lapidator.rs` (edit, +1/-1 LOC) — adiciona `const CLAUDE_MODEL: &str = "claude-sonnet-4-6";` no topo e troca `.arg("claude-sonnet-4-6")` por `.arg(CLAUDE_MODEL)` na linha 148

## Tarefas

### ui Agent (Wave 3-polish)

- [ ] Criar `apps/dashboard/src/components/prd/ConfrontBanner.tsx`: componente puro que recebe `confront: PrdConfront | null` e renderiza nada quando `confront === null` ou ambas as listas vazias; caso contrário renderiza o mesmo bloco âmbar de hoje (mantém classes Tailwind atuais)
- [ ] Editar `IntentHero.tsx`: remover o JSX `hasConfrontWarnings && <div role="alert" ...>`, importar e renderizar `<ConfrontBanner confront={confront} />` no mesmo lugar
- [ ] Criar `apps/dashboard/src/hooks/useLapidator.ts` com assinatura `useLapidator(projectPath: string | null): { intent, setIntent, lapidate(applyToForm: (r: LapidatedPrd) => void), isLapidating, lapidateError, confront, claudeAvailable, selectedEntities, setSelectedEntities }`. Move `useEffect` que checa Claude. `lapidate` chama o wrapper API, em sucesso roda o callback do caller (pra setForm) + set confront + set selectedEntities; em erro set lapidateError + toast.
- [ ] Editar `Prd.tsx`: substituir os 6 useState (intent, isLapidating, lapidateError, confront, claudeAvailable, selectedEntities) + o useEffect + a function handleLapidate por `const lap = useLapidator(activeProjectPath);` e adaptar a função local de submit pra invocar `lap.lapidate(applyToForm)` onde `applyToForm` é o setState que já existe
- [ ] Editar `prd_lapidator.rs`: declarar `const CLAUDE_MODEL: &str = "claude-sonnet-4-6";` no topo (após `use` statements, antes da primeira struct); trocar o literal `"claude-sonnet-4-6"` da linha 148 por `CLAUDE_MODEL`
- [ ] Validar: `pnpm --filter mustard-dashboard exec tsc -b` + `pnpm --filter mustard-dashboard build` + `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`

## Limites

- `apps/dashboard/src/components/prd/ConfrontBanner.tsx`
- `apps/dashboard/src/components/prd/IntentHero.tsx`
- `apps/dashboard/src/hooks/useLapidator.ts`
- `apps/dashboard/src/pages/Prd.tsx`
- `apps/dashboard/src-tauri/src/prd_lapidator.rs`

NÃO toca contratos JSON (PrdData/LapidatedPrd shape), NÃO altera comportamento visível ao usuário, NÃO mexe em outras páginas/rotas.
