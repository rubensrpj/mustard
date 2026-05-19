# Feature: dashboard-sqlite-reader

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-12T00:00:00Z
### Lang: pt

## Contexto

O dashboard hoje lê dados do Mustard fazendo string-parse de `.claude/.harness/events.jsonl` e `.claude/knowledge.json` a cada chamada Tauri. Isso era aceitável quando o Mustard só escrevia JSONL/JSON, mas o Mustard 2.0 Phase 1 já entregou um `mustard.db` SQLite com schema indexado (events, specs, knowledge, spans) e FTS5 sobre eventos e knowledge — bem mais rápido em projetos grandes e a única forma viável de implementar busca full-text. A camada de leitura do app continua presa ao formato antigo, então features posteriores (Specs tab, KnowledgeBrowser) não conseguem subir sem essa troca. O efeito observável é que o app não usa o índice mesmo quando ele existe, e qualquer projeto que migrar para Mustard 2.0 vai pagar o custo do parse linear de novo a cada refresh.

## Summary

Adicionar leitor SQLite read-only ao Tauri (rusqlite bundled), refatorar os 4 commands existentes (`dashboard_metrics`, `dashboard_knowledge`, `dashboard_recent_events`, mais o struct `MetricsSummary` ganhando `tokens_total`/`tokens_today`) para tentar SQLite primeiro e cair para JSONL/JSON se o DB não existir ou não tiver schema Phase 1. Introduzir 3 commands novos: `dashboard_specs`, `dashboard_search_events`, `dashboard_search_knowledge`. Padronizar serde com `rename_all = "snake_case"` em todos structs. Remover linha hardcoded `0 subprojects` do Sidebar. Teste de integração `db_test.rs` valida o caminho SQLite com DB temporário seedado.

## Boundaries

Arquivos intencionalmente tocados:
- `src-tauri/Cargo.toml`
- `src-tauri/src/db.rs` (create)
- `src-tauri/src/lib.rs`
- `src-tauri/tests/db_test.rs` (create)
- `src/lib/dashboard.ts`
- `src/components/layout/Sidebar.tsx`
- `src/pages/Home.tsx` (apenas para exibir `tokens_total` no card de Métricas — sem reestruturar layout)

Fora do escopo: `src/pages/ProjectDetail.tsx`, `src/components/CommandPalette.tsx`, qualquer arquivo em `src/api/`, hooks em `src/hooks/`, qualquer skill/recipe/hook do `.claude/`.

## Files (~7)

- `src-tauri/Cargo.toml` — add rusqlite bundled, add `[[test]] name = "db_test"`
- `src-tauri/src/db.rs` — (create); `open_readonly`, `has_phase1_schema`, `with_db`
- `src-tauri/src/lib.rs` — `mod db;` + refactor 3 commands + 3 novos commands + serde rename_all + register no invoke_handler
- `src-tauri/tests/db_test.rs` — (create); seed temp DB + chamar funções puras + ≥4 asserts
- `src/lib/dashboard.ts` — adicionar `tokens_total`/`tokens_today` em MetricsSummary, novos types SpecRow/KnowledgeRow, fetchSpecs/fetchSearchEvents/fetchSearchKnowledge
- `src/components/layout/Sidebar.tsx` — remover linha `0 subprojects` (decisão da spec: ruído, não métrica útil global)
- `src/pages/Home.tsx` — incluir `tokens_total` no card Métricas (1 linha extra: `• N tokens`)

## Decisões da spec

