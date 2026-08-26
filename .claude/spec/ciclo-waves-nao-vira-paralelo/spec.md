---
id: spec.ciclo-waves-nao-vira-paralelo
---

# ciclo declarado entre waves recusa a rodada em vez de fingir uma ordem

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Um plano de waves — lotes de trabalho despachados por rodadas — declara suas dependências numa coluna `Depends on`. Quando essa coluna se contradiz (wave 2 diz depender da 3, wave 3 diz depender da 2), não existe ordem que a satisfaça.

`assign_levels` não detectava isso. Pior: o comentário dela afirmava que o ciclo degradava para o nível 0, e um teste sustentava essa afirmação. Medido, não era o que acontecia — a contradição saía com níveis arbitrários e atravessava o sistema disfarçada de plano sequencial comum. Nada na saída denunciava o defeito, e é isso que torna o caso urgente: uma falha invisível é pior que uma barulhenta.

## Usuários/Stakeholders

Quem escreve um plano de waves à mão, e o orquestrador que o despacha. O primeiro passa a receber o erro apontado; o segundo deixa de despachar trabalho numa ordem inventada.

## Métrica de sucesso

Um plano contraditório para o despacho e nomeia as waves DO LAÇO, em vez de produzir uma rodada. Nenhuma wave é perdida, e nenhuma wave correta é acusada.

## Não-Objetivos

- O ciclo INFERIDO por imports continua sendo WARN. Aquela inferência é heurística, e as fronteiras explícitas do planejador prevalecem sobre ela.
- `wave-overlap-check` continua apenas avisando; não passa a bloquear.
- `boundary_gate` continua permissivo de propósito — um hook de escrita não pode negar gravação porque um plano se contradiz.
- Ampliar o leitor da coluna `Depends on` fica de fora. Ele lê `[[…]]` e nada mais: número nu não é lido, e autorreferência é descartada em vez de recusada. Ambas as lacunas são anteriores a esta unidade, e ler dependência de texto livre foi tentado e revertido — inventava contradição onde não havia.
- Tornar o registro da recusa durável fica de fora. Ele expira em dez minutos como qualquer falha de despacho, porque esse prazo é a única coisa que limpa o registro depois que o plano é consertado.

## Critérios de Aceitação

- **AC-1** — when um plano declara um ciclo na coluna `Depends on`, then o despacho recusa a rodada e nomeia as waves travadas em vez de inventar uma ordem
  Command: `cargo test -p mustard-rt --lib declared_cycle_refuses_the_round`
- **AC-2** — when a rodada é recusada, then nenhum evento `pipeline.wave.start` é gravado, para que a re-execução depois do conserto comece limpa
  Command: `cargo test -p mustard-rt --lib refused_round_emits_no_wave_start`
- **AC-3** — when o mesmo plano contraditório passa pela face permissiva, then todas as waves continuam presentes: recusa-se a rodada, nunca o trabalho
  Command: `cargo test -p mustard-rt --lib declared_cycle_refuses_the_dispatch_face_and_drops_nothing`
- **AC-4** — when uma wave apenas ESPERA atrás do ciclo, then ela não é nomeada como contradição, e o nível dela fica ACIMA da dependência, nunca abaixo
  Command: `cargo test -p mustard-rt --lib levels_over_a_contradictory_plan`
- **AC-5** — when as waves do ciclo terminam e a que esperava atrás fica pendente, then ela despacha, em vez de ficar presa para sempre
  Command: `cargo test -p mustard-rt --lib wave_behind_a_completed_cycle_dispatches`
- **AC-6** — when todas as waves de uma spec contraditória já terminaram, then ela ainda alcança a rodada de revisão, porque ordem não decide nada quando não há o que ordenar
  Command: `cargo test -p mustard-rt --lib completed_cyclic_spec_still_reaches_its_review_round`
- **AC-7** — when as waves do ciclo já terminaram mas outra segue pendente, then a rodada não é recusada, porque a contradição já não governa nada
  Command: `cargo test -p mustard-rt --lib completed_cycle_does_not_block_a_clean_pending_wave`
