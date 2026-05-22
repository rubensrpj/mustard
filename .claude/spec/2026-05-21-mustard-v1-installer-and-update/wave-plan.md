# Mustard v1 — Instalador multi-SO, PATH integration e update-notify

### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full (wave plan)
### Checkpoint: 2026-05-21T18:00:00Z
### Lang: pt
### Total waves: 5

## PRD

## Contexto

Hoje o Mustard é um conjunto de crates Rust + um app Tauri em `apps/dashboard/` que só compila localmente — não existe instalador para usuário final. Quem quer usar precisa clonar o monorepo, ter Rust toolchain instalado, rodar `cargo install --path apps/cli` + `cargo install --path apps/rt` separados, descobrir sozinho que RTK é dependência hard, e ainda assim não tem um app desktop empacotado. O cli em `apps/cli/Cargo.toml` ainda carrega versão `3.1.36` herdada da era npm/bun (eliminada na spec `2026-05-19-eliminate-bun`), enquanto os outros crates (`mustard-rt`, `mustard-dashboard`, `mustard-core`) ficaram em `0.1.0`. O app Tauri se chama "Mustard Dashboard" no `tauri.conf.json`, mas o usuário pensa só em "Mustard". O workflow de CI atual (`.github/workflows/ci.yml`) referencia `packages/cli` — diretório que não existe mais — e o workflow `dashboard-release.yml` criado nesta sessão builda só o app sem empacotar `mustard`/`mustard-rt`/`rtk` junto. Resultado: o produto não tem caminho de distribuição cohérent para Windows, macOS ou Linux, e quem instala o app não tem CLI funcional no terminal — o que quebra a promessa core do Mustard, já que os hooks instalados em cada `.claude/settings.json` chamam `mustard-rt` por nome esperando que esteja no `PATH`.

Esta feature empacota o Mustard como produto desktop multi-SO instalável que entrega no `PATH` quatro binários (`mustard`, `mustard-rt`, `rtk`, e o app GUI `mustard-app`), faz integração nativa por sistema operacional (script NSIS no Windows, diálogo first-run no macOS via Authorization Services, postinst no Linux .deb/.rpm), renomeia `mustard-dashboard` → `mustard-app`, unifica todos os crates do workspace na versão `1.0.0`, adiciona detector de novas releases via GitHub API (modo update-notify — sem code-signing nesta v1, sem auto-update silencioso), refatora o fluxo "Adicionar projeto" do app para detectar adaptativamente se a pasta já tem `.claude/` e oferecer instalar quando não tiver, e expõe banner persistente para projetos no registry que ficam com `.claude/` em versão menor que a do app.

## Usuários/Stakeholders

Quem hoje só consegue usar o Mustard clonando o repo e o quer instalar como ferramenta de verdade (Windows, macOS, Linux) com `mustard init` rodando direto do terminal e o app desktop abrindo sem ginástica. Inclui também novos usuários atraídos pelo site `mustardia.com` (fora do escopo desta spec) que precisam baixar um instalador e ter Mustard funcionando out-of-the-box, sem saber o que é cargo ou rustup.

## Métrica de sucesso

Um usuário Windows/macOS/Linux baixa o instalador correto pra plataforma dele a partir do GitHub Releases, instala, abre um terminal NOVO e roda `mustard --version` retornando `mustard 1.0.0`, abre o app Mustard que mostra welcome screen, clica "Adicionar projeto", escolhe uma pasta vazia, e ao final tem `.claude/` instalado naquela pasta + projeto registrado no app + log honesto mostrando o que foi feito. Quando uma nova release é publicada no GitHub, o app, ao abrir, detecta e mostra dialog "Nova versão disponível [Ver release] [Mais tarde]" — sem auto-instalar nada.

## Não-Objetivos

