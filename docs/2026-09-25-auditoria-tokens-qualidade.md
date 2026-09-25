# Auditoria do Mustard: consumo de tokens e qualidade de código

**Data:** 25/09/2026 (revisão 3).
**Base auditada:** branch `dev`. O código é o do commit `7464541`; os commits seguintes só alteram este documento.

## Como esta auditoria foi feita

Foram três passadas.

1. **Leitura do código contra cada item do checklist** (revisões 1 e 2).
2. **Verificação das fontes externas do checklist e da documentação oficial do Claude Code** (revisão 3). As revisões anteriores não abriram nenhuma fonte e só repetiam as confianças do checklist. Isso foi falha da auditoria, não limitação técnica.
3. **Segunda varredura do código** (revisão 3), focada no que a primeira não cobriu:
   - o que os comandos `mustard-rt` imprimem no contexto;
   - a publicação de páginas;
   - o texto que os ganchos injetam;
   - o que é imposto por código e o que é só instrução.

**Citações:** todas as citações `arquivo:linha` foram reconferidas. A revisão 2 tinha 5 erros (3 caminhos inexistentes e 2 linhas deslocadas), todos corrigidos aqui.

**Legenda de veredito:**
- ATENDE;
- PARCIAL;
- NÃO ATENDE;
- NÃO SE APLICA;
- PENDENTE: exige uma execução manual que esta auditoria não fez.

**Legenda de confiança:**
- **[ALTA]:** conferi no código ou li na fonte original.
- **[MÉDIA]:** uma de três situações:
  - inferência feita sem executar nada;
  - fonte lida só por trechos de busca;
  - achado da segunda varredura que não reconferi linha a linha.
- **[BAIXA]:** hipótese.

**Siglas e termos:**
- **MCP** — Model Context Protocol, protocolo de ferramentas externas.
- **SDD** — Spec-Driven Development, desenvolvimento orientado a especificação.
- **MDD** — Model-Driven Development, desenvolvimento dirigido por modelos.
- **EARS** — Easy Approach to Requirements Syntax, formato padronizado de frase de requisito.
- **BM25** — algoritmo clássico de ranqueamento de busca textual.
- **QA** — Quality Assurance, garantia de qualidade.
- **TTL** — Time To Live, quanto tempo o cache de prompt sobrevive sem uso.
- **CLI** — Command Line Interface, interface de linha de comando.
- **JSON** — formato de dados em texto. **JSONL** — um JSON por linha.
- **SHA** — identificador de um commit do git.
- **Subagente** — outra instância do modelo, com contexto próprio, que recebe uma tarefa e devolve um resumo.
- **Onda** — o lote de tarefas que o Mustard entrega a um subagente de execução.

---

## Verificação das fontes externas

**Bloqueio de rede:** a política de rede deste ambiente bloqueou uvik.net, martinfowler.com, arxiv.org, stationx.net e boringbot.substack.com. Nesses casos, a evidência veio de trechos indexados por busca, que o resumidor pode ter parafraseado levemente. Para ler o texto integral, é preciso liberar esses domínios em "Network access" nas configurações do ambiente de nuvem.

| Fonte | Como foi lida | Resultado |
|---|---|---|
| Uvik, benchmark SDD 2026 | Trechos de busca [MÉDIA] | 9 afirmações confirmadas, 2 parciais e 2 não encontradas: a mediana de 93 linhas do OpenSpec e o desvio de 2,4% a 12,5%. A inconsistência de datas foi confirmada. |
| Gloaguen et al., ETH Zurich (arXiv 2602.11988) | v1 (12/02/2026) lida inteira, de uma cópia em PDF [ALTA]; v2 (23/06/2026) só por trechos [MÉDIA] | Confirmado, com nuances (ver 1.3). |
| Böckeler, Thoughtworks (15/10/2025) | Trechos de busca [MÉDIA] | Confirmado, com duas atenuações de redação. |
| Blog da Anthropic (14/08/2026) | Lido [ALTA] | 3 afirmações confirmadas, 2 diferentes e 1 não encontrada. |
| Documentação do Claude Code: `costs`, `prompt-caching`, `sub-agents`, `skills`, `env-vars`, `hooks` | Lida [ALTA] | Corrige quatro premissas do checklist (lista abaixo). |
| Issue anthropics/claude-code#37793 | Lida [ALTA] | É relato de usuário, não declaração da Anthropic. Continua aberta. |
| StationX e Boringbot | Bloqueadas | NÃO VERIFICADO |
| JuanjoFuchs, claude-code-tips | Lido [ALTA] | Parcial (ver itens). |

**Premissas do checklist que a verificação corrigiu:**
1. **"Cada ferramenta MCP leva o schema JSON completo para o contexto."** Está desatualizado. Hoje as definições MCP são carregadas sob demanda por padrão: "only tool names and server instructions enter context until Claude uses a specific tool" (documentação `costs`) [ALTA].
2. **"Conectar/desconectar MCP apaga o cache inteiro."** Só acontece quando as ferramentas estão carregadas no prefixo. No padrão, com carregamento sob demanda, a mudança "only appends new content and doesn't disturb anything already cached" (documentação `prompt-caching`) [ALTA].
3. **"CLAUDE.md abaixo de ~200 linhas [MÉDIA — guias]".** É recomendação oficial: "Aim to keep CLAUDE.md under 200 lines by including only essentials" (documentação `costs`) [ALTA].
4. **"Divisão de modelos [MÉDIA — hipótese sem resultado]".** É recomendação oficial de custo, mesmo sem estudo controlado (ver 5.4) [ALTA quanto à recomendação].
5. **"Trabalho em blocos com contexto limpo", atribuído ao blog da Anthropic.** O blog não diz isso; a frase vem do repositório JuanjoFuchs. O blog diz "/clear when you start something new, and /compact when the earlier part of the same task is done" e "One long session costs more than the same work spread over a few short ones" [ALTA].
6. **"Até 90% de desconto".** O blog diz "Reading from the cache costs 0.1x the input price", o que equivale a 90% de desconto. Uma mudança no prefixo não apaga "tudo": "everything behind it gets prefilled again", ou seja, recalcula só o que vem depois do ponto alterado [ALTA].
7. **Duas atenuações no Böckeler:**
   - "exagerar por seguir regras à risca" é, no original, "too eagerly following instructions" (seguir instruções com avidez demais);
   - "tutoriais quase sempre partem do zero" é, no original, "usually" (geralmente) [MÉDIA].
8. **Métrica principal da Uvik.** A frase "a métrica principal é custo ÷ aceitos" não foi encontrada. A página reporta custo por ticket mesclado (ver 0.3) [MÉDIA].

---

## Achado transversal: a documentação do Mustard descreve um sistema que não existe mais [ALTA]

- **`README.md:77` diz que "O roteador é injetado em todo prompt e classifica o pedido sozinho".**
  - Hoje o texto por prompt é uma linha de até 100 caracteres (`apps/rt/src/hooks/session/prompt_entry.rs:18-28`).
  - Um teste garante isso (`every_message_gets_only_the_short_line`).
  - Não existe mais classificação de pedido.
- **O README descreve `spec.md`, `wave-plan.md` e `wave-N-{role}/spec.md`.**
  - Hoje a spec é um arquivo de eventos, `.claude/spec/<nome>/spec.ndjson`, com 36 tipos de evento (`packages/core/src/domain/spec_events/types.rs:301`).
  - Esse arquivo só é gravado por `mustard-rt run write`.
- **O README cita "≥2 camadas/subprojetos ou entidade nova"** como critério do fluxo completo. Esse critério só existe no enum `Scope` (`Light`, `Medium`, `Full`, em `packages/core/src/domain/model/pipeline.rs:51-57`), que é código morto no fluxo atual.
- **O `CLAUDE.md` da raiz cita `base_gate.rs:149` como mecanismo ativo.** O arquivo está desligado: "Sem nenhum chamador… segue no repositório por decisão do usuário… Decidido em 17/09" (`apps/rt/src/commands/event/base_gate.rs:46-49`).
- **O README diz que o porteiro de base minera o repositório.** Hoje o scan roda ao abrir uma spec (`apps/rt/src/commands/flow/open.rs:432`) e depois de cada commit de rodada.
- **As fases reais não são as 6 do checklist.** São `survey, plan, approved, running, closed, pr_open, delivered, discarded` (`types.rs:277-278`). O mapeamento usado neste documento:

