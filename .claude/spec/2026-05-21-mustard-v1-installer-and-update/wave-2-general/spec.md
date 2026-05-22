# Wave 2 — CI release workflow multi-SO + RTK bundle

### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full (wave)
### Wave: 2
### Role: general
### Checkpoint: 2026-05-21T18:00:00Z
### Lang: pt
### Parent: 2026-05-21-mustard-v1-installer-and-update

## PRD

## Contexto

O workflow `.github/workflows/dashboard-release.yml` (criado nesta sessão) builda só o app Tauri sem empacotar os binários `mustard`, `mustard-rt` e `rtk` junto. O `.github/workflows/ci.yml` ainda referencia `packages/cli` — diretório removido na migração para `apps/cli` (memory `project_monorepo_layout`). Esta wave substitui ambos por um workflow unificado `release.yml` que (a) constrói a matriz cross-SO (Windows + Linux + macOS Intel + macOS ARM), (b) instala `mustard-cli`/`mustard-rt` via `cargo install --path` e o `rtk` upstream via `cargo install rtk` em cada runner, (c) copia os 3 binários pro diretório `bundle/external/` que o Tauri 2 bundler inclui no instalador via `externalBin`, e (d) gera os artefatos `.msi/.exe/.dmg/.deb/.rpm/.AppImage` no GitHub Releases (draft) com tag pattern `mustard-v*`.

## Métrica de sucesso

Empurrar tag `mustard-v1.0.0` dispara workflow que gera 6 artefatos (msi+exe+dmg-arm+dmg-x64+deb+AppImage+rpm) anexados a release draft. Workflow_dispatch manual também funciona e sobe artefatos como workflow artifacts (sem release). `ci.yml` deixa de quebrar por referência stale.

## Critérios de Aceitação

- [ ] AC-W2-1: `release.yml` existe com matriz dos 4 alvos — Command: `node -e "const s=require('fs').readFileSync('.github/workflows/release.yml','utf8');const need=['windows-latest','ubuntu-22.04','macos-latest','aarch64-apple-darwin','x86_64-apple-darwin'];const miss=need.filter(x=>!s.includes(x));if(miss.length){console.error(miss);process.exit(1)}"`
- [ ] AC-W2-2: `release.yml` instala rtk via cargo — Command: `node -e "const s=require('fs').readFileSync('.github/workflows/release.yml','utf8');if(!/cargo install rtk/.test(s)){console.error('rtk install step missing');process.exit(1)}"`
- [ ] AC-W2-3: `release.yml` aciona em tag `mustard-v*` e workflow_dispatch — Command: `node -e "const s=require('fs').readFileSync('.github/workflows/release.yml','utf8');if(!/mustard-v\\*/.test(s)||!/workflow_dispatch/.test(s)){console.error('triggers missing');process.exit(1)}"`
- [ ] AC-W2-4: `dashboard-release.yml` deletado — Command: `node -e "if(require('fs').existsSync('.github/workflows/dashboard-release.yml')){process.exit(1)}"`
- [ ] AC-W2-5: `ci.yml` sem referência a `packages/cli` — Command: `node -e "const s=require('fs').readFileSync('.github/workflows/ci.yml','utf8');if(s.includes('packages/cli')){process.exit(1)}"`
- [ ] AC-W2-6: `tauri.conf.json` declara `externalBin` com os 3 binários — Command: `node -e "const j=JSON.parse(require('fs').readFileSync('apps/app/src-tauri/tauri.conf.json','utf8'));const eb=j.bundle&&j.bundle.externalBin||[];const want=['mustard','mustard-rt','rtk'];const miss=want.filter(b=>!eb.some(e=>e.includes(b)));if(miss.length){console.error('externalBin missing',miss);process.exit(1)}"`

## Plano

## Summary

Cria `release.yml` matricial baseado em `tauri-apps/tauri-action@v0` (validado via Context7), pinned em RTK upstream via `cargo install rtk --version X.Y.Z` (escolher versão estável durante implementação), Linux instala system deps + adicional para .deb postinst hooks, declara externalBin no `tauri.conf.json` para que o Tauri bundler inclua `mustard`, `mustard-rt` e `rtk` nos instaladores. Remove `dashboard-release.yml` que foi superseded. Limpa `ci.yml` para apontar pra `apps/cli` correto.

## Checklist

### General Agent

- [ ] Criar `.github/workflows/release.yml` com:
  - Trigger: `push: tags: mustard-v*` + `workflow_dispatch`
  - Matriz: macos-latest (target aarch64-apple-darwin), macos-latest (target x86_64-apple-darwin), ubuntu-22.04, windows-latest
  - Setup steps: pnpm 10.18.1, Node 20, Rust stable com targets corretos pra macOS
  - Cache de Rust + pnpm
  - Linux: `apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev build-essential`
  - Step "Build CLI binaries": `cargo install --path apps/cli --root tmp/bins && cargo install --path apps/rt --root tmp/bins && cargo install rtk --version <pin> --root tmp/bins`
  - Step "Stage externalBin": copia os 3 binários (`mustard`, `mustard-rt`, `rtk` + variantes `.exe` no Windows) pra `apps/app/src-tauri/bin/` com naming convention do Tauri (`<name>-<target-triple>`)
  - Step `pnpm install --frozen-lockfile`
  - Step `tauri-apps/tauri-action@v0` com `projectPath: apps/app`, `tagName: mustard-v__VERSION__`, `releaseDraft: true`, args = matrix.args
  - Step `actions/upload-artifact@v4` (condicional `workflow_dispatch`) com bundle paths
- [ ] Atualizar `apps/app/src-tauri/tauri.conf.json`:
  - Adicionar `bundle.externalBin: ["bin/mustard", "bin/mustard-rt", "bin/rtk"]`
  - Adicionar `bundle.windows.nsis.installerHooks` apontando pra `installer/windows-path.nsh` (Wave 3 cria o arquivo)
  - Adicionar `bundle.linux.deb.files` ou config equivalente pra postinst (Wave 3 cria os scripts)
  - Confirmar `bundle.targets: "all"`
- [ ] Deletar `.github/workflows/dashboard-release.yml`
- [ ] Atualizar `.github/workflows/ci.yml`:
  - Remover referências a `packages/cli` (defaults working-directory)
  - Apontar pra `apps/cli` quando aplicável
  - Manter advisory jobs separados pra Linux + Windows
- [ ] Build/type-check: `cargo check --workspace` (sanity)
- [ ] Validação local opcional: rodar parte do workflow via `act` se disponível, ou validar sintaxe YAML

## Files (~3)

```
.github/workflows/release.yml                         — NOVO
.github/workflows/dashboard-release.yml               — REMOVIDO
.github/workflows/ci.yml                              — cleanup packages/cli
apps/app/src-tauri/tauri.conf.json                    — externalBin + bundle config
```