- Code-signing macOS (Apple Developer $99/ano) e Windows EV cert ($200-400/ano) — sem signing, Gatekeeper/SmartScreen mostram aviso "publisher desconhecido" mas a instalação funciona. Migrável quando houver volume/conta.
- Full auto-update silencioso (depende de signing). v1 fica em update-notify; migração é cirúrgica (troca handler do botão) quando signing chegar.
- Notarization macOS (depende de signing).
- Canais de distribuição extras: brew tap, scoop bucket, winget, AUR, repo apt próprio. v1 fica em GitHub Releases + caminho `cargo install --git https://github.com/...` pra power-user.
- Publicação em crates.io de `mustard-cli`/`mustard-rt` — fica via `--git` por ora.
- Update channels (stable + beta/canary) — só stable em v1.
- Site `mustardia.com` — feature separada (HTML/Vercel deploy).
- Escrever em `~/.claude/settings.json` global do user sem `MUSTARD_GLOBAL_PERMISSIONS=1` — comportamento opt-in preservado (memory `feedback_mustard_install_workflow`).
- Substituir o RTK upstream por fork — v1 bundla a release upstream pinned por versão.
- Migrar dados do `mustard-dashboard` antigo (não há usuários em prod — memory `feedback_no_migration_dev_phase`).

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: Workspace inteiro compila sem erros após rename `mustard-dashboard` → `mustard-app` — Command: `cargo check --workspace`
- [ ] AC-2: Todos os crates estão na versão `1.0.0` — Command: `node -e "const fs=require('fs');const toml=p=>fs.readFileSync(p,'utf8');const want='1.0.0';const files=['apps/cli/Cargo.toml','apps/rt/Cargo.toml','apps/app/src-tauri/Cargo.toml','packages/core/Cargo.toml'];const bad=files.filter(f=>!new RegExp('^version\\\\s*=\\\\s*\\\"'+want+'\\\"','m').test(toml(f)));if(bad.length){console.error('bad version in',bad);process.exit(1)}"`
- [ ] AC-3: `tauri.conf.json` declara `productName: \"Mustard\"` e `identifier: \"com.atiz.mustard\"` — Command: `node -e "const j=JSON.parse(require('fs').readFileSync('apps/app/src-tauri/tauri.conf.json','utf8'));if(j.productName!=='Mustard'||j.identifier!=='com.atiz.mustard'){console.error(JSON.stringify({productName:j.productName,identifier:j.identifier}));process.exit(1)}"`
- [ ] AC-4: Existe `.github/workflows/release.yml` com matriz dos 4 alvos (Windows, Linux, macOS Intel, macOS ARM) e step que empacota `rtk` no bundle — Command: `node -e "const s=require('fs').readFileSync('.github/workflows/release.yml','utf8');const need=['windows-latest','ubuntu-22.04','macos-latest','aarch64-apple-darwin','x86_64-apple-darwin','cargo install rtk'];const miss=need.filter(x=>!s.includes(x));if(miss.length){console.error('release.yml missing:',miss);process.exit(1)}"`
- [ ] AC-5: `.github/workflows/dashboard-release.yml` foi removido (substituído pelo release.yml unificado) — Command: `node -e "if(require('fs').existsSync('.github/workflows/dashboard-release.yml')){console.error('dashboard-release.yml should have been removed');process.exit(1)}"`
- [ ] AC-6: `.github/workflows/ci.yml` não referencia mais `packages/cli` — Command: `node -e "const s=require('fs').readFileSync('.github/workflows/ci.yml','utf8');if(s.includes('packages/cli')){console.error('ci.yml still references packages/cli');process.exit(1)}"`
- [ ] AC-7: Módulo Rust `path_check` no app expõe função que detecta presença de `mustard`, `mustard-rt`, `rtk`, `claude` no PATH — Command: `cargo test -p mustard-app path_check`
- [ ] AC-8: Tauri command `check_for_updates` existe e retorna objeto serializável com versão atual + última versão do GitHub — Command: `node -e "const s=require('fs').readFileSync('apps/app/src-tauri/src/update_check.rs','utf8');if(!/check_for_updates/.test(s)){console.error('check_for_updates missing');process.exit(1)}"`
- [ ] AC-9: Tauri command `list_out_of_sync_projects` retorna projetos do registry com versão < app — Command: `cargo test -p mustard-app project_sync`
- [ ] AC-10: Componente `AddProjectDialog.tsx` chama `detect_project_mustard` antes de exibir UI (fluxo adaptativo Q5) — Command: `node -e "const s=require('fs').readFileSync('apps/app/src/components/projects/AddProjectDialog.tsx','utf8');if(!/detect_project_mustard/.test(s)){console.error('adaptive detect missing');process.exit(1)}"`
- [ ] AC-11: O frontend tem banner persistente quando `rtk` não detectado no PATH — Command: `node -e "const fs=require('fs');const f='apps/app/src/components/banners/PrereqBanner.tsx';if(!fs.existsSync(f)){console.error('PrereqBanner.tsx missing');process.exit(1)};const s=fs.readFileSync(f,'utf8');if(!/rtk/i.test(s)){console.error('PrereqBanner does not reference rtk');process.exit(1)}"`
- [ ] AC-12: Welcome screen existe e é mostrado quando registry está vazio — Command: `node -e "const fs=require('fs');const f='apps/app/src/components/welcome/WelcomeScreen.tsx';if(!fs.existsSync(f)){console.error('WelcomeScreen.tsx missing');process.exit(1)}"`
- [ ] AC-13: Build do app Tauri completa em modo release (debug bundle) — Command: `cargo build --manifest-path apps/app/src-tauri/Cargo.toml`

