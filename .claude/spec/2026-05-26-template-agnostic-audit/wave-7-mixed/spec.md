# wave-7-mixed — Policy enforcement (sweep + tone wire + language-audit)

### Parent: [[2026-05-26-template-agnostic-audit]]
### Stage: Plan
### Outcome: Active
### Flags:

## Resumo

Depois do refator de tipos i18n (W5), faz o sweep mecânico, wire do tone, audit de idioma por artefato, e doc.

(a) **Sweep**: trocar `== "pt"` → `== "pt-BR"` e `== "en"` → `== "en-US"` em `apps/rt/src/`, `apps/dashboard/src-tauri/`, tests. Atualizar `mustard.json#specLang = "pt-BR"`, adicionar `tone: "didactic"`. Dashboard React: `Lang = "pt-BR" | "en-US"`.
(b) **Tone wire**: `spec_draft.rs` lê tone do `mustard.json` e injeta no prompt do agente drafter como instrução textual.
(c) **language-audit**: novo `mustard-rt run language-audit` lista arquivos com palavras PT distintivas (com diacrítico) acima de threshold em targets pré-definidos. Soft warning, exit 0.
(d) **Doc + SKILLs**: REWRITE `templates/refs/feature/spec-language.md` cobrindo 3 dimensões; MODIFY SKILLs feature/bugfix do template + espelho local.

## Network

- Parent: [[2026-05-26-template-agnostic-audit]]
- Depends on: [[wave-5-mixed]] (precisa dos tipos SupportedLocale + UserLocale)

## Arquivos

### Sweep (a)
- `apps/rt/src/run/{wave_scaffold,amend_finalize,agent_prompt_render,spec_lang_resolve,spec_memory,plan_from_spec,i18n_translate,emit_pipeline}.rs` (MODIFY `== "pt"` → `== "pt-BR"`)
- `apps/rt/src/hooks/amend_capture.rs` (MODIFY)
- `apps/dashboard/src/i18n.ts` + `apps/dashboard/src/lib/i18n.ts` (MODIFY type Lang)
- `apps/dashboard/src/pages/Settings.tsx` (MODIFY dropdown values)
- `apps/dashboard/src-tauri/src/commands/settings.rs` (MODIFY)
- `apps/dashboard/mustard.json` + `.claude/mustard.json` (MODIFY: specLang BCP-47 + add tone)
- Tests: `apps/rt/tests/{migrate_spec_headers,amend_finalize,pipeline_state_projection_test}.rs` + fixtures (MODIFY asserts + fixtures novas; manter fixtures legacy para validar reader tolerante)

### Tone (b)
- `apps/rt/src/run/spec_draft.rs` (MODIFY — ler tone + injetar)

### Audit (c)
- `apps/rt/src/run/language_audit.rs` (CREATE)
- `apps/rt/src/run/mod.rs` (MODIFY — registrar)

### Doc + SKILLs (d) — auditoria ampliada

Toda menção a `pt`/`en` curto, `Lang: pt|en`, `Lang=pt`, `Lang=en`, `"Spec language: pt | en?"`, `specLang: "pt" | "en"` precisa virar BCP-47 (`pt-BR`/`en-US`). Lista completa após Grep:

#### Templates (payload do `mustard init`)
- `apps/cli/templates/refs/feature/spec-language.md` (REWRITE pesado — 3 dimensões + Header Translation Table + toda menção `Lang: pt|en` vira `Lang: pt-BR|en-US`)
- `apps/cli/templates/refs/spec/approve-only-flow.md` (MODIFY — linha 71: `(Lang=en: ...)` vira `(Lang=en-US: ...)`)
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (MODIFY — linhas 25, 114, 116, 129, 150, 180, 203, 231, 232: todas referências `pt|en` viram `pt-BR|en-US`; cascade text; AskUserQuestion text; type union `"lang": "pt-BR" | "en-US"`; condicionais `Lang=pt`/`Lang=en` viram `Lang=pt-BR`/`Lang=en-US`)
- `apps/cli/templates/commands/mustard/bugfix/SKILL.md` (MODIFY — linhas equivalentes: cascade, AskUserQuestion, HARD RULE de headers, condicionais)
- `apps/cli/templates/commands/mustard/tactical-fix/SKILL.md` (MODIFY — linha 79+: `Lang=en` vira `Lang=en-US`)

#### Espelho local (`.claude/` da raiz — o que está vivo neste repo)
- `.claude/commands/mustard/feature/SKILL.md` (MODIFY — espelho com mesmas linhas problemáticas)
- `.claude/commands/mustard/bugfix/SKILL.md` (MODIFY)
- `.claude/commands/mustard/tactical-fix/SKILL.md` (MODIFY)
- `.claude/refs/feature/spec-language.md` (REWRITE — espelho do template)

