---
id: spec.consertar-loop-outcome-digest-janela
---

# consertar loop de outcome do digest: janela de pesquisa por-query com fechamento deterministico do marcador active-research

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

consertar loop de outcome do digest: janela de pesquisa por-query com fechamento deterministico do marcador active-research.

Âncoras (do scan):
- apps/rt/src/commands/feature.rs (marker, feature, emit, anchors)
- apps/dashboard/src-tauri/src/amend_queries.rs (window, session)
- packages/core/src/domain/model/contract.rs (outcome, observer, session)
- apps/dashboard/src/api/promptEconomy.ts (session)
- apps/scan/src/classify.rs (marker)
- apps/cli/build.rs (emit)
- apps/mcp/src/lib.rs (session)
- apps/rt/src/hooks/observe/session_stop_observer.rs (marker, observer, window, session)
- apps/rt/src/hooks/observe/amend_window_inject.rs (observer, window, session, emit)
- apps/rt/src/commands/lexicon_suggest.rs (window, correlate, session)
- apps/rt/src/hooks/observe/change_request_log.rs (outcome, observer, emit)
- apps/rt/src/hooks/session/session_knowledge_observer.rs (observer, session, emit)

Fatias recorrentes (precedente a espelhar): Result+path (×2) — o mecanismo `amend_window_inject` (`apps/rt/src/hooks/observe/amend_window_inject.rs`) já implementa uma janela com `opened_at`/`expires_at` persistida em JSON e expiração por idade; é o molde a espelhar.

**Por que agora.** O "loop de outcome" — a correlação entre o que o digest sugeriu (`feature.query`) e o que o operador realmente abriu (`feature.outcome`) — é o keystone para medir QUALQUER mudança no digest (A/B). Hoje ele está quebrado por dois defeitos no marcador `active-research.json` (escrito por `feature.rs:323 drop_research_marker`, lido por `feature_outcome_observer.rs`):

1. **Marcador único sobrescrito por query.** `drop_research_marker` grava um só arquivo por sessão, sobrescrito a cada `feature`. Numa rajada de pesquisa (medido nos eventos gravados: **uma sessão com 12 queries, outra com 7**), só sobrevive o marcador da última query. **Medido: 29 de 41 queries gravadas ficam com ZERO outcome.**
2. **Marcador nunca é limpo.** Não há `remove_file`/expiração/cleanup (confirmado por grep: o marcador só é escrito em `feature.rs:323` e lido pelo observer). Depois da última query ele permanece no disco o resto da sessão, então TODA leitura/edição da fase de implementação emite um `feature.outcome` contra o marcador velho. **Medido: 655 de 737 outcomes (89%) empilham na última query de cada sessão, quase todos `wasAnchor=false` (ruído). Só 4 de 41 queries têm um hit real de anchor.**

Consequência: o ground-truth é simultaneamente *lossy* (janelas anteriores descartadas) e *ruidoso* (última janela poluída pela sessão inteira). A medição A/B da feature `digest-concern-split-por-co` voltou inconclusiva por causa disso, não por falha do código medido. Sem consertar o instrumento, nem a co-ocorrência determinística atual nem o futuro juiz Haiku (uma camada acima) podem ser validados.

## Usuários/Stakeholders

- **O processo de evolução do digest (indireto, mas é o ponto):** qualquer A/B de mudança no digest — a co-ocorrência atual, o futuro juiz Haiku, ajustes de ranking — passa a ter ground-truth confiável. Hoje é cego.
- **O `digest-precision` (`apps/rt/src/commands/digest_precision.rs`):** a projeção que dobra `feature.query × feature.outcome` em recall/precisão deixa de reportar números inflados por ruído de fase de implementação.
- **O operador/IA (indireto):** a métrica de "o digest acertou onde olhar?" volta a refletir a realidade.

## Métrica de sucesso

A correlação query→leitura fica acurada: **cada `feature.query` recebe os `feature.outcome` apenas da SUA janela de pesquisa, e leituras/edições posteriores ao fim da pesquisa (fase de implementação) NÃO contam.** Verificável sobre fixture/eventos: numa sequência [query → 2 reads de anchor → tool não-pesquisa → 3 reads], só os 2 primeiros reads viram outcome daquela query. Invariante de não-regressão: o `digest-precision` continua produzindo JSON byte-estável e ordenado com a nova forma de marcador.

