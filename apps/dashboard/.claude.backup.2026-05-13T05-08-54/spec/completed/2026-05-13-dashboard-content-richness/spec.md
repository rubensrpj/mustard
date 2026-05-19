# Feature: dashboard-content-richness

### Status: closed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T03:00:00Z
### Lang: pt

## Contexto

O dashboard hoje renderiza o conteúdo lido do disco com fidelidade fraca em três pontos visíveis. SpecDetail extrai `## Acceptance Criteria` e `## Checklist` por regex e monta a lista manualmente, então task lists `- [ ]` aparecem como literal, blocos de código não recebem estilo e headings dentro de seções não viram heading. A descoberta de skills (`dashboard_skills` em `src-tauri/src/lib.rs`) varre apenas `base/.claude/skills` (foundation) e `base/.claude/commands/mustard` (command), ignorando `{subproject}/.claude/skills/` — em repos multi-subprojeto como sialia, as skills específicas de cada subprojeto nunca aparecem. E o feed de eventos retorna só `event_type`, `ts` e um `summary` extraído de chaves opcionais do payload, descartando informação rica que já está armazenada (tool_name, file_path, command, pattern, spec, wave, actor). O resultado é que o usuário vê os mesmos dados de forma mais pobre do que o que existe no disco — qualidade percebida cai sem que falte feature.

## Resumo

Plugar um renderer de markdown real (react-markdown + remark-gfm) em SpecDetail; estender `dashboard_skills` para iterar subprojetos via `sync-detect`; e enriquecer `RecentEvent` com `tool_name`, `target`, `wave`, `spec`, `actor_kind`, `actor_id` (lendo do payload), consumindo nos componentes de Activity e ProjectDetail.

## Limites

Edições intencionalmente restritas a:

- `package.json`, `pnpm-lock.yaml` (adicionar react-markdown + remark-gfm)
- `src/components/Markdown.tsx` (novo)
- `src/pages/SpecDetail.tsx`
- `src/pages/Activity.tsx`
- `src/pages/ProjectDetail.tsx`
- `src/lib/dashboard.ts` (tipo `RecentEvent` TS bridge)
- `src-tauri/src/lib.rs` (struct `RecentEvent`, `dashboard_skills`, fallback JSONL em `dashboard_recent_events`)
- `src-tauri/src/db.rs` (`recent_events_from_db`, `search_events_from_db`, novo helper `extract_event_details`)

Fora de boundary: schemas Tauri auto-gerados, hooks, scripts, demais páginas/componentes.

## Arquivos

Novos (~1):
- `src/components/Markdown.tsx`

Modificados (~8):
- `package.json` + `pnpm-lock.yaml`
- `src/pages/SpecDetail.tsx`
- `src/pages/Activity.tsx`
- `src/pages/ProjectDetail.tsx`
- `src/lib/dashboard.ts`
- `src-tauri/src/lib.rs`
- `src-tauri/src/db.rs`

## Component Contract

### `src/components/Markdown.tsx`

**Propósito:** Renderizar markdown bruto com GFM (task lists, tables, strikethrough), tipografia condizente com o restante do dashboard (`text-sm`, `font-mono` para code, `text-muted-foreground` para hierarquia).

**Props:**
- `content: string` — markdown bruto. Wrapper passa tudo para `<ReactMarkdown>` com `remarkPlugins={[remarkGfm]}`.

**Component overrides (obrigatórios):**
- `code` inline → `<code className="font-mono text-xs bg-muted px-1 py-0.5 rounded">`
- `code` block → `<pre className="font-mono text-xs bg-muted/40 p-3 rounded border border-border overflow-x-auto"><code>...`
- `input[type=checkbox]` → renderizar `disabled checked={props.checked}` com `translate-y-[1px] mr-1` (task list GFM gera input HTML real)
- `h1` → `text-base font-semibold mt-4 mb-2`
- `h2` → `text-sm font-semibold mt-3 mb-1.5 text-foreground`
- `h3` → `text-xs font-semibold mt-2 mb-1 uppercase tracking-wider text-muted-foreground`
- `ul` → `list-disc pl-4 space-y-0.5 text-sm`
- `ol` → `list-decimal pl-4 space-y-0.5 text-sm`
- `li` → `leading-relaxed`
- `a` → `text-primary hover:underline`
- `p` → `text-sm leading-relaxed`

**Sem estado, sem efeitos.** É um wrapper puro.

## Tarefas

### Frontend Agent (Wave 1 — Markdown Renderer) (parallel-safe)