#### Validação
- Após mudanças: `grep -rn "Lang: pt\b\|Lang: en\b\|Lang=pt\b\|Lang=en\b\|\"pt\" | \"en\"" apps/cli/templates/ .claude/commands/ .claude/refs/` retorna **zero hits** (exceto blocos de retrocompatibilidade documentando que `pt`/`en` legados são lidos com warning)

## Tarefas

### Sweep Agent
- [ ] Sweep `s/== "pt"/== "pt-BR"/g` e `s/== "en"/== "en-US"/g` em `apps/rt/src/` (verificar cada match — alguns podem ser intencionais para comparar contra `UserLocale::as_str()` legado, mas no contexto da W7 todas as comparações esperam o valor canônico BCP-47)
- [ ] `apps/rt/src/hooks/amend_capture.rs` linha 449: `s.eq_ignore_ascii_case("pt")` → manter case-insensitive mas comparar contra `"pt-br"` (mantém fallback compat)
- [ ] Dashboard: `Lang = "pt" | "en"` → `Lang = "pt-BR" | "en-US"` em `i18n.ts` + `lib/i18n.ts`; atualizar `lng:"pt"`, `fallbackLng:"pt"`, função `setLanguage`
- [ ] `Settings.tsx`: dropdown values atualizados
- [ ] `apps/dashboard/src-tauri/src/commands/settings.rs`: leitura/escrita BCP-47
- [ ] `.claude/mustard.json` e `apps/dashboard/mustard.json`: `specLang: "pt-BR"`, adicionar `tone: "didactic"`
- [ ] Tests: atualizar fixtures NOVAS para BCP-47, manter fixtures legacy para validar reader tolerante (cobre user que tem `### Lang: pt` em spec histórica)
- [ ] `cargo build` workspace verde

### Tone Agent
- [ ] `apps/rt/src/run/spec_draft.rs`: ler `tone` de `mustard.json` via `mustard_core::config` (ou helper novo se não existe — `read_mustard_json_tone(cwd) -> Tone`)
- [ ] Injetar no prompt do agente drafter como bloco: `"## Tone\n\nWrite this spec in {tone} tone:\n- didactic: expand abbreviations on first use, prefer plain words, explain why\n- technical: direct, jargon ok, no hand-holding\n- concise: minimal prose, focus on facts\n"`
- [ ] Test inline: agent prompt gerado contém substring "in didactic tone" quando `mustard.json#tone = "didactic"`

### Audit Agent
- [ ] CREATE `apps/rt/src/run/language_audit.rs` (~150 linhas)
- [ ] Subcomando: `mustard-rt run language-audit [--format text|json]`
- [ ] Targets pré-definidos (paths relativos ao cwd, varredura recursiva):
  - `apps/cli/templates/` (EXCETO `templates/refs/feature/spec-language.md` na allow-list)
  - `apps/cli/templates-extras/` (NÃO escanear — opt-in pode estar em qualquer idioma)
  - `apps/*/src/`
  - `packages/*/src/`
  - `.claude/refs/`, `.claude/commands/`, `.claude/skills/`
- [ ] Extensões: `.md`, `.rs`, `.ts`, `.tsx`, `.json` (para JSON pegar só `description` fields se vier ao caso, ou skipar — decisão do agent)
- [ ] Heurística PT-BR: contar palavras com diacrítico únicas a português entre `["não", "está", "também", "função", "ação", "configuração", "para", "porém", "então", "específico", "específica", "diretório", "comando", "execução", "padrão", "código", "deve", "está", "são", "será", "está"]`
- [ ] Threshold: ≥3 palavras distintivas únicas por arquivo = hit
- [ ] Skip allow-list (regex paths): `apps/cli/templates/refs/feature/spec-language.md`, `apps/rt/tests/fixtures/`, qualquer arquivo com primeira linha contendo `<!-- LANG: pt-allowed -->` marker
- [ ] Output JSON: `{"scanned": N, "hits": [{"file": "...", "matches": K, "samples": [...]}], "ok": bool}`
- [ ] Output text (default): linha por linha + summary
- [ ] Registrar em `mod.rs`: `mod language_audit;` + variant `RunCmd::LanguageAudit { format: Option<String> }` + dispatch
- [ ] Tests inline: arquivo PT puro = hit; EN puro = no hit; threshold 2 = no hit; allow-list = no hit; marker = no hit

