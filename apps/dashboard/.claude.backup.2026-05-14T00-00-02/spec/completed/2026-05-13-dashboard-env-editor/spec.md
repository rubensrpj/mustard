# Feature: dashboard-env-editor

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T19:30:00Z
### Lang: pt

## Contexto

As variáveis de ambiente `MUSTARD_*` controlam comportamentos críticos da pipeline (modos `strict|warn|off` para QA gate, close gate, commit gate, bash redirect, model routing, duplication/convention checks; thresholds para cluster discovery; lista CSV de hooks desabilitados; etc.). Hoje só são editáveis via JSON cru em `.claude/settings.json#env`, sem documentação inline do que cada modo faz nem de quais valores são válidos. O usuário precisa abrir `pipeline-config.md` em outro editor pra descobrir que `MUSTARD_QA_GATE_MODE=warn` significa "loga aviso mas deixa CLOSE passar" — fricção real que vira "deixa o default e reza". O dashboard legado tinha um editor visual desses envs com select por valor e descrição contextual; sem ele, a equipe não consegue ajustar gates de pipeline durante uma emergência (rampa de feature, troubleshoot de hook quebrado) sem risco de digitar errado e quebrar o JSON inteiro.

## Resumo

Portar o editor visual de envs `MUSTARD_*` para a página `/settings`. Catálogo declarativo em TS (`src/data/env-catalog.ts`) agrupado por domínio (Pipeline Gates, Cost Hooks, Anti-Slope, Cluster Discovery, Scan, Lang). Comandos Rust `dashboard_read_env` / `dashboard_write_env` em `lib.rs` que leem/mesclam `.claude/settings.json#env` preservando demais fields (permissions, hooks, mcpServers) com escrita atômica (tmp + rename). Section "Environment" anexada à Settings page com select por value, valueDocs em muted abaixo, botões Salvar/Descartar.

## Limites

Arquivos intencionalmente tocados:

- `src-tauri/src/lib.rs` (2 comandos novos + registro no invoke_handler)
- `src/data/env-catalog.ts` (NOVO — typed catalog)
- `src/api/env.ts` (NOVO — wrappers `invoke('dashboard_read_env'|'dashboard_write_env')`)
- `src/pages/Settings.tsx` (extensão — section "Environment")

Fora do escopo: editor visual de `permissions`, `hooks`, `statusLine`, `mcpServers` (só `env`); leitura de envs do `process.env` do sistema (só do settings.json do projeto); validação semântica entre keys (ex: avisar se `QA_GATE=off` mas `CLOSE_GATE=strict` — útil mas pra futuro); GUI para criar nova key fora do catálogo (catálogo é authoritativo).

## Arquivos (~4)

| Arquivo | Operação | Notas |
|---------|----------|-------|
| `src-tauri/src/lib.rs` | modificar | `dashboard_read_env(repo_path)` retorna `HashMap<String,String>`; `dashboard_write_env(repo_path, env)` faz merge no field `env` preservando demais fields, escrita atômica via tmp+rename. Registrar ambos no `invoke_handler!`. |
| `src/data/env-catalog.ts` | criar | Interfaces `EnvKey { key, default, options[], desc, valueDocs: Record<string,string> }`, `EnvGroup { group, desc, keys[] }`. Array `ENV_CATALOG: EnvGroup[]` com 6 grupos / ≥18 keys. |
| `src/api/env.ts` | criar | `readEnv(repoPath): Promise<Record<string,string>>` e `writeEnv(repoPath, env): Promise<void>`. Wrappers de `invoke()` da `@tauri-apps/api/core`. |
| `src/pages/Settings.tsx` | modificar | Manter section "Diretório de projetos" intacta. Adicionar section "Environment" condicional (só com projeto selecionado). Loop `ENV_CATALOG`, cada key com select + valueDocs muted. Botões Salvar/Descartar. `useQuery` + `useMutation` do react-query. Toast `sonner` (já instalado na Wave E). |

## Component Contract — Settings page (extensão)

- **Props**: nenhuma (page top-level já existente).
- **Estado novo** (no componente Settings):
  - `pendingEnv: Record<string, string>` — overrides locais não persistidos (vazio até user mudar algo)
- **Derivado**:
  - `envFromDisk` via `useQuery(['env', selectedProject.path], () => readEnv(selectedProject.path))` (enabled só se há projeto selecionado)
  - `effectiveEnv` (useMemo): `{ ...envFromDisk, ...pendingEnv }` — usado pra renderizar selects
  - `hasPending` (useMemo): `Object.keys(pendingEnv).length > 0`
