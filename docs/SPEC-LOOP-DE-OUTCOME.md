# Spec — Loop de outcome (a keystone)

> Status: PROPOSTA (aguardando aprovação). É a peça que falta do Bloco A do roadmap
> ([[project-mustard-architecture-roadmap]]): o `lexicon-enrich` PREENCHE o glossário, mas não há
> MEDIDA de se o locator está bom. Sem medida, o redesenho do locator não tem critério de parada —
> a fonte nº1 da churn (#6).

## Problema, em uma frase

Depois de uma busca (`mustard-rt run feature`), **o orquestrador abriu mesmo os arquivos que o digest
apontou?** Hoje ninguém registra isso. Logo: (a) não dá pra saber quando o locator está "bom o
bastante" (você redesenha pra sempre), e (b) não há sinal pra rebaixar palavra genérica que nunca
leva a leitura (o resíduo `novo/janela`, que IDF e stoplist não pegam).

## Invariante

**Observer-only.** O gancho é telemetria pura: roda fire-and-forget, NUNCA bloqueia, fail-open (o
contrato de Observer do `apps/rt`). A métrica é uma **projeção determinística** sobre o NDJSON. O
digest segue determinístico; a IA não entra aqui. É a tese "dado-não-código": o USO produz DADO
(eventos), uma dobra determinística vira MÉTRICA.

## Design — 3 ondas: SINAL → MÉTRICA → FEEDBACK (condicional)

### Onda 1 — o SINAL (gancho fino)

- Um Observer `PostToolUse(Read|Edit|Write)` emite `feature.touch {file}` — o arquivo tocado + a
  atribuição de sessão/spec que os eventos já carregam. **Reúso:** `file_path_of` + o padrão de
  Observer do `hooks/write/post_edit.rs` (que já extrai o arquivo de Edit e correlaciona com a spec
  ativa via `find_active_spec`); o `tool_use_counter`/`main_context_counter` já roda em
  `PostToolUse(Read|Edit|Write)` — **verificar na Onda 1 se ele já carrega o `file_path` (reusar) ou
  só conta (estender)**. NUNCA dá veredito (Read sempre prossegue).
- O evento `feature.query` JÁ carrega as âncoras (os `files` por termo) — o conjunto SUGERIDO. Sem
  mudança aqui.

### Onda 2 — a MÉTRICA (a projeção; o valor principal)

- Uma projeção determinística `mustard-rt run digest-precision` (ou dobrada no `/stats`) sobre
  `feature.query` (âncoras) × `feature.touch` (leituras), correlacionadas por **sessão + janela de
  tempo** (leitura entre a query Q e a Q+1 pertence a Q — sem precisar de ID na query). Calcula:
  - **recall**: das âncoras sugeridas, quantas foram lidas? (o digest apontou certo?)
  - **precisão**: dos arquivos lidos, quantos foram sugeridos? (você teve que ir além do digest?)
  - **por-termo**: quais termos de query levaram a leitura (alto valor) vs nunca (baixo valor).
- **Superfície:** JSON pro dashboard (um painel "precisão do locator") + um número que o orquestrador
  vê. **É o CRITÉRIO DE PARADA:** plota ao longo do tempo; quando achata, pare de redesenhar o
  locator. **Reúso:** o padrão de projeção do core (`core-projection-pattern`) + o canal NDJSON.

### Onda 3 — o FEEDBACK (CONDICIONAL — só se a Onda 2 mostrar que precisa)

- SE a precisão-por-termo mostrar termo cronicamente-não-lido poluindo as âncoras, escrever um overlay
  `term-precision` (`.claude/term-precision.toml`, MESMO padrão do overlay de léxico) que o digest lê
  pra **rebaixar** esses termos no score IDF da âncora. Derivado do USO, sem lista — o conserto
  agnóstico do `novo/janela`.
- **GATED na Onda 2:** não construir especulativamente. Medir primeiro; só fechar o loop se o número
  disser que o resíduo importa. (Disciplina anti-over-build — o próprio ponto #6.)

## Reúso (não reinventar)

| Peça | Origem | Uso |
|---|---|---|
| `file_path_of`, padrão Observer, `find_active_spec` | `hooks/write/post_edit.rs` | extrair o arquivo + atribuir à spec/sessão ativa |
| `tool_use_counter`/`main_context_counter` | `hooks/task/` | já roda em PostToolUse(Read/Edit/Write) — estender p/ emitir o arquivo |
| evento `feature.query` (âncoras por termo) | `commands/feature.rs` | o conjunto SUGERIDO, já emitido |
| projeção determinística + canal NDJSON | core `projection` + `shared/events` | dobrar query×touch → métrica byte-estável |
| overlay que o digest lê (`.claude/lexicons/`) | `scan/matching.rs`, `lexicon_suggest` | molde do `term-precision.toml` da Onda 3 |

## Acceptance Criteria (cada um com comando/cenário)

- **AC-1** após um `feature.query` + um `Read` de uma de suas âncoras, a projeção marca aquela âncora
  como "lida" (hit).
- **AC-2** a projeção é dobra determinística: mesmos eventos → mesmos números (byte-estável).
- **AC-3** um `Read` de arquivo NÃO-âncora é contado como leitura-não-sugerida (precisão < 1).
- **AC-4** o Observer é fail-open e nunca bloqueia (um Read sempre prossegue, mesmo com erro de IO).
- **AC-5 (Onda 3, se construída)** termo com precisão cronicamente-zero é rebaixado e as âncoras do
  digest mudam de acordo (e2e: semear eventos → projeção → overlay → digest).

## Fora de escopo / decisões

- **Onda 1:** reusar um evento de tool existente vs adicionar um observer fino — resolver lendo o
  `tool_use_counter` na Onda 1 (não assumir agora).
- **Onda 3 é CONDICIONAL** — não construir o feedback antes de a Onda 2 mostrar que o resíduo importa.
- IA no caminho (digest/projeção continuam determinísticos); reescrever o dispatch (#4) é outra spec.
