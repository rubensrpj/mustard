# dependency-precheck — gate factual de pré-condições antes de dispatch

## PRD

## Contexto

Hoje o orquestrador despacha agentes Opus (impl, ui, etc.) confiando que a spec é executável — que todos os símbolos, componentes, paths e imports que ela referencia já existem no codebase. Quando essa premissa quebra (ex.: Wave 5 do `dashboard-design-system` assumiu que a Wave 2 entregaria `EditorialBand`, `KpiValue`, `KPIRow`, `DeltaText`, `DataRow`, `CostBar`, `LegendSwatch`, mas a Wave 2 explicitamente excluiu esses primitives do escopo), o agente só descobre o gap no meio do dispatch, depois de gastar ~90k tokens fazendo Read/Grep/Glob/build até concluir BLOCKED. Existe um gate `Pre-EXECUTE Existence Gate` que valida se os **arquivos da spec já foram modificados** (idempotência de retry), mas não existe gate que valide se as **dependências externas** que a spec assume — primitives, símbolos, exports importados — realmente existem. O `dependency-precheck` fecha essa lacuna: roda em ≤2s, faz greps factuais (não heurísticos) no subproject, e bloqueia o dispatch antes de gastar tokens quando algum símbolo necessário não existe — sugerindo paths concretos para a sub-spec tactical-fix que precisaria criá-los.

## Usuários/Stakeholders

Quem é afetado: usuários do Mustard rodando pipelines `/resume` e `/feature` em projetos com waves dependentes (Mustard é o caso vivo). Quem pediu: o owner do Mustard (Rubens), depois de observar 60k+ tokens desperdiçados no dispatch BLOCKED da Wave 5 hoje (2026-05-23).

## Métrica de sucesso

O dispatch BLOCKED da Wave 5 que motivou esta spec seria evitado: rodar `mustard-rt run dependency-precheck --spec .claude/spec/2026-05-23-dashboard-design-system/wave-5-ui` retorna `ok: false` listando os 7 primitives ausentes em ≤2s, antes de qualquer Task ser despachada. Próximas waves de qualquer pipeline futuro com premissa stale são interceptadas pelo mesmo mecanismo.

## Não-Objetivos

- **Não** valida lógica de runtime, tipos TypeScript, regras de negócio — só presença factual de símbolo/path.
- **Não** roda compilador/linter — é grep puro, não dispatch de tool externa.
- **Não** valida APIs externas, versões de pacote, ou compatibilidade de schema — só símbolos do próprio repo.
- **Não** substitui o `Pre-EXECUTE Existence Gate` (Haiku verifica work-already-done; este verifica deps-not-yet-built — concerns ortogonais).
- **Não** valida specs Light que não declaram `## Arquivos` (sem alvo, nada para checar).
- **Não** valida specs cujo conteúdo de `## Arquivos` é só novo (todos os símbolos seriam criados pela própria spec — nada externo a validar).

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent. Fixtures vivem em `apps/rt/tests/fixtures/dependency_precheck/` (paths estáveis, não-spec — não movem com lifecycle).

- [x] AC-1: cargo build verde — Command: `cargo build -p mustard-rt`
- [x] AC-2: cargo test verde (cobre parser + whitelist + self-created exclusion + section stripping) — Command: `cargo test -p mustard-rt dependency_precheck`
- [x] AC-3: detecta primitives ausentes em fixture estilo Wave 5 — Command: `node -e "const out=require('child_process').execSync('cargo run -q -p mustard-rt -- run dependency-precheck --spec apps/rt/tests/fixtures/dependency_precheck/missing-primitives.md',{encoding:'utf8'});const r=JSON.parse(out);if(r.ok!==false){console.error('expected ok:false, got',JSON.stringify(r));process.exit(1)}const need=['EditorialBand','KpiValue','DeltaText'];const got=r.missing.map(m=>m.symbol);for(const n of need){if(!got.includes(n)){console.error('missing detection of',n,'— got:',got);process.exit(1)}}console.log('ok')"`
- [x] AC-4: fixture sem deps externas retorna ok:true — Command: `node -e "const out=require('child_process').execSync('cargo run -q -p mustard-rt -- run dependency-precheck --spec apps/rt/tests/fixtures/dependency_precheck/no-external-deps.md',{encoding:'utf8'});const r=JSON.parse(out);if(r.ok!==true){console.error('expected ok:true, got',JSON.stringify(r));process.exit(1)}console.log('ok')"`
- [x] AC-5: símbolos cujos paths estão em `## Arquivos` da própria spec NÃO viram missing (fixture self-create) — Command: `node -e "const out=require('child_process').execSync('cargo run -q -p mustard-rt -- run dependency-precheck --spec apps/rt/tests/fixtures/dependency_precheck/self-created.md',{encoding:'utf8'});const r=JSON.parse(out);const got=(r.missing||[]).map(m=>m.symbol);const selfCreated=['LocalPrimitiveA','LocalPrimitiveB'];for(const s of selfCreated){if(got.includes(s)){console.error('false positive: self-created symbol flagged:',s);process.exit(1)}}if(r.ok!==true){console.error('expected ok:true, got',JSON.stringify(r));process.exit(1)}console.log('ok')"`
- [x] AC-6: subcomando integrado no resume-flow canonical + .claude — Command: `node -e "const fs=require('fs');for(const p of ['apps/cli/templates/refs/spec/resume-flow.md','.claude/refs/spec/resume-flow.md']){const c=fs.readFileSync(p,'utf8');if(!c.includes('dependency-precheck')){console.error('missing integration in',p);process.exit(1)}}console.log('ok')"`
- [x] AC-7: subcomando integrado no feature SKILL.md canonical + .claude — Command: `node -e "const fs=require('fs');for(const p of ['apps/cli/templates/commands/mustard/feature/SKILL.md','.claude/commands/mustard/feature/SKILL.md']){const c=fs.readFileSync(p,'utf8');if(!c.includes('dependency-precheck')){console.error('missing integration in',p);process.exit(1)}}console.log('ok')"`