- [ ] Instalar deps: `pnpm add react-markdown remark-gfm` (registra em `dependencies`, NÃO em devDependencies)
- [ ] Criar `src/components/Markdown.tsx` com a estrutura do Component Contract acima — wrapper único, exporta `Markdown` named (não default)
- [ ] Refatorar `src/pages/SpecDetail.tsx`: remover `extractSection`, `parseAcceptanceCriteria`, `parseChecklist`, `AcItem`, `ChecklistNode`, os `useMemo` correspondentes e as `<section>` que dependiam deles. Substituir por um único `<section>` com `<Markdown content={markdown} />` cobrindo o documento inteiro. Manter o bloco "Affected files" como `<section>` separada (vem de `row.affected_files`, não do markdown). Manter o header (breadcrumb, badges, voltar), loading e error states intactos
- [ ] Verificar tipos: `pnpm run build` deve passar (vite usa `tsc -b`)
- [ ] Confirmar visual: a spec aberta deve mostrar headings hierárquicos, code blocks estilizados, checkbox HTML real para `- [ ]` / `- [x]`

### Backend Agent (Wave 1 — Rust) (parallel-safe)

- [ ] Estender struct `RecentEvent` em `src-tauri/src/lib.rs` (linha ~190): adicionar campos `spec: Option<String>`, `wave: Option<i64>`, `actor_kind: Option<String>`, `actor_id: Option<String>`, `tool_name: Option<String>`, `target: Option<String>`. Manter os 3 campos existentes (`event_type`, `ts`, `summary`). Todos `Option`, preservar `#[serde(rename_all = "snake_case")]`
- [ ] Criar helper em `src-tauri/src/db.rs`: `fn extract_event_details(payload: &Option<String>, event_type: &str) -> (Option<String>, Option<String>)` retornando `(tool_name, target)`. Parsea payload JSON: `tool_name = payload.tool_name`; `target = payload.tool_input.file_path || payload.tool_input.command || payload.tool_input.pattern || payload.tool_input.url`. Para `event_type == "agent.start"`, target = `payload.agent_type`. Para `event_type == "pipeline.phase"`, target = `payload.phase`. Retorna `(None, None)` se payload é `None` ou JSON inválido (fail-soft, nunca explode)
- [ ] Atualizar `recent_events_from_db` em `src-tauri/src/db.rs`: SELECT já traz `spec` (linha 196 hoje descartava como `_spec`); usar. Acrescentar colunas `wave`, `actor_kind`, `actor_id` se existirem no schema (se não, deixar `None`). Chamar `extract_event_details(&payload, &event_type)` para popular `tool_name` e `target`
- [ ] Atualizar `search_events_from_db` em `src-tauri/src/db.rs` análogo a `recent_events_from_db`
- [ ] Atualizar o fallback JSONL em `dashboard_recent_events` (`src-tauri/src/lib.rs` linhas 339-362): para cada `serde_json::Value`, popular todos os novos campos lendo `v["spec"]`, `v["wave"]`, `v["actor_kind"]`, `v["actor_id"]`, `v["payload"]["tool_name"]`, etc. — mesma lógica do helper, mas operando sobre o `Value` já parseado. Pode extrair função `extract_from_json_value` em `lib.rs` para evitar duplicação
- [ ] Estender `dashboard_skills` em `src-tauri/src/lib.rs` (linhas 270-337): após o loop existente sobre foundation+command, invocar `node .claude/scripts/sync-detect.js` (mesmo pattern de `dashboard_subprojects` linhas 200-209), parsear o JSON, iterar `subprojects[]`. Para cada subprojeto, montar `base.join(&name).join(".claude").join("skills")` e aplicar o mesmo walk do bloco existente, com `source = format!("subproject:{}", name)`. Se sync-detect falhar (exit non-zero), logar via `eprintln!` e continuar com foundation+command apenas (fail-open)
- [ ] Verificar compilação: `cargo check --manifest-path src-tauri/Cargo.toml` deve passar sem warnings

### Frontend Agent (Wave 2 — Consume Rich Backend)

**Dependência:** Wave 1 Backend completa (campos novos disponíveis no IPC).

