# Feature: b5-cli-to-rust

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-19T15:10:00Z
### Lang: pt

> Parte B, item B5. Refinada pelo ANALYZE em 2026-05-19: o `src/` real foi inventariado (12 arquivos `.ts`), as fronteiras com `mustard-core` foram confirmadas e as decisões em aberto (mcp-memory, migrate, carimbo de versão) foram resolvidas. Depende de B2; B3/B4 concluídos.

## Contexto

A CLI do Mustard — `init`, `update`, `add`, `config`, `review` — é TypeScript executado via Bun, hoje em `packages/cli/src/*.ts`. Para o app Tauri instalar o Mustard ao selecionar uma pasta, essa lógica precisa rodar a partir do app, e o backend do Tauri é Rust. Portar a CLI para Rust elimina a necessidade de um sidecar ou de embarcar um runtime: o app Tauri e o motor de instalação passam a ser o mesmo workspace Rust, e o `init` vira uma chamada nativa. O `init`/`update` também passam a gravar um carimbo de versão no `.claude/` do projeto — algo que hoje não existe e que o dashboard (B6) precisa para detectar a versão instalada.

## Resumo

Portar a CLI de `packages/cli/src/*.ts` para Rust, consumindo `mustard-core`. O crate Rust ocupa o próprio `packages/cli/` (crate `mustard-cli`, binário `mustard` + `lib` para o Tauri), parsing via `clap`, edition 2024; `packages/cli/templates/` permanece como payload de dados, intocado. **Escopo real do porte (9 arquivos, ~1.786 LOC):** `cli.ts`, `commands/{init,update,add,config,review,auto-update}.ts`, `runtime/detect-runtime.ts`, `services/npm.ts`. O `init`/`update` passam a gravar um campo `version` em `mustard.json`. Após o porte, o app Tauri invoca o `init` nativamente via `lib`.

## Entidades

N/A — infraestrutura de CLI.

## Component Contract

N/A.

## Arquivos

- `packages/cli/Cargo.toml` — crate `mustard-cli`: `[[bin]] name="mustard"` + `[lib]`; edition 2024; deps de `workspace.dependencies` (`clap`, `anyhow`, `serde`, `serde_json`, `jiff`) + `mustard-core` como path dep
- `Cargo.toml` raiz — registrar `packages/cli` em `members`
- `packages/cli/src/` — código Rust da CLI; substitui o `src/*.ts` portado
- `packages/cli/templates/` — payload copiado por `init`; permanece markdown/JSON, intocado
- Campo `version` em `mustard.json` (`.claude/`), gerado por `init`/`update`

## Limites

- `packages/cli/` (código da CLI Rust + `Cargo.toml`), o carimbo de versão, `Cargo.toml` raiz, e o ponto de chamada em `apps/dashboard/src-tauri` (Wave 3, só a função `init`/`update`).
- **Fora dos limites:** os hooks/scripts (B3/B4); o conteúdo de `packages/cli/templates/` (payload, não código); a UI de instalação do Tauri (B6).

## Tarefas

### Impl Agent (Wave 1) — scaffold + init

- [x] Criar `packages/cli/Cargo.toml` (`mustard-cli`, bin `mustard` + lib, edition 2024) e registrar `packages/cli` nos `members` do `Cargo.toml` raiz.
- [x] `cli.rs`/`main.rs` + `lib.rs`: parsing `clap` com subcomandos `init|update|add|config|review` e dispatch.
- [x] Módulo compartilhado `fs_ops`: cópia recursiva de diretório + merge cirúrgico de JSON (consumido por `init` e `update`).
- [x] Portar `services/npm.ts` → `npm.rs` (`get_latest_version`, `compare_versions`).
- [x] Portar `runtime/detect-runtime.ts` — confirmado vestigial; portada versão mínima (`runtime: {kind:"native", os, arch}`).
- [x] Portar `init`: scan de stack, cópia de `templates/` → `.claude/`, geração de `mustard.json` (incl. fluxo git interativo), perms globais, instalação RTK.
- [x] `init` grava o campo `version` em `mustard.json` via `env!("CARGO_PKG_VERSION")`.
- [x] Escrever a suíte de testes de `init` do zero (não há oráculo JS — ver Preocupações).

### Impl Agent (Wave 2) — update, add, config, review, auto-update