## Plano

## Informações da Entidade

N/A — não é entidade de domínio; é um novo `RunCmd` no enum em `apps/rt/src/run/mod.rs`, padrão idêntico aos 50+ subcomandos existentes (`exec-rewave-check`, `recipe-match`, `spec-extract` são os mais próximos como referência).

## Arquivos

- `apps/rt/src/run/dependency_precheck.rs` (novo) — parser de spec + grep + JSON output + unit tests
- `apps/rt/src/run/mod.rs` (patch) — `mod dependency_precheck;` + variant `DependencyPrecheck` no enum `RunCmd` + match arm em `dispatch()`
- `apps/rt/tests/fixtures/dependency_precheck/missing-primitives.md` (novo) — fixture com `<EditorialBand>`/`<KpiValue>`/`<DeltaText>` + imports `@/components/page` que devem disparar missing
- `apps/rt/tests/fixtures/dependency_precheck/no-external-deps.md` (novo) — fixture sem JSX/imports externos (só prose)
- `apps/rt/tests/fixtures/dependency_precheck/self-created.md` (novo) — fixture que declara `LocalPrimitiveA`/`LocalPrimitiveB` em `## Arquivos` e os referencia em JSX (must NOT flag)
- `apps/cli/templates/commands/mustard/resume/SKILL.md` (patch) — novo step `12d` (após Existence Gate, antes do dispatch)
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (patch) — step análogo na seção `EXECUTE Phase (Light scope — same session)` (antes do passo 6 "Dispatch agents")
- `.claude/commands/mustard/resume/SKILL.md` (patch) — mirror do canonical para ativação imediata neste repo
- `.claude/commands/mustard/feature/SKILL.md` (patch) — mirror análogo

## Tarefas

### rt-impl Agent (Wave 1)