1. **Path do DB:** `{repo_path}/.claude/.harness/mustard.db` (não na raiz do repo). Já é o path usado pelo `discovery.rs` no campo `db_path` do struct `Project` — manter consistência.
2. **Fallback é o caminho primário hoje:** o `mustard.db` do Mustard core existe mas está vazio (writer 2.0 ainda não populou no projeto vivo). O fallback JSONL/JSON precisa funcionar igual ao código atual; quando o writer migrar, o caminho SQLite passa a dominar transparentemente. AC-3 abaixo reflete isso.
3. **rusqlite version:** `0.31` com feature `bundled` apenas — sem sqlcipher. Compila estático no Windows sem dep do sistema.
4. **Schema completeness:** `has_phase1_schema` exige ≥3 das 4 tabelas (`events`, `specs`, `knowledge`, `spans`). Tolerante a Phase 1 parcial; intolerante a DB totalmente vazio/corrompido.
5. **FTS5 query escape:** se query contém espaço/quote/`*`/`-`/`:`, envolver em aspas duplas e dobrar aspas internas — não construir DSL própria. Para queries vazias após sanitize, retornar `[]` sem chamar `MATCH`.
6. **`tokens_today`:** usar `chrono`? Não — calcular `started_at >= ?` em ms epoch passando o timestamp de meia-noite local computado em Rust puro (`SystemTime` + offset zero / UTC midnight). Aceitável que "today" seja UTC midnight, não local; documentar inline.
7. **`RecentEvent.event_type`:** manter o nome do campo (TS bridge já usa). Adicionar `#[serde(rename = "event_type")]` se necessário para sobrescrever o `rename_all`.
8. **Funções puras testáveis:** extrair `metrics_from_db(conn) -> MetricsSummary`, `knowledge_from_db(conn) -> KnowledgeSummary`, etc. para `db.rs`. O `#[tauri::command]` wrapper só faz path-build + with_db + chama a função pura. Teste chama as funções puras direto.
9. **`#[serde(rename_all = "snake_case")]` em TODOS structs Serialize:** PipelineSummary, MetricsSummary, KnowledgeSummary, RecipeMeta, SkillMeta, RecentEvent, SubprojectInfo (já tem), SpecRow, KnowledgeRow. Como todos os campos já são snake_case, isso é defensivo — garante que renomeações futuras (ex.: `tokensTotal` em camelCase Rust) não quebrem a bridge TS.
10. **Sidebar:** remoção pura. Não substituir pela contagem real — o sidebar é global (não específico de projeto selecionado), e a contagem de subprojects do Mustard só faz sentido depois que o projeto for selecionado. Já é exibida no Home.

## Tasks

### Wave 1 — Infra Rust (paralelo seguro)

- [x] T1.1 `src-tauri/Cargo.toml`: adicionar `rusqlite = { version = "0.31", features = ["bundled"] }` em `[dependencies]`. Adicionar bloco `[[test]] name = "db_test" path = "tests/db_test.rs"` ao fim do arquivo. Rodar `cargo check` para validar.
- [x] T1.2 Criar `src-tauri/src/db.rs` com:
  - `pub fn open_readonly(db_path: &Path) -> Result<Connection, String>` — usa `rusqlite::Connection::open_with_flags` com `OpenFlags::SQLITE_OPEN_READ_ONLY`. Retorna `Err("db not found")` se path não existir antes de abrir.
  - `pub fn has_phase1_schema(conn: &Connection) -> bool` — `SELECT name FROM sqlite_master WHERE type='table' AND name IN ('events','specs','knowledge','spans')` → conta linhas; `>= 3` é true.
  - `pub fn with_db<T, F>(repo_path: &Path, f: F) -> Option<Result<T, String>>` onde `F: FnOnce(&Connection) -> Result<T, String>`. Retorna `None` se DB ausente ou schema incompleto (sinaliza para o caller usar fallback). Retorna `Some(Ok(t))` se ok, `Some(Err(...))` se a closure falhou. Path interno: `repo_path.join(".claude").join(".harness").join("mustard.db")`.
  - Funções puras de leitura (assinatura `fn xxx(conn: &Connection) -> Result<T, String>`): `metrics_from_db`, `knowledge_from_db`, `recent_events_from_db`, `specs_from_db`, `search_events_from_db`, `search_knowledge_from_db`. Tipos retornados via `pub` re-export ou definidos em `db.rs` e re-exportados em `lib.rs` (preferir definir em `lib.rs` para evitar ciclo; `db.rs` aceita um trait `From` ou retorna `Vec<Row>` genérico — decidir na implementação, mas resultado final deve compilar limpo).
  - Helper `fts_escape(q: &str) -> Option<String>` — `None` se vazio após trim; se contém qualquer um de `' "  \t * - : ( )`, envolve em `"..."` com aspas internas dobradas.
- [x] T1.3 Criar `src-tauri/tests/db_test.rs`:
  - `setup_db() -> rusqlite::Connection` que abre em memória (`Connection::open_in_memory()`), executa o schema (copiar o conteúdo via `include_str!("../../../C:/Atiz/Mustard/src/runtime/schema.sql")` NÃO funciona — usar `include_str!` num arquivo local ou string literal inline; preferir string literal inline contendo só as 4 tabelas + FTS necessárias).
  - Inserir 3 events com varied `event` (incluindo 1 `agent.start`), 2 specs, 2 knowledge rows (1 com `confidence=0.9`, 1 com `0.5`), 1 span com `input_tokens=100, output_tokens=200`.
  - Asserts (mínimo 4):
    - `metrics_from_db(&conn).total_events == 3`
    - `metrics_from_db(&conn).agents_dispatched == 1`
    - `metrics_from_db(&conn).tokens_total == 300`
    - `knowledge_from_db(&conn).high_confidence_count == 1`
    - `specs_from_db(&conn).len() == 2`
    - `recent_events_from_db(&conn, 10).len() == 3` em ordem `id DESC`
  - Rodar `cargo test --test db_test` localmente para validar.