| Fase do checklist | Fase real do Mustard |
|---|---|
| ANALYZE | `survey` (levantamento, comando `grill`) |
| PLAN | `plan` |
| EXECUTE | `running` (rodada de ondas) |
| REVIEW | revisão final, que roda dentro do `close` |
| QA | lint e testes do `close`; não existe fase QA separada |
| CLOSE | `closed` |

**Correção:** reescrever as seções afetadas do README e do `README.en.md`. Para a regra do `base_gate` no `CLAUDE.md`, há duas saídas: corrigir a regra ou religar o portão. Essa decisão é sua.
- **Custo:** baixo.
- **Risco de não corrigir:** quem lê a documentação, inclusive o modelo quando ela entra no contexto, raciocina sobre um sistema que não existe.

---

## 0. Antes de mexer: medir (linha de base)

### 0.1 Registra tokens de entrada, saída e cache por fase?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - **O único registro de tokens é o evento `send`**, com os campos `tokens`, `steps`, `caller_tokens` e `caller_steps` (`types.rs:555-560`).
  - **Os números são informados pelo próprio modelo** no bloco `<USAGE>…</USAGE>` (`packages/core/src/platform/i18n/flow.rs:612-619`). O parser está em `apps/rt/src/commands/flow/round/report.rs:752-768`, e o bloco só é aceito para ondas (`report.rs:711`).
  - **Não há divisão entre entrada, saída e cache.**
  - **Por fase existe só o tempo** ("Time per phase", `packages/core/src/platform/i18n/page.rs:885-886`).
  - **O struct `TelemetrySummaryEntry` é código morto** (`packages/core/src/view/summary/mod.rs:191-203`).
  - **O tamanho de cada injeção de gancho é registrado**, estimado como caracteres ÷ 4 (`apps/rt/src/dispatch.rs:125-137`; `page.rs:899-900`).
- **Fonte externa (documentação `prompt-caching` e `costs`) [ALTA]:** o Claude Code já entrega os números que faltam, e o Mustard não usa nenhuma destas vias.
  - **Status line:** recebe a cada turno o objeto `current_usage`, com `cache_creation_input_tokens` e `cache_read_input_tokens`, e o objeto `prompt_cache`. O Mustard tem status line própria (`apps/rt/src/commands/statusline/`), mas não grava nada.
  - **`/usage`:** mostra a linha "Prompt cache (main)", com taxa de acerto, falhas e causa provável da última falha (v2.1.251 ou posterior). Nos planos de assinatura, também divide o uso por subagente, skill e plugin.
  - **OpenTelemetry:** exporta tokens de cache por sessão.
- **Correção de menor custo:**
  1. Na status line, gravar o delta de tokens de cada turno junto com a fase corrente. Cobre a conversa principal.
  2. Para os subagentes, somar o `usage` do transcript de cada onda e de cada revisão.
  3. Substituir os números autodeclarados por esses números medidos.
  - **Custo:** médio.

### 0.2 Existe conjunto fixo de tarefas de teste?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - Não há benchmark de ponta a ponta.
  - O único teste de recuperação é o do scan, e ele recusa benchmark curado (`apps/scan/tests/retrieval_self_recall.rs:15`).
- **Fonte externa:** a Uvik publicou os resultados em CSV sob licença CC BY 4.0 [MÉDIA]. O método pode ser copiado:
  - um revisor sênior que não executou a tarefa decide se o código é mesclado;
  - o controle sem spec recebe só o texto do ticket, os critérios e o arquivo de instruções do projeto.
- **Correção:**
  - Criar `bench/` com 10 pedidos reais sobre um repositório congelado: 3 correções, 3 refatorações e 4 funcionalidades, de 1 a 5 arquivos.
  - Rodar cada pedido com o Mustard ligado e com ele desligado.
  - Guardar os números do item 0.1 e o veredito humano.

### 0.3 O custo é medido por tarefa aceita?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - Por spec, existem contagens de vereditos, retrabalho, chamadas recusadas e avisos (`page.rs:873-912`).
  - O gasto total por spec é somado, mas a partir dos números autodeclarados (`apps/rt/src/commands/spec_events/pages/copy.rs:718-740`).
  - Nada agrega esses números entre specs.
- **Fonte externa (Uvik) [MÉDIA]:** a página reporta custo por ticket mesclado.

| Fluxo | Custo por ticket mesclado |
|---|---|
| Kiro | US$ 2,35 |
| Controle sem spec | US$ 2,43 |
| Spec Kit | US$ 3,33 |
| BMAD | US$ 4,23 |

- **Leitura desses números:** mesmo pelos números do próprio fornecedor, o fluxo com spec mais barato só empatou com o controle sem spec, e o mais caro custou 74% a mais.
- **Correção:** depois do item 0.1, criar `mustard-rt run metrics` para calcular `soma(tokens medidos) ÷ specs em fase delivered`.
  - **Custo:** baixo.

### 0.4 `/usage` e `/context` com o Mustard ligado e desligado
- **Veredito:** PENDENTE. É manual e não roda neste ambiente.
- **Estimativa pelo código:**

| Peça | Tamanho | Quando entra no contexto | Confiança |
|---|---|---|---|
| Estilo de saída | corpo de 3.244 caracteres (en-US) / 3.118 (pt-BR) | toda requisição | [ALTA] |
| Mapa da sessão | 2.080 bytes | todo início de sessão, inclusive depois de `/clear`, compactação, retomada e fork | [ALTA] |
| Linha curta por mensagem | 75 a 96 caracteres | toda mensagem | [MÉDIA] |
| Descrições dos 4 agentes | 76 a 158 caracteres cada | lista de agentes | [MÉDIA] |
| Descrições dos comandos do plugin | `continue` 173 e `upsert` 304 caracteres | lista de comandos | [MÉDIA] |

  - O comando `pr` declara `disable-model-invocation: true`. Pela semântica documentada para skills, a descrição dele fica fora do contexto [MÉDIA].
  - O Mustard não instala nenhuma skill (ver 1.5).
- **Total fixo:** cerca de 1.600 tokens por sessão, mais cerca de 20 a 25 por mensagem. É baixo.
- **Observação [ALTA]:** o custo do Mustard não está no prefixo fixo. Está no fluxo: seções 3 e 4 e observações 5, 12 e 13.

---

## 1. Sobrecarga fixa

### 1.1 Definições de ferramentas MCP
- **Veredito:** NÃO SE APLICA / ATENDE [ALTA]
- **Evidência:** "The harness declares no MCP server, so init writes no `.mcp.json`" (`apps/cli/src/commands/init/mod.rs:385-388`).
- **Fonte externa:**
  - A premissa está desatualizada (ver premissa 1).
  - A issue #37793 [ALTA]:
    - é relato de usuário, aberto em 23/03/2026 na versão 2.1.81;
    - envolvia 34 servidores e cerca de 566 ferramentas;
    - erro relatado: "prompt is too long: 209117 tokens > 200000 maximum";
    - continua aberta, sem resposta da Anthropic;
    - não testou subagente com `tools:` restrito.

#### 1.1a Quantas ferramentas o MCP do Mustard expõe?
- **Resposta:** nenhuma.
- **Sobras:**
  - `.gitignore:164` lista `plugin/bin/mustard-mcp`;
  - `IMPLEMENTACAO-MUSTARD-2.md:95` planeja um MCP.

#### 1.1b Dá para expor menos ferramentas por fase?
- **Resposta:** não se aplica a MCP.
- **O que a documentação diz (`sub-agents`) [ALTA]:**
  - os subagentes herdam as ferramentas MCP da sessão principal, "narrowed by two filters";
  - os 4 agentes do Mustard listam só ferramentas nativas em `tools:`, por exemplo `packages/core/templates/agents/pt-BR/wave.md:4`.
