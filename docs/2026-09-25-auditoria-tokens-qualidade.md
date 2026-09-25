# Auditoria do Mustard: consumo de tokens e qualidade de código

## Contexto
Você pediu para cruzar o checklist de auditoria (tokens + qualidade, pipeline ANALYZE→PLAN→EXECUTE→REVIEW→QA→CLOSE) com o código real do Mustard. Resultado abaixo: um veredito por item — ATENDE, PARCIAL ou NÃO ATENDE — com evidência `arquivo:linha` e a correção de menor custo. As evidências foram levantadas por leitura do código na branch `dev` (commit `7464541`), e os pontos centrais foram conferidos à mão.

**Achado transversal [confiança ALTA]:** o README descreve um desenho que não existe mais. Não há mais roteador injetado em todo prompt, não há mais `spec.md`/`wave-plan.md` e não há mais `dependency_precheck`. A spec atual é um log de eventos (`.claude/spec/<nome>/spec.ndjson`), escrito só via `mustard-rt run write`. O próprio pipeline também não tem mais as 6 fases do checklist: as fases reais são `survey, plan, approved, running, closed, pr_open, delivered, discarded` (`packages/core/src/domain/spec_events/types.rs:277-278`). Não existe fase QA separada; o QA roda dentro do `run close`.

---

## 0. Medição (linha de base)

| Item | Veredito | Evidência | Correção de menor custo |
|---|---|---|---|
| Tokens entrada/saída/cache por fase | **NÃO ATENDE** | Só existe um total **autodeclarado** por onda (`tokens`, `caller_tokens` no evento `send`, `types.rs:555-560`; parser em `apps/rt/src/commands/flow/round/report.rs:752-768`), sem divisão entrada/saída/cache. Por fase só existe tempo (`page.rs:885`). `cache_read_input_tokens` aparece só em testes. O struct `TelemetrySummaryEntry` (`packages/core/src/view/summary/mod.rs:191-203`) é código morto. | Ler o `transcript_path` (já recebido pelos hooks) no hook `Stop`/`SessionEnd`, somar `usage` (input, output, cache_read, cache_creation) e gravar um evento com o valor do campo `phase` do `state` corrente. É dado **medido**, e não autodeclarado pelo modelo como hoje. |
| Conjunto fixo de tarefas de teste | **NÃO ATENDE** | Nenhum benchmark. `apps/scan/tests/retrieval_self_recall.rs:15` recusa benchmark curado, mas só para o scan. | Pasta `bench/` com 10 pedidos reais (3 correções, 3 refatorações, 4 funcionalidades) sobre um repositório congelado, rodados via `claude -p` com o Mustard ligado e desligado. |
| Custo por tarefa **aceita** | **NÃO ATENDE** | Os contadores de veredito e retrabalho existem por spec (`page.rs:873-912`), mas não são agregados entre specs nem divididos pelo custo. | Depois do item acima: `soma(tokens) ÷ specs em fase delivered`. |
| `/usage` e `/context` com e sem Mustard | **NÃO FEITO** (manual) | — | Rodar uma vez. O custo fixo estimado está na seção 1. |

## 1. Sobrecarga fixa

| Item | Veredito | Evidência | Correção |
|---|---|---|---|
| Schemas de ferramentas MCP | **ATENDE** (não se aplica) | O Mustard não expõe servidor MCP: "The harness declares no MCP server" (`apps/cli/src/commands/init/mod.rs:385-388`). Sobras: `.gitignore:164` (`plugin/bin/mustard-mcp`) e a menção a um MCP planejado em `IMPLEMENTACAO-MUSTARD-2.md:95`. | Remover as sobras. Se o MCP planejado vier, expor no máximo 2 ferramentas com descrição curta. |
| Liga/desliga MCP no meio da sessão | **ATENDE** | Nada faz isso. | — |
| CLAUDE.md / AGENTS.md com visão geral | **PARCIAL** | Nunca escreve CLAUDE.md (`apps/rt/src/commands/scan.rs:48`). Mas injeta no SessionStart um "terreno" (uma linha por subprojeto, até 16 linhas, `apps/rt/src/commands/orient.rs:192-223`), que é justamente a visão geral que o estudo da ETH Zurich diz não ajudar. O `session-map.md` (37 linhas, cerca de 2 kB) tem regras de fluxo, não arquitetura, e isso está correto. | Tirar o terreno da injeção padrão e deixá-lo sob demanda (`mustard-rt run orient`). Custo: uma linha em `session_start_inject.rs:111-122`. **Precisa de medição antes**: o terreno é pequeno (cerca de 16 linhas) e o ganho é incerto. |
| CLAUDE.md abaixo de ~200 linhas | **ATENDE** | Total no SessionStart ≤ 3.000 bytes (`session_start_inject.rs:72`). Estilo de saída: 57 linhas. | — |