- **Mutation**: `useMutation({ mutationFn: (env) => writeEnv(selectedProject.path, env), onSuccess: () => { queryClient.invalidateQueries(['env', selectedProject.path]); setPendingEnv({}); toast.success('Salvo'); }, onError: (e) => toast.error('Erro: ' + e.message) })`
- **Handlers**:
  - `onSelectChange(key, value)` → `setPendingEnv(prev => value === envFromDisk[key] ? omitKey(prev, key) : { ...prev, [key]: value })`
  - `onSave()` → mutate effectiveEnv (envia merged completo)
  - `onDiscard()` → `setPendingEnv({})`
- **Render**:
  - Cards por grupo: `<Card size="sm">` com header (group name + desc), body iterando keys.
  - Cada key: `<label className="font-mono text-[13px]">{key}</label>` + Badge `default: {default}`, `<p className="text-[13px] text-muted-foreground">{desc}</p>`, `<select>` com options, `<p className="text-[12px] text-muted-foreground/70 mt-0.5">{valueDocs[selectedValue]}</p>`.
  - Bottom da section: 2 botões `Salvar mudanças` (disabled se `!hasPending`) / `Descartar` (disabled se `!hasPending`). Counter "(N pendentes)" ao lado.

## Tarefas

### Backend Agent (Wave 1) — Rust

- [ ] **(parallel-safe)** Em `src-tauri/src/lib.rs`, adicionar comando `dashboard_read_env`:
  - Assinatura: `fn dashboard_read_env(repo_path: String) -> Result<HashMap<String, String>, String>`.
  - Lógica: ler `{repo_path}/.claude/settings.json` via `std::fs::read_to_string`, parsear como `serde_json::Value`. Se field `env` for object, converter para `HashMap<String,String>` (cada value via `.as_str().unwrap_or("").to_string()`). Se arquivo não existe ou `env` ausente, retornar `HashMap::new()`. Erros (parse fail, IO error) → `Err(e.to_string())`.

- [ ] **(parallel-safe)** Em `src-tauri/src/lib.rs`, adicionar comando `dashboard_write_env`:
  - Assinatura: `fn dashboard_write_env(repo_path: String, env: HashMap<String, String>) -> Result<(), String>`.
  - Lógica: ler `{repo_path}/.claude/settings.json` (se não existe, começar com `serde_json::json!({})`), parsear como `serde_json::Value`. Garantir que é object. Mutar `value["env"] = serde_json::to_value(env)?`. Serializar com `serde_json::to_string_pretty` (preservar legibilidade). **Escrita atômica**: escrever em `{settings_path}.tmp` primeiro, então `std::fs::rename(tmp, settings_path)`. Em caso de erro durante write, remover tmp e retornar `Err`. NÃO sobrescrever os outros fields (`permissions`, `hooks`, `mcpServers`, `$schema`, etc.) — o `serde_json::Value` mantém os demais intactos porque só mutamos `["env"]`.

- [ ] Em `src-tauri/src/lib.rs`, registrar `dashboard_read_env, dashboard_write_env` no `tauri::generate_handler![...]` (linha ~926-933 hoje), preservando ordem alfabética relativa.

- [ ] Validar com `cargo check` (path explícito Windows: `& "$env:USERPROFILE\.cargo\bin\cargo.exe" check`) ou via `npm run tauri build` se o agente preferir delegar.

### UI Data Agent (Wave 1) — TS catalog

- [ ] **(parallel-safe)** Criar `src/data/env-catalog.ts`:
  - Exportar `interface EnvKey { key: string; default: string; options: string[]; desc: string; valueDocs: Record<string, string>; }`.
  - Exportar `interface EnvGroup { group: string; desc: string; keys: EnvKey[]; }`.
  - Exportar `ENV_CATALOG: EnvGroup[]` com **6 grupos / ≥18 keys**:
    1. **Pipeline Gates**: `MUSTARD_QA_GATE_MODE` (strict/warn/off, default strict), `MUSTARD_CLOSE_GATE_MODE` (strict/warn/off, default strict), `MUSTARD_COMMIT_GATE_MODE` (strict/warn/off, default warn), `MUSTARD_SPEC_SIZE_MODE` (strict/warn/off, default warn).
    2. **Cost Hooks**: `MUSTARD_BASH_REDIRECT_MODE` (strict/warn/off, default strict), `MUSTARD_MODEL_GATE_MODE` (strict/warn/off, default strict), `MUSTARD_DISABLED_HOOKS` (free CSV, default ``).
    3. **Anti-Slope**: `MUSTARD_DUPLICATION_MODE` (strict/warn/off, default off), `MUSTARD_CONVENTION_MODE` (strict/warn/off, default off).
    4. **Cluster Discovery**: `MUSTARD_CLUSTER_MIN_FILES` (numbers '2'..'10', default '5'), `MUSTARD_CLUSTER_MIN_SUFFIX_LEN` (numbers '2'..'10', default '6'), `MUSTARD_CLUSTER_MIN_BASE_INHERITORS` (numbers '2'..'10', default '3'), `MUSTARD_CLUSTER_MAX` ('10'/'30'/'50'/'100', default '30'), `MUSTARD_DECORATOR_MIN` (numbers '2'..'10', default '3'), `MUSTARD_FN_PREFIX_MIN` (numbers '2'..'10', default '5'), `MUSTARD_FN_PREFIX_MIN_LEN` (numbers '2'..'5', default '2'), `MUSTARD_NAMING_DOMINANCE` ('0.5'/'0.6'/'0.7'/'0.8'/'0.9'/'0.95', default '0.6'), `MUSTARD_CLUSTER_CACHE` ('on'/'off', default 'on').
    5. **Scan**: `MUSTARD_SCAN_IGNORE` (free CSV de pasta names, default ``).
    6. **Lang**: `MUSTARD_SPEC_LANG` ('pt'/'en', default 'en').
  - `desc` plain text 1 linha por key descrevendo o que ela controla. `valueDocs` map por valor (ex: `{ strict: 'Bloqueia X', warn: 'Loga aviso', off: 'Desabilita check' }`). Conteúdo derivado de `pipeline-config.md` § Enforcement Hooks e Cluster Discovery Tuning.