- **O que ela não diz:** se as definições das ferramentas fora da lista chegam a ser enviadas. Com o carregamento sob demanda padrão, no máximo os nomes entrariam [MÉDIA].

### 1.2 Liga ou desliga MCP no meio da sessão?
- **Veredito:** ATENDE [ALTA]
- **Evidência:** nenhum código faz isso. `enabledMcpjsonServers` é preservado (`apps/rt/src/hooks/session/statusline_heal_observer.rs:49`).
- **Fonte externa:** a premissa vale só quando as ferramentas estão no prefixo (ver premissa 2).
- **Complemento da documentação `prompt-caching`:** ligar ou desligar um plugin não invalida o cache para as skills, comandos, agentes e ganchos dele. É o caso do Mustard [ALTA].

### 1.3 Arquivos de contexto do repositório (CLAUDE.md, AGENTS.md)
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - O Mustard nunca escreve `CLAUDE.md` nem `AGENTS.md` (`apps/rt/src/commands/scan.rs:48`).
  - O `init` grava:
    - `mustard.json`;
    - `.claude/settings.local.json`, ou `.claude/settings.json` na instalação compartilhada (`apps/cli/src/commands/init/mod.rs:235-239`);
    - `.claude/.gitignore`;
    - os textos de `harness_texts` (`packages/core/src/platform/project_seed/files.rs:46-56`): o mapa da sessão, os 2 modelos de página HTML e os 4 agentes.
- **Fonte externa, ETH Zurich:** "Evaluating AGENTS.md: Are Repository-Level Context Files Helpful for Coding Agents?", de Gloaguen, Mündler, Müller, Raychev e Vechev.
  - **Na versão 2 [MÉDIA]:** "providing context files does not generally improve task success rates, while increasing inference cost by over 20% on average".
  - **Na versão 1 [ALTA], a afirmação é mais forte:** "tend to reduce task success rates".
  - **Números da versão 1 [ALTA]:**
    - arquivos escritos por desenvolvedores: +4% em média;
    - arquivos gerados por modelo: −3% em média, com custo 20% e 23% maior;
    - quando a documentação do repositório foi removida, os arquivos gerados ajudaram (+2,7%). Ou seja, só ajudam quando não repetem o que já existe.
  - **Limite do estudo:** só Python, 138 instâncias de 12 repositórios.

#### 1.3a O Mustard gera ou injeta visão geral do repositório?
- **Resposta:** injeta. É o "terreno": uma linha por subprojeto, no máximo 16 linhas (`TERRAIN_ROWS_CAP`, `apps/rt/src/commands/orient.rs:192-223`), em todo início de sessão.
- **Fonte externa:**
  - "repository overviews, although popular and recommended by model providers, are not helpful" (versão 2) [MÉDIA];
  - na versão 1, as visões gerais não reduziram o número de passos até o agente tocar o primeiro arquivo relevante [ALTA].
- **Observação [BAIXA–MÉDIA]:**
  - O terreno é gerado por um programa determinístico, não por um modelo. Aplicar o estudo a ele é extrapolação.
  - São só 16 linhas.
  - O ganho de tirá-lo é incerto, mas não há evidência de que ele ajude.
- **Correção:** deixar o terreno sob demanda (`mustard-rt run orient`), tirando-o do início de sessão (`session_start_inject.rs:111-122`). Decidir pelo benchmark.

### 1.4 CLAUDE.md abaixo de ~200 linhas
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - O Mustard não escreve `CLAUDE.md`.
  - O início de sessão tem teto de 3.000 bytes (`apps/rt/src/hooks/session/session_start_inject.rs:72`).
  - O `CLAUDE.md` deste repositório tem 6 linhas.
- **Fonte externa:** é recomendação oficial (premissa 3). O JuanjoFuchs é mais rígido: "ideally under ~60 lines" [ALTA].

### 1.5 (novo) Skills: descrições no contexto de toda sessão
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - O `init` não instala skills: "content payload (commands/skills/agents/refs) now ships in the `mustard` plugin — not written" (`apps/cli/src/commands/init/mod.rs`, cabeçalho).
  - O plugin não tem pasta `skills/`.
  - As 4 skills de `apps/cli/templates-extras/skills/` (design-craft, grill-me, hallmark, react-best-practices) estão órfãs: nenhum código ou empacotamento cita essa pasta.
  - O manifesto `apps/cli/templates/.artifacts.json` lista refs e comandos que não existem mais.
- **Fonte externa (documentação `skills`) [ALTA]:**
  - a descrição de cada skill fica sempre no contexto, cortada em 1.536 caracteres;
  - com `disable-model-invocation: true`, a descrição sai do contexto.
- **Observação:** se as skills órfãs voltarem, a descrição do `hallmark` sozinha tem 1.154 caracteres [MÉDIA].
- **Correção:** apagar `templates-extras/skills/` e o `.artifacts.json` velho, ou religá-los conscientemente.

---

## 2. Cache de prompt

### 2.1 O prefixo é idêntico entre chamadas?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - O estilo de saída é estático.
  - O terreno é "byte-stable… no timestamps" (`orient.rs:31`).
  - A linha por mensagem é fixa (`prompt_entry.rs:18-28`). A nota de clareza, que aparece uma vez, entra no fim, na mensagem do usuário.
- **Fonte externa (`prompt-caching`) [ALTA]:** "The match is exact, so a change anywhere in the prefix recomputes everything after it."
- **Observação [ALTA]:** a linha curta também é anexada aos avisos do próprio ambiente, como notificações de tarefa (`prompt_entry.rs:110-113`: só a gravação é pulada, a linha não). O custo é pequeno e fica no fim, então não quebra o cache.

### 2.2 O conteúdo variável fica no fim?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - Os valores voláteis ficam no `additionalContext` do início da sessão (`session_start_inject.rs:111-122`).
  - Os avisos por turno entram junto do resultado da ferramenta.

### 2.3 Modelo e esforço definidos antes?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - Os 4 agentes declaram `model: opus` e `effort: xhigh` (`packages/core/templates/agents/*/wave.md:5-6` e equivalentes).
  - O binário também fixa o modelo (`packages/core/src/domain/wave_prompt.rs:70-78`).
  - Nada troca o modelo da sessão principal.
- **Fonte externa:**
  - **Blog, confirmado:** "Set your model and effort level before you start. Changing either one mid-conversation can bust your prompt cache" [ALTA].
  - **Documentação `sub-agents`:**
    - `effort` é campo válido, com os valores `low`, `medium`, `high`, `xhigh` e `max`, "available levels depend on the model" [ALTA];
    - o apelido `opus` resolve para o modelo exato da sessão principal quando ela já é da família Opus [ALTA].
  - **Documentação `prompt-caching`:** no Opus 5.5 e no Fable 5.1, com chave de API ou assinatura, trocar o esforço **não** invalida o cache [ALTA].
- **Observação [ALTA]:**
  - Os comandos do plugin não declaram `model:`. Se declarassem, cada chamada seria uma troca de modelo e perderia o cache naquele turno, conforme a documentação.
  - Cada subagente tem cache próprio.

### 2.4 Sessões longas paradas e expiração do cache
- **Veredito:** NÃO ATENDE [ALTA quanto à ausência de tratamento; MÉDIA quanto ao tamanho do impacto]

**Fonte externa, tempo de vida do cache [ALTA]:**

| Tipo de requisição | Assinatura, dentro do plano | Chave de API, créditos ou nuvem |
|---|---|---|
| Conversa principal | 1 hora | 5 minutos |
| Subagentes, compactação e demais | 5 minutos | 5 minutos |

- Blog: "the cache expires after an hour on a subscription or five minutes on an API key… the next turn prefills the whole conversation again".
- Documentação `prompt-caching`, sobre subagentes: "they get five minutes even on a subscription until you choose a longer one".
- O TTL pode ser ajustado:
  - conversa principal: `promptCacheTtl` ou `CLAUDE_CODE_PROMPT_CACHE_TTL`;
  - subagentes: `subagentPromptCacheTtl`, ou o campo `experimental.cacheTtl` no frontmatter do agente (v2.1.248 ou posterior).
  - O TTL de 1 hora cobra mais caro pela gravação no cache.

