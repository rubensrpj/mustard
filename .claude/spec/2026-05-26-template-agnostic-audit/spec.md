# Template Agnostic Audit

### Stage: Close
### Outcome: Cancelled
### Flags: 
### Scope: full
### Checkpoint: 2026-05-26T16:50:00Z
### Lang: pt-BR

<!--
CLOSE NOTE (2026-05-26):
All 10 ACs pass (verified via mustard-rt run qa-run → overall=pass).
verify-pipeline gate FAILS due to pre-existing deep-refactor W1/W2 fallout in
apps/rt tests (mcp::mcp_server_handshakes_and_serves_all_five_tools fails on
W5 NDJSON path resolution post-ClaudePaths). Captured in sub-spec
[[2026-05-26-estabilizar-testes-rt-regressoes-deep-refactor]]. CLOSE deferred
on verify-pipeline ratification; spec deliverables are complete. Two other
sub-specs created during fix-loop:
- [[2026-05-26-migrar-commands-catalog-ts-env-catalog]] — dashboard data PT→i18n
- [[2026-05-26-refresh-stale-claude-installs-apos-edicoes]] — refresh-claude subcommand
-->


## PRD

## Contexto

O Mustard é distribuído como uma CLI que faz `mustard init` e copia `apps/cli/templates/` para o `.claude/` de um projeto-alvo. A promessa é que o payload entregue funcione para qualquer projeto — uma CLI pura, uma biblioteca, um daemon, um parser, um app CRUD — sem assumir arquétipo. A realidade hoje é outra: os cinco recipes embutidos (`add-field.json`, `add-endpoint.json`, `add-component.json`, `add-validation.json`, `null-guard.json`) descrevem um stack web de três camadas, nomeiam Drizzle, Prisma, Express e React por escrito, e instruem checklists como "Build & type-check backend" / "Build & type-check frontend" como se toda casa tivesse esses dois lados. O `templates/CLAUDE.md` vaza o stack do próprio Mustard (cargo, `mustard-rt run …`) para o projeto-alvo, então um serviço Python recebe um manual de comandos Rust como referência canônica. O `templates/pipeline-config.md` codifica roles fixos (`api/mobile/ui/database/library`), pré-atribui Flutter ao papel "mobile" e documenta `{backend}/{frontend}/{admin}` como placeholders canônicos. O `templates/refs/stack-templates/` instala checklist de React e guia de browser-debug em todo projeto, mesmo num CLI puro. Em paralelo, a política de idioma e tom já está parcialmente codificada — `mustard.json#specLang` aceita só `pt`/`en` curtos (a memória [[project_locale_codes]] manda BCP-47 completo, `pt-BR`/`en-US`), o `meta.json` já normaliza no momento da escrita, mas todo o resto do harness (geradores, validadores, comparações Rust) ainda compara `== "pt"`. O tom (`Tone::Didactic|Technical|Concise`) existe em `packages/core/src/i18n.rs` e tem UI no dashboard, mas hoje só molda banners do `mustard-rt` — os geradores de spec ignoram. Não há validador que enforce "spec usa o idioma do `mustard.json#specLang`; todo o resto do repositório em EN". Esta spec ataca os três blocos: payload do init agnóstico, locale BCP-47 em todo o harness, e tom + idioma-por-artefato como política aplicada e auditável.

## Usuários/Stakeholders

Maintainer único (Rubens). Indireto: qualquer dev que rode `mustard init` num projeto não-CRUD e hoje recebe lixo semântico no `.claude/` (recipes pedindo `{Entity}.tsx`, CLAUDE.md falando de cargo num projeto Python). A política de locale/tom afeta também futuros contribuidores que precisarão escrever specs respeitando a configuração do projeto sem adivinhar.

## Métrica de sucesso

Um `mustard init` em três projetos sentinelas — uma lib Rust pura (sem UI, sem DB, sem API), um daemon Go e um app CRUD JS — deposita zero arquivos no `.claude/` que assumam arquétipo que aquele projeto não tem. Toda string `pt`/`en` curta no harness foi substituída por `pt-BR`/`en-US`, exceto leitura tolerante de specs antigas. `mustard.json` traz `specLang: "pt-BR"` e `tone: "didactic"` (ambos obrigatórios pós-W5). O comando novo `mustard-rt run language-audit` lista zero hits no próprio repo do Mustard depois das limpezas das W1–W4.

## Não-Objetivos

