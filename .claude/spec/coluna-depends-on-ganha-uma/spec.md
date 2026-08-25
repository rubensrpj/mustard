---
id: spec.coluna-depends-on-ganha-uma
---

# a coluna Depends on ganha uma gramatica e um leitor so, e a caminhada topologica passa a ser uma

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

A coluna `Depends on` do plano de waves tinha **dois leitores que discordavam**, e cada um errava para um lado.

O leitor do despacho só enxergava `[[…]]`. Um ciclo escrito com números nus (`| 2 | cli | 3 |`) não gerava aresta nenhuma, então a recusa recém-instalada não o via e o plano achatava numa rodada paralela — o defeito original, intacto, por uma porta que ninguém tinha aberto.

O leitor da checagem prévia varria todos os tokens da célula, e pega os dígitos iniciais de qualquer um deles. Uma célula com prosa — `nada, ver os 2 anexos` — declarava dependência da wave 2. Isso faz a checagem procurar símbolos numa wave que não é dependência de verdade, e reportá-los faltando.

Havia também duas caminhadas topológicas separadas no crate, resolvendo o mesmo problema e discordando da resposta: a de imports acusava tudo que não conseguiu posicionar, incluindo quem apenas espera atrás de um laço.

## Usuários/Stakeholders

Quem escreve um plano de waves à mão, em qualquer das duas formas de autoria; e a checagem prévia, que deixa de acusar símbolo faltando por causa de um número solto numa frase.

## Métrica de sucesso

Uma célula é lida do mesmo jeito pelos dois leitores. Prosa não declara dependência em nenhum dos dois. Ciclo escrito com números nus é recusado como o escrito com wikilinks. E existe uma definição de "ciclo" no crate, não duas.

## Não-Objetivos

- Autorreferência continua descartada, não recusada. Decisão da unidade anterior, mantida pelo mesmo motivo.
- Linha de wave duplicada não foi tocada: a acusação de que ela perde dependências foi REFUTADA por medição, e o comportamento atual está travado por teste.
- O registro da recusa continua expirando em dez minutos. Torná-lo durável exige um sinal de limpeza, que segue fora de escopo.

## Critérios de Aceitação

- **AC-1** — when a coluna é escrita com números nus, then o ciclo é lido igual ao escrito com wikilinks, porque os dois leitores passam a usar a mesma gramática
  Command: `cargo test -p mustard-rt --lib bare_number_deps_are_read_like_wikilinks`
- **AC-2** — when a célula contém PROSA com dígitos, then nenhuma dependência é declarada, em nenhum dos dois leitores
  Command: `cargo test -p mustard-rt --lib depends_on_tests`
- **AC-3** — when a célula tem wikilink E prosa em volta, then só o wikilink conta: quem escreveu um link foi explícito
  Command: `cargo test -p mustard-rt --lib wikilinks_are_the_dependencies_and_prose_around_them_is_not`
- **AC-4** — when um nó apenas espera atrás de um laço, then ele não é nomeado como ciclo, na caminhada de imports também
  Command: `cargo test -p mustard-rt --lib what_waits_behind_a_loop_is_not_named`
- **AC-5** — when um nó está ENTRE dois laços, sem estar em nenhum, then ele não é nomeado
  Command: `cargo test -p mustard-rt --lib a_node_between_two_loops_is_not_named`
- **AC-6** — when dois laços distintos se ordenam entre si, then os níveis respeitam essa ordem
  Command: `cargo test -p mustard-rt --lib two_distinct_loops_are_ordered`
- **AC-7** — when uma wave é listada DUAS vezes, then sobra uma linha, com as dependências da primeira — o comportamento que a revisão acusou de quebrado e a medição refutou
  Command: `cargo test -p mustard-rt --lib a_wave_listed_twice_keeps_the_first_rows_dependencies`
- **AC-8** — when a caminhada topológica compartilhada ordena qualquer grafo, then a suíte dela inteira passa
  Command: `cargo test -p mustard-rt --lib shared::dag`

## Checklist

