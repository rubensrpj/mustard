# Auditoria do Mustard: consumo de tokens e qualidade de código

**Data:** 25/09/2026.
**Base auditada:** branch `dev`, commit `7464541`.
**Método:** leitura do código. Cada afirmação traz `arquivo:linha`; os pontos centrais foram conferidos manualmente.

**Legenda de veredito:**
- **ATENDE**
- **PARCIAL**
- **NÃO ATENDE**
- **NÃO SE APLICA**
- **PENDENTE:** exige execução manual que esta auditoria não fez.

**Legenda de confiança da minha afirmação sobre o código** (não é a mesma coisa que a confiança da fonte externa do checklist):
- **[ALTA]:** li o código e conferi.
- **[MÉDIA]:** inferência a partir do código, sem execução.
- **[BAIXA]:** hipótese.

**Siglas:**
- **MCP** — Model Context Protocol, protocolo de ferramentas externas.
- **SDD** — Spec-Driven Development, desenvolvimento orientado a especificação.
- **MDD** — Model-Driven Development, desenvolvimento dirigido por modelos.
- **EARS** — Easy Approach to Requirements Syntax, um formato padronizado de frase de requisito.
- **BM25** — algoritmo clássico de ranqueamento de busca textual.
- **QA** — Quality Assurance, garantia de qualidade.

**Aviso sobre as fontes externas:** não abri nem verifiquei os estudos citados no checklist (Uvik, ETH Zurich, Böckeler, Anthropic). As confianças atribuídas a eles são as do checklist original. Esta auditoria verifica só o lado do Mustard.

---

## Achado transversal: a documentação descreve um Mustard que não existe mais [ALTA]

Antes dos itens, isto afeta a leitura de todos eles:

- **`README.md:77`** diz "O roteador é injetado em todo prompt e classifica o pedido sozinho". Hoje o texto por prompt é uma linha de no máximo 100 caracteres. Um teste garante isso (`apps/rt/src/hooks/session/prompt_entry.rs:22-24` e o teste `every_message_gets_only_the_short_line`). Não há mais classificação de pedido.
- **O README descreve `spec.md`, `wave-plan.md` e `wave-N-{role}/spec.md`.** A spec atual é um arquivo de eventos, `.claude/spec/<nome>/spec.ndjson`, com 36 tipos de evento (`packages/core/src/domain/spec_events/types.rs:301`), gravado só via `mustard-rt run write`.
- **O README cita "≥2 camadas/subprojetos ou entidade nova"** como critério do fluxo completo. Esse critério só existe no enum `Scope` (`packages/core/src/domain/model/pipeline.rs:51-57`), que é usado apenas por um validador antigo e por testes. É código morto no fluxo atual.
- **O `CLAUDE.md` da raiz cita `base_gate.rs:149` como mecanismo ativo** ("O `base_gate` já prescreve `git pull --ff-only`"). O arquivo está desligado: "Sem nenhum chamador… segue no repositório por decisão do usuário" (`apps/rt/src/commands/event/base_gate.rs:46-49`). O README também diz que "o porteiro de base minera o repositório no caminho de entrada". Hoje o scan é disparado ao abrir uma spec (`apps/rt/src/commands/flow/open.rs:432`) e depois de cada commit de rodada.
- **As fases reais não são as 6 do checklist.** São `survey, plan, approved, running, closed, pr_open, delivered, discarded` (`types.rs:277-278`). Neste documento, cada fase do checklist é mapeada assim:
  - **ANALYZE** = `survey` (levantamento, comando `grill`);
  - **PLAN** = `plan`;
  - **EXECUTE** = `running` (rodada de ondas);
  - **REVIEW** = revisão final dentro do `close`;
  - **QA** = testes do `close`, pois não existe fase QA separada;
  - **CLOSE** = `closed`.

**Correção:** reescrever as seções do README, o `README.en.md` e a regra do `base_gate` no `CLAUDE.md`, ou religar o `base_gate`; a decisão é sua. **Custo:** baixo. **Risco de não corrigir:** quem lê a documentação (inclusive o próprio modelo, quando ela é injetada ou lida) vai raciocinar sobre um sistema errado.

---

## 0. Antes de mexer: medir (linha de base)

### 0.1 Registra tokens de entrada, saída e cache por fase?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - O único registro de tokens está no evento `send`: campos `tokens`, `steps`, `caller_tokens` e `caller_steps` (`types.rs:555-560`).
  - Os valores são **informados pelo próprio modelo**, copiando o que a plataforma mostrou, no bloco `<USAGE>…</USAGE>` (`packages/core/src/platform/i18n/flow.rs:612-619`; parser em `apps/rt/src/commands/flow/round/report.rs:752-768`). Só são aceitos para ondas (`report.rs:711`).
  - Não há divisão entre entrada, saída e cache. Survey, plan, revisão e close não têm contagem nenhuma.
  - Por fase existe só **tempo** ("Time per phase", `packages/core/src/platform/i18n/page.rs:885-886`).
  - `cache_read_input_tokens` e `cache_creation_input_tokens` aparecem só em JSON de teste (`apps/rt/src/commands/statusline/mod.rs:277-278`).
  - O struct `TelemetrySummaryEntry` (`packages/core/src/view/summary/mod.rs:191-203`) tem os campos certos (`total_tokens`, `by_model`, `by_agent`), mas ninguém o preenche: é código morto.
  - Existe, sim, o registro do **tamanho de cada injeção de hook**, em caracteres e em tokens estimados por caracteres ÷ 4 (`apps/rt/src/dispatch.rs:125-137`; `page.rs:899-900`).
