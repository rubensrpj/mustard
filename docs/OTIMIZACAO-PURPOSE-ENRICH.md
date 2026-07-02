# Otimização do purpose-enrich — baratear o índice de recall sem perder qualidade

> Documento de arquitetura (regra "planos → docs/"). Aprovado em 2026-06-26: **medir primeiro (Fase 0)**.
> Origem: discussão sobre o custo do enriquecimento de `purpose` no `/scan` escalar linear com o tamanho do repo.

## 1. Problema

O `/scan` enriquece **todo** método de lógica com um `purpose` (uma frase de propósito) escrito por **Sonnet** lendo o corpo. Esse é o custo de token dominante do `/scan` e cresce **linear com o número de métodos** — ~$50 (estimativa interna) para o sialia, e proporcionalmente mais para repos grandes. Haiku foi testado e **reprovou** (recall 3-4/10; um resumo errado polui o índice permanentemente), então "usar modelo mais barato" está bloqueado pela qualidade. A preocupação é **projetos grandes**.

## 2. Fatos validados (fecham o espaço de solução)

- **Completude é requisito, não desperdício.** No miss (`centralFound=false`) não há como saber *qual* método enriquecer — identificar o método relevante pelo significado **é** o trabalho do índice. Logo, enriquecimento **lazy / sob demanda foi refutado**: não dá para enriquecer só "o que vai ser pesquisado" porque descobrir isso é justamente o que o índice completo faz.
- **O custo de token mora no enriquecimento** (corpos fatiados que vão aos agentes + baseline de cada agente + saída). O `grain.model.json` **nunca entra na janela de nenhum agente** — é lido só pelo binário Rust (`feature/SKILL.md:60` proíbe lê-lo inteiro; toda menção a ele nas skills é `--model <caminho>` passado ao binário). Os corpos da worklist vêm de **ler o código-fonte e fatiar** (`enrich_purpose.rs:214-219`), não do modelo.
- **Sidecar NDJSON ≠ economia de token.** Separar `purpose` para um arquivo à parte é ganho de **disco/CPU e simplicidade de código** (mata o carry-forward fuzzy de 2,5–3,5 MB por scan), mas **zero token** — o modelo não é lido por IA. Vale fazer, mas por esse motivo, não como economia de IA.

## 3. O enquadramento afiado (descoberta do RESULT.md medusa v3, inglês-canônico)

Depois da mudança para **retrieval inglês-canônico** (`english-canonical-retrieval`), no benchmark medusa (`labels-v2-en.ndjson`):

| caminho | name@5 | purpose@5 | **combined@5 (name OU purpose)** |
|---|---|---|---|
| EN canônico | **0.82** | 0.73 | **1.0 (11/11)** |

Leitura: **o name-match (grátis, determinístico) já carrega a base** (0.82@5); o `purpose-search` virou a **rede secundária** que pega só os casos onde o nome diverge do domínio. O `combined@5` = 1.0 é o que o juiz Sonnet realmente enxerga.

**Consequência para a otimização:** todo o custo de Sonnet do enrich existe hoje para **fechar o resíduo** (~18% dos casos) que o name-match não pega. O substituto barato (embeddings) **não precisa ser o índice inteiro** — precisa apenas **igualar a contribuição-resíduo do purpose a custo ~zero**. A métrica de decisão correta não é "embedding ≥90% sozinho", e sim:

> **`combined@5(name + embedding)` ≈ `combined@5(name + purpose-Sonnet)` ≈ 1.0 ?**

Se sim, embeddings substituem o Sonnet no caminho quente sem perda de recall, a custo local desprezível.

## 4. Direção

Manter o índice **completo** (requisito), mas baratear o **custo por método**: trocar o sumário-Sonnet por um **embedding local** como substrato de recall — um vetor por método (cobertura total), computado em Rust (ONNX/`fastembed`, sem Python, local-first), ~1000× mais barato por item que uma chamada Sonnet. A própria medição prévia do projeto aponta viabilidade (memória *domain-vs-plumbing*: embeddings pré-treinados + proveniência ≈ 94%, acima dos 7-8/10 do sumário-LLM). Como o retrieval já é inglês-canônico (query traduzida para EN), basta um modelo de embedding **monolíngue** de prateleira.

