# Wave 3 — PATH integration per-SO (NSIS, postinst, macOS first-run)

### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full (wave)
### Wave: 3
### Role: general
### Checkpoint: 2026-05-21T18:00:00Z
### Lang: pt
### Parent: 2026-05-21-mustard-v1-installer-and-update

## PRD

## Contexto

O instalador empacotado pela Wave 2 vai entregar `mustard`, `mustard-rt` e `rtk` dentro do diretório de instalação, mas isso não é o bastante — eles precisam ser acessíveis como comandos de terminal de qualquer lugar. No Windows isso é uma entrada `HKCU\Environment\Path` apontando pra `<install>\bin`; no Linux é symlinks em `/usr/local/bin/`; no macOS é symlinks em `/usr/local/bin/` que exigem elevation (`/usr/local/bin/` é root-owned por padrão). Cada plataforma tem mecanismo nativo (NSIS hook Windows, postinst .deb/.rpm Linux, app first-run dialog macOS via Authorization Services). Esta wave cria os 3 scripts/módulos. O Tauri 2 já tem hooks NSIS (`bundle.windows.nsis.installerHooks`) e `.deb`/`.rpm` postinst (`bundle.linux.deb.files`); o macOS precisa de código Rust no app que detecta no startup que `/usr/local/bin/mustard` não existe e oferece criar via `osascript -e "do shell script ... with administrator privileges"`.

## Métrica de sucesso

Em cada SO após instalação fresca: abrir terminal novo → `mustard --version` retorna `mustard 1.0.0`. No macOS especificamente, primeira abertura do app mostra diálogo "Install command-line tools? Esta operação pede senha de administrador." e clicar "Install" + autenticar resulta em `which mustard` retornando `/usr/local/bin/mustard`.

## Critérios de Aceitação

- [ ] AC-W3-1: Arquivo `apps/app/src-tauri/installer/windows-path.nsh` existe e escreve em HKCU Environment Path — Command: `node -e "const s=require('fs').readFileSync('apps/app/src-tauri/installer/windows-path.nsh','utf8');if(!/HKCU/.test(s)||!/Environment/.test(s)){console.error('NSIS script does not write registry');process.exit(1)}"`
- [ ] AC-W3-2: Arquivo `apps/app/src-tauri/installer/linux-postinst.sh` existe e cria symlinks em `/usr/local/bin/` — Command: `node -e "const s=require('fs').readFileSync('apps/app/src-tauri/installer/linux-postinst.sh','utf8');if(!/ln -sf/.test(s)||!/\\/usr\\/local\\/bin/.test(s)){console.error('postinst missing symlink');process.exit(1)}"`
- [ ] AC-W3-3: Módulo Rust `path_check` compila e expõe função pública — Command: `cargo test -p mustard-app path_check`
- [ ] AC-W3-4: Tauri command `install_cli_tools_macos` registrado em invoke_handler — Command: `node -e "const s=require('fs').readFileSync('apps/app/src-tauri/src/lib.rs','utf8');if(!/install_cli_tools_macos/.test(s)){console.error('install_cli_tools_macos not registered');process.exit(1)}"`

## Plano

## Summary

Cria 3 arquivos não-Rust (NSIS hook, postinst .sh, postrm .sh) que são copiados pelo Tauri bundler para os instaladores correspondentes; cria 2 módulos Rust no `src-tauri/src/` — `path_check.rs` (detecta presença dos 4 binários no PATH via `which`) e `cli_tools_installer.rs` (Tauri command que no macOS chama `osascript -e "do shell script 'ln -sf ... && ln -sf ...' with administrator privileges"`). Registra os comandos no `lib.rs::run()` invoke_handler.

## Checklist

### General Agent

- [ ] Criar `apps/app/src-tauri/installer/windows-path.nsh`:
  - Hook `customInstall` ou equivalente NSIS Tauri
  - Lê valor atual de `HKCU\Environment\Path`
  - Adiciona `$INSTDIR\bin` se ainda não estiver
  - Envia mensagem broadcast WM_SETTINGCHANGE pra outros processos saberem do update
  - Hook `customUnInstall` pra remover na desinstalação
- [ ] Criar `apps/app/src-tauri/installer/linux-postinst.sh`:
  - `#!/bin/sh` shebang
  - `ln -sf /opt/Mustard/bin/mustard /usr/local/bin/mustard` (idempotente)
  - Mesmo pra `mustard-rt` e `rtk`
  - `exit 0`
- [ ] Criar `apps/app/src-tauri/installer/linux-postrm.sh`:
  - `rm -f /usr/local/bin/mustard /usr/local/bin/mustard-rt /usr/local/bin/rtk` (somente quando purge)
  - `exit 0`
- [ ] Criar `apps/app/src-tauri/src/path_check.rs`:
  - `pub enum BinaryStatus { Present { version: String, path: String }, Missing }`
  - `pub struct PrereqStatus { pub mustard, pub mustard_rt, pub rtk, pub claude_code }` (4 campos `BinaryStatus`)
  - `pub fn check_prereqs() -> PrereqStatus` — usa `which` crate ou `std::process::Command` pra encontrar binários
  - Cada binário: roda `<name> --version` (com `process_util::no_window_command`) e extrai versão por regex
  - Tauri command `#[tauri::command] async fn prereq_status() -> PrereqStatus`
  - Testes unitários cobrindo Present/Missing
- [ ] Criar `apps/app/src-tauri/src/cli_tools_installer.rs`:
  - Tauri command `#[tauri::command] async fn install_cli_tools_macos() -> Result<(), String>`
  - Determina caminho dos binários (dentro do `.app/Contents/Resources/` em prod, ou ENV de dev)
  - Constrói script: `ln -sf <bin>/mustard /usr/local/bin/mustard && ln -sf <bin>/mustard-rt /usr/local/bin/mustard-rt && ln -sf <bin>/rtk /usr/local/bin/rtk`
  - Roda via `osascript -e "do shell script \"<script>\" with administrator privileges"` (cuidar com quoting)
  - Tauri command `#[tauri::command] async fn install_cli_tools_status() -> bool` — só checa se symlinks existem
  - Linux/Windows: command retorna `Err("not supported on this platform")` — frontend não chama nesses SOs
- [ ] Registrar `mod path_check;` e `mod cli_tools_installer;` em `apps/app/src-tauri/src/lib.rs`
- [ ] Adicionar `path_check::prereq_status`, `cli_tools_installer::install_cli_tools_macos`, `install_cli_tools_status` no `invoke_handler!` macro
- [ ] Adicionar dep `which = "6"` ou usar `std::process::Command` direto (preferir `std`)
- [ ] Build/type-check: `cargo check -p mustard-app`
- [ ] Atualizar `tauri.conf.json` referências de installer hooks (criadas em Wave 2 mas escalonando aqui se Wave 2 deixou placeholder)

## Files (~6)

```
apps/app/src-tauri/installer/windows-path.nsh         — NOVO
apps/app/src-tauri/installer/linux-postinst.sh        — NOVO
apps/app/src-tauri/installer/linux-postrm.sh          — NOVO
apps/app/src-tauri/src/path_check.rs                  — NOVO
apps/app/src-tauri/src/cli_tools_installer.rs         — NOVO
apps/app/src-tauri/src/lib.rs                         — mod + invoke_handler
```