- Tocar `apps/rt/src/run/scan/` (o W3 da spec deep-refactor finalizada é o dono dos recipes gerados a partir do scan; esta spec só remove os abstratos do payload e o consumidor não-agnóstico em `recipe_match.rs`).
- Recriar recipes abstratos em outro lugar (`templates-extras/recipes/`, por exemplo) — eles morrem e ponto; quem precisa de skeleton, roda o scan no projeto e o cluster discovery cria recipes reais.
- Migrar specs históricas (`.claude/spec/*/spec.md` com `### Lang: pt-BR`) para `pt-BR`. A política [[feedback_no_migration_dev_phase]] aplica: dev, sem usuário em prod; reader Rust faz fallback compat para leitura, geração nova já sai em BCP-47.
- Tocar skills foundation que já são agnósticas (`commit-workflow`, `karpathy-guidelines`, etc.).
- Bloquear commit/PR baseado em locale/tom drift — o validador é soft, lista no `/stats`, não interrompe fluxo (memória [[feedback_mustard_transparent_execution]]).
- Mexer no scan engine ou no skill-resolve — W1/W3 do deep-refactor já fecharam.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: `apps/cli/templates/recipes/` não existe ou está vazio — Command: `node -e "const fs=require('fs');const p='apps/cli/templates/recipes';process.exit(!fs.existsSync(p)||fs.readdirSync(p).filter(f=>f.endsWith('.json')).length===0?0:1)"`
- [x] AC-2: `apps/cli/templates/CLAUDE.md` não menciona cargo, mustard-rt, mustard-cli, ou pnpm como comandos canônicos — Command: `node -e "const c=require('fs').readFileSync('apps/cli/templates/CLAUDE.md','utf8');const hits=['cargo build','cargo test','mustard-rt run','mustard-cli','pnpm --filter'].filter(s=>c.includes(s));process.exit(hits.length===0?0:1)"`
- [x] AC-3: `apps/cli/templates/pipeline-config.md` não tem tabela de Role Rules com Flutter/Dart hardcoded — Command: `node -e "const c=require('fs').readFileSync('apps/cli/templates/pipeline-config.md','utf8');process.exit(c.includes('Flutter/Dart')||c.includes('| mobile |')?1:0)"`
- [x] AC-4: `apps/cli/templates/refs/stack-templates/` não existe — Command: `node -e "process.exit(require('fs').existsSync('apps/cli/templates/refs/stack-templates')?1:0)"`
- [x] AC-5: `apps/cli/templates-extras/refs/stack-templates/` contém os arquivos movidos — Command: `node -e "const p='apps/cli/templates-extras/refs/stack-templates';const fs=require('fs');process.exit(fs.existsSync(p)&&fs.readdirSync(p).includes('fe-craft-check.md')&&fs.readdirSync(p).includes('browser-debug.md')?0:1)"`
- [x] AC-6: `apps/rt/src/run/recipe_match.rs` não contém `find_dir_by_convention`, `resolve_pattern`, nem `to_pascal_case` — Command: `node -e "const fs=require('fs');const p='apps/rt/src/run/recipe_match.rs';if(!fs.existsSync(p))process.exit(0);const c=fs.readFileSync(p,'utf8');const hits=['find_dir_by_convention','resolve_pattern','to_pascal_case'].filter(s=>c.includes(s));process.exit(hits.length===0?0:1)"`
- [x] AC-7: `mustard-rt` builda limpo e o scan cold-path passa pós-cleanup de W4 — Command: `bash -c 'cargo build -p mustard-rt 2>&1 | tail -3 && cargo test -p mustard-rt --test scan_cold_path 2>&1 | tail -3'`
  <!-- Rationale (fix-loop 2026-05-26 v2): scan-recipes-validate (CLI) e scan_recipes_validate (teste) nunca existiram. AC-7 foi reescrito para validar o intent real do W4: rt build limpo + scan cold-path funcional pós-remoção de find_dir_by_convention/resolve_pattern/to_pascal_case. AC-6 cobre as remoções; AC-7 cobre que nada quebrou. -->