## 5. Plano em fases

### Fase 0 — Medir antes de construir (APROVADA)
- **Corpora:** medusa (TS) e saleor (Python) — labels versionados no repo, repos OSS públicos → **self-contained, sem depender do sialia privado**.
- **Retrievals comparados por caso:**
  - (a) **name-match** — baseline grátis (já no `recall-bench`).
  - (b) **purpose-search Sonnet** — a rede secundária cara de hoje (já no `recall-bench`).
  - (c) **embedding local** — protótipo novo (isolado, NÃO no binário shippado ainda).
- **Métrica de decisão:** `combined@5(name + embedding)` vs `combined@5(name + purpose)`. Gate: o embedding fecha o resíduo tão bem quanto o Sonnet (≈1.0)?
- **Protótipo isolado** via `fastembed`-rs (ONNX): embedar corpo/assinatura de cada método + a query EN, similaridade de cosseno, ranquear, medir recall@1/@5 — uma terceira retrieval comparável ao `recall-bench`. Mantido fora do binário shippado para **não comprometer empacotamento** enquanto o número não justifica.

### Resultado Fase 0 — medusa (2026-06-26)

Protótipo isolado (`fastembed 5.17.2`, ONNX local, fora do workspace), embedando o **corpo cru** de cada método (mesmo `slice_body`, cap 55). Corpus: medusa `packages/modules/order` (350 métodos), labels `labels-v2-en.ndjson` (11 casos EN). Comparado com o `recall-bench` (name) e o purpose-Sonnet documentado.

| retrieval | @1 | @5 | **combined@5 (name OU X)** |
|---|---|---|---|
| name-match | 0.636 | 0.818 | — |
| embedding **BGE-small** (corpo cru) | 0.545 | 0.818 | 0.909 (10/11) |
| embedding **BGE-base** (corpo cru) | 0.636 | **0.909** | **1.0 (11/11)** |
| purpose-Sonnet (documentado) | — | 0.73 | **1.0 (11/11)** |

