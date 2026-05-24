# W4 — Language and tone (absorve `2026-05-24-config-idioma-tom`)

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: full
### Checkpoint: 2026-05-24T19:30:00Z
### Lang: pt-BR
### Parent: 2026-05-24-mustard-unification

## Contexto

Absorve integralmente a spec ativa `2026-05-24-config-idioma-tom`. Hoje há tabelas bilíngues PT/EN espalhadas em três ou mais arquivos, banners hardcoded em pt-BR misturados com código en-US, e nenhum módulo central que aplique tom (didático vs técnico). O user formalizou:

- Spec inteira em pt-BR ou en-US (sem mistura).
- Código + comentários + doc-comments sempre en-US.
- Locale em formato BCP-47 (`pt-BR`/`en-US`), nunca abreviado (cf. `project_locale_codes`).
- `mustard.json` ganha campos `lang` (BCP-47) e `tone` (didactic/technical/concise).

## Tarefas

- [ ] **T4.1.** Módulo `packages/core/src/i18n.rs` com:
  - `enum Locale { PtBr, EnUs }` (parseamento estrito; rejeita formas curtas com erro tipado).
  - `enum Tone { Didactic, Technical, Concise }`.
  - `struct I18n { lang: Locale, tone: Tone }`.
  - `fn translate(key: &str) -> &str` — tabela embedded com chaves canônicas (banners, AC, prompts visíveis).
  - `fn apply_tone(text: &str, tone: Tone) -> String`.
  - `fn slugify(text: &str, lang: Locale) -> String` (PT vs EN diff de acentos/stopwords).
- [ ] **T4.2.** Schema em `mustard.json`: campos `lang` (`"pt-BR"|"en-US"`) e `tone` (`"didactic"|"technical"|"concise"`). Cascade de resolução: header `### Lang:` da spec → `mustard.json#lang` → AskUserQuestion única (gravada em `mustard.json` após resposta).
- [ ] **T4.3.** Refactor de banners hardcoded em pt-BR em `apps/rt/src/{hooks,run,mcp,report,dispatch}.rs` → `i18n.translate("banner.close.success")` etc. Catálogo de chaves documentado em `packages/core/src/i18n.rs`.
- [ ] **T4.4.** Refactor de CLI commands em `apps/cli/src/commands/**` para usar `I18n`.
- [ ] **T4.5.** Dashboard ganha página/aba `Settings` com seletor de `lang` e `tone`. Tauri command `set_language` + `set_tone` que escreve em `mustard.json`.
- [ ] **T4.6.** `apps/rt/src/run/spec_slug.rs` lang-aware (PT slug != EN slug; acentos removidos só do PT).
- [ ] **T4.7.** Novo subcomando `mustard-rt run i18n translate-heading --from "## Tasks" --to-lang pt-BR` (entregue como item do W6, listado aqui para visibilidade).
- [ ] **T4.8.** Novo subcomando `mustard-rt run spec-lang resolve --spec <path>` que devolve JSON `{lang: "pt-BR"|"en-US", source: "header"|"mustard.json"|"ask"}` (entregue no W6).
- [ ] **T4.9.** Padronização siglas: `W3` em en-US, `Onda 3` em pt-BR via `i18n.translate("wave.label", n)`.
- [ ] **T4.10.** Validação: rejeitar `lang: "pt"` ou `"en"` na entrada (formas curtas). Erro tipado `LocaleError::ShortForm`.
- [ ] **T4.11.** Emit `pipeline.economy.operation.invoked { operation: "i18n-translate", duration_ms, tokens_used: 0 }` (saving de cada Read de tabela bilíngue).

## Files

- `packages/core/src/i18n.rs` (novo)
- `packages/core/src/lib.rs` (exportar `i18n`)
- `mustard.json` (schema novo)
- `apps/rt/src/hooks/**` (banners)
- `apps/rt/src/run/**` (banners + spec_slug)
- `apps/rt/src/mcp/**`, `apps/rt/src/report.rs`, `apps/rt/src/dispatch.rs` (banners)
- `apps/cli/src/commands/**` (banners)
- `apps/cli/templates/refs/feature/spec-language.md` (vira só Header Translation Table; resolução roda em Rust)
- `apps/dashboard/src/pages/Settings.tsx` (seletor)
- `apps/dashboard/src-tauri/src/commands/settings.rs` (`set_language`, `set_tone`)

## Critérios de Aceitação

- [ ] **AC-4.1.** `packages/core/src/i18n.rs` existe com `enum Locale { PtBr, EnUs }`. Command: `node -e "const t=require('fs').readFileSync('packages/core/src/i18n.rs','utf8');if(!/enum Locale/.test(t)||!/PtBr/.test(t)||!/EnUs/.test(t))process.exit(1)"`
- [ ] **AC-4.2.** `mustard.json` tem `lang` em BCP-47. Command: `node -e "const j=JSON.parse(require('fs').readFileSync('mustard.json','utf8'));if(!/^(pt-BR|en-US)$/.test(j.lang||''))process.exit(1)"`
- [ ] **AC-4.3.** Locale curto rejeitado. Command: `rtk cargo test -p mustard-core i18n_rejects_short_form 2>&1 | grep -q "ok"`
- [ ] **AC-4.4.** Banners hardcoded em pt-BR em `apps/rt/src/**` reduzidos a zero (ou no máximo dentro de seções de erro raras). Command: `node -e "const{execSync}=require('child_process');const out=execSync('rg -t rust \"(Aprovar|Está|Você está|Avançar|Confirmar)\" apps/rt/src/ -l',{encoding:'utf8'}).trim();if(out.split('\\n').filter(Boolean).length>3)process.exit(1)"`
- [ ] **AC-4.5.** Dashboard Settings tem seletor de lang. Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Settings.tsx','utf8');if(!/lang|locale/i.test(t))process.exit(1)"`
- [ ] **AC-4.6.** `cargo test -p mustard-core i18n_translates_known_keys` passa.

## Notas

- BCP-47 vs forma curta: meta.json (W3) usa `pt-BR`/`en-US` final; durante migração aceita `pt`/`en` com warning para retrocompatibilidade.
- Paralelizável com W5.
- W6 entrega os subcomandos `i18n translate-heading` e `spec-lang resolve`.
