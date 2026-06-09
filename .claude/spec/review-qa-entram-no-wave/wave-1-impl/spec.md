# wave-1-impl

## Resumo

RT: rodada de review no wave-advance + fallback de TASK no render

## Rede

- Pai: [[review-qa-entram-no-wave]]

## Tarefas

- [ ] Ler apps/rt/src/commands/pipeline/wave_advance.rs e dispatch_plan.rs: hoje so emitem itens role=impl; quando todas as ondas impl tem pipeline.wave.complete, devolvem []. Adicionar a rodada de review ANTES do []: 1 item {role: review, subagent_type: mustard-review, subproject} por subprojeto distinto tocado pelas ondas (ordem deterministica, ex. alfabetica), com prompt renderizado pelo miolo do agent-prompt-render (role review). Sinal de 'review ja feita' p/ nao repetir a rodada: eventos review.result existentes do spec (1 por subprojeto) — documentar a semantica de re-invocacao.
- [ ] Em apps/rt/src/commands/agent/agent_prompt_render.rs, read_task_steps: quando o spec operacional nao tem secao Tasks (## Tasks / ## Tarefas), fazer fallback deterministico para o conteudo de ## Contexto + ## Criterios de Aceitacao do spec (com cabecalho indicando a origem), em vez de TASK vazio. Spec COM Tasks segue byte-identico.
- [ ] Testes: wave_advance_review_* (rodada emitida pos-impl-completas; nao re-emite com review.result presente; ordem deterministica; spec unica tambem ganha review) e task_fallback_* (TF sem Tasks -> TASK nao-vazio com Contexto+AC; spec com Tasks identico ao atual).
- [ ] Rodar cargo test -p mustard-rt completo e reportar.

## Arquivos

- `apps/rt/src/commands/pipeline/wave_advance.rs`
- `apps/rt/src/commands/pipeline/dispatch_plan.rs`
- `apps/rt/src/commands/agent/agent_prompt_render.rs`
