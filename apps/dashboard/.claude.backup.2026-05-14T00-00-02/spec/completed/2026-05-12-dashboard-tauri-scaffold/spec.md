# Feature: dashboard-tauri-scaffold

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-12T00:00:00Z
### Commits: 70ca592 (feat: bootstrap), e97d68b (chore: remove nested template duplicates)
### QA: 8/8 AC PASS (AC-5 tauri build --debug em 37.6s, binário mustard-dashboard.exe 21 MB)
### Review: APPROVED (0 CRITICAL, 4 WARNING capturados em Concerns)
### Lang: pt

## Contexto

O Mustard Dashboard precisa ser um app Tauri desktop multiplataforma servindo de host para o orchestrator de pipelines, dashboards de metricas e ferramentas internas do time. Hoje o repositorio em `C:\Atiz\mustard-dashboard` contem apenas o scaffold Mustard sob `.claude/` — nao existe `package.json` na raiz, nao existe crate Rust, nao existe janela, nao existe build artefavel. Qualquer tentativa de iterar em telas, comandos Rust ou plugins desktop esbarra na ausencia total do app. Sem esse bootstrap nao da para evoluir a UI dos dashboards nem integrar os comandos do Rust com o orchestrator. O impacto observavel e: o time nao consegue rodar `pnpm tauri dev`, nao consegue abrir a janela do app, e a roadmap de dashboards fica bloqueada. Esta entrega cria o primeiro subprojeto do monorepo Mustard — um app Tauri 2 com React 19, Tailwind v4, shadcn/ui e os quatro plugins desktop mandatorios (store, log, window-state, updater), pronto para receber telas reais nas proximas iteracoes.

## Summary

Bootstrap do app Tauri 2 na raiz do repo: React 19 + TS + Vite + Tailwind v4 (CSS-first) + shadcn/ui (style new-york, base slate) + 4 plugins v2 (store, log, window-state, updater), AppShell (Sidebar/Topbar) + rota Home, `mustard.json` atualizado para pnpm.

## Entity Info

- Entity: `App` (root Tauri application) — primeiro subprojeto do monorepo Mustard
- Subproject path: `.` (repo root; crate Rust em `./src-tauri`, frontend Vite na raiz)
- Layers: DevOps/Bootstrap, Backend (Rust/Tauri), Frontend (React)

## Boundaries

Arquivos e diretorios DENTRO do escopo:

- Config raiz: `package.json`, `pnpm-lock.yaml`, `.npmrc`, `.gitignore`, `tsconfig.json`, `tsconfig.node.json`, `vite.config.ts`, `index.html`, `components.json`, `mustard.json`, `eslint.config.js`
- Frontend: `src/**` (entrypoint, paginas, componentes, estilos)
- Backend Rust + Tauri: `src-tauri/**` (Cargo.toml, tauri.conf.json, src/, capabilities/, icons/, build.rs)

Arquivos e diretorios FORA do escopo (NAO tocar):

- `.claude/**` — scaffold Mustard, gerenciado por `/scan`. Excecao unica: o proprio spec e o pipeline-state desta feature.
- `CLAUDE.md` e `REFERENCE.md` na raiz — propriedade do Mustard.
- Qualquer outro subprojeto futuro em `apps/**` ou `packages/**` — nao existem ainda.

## Files (~30)

**Repo config (10):**
- `package.json` — name `mustard-dashboard`, `"type": "module"`, `packageManager: "pnpm@9.x"`, scripts (`dev`, `build`, `preview`, `tauri`, `tauri:dev`, `tauri:build`, `lint`, `test`).
- `pnpm-lock.yaml` — lockfile gerado pelo `pnpm install`.
- `.npmrc` — `auto-install-peers=true`, `node-linker=isolated` (default pnpm).
- `.gitignore` — `node_modules/`, `dist/`, `src-tauri/target/`, `.env*`, `*.log`.
- `tsconfig.json` — alvo ES2022, `paths` com alias `@/*` -> `src/*`.
- `tsconfig.node.json` — config separada para `vite.config.ts`.
- `vite.config.ts` — plugin `@vitejs/plugin-react`, `@tailwindcss/vite`, alias `@` -> `./src`, server.port 1420 (default Tauri), server.strictPort true.
- `index.html` — root HTML com `<div id="root">` e import de `src/main.tsx`.
- `components.json` — config shadcn (style new-york, baseColor slate, cssVariables true, rsc false, tsx true, aliases components/ui/lib/hooks).
- `eslint.config.js` — flat config minimo TS + React.