- **AC-8** — when a coluna contém PROSA com dígitos, then nenhuma dependência é declarada: ler números soltos de texto livre inventava ciclo onde não havia
  Command: `cargo test -p mustard-rt --lib prose_in_the_depends_cell_declares_no_edge`
- **AC-9** — when a rodada é recusada, then fica registrado um `pipeline.dispatch_failure`, para que a parada seja visível a quem retoma a spec
  Command: `cargo test -p mustard-rt --lib refused_round_records_a_dispatch_failure`
- **AC-10** — when a mesma spec quebrada é reinvocada várias vezes, then fica UM registro de falha, não um por invocação (eles somam na métrica de retentativas)
  Command: `cargo test -p mustard-rt --lib dispatch_failure_is_recorded_once_not_per_invocation`
- **AC-11** — when uma wave está ENTRE dois laços, sem estar em nenhum, then ela não é nomeada: pertencer a um laço é alcançar a si mesma, não é sobrar de uma peneira
  Command: `cargo test -p mustard-rt --lib levels_wave_between_two_loops_is_not_named`
- **AC-12** — when qualquer wave declara depender de outra, then ela nunca recebe nível ABAIXO dessa dependência, nem entre dois laços distintos
  Command: `cargo test -p mustard-rt --lib levels_order_two_distinct_loops`
- **AC-13** — when um laço tem QUALQUER membro ainda pendente, then a rodada é recusada inteira, sem exceção por wave: isentar um membro num laço de três deixa outro bloqueando e prende o isento para sempre
  Command: `cargo test -p mustard-rt --lib three_wave_loop_with_one_member_complete_refuses_whole`
- **AC-14** — when o registro anterior da recusa já expirou, then um novo é gravado: o guarda contra duplicata não pode calar o próprio sinal
  Command: `cargo test -p mustard-rt --lib dispatch_failure_is_recorded_again_once_the_old_one_expired`

## Checklist

- [x] T1 — `assign_levels` troca a relaxação com teto por descascamento e devolve as waves não colocadas.
- [x] T2 — `build_plan` ganha a face `build_plan_checked`, que recusa; a permissiva segue servindo auditorias e o hook de escrita.
- [x] T3 — `advance()` devolve `Result` e `run()` imprime a forma de erro já existente no projeto.
- [x] T4 — o teste que fixava o comportamento errado é substituído; os casos novos cobrem ciclo mútuo, wave que só espera atrás dele, dependência fora do plano e célula com prosa.
- [x] T5 — a prosa que o orquestrador segue aprende a ler a rodada recusada.
- [x] T6 — a recusa vale só sobre wave travada AINDA PENDENTE, e só sobre quem está no laço; quem espera atrás despacha quando a dependência termina.
- [x] T7 — a rodada recusada grava `pipeline.dispatch_failure`, idempotente por motivo e com carimbo de tempo, para não parecer pipeline ocioso a quem retoma.
- [x] T8 — waves travadas recebem níveis distintos, para as auditorias e o guarda de escrita lerem o plano como liam antes.
- [x] T9 — o cálculo de níveis passa a usar componentes fortemente conexos: pertencer a um laço vira uma definição (alcançar a si mesma) em vez de sobra de peneira, e os níveis saem do grafo com cada laço colapsado num nó só.
- [x] T10 — REVERTIDO: ler números nus da coluna. Ler dígitos de texto livre inventava ciclo em célula com prosa e recusava plano sem contradição. A lacuna fica declarada, não remendada.
- [x] T11 — REVERTIDO: tratar autorreferência como ciclo. Distinguir declaração de artefato de resolução exigia heurística sobre a grafia do token, que quebrava para papel começando com `wave`.
- [x] T12 — REVERTIDO: isentar `cyclic-dependency` do prazo de dez minutos. Esse prazo é a única coisa que limpa o registro; isentá-lo deixava `mode: ask` para sempre mesmo depois do plano consertado.

## Definitions

- **ciclo declarado** — Uma contradição escrita à mão na coluna `Depends on` do `wave-plan.md`: wave 2 declara depender da 3 e wave 3 declara depender da 2. Distinto do ciclo INFERIDO por imports, que `wave-dependency` reporta como WARN porque a inferência é heurística.
- **nível topológico** — O número que `assign_levels` atribui a cada wave. Waves de mesmo nível não dependem umas das outras e são despachadas na mesma rodada; nível maior significa rodada posterior.
- **face permissiva** — `build_plan`, que diante de um plano malformado devolve todas as waves em vez de recusar. Serve auditorias e o hook de escrita, que não podem bloquear por causa de um plano contraditório.

