# Tactical Fix: price_frame fallback quando model é None/desconhecido

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-23T00:30:00Z
### Lang: pt
### Parent: [[2026-05-22-economia-didatica-e-economias-reais]]

## Contexto

Derivado de [[2026-05-22-economia-didatica-e-economias-reais]].

Hoje `price_frame` em `packages/core/src/economy/sources/transcript.rs:197-215` zera o custo (`None`) sempre que:

1. O atributo `model` da span está ausente — `let model = model?;` curto-circuita
2. O modelo é desconhecido pela tabela de preços — `if in_per_m == 0 && out_per_m == 0 { return None; }`

Consequência: linhas de `spans` (e, depois da migração, de `run_usage`) com 15k tokens registrados mas `cost_usd_micros = NULL`. A tabela "Custo estimado por spec / onda" mostra `$0,00` enganoso. O usuário não consegue diferenciar "rodou de graça" de "rodou mas não soubemos precificar".

A causa é que algumas spans (especialmente as emitidas pelo próprio mustard-rt ou via MCP) não carregam o atributo `gen_ai.request.model`. A Anthropic não cobra de graça — só não temos como saber qual modelo foi usado.

## Decisão de design

Em vez de retornar `None` (que vira NULL no SQLite e some no agregado), usar o **modelo default do project routing** (sonnet) como fallback de precificação. Razão:

- A routing table de Mustard (CLAUDE.md) tem **Default: sonnet** — é a baseline mais provável quando nada está explícito
- Subestima opus calls, superestima haiku calls — mas o erro médio é pequeno e o usuário vê uma estimativa, não um zero falso
- Cada fallback emite `eprintln!` em debug para rastreabilidade (vai pra stderr do mustard-rt, não polui stdout)
- O retorno passa a ser sempre `Some(...)` quando há tokens > 0; só permanece `None` se input+output forem ambos zero (caso degenerado)

Alternativa rejeitada: retornar uma sentinela negativa para "preço desconhecido". Quebraria a soma agregada e exigiria mudança em todos os leitores. Fallback aditivo é mais surgical.

## Arquivos

- `packages/core/src/economy/sources/transcript.rs` — refatorar `price_frame`: branch quando `model` é None, branch quando modelo é desconhecido; comentários explicando cada ponto da decisão.
- (sem mudança no schema, no DTO, ou no dashboard — display "—" para 0 continua válido se input+output forem 0 mesmo)

## Tarefas

### Library Agent

- [x] Refatorar `price_frame` em `transcript.rs`:
  - Manter contrato: `Option<i64>` (retornar `None` se input+output forem ambos 0 = caso degenerado de span vazia)
  - Branch A: `model is None` → usar pricing de `claude-sonnet-4-6`, eprintln debug
  - Branch B: `model_pricing_usd_micros_per_million` retornou (0, 0) → mesmo fallback, eprintln com o nome desconhecido
  - Adicionar bloco de comentário no topo da função explicando a política de fallback
  - Comentários inline em cada decisão (porquê de cada branch)
- [x] Atualizar/adicionar teste em `tests` ou inline: `price_frame(None, Some(1000), Some(500), None)` retorna `Some(...)` não-zero (não mais `None`)
- [x] Atualizar teste existente `price_frame_unknown_model_returns_none` (se houver) para refletir o novo contrato — agora unknown model também tem preço pela default

## Critérios de Aceitação

- [x] AC-1: build core verde — Command: `cargo build -p mustard-core`
- [x] AC-2: testes core verdes — Command: `cargo test -p mustard-core --lib`
- [x] AC-3: fallback documentado — Command: `bash -c "grep -q 'fallback' packages/core/src/economy/sources/transcript.rs && echo ok"`
- [x] AC-4: comentário citando default sonnet — Command: `bash -c "grep -q 'claude-sonnet' packages/core/src/economy/sources/transcript.rs && echo ok"`

## Limites

- Não alterar o schema de `run_usage` nem o modelo `ApiCostFrame`
- Não tocar `model_pricing_usd_micros_per_million` (a tabela em si está correta — bug é no consumidor)
- Não mudar o dashboard (display "—" para zero continua útil para spans sem tokens)
- Surgical: só a função `price_frame` + um teste novo