**Evidência no Mustard:**
- **Não há tratamento de expiração de cache.**
- **Existem, sim, pausas longas:**
  1. **Esperas humanas:** a aprovação, as perguntas do levantamento (um ponto por mensagem), a aceitação de replanejamento e as perguntas de pendências no fechamento.
  2. **Cada onda.** A sessão principal fica parada enquanto o subagente trabalha. As ondas medidas tiveram de 36 a 403 idas e voltas, com média de 153 (`wave_prompt.rs:38-42`).
     - Com chave de API (5 min), toda onda que passa de 5 minutos faz a sessão principal reler o contexto inteiro sem cache quando a onda volta [MÉDIA: o mecanismo está documentado, mas a duração das ondas não foi medida].
  3. **Dentro da onda.** O Mustard força comandos `cargo` a rodar em primeiro plano, com limite de 10 minutos (`apps/rt/src/hooks/bash/waiting.rs:104-128`).
     - O cache do subagente dura 5 minutos por padrão, então um `cargo` de mais de 5 minutos o expira [MÉDIA].
     - Essa reescrita só vale para `cargo`, isto é, para projetos Rust.
- **Compactação:**
  - O Mustard recomenda `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE = "15"` (`apps/rt/src/hooks/session/conversation_size.rs:30-34`).
  - **Correção da revisão 2: a variável tem documentação pública.** Segundo a documentação `env-vars` [ALTA]: "Set the percentage (1-100) of the auto-compact window at which auto-compaction triggers… can't raise the threshold… Applies to both main conversations and subagents".
  - Ou seja, os 15% valem também para as ondas. O pedido de uma onda pode chegar a 25.000 tokens estimados (`wave_prompt.rs:193`). Conforme o tamanho da janela de compactação, a onda pode compactar cedo e perder detalhe do próprio pedido [MÉDIA; não medido].
- **Correção da revisão 2 sobre o custo da compactação:** a revisão 2 dizia que a próxima chamada paga tudo a preço cheio. Não é assim. Segundo a documentação `prompt-caching` [ALTA]:
  - o turno seguinte grava no cache só o resumo, que é curto;
  - o pedido de resumo lê o prefixo do cache se o cache ainda estiver válido.
  - O custo real da compactação frequente está em dois pontos: os pedidos de resumo repetidos e a perda de detalhe.
  - A documentação recomenda "run /compact at a natural break in your work, such as between tasks, instead of waiting for auto-compaction to trigger mid-task".
- **Correção:**
  1. Contar as compactações e as falhas de cache por spec. O gancho `PreCompact` já existe, e a status line recebe o objeto `prompt_cache`.
  2. Rever os 15%, sobretudo para as ondas.
  3. Quem usa chave de API deve avaliar `promptCacheTtl=1h` para a conversa principal e `experimental.cacheTtl: 1h` para as ondas. Decidir pelo número, porque a gravação custa mais.

---

## 3. ANALYZE / PLAN (levantamento `survey` e plano `plan`)

### 3.1 Tamanho da especificação por tarefa

#### 3.1a Quantas linhas o Mustard gera por tarefa? Existe limite?
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - **Não há teto para a spec inteira.**
  - **Há tetos por campo:**
    - título de tarefa: 70 caracteres (`packages/core/src/domain/spec_events/refusal.rs:175`);
    - entrega: 8.000 caracteres (`types.rs:274`; `packages/core/src/domain/spec_events/check.rs:324-325`);
    - mensagem: título de 60 e corpo de 4.000 caracteres (`packages/core/src/domain/spec_events/message.rs:12-14`);
    - pedido de onda: 25.000 tokens estimados (`wave_prompt.rs:193`).
  - **Há um piso:** um ponto por lacuna do tipo de trabalho, que pode ser marcado como não aplicável. São 9 lacunas em funcionalidade, 5 em correção e 9 em refatoração (`packages/core/src/domain/survey.rs:241-270`).
  - **Saída do `grill`** [MÉDIA, segunda varredura, com os tetos conferidos por amostra]:
    - até 5 lições, com o texto integral (`TOP = 5`, `packages/core/src/domain/search.rs:25`, conferido);
    - até 5 specs anteriores;
    - até 3 lembretes, cada um com a mensagem antiga inteira do usuário, sem corte.
  - **Repetição a cada gravação [ALTA]:**
    - enquanto os pontos são gravados, cada `write point` reimprime a lista inteira dos pontos ainda não gravados, junto com uma instrução de 416 caracteres (`apps/rt/src/commands/spec_events/write.rs:934-937`);
    - isso soma cerca de n(n−1)/2 pontos repetidos: 36 numa funcionalidade e 10 numa correção.
- **Fonte externa (Uvik) [MÉDIA]:**
  - "the spec phase used 35.1% of all tokens in the spec arms", "more than 40% for BMAD Method";
  - mediana geral de 128 linhas por ticket: BMAD 188, Spec Kit 132 e Kiro 106;
  - os 93 do OpenSpec **não foram encontrados**;
  - o OpenSpec teve a fase de spec mais curta (12 minutos, contra 28 do BMAD) e o maior número de tickets mesclados (42 de 50).
- **Correção de menor custo:**
  1. Ecoar só o próximo ponto, e não a lista inteira.
  2. Cortar lembretes e lições num teto de caracteres.
  3. Emitir um **aviso** no `plan` quando houver mais de 25 itens combinados ou mais de 8 critérios. Os números são arbitrários; ajustar pelo benchmark.

### 3.2 Rota rápida para tarefa pequena

#### 3.2a Critério da Uvik
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:** nenhum dos 6 sinais é usado para rotear.
- **Sinais que o Mustard já consegue calcular:**

| Sinal | Calculável? | De onde vem |
|---|---|---|
| (1) 3 ou mais arquivos | sim | `task.files` (`apps/rt/src/commands/flow/plan.rs:539-551`) |
| (2) 4 ou mais critérios | sim | número de eventos `criterion` |
| (3) cruza fronteira de módulo, serviço ou repositório | sim | subprojetos distintos no mapa |
| (4) muda schema ou API pública | parcial | há `contract`, mas não há item de mudança de dados |
| (5) mais de um agente ou pessoa | sim | mais de uma onda |
| (6) código regulado | não | precisaria de configuração |

- **Fonte externa (Uvik, o "Spec Fit Test") [MÉDIA]:** "Write a spec when 3 or more of these statements are true".
  - O item 3 é "crosses a module, service or repository boundary".
  - Em tickets de 1 ou 2 arquivos, "little advantage".
  - "did not pay for itself on bug-fix, refactor, or test-writing tickets".
  - Fora do critério: "give the agent the ticket text and the acceptance criteria, then review the result".
- **Fonte externa (Böckeler) [MÉDIA]:** uma boa ferramenta "would at the very least have to provide flexibility for a few different core workflows, for different sizes and types of changes".
- **Problema de ordem [MÉDIA]:** vários sinais só existem depois do plano. A rota curta, portanto, tem de ser decidida no plano, dispensando onda e revisões, e não no levantamento.

#### 3.2b O Mustard tem "modo leve" automático?
- **Veredito:** NÃO ATENDE. É o gargalo de custo mais provável [ALTA quanto ao fato; MÉDIA quanto ao tamanho].
- **Evidência:**
  - Todo pedido que muda arquivo abre spec (`packages/core/templates/mustard/en-US/session-map.md:7`).
  - O `write_gate` bloqueia a escrita sem spec aprovada (`apps/rt/src/hooks/write/write_gate.rs:14-17`).
  - O modo `--condensed` só junta os pontos (`apps/rt/src/commands/flow/grill.rs:19-20`).
  - A revisão final é obrigatória, "nem a de uma onda só" (`apps/rt/src/commands/flow/close.rs:12-13`).
  - **Há um terceiro subagente por spec, não dois [ALTA].** No fim do levantamento, a sessão é instruída a despachar o agente `mustard-review` (Opus, `xhigh`) para revisar o levantamento inteiro (`packages/core/src/platform/i18n/survey.rs:127-137`).
  - **Consequência:** uma correção de 1 linha usa no mínimo 3 subagentes Opus com esforço `xhigh`: a revisão externa do levantamento, a onda e a revisão final.
  - **O modo solo foi escrito e nunca ligado [ALTA]:**
    - o texto `round.next.solo` existe: "Do this round's wave yourself…";
    - nenhum código o usa;
    - `open_copies(…, false, …)` está fixo (`apps/rt/src/commands/flow/round/answer.rs:575`);
    - `is_solo_work`, citado num comentário, não existe.
  - **Existe uma saída manual, só que tudo ou nada:** "On a branch Mustard did not open, it blocks nothing" (`session-map.md:9`).