- **Observação:** o total por onda depende de o modelo copiar um número corretamente. É um dado frágil, e não serve como linha de base.
- **Correção de menor custo:**
  1. No hook `Stop` e no `SessionEnd`, ler o arquivo em `transcript_path` (os hooks já o recebem), que é JSONL (JSON por linha).
  2. Somar `usage.input_tokens`, `output_tokens`, `cache_read_input_tokens` e `cache_creation_input_tokens` das mensagens novas desde a última leitura.
  3. Gravar um evento com a fase corrente, lida do último `state`.
  4. Fazer o mesmo com o transcript de cada subagente de onda e de revisão.
  - **Custo:** médio (um hook e um tipo de evento).

### 0.2 Existe um conjunto fixo de tarefas de teste?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - Não há benchmark de ponta a ponta.
  - O único teste de recuperação é do scan, e recusa explicitamente benchmark curado: "A curated benchmark is a maintenance debt and a bias" (`apps/scan/tests/retrieval_self_recall.rs:15`).
  - `docs/2026-08-14-build-cycle-measurements.md` mede tempo de compilação, não o modelo.
- **Observação:** a objeção do scan (viés de conjunto curado) é legítima para recuperação de arquivos, mas não se aplica aqui. Para comparar **versões do harness**, o conjunto fixo é justamente o que elimina o viés de comparar tarefas diferentes.
- **Correção:**
  - Criar a pasta `bench/` com 10 pedidos reais sobre um repositório congelado (um commit fixo): 3 correções de defeito, 3 refatorações e 4 funcionalidades, de 1 a 5 arquivos.
  - Rodar via `claude -p` duas vezes, com o Mustard ligado e desligado.
  - Guardar o resultado do item 0.1 e o veredito humano (aceito ou não).
  - **Custo:** médio, mas é uma vez só.

### 0.3 O custo é medido por tarefa aceita?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - Por spec existem contagens de vereditos aprovados e rejeitados, ondas retrabalhadas, chamadas recusadas, bloqueios e avisos (`page.rs:873-912`).
  - O gasto total por spec é somado e mostrado (`apps/rt/src/commands/spec_events/pages/copy.rs:718-740`: "Total spend: {waves} wave tokens + {caller} orchestrator tokens").
  - Nada divide custo por spec entregue, e nada agrega entre specs.
- **Correção:** depois de 0.1, um comando `mustard-rt run metrics` que varre todos os `spec.ndjson` e calcula `soma(tokens) ÷ número de specs em fase delivered`. **Custo:** baixo, porque os dados já estão nos eventos.

### 0.4 `/usage` e `/context` com o Mustard ligado e desligado
- **Veredito:** PENDENTE — manual; não roda neste ambiente.
- **Estimativa pelo código [MÉDIA]** (bytes ÷ 4 ≈ tokens):
  - Início de sessão: teto de 3.000 bytes, cerca de 750 tokens (`apps/rt/src/hooks/session/session_start_inject.rs:72`).
  - Estilo de saída: 3,4 kB, cerca de 850 tokens (`plugin/output-styles/mustard-pt-BR.md`, 57 linhas).
  - Por prompt: no máximo 100 caracteres.
  - Definições dos 4 agentes: os campos `description` entram na lista de agentes disponíveis; o corpo só entra no subagente.
  - Comandos do plugin: `continue.md` 1,1 kB, `pr.md` 2,0 kB e `upsert.md` 2,9 kB, carregados só quando chamados.
- **Observação:** a sobrecarga fixa é **baixa**. Se o Mustard é caro, o custo está no fluxo (seções 3 e 4), não no prefixo.
- **Correção:** rodar `/context` uma vez em cada modo e registrar os números neste documento.

---

## 1. Sobrecarga fixa

### 1.1 Definições de ferramentas MCP
- **Veredito:** NÃO SE APLICA / ATENDE [ALTA]
- **Evidência:**
  - "The harness declares no MCP server, so init writes no `.mcp.json`" (`apps/cli/src/commands/init/mod.rs:385-388`).
  - Não há `rmcp` nos `Cargo.toml` e não há `mcpServers` em `plugin/.claude-plugin/plugin.json`.

#### 1.1a Quantas ferramentas o MCP do Mustard expõe? Schemas enxutos?
- **Resposta:** zero ferramentas. Não há schema.
- **Sobras a limpar:**
  - `.gitignore:164` lista `plugin/bin/mustard-mcp`;
  - `IMPLEMENTACAO-MUSTARD-2.md:95` planeja "MCP find_anchors + rank_files".
- **Correção:**
  - Remover a linha do `.gitignore`.
  - Se o MCP planejado for feito, limitar a no máximo 2 ferramentas, com descrição de uma frase e sem exemplos no schema.

#### 1.1b Dá para expor menos ferramentas por fase?
- **Resposta:** não se aplica a MCP.
- **Observação [MÉDIA]:** o problema da issue #37793 (subagente herdando os schemas MCP do agente principal) continua possível com os **servidores MCP que o próprio usuário instalou**. O Mustard mitiga isso porque os 4 agentes declaram `tools:` restrito, por exemplo `tools: Read, Grep, Glob, Edit, Write, Bash` (`packages/core/templates/agents/pt-BR/wave.md:4`). Não verifiquei se o Claude Code deixa de carregar os schemas MCP quando `tools:` é restrito ou se só bloqueia a chamada.
- **Correção:** medir com `/context` dentro de um subagente de onda num ambiente com MCP instalado.

### 1.2 Liga ou desliga MCP no meio da sessão?
- **Veredito:** ATENDE [ALTA]
- **Evidência:** nenhum código faz isso. O único trecho relacionado **preserva** `enabledMcpjsonServers` intacto (`apps/rt/src/hooks/session/statusline_heal_observer.rs:49`).

