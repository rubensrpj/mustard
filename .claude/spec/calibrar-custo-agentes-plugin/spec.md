---
id: spec.calibrar-custo-agentes-plugin
---

# Declarar model e effort no frontmatter dos tres agentes do plugin

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Hoje todo subagente que o Mustard despacha roda no modelo da sessao. `DispatchItem`
(`apps/rt/src/commands/pipeline/dispatch_plan.rs:60`) e `AdvanceItem`
(`apps/rt/src/commands/pipeline/wave_advance.rs:79`) carregam papel, subprojeto, tipo de agente e
prompt — e nenhum campo de modelo ou de esforco de raciocinio. `model` so aparece depois, lido do
`tool_input` para telemetria e preco.

O problema: os tres agentes do plugin fazem trabalhos de dificuldade muito diferente e pagam o
mesmo. Destilar linhas de Guards e ranquear patterns e dar forma a fatos que o binario ja calculou e
entregou no prompt de dispatch. Revisao adversarial e o oposto — e o unico dos tres onde o
raciocinio e o produto.

O que muda: o frontmatter de subagente do Claude Code aceita `model:` e `effort:`. Declarar os dois
nos tres arquivos de `plugin/agents/` move a decisao de custo para onde ela e lida, sem campo novo em
Rust.

Como termina: `guards` e `patterns` rodam em sonnet com effort `low`; `review` herda o modelo da
sessao com effort `high`. E `apps/rt/tests/plugin_agents.rs` — que hoje so vigia a tabela de
placeholders do ref — passa a ser a catraca tambem dessa superficie.

## Usuários/Stakeholders

Quem despacha um scan enrich (guards + patterns) e quem despacha REVIEW/QA: o custo de raciocinio de
cada um passa a corresponder ao trabalho. Quem edita `plugin/agents/*.md` depois: a catraca avisa
quando uma chave cai ou vira um valor que o runtime ignora em silencio.

## Métrica de sucesso

Os tres arquivos declaram `model` e `effort` com valores do vocabulario aceito, e o teste que exige
isso falha contra a arvore de hoje e passa depois da mudanca.

## Não-Objetivos

- Levar modelo/effort aos papeis que caem em agentes built-in (`explore` -> `Explore`, `impl` ->
  `general-purpose`): exigiria campo novo em `DispatchItem`/`AdvanceItem`. Fica para depois.
- Fixar no teste QUAL modelo cada agente usa. A catraca exige declaracao valida, nao uma afinacao —
  retunar sonnet/haiku e decisao de operacao, nao regressao.

## Critérios de Aceitação

- **AC-1** — when `plugin/agents/*.md` e lido pelo teste, then todo agente declara `model` e `effort`
  no frontmatter
  Command: `cargo test -p mustard-rt --test plugin_agents shipped_agents_declare_model_and_effort`
  Control: `cargo test -p mustard-rt --test plugin_agents agent_prompt_ref_documents_every_placeholder`
- **AC-2** — when os tres arquivos sao lidos, then a calibracao declarada e a decidida: `review`
  herda o modelo da sessao com effort `high`, `guards` e `patterns` rodam mais barato com effort
  `low`
  Command: `grep -qx 'model: inherit' plugin/agents/mustard-review.md && grep -qx 'effort: high' plugin/agents/mustard-review.md && grep -qx 'effort: low' plugin/agents/mustard-guards.md && grep -qx 'effort: low' plugin/agents/mustard-patterns.md`
  Control: `grep -qx 'name: mustard-review' plugin/agents/mustard-review.md`
- **AC-3** — o build do projeto passa verde
  Command: `cargo build -p mustard-rt`

## Checklist

- [x] T1 — escrever as duas funcoes de teste em `apps/rt/tests/plugin_agents.rs` e ver AC-1 vermelho.
- [x] T2 — declarar `model: sonnet` + `effort: low` em `mustard-guards.md` e `mustard-patterns.md`.
- [x] T3 — declarar `model: inherit` + `effort: high` em `mustard-review.md`.
- [x] T4 — rodar AC-1, AC-2 e AC-3 verdes.

## Definitions

- **model: (frontmatter de subagente)** — Chave lida pelo Claude Code, nao pelo Mustard: fixa em qual modelo aquele subagente roda. Aceita apelido (opus/sonnet/haiku/fable), id completo, ou `inherit` — herdar o modelo da sessao, que e o default quando a chave esta ausente.
- **effort: (frontmatter de subagente)** — Chave lida pelo Claude Code: o orcamento de raciocinio daquele subagente. Valores: low, medium, high, xhigh, max.
- **catraca (ratchet)** — Teste que trava uma superficie que nenhum compilador defende. Os arquivos de plugin/agents/*.md sao lidos pelo Claude Code em runtime: derrubar uma chave nao quebra build nenhum, so muda o comportamento em silencio em toda instalacao. O teste e o unico aviso.

## Decisions

- guards e patterns declaram `model: sonnet` + `effort: low`.
  Reason: Os dois fazem extracao estruturada sobre fatos que o binario ja computou e entrega no prompt de dispatch. Sonnet e muito mais barato que o Opus da sessao. Nao se desceu a haiku porque o que esses dois escrevem fica versionado: uma linha de Guards marcada [critical] vira Deny automatico no gate de edicao, e um molde {role}-pattern e copiado por todo implementador seguinte — um molde fraco custa depois, em codigo que nasce na forma errada.
- review declara `model: inherit` + `effort: high`.
  Reason: E verificacao adversarial: e o unico dos tres onde o raciocinio e o produto. Herdar a sessao garante que a revisao nunca seja mais fraca que quem implementou; o effort alto e o que o papel pede.
- `inherit` fica declarado explicitamente em review, mesmo sendo o default.
  Reason: Com os tres arquivos declarando model, a ausencia da chave passa a ser ambigua entre `esqueceram` e `queriam a sessao`. Declarado, o teste pode exigir a chave nos tres sem excecao.
- A catraca valida tambem o VALOR, nao so a presenca da chave.
  Reason: Um `effort: fast` e aceito pelo arquivo e ignorado em runtime — exatamente o modo de falha silenciosa que este arquivo de teste existe para pegar. Presenca sem vocabulario deixaria passar o erro de digitacao.
- Fora de escopo: levar model/effort aos papeis que caem em agentes built-in (explore -> Explore, impl -> general-purpose).
  Reason: Exigiria campo novo em DispatchItem/AdvanceItem no Rust. Fica para depois, se esta fase se pagar.

## Evidence

- DispatchItem nao tem campo de modelo nem de effort — o subagente despachado herda o modelo da sessao.
  Evidence: `apps/rt/src/commands/pipeline/dispatch_plan.rs:60`
- AdvanceItem tambem nao tem campo de modelo nem de effort.
  Evidence: `apps/rt/src/commands/pipeline/wave_advance.rs:79`
- plugin_agents.rs hoje nao abre nenhum arquivo de plugin/agents/*.md: so vigia a tabela de placeholders do ref agent-prompt. A asserção nova e uma funcao de teste nova.
  Evidence: `apps/rt/tests/plugin_agents.rs`
- Nenhum codigo Rust le o frontmatter de plugin/agents/*.md; quem le e o Claude Code. Nao ha copia embutida no binario nem parity test a atualizar.
  Evidence: `plugin/agents`