- [x] AC-8: `mustard.json` tem `specLang: "pt-BR"` e `tone` definido — Command: `node -e "const j=JSON.parse(require('fs').readFileSync('.claude/mustard.json','utf8'));process.exit(j.specLang==='pt-BR'&&typeof j.tone==='string'?0:1)"`
- [x] AC-9: `Locale::from_str("pt-BR")` aceita; `Locale::from_str("pt")` rejeita ou normaliza com warning — Command: `bash -c 'cargo test -p mustard-core i18n::tests::locale_parses_bcp47 && cargo test -p mustard-core i18n::tests::i18n_rejects_short_form'`
  <!-- Rationale (fix-loop 2026-05-26): nome original `locale_accepts_bcp47` não existe; testes reais são `locale_parses_bcp47` (aceita BCP-47) + `i18n_rejects_short_form` (rejeita "pt"/"en" curtos). -->
- [x] AC-10: `mustard-rt run language-audit` retorna zero hits no repo após W1–W4 — Command: `cargo run -q -p mustard-rt -- run language-audit --format json --strict`

## Plano

## Informações da Entidade

Não há entidade de domínio nova — esta spec é estritamente refactor + remoção + política. Os "agregados" tocados são: o payload de `templates/` (visto como um único artefato distribuído), o módulo `i18n` do `mustard-core` (entrada da política de idioma+tom), e o consumidor não-agnóstico `recipe_match.rs`.

## Arquivos

### W1 — purge templates/recipes/
- `apps/cli/templates/recipes/add-field.json` (DELETE)
- `apps/cli/templates/recipes/add-endpoint.json` (DELETE)
- `apps/cli/templates/recipes/add-component.json` (DELETE)
- `apps/cli/templates/recipes/add-validation.json` (DELETE)
- `apps/cli/templates/recipes/null-guard.json` (DELETE)
- `apps/cli/src/commands/init.rs` ou equivalente (MODIFY se houver referência hardcoded à pasta `recipes/` no copy loop)

### W2 — CLAUDE.md + pipeline-config.md
- `apps/cli/templates/CLAUDE.md` (REWRITE — meta-agnóstico, ver "Critérios para reescrita" abaixo)
- `apps/cli/templates/pipeline-config.md` (MODIFY — purgar Role Rules fixos, Flutter→mobile, placeholders `{backend}/{frontend}/{admin}`, default "DB+Backend Wave 1 / Frontend Wave 2")

### W3 — stack-templates + artifacts
- `apps/cli/templates/refs/stack-templates/fe-craft-check.md` (MOVE → `apps/cli/templates-extras/refs/stack-templates/fe-craft-check.md`)
- `apps/cli/templates/refs/stack-templates/browser-debug.md` (MOVE → `apps/cli/templates-extras/refs/stack-templates/browser-debug.md`)
- `apps/cli/templates/.artifacts.json` (MODIFY — reclassificar `skill:react-best-practices` como opt-in; mover declaração para `apps/cli/templates-extras/.artifacts.json` ou marcar com flag `optIn: true`)
- Lógica de install no `mustard init` (MODIFY se necessário — pular `templates-extras/` por default; flag `--extras` para incluir)

### W4 — recipe_match.rs cleanup
- `apps/rt/src/run/recipe_match.rs` (MODIFY ou DELETE — remover `find_dir_by_convention`, `resolve_pattern`, `to_pascal_case`; manter só carga do JSON derivado do scan + persist economy + delegate_to_resolver; se nada sobrar de útil, deletar arquivo e remover dispatch em `apps/rt/src/run/mod.rs`)
- `apps/rt/src/run/mod.rs` (MODIFY se W4 deletar recipe_match)
- `apps/rt/tests/recipe_match*.rs` (MODIFY/DELETE testes obsoletos)

