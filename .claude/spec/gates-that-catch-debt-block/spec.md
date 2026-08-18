---
id: spec.gates-that-catch-debt-block
---

# Gates that catch real debt must block instead of warning, and the house style must explain without jargon

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Gates that catch real debt must block instead of warning, and the house style must explain without jargon.

Por que agora.

## Usuários/Stakeholders

Quem se beneficia.

## Métrica de sucesso

Métrica de sucesso.

## Não-Objetivos

O que fica de fora.

## Critérios de Aceitação

- **AC-1** — quando um documento passa do orçamento de linhas, então a escrita é RECUSADA sem que ninguém precise configurar nada — o padrão bloqueia.
  Command: `cargo test -p mustard-rt --test gates_block_debt ac1_documento_acima_do_orcamento_e_recusado_por_padrao` Expect: `[1-9][0-9]* passed`
- **AC-2** — quando um agente escreve um arquivo que a spec não listou, então ele é AVISADO e o trabalho segue: o alvo aqui é plano incompleto, não dívida, e travá-lo é atrito que não protege nada.
  Command: `cargo test -p mustard-rt --test gates_block_debt ac2_arquivo_fora_do_plano_avisa_e_deixa_seguir` Expect: `[1-9][0-9]* passed`
- **AC-3** — quando `/mustard:spec` é chamado sem argumento de DENTRO da branch de uma unidade, então a prosa manda ir direto para aquela unidade, sem tabela e sem pergunta.
  Command: `cargo test -p mustard-rt --test gates_block_debt ac3_picker_dentro_da_branch_nao_pergunta` Expect: `[1-9][0-9]* passed`
- **AC-4** — o build do projeto passa verde
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — primeira tarefa rastreável.