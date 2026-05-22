# wave-2-general: Tauri command lapidate_prd

### Parent: [[2026-05-20-dashboard-prd-ai-lapidator]]
### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full
### Lang: pt
### Checkpoint: 2026-05-20T00:00:00Z

## PRD

## Contexto

O dashboard Mustard é uma app Tauri 2 (Rust backend em `apps/dashboard/src-tauri/`, frontend React 19 em `apps/dashboard/src/`). Tauri commands são funções Rust anotadas com `#[tauri::command]` que o frontend invoca via `invoke('nome', args)` — toda comunicação Rust↔TS passa por aí. Hoje há 11 módulos Rust em `src-tauri/src/` (discovery, projects, telemetry, etc.) registrados em `lib.rs` no `invoke_handler!`.

Esta wave adiciona um novo módulo `prd_lapidator.rs` com dois Tauri commands:
1. `check_claude_available() -> bool` — testa `claude --version` (silenciosamente) e devolve disponibilidade
2. `lapidate_prd(intent, project_path) -> PrdData` — executa o `claude` CLI em modo print (`claude -p "/mustard:prd <intent>"`), captura o stdout JSON, parseia em `PrdData` tipado

**Execução 100% background no Windows.** A primeira tentativa ingênua de `Command::new("claude")` em Windows abre uma janela de console flash visível. Pra evitar isso, o módulo usa `CommandExt::creation_flags(0x08000000)` (CREATE_NO_WINDOW) condicional via `#[cfg(windows)]`. Em macOS/Linux essa flag é no-op.

A arquitetura é provider-pluggable: trait `PrdProvider` com impl `ClaudeCliProvider`. Não implementamos OpenRouter agora — a trait apenas garante que plugar depois é troca de struct, não refator.

## Métrica de sucesso

Invocar `lapidate_prd("add login refresh token", "C:\\path\\to\\project")` do frontend devolve um struct `PrdData` populado em ≤30s, **sem abrir nenhuma janela visível no Windows**, ou um erro tipado (`ClaudeNotFound | ClaudeError | Timeout | InvalidJson`) com mensagem clara.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: módulo Rust compila sem erro — Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [ ] AC-2: comandos registrados no invoke_handler de lib.rs — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');if(!f.includes('lapidate_prd')||!f.includes('check_claude_available')||!f.includes('mod prd_lapidator'))process.exit(1)"`
- [ ] AC-3: trait PrdProvider existe e ClaudeCliProvider implementa — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src-tauri/src/prd_lapidator.rs','utf8');['trait PrdProvider','struct ClaudeCliProvider','impl PrdProvider for ClaudeCliProvider'].forEach(s=>{if(!f.includes(s)){console.error('missing:',s);process.exit(1)}})"`
- [ ] AC-4: struct PrdData deriva Serialize/Deserialize — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src-tauri/src/prd_lapidator.rs','utf8');if(!f.includes('derive')||!f.includes('Serialize')||!f.includes('Deserialize')||!f.includes('struct PrdData'))process.exit(1)"`
- [ ] AC-5: CREATE_NO_WINDOW aplicado em Windows — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src-tauri/src/prd_lapidator.rs','utf8');['cfg(windows)','creation_flags','0x08000000'].forEach(s=>{if(!f.includes(s)){console.error('missing:',s);process.exit(1)}})"`

## Plano

## Arquivos

- `apps/dashboard/src-tauri/src/prd_lapidator.rs` (novo, ~180 linhas) — módulo com trait, providers, dois Tauri commands, tipos, no-window helper
- `apps/dashboard/src-tauri/src/lib.rs` (edit, +6 linhas) — `mod prd_lapidator;` + dois entries no `invoke_handler!`

## Tarefas

### general Agent (Wave 2)

