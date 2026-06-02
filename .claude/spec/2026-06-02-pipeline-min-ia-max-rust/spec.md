# Pipeline min-IA/max-Rust: mover lógica determinística pro mustard-rt, remover seleção de modelo, auditar todos os prompts

<!-- drafter:tone=didactic -->

<!-- PRD -->

## Contexto

Auditoria da fronteira prompt-vs-Rust (critério do `pipeline-config.md § Skill Discovery Heuristic`: estado→Rust, decisão-humana→LLM) achou lógica determinística ainda decidida por prompt. Decisão do usuário: **fazer todos os candidatos + revisar cada prompt usado pelo mustard**. Conserto mora no tool (não em memória, não na spec gerada) — [[feedback-mustard-fix-tool-not-spec]].

Decisões fixadas:
- **Seleção de modelo deixa de existir.** Agentes despachados **sempre herdam o modelo da sessão principal** — sem tabela de roteamento, sem sonnet-vs-opus por escopo, sem coluna "Modelo" no wave-plan. O `model_routing_gate` é **removido** (era o mecanismo de seleção).
- Lógica determinística (sequência de eventos, classificação de escopo, merge de waves, descoberta de comandos) migra pra comandos `mustard-rt run …`.

## Usuários/Stakeholders

Operadores do pipeline `/feature`,`/spec`,`/maint`. Ganham: menos decisão/erro do LLM, zero drift prosa-vs-código, custo de token menor, comportamento auditável e testável.

## Métrica de sucesso

- Nenhum prompt escolhe modelo; agentes herdam a sessão; `model_routing_gate` e a coluna "Modelo" eliminados.
- Sequência de approve e o merge de "rejeitar decomposição" viram comandos determinísticos.
- Classificação de escopo (Light/Extended-Light/Full) retornada por código.
- `/maint deps` chama `maint-deps` em vez de o LLM ler a tabela de Agents.
- Todo prompt (SKILLs + refs + template de agente) revisado: drift corrigido, fronteira prompt-vs-Rust explícita.

## Não-Objetivos

- NÃO mexer no julgamento real do LLM (elicitação, lapidação, precedente-vs-net-new, captura de memória, classificação de escalonamento por texto livre, redação de AC/commit).
- NÃO reescrever specs já geradas.

## Critérios de Aceitação

- [ ] AC-1: nao existe mais selecao de modelo no codigo nem prosa (grep limpo) — Command: `rg -n "model_routing_gate|Model Selection|Modelo \||wave_model|waveModel" apps packages .claude/pipeline-config.md && echo NONE_OK || echo CHECK`
- [ ] AC-2: workspace compila, testa e linta verde apos remover o gate de modelo — Command: `cargo test -p mustard-rt && cargo clippy -p mustard-rt`
- [ ] AC-3: existe comando approve-spec que emite a sequencia de aprovacao deterministicamente — Command: `cargo test -p mustard-rt -- approve_spec`
- [ ] AC-4: existe comando wave-collapse que faz o merge para single-wave (Full) / single-spec (Light) — Command: `cargo test -p mustard-rt -- wave_collapse`
- [ ] AC-5: scope-classify retorna light/extended-light/full deterministico dos sinais — Command: `cargo test -p mustard-rt -- scope_classify`
- [ ] AC-6: SKILL do maint delega ao maint-deps (sem leitura manual da tabela de Agents) — Command: `rg -n "maint-deps" apps/cli/templates/commands/mustard/maint/SKILL.md`
- [ ] AC-7: workspace inteiro verde — Command: `cargo test && cargo clippy --all-targets`

<!-- PLAN -->

## Entidades

Modelo do pipeline: escopo, decomposição, sequências de eventos, seleção de modelo (a remover). Sem entidade de domínio nova.

## Arquivos