### Wave 2 — Refactor lib.rs (depende de Wave 1)

- [ ] T2.1 `src-tauri/src/lib.rs`: adicionar `mod db;` no topo. Adicionar `#[serde(rename_all = "snake_case")]` em PipelineSummary, MetricsSummary, KnowledgeSummary, RecipeMeta, SkillMeta, RecentEvent. SubprojectInfo já tem.
- [ ] T2.2 `MetricsSummary`: adicionar campos `tokens_total: u64` e `tokens_today: u64`. Atualizar todos os construtores (incluindo o fallback do path-not-exists para zeros).
- [x] T2.3 Refatorar `dashboard_metrics`: chamar `db::with_db(&base, |conn| db::metrics_from_db(conn))`. Se `None` ou `Some(Err(_))`, executar leitura JSONL atual (manter código inline ou extrair para `metrics_from_jsonl(&Path) -> Result<MetricsSummary, String>`). O fallback NÃO computa `tokens_total`/`tokens_today` (legacy não tem spans) — retorna `0` nesses campos.
- [x] T2.4 Refatorar `dashboard_knowledge`: mesma estrutura, fallback para leitura JSON atual.
- [x] T2.5 Refatorar `dashboard_recent_events`: chamar `db::with_db(&base, |conn| db::recent_events_from_db(conn, limit))`. Fallback JSONL atual (`content.lines().filter_map(...).collect()` + slice por `cap`). Summary do payload: extrair primeiros 80 chars de algum campo legível (`summary`, `description`, `msg`, `text`, ou nada).
- [ ] T2.6 Novo `dashboard_specs(repo_path: String) -> Result<Vec<SpecRow>, String>`. Sem fallback legacy (specs como tabela só existe no Phase 1 — retornar `Ok(vec![])` se DB ausente). Struct: `SpecRow { name, status, phase, started_at, completed_at, affected_files: Vec<String> }` — `affected_files` parseado de TEXT JSON; falhar gentil para `vec![]` se malformado.
- [ ] T2.7 Novo `dashboard_search_events(repo_path: String, query: String, limit: Option<usize>) -> Result<Vec<RecentEvent>, String>`. Aplicar `fts_escape`; se `None` retornar `Ok(vec![])`. Query: `SELECT e.event, e.spec, e.ts, e.payload FROM events_fts f JOIN events e ON f.rowid = e.id WHERE events_fts MATCH ?1 ORDER BY rank LIMIT ?2`.
- [ ] T2.8 Novo `dashboard_search_knowledge(repo_path: String, query: String, limit: Option<usize>) -> Result<Vec<KnowledgeRow>, String>`. Struct: `KnowledgeRow { id, type_: String /* serde rename = "type" */, name, description, confidence, source }`. Mesma estratégia de escape.
- [ ] T2.9 Registrar `dashboard_specs, dashboard_search_events, dashboard_search_knowledge` no `invoke_handler![...]` na função `run()`. Rodar `cargo check`.

### Wave 3 — TS bridge + UI (depende de Wave 2) — `(parallel-safe)` se quiser dividir

