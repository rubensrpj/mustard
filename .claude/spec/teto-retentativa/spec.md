---
id: spec.teto-retentativa
---

# Teto de retentativa: o wave que gira sem limite sai da rodada e vira escalacao

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Hoje um wave que falha e volta para correção pode ser redespachado para sempre. O comando `wave-advance` monta a rodada de despacho filtrando só por uma coisa: o wave já fechou ou não fechou (`wave_advance.rs:269`). Um wave que abriu, não fechou e continua sendo entregue de novo é indistinguível, para o código, de um wave que nunca começou.

Existem contadores de retentativa no projeto, mas nenhum deles é comparado com um limite. Os dois que existem — `metrics_wave_status.rs:229` e `event_projections/pipeline.rs:71` — somam para relatório e param aí. Não há portão que leia esses números.

O que este trabalho acrescenta é o limite: uma contagem lida no mesmo lugar onde a rodada é montada, e uma saída para o wave que a estoura. Acima do teto o wave não volta para a fila — sai dela, e no lugar dele entra uma linha que diz ao operador que aquele wave parou de ser tentado e por quê.

Há uma armadilha de nomes no caminho, e ela é a razão de metade do desenho. O evento `retry.attempt` **já existe**, já é emitido a cada retry de hook medido (`session_knowledge_observer.rs:324`), e o roteador o classifica como `friction` por literal exato (`route.rs:77`). Reusar esse nome para retentativa de wave misturaria dois sinais que não têm nada a ver um com o outro e estragaria a telemetria de fricção que já roda. Por isso o evento novo nasce com nome próprio sob o prefixo `pipeline.`, que o roteador manda para o balde `pipeline` antes de chegar no braço de fricção (`route.rs:62`) — e há um critério de aceitação que existe só para provar isso.

## Usuários/Stakeholders

O operador que despacha um pipeline: é ele quem hoje precisa notar, no olho, que a mesma wave está sendo tentada pela quinta vez, e quem paga o custo de tokens enquanto não nota.

O orquestrador, que hoje recebe a mesma rodada de volta indefinidamente e não tem sinal deterministico algum para parar.

## Métrica de sucesso

Nenhum wave é despachado mais vezes do que o teto configurado. Depois deste trabalho, um `.events/` que carregue retentativas acima do teto faz `wave-advance` devolver a rodada sem aquele wave — verificável por teste, sem depender de observação de um pipeline real.

E a contagem de fricção não muda: o balde `friction` continua contendo exatamente o que continha antes.

## Não-Objetivos

- Não muda `retry.attempt`, nem quem o emite, nem como ele é contado. O evento existente fica intacto.
- Não cria teto de retentativa para o laço de revisão por subprojeto (`review_round`), que tem chave de contagem diferente (subprojeto, não número de wave). Se o laço de revisão precisar de teto, é outra unidade de trabalho.
- Não tenta reconstruir retentativas passadas: a contagem começa a valer a partir dos eventos que este trabalho passa a escrever.
- Não mexe no `retry_count` como campo de outros eventos — só o consome no evento novo.

## Critérios de Aceitação

- AC-2 — Quando o classificador de rota recebe o nome do evento novo, entao ele o coloca no balde `pipeline` e o balde `friction` continua reconhecendo `retry.attempt`. Command: `grep -q 'fn retry_event_is_not_routed_as_friction' apps/rt/src/shared/events/route.rs && cargo test -p mustard-rt retry_event_is_not_routed_as_friction 2>&1` Expect: `test result: ok\. 1 passed`
- AC-1 — Quando o log `.events/` de um spec carrega retentativas de um wave em numero igual ou maior que o teto, entao `wave-advance` devolve a rodada sem aquele wave e com um item de escalacao que o nomeia. Command: `grep -q 'fn retry_ceiling_pulls_wave_from_round' apps/rt/src/commands/pipeline/wave_advance.rs && cargo test -p mustard-rt retry_ceiling_pulls_wave_from_round 2>&1` Expect: `test result: ok\. 1 passed`
- AC-3 — Quando os tres agentes do plugin sao lidos, entao cada frontmatter declara `maxTurns` com um inteiro positivo. Command: `grep -lE '^maxTurns: [1-9][0-9]*$' plugin/agents/mustard-guards.md plugin/agents/mustard-patterns.md plugin/agents/mustard-review.md` Expect: `mustard-review\.md`

<!-- PLAN -->

## Arquivos