- **Contagem estimada para uma correção pequena** (1 onda, 1 tarefa, 1 critério; contagem derivada do código, não medida) [MÉDIA]:

| Tipo de chamada | Quantidade |
|---|---|
| Chamadas `mustard-rt run` via Bash | cerca de 35 a 45 |
| Chamadas `ArtifactData` | 5 a 7 |
| Publicações `Artifact` | 1 a 2 |
| Leituras de arquivo | 3 a 6 |
| Subagentes (Task) | 3 |
| Perguntas `AskUserQuestion` | pelo menos 4 |
| Turnos do usuário | pelo menos 5 |

  - Cada chamada é um turno do modelo que relê o contexto, ainda que a maior parte venha do cache.
- **Correção:**
  - **Menor custo:** religar o modo solo já escrito, em que a sessão principal faz a onda de 1 tarefa. Dispensar a revisão externa do levantamento quando os sinais do 3.2a derem menos de 3. Manter a prova executada pelo binário no fechamento.
  - **Passo seguinte:** dispensar também a revisão final nesse caso.
  - A decisão é sua, porque mexe na tese do projeto.

### 3.3 Verbosidade e repetição

#### 3.3a O PLAN duplica o ANALYZE? A spec repete código existente?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - Cada item é gravado uma vez.
  - O pedido da onda leva os códigos dos itens, não o texto (`wave_prompt.rs:10-12`).
  - Os fatos citam `arquivo:linha` em vez de copiar código (`packages/core/src/domain/spec_events/check.rs:370-381`), e a citação é verificada (`packages/core/src/domain/citation.rs:5-6`).
- **Fonte externa (Böckeler) [MÉDIA]:**
  - no spec-kit: "repetitive with each other and with existing code. Some contained code already";
  - no Kiro: "turned this small bug into 4 'user stories' with a total of 16 acceptance criteria".
- **Observação:** o desenho do Mustard evita a repetição entre spec e código. Mas a repetição **na saída dos comandos** existe: ver 3.1a e a observação 12.

### 3.4 Completude da especificação (8 itens)
- **Veredito:** PARCIAL; cobre 6 de 8 [ALTA]

| Item | Presente? | Evidência |
|---|---|---|
| Objetivo | sim | primeiro `context` (`grill.rs:5-6`; `types.rs:426`) |
| Escopo | sim, exceto em `fix` | `out_of_scope` (`types.rs:423`) |
| Entradas e saídas | sim | `contract` com exemplo (`types.rs:420`) |
| Mudanças de dados | **não** | não há item de schema ou migração |
| Casos de borda | sim | `edge_case` com `expected` (`types.rs:422`) |
| Testes de aceitação | sim, e forte | `criterion` com `proof` e forma EARS (`types.rs:291-292`) |
| Restrições | sim | `rule`, `limit`, `decision` (`types.rs:418-424`) |
| O que NÃO pode mudar | **só em `refactor`** | `MustNotChange` (`survey.rs:257`) |

- **Fonte externa (Uvik) [MÉDIA]:**
  - os 8 itens batem: "Goal, Scope, Inputs and outputs, Data changes, Edge cases, Acceptance tests, Constraints, No-change list";
  - "specs with 6 or more of the 8 checklist items merged 89.3% of tickets, against 60.0% otherwise".

#### 3.4a A "lista do que não pode mudar" existe?
- **Veredito:** NÃO ATENDE para `feature` e `fix` [ALTA]
- **Evidência:**
  - `fix` tem só `Symptom, Reproduction, ExpectedVsActual, Cause, DoneProof` (`survey.rs:254`), sem `OutOfScope`.
  - `feature` não tem `MustNotChange` (`survey.rs:243-253`).
- **Correção:**
  - acrescentar `MustNotChange` a `feature` e `fix`, e `OutOfScope` a `fix`;
  - acrescentar uma lacuna de mudança de dados em `feature`.
  - **Custo:** mínimo.

---

## 4. EXECUTE (rodada de ondas)

### 4.1 Falsa sensação de controle (texto principal)
- **Fonte externa (Böckeler) [MÉDIA]:**
  - "I frequently saw the agent ultimately not follow all the instructions";
  - "went way overboard because [it was] too eagerly following instructions".
- **Veredito:** PARCIAL [ALTA quanto aos itens conferidos]. O Mustard impõe por código parte do que exige, mas vários pontos centrais são só instrução.

**Imposto por código:**
- A entrega só é gravada com um envio aberto, e o texto tem teto.
- O arquivo entregue tem de existir.
- A prova tem de citar um critério existente.
- O build e as provas rodam antes do commit.
- Prova verde que rodou zero testes é recusada no fechamento.
- O `cargo` fica em primeiro plano.
- Escrita direta nos arquivos da spec é negada.
- O veredito final tem de responder por todo item combinado.
- O limite é de 2 rodadas de correção.

**Só instrução, sem verificação** [ALTA nos itens com citação; MÉDIA nos demais]:
- **Ficar na lista de arquivos.** A rodada comita tudo o que o `git status` da cópia mostra e só avisa (`report.rs:251-256`).
- **Despachar a onda a um subagente.** `run write delivered` e `run write verdict` aceitam qualquer chamador, e o binário carimba o autor sozinho (`report.rs:597-598` e `612-615`).
- Ler por trecho e não reler depois de editar. Só há corte automático para Rust.
- Rodar a suíte inteira só uma vez, no fim.
- Não fazer `git add`, commit, push ou troca de branch. Só o campo `commit` com cara de SHA é checado.
- Registrar a linha `USAGE`. A falta dela só gera aviso.

### 4.1a Marca existente vs. a criar?
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - `files: [{path, new?}]` (`plan.rs:539-551`).
  - Um arquivo sem `new` que não existe é recusado (`plan.rs:417-423`).

### 4.1b Verificação determinística de duplicação?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - Um arquivo marcado `new: true` que **já existe** passa sem aviso: o código só testa `!new` (`plan.rs:417-418`).
  - O `dependency_precheck` foi removido. As fixtures dele ficaram órfãs em `apps/rt/tests/fixtures/dependency_precheck/`, e sobraram referências em `packages/core/src/lib.rs:110` e `packages/core/src/domain/source_lang.rs:10`.
- **Fonte externa (Böckeler, spec-kit) [MÉDIA]:** o agente "ignored the notes that these were descriptions of existing classes… took them as a new specification… creating duplicates".
- **Correção:**
  1. Recusar `new && world.file_lines(path).is_some()` em `plan.rs:417`.
  2. Avisar quando um nome novo já existe no mapa.
  3. Apagar as fixtures órfãs.

### 4.2 Saída ruidosa de comandos

#### 4.2a Builds e testes rodam com flags silenciosas?
- **Veredito:** ATENDE para comandos do projeto; NÃO ATENDE para a saída do próprio Mustard [ALTA]
- **Evidência, comandos do projeto:**
  - O `rtk` é obrigatório no `init` (`apps/cli/src/commands/init/tools.rs:30-60`).
  - Ele vem ligado como gancho `PreToolUse` de Bash (`packages/core/src/platform/project_seed/settings.rs:394-410`).
  - Os agentes são orientados a usá-lo (`packages/core/templates/agents/en-US/wave.md:23`).
- **Fonte externa:**
  - **Documentação `costs` [ALTA]:** o exemplo oficial é justamente um gancho `PreToolUse` que reescreve o comando de teste para mostrar só as falhas. É o mesmo padrão do `rtk`.
  - **Blog [ALTA]:** "Add quiet flags to noisy commands". Saídas acima de 30.000 caracteres vão para arquivo (`BASH_MAX_OUTPUT_LENGTH`, com padrão de 30.000 e máximo de 150.000, segundo a documentação `env-vars`). "the real cost is output under the limit".
