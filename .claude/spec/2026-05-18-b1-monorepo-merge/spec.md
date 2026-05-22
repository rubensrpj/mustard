# Feature: b1-monorepo-merge

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-19T00:15:15Z
### Lang: pt

> Parte B, item B1 — raiz. Refinada em 2026-05-18: as três decisões de merge foram fechadas (ver Resumo). Aprovável. **Parte A concluída — B pode começar.**

## Contexto

Hoje `mustard` (a CLI, em `C:\Atiz\mustard`) e `mustard-dashboard` (o app Tauri, em `C:\Atiz\mustard-dashboard`) são repositórios separados. O dashboard consome o contrato `.claude/` — `events.jsonl`, `pipeline-states`, `entity-registry.json` — que a CLI gera, mas esse contrato vive sem versionamento conjunto: uma mudança na CLI pode quebrar o dashboard sem nenhum sinal. Unir os dois num repositório único torna o contrato interno e versionado junto, e prepara o terreno para a migração Rust e a distribuição via Tauri. Esta é a fundação estrutural da Parte B — nada mais dela acontece sem isto.

## Resumo

Reestruturar o repositório `mustard` como monorepo. **Três decisões fechadas no refino:**

1. **Histórico git:** o `mustard` permanece como o repositório raiz (histórico intacto); o código da CLI move para `packages/cli/` via `git mv`. O `mustard-dashboard` entra em `apps/dashboard/` via `git subtree add --prefix=apps/dashboard`, preservando o histórico dele.
2. **Gerenciador:** **pnpm workspace** único (`pnpm-workspace.yaml` cobrindo `packages/*` e `apps/*`). A CLI migra de npm → pnpm; o dashboard já usa pnpm. Bun continua como *runtime* da CLI/hooks/scripts — gerenciador ≠ runtime. Um `Cargo.toml` de workspace coexiste (o `apps/dashboard/src-tauri` já é crate; B2+ adicionam outros).
3. **`.claude/`:** o `.claude/` atual do `mustard` (specs, pipeline-states, orquestrador) **permanece na raiz** — é o orquestrador do monorepo. O `.claude/` do dashboard vira `apps/dashboard/.claude/` (contexto de subprojeto, chega via subtree). `packages/cli/` recebe contexto de subprojeto gerado por `/scan` pós-merge.

## Entidades

N/A — reestruturação de repositório.

## Component Contract

N/A.

## Arquivos

- Raiz: `package.json` (workspace), `pnpm-workspace.yaml`, `Cargo.toml` (workspace), `.gitignore` consolidado
- `packages/cli/` — destino de `bin/`, `src/`, `dist/`, `templates/` + `package.json`/`tsconfig` da CLI
- `apps/dashboard/` — destino do `mustard-dashboard` (via subtree)
- `.claude/` raiz — permanece (orquestrador)
- `CLAUDE.md` raiz, `mustard.json` — ajustar para a estrutura monorepo

## Limites

- Estrutura de diretórios da raiz, `packages/cli/`, `apps/dashboard/`
- **Fora dos limites:** lógica de qualquer hook/script/comando (só movem de lugar); a migração Rust (B2-B5); a reformulação do dashboard (B6); o `.claude/` raiz (permanece, não é tocado pela movimentação).

## Tarefas

### Templates Agent (Wave 1) — estrutura, workspace e import

- [x] Criar o layout: `packages/cli/`, `apps/dashboard/`. Criar `pnpm-workspace.yaml` (`packages: ['packages/*', 'apps/*']`) e `Cargo.toml` de workspace na raiz.
- [x] `git mv` do código da CLI (`bin/`, `src/`, `dist/`, `templates/` + `package.json`, `tsconfig`, configs) para `packages/cli/`. O `.claude/` raiz NÃO move.
- [x] `git subtree add --prefix=apps/dashboard <repo do mustard-dashboard> main` — importa o dashboard com histórico (branch trunk `main`).
- [x] Migrar a CLI de npm → pnpm: remover `package-lock.json`; gerar `pnpm-lock.yaml` no workspace. Remover o `bun.lock` órfão do dashboard (ele usa pnpm).
- [x] Consolidar `.gitignore` na raiz.