- [ ] Atualizar tipo `RecentEvent` em `src/lib/dashboard.ts`: adicionar `spec?: string | null`, `wave?: number | null`, `actor_kind?: string | null`, `actor_id?: string | null`, `tool_name?: string | null`, `target?: string | null`. Preservar campos existentes
- [ ] Refatorar render de eventos em `src/pages/Activity.tsx` (linhas 122-152): cada `<li>` mostra StatusDot + Badge `event_type` + projectName, então **se** `tool_name` existir, badge outline mono `text-[10px]` para tool_name; **se** `target`, `<code className="text-xs text-muted-foreground font-mono truncate max-w-md">{target}</code>`; **se** `spec`, `<span className="text-xs text-muted-foreground">{spec}</span>`; **se** `wave !== null && wave !== undefined`, `<span className="text-xs text-muted-foreground">W{wave}</span>`; timestamp vai para `ml-auto` à direita. Manter `summary` como fallback (`— {truncate(summary, 200)}`) **só se** nenhum dos campos ricos estiver disponível
- [ ] Atualizar a seção "Eventos" em `src/pages/ProjectDetail.tsx` para usar o mesmo padrão visual rico do Activity (extrair função helper local `renderEventLine` se houver duplicação significativa entre ambas — só extrair se a duplicação for óbvia, senão deixar inline em ambas)
- [ ] Atualizar a seção "SKILLS" do tab About em `src/pages/ProjectDetail.tsx`: agrupar skills por `source`. Headings de grupo: "Foundation" para `foundation`, "Commands" para `command`, "Subprojeto: {nome}" para `subproject:{nome}` (extrair `nome` do prefixo). Renderização simples — `<div>` com `<h3>` por grupo seguido de `<ul>` de skills, sem accordion (manter consistência com tipografia atual; accordion fica para uma futura iteração se preciso)
- [ ] `pnpm run build` passa, sem erros de tipo

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent. (Header kept in English because `qa-run.js` does not yet parse the PT heading "Critérios de Aceitação".)

- [x] AC-1: Build TypeScript passa — Command: `pnpm run build`
- [x] AC-2: cargo check passa sem warnings — Command: `node -e "const{execSync}=require('child_process');const path=require('path');const cargo=path.join(process.env.USERPROFILE||process.env.HOME,'.cargo','bin',process.platform==='win32'?'cargo.exe':'cargo');execSync(JSON.stringify(cargo)+' check --manifest-path src-tauri/Cargo.toml --message-format=short',{stdio:'inherit'})"`
- [x] AC-3: Componente Markdown criado em src/components/Markdown.tsx — Command: `node -e "process.exit(require('fs').existsSync('src/components/Markdown.tsx')?0:1)"`
- [x] AC-4: react-markdown e remark-gfm presentes em dependencies — Command: `node -e "const p=require('./package.json');process.exit((p.dependencies&&p.dependencies['react-markdown']&&p.dependencies['remark-gfm'])?0:1)"`
- [x] AC-5: Struct RecentEvent estendido com tool_name e target em lib.rs — Command: `node -e "const c=require('fs').readFileSync('src-tauri/src/lib.rs','utf8');process.exit((c.includes('pub tool_name:')&&c.includes('pub target:')&&c.includes('pub wave:'))?0:1)"`
- [x] AC-6: dashboard_skills itera subprojetos via sync-detect — Command: `node -e "const c=require('fs').readFileSync('src-tauri/src/lib.rs','utf8');const i=c.indexOf('fn dashboard_skills');const j=c.indexOf('fn ',i+10);const body=c.slice(i,j>0?j:c.length);process.exit(body.includes('sync-detect.js')?0:1)"`
- [x] AC-7: SpecDetail usa Markdown component, parseAcceptanceCriteria removido — Command: `node -e "const c=require('fs').readFileSync('src/pages/SpecDetail.tsx','utf8');process.exit((c.includes('Markdown')&&!c.includes('parseAcceptanceCriteria'))?0:1)"`

## Preocupações

- WARN (analyze-validation, layer-gap, x3): heurística não detectou extensões `.tsx` / `.rs` na seção `## Arquivos`. Falso positivo — `src/components/Markdown.tsx`, `src/pages/*.tsx`, `src-tauri/src/lib.rs` e `src-tauri/src/db.rs` estão listados em `## Arquivos`. Sem ação.

## Não-Objetivos

- Não adicionar accordion / collapse para grupos de skills (só agrupamento por heading)
- Não persistir filtros do Activity por evento rico (filtros atuais por `event_type` permanecem)
- Não adicionar sintaxe-highlight em code blocks do Markdown (só monospace + bg) — Prism/Shiki ficam fora
- Não tocar em outras páginas (Home, Settings, Knowledge, etc.) — só onde eventos/skills/spec markdown já são renderizados
- Não migrar parsing do `## Files` ou `## Acceptance Criteria` para Markdown — `affected_files` continua vindo do `row` (SQLite/state files), não do markdown
- Não trocar pnpm por npm (`pnpm@10.18.1` está pinado em packageManager)