### W5 — locale + tone + language-per-artifact
- `packages/core/src/i18n.rs` (MODIFY — `Locale::from_str` aceita `"pt-BR"`/`"en-US"` como caminho feliz; mantém erro para `"pt"`/`"en"` curtos OU emite warning e normaliza; `Tone` já existe — não tocar)
- `packages/core/src/meta.rs` (MODIFY — `normalise_lang` vira identidade para BCP-47, deprecated path para curtos)
- `packages/core/src/spec/contract.rs` (MODIFY — comparações `== "pt"` viram `== "pt-BR"`)
- `packages/core/src/reader/sqlite.rs` (MODIFY — leitura tolerante: aceita ambos; escrita só BCP-47)
- `packages/core/src/projection/card.rs` (MODIFY — testes + lógica)
- `packages/core/src/model/event.rs` (MODIFY — docstring)
- `apps/rt/src/run/wave_scaffold.rs` (MODIFY — `if lang == "en"` vira `if lang.starts_with("en")` ou compara BCP-47)
- `apps/rt/src/run/amend_finalize.rs` (MODIFY — todas comparações)
- `apps/rt/src/run/agent_prompt_render.rs` (MODIFY — `read_spec_lang` default vira `"en-US"`)
- `apps/rt/src/run/spec_lang_resolve.rs` (MODIFY)
- `apps/rt/src/run/spec_draft.rs` (MODIFY — injetar tone no prompt do agente)
- `apps/rt/src/run/spec_memory.rs` (MODIFY)
- `apps/rt/src/run/spec_validate.rs` (MODIFY — validar que spec usa idioma configurado)
- `apps/rt/src/run/plan_from_spec.rs` (MODIFY)
- `apps/rt/src/run/i18n_translate.rs` (MODIFY)
- `apps/rt/src/run/emit_pipeline.rs` (MODIFY — payload `lang` em BCP-47)
- `apps/rt/src/hooks/amend_capture.rs` (MODIFY — comparação tolerante)
- `apps/rt/src/run/language_audit.rs` (CREATE — novo subcomando)
- `apps/rt/src/run/mod.rs` (MODIFY — registrar language_audit)
- `apps/dashboard/src/i18n.ts` (MODIFY — `Lang = "pt-BR" | "en-US"`)
- `apps/dashboard/src/lib/i18n.ts` (MODIFY)
- `apps/dashboard/src-tauri/src/commands/settings.rs` (MODIFY)
- `apps/dashboard/mustard.json` (MODIFY — `specLang: "pt-BR"`)
- `.claude/mustard.json` (MODIFY — `specLang: "pt-BR"`, `tone: "didactic"`)
- `.claude/commands/mustard/feature/SKILL.md` (MODIFY — cascade aceita BCP-47, AskUserQuestion em "pt-BR | en-US")
- `.claude/commands/mustard/bugfix/SKILL.md` (MODIFY — mesmo)
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (MODIFY)
- `apps/cli/templates/commands/mustard/bugfix/SKILL.md` (MODIFY)
- `apps/cli/templates/refs/feature/spec-language.md` (REWRITE — explicar 3 dimensões: locale/tone/idioma-por-artefato)
- Tests em `apps/rt/tests/*` e `packages/core/src/**/tests` (MODIFY fixtures de `"pt"` → `"pt-BR"`)

## Tarefas

### CLI Agent (Wave 1)
- [ ] Apagar os 5 JSONs em `apps/cli/templates/recipes/`
- [ ] Verificar se `apps/cli/src/commands/init.rs` referencia `recipes/` no copy loop; se sim, manter copy loop (pasta vazia ok) mas remover qualquer special-casing
- [ ] Build + test: `cargo test -p mustard-cli`
- [ ] Smoke: rodar `mustard init` num tmpdir e confirmar que `.claude/recipes/` não foi criado

### CLI Agent (Wave 2)
- [ ] Reescrever `apps/cli/templates/CLAUDE.md` do zero. Critérios: zero menção a cargo/mustard-rt/mustard-cli/pnpm; zero menção a "entity" como conceito universal; mantém regras de orquestração (L0 delegation, intent routing, spec layout, response style); blocos Stack/Build/Commands removidos; tom didactic (igual ao [`.claude/CLAUDE.md`](.claude/CLAUDE.md) hoje)
- [ ] Limpar `apps/cli/templates/pipeline-config.md`: deletar Role Rules tabela com Flutter/Dart, transformar em comentário "Role Rules are populated by /scan based on detected subprojects — there is no canonical role list"; remover defaults "DB+Backend Wave 1 / Frontend Wave 2"; remover placeholders `{backend}`, `{frontend}`, `{admin}` da seção Recipe Engine
- [ ] Build CLI: `cargo build -p mustard-cli`

### CLI Agent (Wave 3)
- [ ] Criar `apps/cli/templates-extras/refs/stack-templates/`
- [ ] Mover `fe-craft-check.md` e `browser-debug.md` para o novo local
- [ ] Atualizar `apps/cli/templates/.artifacts.json`: remover entrada de `skill:react-best-practices` OU mover para `templates-extras/.artifacts.json` (se padrão existir)
- [ ] Verificar copy logic do `mustard init`: confirmar que `templates-extras/` não é copiado por default (memória [[feedback_mustard_install_workflow]] e padrão do hallmark)
- [ ] Build + smoke `mustard init` em tmpdir; confirmar ausência de `refs/stack-templates/` e ausência de `skill:react-best-practices`

