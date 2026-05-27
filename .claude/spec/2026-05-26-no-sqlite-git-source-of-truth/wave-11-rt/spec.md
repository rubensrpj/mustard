# Wikilink footer hook (auto-rodapé clicável)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 3E (renumbered wave-11-rt). **Wikilink footer hook (auto-rodapé clicável).** CREATE `apps/rt/src/hooks/wikilink_footer.rs` — PostToolUse, casa `Write|Edit` em `.claude/{memory,knowledge,spec}/**/*.md`. Lê arquivo, chama `mustard_core::atomic_md::wikilink::render_footer(body, &[memory_dir, knowledge_dir, spec_dir])`, substitui bloco entre sentinelas `
` (cria se ausente, remove se body sem `[[]]`). Idempotente: 2ª chamada com mesmo body é no-op (sentinelas estáveis). Toda lógica vive em W1C; hook é só "quando rodar" (SRP). MODIFY `apps/rt/src/hooks/mod.rs` (+1 linha `pub mod wikilink_footer;`); MODIFY `apps/rt/src/registry.rs` (+1 entrada em `Registry::new()` conforme contrato apps/rt/CLAUDE.md — nunca tocar dispatcher). CREATE `apps/rt/tests/wikilink_footer_hook.rs` (integration: tmpdir com `[[memory_X]]` + `[[orphan]]`, dispara hook, valida footer presente, link resolvido, órfão marcado, regen idempotente).

**Files (4):** `apps/rt/src/hooks/wikilink_footer.rs` (CREATE), `apps/rt/src/hooks/mod.rs` (MODIFY +1 linha), `apps/rt/src/registry.rs` (MODIFY +1 entrada), `apps/rt/tests/wikilink_footer_hook.rs` (CREATE).

**Verify:** `cargo test -p mustard-rt --test wikilink_footer_hook`.

**Independente das outras W3** — depende só de W1C (já comitada em `dev_rubens`: `376fca5`).

## Critérios de Aceitação

- [x] AC-3E-1: `cargo test -p mustard-rt --test wikilink_footer_hook` passa com 4 casos: link resolvido, órfão marcado, body sem `[[]]` → footer removido, regen idempotente. Command: `cargo test -p mustard-rt --test wikilink_footer_hook`
- [x] AC-3E-2: `cargo build -p mustard-rt` passa. Command: `cargo build -p mustard-rt`
- [x] AC-3E-3: A lógica de render do footer vive em `mustard_core::atomic_md::wikilink::render_footer` (W1C) — o hook NÃO duplica parser/regex. Command: `bash -c "! grep -nE 'fn render_footer|Regex::new.*\\\\[\\\\[' apps/rt/src/hooks/wikilink_footer.rs"`

## Plano

## Arquivos

- `apps/rt/src/hooks/wikilink_footer.rs`
- `apps/rt/src/hooks/mod.rs`
- `apps/rt/src/registry.rs`
- `apps/rt/tests/wikilink_footer_hook.rs`

## Tarefas

1. `apps/rt/src/hooks/wikilink_footer.rs` (CREATE) — hook PostToolUse com matcher `Write|Edit`. Body: detecta paths `.claude/(memory|knowledge|spec)/**/*.md`. Lê o arquivo do disco, chama `mustard_core::atomic_md::wikilink::render_footer(body, &[memory_dir, knowledge_dir, spec_dir])`, substitui bloco entre sentinelas `
` (cria se ausente, remove se body sem `[[]]`). Idempotente. Outcome = `Outcome::Allow` (não bloqueia tool, só age post-edit como rewrite atomic).
2. `apps/rt/src/hooks/mod.rs` (MODIFY) — adicionar `pub mod wikilink_footer;` (+1 linha exata)
3. `apps/rt/src/registry.rs` (MODIFY) — adicionar entrada em `Registry::new()` conforme contrato `apps/rt/CLAUDE.md` (matcher PostToolUse `Write|Edit`, módulo `wikilink_footer`); nunca tocar dispatcher
4. `apps/rt/tests/wikilink_footer_hook.rs` (CREATE) — integration: cria tmpdir com fixture `.claude/memory/foo.md` referenciando `[[bar]]` (resolve) + `[[orphan]]` (não resolve); dispara hook via `mustard-rt on PostToolUse` simulando Write; valida (a) footer presente, (b) `[bar](path)` clicável, (c) `⚠ não resolvido` para órfão, (d) re-dispara hook com mesmo body → diff vazio (idempotente), (e) edita body removendo todos `[[]]` → footer some

## Dependências

Depende **só de W1C** (já comitada: `376fca5`). Pode rodar em paralelo com W3A-D. Possível conflito de merge com W3D (que também toca `hooks/mod.rs` e `registry.rs`); orquestrador rebaseia se conflitar.

## Limites

- CAP RÍGIDO: ≤5 arquivos (4 nesta sub-spec)
- Hook é SRP: "quando rodar" — toda lógica de render mora em `mustard_core::atomic_md::wikilink`
- Sentinelas estáveis garantem idempotência; teste cobre regen
- Outcome do hook é `Allow` (não bloqueia tool); é write-after-write atomic
- Commit message sugerido: `feat(wave-3/rt): W3E — wikilink_footer hook auto-renders clickable footers`

<!-- wikilinks-footer-start -->
- [2026-05-26-no-sqlite-git-source-of-truth](?) ⚠ não resolvido
- [memory_X](?) ⚠ não resolvido
- [orphan](?) ⚠ não resolvido
- [bar](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->

<!-- wikilinks-footer-start -->
- [2026-05-26-no-sqlite-git-source-of-truth](?) ⚠ não resolvido
- [memory_X](?) ⚠ não resolvido
- [orphan](?) ⚠ não resolvido
- [bar](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->