### Templates Agent (Wave 2) — validação e re-scan

- [x] Rodar `/mustard:scan` para o orquestrador raiz redescobrir os 2 subprojetos (`packages/cli`, `apps/dashboard`) e gerar o contexto de cada um.
- [x] Validar build isolado: `packages/cli` (build atual da CLI) e `apps/dashboard` (`pnpm build`).
- [x] Ajustar caminhos quebrados pela movimentação (referências relativas em scripts, paths em configs/CI).
- [x] Atualizar `CLAUDE.md` raiz + a tabela Project Structure com o layout monorepo.

## Dependências

- Parte A concluída ✓.
- Raiz da Parte B: B2-B6 dependem desta.

## Preocupações

- **Estado do dashboard antes do subtree:** o `git subtree add` precisa do `mustard-dashboard` acessível (path local) e commitado. Confirmar no início do EXECUTE que o repo está sem mudanças pendentes.
- **`dist/`:** é build-output. Verificar no EXECUTE se é versionado hoje — se for, decidir mover vs `.gitignore`.
- **Docs raiz** (`README`/`CHANGELOG`/`TUTORIAL`/`curso-mustard.html`): default = ficam na raiz como visão do monorepo. Reavaliar no EXECUTE se algo é específico da CLI.

## Critérios de Aceitação

- [x] AC-1: O layout monorepo existe — Command: `node -e "const fs=require('fs');['packages/cli','apps/dashboard','pnpm-workspace.yaml'].forEach(p=>{if(!fs.existsSync(p))process.exit(1)})"`
- [x] AC-2: A CLI builda no novo local — Command: `bash -c 'cd packages/cli && pnpm build'`
- [x] AC-3: O dashboard builda no novo local — Command: `bash -c 'cd apps/dashboard && pnpm build'`
- [x] AC-4: O histórico do dashboard foi preservado — Command: `bash -c 'test -n "$(git log --oneline -- apps/dashboard | head -1)"'`

## Notas de Execução

- **Import do dashboard (desvio consciente):** o tip do `main` do `mustard-dashboard` (`606d569`) estava mid-refactor — ~14 componentes e deps não-commitados. O `subtree add` inicial importou só o commitado e o build (AC-3) falhou. Com decisão do usuário, as mudanças em andamento foram commitadas no repo de origem (`0eb9c74`) e trazidas via `subtree pull` (`a62a633`); AC-3 passou. O repo antigo `C:/Atiz/mustard-dashboard` fica obsoleto — trabalho futuro do dashboard segue no monorepo.
- **`/mustard:scan`:** a redescoberta usou `sync-detect.js` + `sync-registry.js` (2 subprojetos confirmados: `apps/dashboard` role `ui`, `packages/cli` detectado via `templates/CLAUDE.md`). O `sync-compile.js` foi removido em commit anterior (`6b12620`) e não foi recriado. Geração profunda de skills/recipes por subprojeto via `/mustard:scan` completo fica como follow-up recomendado.
- **`dist/`:** não era versionado — confirmado; nada a mover, regenera no build.
- **Docs raiz** (`README`/`CHANGELOG`/`TUTORIAL`/`curso-mustard.html`, `docs/`, `assets/`): mantidos na raiz como visão do monorepo, conforme default.
- **Limpeza:** arquivos de estado efêmero rastreados que entraram na importação (`.compact-state/`, `.harness/`, `.claude.backup.*`, `.metrics/` aninhados) foram removidos do índice e cobertos no `.gitignore`.
- **Sequência de commits:** `6880b1d` (move CLI + workspace) → `5dcda97` (subtree add) → `7e3ca7d` (pnpm + gitignore) → `54f99a2` (CI + CLAUDE.md + registry) → `a62a633`/`fad1f17` (import WIP dashboard + lockfile) → limpeza de estado efêmero.

## Não-Objetivos

- Não reescrever nada em Rust — só movimentação (isso é B2-B5).
- Não mudar comportamento de CLI nem dashboard.
- Não publicar o monorepo em npm.
- Não tocar o `.claude/` raiz — ele permanece como orquestrador.