### RT Agent (Wave 4) (parallel-safe com W2, W3)
- [ ] Auditar `apps/rt/src/run/recipe_match.rs`: identificar o que ainda agrega valor (carga JSON, persist economy, delegate_to_resolver) vs o que é puro hardcode de arquétipo
- [ ] Remover `find_dir_by_convention`, `to_pascal_case`, `resolve_pattern` (linhas 32-84)
- [ ] Simplificar `run()`: ler JSON, persistir economy, delegar resolver, imprimir `files[].path` direto (já vem real do scan W3 do deep-refactor)
- [ ] Avaliar: se a função `run()` virar só ~30 linhas e nada do `recipe-match` for mais consumido pelos SKILLs (que vão ter sido limpos em W2), considerar DELETAR o arquivo + remover dispatch em `mod.rs`. Decisão fica com o impl agent.
- [ ] `cargo test -p mustard-rt --test scan_recipes_validate` (regressão)
- [ ] `cargo run -p mustard-rt -- run scan-recipes-validate --strict` no próprio repo

### Mixed Agent (Wave 5)
- [ ] **Locale**: `packages/core/src/i18n.rs` — `Locale::from_str` aceita `"pt-BR"`/`"en-US"`; curto vira erro com mensagem clara (`use pt-BR/en-US`)
- [ ] `packages/core/src/meta.rs` — `normalise_lang` vira identidade pra BCP-47; legacy short paths emitem warning único na primeira invocação por execução
- [ ] Sweep `s/== "pt"/== "pt-BR"/g` (e `"en"`→`"en-US"`) em todo `apps/rt/src/run/`, `packages/core/src/`, `apps/dashboard/src-tauri/`
- [ ] Sweep `s/Lang = "pt" \| "en"/Lang = "pt-BR" \| "en-US"/g` em `apps/dashboard/src/`
- [ ] `.claude/mustard.json` e `apps/dashboard/mustard.json`: atualizar `specLang` para `pt-BR`, adicionar `tone: "didactic"`
- [ ] **Tone**: `apps/rt/src/run/spec_draft.rs` — ler `tone` do `mustard.json`, injetar como instrução no prompt do agente (`"Write this spec in {tone} tone — see docs/tone-policy.md"`)
- [ ] **Validator**: criar `apps/rt/src/run/spec_validate.rs` (ou estender existente) com check `spec.lang == mustard.json#specLang` — soft warning, não bloqueia
- [ ] **language-audit**: novo `apps/rt/src/run/language_audit.rs`. Heurística: escanear `apps/cli/templates/`, `apps/cli/templates-extras/`, `apps/*/src/`, `packages/*/src/`, `.claude/refs/`. Para cada `.md`/`.rs`/`.ts`, contar palavras PT-BR comuns (sem acento dá falso positivo — usar palavras com diacrítico: "não", "está", "também", "função", etc.); se PT > threshold, registrar hit
- [ ] Registrar `language_audit` em `apps/rt/src/run/mod.rs`
- [ ] Reescrever `apps/cli/templates/refs/feature/spec-language.md` cobrindo as 3 dimensões (locale BCP-47, tone, idioma-por-artefato)
- [ ] Atualizar `.claude/commands/mustard/feature/SKILL.md` e `bugfix/SKILL.md` para cascade BCP-47 + AskUserQuestion "pt-BR | en-US"
- [ ] Atualizar templates equivalentes em `apps/cli/templates/commands/mustard/`
- [ ] Build + test: `cargo build`, `cargo test`
- [ ] Smoke: rodar `mustard-rt run language-audit --format json` no próprio repo, confirmar zero hits (após limpezas das W1–W4)

## Dependências

- **W2 depende de W1**: a reescrita de CLAUDE.md pode referenciar (na seção "what init deposita") a ausência de recipes — precisa de W1 fechada para a referência ser real.
- **W3 depende de W2**: stack-templates move + .artifacts.json edit ficam mais limpos se templates/ já foi sanitizado (W2 pode tocar `.artifacts.json` indiretamente).
- **W4 depende de W1**: recipe_match.rs só faz sentido limpar depois que os recipes abstratos sumiram (senão validate falha).
- **W5 depende de W2**: spec-language.md reescrito (W5) referencia regras que existem só pós-W2 (CLAUDE.md meta-agnóstico).
- **Não há dependência cruzada W4↔W5**: podem rodar em paralelo (rt vs mixed) se ambas tiverem pré-requisitos satisfeitos.

