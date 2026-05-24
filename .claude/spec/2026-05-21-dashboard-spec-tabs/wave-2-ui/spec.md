# Wave 2 — Ondas: contagem real + drawer com markdown

## Resumo

A aba "Ondas" hoje mostra `wave.files_changed` que vem do projeção SQLite e é zero para waves sem eventos `tool.use` (caso comum em specs migradas / fechadas há tempo). O usuário quer ver o número real de arquivos listados no bloco `## Arquivos` do `wave-N-{role}/spec.md` da sub-spec — esse é o canon versionado. Adicionalmente, clicar numa wave abre um drawer (`<WaveMarkdownDrawer>`) com o markdown completo daquela wave renderizado. Adiciona-se um subcommand Rust `mustard-rt run wave-files --spec --wave` que devolve a contagem + o markdown, e um command Tauri `dashboard_spec_wave_files` que invoca esse subcommand.

## Contexto

`SpecWavesTab.tsx` recebe `waves: SpecWave[]` do hook `useSpecWaves`. Cada `SpecWave` tem `files_changed` (vindo do reader Rust). Esse número é a contagem de eventos `tool.use` com `tool_name in (Write|Edit)` por wave — útil pra runtime, mas zero pra waves que rodaram antes do tracker ou em sessão paralela. Pra o usuário, o canon é "quantos arquivos a sub-spec da wave declara em `## Arquivos`".

O fix: ler `wave-N-{role}/spec.md`, extrair o bloco `## Arquivos`, contar linhas não-vazias (entradas do bloco code-fence ou bullets). Devolver `subspec_files_count` num novo campo do payload do command.

Clique abre o `<WaveMarkdownDrawer>` (shadcn `Sheet` à direita). Reutiliza `react-markdown` v10 — o `<SpecMarkdownViewer>` já tem o padrão de `pre` override (memory `react_markdown_v10`); a gente extrai o renderer comum se necessário, mas vai mais direto criar um drawer dedicado com o `<MarkdownRenderer>` da própria pasta (se não houver, embutir local).

## Arquivos

```
apps/rt/src/run/wave_files.rs                                — NOVO: subcommand mustard-rt run wave-files
apps/rt/src/run/mod.rs                                       — registrar wave-files
apps/dashboard/src-tauri/src/lib.rs                          — NOVO command dashboard_spec_wave_files
apps/dashboard/src/lib/dashboard.ts                          — wrapper dashboardSpecWaveFiles
apps/dashboard/src/hooks/useSpecWaveFiles.ts                 — NOVO hook
apps/dashboard/src/components/specs/SpecWavesTab.tsx         — onda clicável + render subspec_files_count
apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx   — NOVO drawer shadcn Sheet com markdown
```

## Tarefas

