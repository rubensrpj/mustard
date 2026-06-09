# Review e QA entram no wave-advance e render ganha fallback de TASK para specs sem secao Tasks

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**Spec PLANEJADA — aguarda a validação real do fluxo de composições (uma feature de verdade atravessando plan-materialize → /spec → wave-advance → close-pipeline, idealmente no sialia) antes de ser aprovada.** Se a run real não doer onde esta spec aposta, reavaliar o escopo antes de executar.

Dois resíduos medidos na sessão de 2026-06-09 (a mesma que entregou as composições):

1. **REVIEW/QA fora do `wave-advance`.** O `dispatch-plan` (e portanto o `wave-advance`) só emite ondas `impl`. Resultado medido: o orquestrador-IA **hand-craftou os prompts de review adversarial o dia inteiro** — violando na prática a regra "NEVER hand-craft" — porque não existe item de dispatch para os papéis `review`/`qa`, embora o `agent-prompt-render` os suporte e os subagent_types travados existam (`mustard-review`). Proposta: quando todas as ondas `impl` de um spec estiverem `pipeline.wave.complete`, o `wave-advance` emite uma **rodada de review** — um item `role=review`, `subagent_type=mustard-review` por subprojeto distinto tocado — antes de devolver `[]`. O QA permanece no `close-pipeline` (já composto).

2. **TASK vazio no caminho de spec única.** Medido no dispatch do TF `2026-06-09-qa-run-pula-ac` via `wave-advance`: o item veio com o prompt renderizado mas o bloco `## TASK` **vazio**, porque TFs não têm `## Tasks` e o `read_task_steps` do render não tem fallback — obrigando o workaround manual `--task-text` (exatamente o relay que o caminho de spec única veio eliminar). Proposta: no `agent-prompt-render`, quando o spec não tem seção Tasks, o TASK cai para o conteúdo de `## Contexto` + `## Critérios de Aceitação` do spec (ou instrução de ler o spec.md), de forma determinística.

Âncoras (verificadas): `apps/rt/src/commands/pipeline/{dispatch_plan,wave_advance}.rs` (emissão de itens/níveis), `apps/rt/src/commands/agent/agent_prompt_render.rs` (`read_task_steps` e o render por papel), `apps/rt/src/commands/review/review_prefetch.rs` (o que já existe de suporte a review), prosa `refs/spec/resume-flow.md` (a rodada de review hoje é manual).

## Usuários/Stakeholders

O orquestrador (REVIEW deixa de ser o último prompt hand-crafted do pipeline) e TFs/specs únicas (dispatch completo sem workaround).

## Métrica de sucesso

Numa run Full real: nenhum prompt de agente é escrito à mão em fase alguma (EXECUTE e REVIEW saem do `wave-advance`; QA do `close-pipeline`). Num TF: o item do `wave-advance` carrega TASK não-vazio derivado do spec.

## Não-Objetivos

- Mudar o conteúdo/qualidade dos prompts de review (o template do render já cobre; trata-se de DISPATCH).
- Automatizar o julgamento da review (continua IA — é onde ela mais paga).
- Tocar o QA (já composto no close-pipeline).

## Critérios de Aceitação

- **AC-1** — wave-advance emite rodada de review (1 item mustard-review por subprojeto tocado) quando todas as ondas impl estão completas, antes de devolver lista vazia
  Command: `cargo test -p mustard-rt wave_advance_review`
- **AC-2** — Render com fallback de TASK: spec sem seção Tasks produz TASK não-vazio derivado de Contexto+AC; spec com Tasks segue idêntico
  Command: `cargo test -p mustard-rt task_fallback`
- **AC-3** — Suíte do rt verde
  Command: `cargo test -p mustard-rt pipeline`

<!-- PLAN -->

## Arquivos

- `apps/rt/src/commands/pipeline/dispatch_plan.rs` / `wave_advance.rs` — rodada de review pós-impl (1 item `mustard-review` por subprojeto tocado; ordem determinística).
- `apps/rt/src/commands/agent/agent_prompt_render.rs` — fallback de TASK (Contexto+AC) quando o spec não tem `## Tasks`.
- `apps/cli/templates/refs/spec/resume-flow.md` + espelho local — a rodada de review vira parte do loop do `wave-advance` (prosa).

## Dependências

Onda 1 (rt: dispatch+render) → Onda 2 (prosa). Lineares.

## Limites

IN: emissão de itens de review no wave-advance; fallback determinístico de TASK; prosa do resume.
OUT: conteúdo/julgamento da review (continua IA); QA (já no close-pipeline); dashboard.