### 1.3 Arquivos de contexto do repositório (CLAUDE.md, AGENTS.md)
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - O Mustard nunca escreve `CLAUDE.md` nem `AGENTS.md`: "no `CLAUDE.md` is ever written" (`apps/rt/src/commands/scan.rs:48`).
  - Conteúdo antigo de instalações anteriores é limpo (`packages/core/src/platform/project_seed/cleanup.rs`).
  - O que o `init` escreve:
    - `mustard.json`;
    - `.claude/settings.json`;
    - `.claude/.gitignore`;
    - `.claude/mustard/session-map.md` (37 linhas, cerca de 2 kB);
    - 4 agentes (31 a 45 linhas cada);
    - 2 modelos de página HTML (`spec.html`, com 97 kB, e `project.html`).

#### 1.3a O Mustard gera ou injeta visão geral do repositório?
- **Resposta:** injeta, sim. É o "terreno": uma linha por subprojeto (`nome · tipo · Nf — papel`), até 16 linhas (`TERRAIN_ROWS_CAP`, `apps/rt/src/commands/orient.rs:192-223`), em todo início de sessão.
- **Observação:** é exatamente o tipo de conteúdo que o estudo da ETH Zurich diz não ajudar (visão geral do projeto). Já o `session-map.md` tem regras de fluxo, não arquitetura, e isso está de acordo com a recomendação.
  - **Confiança de que tirar o terreno economiza algo relevante:** BAIXA. São no máximo 16 linhas, e o terreno pode ajudar o modelo a escolher o subprojeto em monorepos.
- **Correção:** tirar o terreno da injeção padrão (`session_start_inject.rs:111-122`) e deixá-lo sob demanda (`mustard-rt run orient`). **Só depois de medir** com o benchmark do item 0.2.

### 1.4 CLAUDE.md abaixo de ~200 linhas
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - O Mustard não escreve `CLAUDE.md`.
  - O total injetado no início da sessão tem teto de 3.000 bytes, cortando os avisos de menor prioridade (`session_start_inject.rs:175-199`).
  - O `CLAUDE.md` do próprio repositório tem 6 linhas.
- **Observação:** o `session-map.md` é reinjetado em **todo** início de sessão, inclusive depois de `/clear`, `compact`, `resume` e `fork` (`once: false`, `packages/core/src/platform/project_seed/files.rs:228-232`). Isso é intencional (o mapa precisa sobreviver à compactação) e tem custo baixo (2 kB).

---

## 2. Cache de prompt

### 2.1 O prefixo é idêntico entre chamadas?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - O estilo de saída é estático, sem data nem contador.
  - O terreno é "deterministic and byte-stable — everything is sorted, no timestamps leak" (`orient.rs:31`).
  - A linha por prompt é fixa (`prompt_entry.rs:18-28`).
  - O único `now()` nos hooks grava em disco, não no contexto (`apps/rt/src/hooks/observe/wave_alive_observer.rs:94`).
- **Observação:** a linha por prompt ganha, uma vez, uma nota depois de uma resposta sinalizada (ex.: "Na última resposta: frase com 29 palavras", `prompt_entry.rs:116-120`). Ela entra no fim, na mensagem do usuário, e por isso não invalida o prefixo em cache.

### 2.2 O conteúdo variável fica no fim?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - Os valores voláteis ficam no `additionalContext` do início da sessão, uma vez por sessão: linha de retomada, pendências, branches mescladas, disco, versão e processos travados (`session_start_inject.rs:111-122`).
  - Os avisos por turno são condicionais e entram no resultado da ferramenta: `write_gate`, `command_guard` e `approval_witness`.

### 2.3 Modelo e esforço definidos antes da sessão?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - Os 4 agentes fixam `model: opus` e `effort: xhigh` no frontmatter (`packages/core/templates/agents/*/wave.md:5-6`, e o mesmo em `review.md`, `wave-solo.md` e `skill.md`).
  - O binário também fixa: `requested_model` retorna sempre "Opus" (`packages/core/src/domain/wave_prompt.rs:70-78`).
  - Nada troca o modelo da sessão principal: não há `ANTHROPIC_MODEL`, `CLAUDE_CODE_SUBAGENT_MODEL` ou `/model` no código.
- **Observação:** cada subagente tem cache próprio. O modelo fixo por agente é bom para o cache dele.

### 2.4 Sessões longas paradas e expiração do cache
- **Veredito:** NÃO ATENDE [ALTA quanto à ausência; MÉDIA quanto ao impacto]
- **Evidência:**
  - Nenhum código trata expiração de cache.
  - O Mustard **recomenda** `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE = "15"` (`apps/rt/src/hooks/session/conversation_size.rs:30-34`).
  - O próprio repositório registra, verificado contra o Claude Code 2.1.278, que esse valor "reduz ainda mais a fração da janela efetiva na qual o corte dispara" (`docs/2026-07-25-revisao-portoes-pipeline-ondas.md:199`). Ou seja, compactação mais cedo e mais frequente.
  - Em toda compactação, o hook `PreCompact` injeta o bloco de retomada (`conversation_size.rs:1-10`), e o início de sessão reinjeta o mapa.
- **Observação:**
  - Cada compactação reescreve o histórico. O prefixo de conversa em cache se perde, e a próxima chamada paga o contexto novo a preço cheio.
  - Compactar cedo também tem vantagem: turnos menores. Qual efeito ganha é empírico.
  - A variável **não tem documentação pública**, e uma atualização do Claude Code pode mudar seu significado sem aviso. Isso é um risco à parte.
- **Correção:**
  1. Contar as compactações por spec (um evento no `PreCompact`; o hook já existe).
  2. Rodar o benchmark com 15 e com 50.
  3. Decidir pelo número.

---

## 3. ANALYZE / PLAN (levantamento `survey` e plano `plan`)

### 3.1 Tamanho da especificação por tarefa