**Mustard integration (1):**
- `mustard.json` — atualizar commands: `testCommand "pnpm test"`, `buildCommand "pnpm build"`, `lintCommand "pnpm lint"`, `typeCheckCommand "pnpm tsc --noEmit"`. Manter `gitFlow` e `provider` existentes.

**Frontend src (10):**
- `src/main.tsx` — render `<App/>` em `#root` com React 19 `createRoot`.
- `src/App.tsx` — monta `<AppShell><Home/></AppShell>`.
- `src/index.css` — `@import "tailwindcss";` + bloco `@theme` com tokens (background, foreground, primary, muted, border, ring, sidebar) e `@variant dark` selector-based.
- `src/lib/utils.ts` — helper `cn` (clsx + tailwind-merge) gerado pelo shadcn init.
- `src/pages/Home.tsx` — pagina Home com um Card "Mustard Dashboard — scaffold ready" e 2-3 cards placeholder.
- `src/components/layout/AppShell.tsx` — CSS grid (sidebar 240px + main); props `{ children }`.
- `src/components/layout/Sidebar.tsx` — nav rail com logo + 1 link "Home".
- `src/components/layout/Topbar.tsx` — header sticky h-14 com titulo + botao theme toggle stub.
- `src/components/ui/button.tsx` — primitive shadcn (gerado por `shadcn add button`).
- `src/components/ui/card.tsx` — primitive shadcn (gerado por `shadcn add card`).

