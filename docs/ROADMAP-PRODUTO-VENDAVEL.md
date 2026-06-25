# Mustard — Roadmap para "extremamente bom, vendável, usável"

> Plano de ação prospectivo (engenheiro sênior, brutalmente honesto). Aprovar este plano ≠ executar.
> Ancorado em medição própria (sessão de recall 0→10/10) + pesquisa de mercado 2026 (Spec Kit, Kiro, OpenSpec, Serena, EARS, agentic-grep-vs-vectors).

## ESTADO DA IMPLEMENTAÇÃO (handoff — leia primeiro)

- **Fundação pronta e validada** (dev_rubens, NÃO pushed): juiz digest-validate (`centralFound`), léxico+auto-heal, `enrich-purpose` (render→apply, incremental), `purpose-search` (recall e2e sialia **10/10**), trigger de enrich padrão no `/scan`. Commits `b9fa5e54`, `b8e7ccae`, `64d152c8`.
- **Fase 0 INICIADA.** Contrato do benchmark escrito em `benchmarks/README.md`.
- **PRÓXIMO PASSO EXATO:** implementar `mustard-rt run recall-bench` (contrato no README: roda name-match + purpose-search por caso, emite recall@k byte-estável; reusa `Scan::digest_query` + `Scan::purpose_search`; teste com fixture sintético). **Depois validar na sialia** com os 10 casos rotulados (listados na memória [[project-mustard-recall-llm-body-summary]]) — aplicar purposes via backup/restore do model, rodar o bench, confirmar name 0/10 → purpose 10/10 PELO RUNNER. **Depois ampliar 10→30 casos.**
- **Decisões travadas:** corpus = **sialia primeiro** (labels NÃO vão pro repo — vazam estrutura privada; só o runner + fixture sintético são versionados); branch = **dev_rubens**; ritmo = critério > rapidez, consultar usuário/web a cada dúvida.
- Sequência geral: Fase 0 (este ponto) → 1 (verificador adversarial) → 2 (EARS+coverage+constitution+rules) → 3 (drift). Ver tabela "Sequenciamento".

---

## Tese estratégica (leia antes de qualquer ponto)

Duas verdades de mercado decidem a alocação de esforço:

1. **A cerimônia de SDD virou commodity.** GitHub Spec Kit (88k★, 28+ agentes), AWS Kiro, OpenSpec (52k★) convergiram no mesmo `spec→plan→tasks→implement`. Competir aí é briga perdida — não vamos ganhar de 88k★.
2. **O retrieval por VETOR morreu para código, e o retrieval por INTENÇÃO está em aberto.** A Anthropic removeu vector search do Claude Code (mai/2025); Cursor/Devin/Windsurf usam grep + just-in-time. O melhor incumbente de retrieval-MCP, **Serena**, é LSP/símbolo (`go-to-def`, `find-refs`) — mas LSP **exige que você já saiba o nome do símbolo**. Nenhum deles acha código a partir do que o usuário **quer dizer** em palavras de domínio.

**O fosso do Mustard é UM e só um: retrieval por intenção/vocabulário** — o digest determinista + léxico + `purpose` que acha `EffectivateAsync` a partir de "efetivar previsão", cross-lingual, sem vector DB. Medido: name-match 0/10 → purpose-search 10/10. **Tudo neste roadmap serve a estreitar o produto nesse fosso e tornar a cerimônia o mais leve possível ao redor dele.**

Regra de priorização: **se um item não fortalece o fosso (retrieval) nem a confiança no output (verificação), ele é Tier 2/3.**

---

## Sequenciamento honesto (não dá pra fazer os 6 de uma vez)

| Fase | Item | Por quê primeiro | Esforço |
|------|------|------------------|---------|
| **0** | (1) Retrieval-MCP "intent→code" + cobertura do purpose | É o produto vendável. Sem wedge vendido, o resto é polir tool que ninguém comprou. | Alto (semanas) |
| **1** | (2) Verificador adversarial 1ª-classe | Transforma "harness de processo" em "output confiável". Esta sessão provou: implementador cravou "all green" 2×, só o e2e real pegou. | Médio |
| **2** | (3) EARS + (4) coverage-check + (5) constitution | Gates determinísticos baratos que sobem a qualidade da spec sem engordar. Bundle. | Baixo cada |
| **3** | (6) Drift detector spec↔código | Nice-to-have; depende de âncoras já existirem. | Baixo |
| **∞** | **Emagrecer a cerimônia** | Anti-investimento contínuo (ver §"O que parar"). | — |