#### 3.1a Quantas linhas o Mustard gera por tarefa? Existe limite?
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - A spec não é texto em linhas: são eventos.
  - **Não há teto para a spec inteira** nem para o número de itens combinados ou de critérios.
  - Os tetos que existem são por campo:
    - título de tarefa: 70 caracteres (`apps/rt/src/commands/spec_events/refusal.rs:175`);
    - entrega de onda: 8.000 caracteres (`types.rs:274`, verificado em `check.rs:324-325`);
    - mensagem de commit: título de 60 e corpo de 4.000 caracteres (`apps/rt/src/commands/spec_events/message.rs:12-14`);
    - pedido de onda: 25.000 tokens estimados, "O pedido não tem teto de linhas" (`wave_prompt.rs:22, 193`; aplicado em `apps/rt/src/commands/flow/round/answer.rs:599-600`).
  - Há um **piso**: o levantamento exige um ponto por lacuna do tipo de trabalho, podendo ser marcado `not_applicable`:
    - funcionalidade: 9 lacunas;
    - correção: 5 lacunas;
    - refatoração: 9 lacunas (`packages/core/src/domain/survey.rs:241-270`).
  - O levantamento também puxa lições do banco e specs anteriores parecidas, via busca BM25, e até 3 lembretes (`survey.rs:4-14`). Isso aumenta o texto do survey.
- **Observação:** sem o item 0.1, não é possível dizer se o survey consome os ~35% do benchmark da Uvik. O piso de 9 pontos numa funcionalidade pequena é um candidato a desperdício [MÉDIA].
- **Correção:** um **aviso** (não recusa) no `plan` quando os itens combinados passarem de 25 ou os critérios passarem de 8. Serve como sinal do caso "bug pequeno que virou 16 critérios". Os números são arbitrários; ajustar pelo benchmark.

### 3.2 Rota rápida para tarefa pequena

#### 3.2a Critério da Uvik (spec só se 3 ou mais sinais forem verdadeiros)
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:** nenhum dos 6 sinais é usado para rotear. O tipo de trabalho (`feature`, `fix`, `refactor`, em `types.rs:282`) só escolhe as perguntas do survey, não o caminho.
- **Sinais que o Mustard já tem como calcular deterministicamente, sem IA:**

| Sinal | Pode calcular? | De onde vem |
|---|---|---|
| (1) 3 ou mais arquivos | sim | `task.files` do plano (`apps/rt/src/commands/flow/plan.rs:539-551`) |
| (2) 4 ou mais critérios | sim | número de eventos `criterion` |
| (3) cruza módulo/serviço | sim | subprojetos distintos no `grain.model.json` |
| (4) muda schema ou API pública | parcial | há eventos `contract`; não há item de "mudança de dados" (ver 3.4) |
| (5) mais de um agente/pessoa | sim | ondas maiores que 1 |
| (6) código regulado | não | precisaria de configuração no `mustard.json` |

- **Problema de ordem [MÉDIA]:** vários sinais só existem **depois** do plano. Para usá-los na rota, é preciso ou estimar antes (pelo mapa, com `suggested_files`) ou fazer a rota curta no ponto em que o plano está pronto: dispensar onda e revisão, e não o survey.

#### 3.2b O Mustard tem "modo leve" automático?
- **Veredito:** NÃO ATENDE. É o **principal gargalo provável** [ALTA quanto ao fato; MÉDIA quanto ao tamanho do impacto].
- **Evidência:**
  - "A request that changes a file opens a spec" (`packages/core/templates/mustard/en-US/session-map.md:7`).
  - O `write_gate` bloqueia a escrita sem spec aprovada (`apps/rt/src/hooks/write/write_gate.rs:14-17`).
  - O modo `--condensed` só junta os pontos do survey num bloco (`apps/rt/src/commands/flow/grill.rs:19-20`). Plano, aprovação, rodada com subagente, revisão final e close continuam obrigatórios.
  - O `close` exige a revisão final: "Nenhuma spec fecha sem ele — nem a de uma onda só" (`apps/rt/src/commands/flow/close.rs:12-14`).
  - A onda de uma tarefa usa o `wave-solo` (`wave_prompt.rs:43-49`), que também é Opus/xhigh.
  - O `Scope::Light` ("PLAN is skipped") existe, mas é código morto (`pipeline.rs:51-57`).
  - **Consequência:** uma correção de 1 linha custa no mínimo 2 subagentes Opus/xhigh (onda + revisão), mais o survey e o plano na sessão principal.
- **Observação:** mudar isso mexe na tese do projeto ("o pipeline completo é a exceção que precisa se justificar", segundo o próprio README, que o código atual não cumpre). O ponto de menor risco é manter o determinístico e cortar a IA:
  - **manter:** spec mínima, critério com `proof` executado pelo binário no close e lint/testes;
  - **dispensar:** subagente de onda (a sessão principal edita) e revisão final, quando os sinais do 3.2a derem menos de 3.
- **Correção:** a decisão é sua. **Custo:** alto.

### 3.3 Verbosidade e repetição

#### 3.3a O PLAN duplica o ANALYZE? A spec repete código que já existe?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - Cada item é gravado uma vez, como evento. O pedido da onda leva **códigos** dos itens, não o texto: "O pedido leva a lista, não o texto" (`wave_prompt.rs:10-12`). As exceções são `done_when` e o bloco de leitura por tarefa.
  - Os agentes leem cada item sob demanda (`run read --term`).
  - Fatos do levantamento citam `arquivo:linha` em vez de copiar código (`apps/rt/src/commands/spec_events/check.rs:370-381`). A citação é verificada: o arquivo precisa existir e ter a linha (`packages/core/src/domain/citation.rs:5-6`).