**Resultado-chave:** com **BGE-base** (768-dim, local, ~440 MB, custo marginal ~zero), `combined@5(name + embedding)` = **1.0 — empata com o purpose-Sonnet (~$50/repo)**. A complementaridade fecha limpa: o embedding resgata os 2 casos que o name-match erra (`open a claim` name#6→emb#2; `mark as delivered` name#null→emb#1), e o name resgata o 1 que o embedding erra (`pick and fulfill` emb#11→name#2). Juntos = 11/11.

**Levers testados (e o que NÃO ajudou):**
- Prefixo de instrução de query do BGE → não ajudou (@1 caiu, @5 igual).
- `nome+corpo` no doc → melhorou @1 mas piorou os casos difíceis (o nome divergente embaça o alvo).
- **Tamanho do modelo (small→base) → fechou o gap** (`registerDelivery` #11→#1). Esse foi o lever real.

**Ambiente de-riscado:** ONNX Runtime nativo linkou no Windows; modelos baixam do HF; inferência sadia (query vs doc-próximo 0.78 > doc-distante 0.46).

**Ressalvas honestas:** UM benchmark (11 casos, TS) — 1 caso = 9%, então **falta saleor (Python)** para confiança cross-linguagem. BGE-base pesa ~440 MB (decisão de empacotamento da Fase 1). Recall vetorial é aproximado vs o `purpose-search` determinístico atual.

### Resultado Fase 0 — saleor (2026-06-26, Python, o caso mais duro)

Saleor `saleor/order` (Python/Django, GraphQL-heavy, 905 métodos), 10 casos. Labels pt-BR **traduzidos para EN** (a arquitetura é inglês-canônico — query chega em EN). BGE-base, corpo cru.

| retrieval | @1 | @5 |
|---|---|---|
| name-match | 0.2 | 0.6 |
| **embedding BGE-base** | 0.6 | **1.0 (10/10)** |
| purpose-search (PT antigo, documentado) | 0.2 | 0.6 |

**Resultado:** o embedding **sozinho** acerta 10/10 @5 — `combined@5(name+embedding)` = **1.0**. No projeto onde o name-match quase não ajuda (0.6) e o purpose-PT batia no limite (0.6), o embedding-EN **supera os dois**. Os 3 casos que eram a "fronteira do fosso" no setup PT (gaps de sinônimo-PT: faturar/quitado/reabastecer) **dissolveram** no inglês-canônico — a query EN embeda direto contra o significado EN do corpo, sem precisar de ponte de léxico.

## VEREDITO Fase 0 — embeddings VALIDADOS (2026-06-26)

| projeto | linguagem | name@5 | embedding-base@5 | **combined@5 (name+emb)** | Sonnet combined@5 |
|---|---|---|---|---|---|
| medusa | TS | 0.818 | 0.909 | **1.0** | 1.0 |
| saleor | Python | 0.6 | 1.0 | **1.0** | (PT: 0.6) |

**Um embedding local grátis (BGE-base, ~440 MB, custo marginal ~zero) iguala ou supera o purpose-Sonnet (~$50/repo, escala linear) no `combined@5` em duas linguagens.** O portão da Fase 0 foi atingido. Isso responde a preocupação de escala: o substrato de recall passa a custar compute local constante, não tokens de Sonnet proporcionais ao tamanho do repo.

**Ressalvas que seguem para a Fase 1:** N pequeno (10-11 casos/projeto) — sialia (privado, maior) somaria confiança mas não bloqueia o go; BGE-base pesa ~440 MB (decisão de empacotamento agora **justificada** pelo número); recall vetorial é aproximado (o protótipo ranqueia tudo sem corte — produção precisa de limiar) vs o `purpose-search` determinístico atual; o baseline Sonnet-EN do saleor não foi medido (só o PT 0.6), mas o embedding 1.0 já o supera.

**→ Recomendação: prosseguir para a Fase 1** — embeddings como substrato de recall no binário do scan, com o Sonnet saindo do caminho quente.

## Fase 1 — IMPLEMENTAÇÃO (2026-06-26)

**Feito e verificado:**
- **Crate novo `apps/embed` (`mustard-embed`)** — isolado do workspace (no `exclude`, como o dashboard): o ONNX/fastembed NÃO entra no binário do hook (`mustard-rt`, roda a cada ação) nem no scan determinístico. Dois subcomandos:
  - `mustard-embed build --model .claude/grain.model.json` → embeda o corpo de cada método de lógica (mesmo `slice_body`, pula testes/build dirs) e grava `.claude/grain.vectors` (binário compacto: magic, tag do modelo, dim, e por registro o caminho + vetor f32). Saída JSON `{ok, methods, dim, out}`.
  - `mustard-embed search --intent "<texto>" --vectors .claude/grain.vectors` → embeda a busca com o MESMO modelo do índice (tag lida do cabeçalho), ranqueia por cosseno, devolve `{"intent", "files":[{"file","score"}]}` — **mesma forma de saída do `purpose-search`** (troca drop-in no caminho de miss).
- **Compilado + instalado** (`cargo install --path apps/embed` → `~/.cargo/bin/mustard-embed`).
- **Smoke test (medusa):** `build` = 294 métodos de lógica (filtro de não-fonte correto), dim 768; `search "mark an order as delivered"` → `register-delivery.ts` #1 (caso que o name-match errava); `search "open a claim"` → `create-claim.ts` #2 (name tinha #6).
- **SKILL religado** (`scan/SKILL.md`, template + deployado): etapa B trocada — saiu toda a maquinaria Sonnet/batches/Workflow, entrou o comando único `mustard-embed build`; REGRAS INVIOLÁVEIS atualizadas (A Guards = único passo com LLM; B índice de recall = local, sem LLM, sem prompt de gasto).

**Empacotamento + wiring (FEITO 2026-06-26):**
- **`install.ps1`** — `mustard-embed` adicionado ao build/install (best-effort: build pesado do ONNX, falha AVISA mas não aborta o núcleo; recall é fail-open). Sintaxe validada (parse-check).
- **Gatilho de miss religado** — `digest-validate.md` passo 0 (`centralFound=false`), template + deployado: era `mustard-rt run purpose-search` → agora `mustard-embed search --vectors .claude/grain.vectors`. Isso fecha o loop: `/scan` constrói os vetores, a recuperação de miss os usa. (Removido o juiz Sonnet `purpose-judge-render`: a similaridade já ordena por relevância — lê-se o top pick.)
- **Testes** — `apps/embed` agora tem 4 testes unitários (slice_body, cosine zero-safe, filtro de não-fonte ancorado, round-trip do arquivo de vetores), 4/4 verdes. Corrigido um bug no filtro (`contains("test_")` substring → falso-positivo em `latest_*`; agora ancorado no nome do arquivo).

## Teste de campo no sialia + endurecimento (2026-06-26)

Rodado o `/scan` no sialia real (5675 métodos de lógica, repo grande C#+TS misto). **Funciona de ponta a ponta**, mas o spot-check de recall expôs o limite que os benchmarks fáceis (medusa/saleor, módulos pequenos focados) escondiam:

- Busca "efetivar previsão de pagamento" → alvo (`IPayableService`) só em **#2** (um componente de tela em #1); "suspender por inadimplência" → `ContractDunningService` **fora do top-3**. Num repo grande e heterogêneo o alvo compete com interfaces, serviços de status/state e componentes de tela de vocabulário parecido → o embedding sozinho é **recall, não precisão**. O `combined@5 = 1.0` do medusa/saleor era otimista.
- Bônus observado: PT funciona (até melhor que EN no sialia) porque o código C# tem **doc-comments em português** que o embedding casa.

**Dois fixes implementados + verificados:**
1. **Incremental (regressão minha, corrigida).** O `build` re-embedava TODOS a cada scan. Agora guarda `(file,name,line)` + hash FNV do corpo no `grain.vectors` (formato `MEV2`) e **reusa o vetor de corpos inalterados** — só re-embeda novos/alterados. Verificado na medusa: build completo 294 métodos = **45,8 s**; 2º build (incremental) = **0,063 s** (reusa 294, embeda 0). Para o sialia, o 2º `/scan` em diante é instantâneo; a lentidão NÃO se repete.
2. **Re-rank inline (recupera a precisão sem custo por método).** O `search` agora devolve `method`+`line` por hit; o `digest-validate` (passo 0, ambas as cópias) instrui o orquestrador a **ler os top ~3-5 spans candidatos e escolher o que EXECUTA a ação** — uma leitura por miss, recupera o papel do antigo juiz-Sonnet-por-método a custo ~zero. Resolve o "#1 errado" do spot-check (o orquestrador lê e rejeita o componente de tela, fica com o serviço que efetiva).

**Custo do primeiro build (honesto):** ~7k métodos no CPU com BGE-base = ~15 min, uma vez. Para acelerar o primeiro build (não os re-scans, que já são instantâneos): slice mais curto ou BGE-small — knobs disponíveis, não aplicados ainda.

**Não medido com rigor:** sem labels reais (o usuário não pode prever o que o usuário-final digita), o `combined@5` no sialia fica como observação qualitativa, não número. O re-rank é mecanismo sólido (modelo capaz lê o código real), validado por construção, não por métrica no sialia.

## SOLUÇÃO — retrieve + rerank (cross-encoder), 2026-06-26

O embedding sozinho (qualquer modelo: BGE, jina-code) regrediu num repo grande porque compara dois vetores SEPARADOS — perde a interação query↔código, e o corpo cru é ruidoso. O padrão SOTA de busca de código por linguagem natural (pesquisado na web: CoIR, retrieve-and-rerank) é **duas etapas**, e o `fastembed` tem as duas localmente:

1. **Retrieve (bi-encoder, recall):** cosseno → top-50 candidatos. O alvo só precisa ESTAR nesse conjunto.
2. **Rerank (cross-encoder, precisão):** `JinaRerankerV2BaseMultilingual` lê **(query, corpo) JUNTOS** e pontua relevância real — é um "juiz" local, grátis, **multilíngue** (lida com query PT direto), sem custo por método.

Implementado em `mustard-embed search` (retrieve top-50 → lê os corpos via `--model` → rerank → top-K). Validado no **sialia real** (jina-code retrieve + jina-rerank), justamente onde o embedding tinha falhado:

| query (EN) | embedding sozinho | **após rerank** |
|---|---|---|
| make a forecasted payable effective | `PayableService.EffectivateAsync` **fora do top-10** | **#2** ✓ |
| suspend a contract due to non-payment | `ContractDunningService` #4-ish | **#5 (no top-5)** ✓ |

O caso que estava perdido (efetivar) voltou para #2; o de inadimplência fica no top-5 (entre vários "Suspend*" — ambiguidade de domínio genuína, mas no conjunto que o orquestrador lê). **O recall foi recuperado a custo local zero.** Não é #1 perfeito sempre (ambiguidade real persiste), mas o alvo passou a ser recuperado — que era o problema.

Wiring: `search` faz retrieve+rerank por padrão (fail-open para cosseno se faltar `--model`/reranker); `/scan` build usa `--embed-model code` (jina-code); `digest-validate` passa `--model`. Modelos baixados 1× no cache da máquina (~2 GB total: embed code + reranker multilíngue).

## Medição honesta + correção de política de língua (2026-06-26)

Adicionado `mustard-embed eval` (recall@k, separando teto-do-retrieve do recall-final) + `gen-labels` (deriva o conjunto rotulado dos doc-comments, estilo CodeSearchNet). Gerado 428 recall-holes reais do sialia (só os divergentes; name-match@5=6,5% confirma que são holes genuínos).

**Erro corrigido:** medi primeiro com queries em **PT** — viola a política decidida ([[project-mustard-english-canonical-retrieval]]): só a spec apresentada é PT; **retrieval e inputs são INGLÊS** (a query chega traduzida). Re-medido com os MESMOS labels traduzidos para EN:

| métrica (428 holes) | PT (errado) | **EN (política correta)** |
|---|---|---|
| retrieve@50 (teto) | 0,69 | **0,91** |
| retrieve@10 | 0,48 | 0,69 |
| @5 sem rerank | 0,36 | 0,52 |

Sob a política correta, o jina-code dá **teto de ~91% recall@50** (e subestimado: metade dos labels aponta p/ interfaces de corpo trivial). O modelo NÃO é o gargalo que o número PT (69%) sugeria. Fiação corrigida: `digest-validate` passo 0 agora passa a intenção **traduzida para inglês**, não as palavras PT do usuário.

## Daemon de busca (2026-06-26) — latência + multi-sessão

O `mustard-embed search` cold-loadava os modelos a cada chamada (~1s embed / ~23s rerank). Solução: um daemon que carrega UMA vez e atende por socket localhost (NDJSON sobre TCP, sem deps).

- **`mustard-embed serve`** (porta 7723) — carrega os modelos uma vez; `Engine` (caches de modelo+índice) compartilhado entre threads via `Arc<Mutex>`. **Thread-por-conexão**: aceita sessões concorrentes sem recusar; a inferência serializa no Mutex (correto no CPU). Indexa **por caminho do `grain.vectors`** → vários projetos/sessões num só daemon, modelos carregados 1×. Recarrega um índice se o arquivo mudar (mtime). Auto-sai após 30min ocioso.
- **`search`** detecta o daemon e faz proxy; se não houver, **sobe-o destacado** (Windows-safe: `DETACHED_PROCESS | CREATE_NO_WINDOW`, sem handles herdados — evita o hang de [[project-mustard-sessionstart-hang-pipe-inheritance]]) e faz poll; fallback cold in-process se indisponível. Transparente para o orquestrador.
- `install.ps1` para o daemon ao reinstalar (próxima sessão sobe fresco).

**Medido (sialia):** retrieve quente **~1s**; rerank quente **~13s** (cold 23s — o daemon tira o load; os 13s restantes são a inferência do cross-encoder sobre 50 docs no CPU, inerente). Duas buscas concorrentes (2 sessões) ambas servidas ✓. Bug corrigido: o socket aceito herdava non-blocking → `read_line` dava WouldBlock e a conexão caía (daemon não servia nada); agora torna-se blocking no accept.

**Caminho pervasivo barato = retrieve-only (~1s, compartilhado):** é o que o orquestrador usa em todo lugar, reduzindo chamadas de LLM. Rerank (13s no CPU) fica opt-in; GPU o tornaria viável sempre.

**Restante (follow-ups, NÃO bloqueiam o uso no sialia):**
1. **Ref profundo** `refs/scan-enrich-purpose.md` ainda descreve o fluxo Sonnet antigo (doc carregado sob demanda; não é o caminho de execução). Reescrever para embedding.
2. **Destino do código Sonnet legado** (`enrich-purpose`, `purpose-search`, `purpose-judge` no `rt`) — manter como fallback ou remover após confiança mais ampla.
3. **Modelo BGE-base** — baixar-sob-demanda (atual, HF no 1º `build`) vs embutir os ~440 MB no instalador.
4. **DRY** — `slice_body`/`cosine` estão COPIADOS de `enrich_purpose`/`recall_bench` no crate isolado (risco de divergir do enrich). Compartilhar via dep em `mustard-core` ou marcar sync explícito.

### Fase 1 — Embeddings como substrato completo (se a Fase 0 validar)
- Vetor por método no scan (cobertura total). `purpose-search` no miss = similaridade de vetor. Sonnet **sai do caminho quente**.
- Decisão de empacotamento entra **aqui** (modelo ONNX ~100-500 MB no binário), justificada pelo número.

### Fase 2 — Híbrido: Sonnet só no resíduo ambíguo
- O índice barato e completo (embedding) é o que **aponta onde o LLM agrega valor** (os casos que o vetor não desambigua). Gasta-se Sonnet só ali — *lazy no modelo caro, completo no substrato barato*. Resolve a objeção "como saber o que enriquecer": o substrato cheap+completo decide.

## 6. Ganhos paralelos (preservam completude, independem da Fase 0)

1. **Dedup de corpos idênticos** — repo grande tem muito boilerplate (handlers CRUD, DTOs, acessadores gerados). Agrupar por hash de corpo normalizado colapsa N corpos iguais em 1 chamada de LLM; todos recebem o `purpose`. Menos chamadas, completude intacta.
2. **Fatia de corpo menor/esperta** — assinatura + linhas de sinal (atribuições, returns, transições de status, DTOs) em vez de 55 linhas cruas (`slice_body(..., 55)`). Corta o termo dominante de input. **Validar no recall-bench antes de confiar** — cortar demais derruba a qualidade.
3. **AgentType enxuto** nos sumarizadores — só `Read`/`Write` em vez de `general-purpose` → baseline ~27k→~8k tokens por agente.
4. **Batch ~150-200** (em vez de ~50) — já aplicado no `scan/SKILL.md` (corta o nº de agentes ~3-4×).

## 7. O que NÃO faremos

- **Lazy / sob demanda sem índice** — refutado (§2): não dá para saber o que enriquecer no miss.
- **Sidecar NDJSON rotulado como economia de token** — é ganho de disco/código, não de IA.
- **Voltar ao Haiku** — 3-4/10 provado; resumo errado é pior que ausente.

## 8. Decisões em aberto

- **Modelo de embedding** (Fase 0/1): candidato `fastembed`-rs com um modelo EN pequeno (ex.: BGE-small-en, ~130 MB ONNX). Trade-off de peso de empacotamento decidido na Fase 1, com o número da Fase 0 na mão.
- **Recall aproximado vs determinístico:** o `purpose-search` atual é 100% determinístico; embeddings introduzem limiar de similaridade (recall aproximado). Aceitável para um *fallback* de miss (já é best-effort), mas é mudança de filosofia que o projeto valoriza — decidir consciente na Fase 1.