**Custo fixo estimado por sessão [MÉDIA]** (bytes ÷ 4 ≈ tokens):
- SessionStart: ≤ 3 kB ≈ 750 tokens.
- Estilo de saída: 3,4 kB ≈ 850 tokens.
- Linha por prompt: ≤ 100 caracteres (`prompt_entry.rs:18-28`).

Isso é baixo. Não é aqui que o dinheiro vai embora.

## 2. Cache de prompt

| Item | Veredito | Evidência | Correção |
|---|---|---|---|
| Prefixo idêntico entre chamadas | **ATENDE** | O estilo de saída é estático. O terreno é "byte-stable… no timestamps" (`orient.rs:31`). A linha por prompt é fixa. | — |
| Variável no fim | **ATENDE** | Os valores voláteis (retomada, pendências, disco, branches) ficam no `additionalContext` do SessionStart, uma vez por sessão (`session_start_inject.rs:111-122`). Injeções por turno são condicionais (avisos do `write_gate` e `command_guard`). | — |
| Modelo/esforço fixos antes | **ATENDE** | Os agentes fixam `model: opus` / `effort: xhigh` no frontmatter (`packages/core/templates/agents/*/wave.md:5-6`). Nada troca o modelo da sessão principal. | — |
| Pausas longas / expiração do cache | **NÃO ATENDE** (risco real) | Nada trata disso. Pior: o Mustard **recomenda** `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=15` (`apps/rt/src/hooks/session/conversation_size.rs:30-34`), ou seja, compactar quando o contexto chega a 15% da janela. Cada compactação reescreve o histórico, quebra o cache e dispara nova injeção de SessionStart + bloco de retomada. | **Medir** o número de compactações por spec antes de mexer. Hipótese [MÉDIA]: subir para 50–60% reduz as recargas a preço cheio. O risco é contexto maior por turno, e só a medição decide. |

## 3. ANALYZE / PLAN (survey + plan)

| Item | Veredito | Evidência | Correção |
|---|---|---|---|
| Tamanho da spec / limite | **PARCIAL** | Há tetos por campo: título de tarefa 70 (`refusal.rs:175`), entrega 8.000 caracteres (`types.rs:274`), pedido de onda 25.000 tokens (`wave_prompt.rs:193`). **Não há teto para a spec inteira** nem para o número de itens combinados. | Aviso (não recusa) no `plan` quando os itens combinados passarem de N (ex.: 25) ou os critérios passarem de 8. Serve como sinal do "bug que virou 16 critérios". |
| Rota rápida para tarefa pequena | **NÃO ATENDE** — **principal gargalo provável** [ALTA quanto ao fato; MÉDIA quanto ao impacto] | Todo pedido que muda arquivo abre spec (`session-map.md:7`). O `write_gate` bloqueia edição sem spec aprovada (`apps/rt/src/hooks/write/write_gate.rs:14-17`). O modo `--condensed` só junta as perguntas do survey; plano, aprovação, rodada com subagente Opus/xhigh, revisão final Opus e close continuam obrigatórios (`apps/rt/src/commands/flow/grill.rs:19-20`). O `Scope::Light` que o README cita é código morto (`packages/core/src/domain/model/pipeline.rs:51-57`). Correção de 1 linha = mínimo 2 subagentes Opus (onda + revisão). | Criar a rota "direta": spec mínima (1 critério com `proof`), **sem subagente de onda** (a sessão principal edita) e sem revisão final quando o critério de rota da Uvik der menos de 3 sinais. Os sinais já calculáveis deterministicamente são: número de arquivos das tarefas, número de critérios, subprojetos distintos (grain model) e se toca contrato. Mantém o `proof` rodando no close, que é a parte barata e determinística. |
| PLAN duplica ANALYZE / repete código | **ATENDE** | Os itens são gravados uma vez; o pedido da onda leva só os códigos dos itens (`wave_prompt.rs:10-12`). Os fatos citam arquivo:linha verificados (`packages/core/src/domain/citation.rs:5-6`) em vez de copiar código. | — |
| Completude (8 itens) | **PARCIAL** — 6 de 8 | Estão cobertos: objetivo, escopo (`out_of_scope`), entradas/saídas (`contract`), casos de borda, critérios de aceitação com `proof` e restrições (`rule`/`limit`/`decision`). **Faltam:** (a) mudança de dados/schema, que não tem slot próprio; (b) "o que NÃO pode mudar", que só existe em `refactor` (`packages/core/src/domain/survey.rs:255-264`). Pior: o `fix` não tem nem `OutOfScope` (`survey.rs:253`). | Acrescentar `MustNotChange` às listas `feature` e `fix`, e `OutOfScope` a `fix`, em `survey.rs:243-253`. São duas linhas: é a correção mais barata de toda a auditoria. |