- [ ] Em `apps/rt/src/run/wave_files.rs`: criar função `pub fn run(spec: Option<&str>, wave: Option<u32>)` que:
  - Resolve `.claude/spec/{spec}/wave-{wave}-*/spec.md` (glob por número da wave, qualquer role).
  - Lê o arquivo, extrai o bloco entre `## Arquivos` e o próximo `## ` (case-sensitive, encoding UTF-8).
  - Conta entradas: linhas não-vazias dentro de code-fence (` ``` `) ou bullets (`- `). Linhas comentário (`//`) ou separadores não contam.
  - Devolve JSON `{ "count": N, "markdown": "<full file content>", "path": "<resolved path>" }` em stdout. Fail-open: arquivo ausente → `{"count":0,"markdown":"","path":null}`.
- [ ] Registrar em `apps/rt/src/run/mod.rs`: novo arm em `dispatch_run` reconhece `"wave-files"` e parseia `--spec`, `--wave`.
- [ ] Adicionar testes em `wave_files.rs`:
  - `counts_files_from_arquivos_block` — fixture com `## Arquivos` + code-fence de 3 paths → conta 3.
  - `counts_files_from_bullets` — fixture com bullets `-` → conta corretamente.
  - `returns_zero_when_file_missing` — caminho que não existe → `count: 0`.
  - `returns_zero_when_arquivos_section_absent` — fixture sem `## Arquivos` → conta 0.
- [ ] Em `apps/dashboard/src-tauri/src/lib.rs`: registrar command `dashboard_spec_wave_files(repo_path: String, spec: String, wave: u32) -> Result<WaveFilesPayload, String>`. A implementação spawna `mustard-rt run wave-files` ou chama direto se houver fronteira de crate disponível (preferir API direta — `apps/dashboard/src-tauri` já depende de `mustard-core`; se `wave_files::run` puder ser exposto via `pub(crate)`, usar; senão, subprocesso é aceitável).
- [ ] Em `apps/dashboard/src/lib/dashboard.ts`: adicionar `export async function dashboardSpecWaveFiles(path: string, spec: string, wave: number): Promise<WaveFilesPayload>`. Type `WaveFilesPayload = { count: number; markdown: string; path: string | null }`.
- [ ] Criar `apps/dashboard/src/hooks/useSpecWaveFiles.ts` no padrão do `useSpecWaves` (`useQuery` por `[repoPath, spec, wave]`, `enabled: !!repoPath && !!spec && wave > 0`, `staleTime: 30_000`).
- [ ] Em `SpecWavesTab.tsx`:
  - Aceitar prop opcional `onOpenWave?: (wave: number) => void`. O `<SpecDetailDashboard>` provê o handler.
  - Cada `<li>` vira `<button>` (semântica) ou `<li onClick role="button" tabIndex={0}>` com handler. `aria-label="Abrir markdown da wave N"`.
  - Trocar `{wave.files_changed} arquivos` para mostrar `{subspec_files_count}` quando disponível (passa via prop derivado de `useSpecWaveFiles` por wave). Fallback pra `files_changed` se a query estiver em loading/erro. Tooltip: "arquivos declarados em `## Arquivos`".
- [ ] Criar `WaveMarkdownDrawer.tsx`: shadcn `Sheet` à direita (largura ~40rem). Recebe `{ open, onOpenChange, repoPath, spec, wave }`. Usa `useSpecWaveFiles` pra pegar o markdown. Render com `react-markdown` v10 (override `pre` separately — ver memory `react_markdown_v10`). Header do sheet: `"Wave {N} — {role}"` (role vem do `SpecWave`).
- [ ] No `<SpecDetailDashboard>` (criado na Wave 1): manter state `openWave: number | null`. Passar `onOpenWave={setOpenWave}` pro `<SpecWavesTab>`. Renderizar `<WaveMarkdownDrawer open={!!openWave} onOpenChange={(o)=>!o && setOpenWave(null)} wave={openWave} ...>`.
- [ ] Build: `cargo build -p mustard-rt`
- [ ] Testes: `cargo test -p mustard-rt --bin mustard-rt wave_files`
- [ ] Dashboard build: `pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W2-1: `mustard-rt run wave-files --spec 2026-05-21-flatten-spec-layout-and-multi-collab --wave 1` retorna `count > 0` — Command: `bash -c 'cargo run -q -p mustard-rt -- run wave-files --spec 2026-05-21-flatten-spec-layout-and-multi-collab --wave 1 | node -e "const j=JSON.parse(require(\"fs\").readFileSync(0,\"utf8\"));process.exit(j.count>0?0:1)"'`
- [ ] AC-W2-2: Testes do wave-files passam — Command: `cargo test -p mustard-rt --bin mustard-rt wave_files`
- [ ] AC-W2-3: `<WaveMarkdownDrawer>` existe — Command: `node -e "process.exit(require('fs').existsSync('apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx')?0:1)"`
- [ ] AC-W2-4: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`

## Limites

- `apps/rt/src/run/wave_files.rs` (novo)
- `apps/rt/src/run/mod.rs`
- `apps/dashboard/src-tauri/src/lib.rs`
- `apps/dashboard/src/lib/dashboard.ts`
- `apps/dashboard/src/hooks/useSpecWaveFiles.ts` (novo)
- `apps/dashboard/src/components/specs/SpecWavesTab.tsx`
- `apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx` (novo)

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs]]
- Depende: [[wave-1-ui]] (precisa do `<SpecDetailDashboard>` pra pendurar o drawer)