### UI API Wrapper Agent (Wave 1) — TS bindings

- [ ] **(parallel-safe)** Criar `src/api/env.ts`:
  - Importar `invoke` de `@tauri-apps/api/core`.
  - Exportar `async function readEnv(repoPath: string): Promise<Record<string, string>>` → `invoke('dashboard_read_env', { repoPath })`.
  - Exportar `async function writeEnv(repoPath: string, env: Record<string, string>): Promise<void>` → `invoke('dashboard_write_env', { repoPath, env })`.

### UI Page Agent (Wave 2) — depende dos 3 anteriores

- [ ] Modificar `src/pages/Settings.tsx`:
  - Manter intacta a section "Diretório de projetos".
  - Importar `useQuery, useMutation, useQueryClient` do `@tanstack/react-query`, `toast` do `sonner`, `ENV_CATALOG, type EnvKey, type EnvGroup` do `@/data/env-catalog`, `readEnv, writeEnv` do `@/api/env`, `useStore` (`s => s.selectedProjectId`) e o list de projects do `useQuery(['discover', projectsRoot])` (mesmo pattern usado em Sidebar).
  - Resolver `selectedProject` (find de `projects` por `selectedProjectId`). Se `!selectedProject`, NÃO renderizar a section "Environment" (apenas a section atual + um hint `<p className="text-[13px] text-muted-foreground">Selecione um projeto na sidebar para editar variáveis MUSTARD_*.</p>`).
  - Section "Environment" (renderizada só com projeto selecionado):
    - Header: `<h2 className="text-sm font-medium">Environment — {selectedProject.name}</h2>` + `<p className="text-[13px] text-muted-foreground">Variáveis MUSTARD_* persistidas em .claude/settings.json#env.</p>`.
    - `useQuery(['env', selectedProject.path], () => readEnv(selectedProject.path), { staleTime: 60_000 })`.
    - `pendingEnv` em `useState<Record<string,string>>({})`.
    - Loop `ENV_CATALOG.map(group => …)` em `<Card size="sm">`, cada card com `<CardHeader><CardTitle className="text-sm font-medium">{group.group}</CardTitle><CardDescription className="text-[13px] text-muted-foreground">{group.desc}</CardDescription></CardHeader>` + body iterando `group.keys`.
    - Cada key: `<div className="px-4 pb-3 flex flex-col gap-1">` com `<div className="flex items-baseline gap-2"><label className="font-mono text-[13px]">{k.key}</label><Badge variant="secondary" className="text-[11px]">default: {k.default}</Badge></div>`, `<p className="text-[13px] text-muted-foreground">{k.desc}</p>`, `<select className="bg-card border border-border rounded text-sm px-2 py-1 focus:border-primary outline-none w-full">` ou `<input>` se `options` vazio (ex: CSV) → `value={effectiveEnv[k.key] ?? k.default}` `onChange={e => onSelectChange(k.key, e.target.value)}`. Abaixo: `<p className="text-[12px] text-muted-foreground/70">{k.valueDocs[effectiveEnv[k.key] ?? k.default] ?? ''}</p>`.
    - Bottom: `<div className="flex items-center gap-2 pt-2 border-t border-border">` com `<button disabled={!hasPending} onClick={onSave}>Salvar mudanças</button>`, `<button disabled={!hasPending} onClick={onDiscard}>Descartar</button>`, `<span className="text-[13px] text-muted-foreground ml-auto">{Object.keys(pendingEnv).length} pendentes</span>`.
  - `onSave`: chama `mutate(effectiveEnv)`. `onSuccess` → invalida query, `setPendingEnv({})`, `toast.success('Salvo')`. `onError` → `toast.error(...)`.

