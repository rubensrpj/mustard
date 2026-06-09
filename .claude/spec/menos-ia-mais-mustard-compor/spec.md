# Menos IA mais mustard: compor fases do pipeline em comandos deterministicos e fechar gotchas de orquestracao

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Auditoria da sessão de 2026-06-09 (3 specs Full executadas ao vivo) mediu onde o orquestrador-IA atua como mero relay de comandos determinísticos: cada spec Full custou ~12-15 round-trips de comandos avulsos no contexto mais caro do sistema, e três gotchas mecanizáveis custaram ciclos de debug de IA. Esta spec converte esses pontos em código Rust determinístico — menos IA, mais mustard — sem tocar no que é juízo legítimo da IA (lapidação de PRD, review adversarial, decisões de design).

Itens (mapeados com evidência da sessão):
1. **Composições de fase** (precedente: `approve-spec` já compõe 4 passos): `plan-materialize` (spec-draft já feito + wave-scaffold + analyze-validation + emit-pipeline scope + emit-phase), `wave-advance` (dispatch-plan + prompts do nível já renderizados inline), `close-pipeline` (verificação de review.results + qa-run + complete-spec + pipeline-summary). Composição = chamada direta às funções internas existentes; NUNCA duplicar lógica nem shellar para si mesmo.
2. **`dispatch-plan` para spec única** retorna `[]` hoje — TFs exigem orquestração manual com `--task-text`. Deve emitir plano de 1 item lendo o spec.
3. **Validação de formato de AC** — verificado: `analyze-validation` não valida AC; o parser do `qa-run` exige `**AC-N** —` + linha `Command:` e degrada para `overall=skip` silencioso. A validação deve REUSAR o mesmo parser do qa-run (fonte única) e emitir WARN quando a seção existe mas zero AC parseiam.
4. **`hardcode-gate`** — novo comando: verifica que o diff/working-tree de `apps/scan/src` e `packages/core/src` (arquivos `.rs`, código de produção) não introduz literais declarados nos registries de dados (`manifest_deps`/`path_markers`/`code_signatures` do `stacks.toml` — strings distintivas; nomes curtos tipo `next` ficam FORA do gate por colidirem com identificadores comuns). Substitui o "IA roda grep e interpreta" dos AC anti-hardcode.
5. **`wave-dependency` integrado** — o comando existe e não é consultado; a prosa do PLAN deve mandar validar/derivar `depends_on` do plano com ele antes do scaffold.
6. **Normalização de checkbox no `wave-scaffold`** — defeito medido: plano com tasks já prefixadas `- [ ]` gera `- [ ] - [ ]` no spec da wave; o scaffold deve fazer strip de prefixo existente antes de prefixar.
7. **Seed `aspnet` (ecossistema dotnet/nuget) no `stacks.toml`** — o backend do sialia é C#/.NET e não tem entry; 1 bloco de dado (deps NuGet distintivas + markers + assinaturas verificadas contra código ASP.NET real) com retorno imediato no repo principal do usuário.

Âncoras reais (da sessão, verificadas):
- `apps/rt/src/commands/mod.rs` — enum `RunCmd` + `dispatch()` (regra do crate: subcomando novo exige os DOIS registros).
- `apps/rt/src/commands/spec/{spec_draft,wave_scaffold,analyze_validation,dispatch_plan,scan_spec}.rs`, `apps/rt/src/commands/agent/agent_prompt_render.rs`, `apps/rt/src/commands/qa/qa_run*.rs`, `apps/rt/src/commands/pipeline/{approve_spec,complete_spec,pipeline_summary}.rs` (localizar os caminhos exatos) — funções a compor.
- `packages/core/src/domain/spec/contract.rs:117-124` — render do checklist (`- [ ] {label}`); a materialização das tasks da wave.
- `packages/core/src/domain/vocabulary/stacks.toml` — registro (4ª entry).
- `.claude/commands/mustard/feature/SKILL.md` + `.claude/refs/spec/resume-flow.md` + cópias em `apps/cli/templates/` — prosa que passa a usar os comandos compostos.

## Usuários/Stakeholders

O orquestrador (menos round-trips por fase, menos contexto queimado), TFs (dispatch sem orquestração manual), autores de spec (AC inválido detectado no PLAN e não no QA), e qualquer mudança data-driven futura (gate anti-hardcode reutilizável).

## Métrica de sucesso

Uma spec Full atravessa PLAN→EXECUTE→CLOSE com ~4 comandos compostos no lugar de ~12-15 avulsos; um TF é despachável via `dispatch-plan`; um spec com AC mal-formatado recebe WARN no `analyze-validation`; o gate anti-hardcode roda como comando único; o scan do sialia detecta a stack do backend .NET.

## Não-Objetivos