| Arquivo | Wave | O que muda |
|---|---|---|
| `packages/core/src/domain/model/event.rs` | 1 | ganha a constante `EVENT_PIPELINE_WAVE_RETRY`, ao lado de `EVENT_PIPELINE_WAVE_START` (linha 114) |
| `apps/rt/src/commands/event/emit_pipeline.rs` | 1 | ganha `emit_wave_retry`, espelhando `emit_wave_start` (linha 1448) |
| `apps/rt/src/shared/events/route.rs` | 1 | ganha o teste de não-contaminação; a tabela `classify_kind` **não** muda |
| `apps/rt/src/commands/pipeline/wave_advance.rs` | 2 | resolve o modo e o teto, conta as retentativas, tira o wave estourado da rodada, acrescenta o item de escalação |
| `plugin/agents/mustard-guards.md` | 3 | frontmatter ganha `maxTurns` |
| `plugin/agents/mustard-patterns.md` | 3 | frontmatter ganha `maxTurns` |
| `plugin/agents/mustard-review.md` | 3 | frontmatter ganha `maxTurns` |
| `plugin/refs/spec/resume-loop.md` | 2 (cascata) | ganha o ramo do item de escalação: nunca despachar, nunca `wave-done` no wave puxado |

A última linha é uma **cascata declarada depois da revisão**, não parte do plano original. A onda 2 criou um formato de linha novo na rodada (`role: "escalation"`) e nenhum consumidor foi ensinado a lê-lo; os dois passos do relay são incondicionais e ambos erram nele. A mudança anterior nesta mesma função publicou o parágrafo dela aqui pelo mesmo motivo — a obrigação é idêntica e foi pulada.

## Limites

IN: a contagem de redespacho de wave e o limite que a lê; o evento próprio que registra cada redespacho; a saída do wave estourado da rodada e o item de escalação que entra no lugar; as duas variáveis de ambiente que configuram modo e número; o teto de turnos dentro dos três subagentes do plugin.

OUT: o evento `retry.attempt` e tudo que o consome; o laço de revisão por subprojeto; a recusa por ciclo de dependência (`AdvanceRefusal::CyclicDependency`), que resolve outro problema e permanece como está; qualquer mudança na tabela `classify_kind`; o painel do dashboard, que lê os eventos pela projeção existente e não precisa de fio novo.

## Concerns

Achados não-bloqueantes das duas revisões, declarados e **não** consertados nesta unidade — ampliar um conserto por achado adjacente já produziu defeito pior neste projeto.

- **MAIOR — a retentativa é contada por INVOCAÇÃO de `advance()`, não por despacho.** Quatro chamadas cruas de `wave-advance`, sem nenhum agente despachado, escalam o wave. Isso é a decisão declarada do plano ("um wave entregue de novo já iniciado E não completado É a retentativa"), e as docs do módulo registram que não existe sinal de despacho persistido confiável — mas com o padrão `strict`, o custo de errar é escalar um wave que ninguém tentou. Um sinal de despacho de verdade é unidade própria.
- **MENOR — `MUSTARD_RETRY_GATE_MODE=off` não zera a contagem.** Ele suspende a retirada, mas as linhas de retentativa continuam sendo escritas; voltar para `strict` re-escala na rodada seguinte. O texto do item de escalação agora diz isso, mas o comportamento em si não mudou.
- **MENOR — o texto da escalação afirma sempre que "os outros waves do mesmo nível foram despachados normalmente"**, o que é falso quando todos os irmãos também estouraram o teto (observado ao vivo: waves 1 e 2 escalados, ambos carregando a frase). Uma condicional resolveria; fica declarado porque a revisão já aprovou e editar depois disso é o movimento que este projeto registrou como criador de defeito pior.
- **MENOR — os números de `maxTurns` (25/40/80) foram escolhidos por natureza do trabalho, sem medição.** Não existe telemetria de turnos por agente no harness. Usos de ferramenta observados neste repositório chegam a 64, então os 80 do revisor têm folga fina.
- **MENOR (molde, justificado) — o item de escalação carrega o texto inline** em vez do stub de prompt, contrariando o `rt-item-pattern`. O desvio está documentado no próprio arquivo e é sólido: o sentido de uma escalação é o operador ler sem despachar agente nenhum para buscar o texto.

## Definitions

- **teto de retentativa** — o numero maximo de vezes que um mesmo wave pode ser redespachado antes de sair da rodada
- **rodada** — a lista de itens que uma invocacao de `wave-advance` devolve ao orquestrador para despachar
- **item de escalacao** — a linha que entra na rodada no lugar do wave que estourou o teto, dizendo que aquele wave parou de ser redespachado e precisa de decisao humana
- **modo do portao** — o trio `off`/`warn`/`strict` que cada portao do projeto resolve na propria variavel `MUSTARD_<CONCERN>_MODE`, com o default declarado no call-site