## 4. EXECUTE (rodada de ondas)

| Item | Veredito | Evidência | Correção |
|---|---|---|---|
| Marca existente vs. novo | **ATENDE** | `files: [{path, new?}]` (`apps/rt/src/commands/flow/plan.rs:539-551`). Arquivo sem `new` que não existe = recusa (`plan.rs:417-423`). | — |
| Checagem determinística de duplicação | **NÃO ATENDE** | O inverso não é checado: `new: true` sobre arquivo que **já existe** passa. `dependency_precheck` foi removido; as fixtures em `apps/rt/tests/fixtures/dependency_precheck/` estão órfãs, e sobram referências velhas em `packages/core/src/lib.rs:110` e `domain/source_lang.rs:10`. | Em `plan.rs:417`, recusar `new && world.file_lines(path).is_some()`. É uma condição. Segundo passo, mais caro: avisar quando um símbolo declarado como novo já existe no grain model. Apagar as fixtures órfãs. |
| Saída ruidosa de comandos | **ATENDE** | O `rtk` está ligado por padrão como hook PreToolUse de Bash (`packages/core/src/platform/project_seed/settings.rs:394-410`; `domain/config.rs:575-576`). `cargo` é reescrito para `rtk cargo` (`apps/rt/src/hooks/bash/waiting.rs:112-128`). A saída do QA é truncada (`qa_run/runner.rs:506`). | Limite [MÉDIA]: o `rtk` só filtra os comandos que conhece. Um PostToolUse genérico que corte saída de Bash acima de N linhas cobriria o resto. Baixa prioridade. |
| Subagentes para trabalho verboso, ferramentas limitadas | **ATENDE** | 4 agentes, todos com `tools:` restrito; revisão só leitura + Bash (`packages/core/templates/agents/en-US/review.md`). | — |
| Caminhos exatos do PLAN ao EXECUTE | **ATENDE** | `task.files`/`must_read` são obrigatórios (`apps/rt/src/commands/spec_events/write.rs:574-590`) e entram no pedido da onda (`wave_prompt.rs:1235-1245`). | — |
| Resumo compacto entre fases | **ATENDE** | Retomada lê só o estado (`apps/rt/src/commands/flow/resume.rs:3-7`). A entrega da onda tem teto de 8.000 caracteres. Não há histórico repassado. | — |

## 5. REVIEW / QA

