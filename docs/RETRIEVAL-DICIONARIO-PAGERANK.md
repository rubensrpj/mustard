# Retrieval por Dicionário + PageRank — Design

> Como o mustard aponta, de forma **determinística e sem LLM pesado por consulta**, o conjunto certo de arquivos a partir de um prompt em linguagem natural — pra o agente de código receber os arquivos exatos em vez de grepar e ler tudo.

## Problema

O trabalho central: dado um prompt do usuário, um passo **determinístico** (sem LLM caro por consulta) lê o `grain.model.json` (o "banco de dados" do projeto, construído pelo scan) e entrega **só os arquivos certos** (os insumos).

Agravantes reais (medidos no sialia):
- **Cross-língua:** prompt em PT (linguagem de negócio) sobre código em inglês/C#/TS → casamento léxico falha por descasamento de vocabulário e falsos cognatos (`cancelado`→cancel, `cores`→core).
- **Poliglota + código gerado:** espelhos Kubb (`.ts` de C#) e barris de enum inflam a centralidade no grafo e poluem o ranking.

## O que já tentamos — e por que não bastou

Duas gerações, ambas medíocres ("sempre retornava lixo"):
1. Léxico curado (`pt-en.toml`) + `purpose` (resumo Haiku por método) + BM25 sobre nomes/purposes.
2. Retrieval "inglês-canônico" só por nome + tradução da query na orquestração.

Verificado no git: a pilha era grande — `mustard-embed` (BGE / Jina-code / Multilingual-E5 + rerankers via ONNX), `enrich_purpose` (648 linhas), `lexicon-enrich`/`pt-mining`, tradução PT→EN. Removida no corte enxuto (`12b32d51`) por peso e por "ficar ociosa" (recuperável de `12b32d51^`).

Diagnóstico da pesquisa (24 fontes, verificação adversarial de 25 afirmações → 19 confirmadas):
- Todas as gerações atacaram **um eixo só — o CASAMENTO** (fazer a query bater no nome/vocabulário). Terra queimada.
- O **RANKING** (qual match é O arquivo) sempre foi fan-in. **PageRank nunca foi tentado** — é o padrão da indústria (Aider repo-map: tree-sitter + PageRank, zero LLM na consulta), o único eixo virgem.
- Retrieval determinístico puro tem teto (~39–52% Acc@1 nos benchmarks; menos num corpus poliglota/PT). A meta certa é **lista curta de alta cobertura (Acc@5/@10)**, não top-1 — o agente de código faz a escolha final.

## Arquitetura

**Core em inglês.** A query é ancorada ao vocabulário do projeto; índice e resumos vivem em inglês.

### 3 artefatos, construídos no SCAN (build-time; IA permitida no build, **nunca** no prompt)

1. **`grain.model.json`** — o grafo de símbolos + dependências (já existe).
2. **Dicionário do projeto** — o artefato-chave. Um mapa `conceito → termo real que o código usa`, próprio deste repositório:
   - **Lado inglês:** extração **determinística** de vocabulário sobre TODO o texto (identificadores + comentários + docstrings + entidades), ranqueado por IDF. Grátis, cobre tudo.
   - **Lado da língua do projeto (PT):** *aliases* gerados por um **modelo forte** (Sonnet/Opus) SÓ sobre o vocabulário distintivo agregado (centenas de termos deduplicados, não milhares de métodos). Barato.
   - É o **tradutor**: no prompt, PT → lookup no dicionário → termos+arquivos reais. Determinístico (stem + lookup; o mustard já tem stemmer/stoplist PT).
3. **`purpose`** (opcional, preguiçoso) — resumo por método (reforço de recall na cauda: método sem comentário e nome opaco). Só é pago se a medição exigir; sob demanda, modelo barato/local, incremental.

### Fluxo da CONSULTA (barato, sem LLM pesado)

1. Prompt PT → **lookup no dicionário** → sementes (termos reais que existem no índice).
2. Casa as sementes contra nomes + purposes.
3. **PageRank personalizado** sobre o grafo de dependências, semeado pelos matches (peso extra nos identificadores citados; **demoção de código gerado antes** de ranquear), aritmética de **ponto-fixo** (byte-estável, como o BM25 ×1024) → **top-5/10**.

Cada peça reforça a próxima: **dicionário** ancora a query (boas sementes), **purpose** enriquece o casamento, **PageRank** ranqueia por estrutura (resgata semente fraca via propagação no grafo, independente do idioma).

## Controle de custo (o custo inicial é o risco)

- A cobertura de "tudo" é **determinística → grátis** (vocabulário de nomes/comentários/docstrings; comentário já é domínio em linguagem natural).
- A IA fica confinada aos **aliases do vocabulário agregado** (dezenas de chamadas em lote → centavos a poucos dólares).
- O **purpose por-método (o caro) é OPCIONAL e PREGUIÇOSO** — dicionário + PageRank primeiro, mede; purpose só se a régua pedir, sob demanda, cheap/local.
- Cortes adicionais: pular gerado/trivial (`generated-markers.toml`), lote, incremental por `body_hash`.

## Plano de ondas + aceite

**Regra de ouro:** cada onda prova ganho de **Acc@5 contra a baseline; sem ganho → morre** (impede virar feature ociosa — foi a ausência de medição que matou embed/purpose antes).

1. **Baseline (a régua)** — harness de ~10–15 prompts PT reais do sialia com o arquivo-alvo anotado à mão; digest de hoje **com** a tradução → Acc@5 atual.
2. **Dicionário do projeto** — extração determinística + aliases (modelo forte, agregado).
3. **Tradução ancorada** — lookup no fluxo da consulta; mede vs. baseline.
4. **Ranker PageRank** — personalizado + demoção de gerado + ponto-fixo + top-5/10; mede.
5. **Purpose (condicional)** — só se 3–4 não fecharem; preguiçoso.
6. **Limpeza** — purgar docs que citam comandos deletados; conferir SKILLs.

## Decisões-chave

- **PageRank no centro, não na manga:** é o eixo (ranking) que nunca fixamos; foi o fan-in que deixou todo casamento bom virar ocioso.
- **Dicionário auto-construído, não curado:** o lado inglês vem do código; o lado PT o modelo deriva *daquele* termo. Não é o `pt-en.toml` (lista chapada à mão) que falhou.
- **IA no build, nunca no prompt:** dicionário e purpose são artefatos de scan; a consulta é lookup + PageRank, 100% determinística.
- **Medir ou morrer:** cada onda tem régua e critério de morte.

---
*Pesquisa completa (Aider/RepoMapper, LocAgent, Agentless, SweRank, CodeXEmbed) e evidências: deep-research task `whffehuib`, 2026-07-08.*
