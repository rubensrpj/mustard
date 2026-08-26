---
id: spec.orcamento-por-evento-quebra-no-windows
---

# o teste de orcamento por evento quebra no windows por causa do CRLF

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**O que acontece hoje.** A trava que mede o orçamento por evento passou no CI do
Linux e falhou no do Windows. O motivo é o fim de linha: um checkout no Windows
entrega os arquivos com CRLF, e cada linha custa um caractere a mais — 34 no maior
injetável. O orçamento deixava 5 caracteres de folga, então 34 a mais estouram.

**Por que isso é um problema.** Não é o número que está errado — é a forma da falha.
Uma trava verde onde foi escrita e vermelha onde ninguém está olhando é a pior forma
que uma trava pode ter: dá confiança na plataforma do autor e cobra na do usuário.

**O que muda.** A reserva passa a ser dimensionada pela plataforma que paga mais caro,
não pela que paga menos, e o censo cede espaço para isso: o teto de linhas cai de 24
para 16, ainda mais que o dobro do que este repositório tem. A margem no pior caso
passa de 5 para 371 caracteres.

## Usuários/Stakeholders

Quem desenvolve em Windows — e qualquer pessoa cujo CI rode lá.

## Métrica de sucesso

A mesma trava dá o mesmo veredito nas duas plataformas.

## Não-Objetivos

- Não normaliza o fim de linha na contagem: o texto injetado é o do checkout, e
  esconder o custo real do CRLF seria medir uma coisa e afirmar outra.

## Critérios de Aceitação

- **AC-1** — quando os injetáveis são medidos com o fim de linha do pior caso, então a
  soma por evento cabe no orçamento com margem
  Command: `cargo test -p mustard-cli --test template_budget`
  Expect: `3 passed`
- **AC-2** — a suíte do projeto passa inteira
  Command: `cargo test --workspace`

## Arquivos

| arquivo | papel |
|---|---|
| `apps/cli/tests/template_budget.rs` | a reserva dimensionada pelo pior caso |
| `apps/rt/src/commands/orient.rs` | o teto de linhas do censo |

## Checklist

- [ ] T1 — primeira tarefa rastreável.