### Build & Type-check (Wave 3)

- [ ] Da raiz: `npx tsc --noEmit` deve passar (exit 0).
- [ ] Da raiz: `npm run build` deve passar (exit 0).
- [ ] `cargo check` (path explícito Windows: `& "$env:USERPROFILE\.cargo\bin\cargo.exe" check --manifest-path src-tauri/Cargo.toml`) deve passar (exit 0).

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript compila sem erros — Command: `npx tsc --noEmit`
- [x] AC-2: `src/data/env-catalog.ts` exporta ENV_CATALOG com ≥18 keys totais — Command: `node -e "const t=require('fs').readFileSync('src/data/env-catalog.ts','utf8');const c=t.split('key:').length-1;process.exit(c>=18?0:1)"`
- [x] AC-3: `src/api/env.ts` exporta readEnv e writeEnv — Command: `node -e "const t=require('fs').readFileSync('src/api/env.ts','utf8');process.exit(t.includes('readEnv')&&t.includes('writeEnv')&&t.includes('dashboard_read_env')&&t.includes('dashboard_write_env')?0:1)"`
- [x] AC-4: `lib.rs` define dashboard_read_env e dashboard_write_env e os registra — Command: `node -e "const t=require('fs').readFileSync('src-tauri/src/lib.rs','utf8');const ok=t.includes('fn dashboard_read_env')&&t.includes('fn dashboard_write_env')&&t.split('dashboard_read_env').length>=3&&t.split('dashboard_write_env').length>=3;process.exit(ok?0:1)"`
- [x] AC-5: Settings.tsx referencia env-catalog e env api — Command: `node -e "const t=require('fs').readFileSync('src/pages/Settings.tsx','utf8');process.exit(t.includes('env-catalog')&&t.includes('readEnv')&&t.includes('writeEnv')?0:1)"`
- [x] AC-6: Settings.tsx mantém section "Diretório de projetos" — Command: `node -e "const t=require('fs').readFileSync('src/pages/Settings.tsx','utf8');process.exit(t.includes('Diret')?0:1)"`
- [x] AC-7: cargo check passa — Command: `"%USERPROFILE%\.cargo\bin\cargo.exe" check --manifest-path src-tauri/Cargo.toml`
- [x] AC-8: Build Vite passa — Command: `npm run build`

## Não-Objetivos

- Editor visual de `permissions`, `hooks`, `statusLine`, `mcpServers`
- Leitura/escrita de envs do `process.env` do sistema operacional
- Validação cruzada entre keys (ex: alertar combinações inválidas)
- UI para criar key arbitrária fora do catálogo (catálogo é authoritativo)
- Histórico/undo de mudanças (só "Descartar" antes de salvar)
- Sync das alterações para outros projetos (uma mudança = um projeto)

## Decisões não-óbvias

- **`serde_json::Value` para preservar fields desconhecidos** — não modelamos `Settings` como struct tipada; isso permite que o Rust ignore `permissions`, `hooks`, `mcpServers` etc. e só mutua `["env"]`. Trade-off: sem validação de schema no Rust, mas o catálogo TS já valida no client.
- **Escrita atômica `tmp + rename`** — mesmo se o processo cair durante o `write_to_string`, `settings.json` original fica intacto. `rename` é atômico em NTFS/POSIX. Se rename falhar (raro), removemos o tmp e propagamos erro.
- **Catálogo declarativo TS** (mesma decisão de Wave D) — versionado com o app, type-safe, independente da instalação do Mustard core.
- **Section "Environment" só visível com projeto selecionado** — o env é por-projeto (cada projeto tem seu próprio `.claude/settings.json`); sem projeto não há contexto pra editar.
- **`pendingEnv` separado de `envFromDisk`** — permite preview do que vai mudar sem persistir; "Descartar" é trivial (`setPendingEnv({})`); a comparação com `envFromDisk[k]` evita registrar como pendente um valor que igual ao já gravado.
- **Lang field MUSTARD_SPEC_LANG no catálogo** — referenciado em `feature/SKILL.md` (cascata de language resolution), incluído pra usuário poder forçar `pt` ou `en` sem editar `mustard.json` manualmente.
- **`MUSTARD_DISABLED_HOOKS` é free CSV** (não select) — lista de nomes de hooks; UI deve renderizar `<input>` se `options.length === 0`.