### Doc Agent (auditoria pt/en em templates/refs/commands)
- [ ] REWRITE `apps/cli/templates/refs/feature/spec-language.md`. Adicionar seção **inicial** "Política de Idioma e Tom do mustard.json — 3 Dimensões":
  - **(1) Idioma da spec** vem de `mustard.json#specLang` (BCP-47: `pt-BR`, `en-US`, `fr-FR`, ...). Headings, narrativa, bullets — tudo no idioma configurado. Não misturar dentro de uma spec.
  - **(2) Tom da narrativa** vem de `mustard.json#tone` (`didactic` | `technical` | `concise`). Aplica em descrição/contexto/limites. Idioma e tom são independentes.
  - **(3) Resto do repositório**: SEMPRE EN (código, templates, refs, ADRs, CONTEXT.md, JSONs, comentários de código). `mustard-rt run language-audit` lista drift soft.
  - Preservar Header Translation Table e exemplos de Contexto Narrative Rules — mas atualizar TODA menção `Lang: pt` para `Lang: pt-BR`, `Lang: en` para `Lang: en-US`, `### Lang: pt` para `### Lang: pt-BR`, etc.
- [ ] MODIFY `apps/cli/templates/refs/spec/approve-only-flow.md`: `(Lang=en: ...)` → `(Lang=en-US: ...)`
- [ ] MODIFY `apps/cli/templates/commands/mustard/feature/SKILL.md`:
  - linha 25: cascade text — substituir `pt|en` por `pt-BR|en-US`
  - linha 114: `### Lang: pt|en` → `### Lang: pt-BR|en-US`; AskUserQuestion text → `"Spec language: pt-BR | en-US?"`
  - linha 116: HARD RULE — atualizar todas referências `Lang=pt` → `Lang=pt-BR`, `Lang=en` → `Lang=en-US`
  - linha 129: `(Lang=pt) / (Lang=en)` → `(Lang=pt-BR) / (Lang=en-US)`
  - linha 150: type union `"lang": "pt" | "en"` → `"lang": "pt-BR" | "en-US"`
  - linhas 180, 203, 231, 232: idem
- [ ] MODIFY `apps/cli/templates/commands/mustard/bugfix/SKILL.md`: linhas 92-94, 96, 104, 106, 108 — mesmo padrão
- [ ] MODIFY `apps/cli/templates/commands/mustard/tactical-fix/SKILL.md`: linha 79+ — `Lang=en` → `Lang=en-US`
- [ ] Espelho local (mesma edição):
  - `.claude/commands/mustard/feature/SKILL.md`
  - `.claude/commands/mustard/bugfix/SKILL.md`
  - `.claude/commands/mustard/tactical-fix/SKILL.md`
  - `.claude/refs/feature/spec-language.md` (REWRITE — espelha o de template)
- [ ] Validação: `grep -rn "Lang: pt\b\|Lang: en\b\|Lang=pt\b\|Lang=en\b\|specLang: \"pt\"\|specLang: \"en\"\|Spec language: pt | en" apps/cli/templates/ .claude/commands/ .claude/refs/` retorna zero (exceto blocos retrocompat documentados)

## Critérios de Aceitação

- [ ] AC-W7-1: `.claude/mustard.json` tem `specLang: "pt-BR"` e `tone` definido — Command: `node -e "const j=JSON.parse(require('fs').readFileSync('.claude/mustard.json','utf8'));process.exit(j.specLang==='pt-BR'&&typeof j.tone==='string'?0:1)"`
- [ ] AC-W7-2: `cargo build` workspace passa — Command: `cargo build`
- [ ] AC-W7-3: `cargo test -p mustard-rt` passa — Command: `cargo test -p mustard-rt`
- [ ] AC-W7-4: `language-audit` zero hits no repo — Command: `bash -c 'cargo run -q -p mustard-rt -- run language-audit --format json | node -e "let s=\"\";process.stdin.on(\"data\",c=>s+=c).on(\"end\",()=>process.exit(JSON.parse(s).hits.length===0?0:1))"'`
- [ ] AC-W7-5: `templates/refs/feature/spec-language.md` menciona "BCP-47" e "tone" — Command: `node -e "const c=require('fs').readFileSync('apps/cli/templates/refs/feature/spec-language.md','utf8');process.exit(c.includes('BCP-47')&&c.toLowerCase().includes('tone')?0:1)"`
- [ ] AC-W7-6: spec_draft injeta tone no prompt — Command: `cargo test -p mustard-rt spec_draft::tests::tone_injected`
- [ ] AC-W7-7: zero menções `pt|en` curto em templates/commands/refs — Command: `bash -c 'count=$(grep -rn -E "Lang: pt\\b|Lang: en\\b|Lang=pt\\b|Lang=en\\b|specLang: \"pt\"|specLang: \"en\"|Spec language: pt \\| en" apps/cli/templates/ .claude/commands/ .claude/refs/ 2>/dev/null | wc -l); test "$count" = "0"'`

## Limites

- MODIFY ~22 arquivos (Rust rt + dashboard React + Tauri + configs + tests + templates + SKILLs)
- CREATE: `apps/rt/src/run/language_audit.rs`
- REWRITE: `templates/refs/feature/spec-language.md`
- FORA: scan engine, skill-resolve, specs históricas em `.claude/spec/*` (não migrar headers); apps/cli/templates-extras/ (opt-in, qualquer idioma); packages/core (W5 dona)