**Wave 1 — remover seleção de modelo (código + config + prosa):**
- `apps/rt/src/hooks/.../model_routing_gate*` — DELETAR módulo + testes; tirar do registry de hooks.
- `apps/rt/src/commands/agent/agent_prompt_render.rs` + `agent_prompt_template.md` — remover `{wave_model}`/`read_wave_model`.
- `apps/rt/src/commands/pipeline/resume_bootstrap/` (`mod.rs`,`wave_progress.rs`) — remover `waveModel`/`extract_wave_model`/`read_wave_model`.
- `apps/rt/src/commands/wave/wave_scaffold.rs` — remover a coluna "Modelo" do wave-plan.
- `.claude/pipeline-config.md` + template — deletar `§ Model Selection`.
- `apps/cli/templates/refs/spec/approve-only-flow.md` (+ `.claude/`) — deletar o passo "Model selection".
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (+ `.claude/`) — remover menções a escolha de modelo.
- `settings.json` (template + `.claude/`) — remover a entrada de hook do `model_routing_gate`.

**Wave 2 — comando approve-spec:** novo `apps/rt/src/commands/spec/approve_spec.rs` (emite `pipeline.stage:Plan`→`status:approved`→`stage:Execute`; patch wave-1 meta quando `--wave-plan`); registrar em `commands/mod.rs`; cabear `approve-only-flow.md` (ref+template) como relay. Memory-decision (5b) fica fora (julgamento).

**Wave 3 — comando wave-collapse:** novo `apps/rt/src/commands/wave/wave_collapse.rs` (`--mode full|light`: Full→1 wave/orquestrador; Light→single spec; merge de `## Arquivos/Tarefas/Limites` via `spec_sections::is_heading`, deleta dirs surplus, patcha sidecars); registrar; cabear o branch "reject decomposition" do `approve-only-flow.md`.

**Wave 4 — scope-classify:** estender `scope_decompose.rs` (ou novo `scope_classify`) p/ retornar `{scope: light|extended-light|full}` dos sinais; adicionar sinal `sliceMatchCount` no digest `feature`; cabear `feature/SKILL.md` p/ chamar e escrever em `spec-draft --scope`.

**Wave 5 — maint-deps wiring:** `apps/cli/templates/commands/mustard/maint/SKILL.md` (+ `.claude/`) — chamar `mustard-rt run maint-deps` e imprimir verbatim, em vez de o LLM ler a tabela de Agents.

**Wave 6 — auditar + corrigir cada prompt:** varredura de TODOS os SKILLs (`apps/cli/templates/commands/mustard/*/SKILL.md` + `.claude/commands/`), refs (`apps/cli/templates/refs/`, `.claude/refs/`) e o `agent_prompt_template.md`: drift prosa-vs-código, instruções obsoletas, fronteira prompt-vs-Rust, consistência template↔`.claude/`. Corrigir o que for determinístico/errado; listar o que é julgamento legítimo.

## Tarefas

- T1 — Wave 1: remover seleção de modelo em todas as superfícies; build/clippy verde.
- T2 — Wave 2: `approve-spec` + testes + cabeamento.
- T3 — Wave 3: `wave-collapse` + testes + cabeamento.
- T4 — Wave 4: `scope-classify` + `sliceMatchCount` + cabeamento.
- T5 — Wave 5: `/maint` delega ao `maint-deps`.
- T6 — Wave 6: auditar e corrigir todos os prompts.

## Dependências

W1 primeiro (toca prosa compartilhada). W2→W3 sequenciais (ambos editam `approve-only-flow.md` + `mod.rs`). W4, W5 independentes. W6 por último (revisa o estado final pós-W1..W5).

## Limites

IN: `apps/rt`, `apps/cli/templates`, `.claude/refs`, `.claude/pipeline-config.md`, `settings.json`, SKILLs. Testes dos comandos novos. OUT: julgamento do LLM; specs já geradas; dashboard.

<!-- wikilinks-footer-start -->
- [feedback-mustard-fix-tool-not-spec](?) ⚠ unresolved
<!-- wikilinks-footer-end -->