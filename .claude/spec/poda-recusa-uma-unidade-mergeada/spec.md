---
id: spec.poda-recusa-uma-unidade-mergeada
---

# a poda recusa uma unidade mergeada quando a base nao foi gravada

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**O que acontece hoje.** Duas unidades passaram pela porta de pull request, foram
mergeadas, e o ritual de saída recusou podar as duas. O motivo: ele procura uma
ANOTAÇÃO dizendo de qual base a unidade saiu, e as duas foram cortadas antes de
existir o conserto que grava essa anotação. A saída oferecida era reabrir a unidade
informando a base — pedir que o operador reafirme um fato sobre um trabalho que já
está dentro da base.

**Por que isso é um problema.** A anotação é uma nota SOBRE uma medição, não a
medição. Quando um branch já está contido noutro, o git PROVA que os commits estão
lá — evidência mais forte do que qualquer nota escrita no momento do corte. Recusar
com a prova disponível é a mesma forma de defeito que este produto vem removendo:
consultar um registro quando existe algo para medir.

**O que muda.** Antes de recusar por falta de anotação, o ritual pergunta ao git qual
branch já contém a unidade. Se EXATAMENTE UM contém — e ele próprio não é unidade de
alguém —, essa é a base. Vários, ou nenhum, e a pergunta continua genuinamente
aberta: aí a recusa é a resposta honesta, e não um palpite vestido de medição.

## Usuários/Stakeholders

Quem entrega uma unidade e tenta encerrá-la — hoje, qualquer unidade cortada antes de
o registro de base existir.

## Métrica de sucesso

Uma unidade comprovadamente mergeada é podada sem que o operador precise reafirmar de
onde ela saiu.

## Não-Objetivos

- Não adivinha em caso de ambiguidade: dois branches contendo a unidade mantêm a
  recusa.
- Não substitui o registro; ele continua sendo a primeira fonte quando existe.

## Critérios de Aceitação

- **AC-1** — quando uma unidade mergeada não tem base anotada e exatamente um branch a
  contém, então esse branch é a base; e quando dois a contêm, a recusa se mantém
  Command: `cargo test -p mustard-rt a_merged_unit_with_no_recorded_base_is_settled_by_containment`
  Expect: `1 passed`
- **AC-2** — o build do projeto passa verde
  Command: `cargo build --workspace`

## Arquivos

| arquivo | papel |
|---|---|
| `apps/rt/src/commands/git_settle.rs` | o ritual de saída e a medição por contenção |

## Checklist

- [ ] T1 — primeira tarefa rastreável.