- [ ] Criar `apps/dashboard/src-tauri/src/prd_lapidator.rs` com:
  - `use std::process::Command; use std::path::Path; use std::time::Duration; use serde::{Serialize, Deserialize};`
  - Helper `fn no_window_command(program: &str) -> Command`:
    ```rust
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
    ```
  - `struct PrdData { type_: String, slug: String, title: String, scope: String, summary: String, why: Option<String>, layers: PrdLayers, boundaries: Vec<String>, checklist: Vec<String>, acceptance_criteria: Vec<PrdAc>, decisions_not_obvious: Option<Vec<String>>, non_goals: Option<Vec<String>>, _confront: PrdConfront }` com `#[derive(Serialize, Deserialize, Clone)]` e `#[serde(rename_all = "camelCase")]`
  - Structs auxiliares: `PrdLayers { backend: bool, frontend: bool, database: bool, design: bool, docs: bool, testes: bool }`, `PrdAc { title: String, command: String }`, `PrdConfront { entities_found: Vec<String>, entities_missing: Vec<String>, paths_exist: Vec<String>, paths_missing: Vec<String> }`
  - `enum PrdError { ClaudeNotFound, ClaudeError(String), Timeout, InvalidJson(String) }` com `impl std::fmt::Display` e `impl From<PrdError> for String`
  - `trait PrdProvider { fn lapidate(&self, intent: &str, project_path: &Path) -> Result<PrdData, PrdError>; }`
  - `struct ClaudeCliProvider;` com `impl PrdProvider`:
    - `let mut cmd = no_window_command("claude");`
    - `cmd.args(["-p", &format!("/mustard:prd {}", intent), "--output-format", "json", "--model", "claude-sonnet-4-6"]).current_dir(project_path);`
    - Spawn + timeout 60s (via `wait_timeout` crate ou impl manual com thread + channel — preferir `wait_timeout` se já estiver nas deps)
    - Map `io::ErrorKind::NotFound` → `PrdError::ClaudeNotFound`
    - Se status != 0 → `PrdError::ClaudeError(stderr_as_string)`
    - Parse stdout: `serde_json::from_slice(&output.stdout).map_err(|e| PrdError::InvalidJson(format!("{}: {}", e, String::from_utf8_lossy(&output.stdout))))`
  - `#[tauri::command] pub async fn check_claude_available() -> bool`:
    - `no_window_command("claude").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)`
  - `#[tauri::command] pub async fn lapidate_prd(intent: String, project_path: String) -> Result<PrdData, String>`:
    - Validar input não-vazio (`intent.trim().is_empty()` → erro)
    - Instanciar `ClaudeCliProvider` e chamar `lapidate(&intent, Path::new(&project_path))`
    - Mapear erro via `to_string()`
- [ ] Verificar se `wait_timeout` crate já está em `Cargo.toml`; se não, adicionar (`wait_timeout = "0.2"`)
- [ ] Editar `apps/dashboard/src-tauri/src/lib.rs`:
  - Adicionar `mod prd_lapidator;` junto aos outros `mod`
  - Adicionar `prd_lapidator::lapidate_prd` e `prd_lapidator::check_claude_available` no `tauri::generate_handler![...]`
- [ ] Build: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml` deve passar
- [ ] Smoke test manual (opcional, fora do CI): rodar `pnpm --filter mustard-dashboard tauri:dev`, abrir DevTools, executar `await window.__TAURI__.invoke('check_claude_available')` — esperar `true` se `claude` no PATH; `false` caso contrário; sem janela flash em Windows

## Limites

- `apps/dashboard/src-tauri/src/prd_lapidator.rs`
- `apps/dashboard/src-tauri/src/lib.rs`

Esta wave NÃO toca o frontend React, NÃO toca CLI/templates, NÃO toca outras crates Rust.

## Dependências

- Wave 1 — sem o slash command `/mustard:prd` registrado, o `claude -p "/mustard:prd ..."` retorna erro. O build/AC desta wave passam mesmo sem a Wave 1 (compilação Rust não exercita o command); smoke test real depende de Wave 1.