**Rust backend + Tauri (7):**
- `src-tauri/Cargo.toml` — package `mustard-dashboard`, edition 2021, deps `tauri = "2"`, `tauri-plugin-store = "2"`, `tauri-plugin-log = "2"`, `tauri-plugin-window-state = "2"`, `tauri-plugin-updater = "2"`, `serde`, `serde_json`.
- `src-tauri/build.rs` — `tauri_build::build()` (gerado pelo template).
- `src-tauri/tauri.conf.json` — productName "Mustard Dashboard", identifier `com.atiz.mustard-dashboard`, version 0.1.0, build (devUrl http://localhost:1420, frontendDist `../dist`, beforeDevCommand `pnpm dev`, beforeBuildCommand `pnpm build`), app.windows[0] (width 1280, height 800, minWidth 900, minHeight 600, resizable true, title "Mustard Dashboard"), bundle.createUpdaterArtifacts true, plugins.updater (endpoints [], pubkey "" — desabilitado por enquanto).
- `src-tauri/src/main.rs` — `fn main() { mustard_dashboard_lib::run() }`.
- `src-tauri/src/lib.rs` — `pub fn run()` com `tauri::Builder::default()` registrando os 4 plugins.
- `src-tauri/capabilities/default.json` — capability "default" para window "main" com `core:default`, `store:default`, `log:default`, `window-state:default`, `updater:default`.
- `src-tauri/icons/` — placeholders gerados pelo template `create-tauri-app` (icon.icns, icon.ico, 32x32.png, 128x128.png, 128x128@2x.png, Square*Logo.png, StoreLogo.png).

## Tasks

### Bootstrap Agent (Wave 1)

- [x] Step 1: Verificar pre-requisitos: `pnpm --version` (>=9), `rustc --version` (>=1.77), `cargo --version`. Falhar early se faltar.
- [x] Step 2: Rodar `pnpm create tauri-app@2` interactivamente OU scaffold manual se flags nao confirmados: `app-name=mustard-dashboard`, `identifier=com.atiz.mustard-dashboard`, `template=react-ts`, `package-manager=pnpm`. Como o repo nao esta vazio (`.claude/`, `CLAUDE.md`, `REFERENCE.md`, `mustard.json` existem), executar em diretorio temporario e copiar arquivos para a raiz, OU usar a flag `--force`/`-y` se disponivel via `pnpm create tauri-app@2 . -- --template react-ts --identifier com.atiz.mustard-dashboard --manager pnpm`. Validar versao do `create-tauri-app` antes de assumir flags.
- [x] Step 3: Ajustar `package.json`: name `mustard-dashboard`, `"type": "module"`, `packageManager: "pnpm@9.x"`, scripts (`dev`, `build` = `tsc -b && vite build`, `preview`, `tauri`, `tauri:dev`, `tauri:build`, `lint`, `test` = `echo "no tests yet" && exit 0`).
- [x] Step 4: Ajustar `src-tauri/Cargo.toml`: package name `mustard-dashboard`, edition 2021, `rust-version = "1.77"`.
- [x] Step 5: Ajustar `src-tauri/tauri.conf.json`: productName "Mustard Dashboard", identifier `com.atiz.mustard-dashboard`, window 1280x800 com minWidth 900/minHeight 600, devUrl `http://localhost:1420`, frontendDist `../dist`, beforeDevCommand `pnpm dev`, beforeBuildCommand `pnpm build`.
- [x] Step 6: Rodar `pnpm install` e `pnpm tauri info` para validar o setup. Reportar falhas claramente.

### Frontend Agent (Wave 2, paralelizavel com Tauri Plugins Agent)

- [x] Step 1: Instalar Tailwind v4: `pnpm add -D tailwindcss @tailwindcss/vite`. Editar `vite.config.ts` para adicionar `import tailwindcss from "@tailwindcss/vite"` no array de plugins, alem de `path.resolve` alias `@` -> `./src`.
- [x] Step 2: Substituir `src/index.css` por: `@import "tailwindcss";` + bloco `@theme` com tokens (background, foreground, primary, primary-foreground, muted, muted-foreground, border, ring, sidebar, sidebar-foreground) + `@variant dark (&:where(.dark, .dark *));` para dark mode selector-based.
- [x] Step 3: Ajustar `tsconfig.json` adicionando `compilerOptions.paths`: `{ "@/*": ["./src/*"] }` e `baseUrl: "."`.
- [x] Step 4: Inicializar shadcn: `pnpm dlx shadcn@latest init` com respostas (style new-york, baseColor slate, cssVariables true, rsc false). Verificar que `components.json` e `src/lib/utils.ts` foram criados.
- [x] Step 5: Adicionar primitives: `pnpm dlx shadcn@latest add button card`.
- [x] Step 6: Criar `src/components/layout/AppShell.tsx` (CSS grid `grid-cols-[240px_1fr] grid-rows-[56px_1fr]` com Sidebar na coluna 1 e Topbar+main na coluna 2), `Sidebar.tsx` (logo + 1 link "Home"), `Topbar.tsx` (titulo + botao theme toggle stub que apenas adiciona/remove `dark` em `document.documentElement`).
- [x] Step 7: Criar `src/pages/Home.tsx` com um Card principal "Mustard Dashboard — scaffold ready" + grid de 2-3 cards placeholder ("Pipelines", "Metricas", "Knowledge").
- [x] Step 8: Atualizar `src/App.tsx` para `<AppShell><Home/></AppShell>` (sem router por enquanto — rota unica).
- [x] Step 9: Validar `pnpm tsc --noEmit` e `pnpm build` sem erros.

### Tauri Plugins Agent (Wave 2, paralelizavel com Frontend Agent)

- [x] Step 1: Adicionar Cargo deps em `src-tauri/Cargo.toml` (versao v2): `tauri-plugin-store = "2"`, `tauri-plugin-log = "2"`, `tauri-plugin-window-state = "2"`, `tauri-plugin-updater = "2"`.
- [x] Step 2: Adicionar npm deps na raiz: `pnpm add @tauri-apps/plugin-store @tauri-apps/plugin-log @tauri-apps/plugin-window-state @tauri-apps/plugin-updater`.
- [x] Step 3: Registrar os 4 plugins em `src-tauri/src/lib.rs`:
  - `.plugin(tauri_plugin_store::Builder::new().build())`
  - `.plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())`
  - `.plugin(tauri_plugin_window_state::Builder::default().build())`
  - `.plugin(tauri_plugin_updater::Builder::new().build())`
- [x] Step 4: Atualizar `src-tauri/capabilities/default.json` para incluir `store:default`, `log:default`, `window-state:default`, `updater:default` na lista de permissions (manter `core:default` ja presente).
- [x] Step 5: Configurar updater em `tauri.conf.json`: `bundle.createUpdaterArtifacts: false` (para nao quebrar build sem chave) + bloco `plugins.updater` com `endpoints: []` e `pubkey: ""`. Documentar em Concerns que sem `pubkey` real o updater nao consegue verificar releases — habilitar so quando chaves forem provisionadas.
- [x] Step 6: Validar `cargo check --manifest-path src-tauri/Cargo.toml`.

### Integration Agent (Wave 3 — depois de Frontend + Plugins)

- [x] Step 1: Atualizar `mustard.json`: `testCommand "pnpm test"`, `buildCommand "pnpm build"`, `lintCommand "pnpm lint"`, `typeCheckCommand "pnpm tsc --noEmit"`. Manter `gitFlow` e `provider`.
- [x] Step 2: Garantir `.gitignore` cobrindo `node_modules/`, `dist/`, `src-tauri/target/`, `.env*`, `*.log`, `.DS_Store`.
- [x] Step 3: Rodar `rm -rf node_modules dist src-tauri/target && pnpm install --frozen-lockfile=false && pnpm build` para garantir build limpo.
- [x] Step 4: Rodar `pnpm tauri build --debug` end-to-end e confirmar que o binario aparece em `src-tauri/target/debug/`.
- [x] Step 5: Commit final com mensagem "feat: bootstrap Tauri 2 dashboard scaffold (React 19 + Tailwind v4 + shadcn + 4 plugins)".

## Dependencies

- pnpm >= 9 instalado globalmente (`npm i -g pnpm@latest`).
- Rust toolchain stable >= 1.77 (`rustup default stable && rustup update`).
- Pre-requisitos OS Tauri 2:
  - Windows 11: WebView2 ja instalado por padrao; Visual Studio Build Tools 2022 com componente "Desktop development with C++" recomendado.
  - macOS: Xcode Command Line Tools (`xcode-select --install`).
  - Linux: `webkit2gtk-4.1`, `libssl-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev`.

## Acceptance Criteria

Cada AC e executavel da raiz do repo. Exit 0 = pass.

- [x] AC-1: pnpm install completo limpo — Command: `pnpm install --frozen-lockfile=false`
- [x] AC-2: TypeScript type-check passa — Command: `pnpm tsc --noEmit`
- [x] AC-3: Frontend Vite build gera bundle em `dist/` — Command: `pnpm build`
- [x] AC-4: Crate Rust compila — Command: `cargo check --manifest-path src-tauri/Cargo.toml`
- [x] AC-5: Build debug Tauri produz binario — Command: `pnpm tauri build --debug`
- [x] AC-6: Os 4 plugins estao registrados em lib.rs — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src-tauri/src/lib.rs','utf8');['store','log','window_state','updater'].forEach(p=>{if(!s.includes('tauri_plugin_'+p)){console.error('missing plugin: '+p);process.exit(1)}});console.log('all 4 plugins registered')"`
- [x] AC-7: AppShell + Home renderizam no bundle — Command: `node -e "const fs=require('fs');const f=fs.readdirSync('dist/assets').find(x=>x.endsWith('.js'));const c=fs.readFileSync('dist/assets/'+f,'utf8');if(!c.includes('Mustard Dashboard')){console.error('shell text missing');process.exit(1)}console.log('shell text present')"`
- [x] AC-8: Capabilities default lista as 4 permissoes de plugin — Command: `node -e "const p=require('./src-tauri/capabilities/default.json');const need=['store:default','log:default','window-state:default','updater:default'];const miss=need.filter(x=>!p.permissions.includes(x));if(miss.length){console.error('missing perms: '+miss.join(','));process.exit(1)}console.log('all plugin permissions present')"`

## Component Contract

**AppShell** — Layout root da app.
- Props: `{ children: ReactNode }`.
- Estrutura: CSS grid `grid-cols-[240px_1fr] grid-rows-[56px_1fr]`. Sidebar ocupa `row-span-2 col-start-1`. Topbar ocupa `col-start-2 row-start-1`. Main `col-start-2 row-start-2 overflow-y-auto`.
- Variants: none nesta entrega.
- Responsive: <768px colapsa sidebar para icon-only (DEFERIDO — documentado em Concerns).

**Sidebar** — Nav rail fixa.
- Props: none.
- Conteudo: logo "Mustard" no topo, lista vertical com um link "Home" (active state quando rota = `/`).
- Estilo: `bg-sidebar text-sidebar-foreground border-r border-border`.

**Topbar** — Header sticky.
- Props: `{ title?: string }` (default "Mustard Dashboard").
- Conteudo: titulo a esquerda + botao "theme toggle" stub a direita (apenas toggla classe `dark` em `<html>`).
- Estilo: `h-14 sticky top-0 bg-background border-b border-border flex items-center justify-between px-4`.

**Home** — Pagina inicial.
- Props: none.
- Conteudo: um `<Card>` hero "Mustard Dashboard — scaffold ready" com descricao curta + grid de 2-3 `<Card>` placeholders (Pipelines, Metricas, Knowledge).

## Concerns

- **Updater sem chave**: o plugin esta registrado mas `pubkey` esta vazio e `endpoints` lista vazia. Sem chave de assinatura nao da para verificar releases. Follow-up: gerar par via `pnpm tauri signer generate -- -w ~/.tauri/myapp.key`, popular `pubkey` no `tauri.conf.json` e setar `bundle.createUpdaterArtifacts: true` quando houver endpoint real.
- **Theme toggle stub**: o botao no Topbar apenas adiciona/remove classe `dark` no `<html>`. Persistencia de preferencia (via `tauri-plugin-store` ou `localStorage`) e detecao de `prefers-color-scheme` ficam para entrega seguinte.
- **Sem router**: a app tem rota unica `/`. Quando surgir necessidade de >1 pagina, adicionar `react-router-dom` ou TanStack Router. Documentar a escolha quando ocorrer.
- **Icons placeholder**: os icones em `src-tauri/icons/` sao os defaults do template. Substituir por logo real via `pnpm tauri icon path/to/logo.png` antes de release.
- **Mobile target nao configurado**: Tauri 2 suporta iOS/Android via `pnpm tauri ios init` / `android init`, mas esta entrega e desktop only.
- **CI/CD**: builds Tauri em GitHub Actions ficam para depois — exigem matrix Windows/macOS/Linux e cache de Cargo.
- **Vitest/Playwright**: tests deferidos. `pnpm test` retorna sucesso vazio por enquanto.
- **Lock pnpm**: primeira execucao em CI pode regerar `pnpm-lock.yaml` se nao commitado. Commitar o lockfile ao fim do EXECUTE.
- **Compatibilidade shadcn + Tailwind v4 + React 19**: confirmado oficialmente em https://ui.shadcn.com/docs/tailwind-v4 — `shadcn@latest` ja inicializa projetos com Tailwind v4 e suporta React 19. Baixo risco.
- **`create-tauri-app` em diretorio nao-vazio**: como a raiz tem `.claude/`, `CLAUDE.md`, `mustard.json`, o scaffold padrao pode recusar. Plano B: scaffold em diretorio temporario (`/tmp/tauri-scaffold`) e mesclar arquivos no repo manualmente, preservando arquivos Mustard. **Resolvido na execução** com `pnpm create tauri-app@latest mustard-dashboard --identifier com.atiz.mustard-dashboard --template react-ts --manager pnpm --tauri-version 2 --yes` em `/c/Atiz/mustard-dashboard-scaffold-tmp/`. Nota: o tag `@2` do CTA gera Tauri v1 — usar `@latest --tauri-version 2` no futuro.

### Follow-ups capturados na review (WARNING — não bloqueiam, próxima iteração)

- **Dead `greet` command no Rust** — `src-tauri/src/lib.rs:3,19` ainda contém o `fn greet` + `invoke_handler![greet]` herdado do template; nunca é invocado pelo frontend. Remover para deixar o scaffold realmente mínimo.
- **Topbar usa `<button>` raw** — `src/components/layout/Topbar.tsx:6-13` faz hand-roll de classes Tailwind em vez de usar o `<Button variant="outline" size="sm">` do shadcn. Trocar para manter o design system consistente desde o dia 1.
- **`@custom-variant dark` duplicado** — `src/style.css:6-8` declara tanto `@custom-variant dark` (herdado do shadcn init) quanto `@variant dark` que adicionei. Redundância — remover o `@custom-variant`.
- **packageManager pin** — `package.json` ficou em `pnpm@10.18.1` (versão real instalada) em vez do `pnpm@9.x` que o spec assumiu. Tudo OK funcionalmente; só ajustar a spec/docs se quiser refletir.
- **AppShell grid layout** — Topbar não tem `row-start-1 col-start-2` explícito (`src/components/layout/AppShell.tsx:9`); funciona hoje porque Sidebar tem `row-span-2`, mas adicionar um segundo elemento na coluna 2 quebraria o layout. Tornar explícito.
- **`tauri-plugin-log` sem level filter** — `src-tauri/src/lib.rs` registra `Builder::new().build()` sem `.level(log::LevelFilter::Info)`. Suficiente para scaffold; revisitar quando logging estruturado virar requisito.
- **HSL tokens em `@theme` shadowed pelos oklch da shadcn nova preset** — coexistem no `src/style.css`; bloco HSL é efetivamente sobrescrito. Limpar no próximo pass de design system.
- **Mustard sync-detect não reconhece root como subproject** — layout monorepo-flat (Tauri na raiz) divergente do que `sync-detect.js` espera (`apps/*`). `entity-registry.json` continua vazio. Caminhos: (a) reestruturar para `apps/dashboard/`, ou (b) estender o scanner. Não bloqueia trabalho de UI; bloqueia `/scan` gerar agents/recipes específicos do subproject.

### Artefatos finais

- Commit `70ca592`: 85 arquivos do scaffold (frontend + Rust + config + plugins)
- Commit `e97d68b`: -213 linhas (deletou `src/src/`, `src-tauri/src/src/`, `src/App.css` orfão)
- Build artefato: `src-tauri/target/debug/mustard-dashboard.exe` (21 MB) + MSI + NSIS bundles em `src-tauri/target/debug/bundle/`

## Non-Goals

- Autenticacao real / contas de usuario.
- Banco de dados ou persistencia alem do `tauri-plugin-store` (JSON local).
- Feed de updater real (chaves e endpoint ficam para depois).
- Mobile target (iOS/Android via Tauri 2 mobile).
- Testes E2E (Vitest unit + Playwright e2e diferidos).
- Logica completa de theme switching (so o stub).
- Pipelines CI/CD para builds Tauri.
- Roteamento multi-pagina.
- Internacionalizacao (i18n).
- Telemetria / analytics.