---

## Continuidade — o que construímos nesta sessão é a FUNDAÇÃO (nada descartado)

O arco desta sessão foi **cumulativo**, e o roadmap CONSTRÓI sobre ele — não joga fora:

- **Juiz digest-validate (`centralFound`)** = o **gatilho** do retrieval por intenção. No `centralFound=false` ele chama o `purpose-search`. Load-bearing, reaproveitado inteiro.
- **Léxico (híbrido + auto-heal)** = o **cache determinista**; ponte confirmada desce pro tier lexical (mais barato que LLM toda vez).
- **`purpose-search`** = recupera o **net-new** que o nome perde.
- **Contrato do juiz (fix de polissemia `import`)** = o mesmo juiz que dispara tudo.

**Divisão de trabalho honesta** (pra NÃO manter dois caminhos meio-sobrepostos às cegas):
- **Léxico/auto-heal** → ponte **EXATA RECORRENTE** (`efetivar→effectivate`: uma vez confirmada, vira tier lexical determinista, custo ~0).
- **`purpose-search`** → **net-new / primeira vez / divergência semântica** que nenhuma ponte cobre ainda.
- São **complementares**: o purpose-search ACHA; o léxico CACHEIA o que recorre. (Sem essa clareza, o risco é manter duas recuperações redundantes — defina qual fira quando.)

**O que NÃO volta como código:** `count×idf` e os 3 atalhos REFUTADOS — corretamente removidos. Mas as **medições viram guardrail** (o "nunca vector DB" do roadmap vem direto delas). Nada do esforço se perdeu: virou **código-fundação** OU **evidência que blinda decisão futura**.

---

## Ponto 1 — Retrieval por intenção como produto (MCP) + cobertura do purpose `[Tier 1 / Fase 0]`

**Objetivo.** Um servidor MCP `mustard-intel` que qualquer agente (Claude Code, Cursor, Cline, Spec Kit) pluga, expondo UMA capacidade que Serena/grep não têm: **achar código por intenção em linguagem de domínio**, cross-lingual, com ponteiros (file:symbol) — não conteúdo (just-in-time).

**SOLID.**
- **SRP:** a lógica de retrieval já vive em `packages/core` + `apps/scan` (digest, ladder, purpose). O MCP é **adaptador fino** — não reimplementa nada.
- **DIP:** o MCP depende de uma trait de retrieval (`IntentRetriever`), não dos detalhes do scan. Hoje há `apps/mcp`; estender, não inchar.
- **ISP:** ferramentas pequenas e separadas — `find_by_intent(query) → [{file, symbol, why}]`, `purpose_of(file#symbol) → string`, `lexicon_bridges(term)`. Cada uma uma responsabilidade.

**Fases.**
1. **Cobertura do purpose-search** (fechar o resíduo medido). Os ~3/10 que só casam por sinônimo (`assinatura↔subscrição`) precisam de ponte de léxico OU prompt de sumário que inclua o termo canônico. Ação: no enrich, instruir o sumário a citar o **verbo de domínio canônico** + alimentar pontes sinônimo→sinônimo no léxico a partir dos `matchedTerms` confirmados. Meta: ≥9/10 robusto num conjunto rotulado ampliado (30 casos, 3 projetos).
2. **MCP `find_by_intent`** — wrap do digest+purpose-search; retorna ponteiros + 1 linha de "porquê" (o conceito que casou). Just-in-time (sem conteúdo).
3. **Posicionamento honesto vs Serena** — README e landing dizem o que É e o que NÃO é: *"Serena = navegação por símbolo (você sabe o nome). Mustard = ponte intenção→código (você sabe só o que quer, e talvez em outra língua)."* Complementa, não substitui. Pluga sob Spec Kit/Cursor.