- **Observação:** `rule` e `contract` exigem `example`, `edge_case` exige `expected` e `error` exige `message` (`types.rs:418-422`). São exemplos ilustrativos, não cópia de código, e ajudam a precisão. A repetição que Böckeler viu (entre specs e com o código) está estruturalmente contida aqui.

### 3.4 Completude da especificação (8 itens)
- **Veredito:** PARCIAL. Cobre 6 de 8 [ALTA]

| Item | Presente? | Evidência |
|---|---|---|
| Objetivo | sim | primeiro `context` (`grill.rs:5-6`; `types.rs:426`) |
| Escopo | sim, exceto em `fix` | `out_of_scope` (`types.rs:423`); lacuna "What stays out" |
| Entradas/saídas | sim | `contract` com exemplo real (`types.rs:420`) |
| Mudanças de dados | **não** | não há item de schema/migração; só `contract`/`rule` genéricos |
| Casos de borda | sim | `edge_case` com `expected` (`types.rs:422`) |
| Testes de aceitação | sim, e forte | `criterion` com `when`/`then`/`proof` e forma EARS (`types.rs:291-292, 429-446`) |
| Restrições | sim | `rule`, `limit`, `decision` com `why` (`types.rs:418-424`) |
| O que NÃO pode mudar | **só em `refactor`** | lacuna `MustNotChange` (`survey.rs:257`) |

#### 3.4a O template tem a "lista do que não pode mudar"?
- **Veredito:** NÃO ATENDE para `feature` e `fix` [ALTA]
- **Evidência:**
  - `fix` tem só `Symptom, Reproduction, ExpectedVsActual, Cause, DoneProof` (`survey.rs:253`). Não tem nem `OutOfScope`.
  - `feature` não tem `MustNotChange` (`survey.rs:243-252`).
- **Observação:** é o caso em que a falta mais dói. Uma correção sem lista do que não pode mudar é exatamente onde o agente "conserta" demais.
- **Correção:** acrescentar `Self::MustNotChange` às listas `feature` e `fix`, e `Self::OutOfScope` a `fix`. São duas linhas, mais o ajuste dos testes que contam as lacunas. **É a correção mais barata da auditoria.**
- **Correção complementar:** acrescentar uma lacuna "Mudança de dados/schema" em `feature`, que custa um item novo no enum e um texto i18n (internacionalização, os textos traduzidos).

---

## 4. EXECUTE (rodada de ondas)

### 4.1 Falsa sensação de controle

#### 4.1a Marca explicitamente o que é existente e o que é a criar?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - `task.files` é uma lista de `{path, new?}`. `declared_files` devolve "o caminho e se ela o marcou como novo" (`plan.rs:539-551`).
  - Arquivo sem `new` que não existe = o plano é recusado (`plan.rs:417-423`).
  - Linha citada além do fim do arquivo = recusa.
  - Tarefa sem arquivo = recusa, com sugestões do mapa (`plan.rs:411-416`).
- **Observação:** só existe `new: true` ou sem marca. Não há "modificar" explícito, mas a ausência de `new` equivale a isso.

#### 4.1b Existe verificação determinística de que nada foi duplicado?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - O inverso não é checado: `new: true` sobre arquivo que **já existe** passa. O código só testa `if !new && … is_none()` (`plan.rs:417`).
  - Não há checagem de símbolo novo que já existe no projeto.
  - O antigo `dependency_precheck` foi removido. As fixtures em `apps/rt/tests/fixtures/dependency_precheck/` ficaram órfãs (nenhum teste as usa).
  - Restam referências velhas em `packages/core/src/lib.rs:110` e `packages/core/src/domain/source_lang.rs:10`.
  - Antes da execução, o que existe é só aviso: nome de código citado que o mapa não conhece, e tarefa que poderia usar uma skill existente.
- **Correção:**
  1. **Mínima:** recusar `new && world.file_lines(path).is_some()` em `plan.rs:417`. É uma condição e uma variante de `Finding`.
  2. **Seguinte:** avisar quando um nome declarado como novo numa tarefa já existe no `grain.model.json` (o mapa já tem as declarações).
  3. **Limpeza:** apagar as fixtures órfãs e as referências mortas.

### 4.2 Saída ruidosa de comandos

#### 4.2a Builds e testes rodam com flags silenciosas?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - O `rtk` ("Rust Token Killer", ferramenta externa que filtra saída de comandos) é obrigatório no `init` (`apps/cli/src/commands/init/tools.rs:30-60`).
  - Vem ligado por padrão como hook `PreToolUse` de Bash, `rtk hook claude` (`packages/core/src/platform/project_seed/settings.rs:394-410`; `packages/core/src/domain/config.rs:575-576`).
  - O Mustard reescreve `cargo` com caminho completo para `rtk cargo` e força o comando para primeiro plano (`apps/rt/src/hooks/bash/waiting.rs:112-128`).
  - Os agentes são orientados a rodar a suíte uma vez, no fim, via `rtk`, "which shows only the failures" (`packages/core/templates/agents/en-US/wave.md:23`).

#### 4.2b Existe hook que resume logs longos antes de o modelo ver?
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - O `rtk` atua **antes** da execução, reescrevendo o comando, e só para os comandos que conhece.
  - Não há `PostToolUse` genérico que corte saída longa. Os `PostToolUse` do Mustard são só `approval_witness` e `wave_alive_observer` (`apps/rt/src/registry.rs:117-156`).
  - A saída do QA no close é truncada (`apps/rt/src/commands/review/qa_run/runner.rs:506-507`).
- **Correção:** um `PostToolUse` de Bash que, acima de N linhas, entregue as primeiras e últimas linhas mais as linhas com "error"/"fail". **Prioridade baixa**, porque o `rtk` cobre os casos comuns [MÉDIA].

### 4.3 Subagentes para trabalho verboso
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - Há 4 agentes, todos com `tools:` restrito:

| Agente | Ferramentas |
|---|---|
| `mustard-wave` e `wave-solo` | Read, Grep, Glob, Edit, Write, Bash |
| `mustard-review` | Read, Grep, Glob, Bash |
| `mustard-skill` | Read, Grep, Glob, Write |

  - A execução pesada vai para subagente. A entrega volta com teto de 8.000 caracteres.
  - Não há agente dedicado a **leitura ampla**. O mapa da sessão só orienta: "Hand to an agent any investigation that opens many files" (`session-map.md:22`).
  - A leitura ampla é substituída por consultas determinísticas ao mapa: `mustard-rt run map examples|importers|slice|users` (`MUSTARD-COMMANDS.md:83`).
- **Observação:** substituir leitura ampla por consulta ao grafo é melhor que um subagente explorador. É a tese do projeto, e está coerente.
- **Correção:** nenhuma necessária.

### 4.4 Referência direta a arquivos (PLAN entrega os caminhos exatos?)
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - `task` tem `files`, `must_read`, `depends_on` e `skill` (`types.rs:479-504`). `files` e `depends_on` são obrigatórios (`apps/rt/src/commands/spec_events/write.rs:574-590`).
  - O pedido da onda lista, por tarefa, os arquivos, o que ler antes e os testes conhecidos de cada arquivo (`wave_prompt.rs:1235-1245`).
  - O hook `subagent_inject` troca o bilhete `MUSTARD-WAVE: <spec> <n>` pelo pedido montado (`apps/rt/src/hooks/task/subagent_inject.rs:1-16`).
- **Observação:** a menção com `@` é recurso da mensagem do usuário e não se aplica ao prompt de um subagente. O equivalente aqui são os caminhos exatos mais a instrução "Read by excerpt" (`wave.md:22`), e isso está atendido.

### 4.5 Trabalho em blocos com contexto limpo (resumo compacto entre fases?)
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - A retomada lê só o estado: "Nada mais do arquivo de eventos é lido" (`apps/rt/src/commands/flow/resume.rs:3-7`).
  - `/mustard:continue` apenas roda a retomada (`plugin/commands/continue.md`).
  - Cada onda é um subagente com contexto novo, que recebe só códigos de itens.
  - A entrega que volta à janela principal tem teto de 8.000 caracteres.
  - Não há histórico repassado entre fases.
- **Observação:** a sessão principal (que conduz survey, plano, aprovação e close) acumula contexto a spec inteira. A compactação a 15% (item 2.4) é o mecanismo que limita isso hoje.

---

## 5. REVIEW / QA

### 5.1 Desvio entre especificação e código

#### 5.1a O REVIEW compara o diff com cada critério, item por item?
- **Veredito:** PARCIAL. O lado determinístico é forte [ALTA]
- **Evidência:**
  - **Itens combinados:**
    - A revisão final precisa responder `met` para **todo** item combinado; faltar um = veredito recusado (`AgreedItemsMissing`, `apps/rt/src/commands/flow/round/report.rs:625-663`).
    - Item com `met: false` força `rejected` e vira tarefa nova.
    - Os tipos cobertos são `rule`, `limit`, `contract`, `error`, `edge_case`, `out_of_scope` e `decision` (`types.rs:418-424`).
  - **Critérios:**
    - O binário **executa** o `proof` de cada critério no close e grava `criterion_run` (`apps/rt/src/commands/flow/close.rs:519-546`).
    - Prova verde que rodou zero testes é recusada (`CriterionRanNoTest`, `close.rs:113-114`).
    - Os critérios das ondas também rodam antes de cada commit (`apps/rt/src/commands/flow/round/commit.rs:763-778`).
  - **Julgamento do modelo:**
    - "a revisão final do conjunto não confere critério nenhum" (`types.rs:634-636`).
    - O `met` e a pergunta "o teste verifica a regra?" (`tests_rule`) são julgamento do modelo (`review.md:21`).
  - **Na aprovação:** grava uma tabela de rastreio item → verificação → arquivo → atendido (`close.rs:673-706`).
- **Observação:** executar a prova é o ponto certo para ser determinístico. O `met` via IA é inevitável para regras em linguagem natural.

#### 5.1b Detecta comportamento adicionado que a spec não pediu?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - Arquivos entregues diferentes dos alterados geram **aviso** (`files-diverged`, `report.rs:251-283`). Isso compara com o que o agente declarou, não com o plano.
  - O agente é **autorizado** a sair da lista: "A file outside the list that the same change needs is part of the work" (`wave.md:30`).
  - Teste criado sem critério que o cite = aviso (`unowned-test`, `close.rs:905-908`).
  - Item combinado que nenhuma onda carregou = aviso (`close.rs:401-411`).
  - O antigo portão de fronteira foi removido. O `write_gate` não tem regra de arquivos planejados (`apps/rt/src/hooks/write/write_gate.rs:1-25`).
  - O único freio é o item `out_of_scope`, julgado pela IA.
- **Correção de menor custo:**
  - No close, calcular deterministicamente os arquivos do diff que não estão em nenhum `task.files`.
  - Exigir que o veredito final responda por cada um, com o mesmo mecanismo do `AgreedItemsMissing`.
  - Não bloqueia; obriga a justificar. **Custo:** baixo.

### 5.2 Revisor independente (roda com contexto limpo?)
- **Veredito:** ATENDE, com ressalva [ALTA]
- **Evidência:**
  - Subagente separado (`mustard-review`), rodando numa cópia própria criada no commit do trabalho (`packages/core/src/platform/i18n/prompt.rs:262-264`).
  - Entrada montada pelo binário só com códigos, "No text is copied in" (`prompt.rs:103`; montagem em `wave_prompt.rs:1123-1141`).
  - Uma única revisão final por spec (`close.rs:12-14`).
  - O modelo do revisor diz: "You are not who did it, and accept no unconfirmed claim" (`review.md:9`).