- [ ] Criar `apps/rt/src/run/dependency_precheck.rs` seguindo padrão `exec_rewave_check.rs`:
  - `pub fn run(spec_arg: Option<&str>)` — entry point, sempre exit 0
  - Helpers internos:
    - `parse_spec_dependencies(text: &str) -> Vec<Dep>` — extrai símbolos JSX, imports, paths
    - `parse_self_created(text: &str) -> HashSet<String>` — extrai paths/símbolos do `## Arquivos`/`## Files`
    - `detect_subproject(files: &[String], root: &Path) -> Option<PathBuf>` — common ancestor de `apps/*/`
    - `strip_review_sections(text: &str) -> String` — remove blocos `## Concerns`/`## Decisions`/`## Notes`/`## Cobertura` (pt+en), evita falso positivo
    - `grep_symbol_in_subproject(symbol: &str, subproject: &Path) -> bool` — `export\s+(function|const|interface|type|class)\s+SYMBOL\b` recursivo em `src/**/*.{ts,tsx,rs,js,jsx,vue,svelte}`
    - Whitelist HTML/SVG primitives: `div`, `span`, `section`, `header`, `footer`, `main`, `nav`, `article`, `aside`, `p`, `a`, `ul`, `ol`, `li`, `table`, `thead`, `tbody`, `tr`, `td`, `th`, `tfoot`, `caption`, `colgroup`, `col`, `form`, `input`, `button`, `select`, `option`, `textarea`, `label`, `fieldset`, `legend`, `img`, `svg`, `path`, `g`, `rect`, `circle`, `line`, `polyline`, `polygon`, `text`, `tspan`, `defs`, `linearGradient`, `radialGradient`, `stop`, `pre`, `code`, `kbd`, `mark`, `details`, `summary`, `dialog`, `figure`, `figcaption`, `blockquote`, `br`, `hr`, `i`, `b`, `strong`, `em`, `small`, `sub`, `sup`, `time`, `var`, `template`, `slot`, `style`, `script`, `link`, `meta`, `title`, `head`, `body`, `html`
  - Regexes:
    - JSX abrindo: `<([A-Z][a-zA-Z0-9]*(?:\.[A-Z][a-zA-Z0-9]*)?)[ \t\n/>]` — captura `<Foo`, `<Foo.Bar`, `<Foo />`
    - Imports nomeados: `import\s*(?:type\s*)?\{\s*([^}]+)\s*\}\s*from\s*["']([^"']+)["']` — explode lista, ignora `as` aliases
    - Imports default: `import\s+([A-Z][A-Za-z0-9]+)\s+from\s+["']([^"']+)["']` (só capitalized — minúscula é namespace utilitário)
    - Paths em `## Arquivos`: bullet `^-\s+\`?([^\s\`]+)\`?` ou linha tipo `- apps/dashboard/src/...`
  - Output JSON (uma linha, exit 0):
    ```json
    {
      "ok": true|false,
      "spec": "<spec slug ou path>",
      "subproject": "<path relativo ou null>",
      "missing": [{"symbol": "X", "kind": "jsx|import|path", "location": "line N", "import_path": "@/components/page" (opcional)}],
      "would_be_created_here": ["X", "Y"],
      "suggested_tactical_fix_files": ["apps/dashboard/src/components/page/X/index.tsx"]
    }
    ```
  - `suggested_tactical_fix_files`: derivado do `import_path` de cada missing (`@/components/page` → `apps/{detected}/src/components/page/{Symbol}/index.tsx`); fallback para `null` se import_path ausente.
  - `ok: false` se `missing.len() > 0`; `ok: true` caso contrário (inclui specs sem `## Arquivos` ou sem símbolos detectáveis).
  - Mode override por env: `MUSTARD_DEPENDENCY_PRECHECK_MODE=block|warn|off` — `off` força `ok: true` independente do resultado; `warn` mantém output mas orquestrador trata como advisório (decisão upstream — este subcomando só reporta).
- [ ] Criar 3 fixtures em `apps/rt/tests/fixtures/dependency_precheck/`:
  - `missing-primitives.md`: header mínimo (Stage/Outcome/Lang) + `## Arquivos` com `apps/dashboard/src/pages/Demo.tsx` + corpo com `<EditorialBand>`, `<KpiValue>`, `<DeltaText>` + `import { EditorialBand, KpiValue, DeltaText } from "@/components/page";` (símbolos NÃO existem no repo Mustard fora desses fixtures — devem virar missing).
  - `no-external-deps.md`: header + `## Arquivos` com `apps/rt/src/somefile.rs` + corpo de prose puro sem JSX/imports (must ok:true).
  - `self-created.md`: header + `## Arquivos` listando `apps/rt/tests/fixtures/dependency_precheck/_fake/LocalPrimitiveA.tsx` e `_fake/LocalPrimitiveB.tsx` + corpo com `<LocalPrimitiveA>` e `<LocalPrimitiveB>` (must ok:true — self-created exclusion).
- [ ] Adicionar em `apps/rt/src/run/mod.rs`:
  - `mod dependency_precheck;` na lista de mods (ordem alfabética próximo a `diff_context`)
  - Variant `DependencyPrecheck { #[arg(long)] spec: String, #[arg(long)] subproject: Option<String> }` no enum `RunCmd` (com `///` doc-comment EN explicando o gate)
  - Match arm em `dispatch()`: `RunCmd::DependencyPrecheck { spec, subproject } => dependency_precheck::run(Some(&spec), subproject.as_deref()),` (ajustar signature se `subproject` for usado para override do auto-detect)
- [ ] Unit tests em `#[cfg(test)] mod tests` no fim de `dependency_precheck.rs`:
  - `whitelist_html_primitives_skipped` — `<div>`, `<table>` não geram missing
  - `jsx_capitalized_extracted` — `<EditorialBand>`, `<Foo.Bar>` extraídos
  - `imports_parsed` — `import { A, B as C } from "./x"` → A, B (não C)
  - `self_created_excluded` — símbolo cujo path está em `## Arquivos` não vira missing
  - `review_sections_stripped` — símbolo em `## Concerns` ou `## Decisions` ignorado
  - `subproject_detection` — `apps/dashboard/src/pages/X.tsx` em `## Arquivos` → subproject = `apps/dashboard`
- [ ] `cargo build -p mustard-rt` e `cargo test -p mustard-rt dependency_precheck` verdes.