**AC (mensuráveis).**
- AC-1 — `find_by_intent` num conjunto rotulado de 30 recall-holes cross-lingual: recall@5 ≥ 90%.
- AC-2 — latência query-time determinista (sem LLM/embedding na busca); P95 < 300ms num repo de ~5k métodos.
- AC-3 — instalável como MCP em Claude Code E Cursor com 1 comando; smoke verde nos dois.

**Honestidade.** Serena é forte e estabelecido. Se tentarmos ser "code search geral", perdemos. **A única vitória é o wedge estreito intenção/vocabulário.** Vender isso exige um conjunto rotulado público que PROVE o 0/10→10/10 num repo aberto (não só sialia privada) — sem prova reproduzível, é só claim.

---

## Ponto 2 — Verificador adversarial como papel de 1ª classe `[Tier 1 / Fase 1]`

**Objetivo.** O implementador **nunca** certifica o próprio trabalho. Um papel `verify` separado tenta **REFUTAR** a entrega com teste em **corpus/dado real**, não rodar o AC numa fixture de brinquedo. (Mercado: *"o padrão mais subutilizado de SDD"*. Esta sessão: subagente cravou "all green" 2×, o e2e real refutou as duas.)

**SOLID.**
- **SRP:** `execute` produz; `verify` refuta. Papéis distintos, prompts distintos. Já existe `mustard-review` (agent read-only) — promover a gate obrigatório.
- **Open-Closed:** adicionar o gate sem tocar no `execute` (o close-gate ganha uma pré-condição, não reescreve a fase).
- **Liskov:** qualquer verificador (Haiku/Sonnet/humano) satisfaz o mesmo contrato `Verdict{refuted: bool, evidence}`.

**Fases.**
1. **Invariante no close-gate:** `qa.result` precisa ter sido autorado por **agente ≠** o que emitiu `execute` (carimbar `author_role`/`agent_id` nos eventos; o gate compara). Hoje o close-gate só checa `overall=pass`.
2. **QA adversarial:** prompt do verificador vira "tente PROVAR que está errado; default = refutado se não conseguir um teste em dado real que passe". Preferir **e2e/corpus real** a unit test sintético (a lição dos 4 bugs).
3. **Painel quando barato:** para mudanças de retrieval/risco, N verificadores independentes com lentes distintas (correção, recall em corpus real, determinismo); maioria refuta → bloqueia.

**AC.**
- AC-1 — close-gate bloqueia CLOSE se `qa.author == execute.author`.
- AC-2 — num caso plantado (bug que passa unit mas falha e2e), o gate adversarial **bloqueia**; o gate antigo (só AC) **liberava** → diferença demonstrada.
- AC-3 — `Verdict` é contrato tipado, parse tolerante (nunca panica).

**Honestidade.** Isso adiciona custo (mais um agente por wave). Mas é o ÚNICO item que ataca a falha que mais dói: output plausível-mas-errado. Sem ele, "vendável" é mentira — ninguém compra um harness que crava verde no que está quebrado.

---

## Ponto 3 — Notação EARS nos Critérios de Aceitação `[Tier 2 / Fase 2]`

**Objetivo.** Cada AC vira uma **afirmação testável única** num dos 5 padrões EARS, em vez de prosa-livre+comando. Reduz ambiguidade e deixa o AC gerar código E teste sem adivinhar. (Spec Kit ainda NÃO tem — diferenciação barata.)

**Os 5 padrões (prescrição exata):**
- Ubíquo: `The <sistema> SHALL <resposta>`
- Evento: `WHEN <gatilho>, the <sistema> SHALL <resposta>`
- Estado: `WHILE <pré-condição>, the <sistema> SHALL <resposta>`
- Indesejado: `IF <gatilho>, THEN the <sistema> SHALL <resposta>`
- Opcional: `WHERE <feature>, the <sistema> SHALL <resposta>`

**SOLID.**
- **SRP:** um validador determinístico `ac-lint` (em `apps/rt`) com uma só função: cada AC casa um padrão EARS **e** tem um comando runnable. Não decide nada; só valida forma.
- **Open-Closed:** os 5 padrões são dados (regex/gramática), não lógica — adicionar dialeto é dado.