- [x] Portar `update` (backup + regeneração de core files preservando arquivos de usuário); re-grava o campo `version`.
- [x] Portar `config` (wrapper fino sobre o fluxo `mustard.json` do `init`).
- [x] Portar `add` e `review` — introduzem HTTP + extração de arquivos; adicionar os crates escolhidos a `workspace.dependencies`.
- [x] Portar `auto-update` (checagem de versão npm).

### Impl Agent (Wave 3) — integração Tauri

- [x] Expor a API `init`/`update` Rust via `lib` do crate `mustard-cli`.
- [x] Wire no backend Tauri (`apps/dashboard/src-tauri`): comandos `mustard_install`/`mustard_update` que invocam `mustard_cli::init`/`update` — sem sidecar.

## Dependências

- B2 (`mustard-core`) — **confirmado:** `packages/core` existe no workspace; expõe `io/event_store.rs`, `config.rs`, `model/`, etc.
- B3/B4 — **concluídos:** hooks e scripts já em Rust (`packages/rt`). A CLI não re-porta nada deles.

## Preocupações

- **CLAUDE.md desatualizado — RESOLVIDO:** não há `src/scanners/` nem `src/generators/`. O `src/` real tem 12 `.ts`; a lógica de scan de stack mora dentro de `init.ts`. (Atualizar a seção `## Structure` do CLAUDE.md raiz é tarefa de housekeeping fora desta spec.)
- **Fronteira `event-store.ts` — RESOLVIDO:** `packages/core/src/io/event_store.rs` já existe (B2). `runtime/event-store.ts` (842 LOC) **não** é portado aqui; a CLI consome o crate. Paridade funcional do `event_store.rs` é responsabilidade de B2.
- **`mcp/mustard-memory.ts` — RESOLVIDO:** não está wired no `.mcp.json` (só `context7`). Não portar — ver Não-Objetivos.
- **`migrate/jsonl-to-sqlite.ts` — RESOLVIDO:** utilitário one-off de import legado (682 LOC), não é CLI core. Não portar — ver Não-Objetivos.
- **`detect-runtime` pós-B3/B4 — NOVO:** hooks e scripts agora são binários Rust (`mustard-rt`). A detecção de runtime Bun/Node pode estar obsoleta ou muito reduzida. O agente de Wave 1 deve verificar se `runtime.chosen` em `mustard.json` ainda tem consumidor real antes de portar integralmente.
- **Superfície de dependências (Wave 2) — NOVO:** `add` (tarball), `review` (Claude API) e `auto-update` (registro npm) exigem cliente HTTP + extração de arquivos — crates novos. Preferir `ureq` (leve, bloqueante, sem runtime async) a `reqwest`; `tar` + `flate2` para extração. Adicionar a `workspace.dependencies`.
- **Paridade do `init`:** não há testes JS de `init` — o porte cria a suíte do zero. O oráculo é o comportamento observável: a árvore de `.claude/` gerada e o conteúdo de `mustard.json`.
- **RTK auto-installer deferido (Wave 1):** `ensure_rtk` foi portado em versão mínima (detecta `rtk` no PATH + roda `rtk init -g`). O auto-installer dual-plataforma (download zip no Windows / `curl`+`chmod` no Unix) foi deferido — é plumbing de plataforma sem efeito na árvore `.claude/`. Reavaliar no CLOSE se deve entrar na Wave 2 ou virar spec própria.

## Critérios de Aceitação

- [x] AC-1: A CLI Rust compila — Command: `bash -c 'cargo build -p mustard-cli'`
- [x] AC-2: `init` numa pasta limpa gera `.claude/` com o campo `version` em `mustard.json` — Command: `bash -c 'cargo test -p mustard-cli init'`
- [x] AC-3: O binário `mustard` expõe os subcomandos — Command: `bash -c 'cargo run -p mustard-cli -- --help | grep -qi init'`

## Não-Objetivos

- Não portar `mcp/mustard-memory.ts` — servidor MCP não wired; se voltar a ser necessário, será spec própria.
- Não portar `migrate/jsonl-to-sqlite.ts` — migração legada one-off; permanece script ou é descartado.
- Não re-portar `runtime/event-store.ts` — pertence a `mustard-core` (B2).
- Não portar o conteúdo de `packages/cli/templates/` — payload markdown/JSON.
- Não construir a UI de instalação — isso é B6.
- Não manter a CLI JS em paralelo após o porte concluído.