## Plano

## Informações da Entidade

`AppVersion` (novo, in-memory): versão lida de `tauri.conf.json` em runtime via Tauri API; comparada com `GitHubRelease.tag_name` pra decidir notify.

`PrereqStatus` (novo, in-memory):

```rust
pub struct PrereqStatus {
    pub mustard: BinaryStatus,     // BinaryStatus = Present { version: String } | Missing
    pub mustard_rt: BinaryStatus,
    pub rtk: BinaryStatus,
    pub claude_code: BinaryStatus,
}
```

`OutOfSyncProject` (novo, in-memory):

```rust
pub struct OutOfSyncProject {
    pub path: String,
    pub installed_version: Option<String>, // from .claude/mustard.json
    pub app_version: String,
}
```

Sem novo schema em SQLite. Registry de projetos (b6) e `mustard.json:version` em cada projeto já existem — feature consome.

## Arquivos

Distribuídos por wave (cada `wave-N-{role}/spec.md` traz a lista exata). Resumo cross-wave:

```
# Wave 1 — Foundation rename + version unify
apps/dashboard/                                       → renomeado para apps/app/
apps/cli/Cargo.toml                                   — version "3.1.36" → "1.0.0"
apps/rt/Cargo.toml                                    — version → "1.0.0"
apps/app/Cargo.toml + apps/app/src-tauri/Cargo.toml   — name "mustard-dashboard" → "mustard-app", lib name, version
apps/app/package.json                                 — name "mustard-dashboard" → "mustard-app"
apps/app/src-tauri/tauri.conf.json                    — productName, identifier
packages/core/Cargo.toml                              — version → "1.0.0"
pnpm-workspace.yaml                                   — paths apps/dashboard → apps/app
package.json (root)                                   — scripts dashboard:* → app:*
Cargo.toml (root workspace)                           — members
README.md + CLAUDE.md(s)                              — referências apps/dashboard

# Wave 2 — CI workflow & build pipeline
.github/workflows/release.yml                         — NOVO (substitui dashboard-release.yml)
.github/workflows/dashboard-release.yml               — REMOVIDO
.github/workflows/ci.yml                              — cleanup referências stale a packages/cli
apps/app/src-tauri/tauri.conf.json                    — bundle.windows.nsis, bundle.linux.deb section

# Wave 3 — PATH integration (Rust + scripts)
apps/app/src-tauri/installer/windows-path.nsh         — NOVO (NSIS hook escreve HKCU PATH)
apps/app/src-tauri/installer/linux-postinst.sh        — NOVO (postinst symlink)
apps/app/src-tauri/installer/linux-postrm.sh          — NOVO (cleanup symlink)
apps/app/src-tauri/src/path_check.rs                  — NOVO (which mustard/mustard-rt/rtk/claude)
apps/app/src-tauri/src/cli_tools_installer.rs         — NOVO (macOS Authorization Services)
apps/app/src-tauri/src/lib.rs                         — registra módulos

# Wave 4 — Update-notify + project sync (Rust backend)
apps/app/src-tauri/src/update_check.rs                — NOVO (GitHub Releases API)
apps/app/src-tauri/src/project_sync.rs                — NOVO (varre registry, lista out-of-sync)
apps/app/src-tauri/src/lib.rs                         — registra commands

# Wave 5 — Frontend UX (React)
apps/app/src/components/projects/AddProjectDialog.tsx       — REFATOR adaptativo
apps/app/src/components/projects/InstallLog.tsx             — NOVO (<details> com log honesto)
apps/app/src/components/welcome/WelcomeScreen.tsx           — NOVO
apps/app/src/components/banners/PrereqBanner.tsx            — NOVO (RTK/Claude Code missing)
apps/app/src/components/banners/ProjectSyncBanner.tsx       — NOVO (projetos out-of-sync)
apps/app/src/components/banners/UpdateAvailableBanner.tsx   — NOVO (update-notify)
apps/app/src/hooks/usePrereqStatus.ts                       — NOVO
apps/app/src/hooks/useUpdateCheck.ts                        — NOVO
apps/app/src/hooks/useOutOfSyncProjects.ts                  — NOVO
apps/app/src/lib/dashboard.ts                               — novos invoke wrappers
apps/app/src/App.tsx                                        — mount banners + welcome
apps/app/src/pages/Preferences.tsx                          — botão "Check for updates"
```