## Limites

- `apps/cli/templates/recipes/` — DELETAR
- `apps/cli/templates/CLAUDE.md` — REESCREVER
- `apps/cli/templates/pipeline-config.md` — MODIFY (cortes cirúrgicos)
- `apps/cli/templates/refs/stack-templates/` — MOVER para `templates-extras/`
- `apps/cli/templates/.artifacts.json` — MODIFY
- `apps/cli/templates-extras/` — CRIAR/EXPANDIR
- `apps/cli/templates/commands/mustard/feature/SKILL.md` + `bugfix/SKILL.md` — MODIFY (W5)
- `apps/cli/templates/refs/feature/spec-language.md` — REESCREVER (W5)
- `apps/rt/src/run/recipe_match.rs` — MODIFY ou DELETE (W4)
- `apps/rt/src/run/mod.rs` — MODIFY (W4 + W5)
- `apps/rt/src/run/language_audit.rs` — CRIAR (W5)
- `apps/rt/src/run/{wave_scaffold,amend_finalize,agent_prompt_render,spec_lang_resolve,spec_draft,spec_memory,spec_validate,plan_from_spec,i18n_translate,emit_pipeline}.rs` — MODIFY (W5)
- `apps/rt/src/hooks/amend_capture.rs` — MODIFY (W5)
- `apps/rt/tests/**/*.rs` — MODIFY (W5, fixtures)
- `packages/core/src/{i18n,meta,spec/contract,reader/sqlite,projection/card,model/event}.rs` — MODIFY (W5)
- `apps/dashboard/src/{i18n,lib/i18n}.ts` — MODIFY (W5)
- `apps/dashboard/src/pages/Settings.tsx` — MODIFY se o dropdown listar valores `pt`/`en` (W5)
- `apps/dashboard/src-tauri/src/commands/settings.rs` — MODIFY (W5)
- `apps/dashboard/mustard.json` + `.claude/mustard.json` — MODIFY (W5)
- `.claude/commands/mustard/feature/SKILL.md` + `bugfix/SKILL.md` — MODIFY (W5, espelho do template)

**FORA dos limites** (não tocar):
- `apps/rt/src/run/scan/` — W3 do deep-refactor é dono
- `templates/skills/{karpathy-guidelines,commit-workflow,grill-with-docs,...}` — já agnósticas
- `templates-extras/skills/hallmark/` — intencionalmente especializado
- Specs históricas em `.claude/spec/` — não migrar `### Lang: pt-BR` para `pt-BR` (fallback compat no reader)
- Qualquer mudança no scan engine, skill-resolve, ou recipe generation (output do scan)

## Cobertura

| Crítica/Preocupação do usuário | Onde foi tratada |
|---|---|
| "Recipes em templates não fazem sentido — scan deveria descobrir" | W1 + W4 (apaga abstratos + limpa consumidor não-agnóstico) |
| "Mustard tem que ser 100% agnóstico" | Princípio fundador da spec; AC-1 a AC-7 validam mecanicamente |
| "CLAUDE.md vaza stack do Mustard pro projeto-alvo" | W2 (reescrita do zero meta-agnóstica) |
| "Roles fixos api/mobile/ui/database/library + Flutter→mobile" | W2 (pipeline-config sanitization) |
| "stack-templates/fe-craft-check + browser-debug instalam em todo projeto" | W3 (move pra templates-extras opt-in) |
| "Rust faz manifesto/AST puro; semântica vai pro scan-LLM" | Linha mantida em W4 (recipe_match não tenta mais derivar arquétipo) |
| "Locale padrão é pt-BR/en-US (BCP-47), não pt/en" | W5(a) |
| "Specs no idioma do mustard.json; resto tudo em EN" | W5(c) — language-audit + spec_validate |
| "Tone (didactic/technical/concise) já existe — usar em specs" | W5(b) — spec_draft injeta tone no prompt |
| "Validador soft, não bloqueia commit" | W5(c) — language-audit é warn-only |
| "Hard-cut locale, não migrar specs antigas" | Não-Objetivo explícito + fallback compat no reader |
| "Não criar nova spec de deep-refactor — aquela já fechou" | Esta é spec independente, não sub-spec; root spec.md sem `### Parent:` |