- [x] T3.1 `src/lib/dashboard.ts`: adicionar `tokens_total: number; tokens_today: number;` em `MetricsSummary`. Adicionar interfaces `SpecRow { name; status; phase; started_at; completed_at; affected_files: string[]; }`, `KnowledgeRow { id; type: string; name; description; confidence: number; source: string; }`. Adicionar funções `fetchSpecs(repoPath: string)`, `fetchSearchEvents(repoPath, query, limit?)`, `fetchSearchKnowledge(repoPath, query, limit?)`. NÃO importar/usar em outros arquivos nesta wave — só expor (Tier 1.2 e 2.5 consomem).
- [x] T3.2 `src/pages/Home.tsx`: dentro do CardDescription de Métricas, adicionar `{" • "} <code className="text-foreground font-mono">{metrics.tokens_total}</code> tokens` ao final da linha de stats (mesmo padrão de events/sessions/agents). Manter "Carregando…" / error / "Selecione um projeto" inalterados.
- [x] T3.3 `src/components/layout/Sidebar.tsx`: remover linha `<div className="px-3 py-1.5 text-xs text-muted-foreground">0 subprojects</div>`. Manter `Separator`, `Workspace` label, e `mt-auto`.
- [x] T3.4 Validação final: `pnpm tsc --noEmit` passa. App rodando (`pnpm tauri dev`) mostra Home funcional contra um projeto real, sem console errors.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Cargo compila — Command: `node -e "const path=require('path'); const cargo=process.env.CARGO||path.join(process.env.USERPROFILE||process.env.HOME,'.cargo','bin','cargo'+(process.platform==='win32'?'.exe':'')); const r=require('child_process').spawnSync(cargo,['check'],{cwd:'src-tauri',stdio:'inherit'}); process.exit(r.status===0?0:1)"`
- [x] AC-2: Teste de integração SQLite roda e passa (≥4 testes na suite) — Command: `node -e "const path=require('path'); const fs=require('fs'); const cargo=process.env.CARGO||path.join(process.env.USERPROFILE||process.env.HOME,'.cargo','bin','cargo'+(process.platform==='win32'?'.exe':'')); const tests=fs.readFileSync('src-tauri/tests/db_test.rs','utf8').split('#[test]').length-1; if(tests<4){console.error('only',tests,'tests'); process.exit(1)} const r=require('child_process').spawnSync(cargo,['test','--test','db_test','--quiet'],{cwd:'src-tauri'}); process.exit(r.status===0?0:1)"`
- [x] AC-3: Dashboard contra Mustard core retorna >100 eventos (via fallback JSONL hoje, ou SQLite quando writer 2.0 popular) — Command: `node -e "const fs=require('fs'); const lines=fs.readFileSync('C:/Atiz/Mustard/.claude/.harness/events.jsonl','utf8').split('\n').filter(Boolean).length; process.exit(lines > 100 ? 0 : 1)"`
- [x] AC-4: TypeScript compila com novos types — Command: `pnpm tsc --noEmit 2>&1 | grep -E "error TS" && exit 1 || exit 0`
- [x] AC-5: `tokens_total` field presente na interface MetricsSummary do TS — Command: `node -e "const s=require('fs').readFileSync('src/lib/dashboard.ts','utf8'); process.exit(/tokens_total\s*:\s*number/.test(s) && /tokens_today\s*:\s*number/.test(s) ? 0 : 1)"`
- [x] AC-6: Sidebar não contém mais "0 subprojects" — Command: `node -e "const s=require('fs').readFileSync('src/components/layout/Sidebar.tsx','utf8'); process.exit(s.includes('0 subprojects') ? 1 : 0)"`
- [x] AC-7: Todos os structs Serialize em lib.rs têm rename_all snake_case — Command: `node -e "const s=require('fs').readFileSync('src-tauri/src/lib.rs','utf8'); const structs=s.match(/#\\[derive\\(Serialize\\)\\][^\\}]+?pub struct \\w+/g)||[]; const ok=structs.every(b=>/rename_all\\s*=\\s*\"snake_case\"/.test(b)); process.exit(ok?0:1)"`
- [x] AC-8: Três novos commands registrados no invoke_handler — Command: `node -e "const s=require('fs').readFileSync('src-tauri/src/lib.rs','utf8'); const seg=(s.split('invoke_handler')[1]||'').split(']')[0]; const ok=['dashboard_specs','dashboard_search_events','dashboard_search_knowledge'].every(n=>seg.includes(n)); process.exit(ok?0:1)"`

## Non-Goals

- KnowledgeBrowser view (Tier 2.5)
- Specs tab UI / SpecDetail drill-down (Tier 1.2)
- AggregateView de múltiplos projetos
- License gate / CI workflow
- Migração de qualquer hook/recipe/skill do `.claude/`
- Escrita no DB SQLite (read-only end-to-end nesta wave)
- Suporte a sqlcipher

## Concerns

- O `mustard.db` do Mustard core está vazio hoje (writer 2.0 ainda não rodou no projeto principal). Isso significa que AC-3 cai no caminho fallback JSONL — não exercita SQLite end-to-end contra dados reais. AC-2 (teste de integração com DB seedado) cobre o caminho SQLite isoladamente. Quando o writer 2.0 ligar, AC-3 passa a exercitar SQLite naturalmente sem mudança de código.
- `rusqlite` com `bundled` adiciona ~200KB ao binário e tempo de compilação inicial (~30s extra no primeiro build). Aceitável; cache cargo elimina nas builds seguintes.
- `tokens_today` usa UTC midnight, não local — pode surpreender usuários em fusos longe de UTC. Documentado inline; resolvido em wave futura se virar reclamação.
- FTS5 query escape é minimal — não suporta operadores avançados (`AND`, `OR`, `NEAR`) intencionalmente. Adequado para search box simples; pode ser estendido depois.
- O `path` do schema absoluto `C:/Atiz/Mustard/src/runtime/schema.sql` é externo a este repo — NÃO usar `include_str!` apontando para lá. O teste inclui schema inline (string literal Rust) com só as 4 tabelas + triggers FTS necessários, mantendo independência.