- **Ressalva [MÉDIA]:** o revisor recebe "What each wave delivered", o texto que o executor escreveu, inclusive "o que decidiu fora do pedido" (`wave.md:38`). Não vê o raciocínio, mas vê a narrativa do executor. Isso pode ancorar o revisor.
- **Correção:** tirar o texto da entrega da entrada do revisor e deixar só diff, itens e commits. Testar antes/depois no benchmark (item 0.2), porque o texto também pode ajudar o revisor a achar o que olhar.

### 5.3 Métricas de qualidade objetivas
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - **Existe, por spec:**
    - vereditos aprovados/rejeitados;
    - ondas retrabalhadas;
    - tempo por fase.
    - Os tipos de evento de métrica são `injection, hook, call, state, verdict, point, send, delivered` (`types.rs:65-69`), e aparecem na página da spec (`packages/core/templates/pages/spec.html:907-934`).
  - **Teto de retrabalho:** 2 rodadas de correção; depois, a decisão vai ao usuário (`MAX_FIX_ROUNDS`, `apps/rt/src/commands/flow/round/stops.rs:15`).
  - **Falta:**
    - defeitos por tarefa aceita;
    - intervenções humanas (as mensagens do usuário existem como eventos `message` com `author: "user"`, mas não são contadas);
    - qualquer agregação entre specs.
  - A pasta `.claude/.metrics/` está declarada ("telemetry rollups", `packages/core/src/io/claude_paths.rs:279-283`), sem nenhum chamador.
  - O único armazenamento entre specs é o banco de lições (`.claude/spec/lessons.ndjson`, `packages/core/src/domain/lessons.rs:1`), que fica só na máquina do desenvolvedor.
- **Correção:** o mesmo `mustard-rt run metrics` do item 0.3, emitindo:
  - specs entregues;
  - rejeições por spec entregue;
  - rodadas de correção;
  - mensagens do usuário após a aprovação (como medida de intervenção humana);
  - specs descartadas.
  - Usar `.claude/.metrics/` ou remover a declaração.

### 5.4 Divisão de modelos
- **Veredito:** NÃO ATENDE, por decisão explícita [ALTA]
- **Evidência:**
  - Todos os agentes são Opus/xhigh (`wave_prompt.rs:70-78`: "todo agente do Mustard sai em Opus").
  - Um teste **proíbe** "Sonnet" (`report.rs:2341-2346`).
  - A spec e o plano são escritos pela sessão principal, com o modelo que o usuário escolheu.
- **Observação:** o checklist marca isso como hipótese sem resultado publicado. Trocar sem benchmark seria mudança às cegas. Pelo volume, a onda é o candidato natural ao primeiro teste.
- **Correção:** depois do item 0.2, testar `effort: high` na onda (mudança menor que trocar o modelo) e comparar custo e aceitação.

### (extra) O que o QA faz de fato
- **Resposta [ALTA]:** não há fase QA. O QA vive dentro do `run close` (`close.rs:491-546`):
  1. Roda `lintCommand` e `testCommand` do `mustard.json` num ambiente limpo, com HOME novo e sem identidade git (`clean_env`, `close.rs:566`).
  2. Roda cada `proof` uma vez.
  3. Recusa o close em qualquer falha (`close.rs:113-127`).
- **Bloqueios adicionais** (`close.rs:727`):
  - onda sem commit;
  - onda rejeitada sem correção;
  - tarefa pendente;
  - pedido do usuário não entregue.
- **Em cada rodada:** o build precisa passar (`commit.rs:743-752`).
- **Avaliação:** é determinístico e sólido.

---

## 6. CLOSE (especificação viva)

### 6.1 A spec é descartada, mantida ou mesclada numa especificação viva?
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - A spec fechada é **mantida** como está: "A pasta de uma spec fechada fica com o arquivo de eventos e a pasta da cópia (`copy/`), e nenhuma página" (`close.rs:9-10`).
  - Nada é mesclado numa spec por módulo.
  - Na descoberta, a pasta é arquivada, mas continua no índice (`apps/rt/src/commands/flow/discard.rs`).
  - O `grain.model.json` é atualizado depois de cada commit de rodada (`commit.rs:260-268`). Isso é estrutura, não comportamento.
  - **O que existe e funciona como especificação viva parcial:** o levantamento de uma spec nova busca, por BM25, as specs anteriores que casam com o objetivo e traz as regras, decisões e erros delas, além das lições do banco (`packages/core/src/domain/survey.rs:8-14`).
- **Observação:** a busca por semelhança de texto reaproveita conhecimento, mas não garante encontrar a regra que vale para o **arquivo** que será mexido.
- **Correção de menor custo:**
  - No close, gravar um índice `arquivo → [rule/contract aprovados]` a partir da tabela de rastreio (`close.rs:673-706`), que já liga item a arquivo.
  - O `plan` de specs futuras avisa quando uma tarefa toca um arquivo com regras anteriores.
  - **Custo:** médio.

### 6.2 Risco de rigidez (antigo MDD) e não determinismo
- **Veredito:** MITIGADO [MÉDIA]
- **Evidência:**
  - A spec não é fonte principal: o código continua sendo.
  - Os critérios são provas executáveis (`proof`), não descrições.
  - O código não é gerado a partir da spec.
  - A não determinação do modelo é contida pelos portões determinísticos: prova executada, build, lint e testes em ambiente limpo.
- **Risco residual:** se a correção do item 6.1 virar "regras que bloqueiam", nasce a rigidez. Mantenha como **aviso**.

---

## 7. Diferencial em sistemas legados

