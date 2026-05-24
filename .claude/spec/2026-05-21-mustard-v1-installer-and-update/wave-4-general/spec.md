# Wave 4 — Update-notify (GitHub API) + project sync backend

### Wave: 4
### Role: general

## PRD

## Contexto

O modo de update escolhido foi update-notify (Q4) — o app não baixa nem instala atualização sozinho; ele só detecta que existe nova versão e abre a página de release no GitHub pra o user reinstalar manualmente. Isso elimina necessidade de signing (Apple Developer + Windows EV) na v1 e mantém migração futura cirúrgica. Esta wave implementa o detector em Rust: módulo `update_check.rs` faz `GET https://api.github.com/repos/<owner>/<repo>/releases/latest`, parseia `tag_name` (formato `mustard-v1.x.y`), compara com a versão atual lida do `tauri.conf.json` e retorna estrutura `UpdateStatus`. Cache de 1 hora client-side via SQLite ou arquivo JSON em data_dir (zero rede no startup quando dentro da janela). Em paralelo, `project_sync.rs` varre o registry de projetos (b6) — para cada projeto, lê `.claude/mustard.json:version` e compara com a versão do app, retornando lista de `OutOfSyncProject`. Tauri command `update_project(path)` roda `mustard_cli::update` (já existe in-process via lib) em um path específico.

## Métrica de sucesso

Tauri command `check_for_updates()` retorna `{ current: "1.0.0", latest: "1.0.1", needs_update: true, release_url: "https://github.com/.../releases/tag/mustard-v1.0.1" }` quando há nova release. `list_out_of_sync_projects()` retorna lista filtrada onde `installed_version < app_version`. `update_project(path)` roda update e retorna log honesto.

## Critérios de Aceitação

- [ ] AC-W4-1: Módulo `update_check.rs` existe e compila — Command: `cargo check -p mustard-app`
- [ ] AC-W4-2: Tauri command `check_for_updates` registrado — Command: `node -e "const s=require('fs').readFileSync('apps/app/src-tauri/src/lib.rs','utf8');if(!/check_for_updates/.test(s)){process.exit(1)}"`
- [ ] AC-W4-3: Tauri command `list_out_of_sync_projects` registrado — Command: `node -e "const s=require('fs').readFileSync('apps/app/src-tauri/src/lib.rs','utf8');if(!/list_out_of_sync_projects/.test(s)){process.exit(1)}"`
- [ ] AC-W4-4: Tauri command `update_project` registrado — Command: `node -e "const s=require('fs').readFileSync('apps/app/src-tauri/src/lib.rs','utf8');if(!/update_project/.test(s)){process.exit(1)}"`
- [ ] AC-W4-5: Testes unitários do `project_sync` passam — Command: `cargo test -p mustard-app project_sync`

## Plano

## Summary

Cria 2 módulos: `update_check.rs` faz GET GitHub Releases (via `ureq` reaproveitado de `mustard-cli`), parseia `tag_name`, compara versões com `semver` crate (já em workspace) e cacheia resultado por 1h em `app_data_dir/.update-cache.json`. `project_sync.rs` lê o registry de projetos (b6 — `projects::list_registered_projects` ou equivalente), para cada projeto lê `.claude/mustard.json:version` (função `read_mustard_json_version` já em `projects.rs`), retorna apenas os out-of-sync. Tauri command `update_project(path)` chama `mustard_cli::update(path, &UpdateOptions { force: false })` e retorna log estruturado.

## Checklist

### General Agent

- [ ] Criar `apps/app/src-tauri/src/update_check.rs`:
  - `pub struct UpdateStatus { pub current: String, pub latest: Option<String>, pub needs_update: bool, pub release_url: Option<String>, pub release_notes: Option<String> }`
  - `pub async fn check_github_releases(owner: &str, repo: &str) -> Result<UpdateStatus, String>` — usa `ureq` (já em workspace via mustard-cli), parseia JSON, extrai `tag_name`, `html_url`, `body`
  - Cache em `app_data_dir().join(".update-cache.json")` com timestamp; respeitar TTL 1h
  - Tauri command `#[tauri::command] async fn check_for_updates(force: bool) -> Result<UpdateStatus, String>` — `force=true` bypassa cache
  - Constante `GITHUB_REPO_OWNER = "atiz-tech"` (ou whatever) e `GITHUB_REPO_NAME = "mustard"` — confirmar com user durante implementação OU ler de tauri.conf.json (`bundle.publisher` se existir)
  - Tratar rate limit (HTTP 403) gracefully: retorna `latest: None` sem panic
  - Comparação de versão via `semver::Version::parse` (crate `semver` já no workspace; senão adicionar — versão `^1.0`)
- [ ] Criar `apps/app/src-tauri/src/project_sync.rs`:
  - `pub struct OutOfSyncProject { pub path: String, pub installed_version: Option<String>, pub app_version: String }`
  - `pub fn out_of_sync_projects(app_version: &str, registry_paths: Vec<String>) -> Vec<OutOfSyncProject>`
  - Por path: `projects::read_mustard_json_version(&Path::new(&path).join(".claude"))` (função pública? senão expor)
  - Compara via semver: instalada < app → incluir
  - Tauri command `#[tauri::command] async fn list_out_of_sync_projects() -> Vec<OutOfSyncProject>` — lê registry da store (zustand persiste em filesystem via tauri-plugin-store)
  - Tauri command `#[tauri::command] async fn update_project(path: String) -> Result<UpdateLog, String>` — chama `mustard_cli::update(Path::new(&path), &UpdateOptions { force: false })` e retorna `UpdateLog { files_changed: Vec<String>, summary: String }`
  - Estrutura `UpdateLog` serializável; mustard_cli::update precisa retornar info de quantos arquivos mudaram (ajustar API se necessário OU contar via dir scan pré/pós)
- [ ] Registrar `mod update_check;` e `mod project_sync;` em `apps/app/src-tauri/src/lib.rs`
- [ ] Adicionar commands no `invoke_handler!`
- [ ] Verificar se `mustard_cli::read_mustard_json_version` é público ou precisa ajustar visibility
- [ ] Build/type-check: `cargo check -p mustard-app`
- [ ] Testes unitários: cobrir cache hit/miss em `update_check`, e ordenação de out-of-sync em `project_sync`

## Files (~3)

```
apps/app/src-tauri/src/update_check.rs                — NOVO
apps/app/src-tauri/src/project_sync.rs                — NOVO
apps/app/src-tauri/src/lib.rs                         — mod + invoke_handler
```