## Tarefas

Wave-by-wave; detalhes em cada `wave-N-{role}/spec.md`. Resumo de dependências:

```
wave-1 (foundation)  ──►  wave-2 (CI)
                     ──►  wave-3 (PATH integration)
                     ──►  wave-4 (update-notify + project sync)
                                          ▼
                                    wave-5 (frontend UX)
                                          ▼
                                       review → qa
```

Wave 1 bloqueia todas as demais (rename precisa estar feito pra paths funcionarem). Wave 2/3/4 são **independentes entre si** após Wave 1 (Wave 2 mexe em CI, Wave 3 em installer/scripts/Rust, Wave 4 em Rust backend novo — zero overlap). Wave 5 depende de Wave 3 e Wave 4 (consome commands Tauri criados nelas). Execução: 1 → (2 ∥ 3 ∥ 4) → 5 → review → qa.

## Tabela de Waves

| Wave | Spec                            | Role     | Resumo                                                        |
|------|---------------------------------|----------|---------------------------------------------------------------|
| 1    | [[wave-1-general]]              | general  | Rename dashboard → app, versão 1.0.0 em todos crates          |
| 2    | [[wave-2-general]]              | general  | release.yml multi-SO matricial + cleanup ci.yml               |
| 3    | [[wave-3-general]]              | general  | PATH integration: NSIS hook, postinst, macOS first-run        |
| 4    | [[wave-4-general]]              | general  | update_check.rs (GitHub API) + project_sync.rs                |
| 5    | [[wave-5-ui]]                   | ui       | AddProjectDialog adaptativo + banners + welcome + InstallLog  |

## Dependências