**Fases.**
1. Templates EARS na composição de AC (o spec-draft sugere o padrão).
2. `mustard-rt run ac-lint` — determinístico, lista AC fora de forma; vira pré-condição do close-gate (modo warn→strict).

**AC.**
- AC-1 — `ac-lint` aceita os 5 padrões válidos, rejeita prosa-livre sem `SHALL`.
- AC-2 — todo AC válido tem exatamente 1 comando runnable associado.

**Honestidade.** Ganho real mas modesto — clareza, não inteligência. É barato; faça junto com o coverage-check.

---

## Ponto 4 — Análise de consistência cruzada PRÉ-execute (coverage gate) `[Tier 2 / Fase 2]`

**Objetivo.** Antes de codar, provar que **todo AC mapeia para uma task e toda task para um AC** — sem órfãos. (Spec Kit tem `/speckit.analyze`; nós só temos gate no fim.)

**SOLID.**
- **SRP:** `coverage-check` lê `spec.md` (AC) + `wave-plan.md` (tasks) e emite um relatório de bijeção. Não edita; só relata + gate.
- **DIP:** lê os artefatos via o mesmo parser que o resto do pipeline usa (não reparseia ad-hoc).

**Fases.**
1. `mustard-rt run coverage-check` → `{acWithoutTask[], taskWithoutAc[], ok}`.
2. Gate antes de EXECUTE (Full scope): órfão ⇒ bloqueia (strict) ou avisa (warn).

**AC.**
- AC-1 — spec com um AC sem task → `coverage-check` reporta + gate bloqueia.
- AC-2 — byte-estável (snapshot insta).

**Honestidade.** Pega defeito barato cedo (escopo incompleto) que hoje só aparece no QA. Vale, mas só em Full scope — em Light é overhead; gate condicional ao scope.

---

## Ponto 5 — `constitution.md` (invariantes do projeto) `[Tier 2 / Fase 2]`

**Objetivo.** UM arquivo de princípios não-negociáveis do projeto, fonte única que o close-gate e o gate de regressão consultam. Hoje os invariantes estão espalhados (Guards por subprojeto, "regras invioláveis", vocabulário de regressão).

**SOLID.**
- **SRP:** `constitution.md` é o ÚNICO dono dos invariantes globais; Guards continuam sendo o escopo de subprojeto (não duplicar).
- **DIP:** o gate de regressão (que hoje tem vocabulário hardcoded tipo `fail-open`, `None`) passa a LER a constitution em vez de embutir.

**Fases.**
1. Template `constitution.md` (seed a partir das "regras invioláveis" já escritas: no-AI-no-binário, determinismo byte-estável, fail-open de hook, etc.).
2. O gate de regressão lê a constitution; violação citada por `file:line` referencia o artigo.

**AC.**
- AC-1 — um diff que introduz `unwrap()` fora de teste cita o artigo da constitution que proíbe.
- AC-2 — sem constitution → fail-open (não bloqueia; degrada).

**Honestidade.** É mais consolidação do que feature nova — você já TEM os invariantes, só dispersos. Baixo custo, melhora coerência e a história de venda ("o harness aplica SUAS regras, não as minhas").

---

## Ponto 6 — Detector de drift spec↔código `[Tier 3 / Fase 3]`

**Objetivo.** Quando um commit toca as âncoras de uma spec, sinalizá-la como possivelmente stale — além do gate QA-stale (que só dispara em edição da spec). (Kiro mantém spec↔código em sync; é nossa lacuna.)

**SOLID.**
- **SRP:** `drift-check` cruza âncoras-da-spec × `git diff`; só sinaliza, não decide.
- **Open-Closed:** estende o conceito QA-stale (de "spec editada" para "código sob âncora editado") sem reescrever o gate.

**Fases.**
1. `mustard-rt run drift-check` → specs cujas âncoras mudaram desde o último `qa.result`.
2. Hook pós-commit emite `pipeline.spec.drift`; dashboard sinaliza.

**AC.**
- AC-1 — editar um arquivo-âncora de uma spec Completed → ela aparece como "drifted".

**Honestidade.** Útil mas Tier 3 — só rende quando há muitas specs vivas. Não priorize antes do fosso (retrieval) estar vendido.

---

