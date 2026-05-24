# Onda 2 — Banners do `mustard-rt` E outputs do `mustard-cli`

## Resumo

Esta onda atualiza todo o texto que o Mustard manda pro terminal — tanto do lado do `mustard-rt` (hooks + subcomandos + MCP + report + dispatch) quanto do `mustard-cli` (init, update, add, review, git_flow, install_nerd_font, config). Cada `println!`/`eprintln!` que fala com humano passa por uma camada de tradução que respeita `lang` e `tone`. Os hooks e comandos não escrevem mais strings literais — pedem por chave (`tr!("bash_guard.rm_blocked")`) e a camada devolve o texto certo, no idioma certo, com o tom certo. A camada vem do `mustard-core` (Onda 1), então ambos os crates importam do mesmo lugar.

## O que muda neste passo

1. **Tabela de mensagens em `mustard-core`.** Estende o módulo `i18n.rs` (criado na Onda 1) com uma tabela `(MessageKey, Lang) -> &'static str` cobrindo todas as chaves de banner. Macro `tr!(key, params...)` pra uso ergonômico nos call sites.

2. **Cobertura no `apps/rt/`:**
   - `src/hooks/**/*.rs` — `bash_guard`, `path_guard`, `close_gate`, `model_routing`, `tracker`, `budget`, `enforce_registry`, `size_gate`, `post_edit`, `knowledge`, `session_start`, `session_cleanup`, `pre_compact`, `prompt_gate`.
   - `src/run/**/*.rs` — `doctor`, `active_specs`, `event_projections`, `sync_detect`, `sync_registry`, `migrate_spec_headers`, `verify_pipeline`, `security_scan`, `recipe_match`, `analyze_validation`, `wave_scaffold`, `qa_run`, e demais.
   - `src/mcp/**/*.rs`, `src/report/**/*.rs`, `src/dispatch.rs` — outputs visíveis ao usuário (erros e infos).

3. **Cobertura no `apps/cli/`:**
   - `src/commands/init.rs` (27 ocorrências), `update.rs` (9), `install_nerd_font.rs` (15), `add.rs` (13), `git_flow.rs` (9), `review.rs` (8), `config.rs` (1). Toda mensagem de "instalado", "atualizado", "erro", "sucesso" passa por `tr!`.

4. **Prefixos de log ficam.** `[boundary-gate]`, `[SPEC-SIZE]`, `[HYGIENE]`, etc. são preservados — servem pra grep/debug. Só o texto humano depois deles é traduzido.

5. **Mensagens de erro de stack interno não vão mais cruas pro `eprintln!`.** Erros como `TelemetryStore::for_project failed` viram chaves com versão amigável. Em pt-BR didático: "Não consegui ler os dados de telemetria. Tente rodar `mustard-rt run doctor`."

6. **Modo `caveman`.** Aplicado via `apply_tone(text, Tone::Caveman, preserve_structured=true)` da Onda 1. Frases viram fragmentos, artigos somem, pleasantries somem. Mas headings, paths, comandos em backtick, AC IDs ficam intactos.

## Arquivos

- `packages/core/src/i18n.rs` — estende com a tabela `MESSAGES` e a macro `tr!`.
- `apps/rt/src/hooks/**/*.rs` — substituir `println!`/`eprintln!` user-facing por `tr!`.
- `apps/rt/src/run/**/*.rs` — idem.
- `apps/rt/src/mcp/**/*.rs`, `src/report/**/*.rs`, `src/dispatch.rs` — idem.
- `apps/cli/src/commands/**/*.rs` — idem.

## Tarefas

### General Agent (Wave 2)

- [ ] Estender `packages/core/src/i18n.rs` com:
  - Enum `MessageKey` listando todas as chaves cobertas (organizadas por domínio: `bash_guard.*`, `path_guard.*`, `close_gate.*`, `cli.init.*`, `cli.update.*`, etc.).
  - Tabela `MESSAGES: phf::Map<(MessageKey, Lang), &'static str>` (ou `HashMap` se phf for pesado).
  - Função `tr(key, lang, tone, params) -> String` que faz lookup + substitui placeholders + aplica `apply_tone(text, tone, true)`.
  - Macro `tr!(key, params...)` pra uso ergonômico.
- [ ] Preencher tabela com traduções em pt-BR e en-US pra cada chave.
- [ ] No `apps/rt/src/hooks/**/*.rs`, percorrer cada arquivo e substituir `println!`/`eprintln!` voltado ao usuário por `tr!`. Erros internos de stack viram chaves com versão amigável.
- [ ] No `apps/rt/src/run/**/*.rs`, idem.
- [ ] No `apps/rt/src/{mcp,report,dispatch}.rs` (e diretórios), idem.
- [ ] No `apps/cli/src/commands/**/*.rs`, idem. Atenção especial em `init.rs` (27 ocorrências) e `install_nerd_font.rs` (15) — os mais densos.
- [ ] Atualizar testes de snapshot afetados (alguns testes comparam string literal — atualizar os snapshots, não desabilitar).
- [ ] `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`.
- [ ] AC-3 e AC-4 do wave-plan passam.

## Dependências

Depende da Onda 1 (precisa de `mustard-core::i18n` existir).

Também depende da spec B (`2026-05-24-meta-sidecar`) — herda terreno simplificado do parser.

## Limites

Esta onda **só** mexe nos arquivos `.rs` listados. Não toca no dashboard, não muda templates de skill (esses dois ficam pra Onda 3).

## Preocupações

- **Performance.** Cada chamada de `tr!` carrega custo baixo. Pra banners (saída raríssima) é irrelevante. Se algum hot path passar por `tr!` (não deveria), o resultado é cacheável.
- **Testes de snapshot.** Mudar a saída visível quebra eventuais testes que comparam string literal. Atualizar os snapshots — não desabilitar testes.
- **CLI emite muita saída.** Os 82 `println!`/`eprintln!` do CLI são um volume considerável. Recomendo organizar por comando: refatorar um comando inteiro de uma vez (init, depois update, etc.) — diff fica mais legível.