### cli-impl Agent (Wave 1, paralelo)

- [ ] Patch `apps/cli/templates/commands/mustard/resume/SKILL.md`:
  - Inserir novo step `12d` (após `12c. Wave Plan Scope`, antes de `13. Plan waves`):
    ```markdown
    12d. **Dependency Precheck (factual gate)**: Run `mustard-rt run dependency-precheck --spec .claude/spec/{specName}/wave-{currentWave}-*/spec.md` (single-spec mode: drop the wave path). Parse JSON. If `ok: false`:
       1. Print summary inline: `BLOCKED — N símbolos ausentes: {comma list}. Sugestão: criar tactical-fix com {suggested_tactical_fix_files}.`
       2. Emit dispatch_failure event with reason `dependency-precheck-failed`.
       3. AskUserQuestion: **"Criar tactical-fix automaticamente"** / **"Investigar manualmente"** / **"Forçar dispatch mesmo assim (override)"**.
       4. Tactical-fix path: invoke `Skill(mustard:tactical-fix)` with parent=current spec, descricao derived from missing symbols.
       5. Override path: emit `pipeline.precheck_override` event with reason, then continue to step 13.
       If `ok: true` (or env `MUSTARD_DEPENDENCY_PRECHECK_MODE=off`): silent, continue to step 13.
       Skip entirely if `resumeMode === "continued"` (cached trust).
    ```
  - Atualizar a tabela INVIOLABLE RULES com a linha: `- ALWAYS run dependency-precheck before dispatch (step 12d) — block on `ok: false` unless user overrides`
- [ ] Patch `apps/cli/templates/commands/mustard/feature/SKILL.md`:
  - Na seção `EXECUTE Phase (Light scope — same session)`, inserir após o passo 4b (Structured Recipe) e antes do passo 5 (Identify relevant skills):
    ```markdown
    4c. **Dependency Precheck**: Run `mustard-rt run dependency-precheck --spec .claude/spec/{spec-name}/spec.md`. If `ok: false`, surface missing symbols + suggested tactical-fix paths via AskUserQuestion (auto-create | investigate | override). Otherwise continue.
    ```
  - Adicionar linha em Rules: `- ALWAYS run dependency-precheck before EXECUTE dispatch (Light + Extended Light) — block on missing externals`
- [ ] Espelhar os patches em `.claude/commands/mustard/resume/SKILL.md` e `.claude/commands/mustard/feature/SKILL.md` (mesmas inserções) para ativação imediata neste repo Mustard sem aguardar `mustard update`.

## Dependências

- Sem nova dependência cargo. Usa `regex` (já no `Cargo.toml` via `mustard-core`), `serde_json`, `clap`, `mustard_core::fs`.
- Não depende da tactical-fix de primitives terminar antes — esta spec é puramente sobre o gate; a tactical-fix usa o gate como consumidora futura.

## Limites

Editar/criar dentro de:
- `apps/rt/src/run/dependency_precheck.rs` (novo)
- `apps/rt/src/run/mod.rs`
- `apps/rt/tests/fixtures/dependency_precheck/{missing-primitives,no-external-deps,self-created}.md` (novos)
- `apps/cli/templates/commands/mustard/{resume,feature}/SKILL.md`
- `.claude/commands/mustard/{resume,feature}/SKILL.md`

**Não tocar:**
- Qualquer outro hook ou subcomando em `apps/rt/src/`
- `apps/rt/Cargo.toml` (nada de nova dep)
- `apps/dashboard/`, `apps/cli/src/`, `packages/core/`
- Qualquer outro arquivo em `apps/cli/templates/commands/mustard/` (só resume e feature)
- `.claude/pipeline-config.md` (gate é factual, não muda config)
- Outros SKILL.md (bugfix/qa/approve/etc. não recebem o gate — não são path de dispatch de impl)

## Cobertura

Mapeamento das críticas do usuário desta conversa:

| Crítica | Onde é endereçada |
|---|---|
| "60k tokens gastos pra chegar nesse estágio" | Métrica de sucesso + AC-3 (gate detecta o caso real em ≤2s) |
| "core do mustard" | Integrado em `/resume` step 12d + `/feature` Light step 4c (não é hook lateral) |
| "subtrair > adicionar" | UM subcomando + DOIS patches em SKILL.md; zero novo hook heurístico |
| "eliminate don't mitigate" | Sensor factual (grep), não heurística; elimina classe de erro |
| "Drive don't ask" | Override sempre disponível; default block + sugestão de tactical-fix |
| "sem permission prompts em loop" | Roda uma vez por dispatch (não por edit) |
| "AC cross-shell Windows" | Todos os AC usam `node -e` (cmd.exe-safe), zero `for`/`test`/`[ ]` |