## Decisions

- o evento novo se chama `pipeline.wave.retry` e nunca reusa o nome `retry.attempt`
  Reason: `route.rs:77` classifica o literal `retry.attempt` como `friction`; qualquer nome com prefixo `pipeline.` cai no primeiro braco de `classify_kind` (`route.rs:62`) e vira `pipeline`, entao a telemetria de friccao de hook fica intacta
- o teto usa DUAS variaveis de ambiente e nao uma: `MUSTARD_RETRY_GATE_MODE` pelo resolvedor compartilhado e `MUSTARD_RETRY_CEILING` como inteiro parseado a parte
  Reason: `resolve_mode` (gate_mode.rs:37-52) devolve o enum `GateMode` de tres estados e nao um numero, entao um teto numerico nao pode sair dele; o par modo+numero ja tem precedente medido no projeto em `MUSTARD_DELEGATION_WARN_MODE` + `MUSTARD_DELEGATION_WARN_THRESHOLD` (delegation_advisory.rs:101-120)
- o modo nasce com default `strict` e o teto com default 3
  Reason: o guard do rt manda cada portao declarar o default no proprio call-site; este portao existe para PARAR um laco que gira sem limite, e em `warn` ele seria decorativo — `off` continua sendo a saida de escape
- o wave que estoura o teto sai da rodada, mas os irmaos saudaveis do mesmo nivel continuam sendo despachados
  Reason: o comentario de wave_advance.rs:231-247 registra, aprendido duas vezes, que recusar a rodada inteira por causa de um unico wave ja estrangulou waves independentes e limpos
- o redespacho e contado no mesmo ponto que ja emite `pipeline.wave.start`
  Reason: em wave_advance.rs:310-315 o `advance()` ja sabe quais waves foram iniciados e ainda nao completados; um wave entregue de novo nesse estado E a retentativa, e as docs do modulo (linhas 25-28) registram que nao existe outro sinal de despacho persistido confiavel
- os tres agentes do plugin passam a declarar `maxTurns` no frontmatter
  Reason: medido no binario do Claude Code 2.1.241: a validacao `has invalid maxTurns '...'. Must be a positive integer` vive no caminho `plugin_load_agents`, e o aviso de campo ignorado para agente de plugin nomeia so `permissionMode` e `mcpServers`

## Evidence

- nenhum contador de retentativa e comparado com um limite: a unica ocorrencia incrementa um contador para relatorio
  Evidence: `apps/rt/src/commands/economy/metrics_wave_status.rs:229`
- o evento `retry.attempt` ja e emitido, um por retry de hook medido, de forma idempotente
  Evidence: `apps/rt/src/hooks/session/session_knowledge_observer.rs:324`
- `retry.attempt` e roteado como `friction` por literal exato, entao reusar esse nome contaminaria a telemetria de friccao
  Evidence: `apps/rt/src/shared/events/route.rs:77`
- todo nome com prefixo `pipeline.` e classificado como `pipeline` antes de qualquer outro braco
  Evidence: `apps/rt/src/shared/events/route.rs:62`
- `retry_count: Option<u32>` ja e campo tipado do modelo de evento
  Evidence: `packages/core/src/domain/model/event.rs:224`
- `advance()` monta a rodada filtrando os itens do nivel pendente por `!completed.contains(&it.wave)` — e ai que um wave sai ou fica
  Evidence: `apps/rt/src/commands/pipeline/wave_advance.rs:269`
- `started_waves` ja devolve o conjunto de waves que carregam `pipeline.wave.start`, e o emit de start ali e idempotente
  Evidence: `apps/rt/src/commands/pipeline/wave_advance.rs:310`
- `resolve_mode` devolve `GateMode` de tres estados e nao tem como carregar um numero
  Evidence: `apps/rt/src/shared/gate_mode.rs:37`
- o par modo+numero ja existe no projeto: um `Mode` resolvido por env e um `threshold` parseado a parte com default no call-site
  Evidence: `apps/rt/src/hooks/task/delegation_advisory.rs:115`
- os tres agentes do plugin nao declaram `maxTurns` hoje
  Evidence: `plugin/agents/mustard-review.md:1`
- o grep por `retry|retries|max_attempt` em apps/rt/src e packages/core/src so devolve telemetria e prosa: o `metrics.retries` somado aqui e relatorio, nunca limite
  Evidence: `apps/rt/src/commands/pipeline/wave_advance.rs:688`