- **Tauri 2** (já no projeto) — `@tauri-apps/api`, `tauri-plugin-dialog`, `tauri-plugin-store`, `tauri-plugin-updater` (instalado mas NÃO ativado nesta v1).
- **NSIS** (built-in do Tauri Windows bundler) — pra hook script de PATH.
- **Authorization Services** (macOS, framework do sistema) — Rust usa via `objc2-security` ou via shell-out a `osascript -e "do shell script ... with administrator privileges"`. Decisão final na Wave 3 (preferência: `osascript` por simplicidade — uma string de shell, sem FFI nova).
- **GitHub REST API** — `GET /repos/{owner}/{repo}/releases/latest`. Rate limit unauth 60/h é mais que suficiente; cache 1h client-side.
- **`ureq`** (já em `mustard-cli`) — reaproveitado pra GET do GitHub. Sem nova dep no app.
- **`rtk`** (upstream, [sigoden/rtk](https://github.com/sigoden/rtk)) — `cargo install rtk` no workflow CI; pin de versão via `--version X.Y.Z`.
- **`postinst` / `postrm`** (Debian/Fedora) — Tauri 2 bundler suporta via `bundle.linux.deb.files` + scripts em `installer/`.
- Sem nova dep npm. Sem nova dep Cargo no app.

## Limites

- `apps/cli/Cargo.toml`, `apps/rt/Cargo.toml`, `packages/core/Cargo.toml` (somente bump de versão)
- `apps/app/` (todo o conteúdo — rename de `apps/dashboard/`)
- `apps/app/src-tauri/installer/` (novo diretório)
- `apps/app/src-tauri/src/path_check.rs`, `cli_tools_installer.rs`, `update_check.rs`, `project_sync.rs` (novos)
- `apps/app/src-tauri/src/lib.rs` (mod registrations + invoke_handler)
- `apps/app/src-tauri/tauri.conf.json` (productName, identifier, bundle.windows, bundle.linux)
- `apps/app/src/components/projects/`, `welcome/`, `banners/` (novos componentes)
- `apps/app/src/hooks/usePrereqStatus.ts`, `useUpdateCheck.ts`, `useOutOfSyncProjects.ts` (novos)
- `apps/app/src/lib/dashboard.ts`, `App.tsx`, `pages/Preferences.tsx`
- `.github/workflows/release.yml` (novo), `dashboard-release.yml` (deletar), `ci.yml` (cleanup)
- `pnpm-workspace.yaml`, `package.json` (root), `Cargo.toml` (root)
- `README.md`, `CLAUDE.md` files com referência a `apps/dashboard`

Out-of-boundary explícito: code-signing (Não-Objetivo), full auto-update silencioso (Não-Objetivo), brew/scoop/winget (Não-Objetivo), publicação crates.io (Não-Objetivo), site mustardia.com (Não-Objetivo), migração de dados de instalações antigas (memory `feedback_no_migration_dev_phase`), reescrita do RTK upstream, mudança de schema em SQLite.

## Cobertura de Críticas

Cada item levantado pelo usuário e seu destino:

| Crítica do usuário | Bucket | Onde |
|---|---|---|
| App deve se chamar "Mustard" (não "Mustard Dashboard") | Coberto | Wave 1 (productName + identifier) |
| Instalar `mustard` (CLI) e `mustard-rt` (runtime) na máquina | Coberto | Wave 2 (bundle) + Wave 3 (PATH) |
| RTK também precisa estar no PATH | Coberto | Wave 2 (cargo install rtk no CI) |
| Selecionar pasta no app → instalar Mustard dentro dela | Coberto | Wave 5 (AddProjectDialog adaptativo) |
| Log honesto pós-install | Coberto | Wave 5 (InstallLog.tsx) |
| Auto-update | Coberto | Wave 4 (update_check.rs) + Wave 5 (UpdateAvailableBanner) |
| Cross-platform (Windows, macOS, Linux) | Coberto | Wave 2 (matriz CI) + Wave 3 (PATH per-SO) |
| Separar cli e app (`mustard-cli`, `mustard-app`) | Coberto | Wave 1 (rename) |
| Manter "Mustard" como nome, `mustardia.com` só site | Coberto | Wave 1 (productName) + Não-Objetivos (site) |
| Bundle único (app + cli + rt + rtk versionados juntos) | Coberto | Wave 2 (CI gera um bundle por SO) + Wave 1 (versão unificada) |
| Update-notify (sem signing) | Coberto | Wave 4 (sem manifesto Tauri) + Wave 5 (banner com [Ver release]) |
| Reset todos crates pra `1.0.0` | Coberto | Wave 1 |
| PATH per-SO nativo | Coberto | Wave 3 (NSIS + Authorization Services + postinst) |
| Update do `.claude/` em projetos out-of-sync | Coberto | Wave 4 (project_sync.rs) + Wave 5 (ProjectSyncBanner) |
| Fluxo adaptativo (detecta `.claude/` antes de perguntar) | Coberto | Wave 5 (AddProjectDialog) |
| Bundle RTK no instalador | Coberto | Wave 2 (cargo install rtk + cp no workflow) |
| Welcome screen na primeira execução | Coberto | Wave 5 (WelcomeScreen.tsx) |
| Banners persistentes (RTK ausente, Claude Code ausente) | Coberto | Wave 5 (PrereqBanner.tsx) |
| Code-signing | Não-Goal | Não-Objetivos |
| Full auto-update silencioso | Não-Goal | Não-Objetivos |
| Brew/Scoop/Winget/AUR | Não-Goal | Não-Objetivos |
| Crates.io publish | Não-Goal | Não-Objetivos |
| Site mustardia.com | Não-Goal | Não-Objetivos |
| Update channels (beta/canary) | Não-Goal | Não-Objetivos |

Todos os pontos do pedido mapeados. Zero items órfãos.
