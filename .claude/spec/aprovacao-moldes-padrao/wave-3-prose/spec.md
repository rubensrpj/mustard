---
id: wave.aprovacao-moldes-padrao.3-prose
---

# wave-3-prose

## Summary

A prosa e a mensagem de recusa param de prometer um caminho que nao existe: nomeiam os gestos que realmente cunham o marcador, inclusive o r seco, e a apresentacao do plano diz de saida qual gesto conta.

## Network

- Parent: [[spec.aprovacao-moldes-padrao]]
- Depends on: [[wave.aprovacao-moldes-padrao.1-approval]]

## Tasks

- [ ] Em refs/spec/resume-loop.md secao A, o paragrafo do caminho alternativo (plan mode indisponivel) deixa de dizer apenas que a resposta cunha o mesmo marcador. Passa a dizer, de saida, ANTES de o plano ser apresentado, qual gesto conta — e a nomear o r seco como a saida de uma linha dentro do branch da unidade. Um portao que so aceita um gesto especifico precisa dizer qual e antes de pedir o gesto; hoje a recusa chega depois de o operador ja ter gasto a resposta.
- [ ] Em commands/spec.md secoes 1 e 3, registrar a forma r sem letra como gesto de aprovacao valido quando o checkout e o branch da propria unidade, com a mesma regra de prompt inteiro que ja governa {letra}r, e dizer por que ela e mais frouxa em nada: continua sendo um prompt digitado por uma pessoa.
- [ ] Em approve_spec.rs, a mensagem de recusa por marcador ausente passa a nomear os TRES gestos que cunham — aceitar o ExitPlanMode, responder o AskUserQuestion de aprovacao, e digitar /mustard:spec {letra}r ou /mustard:spec r dentro do branch da unidade — em vez de citar dois. A recusa e lida por quem acabou de descobrir que o gesto nao contou; ela e o lugar onde o gesto certo tem de estar escrito.
- [ ] Conferir os testes de prosa que ja existem (spec_flow_prose.rs, approval_refusal_explains.rs) e estender o que trava cada afirmacao, para que a prosa nao volte a prometer o que o codigo nao faz.

## Files

- `plugin/refs/spec/resume-loop.md`
- `plugin/commands/spec.md`
- `apps/rt/src/commands/spec/approve_spec.rs`
- `apps/rt/tests/spec_flow_prose.rs`
- `apps/rt/tests/approval_refusal_explains.rs`