- [x] T1 — `shared/dag.rs` passa a ser a única atribuição de níveis do crate, genérica sobre o tipo do nó.
- [x] T2 — `dispatch_plan::assign_levels` vira adaptador fino sobre ela.
- [x] T3 — `wave_dependency::topological_waves` também, e herda a correção de nomear o laço em vez de tudo que ficou travado.
- [x] T4 — `wave_lib::depends_on_tokens` passa a ser a única gramática da coluna `Depends on`.
- [x] T5 — os dois leitores da coluna passam a usá-la; cada um segue resolvendo os tokens para o seu próprio tipo.
- [x] T6 — teste travando o comportamento de linha duplicada, que a medição mostrou já estar correto.

## Definitions

- **gramática da célula** — A regra de quando a coluna `Depends on` declara dependências. Marcador vazio não declara nada; se há `[[…]]`, esses são as dependências e o resto é comentário; senão a célula pode ser lista de números nus, mas só se TODOS os tokens tiverem forma de referência a wave. Um token que não tem, e a célula é prosa.
- **estar num laço** — Alcançar a si mesma pelas arestas de dependência. É definição, não resultado de peneira — quem apenas espera atrás de um laço, ou está entre dois, não se alcança e portanto não está em laço nenhum.

## Decisions

- A regra da lista nua é tudo-ou-nada: um token sem forma de wave e a célula inteira não declara nada.
  Reason: Escolher os tokens com cara de wave DENTRO de prosa foi tentado na unidade anterior e revertido: `nada (ver os 2 anexos)` virava aresta na wave 2, e duas células assim recusavam um plano correto. Ler dependência de texto livre não descobre contradição, inventa.
- A gramática mora em `wave_lib`, e cada leitor resolve os tokens do seu jeito.
  Reason: O que os dois leitores precisavam compartilhar era a decisão sobre O QUE é uma dependência, não como resolvê-la: um resolve para `u32` com mapa de papéis, o outro para `WaveNumber`. Compartilhar só a gramática elimina a divergência sem forçar um tipo comum.
- Uma única atribuição de níveis no crate, genérica sobre o tipo do nó.
  Reason: As duas caminhadas resolviam o mesmo problema e discordavam da resposta: a de imports reportava tudo que não conseguiu posicionar, incluindo quem só espera atrás do laço. Unificar propaga a correção em vez de manter duas definições de 'ciclo'.
- Autorreferência continua descartada, não recusada.
  Reason: Decisão da unidade anterior, mantida. A forma de papel nu existe para absorver autoria solta do agente de plano, e recusar o spec inteiro por causa desse atalho custa mais do que o caso vale.

## Evidence

- `parse_wave_number_from_token` pega os dígitos INICIAIS de qualquer token, então a checagem prévia lia `os 2 anexos` como dependência da wave 2. Uma dependência fantasma ali faz a checagem procurar símbolos numa wave que não é dependência, e reportá-los faltando.
  Evidence: `apps/rt/src/commands/review/dependency_precheck.rs:831`
- O leitor do dispatch só enxergava `[[…]]`, então um ciclo escrito com números nus não gerava aresta nenhuma e o plano achatava numa rodada paralela — o defeito original, intacto, por uma porta que ninguém tinha aberto.
  Evidence: `apps/rt/src/commands/pipeline/dispatch_plan.rs:448`
- REFUTADO: a acusação de que linha de wave duplicada perde as dependências e despacha a wave duas vezes. `parse_wave_plan_table` ordena e deduplica por número antes de construir qualquer WaveRow, mantendo a PRIMEIRA linha. Medido no binário: plano com a wave 2 repetida devolve um item, com a dependência da primeira linha preservada.
  Evidence: `apps/rt/src/commands/pipeline/dispatch_plan.rs:367`
- `topological_waves` reportava como ciclo tudo que não conseguiu visitar, o que inclui quem apenas espera atrás do laço — a mesma imprecisão que a unidade anterior corrigiu do lado das waves. Unificar propagou a correção.
  Evidence: `apps/rt/src/commands/wave/wave_dependency.rs:190`
- Medido nos 32 planos de wave do repositório: zero contraditórios e zero usando a forma de número nu. Todos os defeitos desta unidade são latentes, não observados.
  Evidence: `.claude/spec:1`
