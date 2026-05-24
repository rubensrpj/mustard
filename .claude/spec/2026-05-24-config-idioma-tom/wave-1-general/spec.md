# Onda 1 — Fundação: config no `mustard.json` + i18n em `mustard-core`

## Resumo

Esta onda monta o alicerce. No final dela: o `mustard.json` aceita os campos `lang` e `tone`; o pacote compartilhado `mustard-core` ganha um módulo `i18n` que lê esses campos, aplica transformação de tom em texto, gera slug de spec respeitando idioma, e migra o `specLang` antigo. Nenhum banner muda ainda — isso é Onda 2. Nenhuma UI muda — isso é Onda 3. Aqui só acontece o que precisa estar pronto pras próximas ondas funcionarem.

## O que muda neste passo

1. **Template do `mustard.json` ganha dois campos novos.** No `apps/cli/templates/mustard.json`, junto com `git`, passam a existir `lang: "pt-BR"` e `tone: "didactic"` como padrão.

2. **`mustard init` escreve os defaults sem perguntar.** Quando alguém roda `mustard init` num projeto novo, o arquivo já sai com os dois campos preenchidos. Sem prompt, sem `AskUserQuestion`.

3. **`mustard update` preserva os valores existentes.** Se o projeto já tem `lang: "en-US"`, `update` não sobrescreve. Usa `fs_ops::merge_json`.

4. **Novo módulo `packages/core/src/i18n.rs`.** Localizado no pacote compartilhado pra que tanto `mustard-cli` quanto `mustard-rt` consumam do mesmo lugar. Expõe:
   - Enums `Lang { PtBr, EnUs }` e `Tone { Didactic, Technical, Caveman }`.
   - Função `lang() -> Lang` e `tone() -> Tone` que lêem o `mustard.json` do projeto ativo (cache em memória).
   - Função `apply_tone(text: &str, tone: Tone, preserve_structured: bool) -> String`. Quando `preserve_structured = true`, blocos de código, headings, paths, comandos e qualquer literal entre backticks ou em estruturas reconhecíveis (linhas começando com `### `, `- [ ] AC-N`, etc.) passam intactos pela transformação.
   - Função `slugify(title: &str, lang: Lang) -> String`. Gera slug kebab-case a partir do título. Em `pt-BR`, normaliza acentos (ã→a, ç→c, etc.). Em `en-US`, comportamento atual de slug.
   - Função `migrate_spec_lang_if_present(mustard_json_path: &Path) -> Result<()>`. Se o JSON tem `specLang: "pt"` ou `"en"`, reescreve como `lang: "pt-BR"`/`"en-US"`, remove `specLang`. Atômico, idempotente.

5. **Hook `session_start` injeta os valores no contexto do orquestrador.** O orquestrador recebe uma linha extra no `additionalContext`: algo como `Idioma do projeto: pt-BR. Tom: didático.`

6. **Gerador de slug consumido pelos criadores de spec.** Onde `wave-scaffold`/`emit-pipeline`/`tactical-fix` criam pastas de spec, passam a chamar `i18n::slugify(title, lang)` em vez de o slug ser ad-hoc.

## Arquivos

- `apps/cli/templates/mustard.json` — defaults `lang` + `tone`.
- `apps/cli/src/commands/init.rs` — garante os dois campos.
- `apps/cli/src/commands/update.rs` — preserva via `merge_json`.
- `packages/core/src/i18n.rs` (novo) — schema + leitor + tom + slug + migração.
- `packages/core/src/lib.rs` — registra `mod i18n;`.
- `apps/rt/src/hooks/session_start.rs` — injeta `lang`+`tone`.
- `apps/rt/src/run/wave_scaffold.rs` — usa `slugify`.
- `apps/rt/src/run/spec_slug.rs` (novo, fino) — façade ergonômica em cima de `i18n::slugify` pra uso dos comandos.

## Tarefas

### General Agent (Wave 1)

- [ ] Adicionar `lang: "pt-BR"` e `tone: "didactic"` ao `apps/cli/templates/mustard.json`.
- [ ] Em `init.rs`, garantir os dois campos no arquivo recém-criado.
- [ ] Em `update.rs`, preservar valores existentes via `fs_ops::merge_json`.
- [ ] Criar `packages/core/src/i18n.rs` com:
  - Enums `Lang` e `Tone` (defaults `PtBr` / `Didactic`).
  - Funções `lang()` e `tone()` com cache (`OnceLock`) por path de projeto.
  - Função `apply_tone(text, tone, preserve_structured)`. Implementar transformação `caveman`: remover artigos comuns (a/o/uma/um/the/a/an), conectivos (que/then/porque/because), pleasantries (claro/sure/certainly), deixar fragmentos com `->`. Quando `preserve_structured=true`, não transforma linhas que casam com `^### |^## |^# |^- \[[ x]\] AC-|^```` `.
  - Função `slugify(title, lang)` — kebab-case, normaliza acentos em `pt-BR`, comportamento atual em `en-US`.
  - Função `migrate_spec_lang_if_present(path)` — reescreve `specLang` como `lang` no formato BCP 47.
- [ ] Registrar `mod i18n;` em `packages/core/src/lib.rs`.
- [ ] Em `apps/rt/src/hooks/session_start.rs`, ler `lang`+`tone` via `i18n::*` e injetar uma linha no contexto.
- [ ] Em `apps/rt/src/run/wave_scaffold.rs`, usar `i18n::slugify(title, lang)` quando o slug é derivado de título.
- [ ] Criar `apps/rt/src/run/spec_slug.rs` (façade) se outros lugares precisarem de uma API mais curta.
- [ ] `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test -p mustard-core`, `cargo test -p mustard-rt`.
- [ ] AC-1, AC-2, AC-7, AC-9 e AC-10 do wave-plan passam.

## Dependências

Depende da spec B (`2026-05-24-meta-sidecar`) estar concluída. Antes de começar esta onda, confirmar que `meta-sidecar` está em `Outcome: Completed`.

## Limites

Esta onda **só** mexe nos arquivos listados. Em particular:

- Não muda nenhum banner de hook (Onda 2).
- Não muda nenhum output de subcomando (Onda 2).
- Não muda nenhum comando do CLI (Onda 2).
- Não muda o dashboard (Onda 3).
- Não traduz nada do código-fonte ou comentários.