- **Evidência, saída do próprio Mustard (onde está o ruído):**
  1. **Todo `run` imprime JSON indentado**, com espaços e quebras de linha que custam tokens (`apps/rt/src/commands/flow/mod.rs:32-38`, `to_string_pretty`) [ALTA].
  2. **Nenhum comando tem teto geral de saída**, e `run read` não tem teto [MÉDIA].
  3. **O `round` imprime o pedido inteiro de cada onda** no campo `dispatch[].prompt` (`answer.rs:637`) [ALTA]. Podem ser até 4 ondas ao mesmo tempo, cada uma com até 25.000 tokens estimados.
     - O modelo lê esse texto como entrada e depois o **copia** na chamada do subagente, agora como tokens de saída, que custam cerca de 5 vezes mais que os de entrada.
     - O gancho que evitaria isso já existe: o bilhete `MUSTARD-WAVE: <spec> <n>` é trocado pelo pedido montado (`apps/rt/src/hooks/task/subagent_inject.rs:32`).
     - Mas nenhum texto distribuído ensina o bilhete. A instrução do `round` diz só "Dispatch this round's requests: each wave to the `mustard-wave` agent…" (`flow.rs:582-585`), e o bilhete só aparece no `MUSTARD-COMMANDS.md:62`, que não é instalado.
     - O mesmo vale para o pedido da revisão final, impresso pelo `close`.
  4. **Saída de falha cortada nos primeiros 100 caracteres** (`apps/rt/src/commands/review/qa_run/runner.rs:506-509` e `815-820`; `apps/rt/src/commands/review/qa_run/mod.rs:136-145`) [ALTA].
     - Vale para build quebrado e prova que falhou (`apps/rt/src/commands/flow/round/commit.rs:744-753`).
     - O começo da saída de um build costuma ser ruído ("Compiling…"), e o erro fica no fim.
     - Consequência provável: o modelo roda o build de novo só para ver o erro [MÉDIA].

#### 4.2b Existe gancho que resume logs longos?
- **Veredito:** ATENDE no mecanismo; PARCIAL na cobertura [ALTA]
- **Evidência:**
  - O `rtk` reescreve **antes** da execução, que é o padrão oficial, mas só cobre os comandos que conhece.
  - A reescrita própria do Mustard só cobre `cargo`.
  - Não há gancho `PostToolUse` que resuma saída (`apps/rt/src/registry.rs:117-156`).
- **Correção:**
  1. Nas falhas, guardar o fim da saída e as linhas de erro, não os 100 primeiros caracteres. **Custo mínimo.**
  2. Ensinar o bilhete `MUSTARD-WAVE` e deixar de imprimir o pedido inteiro. **Custo mínimo a baixo.**
  3. Imprimir JSON compacto. **Custo mínimo.**

### 4.3 Subagentes para trabalho verboso
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - Os 4 agentes têm `tools:` restrito:

| Agente | Ferramentas |
|---|---|
| `mustard-wave` e `wave-solo` | Read, Grep, Glob, Edit, Write, Bash |
| `mustard-review` | Read, Grep, Glob, Bash |
| `mustard-skill` | Read, Grep, Glob, Write |

  - A entrega volta com teto de 8.000 caracteres.
  - A leitura ampla é trocada por consulta ao mapa (`mustard-rt run map …`).
  - **Não usam opções documentadas que afetam custo** (documentação `sub-agents`) [ALTA]:
    - `maxTurns`: as ondas vão de 36 a 403 idas e voltas, sem teto;
    - `omitClaudeMd` (v2.1.271 ou posterior): o pedido da onda é completo, mas cada onda e cada revisão carregam o `CLAUDE.md` do projeto;
    - `experimental.cacheTtl`.
- **Fonte externa:**
  - **Documentação `costs` [ALTA]:** "Delegate verbose operations to subagents".
  - **Blog [ALTA]:** "for small jobs a subagent is just overhead". Isso pesa contra usar subagente em correção de 1 linha (ver 3.2b).
- **Observação [MÉDIA]:** `omitClaudeMd` economiza, mas tem custo de qualidade. Pela ETH, as instruções do projeto **são** seguidas e ajudam em práticas fora do padrão. Medir antes de ligar.

### 4.4 Referência direta a arquivos
- **Veredito:** ATENDE [ALTA]
- **Evidência:**
  - `task.files` e `must_read` são obrigatórios (`apps/rt/src/commands/spec_events/write.rs:574-590`).
  - Eles entram no pedido da onda (`wave_prompt.rs:1252`, `fn tasks`).
- **Fonte externa (blog, confirmado) [ALTA]:** o `@` "saves a Read call, or a search". Mencionar o mesmo arquivo de novo "generally attaches a second copy". O `@` é recurso da mensagem do usuário; para o subagente, o equivalente são os caminhos exatos.

### 4.5 Blocos com contexto limpo entre fases
- **Veredito:** PARCIAL. A revisão 2 dizia ATENDE, o que estava errado [ALTA]
- **Evidência:**
  - Cada onda roda com contexto limpo, e a retomada lê só o estado (`apps/rt/src/commands/flow/resume.rs:3-7`).
  - A sessão principal conduz levantamento, plano, rodadas e fechamento numa única conversa.
  - Só existe **uma** sugestão de `/clear`, logo depois da aprovação (`packages/core/src/platform/i18n/gates.rs:243-246`: "Suggest clearing the conversation with `/clear`").
  - Nada sugere limpar o contexto entre rodada e fechamento, nem entre specs.
  - Em sessão de nuvem, `/clear` não existe; a documentação manda abrir uma sessão nova.
- **Fonte externa:** blog, "/clear when you start something new, and /compact when the earlier part of the same task is done" [ALTA].
- **Correção:** sugerir `/clear` também depois do fechamento, e `/compact` depois de cada rodada. O início de sessão já reinjeta o mapa e a linha de retomada, então **nada se perde**.
  - **Custo:** mínimo.

---

## 5. REVIEW / QA

### 5.1 Desvio entre especificação e código

#### 5.1a O REVIEW compara o diff com cada critério?
- **Veredito:** PARCIAL; o lado determinístico é forte [ALTA]
- **Evidência:**
  - Todo item combinado precisa de uma resposta `met`; faltar alguma recusa o veredito (`report.rs:625-663`).
  - O binário executa todas as provas no fechamento (`close.rs:519-546`) e recusa prova com zero testes (`close.rs:113-114`).
  - O `met` e a pergunta "o teste verifica a regra?" são julgamento do modelo.
- **Fonte externa (Uvik) [MÉDIA]:**
  - o desvio é definido ("a second reviewer compares each merged patch with its spec… logs each mismatch (spec drift)");
  - **a faixa de 2,4% a 12,5% não foi encontrada.**

#### 5.1b Detecta comportamento não pedido?
- **Veredito:** NÃO ATENDE [ALTA]
- **Evidência:**
  - Arquivos fora do declarado geram só aviso, e tudo vai para o commit (`report.rs:251-256`).
  - O agente é autorizado a sair da lista (`wave.md:30`).
  - Teste sem critério e item sem onda geram só aviso (`close.rs:905-908`; `close.rs:401-411`).
- **Correção:** no fechamento, listar deterministicamente os arquivos do diff fora de todo `task.files` e exigir que o veredito final responda por cada um, com o mecanismo do `AgreedItemsMissing`.
  - **Custo:** baixo.

### 5.2 Revisor independente
- **Veredito:** ATENDE, com três ressalvas [ALTA]
- **Evidência:**
  - Subagente separado, trabalhando numa cópia própria (`packages/core/src/platform/i18n/prompt.rs:262-264`).
  - A entrada é montada pelo binário só com códigos (`prompt.rs:103`).