- Remover a IA de onde ela é juízo legítimo (lapidação, review adversarial, decisões de design).
- Reescrever o fluxo das SKILLs além de apontar para os comandos compostos (a estrutura ANALYZE→PLAN→EXECUTE→CLOSE não muda).
- Auto-aprovação: `plan-materialize` NÃO aprova; a aprovação continua humana via `/spec` (scope_guard intacto).
- Tocar o dashboard.

## Critérios de Aceitação

- **AC-1** — Composições de fase: `plan-materialize`, `wave-advance` e `close-pipeline` existem (enum + dispatch), compõem as funções existentes sem duplicação, saída JSON determinística; testes cobrem o caminho feliz e o degradado de cada um
  Command: `cargo test -p mustard-rt composite`
- **AC-2** — `dispatch-plan` de spec única emite plano de 1 item (role impl, subproject inferido do spec, prompt_cmd com --task-text derivado) em vez de lista vazia
  Command: `cargo test -p mustard-rt dispatch_single_spec`
- **AC-3** — `analyze-validation` valida AC com o MESMO parser do qa-run: seção presente + zero AC parseáveis → WARN `unparseable-ac`
  Command: `cargo test -p mustard-rt ac_format_validation`
- **AC-4** — `hardcode-gate` detecta literal de registry introduzido em `.rs` de produção e passa limpo no working tree atual
  Command: `cargo test -p mustard-rt hardcode_gate`
- **AC-5** — `wave-scaffold` normaliza prefixo de checkbox (task já prefixada não duplica)
  Command: `cargo test -p mustard-rt checkbox_normalize`
- **AC-6** — Registro com 4ª stack `aspnet` (ecossistema dotnet) e parse verde
  Command: `cargo test -p mustard-core stacks_registry_parses`
- **AC-7** — Prosa atualizada: SKILL do /feature e refs do /spec citam `plan-materialize`/`wave-advance`/`close-pipeline`/`wave-dependency` (local + templates)
  Command: `rg -l "wave-advance" .claude/commands/mustard apps/cli/templates/commands/mustard .claude/refs apps/cli/templates/refs`
- **AC-8** — Suíte completa do rt verde
  Command: `cargo test -p mustard-rt`

<!-- PLAN -->

## Arquivos

Panorâmico (detalhe por onda nas sub-specs):

**Onda 1 — core (dados + normalização):**
- `packages/core/src/domain/vocabulary/stacks.toml` — 4ª entry `aspnet` (deps NuGet distintivas, markers, assinaturas verificadas).
- `packages/core/src/domain/spec/contract.rs` (e/ou o ponto de materialização das tasks da wave) — strip de prefixo `- [ ]`/`- [x]` antes de prefixar.

**Onda 2 — rt (gotchas determinísticos):**
- `apps/rt/src/commands/spec/analyze_validation.rs` — validação de AC reusando o parser do qa-run (fonte única; expor/reusar de onde ele vive).
- `apps/rt/src/commands/spec/dispatch_plan.rs` — plano de 1 item para spec única (não-wave).
- `apps/rt/src/commands/` + `mod.rs` — novo `hardcode-gate` (enum + dispatch), lendo os literais do registry embutido.

**Onda 3 — rt (composições de fase):**
- `apps/rt/src/commands/` + `mod.rs` — `plan-materialize`, `wave-advance`, `close-pipeline` (enum + dispatch ×3), compondo as funções internas existentes (wave_scaffold, analyze_validation, emits, dispatch_plan, agent_prompt_render, qa_run, complete_spec, pipeline_summary) por chamada direta — sem duplicar lógica, sem shellar para o próprio binário, saída JSON byte-estável.

**Onda 4 — prosa (local + templates):**
- `.claude/commands/mustard/feature/SKILL.md` + `apps/cli/templates/commands/mustard/feature/SKILL.md` — PLAN usa `plan-materialize` + valida `depends_on` com `wave-dependency`.
- `.claude/refs/spec/resume-flow.md` + `apps/cli/templates/refs/spec/resume-flow.md` — EXEC usa `wave-advance`; CLOSE usa `close-pipeline`.

## Dependências

- **Onda 1 (core)** e **Onda 2 (rt gotchas)** são independentes — nível 0, paralelas.
- **Onda 3 (composições)** depende da 2 (`wave-advance` embute o dispatch-plan de spec única; `plan-materialize` embute o analyze-validation com checagem de AC).
- **Onda 4 (prosa)** depende da 3 (cita os comandos que precisam existir).

## Limites

IN: comandos compostos, dispatch de spec única, validação de AC, hardcode-gate, normalização de checkbox, seed aspnet, prosa apontando para os compostos.
OUT: auto-aprovação (a aprovação humana via /spec permanece); reescrita do fluxo das SKILLs; dashboard; remover IA de lapidação/review/design.