## Não-Objetivos

- **Mudar o ALGORITMO do digest** (co-ocorrência, ranking, Haiku): esta spec conserta só o INSTRUMENTO de medição (a janela de outcome), não o que é medido.
- **Persistir histórico de janelas / banco**: a janela é um marcador efêmero no disco da sessão, como hoje; nada de SQLite.
- **Bloquear ou alterar qualquer tool**: o observer é telemetria fail-open (`observe()` → `()`), nunca veredito — uma leitura jamais é barrada.
- **Reescrever `digest-precision`**: ele continua consumindo `feature.query × feature.outcome`; só a qualidade dos `feature.outcome` melhora.

## Critérios de Aceitação

- **AC-1** — Build do pipeline verde
  Command: `cargo build`
- **AC-2** — Suite completa verde (sem regressão)
  Command: `cargo test`
- **AC-3** — Uma leitura DENTRO da janela aberta (logo após a query, antes de qualquer tool não-pesquisa) emite `feature.outcome`
  Command: `cargo test outcome_within_open_window_emits`
- **AC-4** — Uma leitura DEPOIS do fechamento da janela (após o primeiro tool que não é Read/Edit/Write) NÃO emite nada
  Command: `cargo test outcome_after_window_close_is_silent`
- **AC-5** — Uma leitura após a expiração por idade da janela NÃO emite nada
  Command: `cargo test outcome_after_age_expiry_is_silent`
- **AC-6** — `digest-precision` continua produzindo JSON byte-estável consumindo a nova forma de marcador (sem regressão)
  Command: `cargo test digest_precision`

## Arquivos

- `apps/rt/src/commands/feature.rs` — `active_research_marker`/`drop_research_marker` (linhas 291-336): a janela passa a carregar estado de abertura/expiração (espelhar `WindowState` de `amend_window_inject`: `opened_at`/`expires_at`); cada `feature` abre uma nova janela. Camada: **rt (escritor do marcador)**.
- `apps/rt/src/hooks/observe/feature_outcome_observer.rs` — disciplina de janela: (a) só emitir `feature.outcome` enquanto a janela está ABERTA (não expirada por idade); (b) passar a observar TODO `PostToolUse` e, no primeiro tool que NÃO é Read/Edit/Write, FECHAR a janela (expirar/remover o marcador) — assim as leituras da fase de implementação não vazam; (c) backstop de expiração por idade (campo `expires_at` no marcador). Camada: **rt (observer/leitor)**.
- `apps/rt/src/registry.rs` — se o observer passar a disparar em `PostToolUse` genérico (não só Read/Edit/Write), ajustar o registro do trigger. Camada: **rt (wiring)**.
- `apps/rt/src/commands/digest_precision.rs` — verificar/ajustar o consumo se a forma do marcador mudar; manter saída byte-estável. Camada: **rt (consumidor — só se necessário)**.

## Limites

IN: dar semântica de janela (abre na query, fecha no primeiro tool não-Read/Edit/Write OU por expiração de idade) ao marcador `active-research.json`; o observer só emite `feature.outcome` dentro da janela aberta; espelhar o padrão `WindowState` (`opened_at`/`expires_at`) do `amend_window_inject`; manter fail-open e byte-estabilidade do `digest-precision`.
OUT: mudar algoritmo/ranking do digest; juiz Haiku; persistência em banco; bloquear tools; reescrever `digest-precision`.

DECISÃO DE DESENHO (a confirmar na implementação): marcador único sobrescrito + fechamento de janela resolve o defeito DOMINANTE (os 655 outcomes de ruído da última query). Marcador POR-QUERY (chaveado por id/ts) só é necessário se a atribuição correta em rajadas de query (as 29 zeradas) for considerada load-bearing — em rajada, as leituras geralmente vêm DEPOIS da última query, então atribuí-las à última query é defensável. Começar pelo mais simples (único + fechamento); só ir pra por-query se um AC exigir.