## Ponto 7 — Artefatos de projeto gerados pelo scan: rules / skills / agents `[Tier 2 / Fase 2, junto da constitution]`

**Objetivo.** Promover o que o scan JÁ minera (stack, role-affixes, recurring slices) nos mecanismos **certos** de steering — sem proliferar artefatos que driftam. (Pesquisa: a Anthropic define 7 formas de steering; cada sinal minerado mapeia pra UMA.)

**Mapeamento (SRP — cada coisa no mecanismo certo, sem sobrepor):**

| Sinal minerado | Mecanismo certo | Vale? |
|---|---|---|
| convenção cross-cutting por-path (ex.: "money = Decimal cents") | **RULE path-scoped** (`paths:` frontmatter) — upgrade dos Guards | ✅ sim |
| procedimento recorrente (recurring slice forte) | **SKILL que APONTA pro precedente vivo** (just-in-time, nunca prosa congelada) | ⚠️ medir adoção |
| papel de trabalho (search/review/implement) | **agente universal** + tools restritas + contexto injetado | ✅ já existe |
| "agente bespoke por projeto" | — | ❌ descartar (papel é universal; "do projeto" é só contexto injetado) |

**Gate de confiança (anti-over-generation).** O scan **over-gera** se não houver trava: todo afixo recorrente "parece" convenção. Só emite rule/skill quando o sinal tem **alto suporte** (recorre ≥ K) **E é não-óbvio** (não derivável de ler 1 arquivo). Senão é ruído que vira dívida.

**SOLID.** DIP — o agente (papel) depende da abstração "contexto do projeto" injetada (rules+guards+digest), nunca de um agente-por-projeto hardcoded. Cada artefato com dono único de responsabilidade: `constitution.md` (invariantes globais), rule (convenção por-path), Guards (orientação por-subprojeto), skill (procedimento → aponta precedente).

**AC.**
- AC-1 — scan emite rule path-scoped só para convenção com suporte ≥ K e fora de precedente único.
- AC-2 — skill-de-slice referencia o arquivo-precedente por path (NÃO copia o corpo).
- AC-3 — zero geração de agente-por-projeto (papéis permanecem universais).

**Honestidade.** É consolidação + um upgrade barato (Guards→rules), não feature nova grande. O valor está no **mecanismo certo + o gate de confiança**. Descartar agentes-bespoke economiza a maior parte do esforço por quase todo o valor. Regra de ouro de todos os três: **gere o mínimo, e o que gerar deve APONTAR pro código, não copiá-lo** (senão drifta — a mesma razão pela qual o mercado leu código fresco em vez de cache).

## O que PARAR de investir (anti-roadmap)

- **Orquestração pesada de pipeline** (spec→wave→QA→close re-paga em contexto a cada turno). O A/B próprio mostrou que NÃO economiza token ($5,46 com × $5,18 sem). O mercado comoditizou. **Emagreça ao mínimo viável** e realoque pro retrieval.
- **Qualquer flerte com vector DB / embedding como índice primário.** Medido (0/10) e confirmado pelo mercado (Anthropic removeu). Embedding só como complemento, nunca índice.
- **Competir em nº de integrações/agentes com o Spec Kit.** Em vez disso, **pluga sob** ele (seja a camada de retrieval que o Spec Kit não tem).

## Métrica-norte de "vendável"

Um número público e reproduzível num repo ABERTO (não a sialia privada): *"recall por intenção cross-lingual: grep/LSP X% → Mustard Y%"*. Sem essa prova reproduzível, "vendável" é narrativa. **Construir esse benchmark público é pré-requisito de qualquer pitch.**

---

## Delta: o que será REMOVIDO e ADICIONADO (inclui dashboard)

### Removido

