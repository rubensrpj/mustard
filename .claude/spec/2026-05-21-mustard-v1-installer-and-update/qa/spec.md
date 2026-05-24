# QA — Mustard v1 installer

### Role: qa

## Critérios de Aceitação

QA agent roda os 13 AC do parent wave-plan diretamente. Esta cópia mantém o mesmo formato pro caso de scan local.

- [ ] AC-1: Workspace compila — Command: `cargo check --workspace`
- [ ] AC-2: Todos crates em 1.0.0 — Command: `node -e "const fs=require('fs');const want='1.0.0';const files=['apps/cli/Cargo.toml','apps/rt/Cargo.toml','apps/app/src-tauri/Cargo.toml','packages/core/Cargo.toml'];const bad=files.filter(f=>!new RegExp('^version\\\\s*=\\\\s*\\\"'+want+'\\\"','m').test(fs.readFileSync(f,'utf8')));if(bad.length){console.error(bad);process.exit(1)}"`
- [ ] AC-3: productName + identifier corretos — Command: `node -e "const j=JSON.parse(require('fs').readFileSync('apps/app/src-tauri/tauri.conf.json','utf8'));if(j.productName!=='Mustard'||j.identifier!=='com.atiz.mustard'){process.exit(1)}"`
- [ ] AC-4: release.yml com matriz multi-SO — Command: `node -e "const s=require('fs').readFileSync('.github/workflows/release.yml','utf8');const need=['windows-latest','ubuntu-22.04','macos-latest','aarch64-apple-darwin','x86_64-apple-darwin','cargo install rtk'];const miss=need.filter(x=>!s.includes(x));if(miss.length){process.exit(1)}"`
- [ ] AC-5: dashboard-release.yml removido — Command: `node -e "if(require('fs').existsSync('.github/workflows/dashboard-release.yml')){process.exit(1)}"`
- [ ] AC-6: ci.yml sem packages/cli — Command: `node -e "const s=require('fs').readFileSync('.github/workflows/ci.yml','utf8');if(s.includes('packages/cli')){process.exit(1)}"`
- [ ] AC-7: path_check tests passam — Command: `cargo test -p mustard-app path_check`
- [ ] AC-8: update_check.rs declara check_for_updates — Command: `node -e "const s=require('fs').readFileSync('apps/app/src-tauri/src/update_check.rs','utf8');if(!/check_for_updates/.test(s)){process.exit(1)}"`
- [ ] AC-9: project_sync tests passam — Command: `cargo test -p mustard-app project_sync`
- [ ] AC-10: AddProjectDialog chama detect_project_mustard — Command: `node -e "const s=require('fs').readFileSync('apps/app/src/components/projects/AddProjectDialog.tsx','utf8');if(!/detect_project_mustard/.test(s)){process.exit(1)}"`
- [ ] AC-11: PrereqBanner referencia rtk — Command: `node -e "const fs=require('fs');const f='apps/app/src/components/banners/PrereqBanner.tsx';if(!fs.existsSync(f)){process.exit(1)};const s=fs.readFileSync(f,'utf8');if(!/rtk/i.test(s)){process.exit(1)}"`
- [ ] AC-12: WelcomeScreen existe — Command: `node -e "if(!require('fs').existsSync('apps/app/src/components/welcome/WelcomeScreen.tsx')){process.exit(1)}"`
- [ ] AC-13: App Tauri builda — Command: `cargo build --manifest-path apps/app/src-tauri/Cargo.toml`
