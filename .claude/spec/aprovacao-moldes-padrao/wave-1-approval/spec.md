---
id: wave.aprovacao-moldes-padrao.1-approval
---

# wave-1-approval

## Summary

A fila que resolve QUAL spec a porta de aprovacao decide passa a parar no primeiro degrau que satisfaz o fato 1, o recuo passa a falar, e /mustard:spec r seco dentro do branch da unidade vira gesto de aprovacao.

## Network

- Parent: [[spec.aprovacao-moldes-padrao]]

## Tasks

- [ ] Em approval_marker_observer.rs, trocar a cadeia or_else de active_spec por uma que so aceita um candidato que satisfaz o fato 1 (is_full_plan && !already_approved). A ordem dos degraus fica: vinculo de sessao, current_spec, unique_pending_full_plan. Um degrau que responde algo que NAO satisfaz o fato 1 deixa de encerrar a busca — hoje encerra, e e por isso que unique_pending_full_plan nunca e alcancado quando .pipeline-states/ carrega um palpite obsoleto.
- [ ] Manter unique_pending_full_plan fail-closed em zero e em mais de um candidato: o degrau existe para o caso inequivoco e nao pode virar um chute quando ha varios planos pendentes.
- [ ] Fazer o recuo por fato 1 FALAR. Hoje o observe retorna antes de unrecognised_answer_notice, entao uma aprovacao legitima que nao encontra spec sai sem uma linha de stderr. Emitir um aviso APENAS quando a resposta selecionada ja e uma aprovacao afirmativa e oferecida (fatos 2 e 3 valendo) e o fato 1 e que falhou — nomeando o que falhou: nenhuma spec resolvivel, spec resolvida fora da janela full+Plan, ou ja aprovada. Este observador ve TODA AskUserQuestion da sessao, entao qualquer aviso mais largo que isso vira ruido em toda pergunta.
- [ ] Em picker_approval_observer.rs, reconhecer o r SECO (/mustard:spec r) como terceira forma, ao lado de {letra}r. A regra de prompt inteiro nao afrouxa: a comparacao continua sendo com o prompt todo, porque uma regra que casasse com trecho deixaria uma mensagem que apenas cita a forma cunhar o marcador.
- [ ] O r seco resolve a spec pelo BRANCH DO CHECKOUT, nunca pela sessao — mesmo principio que faz {letra}r resolver pela letra: o gesto e que nomeia a spec, e honrar a sessao cunharia um gesto genuino contra a spec errada. Reaproveitar slug_of_work_branch de resume_bootstrap/mode_decision.rs (expor o minimo necessario) em vez de re-soletrar a leitura do nome do branch.
- [ ] Fora do branch de uma unidade — base de integracao, HEAD solto, diretorio que nao e repositorio — o r seco nao cunha nada e nao decide nada.
- [ ] Testes cobrindo: o palpite obsoleto que nao sombreia mais o plano pendente; o recuo por fato 1 que nomeia a razao; o r seco que cunha dentro do branch da unidade e nao cunha fora dele; e a metade inversa de sempre, o mesmo texto chegando como relatorio de subagente nao cunhando nada.

## Files

- `apps/rt/src/hooks/observe/approval_marker_observer.rs`
- `apps/rt/src/hooks/observe/picker_approval_observer.rs`
- `apps/rt/src/hooks/observe/plan_approval_observer.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs`