## Decisions

- Recusar a rodada em vez de emitir um aviso.
  Reason: O ciclo de import é inferido e heurístico, então WARN está certo lá. O ciclo da coluna `Depends on` foi escrito por uma pessoa; não há ordem que o satisfaça, e despachar em qualquer ordem seria inventar uma resposta que o plano não contém.
- Duas faces sobre o mesmo cálculo: `build_plan` permissiva e `build_plan_checked` que recusa.
  Reason: Dos três chamadores, um é `boundary_gate`, o hook que autoriza escrita de arquivo. Endurecê-lo faria um plano mal escrito virar bloqueio de gravação no meio do trabalho de alguém. Só o despacho tem motivo para recusar.
- A recusa viaja no JSON e o processo sai com código 0.
  Reason: É a regra que este crate já segue nos hooks: bloqueio se expressa no documento, nunca via código de saída não-zero.
- A saída da recusa é um objeto, não um array vazio.
  Reason: `[]` já significa 'não sobrou wave, vá fechar a spec'. Reportar uma contradição com a palavra que quer dizer 'você terminou' mandaria o orquestrador fechar em vez de consertar.
- O campo `cycle` inclui as waves que esperam ATRÁS do ciclo, não só os membros do laço.
  Reason: Quem vai consertar a tabela precisa ver o conjunto travado inteiro, não a ponta dele.
- Trocar a relaxação por descascamento em vez de instrumentar a relaxação existente.
  Reason: O teto de iterações só existia para conter uma relaxação que podia girar. No descascamento, uma passada que não coloca ninguém encerra o laço, e o que sobra sem nível É o ciclo — mesma semântica de `stuck` da caminhada topológica que o projeto já tinha.

## Evidence

- `assign_levels` NÃO degradava um ciclo para o nível 0, apesar do que o comentário afirmava. O ramo `else if !resolved` só deixa a wave no zero na primeira passada; da segunda em diante todo nó já tem entrada no mapa, `resolved` é sempre verdadeiro, e cada passada incrementa os membros do ciclo até o teto parar o laço.
  Evidence: `apps/rt/src/commands/pipeline/dispatch_plan.rs:548`
- Medido com a função isolada: para `1 <-> 2` saíam os níveis 4 e 5; para `2 <-> 3` com uma wave limpa antes, {1:0, 2:6, 3:7}. Ou seja, a contradição atravessava disfarçada de plano sequencial comum, e nada na saída denunciava o defeito.
  Evidence: `apps/rt/src/commands/pipeline/dispatch_plan.rs:581`
- Uma wave que declara depender de si mesma ganhava nível 2 e passava calada.
  Evidence: `apps/rt/src/commands/pipeline/dispatch_plan.rs:548`
- O teste `levels_cycle_degrades_to_zero_without_dropping` fixava o comportamento errado, e nem o fixava direito: conferia apenas que as duas chaves existiam no mapa, nunca os níveis. Traduzido para a API nova e rodado contra o código corrigido, falha com `left: 0, right: 2` — é essa a prova vermelha do critério.
  Evidence: `apps/rt/src/commands/pipeline/dispatch_plan.rs:790`
- `build_plan` tem três chamadores de produção, e um deles é um hook de escrita — a razão de a correção ter duas faces em vez de uma recusa geral.
  Evidence: `apps/rt/src/hooks/write/boundary_gate.rs:254`
- A forma de erro `{"error":"cyclic-dependency","cycle":[…]}` já existia no projeto para o ciclo inferido por imports; foi reusada em vez de inventada.
  Evidence: `apps/rt/src/commands/wave/wave_dependency.rs:267`
- A prosa que o orquestrador segue descrevia apenas o array e o `[]`. Sem ajuste, o objeto de erro chegaria a um leitor sem regra para ele, e o risco concreto era cair no ramo do `[]`, que manda fechar a spec.
  Evidence: `plugin/refs/spec/resume-loop.md:63`