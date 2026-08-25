---
id: spec.tom-configurado-nao-chega-conversa
---

# O tom declarado no mustard.json nao chega a conversa: ele e lido num lugar so e entra apenas nos pedidos enviados aos agentes que escrevem arquivos

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O `mustard.json` deste projeto declara `"tone": "didactic"` desde antes desta sessão. Esse campo é lido em **um lugar só**: o renderizador que monta o pedido enviado aos agentes que escrevem arquivos. A conversa com o operador não recebe nada.

O efeito apareceu duas vezes na mesma sessão: o operador não entendeu explicações escritas com jargão não traduzido, siglas sem o nome por extenso e caminhos longos de raciocínio. E ele apontou o certo — *"no mustard.json já é solicitado o tipo de interação, acredito que isso não é aplicado de forma correta"*.

Estava mesmo. A configuração existia, o produto sabia dela, e ela não chegava até onde importava.

**Escrever simples dependia de lembrança em vez de configuração** — e lembrança falha, como falhou.

## Usuários/Stakeholders

Quem declara um tom no `mustard.json` e hoje só o vê aplicado nos arquivos que os agentes escrevem, nunca na conversa.

## Métrica de sucesso

Num projeto que declara `tone: didactic`, a regra de escrita acompanha **cada mensagem** do operador. Num projeto que não declarou nada, nada é injetado.

## Não-Objetivos

- **Entregar no início da sessão.** Aquela ocasião já está em 8.594 de um teto de 10.000 caracteres; a revisão anterior mediu 9.327 com este texto lá, folga menor que o próprio parágrafo. E a entrega por mensagem a torna redundante.
- **Deixar o texto da regra editável no `mustard.json`.** `tone` é lista fechada: o operador escolhe qual das três palavras, não escreve o texto. Quem quiser palavras próprias já tem a lista `inject`, que aponta para um arquivo — sem código nenhum.
- **Regras para `technical` e `concise`.** Esta unidade atende o tom que o operador declarou e que falhou. Os outros dois não têm caso.

## Critérios de Aceitação

AC = critério de aceitação: uma frase verificável por um comando.

- **AC-1** — when o operador envia uma mensagem comum num projeto que declara `tone: didactic`, then a regra de escrita vem junto, nessa mensagem
  Command: `cargo test -p mustard-rt the_writing_rule_rides_every_prompt 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt pipeline_prompt_allows 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — when o projeto não escreveu `tone` — nem arquivo nenhum, ou outro valor —, then nada é injetado
  Command: `cargo test -p mustard-rt an_undeclared_tone_injects_nothing 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt pipeline_prompt_allows 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-3** — when a mensagem é um comando `/mustard:*`, then a regra vem do mesmo jeito — a resposta a um comando é escrita para o operador como qualquer outra
  Command: `cargo test -p mustard-rt the_writing_rule_rides_a_slash_command_too 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt pipeline_prompt_allows 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-4** — o build do workspace passa verde
  Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

| arquivo | o que muda |
|---|---|
| `apps/rt/src/hooks/session/prompt_submit_inject.rs` | a regra de escrita entra na injeção que acompanha cada mensagem; testes AC-1/2/3 |

O campo é lido **como foi escrito**, nunca resolvido: o valor resolvido tem `didactic` por padrão, e usá-lo poria a regra diante de todo projeto que apenas tem um `mustard.json`.

## Limites

IN: a injeção por mensagem e a leitura do campo.
OUT: tudo em `## Não-Objetivos`; o início de sessão; o renderizador de pedido de agente, que continua como está.

## Definitions

- **tom** — o campo `tone` do `mustard.json` — uma lista fechada de tres palavras (`didactic`, `technical`, `concise`) que declara COMO o projeto quer ser escrito.
- **injecao** — texto que o mustard acrescenta a janela da sessao por um gancho do Claude Code. Ha duas ocasioes: uma vez no inicio da sessao, ou junto de cada mensagem do operador.

## Decisions

- A entrega e a CADA mensagem do operador, nao uma vez no inicio.
  Reason: Medido pelo operador antes de aprovar: o texto tem 126 tokens, cerca de 0,04 por cento do que uma sessao longa consome, enquanto UMA troca de mal-entendido — resposta errada, correcao, reescrita — passa de mil tokens, e isso aconteceu duas vezes na sessao em que o pedido nasceu. Entregue uma vez, a regra fica cada vez mais distante conforme a conversa cresce.
- Ler o campo COMO FOI ESCRITO, nunca o valor resolvido.
  Reason: O valor resolvido tem `didactic` por padrao. Le-lo poria a regra diante de todo projeto que apenas tem um `mustard.json`, incluindo quem nunca pediu. Padrao e a ausencia de escolha; esta regra so responde a uma escolha escrita.
- Nao entrar no inicio da sessao.
  Reason: Aquela ocasiao ja esta em 8.594 de um teto de 10.000 caracteres, e a revisao anterior mediu 9.327 quando o texto estava la — folga menor que o proprio paragrafo. Alem disso a entrega por mensagem torna a do inicio redundante.
- O texto da regra vive no codigo, nao no `mustard.json`.
  Reason: `tone` e uma lista fechada de tres palavras: o operador escolhe qual, nao escreve o texto de cada uma. Quem quiser palavras proprias ja tem caminho — a lista `inject` do `mustard.json` aponta para um arquivo cujo conteudo entra na conversa, e nao precisa de codigo nenhum.

## Evidence

- O campo `tone` e lido em UM lugar so: o renderizador de pedido de agente, que monta o texto enviado aos agentes que escrevem arquivos.
  Evidence: `apps/rt/src/commands/agent/render/role.rs:111`
- A ocasiao que acompanha cada mensagem do operador ja existe e ja compoe varias partes numa injecao so — e onde a regra cabe.
  Evidence: `apps/rt/src/hooks/session/prompt_submit_inject.rs:186`
- O campo e opcional no esquema (`Option<String>`), entao da para distinguir quem escreveu de quem so tem um `mustard.json`.
  Evidence: `packages/core/src/domain/config.rs:352`
- Medido: a injecao de inicio de sessao esta em 8.594 de um teto de 10.000 caracteres; a revisao da unidade anterior mediu 9.327 quando este texto estava la.
  Evidence: `packages/core/src/platform/seeds.rs:39`
- Reproduzido em campo, duas vezes na mesma sessao: o operador nao entendeu explicacoes escritas com jargao nao traduzido, num projeto cujo `mustard.json` declara `tone: didactic` desde antes.
  Evidence: `apps/rt/src/commands/agent/render/role.rs:126`