### 7.1 O Mustard funciona bem em código existente? Há números antes/depois?
- **Veredito:** PARCIAL [ALTA quanto ao mecanismo; sem dado quanto ao resultado]
- **Evidência:**
  - O scan determinístico para `.claude/grain.model.json` foi feito para entrar em repositório existente: "Mustard never reads project source to understand a repo" (`apps/rt/src/commands/scan.rs:1-11`).
  - O `init` lida com `.claude/` existente por sobrescrita, mesclagem ou cópia de segurança (`apps/cli/src/commands/init/mod.rs:13-15`), e recusa subpastas de um repositório.
  - A skill do projeto é gerada a partir de exemplos que o binário escolhe no código real (`packages/core/templates/agents/en-US/skill.md:3`).
  - Os fatos do survey citam `arquivo:linha` verificados.
  - **Não há nenhuma medição antes/depois**, nem modo "legado" explícito.
- **Correção:** rodar o benchmark do item 0.2 num repositório legado de cliente, com e sem o Mustard. É o dado que transforma o diferencial em argumento.

---

## Ressalva sobre o benchmark da Uvik (mantida do checklist)
- Os números da Uvik são o único estudo controlado citado (50 tickets reais em Python, 5 fluxos, revisão cega).
- A página tem uma inconsistência: foi publicada em 24/09/2026 e informa execução em 15/10/2026, uma data posterior à publicação.
- A empresa também vende serviços relacionados.
- Use os números como direção, não como verdade. Os achados da ETH Zurich e de Böckeler são mais confiáveis.
- **Consequência para esta auditoria:** os vereditos dos itens 3.1, 3.2 e 5.1 medem se o Mustard **tem** o mecanismo. Se o mecanismo **compensa**, só o benchmark próprio (item 0.2) vai dizer.

---

## Observações fora do checklist (achados da auditoria)

1. **Documentação desatualizada:** README, `README.en.md` e `CLAUDE.md` da raiz. Detalhado no achado transversal. [ALTA]
2. **Código morto e sobras** [ALTA]:
   - `Scope` (`pipeline.rs:51-57`);
   - `TelemetrySummaryEntry` (`view/summary/mod.rs:191-203`);
   - `base_gate` (desligado por decisão sua em 17/09);
   - fixtures `dependency_precheck`;
   - referências em `lib.rs:110` e `source_lang.rs:10`;
   - `.claude/.metrics/` sem chamador;
   - `.gitignore:164` (`mustard-mcp`).
3. **Variável de compactação sem documentação pública** (`CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`) é recomendada pelo produto. Uma atualização do Claude Code pode mudar o efeito sem aviso. [ALTA quanto ao fato]
4. **Tokens autodeclarados:** o gasto mostrado na página da spec depende de o modelo copiar números corretamente. [ALTA]
5. **Página `spec.html` com 97 kB:** é publicada via ferramenta de artefato quando um comando pede. Se o conteúdo passar pelo contexto do modelo a cada publicação, é custo relevante. **Não verifiquei** se passa [BAIXA]. Medir no item 0.1.
6. **Limites de clareza das respostas** (25 palavras por frase, 15 linhas): só avisam, não bloqueiam (`packages/core/src/domain/clarity/mod.rs:36,40`). É coerente com o custo baixo. [ALTA]

---

## Prioridade (custo × impacto) — recomendação

| # | Ação | Itens | Custo | Por quê |
|---|---|---|---|---|
| 1 | Medição real de tokens por fase via transcript | 0.1, 0.3, 5.3 | médio | Sem isso, toda mudança de custo é aposta |
| 2 | `MustNotChange` em feature/fix e `OutOfScope` em fix | 3.4a | mínimo | Duas linhas; o item "mais barato e mais esquecido" |
| 3 | Recusar `new: true` sobre arquivo existente | 4.1b | mínimo | Uma condição |
| 4 | Corrigir README, `README.en.md` e `CLAUDE.md`; remover código morto | transversal, obs. 2 | baixo | Documentação errada engana pessoas e modelo |
| 5 | Arquivos fora do plano respondidos na revisão | 5.1b | baixo | Fecha o maior buraco de qualidade |
| 6 | Conjunto fixo de tarefas (benchmark) | 0.2, 7.1 | médio | Decide os itens 7 a 9 |
| 7 | Rota direta para tarefa pequena | 3.2 | alto | Maior ganho provável de tokens; exige sua decisão |
| 8 | Medir e decidir: `AUTOCOMPACT=15`, texto do executor no revisor, `effort` da onda, terreno | 2.4, 5.2, 5.4, 1.3a | baixo (depois de 1 e 6) | Hipóteses que só o número decide |
| 9 | Índice arquivo → regras aprovadas | 6.1 | médio | Especificação viva sem rigidez |

---

## Fontes (do checklist original, não verificadas nesta auditoria)
- Uvik Software — Spec-Driven Development Benchmark 2026: https://uvik.net/spec-driven-development-benchmark/
- Gloaguen et al. (ETH Zurich) — Evaluating AGENTS.md: https://arxiv.org/abs/2602.11988
- Böckeler (Thoughtworks) — Understanding Spec-Driven-Development: https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html
- Anthropic — Maximizing the value of your Claude Code sessions: https://claude.com/blog/maximizing-the-value-of-your-claude-code-sessions
- Claude Code Docs — Manage costs effectively: https://code.claude.com/docs/en/costs
- Issue #37793 (subagentes herdando schemas MCP): https://github.com/anthropics/claude-code/issues/37793
- StationX — Reduce Claude Code token usage: https://app.stationx.net/articles/reduce-claude-code-token-usage
- Boringbot — How to save millions in Claude tokens: https://boringbot.substack.com/p/how-to-save-millions-in-claude-tokens
- JuanjoFuchs — claude-code-tips: https://github.com/JuanjoFuchs/claude-code-tips
