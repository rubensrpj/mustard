# MarkdownStore primitivo — compartilhado por memory/knowledge/spec readers e footer hook

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-27T09:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 1C. **MarkdownStore primitivo (compartilhado por memory/knowledge/spec readers + footer hook + Obsidian backlinks).** CREATE `packages/core/src/atomic_md/{mod,store,frontmatter,wikilink}.rs`. `MarkdownStore` é struct concreta: `scan_dir(dir) -> Vec<MarkdownDoc>` eager YAML frontmatter + lazy body (rayon par_iter se >50 arquivos); `read_one(path) -> MarkdownDoc`; `write_atomic(path, doc)` via tmpfile + rename (durabilidade). **Módulo `wikilink` (pure functions, regex única `\[\[([^\]]+)\]\]`):** `find_outgoing_links(body) -> Vec<String>` extrai todos `[[name]]` do corpo; `find_backlinks(target, docs) -> Vec<PathBuf>` casa target contra docs scaneados; `resolve(name, search_dirs) -> Option<PathBuf>` busca name nos dirs canônicos (memory/, knowledge/, spec/), retorna path relativo; `render_footer(body, search_dirs) -> String` gera bloco `<!-- wikilinks-footer-start -->...<!-- wikilinks-footer-end -->` com `- [name](relative/path.md)` por link + `⚠ não resolvido` para órfãos. **Por que aqui:** mesma regex serve outgoing/backlinks/resolve (DRY); hook footer (W3D) consome `render_footer` sem reimplementar parser. Benchmarks: scan 200 arquivos <100ms cold + <5ms cached; find_backlinks <30ms; render_footer <2ms por arquivo.

## Critérios de Aceitação

- [x] AC-1C-1: benchmarks de scan, find_backlinks e render_footer passam nos thresholds + casos: link resolvido, link órfão, body sem links (footer vazio), regen idempotente. Command: `cargo test -p mustard-core atomic_md::store::bench atomic_md::wikilink::bench atomic_md::wikilink::test_render_footer`

## Plano

## Arquivos

- `packages/core/src/lib.rs` (export)
- `packages/core/src/atomic_md/mod.rs`
- `packages/core/src/atomic_md/store.rs`
- `packages/core/src/atomic_md/frontmatter.rs`
- `packages/core/src/atomic_md/wikilink.rs`

## Tarefas

1. `packages/core/src/atomic_md/frontmatter.rs` — CREATE: `pub struct Frontmatter(pub serde_yaml::Value)` (ou `indexmap::IndexMap` se serde_yaml pesado); `pub fn parse(text: &str) -> (Option<Frontmatter>, &str)` extrai bloco `---\n...\n---` do início, retorna `(frontmatter, corpo_restante)`; lenient: YAML inválido retorna `None` sem panic
2. `packages/core/src/atomic_md/store.rs` — CREATE: `pub struct MarkdownDoc { pub path: PathBuf, pub frontmatter: Option<Frontmatter>, pub body: String }`; `pub struct MarkdownStore`; impl `scan_dir(dir: &Path) -> Vec<MarkdownDoc>` — glob `*.md`, eager parse frontmatter, body lazy (String vazio até `read_one` explícito); usa `rayon::par_iter` se `count > 50`; impl `read_one(path: &Path) -> MarkdownDoc` lê body completo; impl `write_atomic(path: &Path, doc: &MarkdownDoc) -> Result<()>` — escreve em `path.with_extension("tmp")` depois `fs::rename` (atomic); bloco `#[cfg(test)]` com benchmark: tmpdir com 200 arquivos `.md`, mede `scan_dir` p95 <100ms cold e <5ms 2ª chamada (cache de `Vec` retornado)
3. `packages/core/src/atomic_md/wikilink.rs` — CREATE: regex única `lazy_static` / `once_cell` `\[\[([^\]]+)\]\]`; `pub fn find_outgoing_links(body: &str) -> Vec<String>`; `pub fn find_backlinks(target: &str, docs: &[MarkdownDoc]) -> Vec<PathBuf>` — itera docs, checa `find_outgoing_links`; `pub fn resolve(name: &str, search_dirs: &[&Path]) -> Option<PathBuf>` — para cada dir tenta `{dir}/{name}.md` e `{dir}/**/{name}.md` (glob simples); `pub fn render_footer(body: &str, search_dirs: &[&Path]) -> String` — extrai links, resolve cada um, gera bloco HTML-comment sentinelas `<!-- wikilinks-footer-start -->` / `<!-- wikilinks-footer-end -->`; idempotente: substitui bloco existente, remove se sem links; bloco `#[cfg(test)]` com: link resolvido, link órfão com `⚠ não resolvido`, body sem `[[]]` retorna body inalterado, chamar `render_footer` duas vezes no mesmo body produz resultado idêntico (idempotência), `find_backlinks` <30ms em 200 docs, `render_footer` <2ms
4. `packages/core/src/atomic_md/mod.rs` — CREATE: `pub mod store; pub mod frontmatter; pub mod wikilink; pub use store::{MarkdownStore, MarkdownDoc}; pub use frontmatter::Frontmatter; pub use wikilink::{find_outgoing_links, find_backlinks, resolve, render_footer};`
5. `packages/core/src/lib.rs` — adicionar `pub mod atomic_md;` e re-exports `pub use atomic_md::{MarkdownStore, MarkdownDoc};`

## Dependências

(nenhuma — W1C não depende de outras sub-specs)

## Limites

- CAP RÍGIDO: ≤5 arquivos (já satisfeito por construção)
- Sem stubs preservando nomes SQLite
- Após commit: `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite" -- 'packages/**/*.rs' 'apps/**/*.rs'` count DEVE decrescer (ou ficar igual se sub-spec não toca esses arquivos — caso W1C que CRIA primitivos novos)
- Benchmarks de performance no AC são binários — passa ou falha