| Item | Veredito | Evidência | Correção |
|---|---|---|---|
| Diff vs. cada critério, item a item | **PARCIAL** | Cada item combinado precisa de resposta `met`; faltar item = veredito recusado (`apps/rt/src/commands/flow/round/report.rs:625-663`). Todo `proof` é **executado** pelo binário no close, e prova verde com 0 testes é recusada (`close.rs:113-114, 519-546`). Isso é forte. Porém "a revisão final… não confere critério nenhum" (`types.rs:634-636`), e o `met` é julgamento do modelo. | Aceitável. O determinístico já está onde deveria estar (executar a prova). |
| Detecta comportamento não pedido | **NÃO ATENDE** | Arquivos entregues fora do plano geram só aviso (`report.rs:251-283`, `files-diverged`). O próprio agente é autorizado a sair da lista (`wave.md:30`). Teste sem critério = aviso (`close.rs:905-908`). | Custo baixo: no close, listar deterministicamente os arquivos do diff que não estão em nenhum `task.files` e exigir que a revisão final responda por cada um (mesmo mecanismo do `AgreedItemsMissing`). Sem bloquear, só obrigando a justificar. |
| Revisor com contexto limpo | **ATENDE, com ressalva** | Subagente separado, cópia própria, entrada montada pelo binário só com códigos (`packages/core/src/domain/wave_prompt.rs:1123-1141`). **Ressalva:** recebe o texto autodeclarado do executor, incluindo "o que decidiu fora do pedido" (`wave.md:38`). Isso abre caminho para ancoragem [MÉDIA]. | Tirar o `own_delivered` do pedido da revisão e deixar só diff + itens. Testar antes/depois com o benchmark (item 0). |
| Métricas objetivas de qualidade | **PARCIAL** | Retrabalho e vereditos por spec; teto de 2 rodadas de correção (`apps/rt/src/commands/flow/round/stops.rs:15`). **Faltam** a agregação entre specs e a contagem de intervenções humanas (mensagens `author:"user"` existem mas não são contadas). `.claude/.metrics/` está declarado sem nenhum chamador (`packages/core/src/io/claude_paths.rs:279-283`). | Um comando `mustard-rt run metrics` que varre todos os `spec.ndjson` e emite: specs entregues, rejeições por spec entregue, rodadas de correção e mensagens do usuário após a aprovação. Tudo já está nos eventos. |
| Divisão de modelos | **NÃO ATENDE** (por decisão) | Tudo Opus/xhigh, fixo no binário (`wave_prompt.rs:70-78`); um teste proíbe "Sonnet" (`report.rs:2341-2346`). | **Não mudar sem benchmark.** É hipótese [MÉDIA], sem resultado publicado. Dado que a onda é o maior consumidor, `effort: high` na onda seria o primeiro teste barato. |

## 6. CLOSE

| Item | Veredito | Evidência | Correção |
|---|---|---|---|
| Spec viva por módulo | **NÃO ATENDE** | A spec fechada fica como está: eventos + cópia (`close.rs:9-10`). Nada é mesclado. O grain model é atualizado, mas é estrutura, não comportamento. | Adiar. O custo é alto, e o risco de rigidez apontado por Böckeler é real. Alternativa barata: no close, gravar os `rule`/`contract` aprovados num índice por arquivo que o `plan` de specs futuras consulta. |
| Risco de rigidez (estilo MDD) | **MITIGADO** | A spec não é fonte principal: o código continua sendo, e os critérios são provas executáveis. | — |

## 7. Código legado

| Item | Veredito | Evidência | Correção |
|---|---|---|---|
| Diferencial em código existente | **PARCIAL** | O scan determinístico para `grain.model.json` (`apps/rt/src/commands/scan.rs:1-11`) e os fatos com citação verificada são próprios para código existente. **Sem nenhum número antes/depois.** | Usar o benchmark do item 0 num repositório legado de cliente. |

---

## Prioridade (custo × impacto) — minha recomendação

1. **Medição real de tokens por fase via transcript** (seção 0). Sem isso, todas as outras mudanças de custo são aposta. Custo: médio (um hook + um evento).
2. **`MustNotChange` em feature/fix e `OutOfScope` em fix** (`survey.rs`). Custo: mínimo.
3. **Recusar `new: true` sobre arquivo existente** (`plan.rs:417`). Custo: mínimo.
4. **Rota direta para tarefa pequena** (seção 3). É o maior ganho provável de tokens [MÉDIA], porque hoje uma correção de 1 linha dispara 2 subagentes Opus/xhigh. Custo: alto, e mexe na tese do projeto. Precisa da sua decisão.
5. **Arquivos fora do plano respondidos na revisão** (seção 5). Custo: baixo.
6. **Corrigir o README** (roteador, `spec.md`, `Scope::Light`) e remover o código morto (`Scope`, `TelemetrySummaryEntry`, fixtures `dependency_precheck`, `.claude/.metrics`). Custo: baixo.
7. Medir o efeito de `AUTOCOMPACT=15` e do `own_delivered` no revisor antes de mudar.