- **Fonte externa (Uvik) [MÉDIA]:** a próxima rodada vai testar "a verifier agent that checks the build against the spec before human review". O Mustard já tem esse verificador.
- **Ressalva 1 [MÉDIA]:** o revisor recebe o texto que o executor escreveu sobre a própria entrega, inclusive "o que decidiu fora do pedido" (`wave.md:38`). Isso pode ancorar o julgamento dele.
- **Ressalva 2, defeito [ALTA]:** o modelo do revisor, instalado em **todo** projeto, manda "install Mustard (`mustard init`) and run what the user would run" numa pasta temporária (`packages/core/templates/agents/en-US/review.md:20`). É instrução específica do repositório do Mustard e não faz sentido num projeto de cliente. Custa tempo, tokens e confunde o revisor.
- **Ressalva 3, trabalho repetido [ALTA]:**
  - o fechamento roda lint, a suíte inteira e cada prova **antes** de despachar o revisor (`close.rs:268-273`);
  - o revisor é instruído a rodar a suíte de novo e cada verificação de novo (`review.md:18` e `21`).
- **Correção:**
  1. Remover a linha do `mustard init` dos modelos distribuídos e ajustar o teste de paridade. **Custo mínimo.**
  2. O revisor lê as execuções gravadas (`criterion_run`) em vez de repetir tudo.
  3. Testar sem o texto do executor.

### 5.3 Métricas objetivas de qualidade
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - Por spec, existem vereditos, retrabalho, tempo por fase e o teto de 2 rodadas (`apps/rt/src/commands/flow/round/stops.rs:15`).
  - Faltam defeitos por tarefa aceita, a contagem de intervenções humanas e qualquer agregação entre specs.
  - `.claude/.metrics/` está declarado sem nenhum chamador (`packages/core/src/io/claude_paths.rs:279-283`).
- **Fonte externa (Uvik) [MÉDIA]:** "0.46 defects per merged ticket across spec workflows versus 0.86 for the no-spec control". O 0,46 é a média dos 4 fluxos com spec.
- **Correção:** `mustard-rt run metrics` com:
  - specs entregues;
  - rejeições por spec;
  - rodadas de correção;
  - mensagens do usuário depois da aprovação;
  - specs descartadas.

### 5.4 Divisão de modelos
- **Veredito:** NÃO ATENDE, por decisão explícita, e **contra a orientação oficial de custo** [ALTA]
- **Evidência:**
  - Todos os agentes são Opus com esforço `xhigh` (`wave_prompt.rs:70-78`).
  - Um teste proíbe Sonnet (`report.rs:2341-2346`).
- **Fonte externa, oficial:**
  - **Documentação `costs` [ALTA]:**
    - "Sonnet handles most coding tasks well and costs less than Opus. Reserve Opus for complex architectural decisions or multi-step reasoning."
    - "For simple subagent tasks, specify `model: haiku`."
    - Gasto alto inesperado "usually traces back to long sessions that were never cleared or to Opus left as the default model".
    - Os tokens de raciocínio são cobrados como saída, e a recomendação é baixar o esforço em tarefas simples.
  - **Blog [ALTA]:** "Give a repeated noisy job its own subagent definition with `model: haiku` (or sonnet)."
  - **Uvik [MÉDIA]:** rodou todos os fluxos com "claude-sonnet-5 in all five arms".
- **Observação:** não há estudo controlado mostrando que Opus com `xhigh` em toda onda compensa. O argumento de qualidade do Mustard é legítimo, mas não está medido.
- **Correção:** com o benchmark, testar primeiro `effort: high` nas ondas e Sonnet no `wave-solo` para correções.

### (extra) O que o QA faz de fato
- **Resposta [ALTA]:** não há fase QA. O `run close` (`close.rs:491-546`):
  1. roda lint e testes num ambiente limpo (`clean_env`, `close.rs:566`);
  2. roda cada prova;
  3. recusa o fechamento em qualquer falha.
- **Mais bloqueios:** onda sem commit, onda rejeitada, pendência aberta e pedido do usuário não entregue (`close.rs:727`).
- **Avaliação:** o QA é determinístico e sólido. O defeito está na saída de falha cortada em 100 caracteres (4.2a).

---

## 6. CLOSE (especificação viva)

### 6.1 A spec é descartada, mantida ou mesclada?
- **Veredito:** PARCIAL [ALTA]
- **Evidência:**
  - A spec fechada é mantida (`close.rs:9-10`), sem mesclagem.
  - O levantamento de uma spec nova busca, por BM25, as specs anteriores e as lições (`survey.rs:8-14`). Funciona como especificação viva parcial.
- **Fonte externa:**
  - **Uvik [MÉDIA]:** no OpenSpec, "The archive step merges the delta specs into the living spec folder".
  - **Böckeler [MÉDIA]** descreve três níveis:
    - spec-first: a spec é escrita antes e usada só na tarefa;
    - spec-anchored: a spec é mantida para evoluir junto com o código;
    - spec-as-source: a spec é a fonte principal.
  - O Mustard é spec-first, com ancoragem parcial pela busca.
- **Correção:** no fechamento, gravar um índice `arquivo → regras e contratos aprovados` a partir da tabela de rastreio (`close.rs:673-706`). O `plan` passa a avisar quando uma tarefa toca esses arquivos.
  - **Custo:** médio.

### 6.2 Risco de rigidez (MDD) e não determinismo
- **Veredito:** MITIGADO [MÉDIA]
- **Fonte externa (Böckeler) [MÉDIA]:** "spec-as-source might end up with the downsides of both MDD and LLMs: inflexibility and non-determinism".
- **Evidência:**
  - A spec do Mustard não é a fonte principal.
  - Os critérios são provas executáveis.
- **Risco residual:** a correção do 6.1 tem de ser **aviso**, nunca bloqueio.

---

## 7. Diferencial em sistemas legados

### 7.1 Funciona em código existente? Há números antes/depois?
- **Veredito:** PARCIAL [ALTA quanto ao mecanismo; não há dado de resultado]
- **Evidência:**
  - O scan determinístico foi feito para entrar em repositório existente (`apps/rt/src/commands/scan.rs:1-11`).
  - Os fatos citam `arquivo:linha` verificados.
  - Não há nenhuma medição antes/depois.
- **Fonte externa:**
  - **Böckeler [MÉDIA]:** "even more work to introduce them into an existing codebase"; os tutoriais são "usually based on creating an application from scratch".
  - **ETH [ALTA, v1]:** os arquivos gerados só ajudaram quando o repositório **não** tinha documentação. Isso sugere que o terreno e o mapa podem valer mais em código legado pouco documentado [BAIXA; é extrapolação].
- **Correção:** rodar o benchmark num repositório legado de cliente, com e sem o Mustard.

---

## Ressalva sobre o benchmark da Uvik (verificada)
- **A inconsistência de datas é real [MÉDIA]:**
  - a página diz "run in Q4 2026, on October 15, 2026", em duas buscas sem data na consulta;
  - ela foi indexada cerca de 15 horas antes de 25/09/2026 e se diz "last updated September 24, 2026" (evidência mais fraca, porque a data estava na consulta).
- **Conflito de interesse confirmado [MÉDIA]:** "Work with the engineers who ran this benchmark… staff augmentation…", "Senior rates are $50 to $99 per hour".
- **Contexto:**
  - 50 tickets reais em Python;
  - 5 fluxos, todos com Sonnet 5;
  - revisão cega;
  - tickets mesclados: controle 36, OpenSpec 42, BMAD 41, Spec Kit 40;
  - tempo mediano até a mesclagem: 29 minutos sem spec, 36 a 55 minutos com spec.
- **Conclusão:** trate como material de venda com dados. Use como direção, não como prova. Os vereditos dos itens 3.1, 3.2 e 5.1 medem se o Mustard **tem** o mecanismo. Se o mecanismo **compensa**, só o benchmark próprio dirá.

---

## Observações fora do checklist

1. **Documentação desatualizada:** o README, o `README.en.md` e o `CLAUDE.md` da raiz (ver o achado transversal) [ALTA].
2. **Código morto e sobras** [ALTA]:
   - `Scope`;
   - `TelemetrySummaryEntry`;
   - `base_gate`, desligado por decisão sua;
   - o modo solo (`round.next.solo`);
   - as fixtures `dependency_precheck`;
   - as referências em `lib.rs:110` e `source_lang.rs:10`;
   - `.claude/.metrics/`;
   - `templates-extras/skills/`;
   - `apps/cli/templates/.artifacts.json`;
   - `.gitignore:164`.
