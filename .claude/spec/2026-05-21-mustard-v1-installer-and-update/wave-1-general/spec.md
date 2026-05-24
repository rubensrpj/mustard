# Wave 1 — Foundation rename e versão 1.0.0

### Wave: 1
### Role: general

## PRD

## Contexto

Hoje o crate Rust do app desktop se chama `mustard-dashboard` e vive em `apps/dashboard/`, enquanto o `tauri.conf.json` declara `productName: "Mustard Dashboard"` e `identifier: "com.atiz.mustard-dashboard"`. As versões dos quatro crates do workspace estão desalinhadas — `mustard-cli` ainda carrega `3.1.36` da era npm/bun (eliminada na spec `2026-05-19-eliminate-bun`), enquanto `mustard-rt`, `mustard-dashboard` e `mustard-core` estão em `0.1.0`. Isso impede tratar o conjunto como "Mustard v1.0.0" pra uma release única; e o nome "Dashboard" no produto contradiz o posicionamento decidido (memory user: brand="Mustard", site URL="mustardia.com"). Esta wave faz o rename mecânico cross-cutting e unifica a versão como `1.0.0`, criando a base que as demais waves precisam.

## Métrica de sucesso

`cargo check --workspace` passa após o rename, `cargo run -p mustard-app --bin mustard-app -- --help` (ou equivalente Tauri) abre o app com título "Mustard", e `cargo pkgid` em cada crate retorna `1.0.0`.

## Critérios de Aceitação

- [ ] AC-W1-1: Workspace inteiro compila — Command: `cargo check --workspace`
- [ ] AC-W1-2: Os 4 crates estão em `1.0.0` — Command: `node -e "const fs=require('fs');const want='1.0.0';const files=['apps/cli/Cargo.toml','apps/rt/Cargo.toml','apps/app/src-tauri/Cargo.toml','packages/core/Cargo.toml'];const bad=files.filter(f=>!new RegExp('^version\\\\s*=\\\\s*\\\"'+want+'\\\"','m').test(fs.readFileSync(f,'utf8')));if(bad.length){console.error(bad);process.exit(1)}"`
- [ ] AC-W1-3: `apps/dashboard/` não existe mais; `apps/app/` existe — Command: `node -e "const fs=require('fs');if(fs.existsSync('apps/dashboard')){console.error('apps/dashboard still exists');process.exit(1)};if(!fs.existsSync('apps/app/src-tauri/Cargo.toml')){console.error('apps/app/src-tauri/Cargo.toml missing');process.exit(1)}"`
- [ ] AC-W1-4: `tauri.conf.json` tem productName "Mustard" + identifier "com.atiz.mustard" — Command: `node -e "const j=JSON.parse(require('fs').readFileSync('apps/app/src-tauri/tauri.conf.json','utf8'));if(j.productName!=='Mustard'||j.identifier!=='com.atiz.mustard'){console.error(JSON.stringify({productName:j.productName,identifier:j.identifier}));process.exit(1)}"`

## Plano

## Summary

Rename mecânico de `apps/dashboard/` → `apps/app/`, do crate `mustard-dashboard` → `mustard-app` (incluindo o `_lib` suffix), do package npm correspondente, atualização de identifier/productName no Tauri, bump de versão pra `1.0.0` em todos os crates do workspace, e propagação das referências em CLAUDE.md/README.md/scripts/pnpm-workspace.yaml. Limpa o arquivo duplicado `apps/dashboard/src-tauri/src-tauri/Cargo.toml` se ainda existir após o rename (artefato antigo de scaffolding).

## Checklist

### General Agent

- [ ] Rename diretório `apps/dashboard/` → `apps/app/` (via `git mv` quando possível pra preservar history)
- [ ] `apps/app/Cargo.toml` (se houver, ou apenas `apps/app/src-tauri/Cargo.toml`): `name = "mustard-app"`, `version = "1.0.0"`
- [ ] `apps/app/src-tauri/Cargo.toml`: `name = "mustard-app"`, `version = "1.0.0"`, `[lib].name = "mustard_app_lib"`
- [ ] `apps/app/src-tauri/tauri.conf.json`: `productName: "Mustard"`, `identifier: "com.atiz.mustard"`, atualizar `windows[0].title` para "Mustard"
- [ ] `apps/app/package.json`: `name: "mustard-app"`, scripts iguais
- [ ] `apps/cli/Cargo.toml`: `version = "1.0.0"` (era `3.1.36`)
- [ ] `apps/rt/Cargo.toml`: `version = "1.0.0"`
- [ ] `packages/core/Cargo.toml`: `version = "1.0.0"`
- [ ] `Cargo.toml` (workspace root): ajustar `members = [...]` para apontar pra `apps/app/src-tauri` (era `apps/dashboard/src-tauri`)
- [ ] `pnpm-workspace.yaml`: atualizar paths `apps/dashboard` → `apps/app`
- [ ] `package.json` (root): renomear scripts `build:dashboard` → `build:app`, `test:dashboard` → `test:app`, `dashboard:dev` → `app:dev`, `dashboard:build` → `app:build`; em todos atualizar `--filter mustard-dashboard` → `--filter mustard-app`
- [ ] Atualizar `apps/cli/templates/CLAUDE.md` e `CLAUDE.md` raiz + `.claude/CLAUDE.md` + `apps/app/CLAUDE.md` referenciando `apps/dashboard` → `apps/app`
- [ ] Atualizar `apps/app/src-tauri/Cargo.toml` `description` se ainda for "A Tauri App" para "Mustard desktop app"
- [ ] Deletar arquivo `apps/app/src-tauri/src-tauri/Cargo.toml` (artefato duplicado de scaffolding, se existir após rename)
- [ ] Atualizar `Cargo.lock` (deixar `cargo check --workspace` regenerar)
- [ ] Atualizar workflow `.github/workflows/dashboard-release.yml` cita `mustard-dashboard` — manter por enquanto (Wave 2 substitui por release.yml unificado)
- [ ] Build/type-check: `cargo check --workspace` deve passar
- [ ] Sanity: `pnpm install` no root re-resolve workspace

## Files (~12)

```
apps/dashboard/                                       → renomeado apps/app/
apps/app/src-tauri/Cargo.toml
apps/app/src-tauri/tauri.conf.json
apps/app/package.json
apps/cli/Cargo.toml
apps/rt/Cargo.toml
packages/core/Cargo.toml
Cargo.toml (root)
pnpm-workspace.yaml
package.json (root)
CLAUDE.md (root + .claude/ + apps/app/)
README.md (se referenciar apps/dashboard)
```