| Onde | O que sai | Por quê |
|------|-----------|---------|
| CLI/SKILL | flags `--full` / `--enrich` | já feito nesta sessão — enrich é padrão |
| Digest | `domain_specificity_x1024` (count×idf) do caminho de ranking | REFUTADO (44%); mantém-se só como stat de corpus p/ PT-mining |
| Núcleo | qualquer rota de embedding/vector como índice primário | medido 0/10; Anthropic já removeu do Claude Code |
| Pipeline | **obrigatoriedade** de ondas por escopo Full (NÃO as ondas em si) | ver "Política de ondas" abaixo |
| `bash_command_gate.rs` | vocabulário de regressão **hardcoded** (`fail-open`, `None`, …) | migra para `constitution.md` (fonte única) |
| **Dashboard** | **aba PRD** (já read-only vestigial) | PRD morreu; `/feature` grelha inline |
| **Dashboard** | **claim de "economia de token da pipeline"** na página Economia | enganoso — o ganho real é leitura-evitada pelo retrieval, não a cerimônia |
| **Dashboard** | detalhe pesado de trace por-onda (rebaixar a mínimo) | serve debug de cerimônia, que estamos de-priorizando |

**Política de ondas (esclarecimento — as ondas NÃO são removidas).** A onda continua como capacidade; o que sai é ela ser **decretada** pelo escopo Full. O ganho real da onda é UM: despachar unidades **independentes** em paralelo (backend+frontend+schema que não se tocam). Quando as camadas se **acoplam** (tipo/contrato compartilhado — o caso comum, e o caso desta própria sessão de purpose), a onda é cerimônia sem payoff. Nova regra: ondas só se materializam quando o **planner** identifica ≥2 unidades comprovadamente independentes; Light/bugfix/"Full acoplado" rodam como **um execute coerente**. A decisão é ganha pelo trabalho, não imposta pelo `scope-classifier`.

### Adicionado

| Fase | Onde | O que entra |
|------|------|-------------|
| 0 | `apps/mcp` | servidor MCP `mustard-intel`: `find_by_intent`, `purpose_of`, `lexicon_bridges` (adaptador fino sobre core/scan) |
| 0 | enrich/léxico | pontes sinônimo→sinônimo a partir de `matchedTerms`; prompt de sumário cita verbo canônico |
| 0 | `benchmarks/` | conjunto rotulado PÚBLICO (repo aberto) + runner `recall-bench` |
| 1 | close-gate | invariante `qa.author ≠ execute.author`; `author_role`/`agent_id` nos eventos |
| 1 | QA | prompt adversarial (refutar em corpus real); `mustard-review` vira gate obrigatório |
| 2 | `apps/rt` | `ac-lint` (valida EARS + comando runnable); `coverage-check` (AC↔task, pré-execute) |
| 2 | raiz | `constitution.md` + gate de regressão lê dela |
| 3 | `apps/rt` + hook | `drift-check` + evento `pipeline.spec.drift` |

### Dashboard — o pivô (não esquecido, e honesto)

**O dashboard NÃO é a superfície de venda.** O comprador vive no agente dele (Claude Code/Cursor/Spec Kit), não num app Tauri. O valor do dashboard é pra VOCÊ observar runs. Então o movimento é **pivotar de "observabilidade de cerimônia" para "prova de retrieval"** — cortar peso de pipeline e adicionar a única tela com valor de venda (screenshot de pitch):

**Adicionar ao dashboard:**
- **Página Recall/Intel (a tela que vende):** o benchmark público (`grep/LSP X% → Mustard Y%`), cobertura do `purpose` (%), breakdown de hit por tier (nome × purpose × léxico), nº de pontes, e a **fila de misses** (queries que ainda falham = backlog de melhoria visível).
- **Indicador de independência de verificação** na Specs/trace (`verificado por agente ≠ implementador ✓`) — a credibilidade do output, visual.
- **Tags EARS + avisos do `ac-lint`** na exibição de AC; **relatório de cobertura** (AC↔task, órfãos destacados) na spec.
- **Filtro "drifted"** na Specs page (ao lado de Ativas/Suspeitas/Encerradas).

**Reenquadrar (não remover) a página Economia:** parar de afirmar economia da cerimônia; medir o ganho REAL = leituras de arquivo evitadas pelo digest (retrieval-locating) vs exploração às cegas. Esse número é honesto e ainda favorável.

**SOLID no dashboard:** cada painel lê UMA projeção do NDJSON (já é o padrão pós-remoção do SQLite); a página Recall lê a projeção do `recall-bench`, não recalcula. Painéis novos são aditivos (Open-Closed) — não tocam Specs/Overview existentes.