3. **`CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`:** tem documentação pública e vale para subagentes. A recomendação de 15% atinge as ondas (ver 2.4) [ALTA].
4. **Tokens autodeclarados:** o gasto da página da spec depende de o modelo copiar números corretamente (ver 0.1) [ALTA].
5. **Publicação e cópia de páginas** [MÉDIA, segunda varredura, com o tamanho do modelo conferido]:
   - O modelo da página da spec tem 97.017 bytes, mais o catálogo de rótulos embutido: cerca de 110 KB, perto de 30 mil tokens.
   - A ordem de publicação da página do projeto diz "read only once and published as it is" (`packages/core/src/platform/i18n/session.rs:77-83`), ou seja, o modelo lê o arquivo.
   - As duas páginas são republicadas a cada versão nova do Mustard.
   - A cada marco (plano, cada `round`, fechamento), há cópia por `ArtifactData`. O documento `computed/current` leva o texto integral dos pedidos de onda ainda não enviados (`apps/rt/src/commands/spec_events/pages/copy.rs:655-662`).
   - O modelo também precisa ler os arquivos de lote e o `next.md`.
   - Medir no item 0.1 antes de mexer.
6. **Limites de clareza** (25 palavras por frase, 15 linhas): só avisam, e o custo é baixo (`packages/core/src/domain/clarity/mod.rs:36,40`) [ALTA].
7. **Efeito colateral de permissão** [ALTA pelo código e pela documentação `hooks`]:
   - Quando um gancho do Mustard só avisa, injeta texto ou reescreve o comando, ele devolve `permissionDecision: "allow"` (`apps/rt/src/hook_output.rs:164-196`).
   - Pela documentação, `"allow"` "skips the permission prompt". As regras de negação continuam valendo.
   - Consequência: um comando `cargo` reescrito, ou uma edição com aviso de branch errada, é aprovado **sem perguntar ao usuário**.
   - A documentação trata `additionalContext` como campo próprio, independente da decisão de permissão.
   - **Correção:** não devolver `allow` em avisos e injeções. **Custo baixo.**
8. **Custo de processo dos ganchos** [MÉDIA]:
   - `PreToolUse` e `PostToolUse` usam o filtro `".*"`, então toda chamada de ferramenta dispara 2 processos, inclusive dentro das ondas.
   - Em instalações publicadas, o `mustard-boot` sempre toma o caminho lento (`sed` e `cat`), medido pelo próprio script em "12 hundredths of a second versus 1" (`plugin/bin/mustard-boot:46-69`).
   - Isso dá cerca de 0,24 s por chamada de ferramenta. Numa onda média de 153 idas e voltas, são uns 37 s de latência. Não é custo de tokens.
   - O `PostToolUse` só precisa do observador de onda e poderia ter filtro mais estreito.
9. **Texto duplicado** [MÉDIA]:
   - quando o gancho de fim de turno bloqueia, o mesmo texto vai em `reason` e em `additionalContext` (`hook_output.rs:107-118`), e os dois chegam ao modelo, segundo a documentação;
   - a linha curta também vai nos avisos do ambiente (ver 2.1).
10. **Defeito no revisor distribuído:** a instrução de `mustard init` (ver 5.2) [ALTA].
11. **Revisor repete provas já executadas** (ver 5.2) [ALTA].
12. **Pedido de onda impresso e depois copiado**, em vez de usar o bilhete `MUSTARD-WAVE` (ver 4.2a) [ALTA].
13. **Saída de falha em 100 caracteres** (ver 4.2a) [ALTA].

---

## Prioridade (custo × impacto) — recomendação

| # | Ação | Itens | Custo | Por quê |
|---|---|---|---|---|
| 1 | Ensinar o bilhete `MUSTARD-WAVE` e parar de imprimir o pedido inteiro | 4.2a, obs. 12 | mínimo a baixo | Tira até 25 mil tokens por onda da janela principal e evita copiá-los como saída |
| 2 | Falha: guardar o fim da saída e as linhas de erro, não os 100 primeiros caracteres | 4.2a, obs. 13 | mínimo | Evita repetir o build só para ver o erro |
| 3 | Remover `mustard init` do revisor distribuído | 5.2, obs. 10 | mínimo | Defeito real em todo projeto de cliente |
| 4 | `MustNotChange` em `feature`/`fix` e `OutOfScope` em `fix` | 3.4a | mínimo | É o item de spec mais barato e mais esquecido |
| 5 | Recusar `new: true` sobre arquivo existente | 4.1b | mínimo | Uma condição |
| 6 | Levantamento: ecoar só o próximo ponto; JSON compacto | 3.1a, 4.2a | baixo | Corta repetição na saída |
| 7 | Não devolver `allow` em avisos e injeções | obs. 7 | baixo | Devolve ao usuário o controle de permissão |
| 8 | Medição real (status line e transcripts) | 0.1, 0.3, 5.3 | médio | Sem ela, toda mudança de custo é aposta |
| 9 | Sugerir `/clear` depois do fechamento e `/compact` depois das rodadas | 4.5 | mínimo | O mecanismo de retomada já existe |
| 10 | README, `CLAUDE.md` e código morto | transversal, obs. 2 | baixo | Documentação errada engana pessoas e modelo |
| 11 | Arquivos fora do plano respondidos na revisão | 5.1b | baixo | Fecha o maior buraco de qualidade |
| 12 | Revisor lê as execuções gravadas em vez de repetir | 5.2, obs. 11 | baixo | Tira trabalho duplicado |
| 13 | Benchmark próprio | 0.2, 7.1 | médio | Decide os itens 14 e 15 |
| 14 | Rota rápida: religar o modo solo e dispensar revisões em mudança pequena | 3.2 | alto | Maior ganho provável; exige sua decisão |
| 15 | Medir e decidir: `AUTOCOMPACT=15` (inclusive nas ondas), TTL, modelo e esforço, texto do executor no revisor, `omitClaudeMd`, terreno | 2.4, 5.2, 5.4, 4.3, 1.3a | baixo, depois de 8 e 13 | Hipóteses que só o número decide |
| 16 | Índice arquivo → regras aprovadas | 6.1 | médio | Especificação viva sem rigidez |

---

## Fontes

| Fonte | Endereço | Status |
|---|---|---|
| Uvik Software, Spec-Driven Development Benchmark 2026 | https://uvik.net/spec-driven-development-benchmark/ | trechos de busca; acesso direto bloqueado |
| Gloaguen et al. (ETH Zurich), Evaluating AGENTS.md | https://arxiv.org/abs/2602.11988 | v1 lida inteira (cópia em PDF); v2 por trechos |
| Böckeler (Thoughtworks), Understanding Spec-Driven-Development | https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html | trechos de busca |
| Anthropic, Maximizing the value of your Claude Code sessions | https://claude.com/blog/maximizing-the-value-of-your-claude-code-sessions | lido |
| Claude Code Docs, Manage costs effectively | https://code.claude.com/docs/en/costs | lido |
| Claude Code Docs, How Claude Code uses prompt caching | https://code.claude.com/docs/en/prompt-caching | lido |
| Claude Code Docs, Subagents | https://code.claude.com/docs/en/sub-agents | lido |
| Claude Code Docs, Skills | https://code.claude.com/docs/en/skills | lido |
| Claude Code Docs, Environment variables | https://code.claude.com/docs/en/env-vars | lido |
| Claude Code Docs, Hooks | https://code.claude.com/docs/en/hooks | lido |
| Issue #37793 | https://github.com/anthropics/claude-code/issues/37793 | lida |
| StationX, Reduce Claude Code token usage | https://app.stationx.net/articles/reduce-claude-code-token-usage | não verificada (bloqueada) |
| Boringbot, How to save millions in Claude tokens | https://boringbot.substack.com/p/how-to-save-millions-in-claude-tokens | não verificada (bloqueada) |
| JuanjoFuchs, claude-code-tips | https://github.com/JuanjoFuchs/claude-code-tips | lido |
