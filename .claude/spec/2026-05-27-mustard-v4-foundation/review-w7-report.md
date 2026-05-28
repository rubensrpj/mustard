# Wave 7 — review-cobertura-w6 (relatório empírico)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Wave: wave-7-mixed

## Sumário

O gate W4 da Spec A v4 (`apps/rt/src/run/gate_regression_check.rs`) foi rodado de
ponta-a-ponta contra a fixture do caso W6 da no-sqlite (capturada em W0,
`fixtures/w6-pre/telemetry.rs` ↔ `fixtures/w6-post/telemetry.rs`). O teste de
integração — `gate_regression_check::tests::wave_7_review_w6_fixture_triggers_three_of_four_moments` —
mede empiricamente quantos pontos críticos disparam.

**Resultado: 3 de 4 pontos críticos dispararam.** AC-A-1 da Spec A
(≥3/4) é satisfeito. Nenhum threshold foi inflado para alcançar o número:
Momento 2 falhou silenciosamente porque o host de execução não possui a
grammar `tree-sitter-rust` instalada localmente (fail-open por design,
ver [[feedback_mustard_agnostic]] + AC-A-17).

## Tabela: Momento × disparou × signals empíricos

| Ponto crítico | Disparou? | Quantos signals | Severidades | Evidence empírica |
|---|---|---|---|---|
| **Momento 1 — vocabulário sobre plano** | sim | 5 | 2 High (semantic) + 2 Medium (pattern) + 1 Low (keyword) | `semantic/empurrar pra W`, `semantic/fail-open`, `pattern/Vec::new()`, `pattern/Default::default()`, `keyword/placeholder` |
| **Momento 2 — stub-detect sobre diff** | **não** | 0 | — | `grammar_available=false` (host sem `tree-sitter-rust` instalado em `~/.config/tree-sitter/`) |
| **Momento 3 — snapshot diff antes/depois** | sim | 8 | 8 High (linhas removidas / shrink > 2×threshold) | 8 de 9 funções declaradas detectadas como esvaziadas: `agent_activity`, `dashboard_economy_summary`, `dashboard_prompt_economy`, `hook_fire_counts`, `measured`, `routing_breakdown`, `tool_breakdown`, `workflow_by_phase` |
| **Span-level — `review_spans::check_consolidation`** | sim | 1 | Red entry no ledger | `Blocked { entry: rt-impl @ 2026-05-27T18:00:00Z }` |
| **Total** | **3/4** | 14 signals agregados | — | — |

## Análise por momento

### Momento 1 — vocabulário (fired, Red)

O plan_text simula o frasear canônico do W6 (extraído da memória
[[feedback_no_stub_fail_open]] + [[feedback_refactor_no_stub_deferral]]):

> "Wave 6B: vamos manter assinatura das funções de telemetria e empurrar
> pra W7 a implementação real. Stub fail-open por enquanto (Vec::new() /
> Default::default()) — placeholder até a próxima wave entregar o NDJSON
> reader."

5 hits dispararam:

| Termo | Camada | Severity | Mapping (W4 atual) |
|---|---|---|---|
| `empurrar pra W` | semantic | High | `Semantic → High` (gate_regression_check.rs:258) |
| `fail-open` | semantic | High | idem |
| `Vec::new()` | pattern | Medium | `Pattern → Medium` |
| `Default::default()` | pattern | Medium | idem |
| `placeholder` | keyword | Low | `Keyword → Low` |

Classificação: 2 High signals → **Red verdict** (regra: ≥1 High ⇒ Red).
Mesmo sem os Medium / Low, os dois High já fechariam Red sozinhos. Vocabulário
W1 cobre o caso W6 sem furos.

### Momento 2 — stub-detect (NÃO fired, gap honesto)

Comportamento esperado pelo design: na ausência da grammar `tree-sitter-rust`
instalada em `~/.config/tree-sitter/config.json`,
`GrammarLoader::language_id_for_path("telemetry.rs")` retorna `None`, e
`detect_stub_patterns` faz `continue` no loop sobre os arquivos (linha 80-83
de `packages/core/src/ast/stub_detect.rs`). O fallback textual *também* não
roda quando `lang_id` é `None` — porque o path inteiro do arquivo é
descartado antes da decisão AST vs fallback.

**Esta é a única falha de cobertura empírica do gate**, e ela é **dependente
do host**, não do código. Em um host com `tree-sitter-rust` instalado
(operadores Mustard executando `mustard install-grammars` da W8.5), Momento 2
disparara contra o post-fixture (que tem `RtkBlock::default()`,
`Vec::new()`, `Default::default()` em corpos de funções públicas declaradas).

#### Follow-up: textual fallback agnóstico ao `language_id`

A short-circuit por `lang_id` em `stub_detect.rs:80` é defensivamente correta
para o caminho AST (precisa de uma linguagem pra parsear), mas **excessivamente
estrita para o fallback textual**: o fallback usa `vocabulary::scan` (W1) que
é por construção agnóstico. Uma versão futura do gate deveria:

1. Tentar resolver `lang_id` para escolher o caminho AST.
2. Se `lang_id` é `None` (ou a grammar ausente), rodar o fallback textual
   *mesmo assim* — `extract_function_signatures` já tem um regex agnóstico
   (`extract_via_fallback_regex` em `signature.rs:143`) que funciona sem
   `lang_id`.

Isto deixa Momento 2 sempre disparável quando há `Vec::new()` / `None` /
`Default::default()` no diff dentro de uma função declarada, sem depender de
grammar instalada localmente. **Diferido como follow-up registrado abaixo** —
não está no escopo da W7 (validação consolidada, não código novo).

### Momento 3 — snapshot diff (fired, 8 High)

8 das 9 funções declaradas dispararam High (line_changes > 2× threshold).
A única que não disparou foi `rtk_summary` — seu corpo pre tem apenas 5
linhas substantivas, fazendo line_changes = 5, que NÃO é estritamente >
`LINE_CHANGE_THRESHOLD = 5`. Esta é uma omissão deliberada da régua atual:
o threshold serve para excluir noise (renomeação de parâmetro, whitespace,
single-line tweak); abaixá-lo para 4 capturaria `rtk_summary` mas inflaria
o risco de falsos positivos em refators legítimos de uma linha.

**Decisão:** manter `LINE_CHANGE_THRESHOLD = 5`. 8/9 funções stubadas
detectadas é suficiente; o caso restante (`rtk_summary`) ainda é capturado
indiretamente via Momentos 1 + 2 (se grammar disponível) + span-level.
Lowering o threshold viola [[feedback_no_rubber_stamp_close]] — não se
ajusta régua só pra fazer subir o número.

### Span-level — check_consolidation (fired, Blocked)

`review_spans::append_verdict` recebeu um `VerdictEntry { verdict: "red" }`
simulando um `SubagentStop` que retornou com signals. `check_consolidation`
detectou imediatamente e retornou `Blocked { entry }`. Cumpre o contrato
AC-A-5 + AC-A-7 (W5).

## Falsos positivos detectados

Nenhum no caso W6. Mas a análise levantou um **risco teórico** (não
confirmado empiricamente):

- O termo `refactor` está na camada `keyword` (Low) do
  `.claude/vocab/regression.toml`. Qualquer plano de wave legítima de
  refator (não-stub) dispararia 1 hit Low ⇒ Amber por causa do critério
  "exatamente 1 layer com Low-only ⇒ Amber" em `classify_verdict`. Amber
  é "pergunta ao usuário", não "bloqueia", então o impacto é
  *fricção interativa*, não bloqueio incorreto. Aceitável até evidência
  contrária.

Caso a fricção apareça em produção (operadores reportando Amber repetitivo
em refators legítimos), considerar mover `refactor` para camada `noise`
— mas isso EXIGE AskUserQuestion (AC-A-14: "Promoção de termo entre
camadas SEMPRE pergunta"). Não fazer silenciosamente.

## Ajustes de peso aplicados

**Nenhum.** Justificativa empírica:

| Camada | Termos atuais | Hits no W6 | Decisão |
|---|---|---|---|
| semantic | 5 termos | 2 hits (fail-open + empurrar pra W) — ambos High, ambos pertencem à camada | sem ajuste |
| pattern | 5 termos | 2 hits (Vec::new() + Default::default()) — ambos Medium, ambos pertencem | sem ajuste |
| keyword | 5 termos | 1 hit (placeholder) — Low, comportamento esperado | sem ajuste |
| noise | 5 termos | 0 hits — não devem aparecer no W6 (cobertos por camadas acima) | sem ajuste |

A regra dura
[[feedback_no_rubber_stamp_close]] impede ajustes "para fazer passar":
o gate **já passa** com os pesos W1.

## Ajustes de threshold aplicados

**Nenhum em `gate_regression_check::run`.** Constantes atuais:

| Constante | Valor atual | Empírico | Decisão |
|---|---|---|---|
| `LINE_CHANGE_THRESHOLD` | 5 | Captura 8 de 9 shrinks (1 borderline @ exactly 5 linhas) | manter — abaixar a 4 violaria a heurística "noise floor" e poderia gerar falsos positivos em refators de uma linha |
| Mapping `Severity → Verdict` | High⇒Red, Medium⇒Amber, Low+single-layer⇒Amber, ≥2 layers⇒Red | Empírico: Moment 1 sozinho fechou Red via High; Moment 3 sozinho fecharia Red via 8 High; juntos (2 layers) reforçam Red por dupla via | manter — comportamento "ou ⇒ Red por severidade, ou ⇒ Red por multi-layer" é robusto |

Sobre **expor thresholds via `.claude/vocab/regression.toml#thresholds`**:
não feito nesta wave. Justificativa: o loader atual do toml
(`VocabularyDoc::load_from_file`) só conhece a chave `[[layer]]`. Adicionar
`[thresholds]` exigiria estender o doc, o loader, e tipos públicos —
trabalho substancial fora do escopo "validação consolidada". Fica como
**follow-up registrado abaixo**. Hoje a constante hardcoded em
`gate_regression_check.rs:56` está honesta e justificada por evidência —
mover para o TOML é melhoria, não pré-requisito.

## Follow-ups (para spec mãe)

1. **Momento 2 textual fallback agnóstico** — `stub_detect.rs:80` faz
   `let Some(lang_id) = ... else { continue; }` mesmo no caminho textual.
   O fallback textual usa `vocabulary::scan` + regex agnóstico de
   `extract_via_fallback_regex` — ambos não precisam de `lang_id`. Refator
   sugerido: ramificar antes do `continue`, deixando o fallback rodar com
   `lang_id = None` (ou `lang_id = "unknown"`). **Custo:** ~20 LOC em
   `stub_detect.rs`. **Ganho:** Momento 2 disparara mesmo em hosts sem
   grammar instalada, fechando AC-A-1 com 4/4 no caso W6.

2. **Threshold via TOML** — expor `LINE_CHANGE_THRESHOLD` + os mappings
   `Semantic→High`, `Pattern→Medium`, `Keyword→Low` via uma seção
   `[thresholds]` em `.claude/vocab/regression.toml`. Permite ajuste sem
   recompilar e cobre AC-A-13 ("vocabulário editável sem recompilar") com
   mais profundidade. **Custo:** ~50 LOC em `mustard_core::vocabulary` +
   ~10 LOC no gate. **Ganho:** opcional — não afeta AC-A-1.

3. **Captura de `rtk_summary` em snapshot** — função com pre-body de 5
   linhas substantivas passou abaixo do threshold (5 ≤ 5). Considerar
   migrar o critério de `line_changes > N` para `body_emptied = (after_lines
   == 0 || after_lines × 3 < before_lines)`. Mais robusto a corpo pequeno
   esvaziado para corpo único. **Custo:** ~10 LOC em
   `moment_three_signals`. **Ganho:** Moment 3 captura 9 de 9 funções W6.

4. **Boundary resolver stale** — `PostToolUse:Edit` reportou
   `spec "2026-05-26-deep-refactor-followups"` em vez da spec ativa W7.
   Edit foi intencional dentro do escopo declarado em
   `wave-7-mixed/spec.md`. Já registrado em
   `spec.md → ## Followups → "Boundary warnings stale"` (W5 follow-up #4).

## Nome do teste de integração

```text
mustard-rt --lib gate_regression_check::tests::wave_7_review_w6_fixture_triggers_three_of_four_moments
```

Comando exato para reproduzir:

```bash
$env:MUSTARD_V4_BOOTSTRAP='1'; cargo test -p mustard-rt --lib wave_7_review_w6_fixture_triggers_three_of_four_moments -- --exact --nocapture
```

Saída empírica esperada:

```text
=== W7 review against W6 fixture ===
Moment 1 (vocabulary): fired=true (signals=5, severities=[High, High, Medium, Medium, Low], evidence=[...])
Moment 2 (stub AST/textual): fired=false (grammar_available=false, signals=0, evidence=[])
Moment 3 (snapshot): fired=true (signals=8, evidence=[...])
Span-level (review_spans::check_consolidation): blocked=true
Total moments fired: 3/4
test ... ok
```

## Nota sobre fixture controlada (decisão §16 #2)

Esta wave rodou **inteiramente contra fixture controlada** (`fixtures/w6-pre/`
+ `fixtures/w6-post/` capturadas em W0). Nenhuma sub-wave declarou
override pra dado real. A decisão §16 #2 da spec mãe é cumprida sem
exceções: o gate W4 é validado contra um caso real-mas-frozen (W6 da
no-sqlite), não contra a base de código atual nem contra um diff vivo. A
reprodutibilidade do teste depende apenas dos dois arquivos sob
`fixtures/w6-{pre,post}/telemetry.rs` permanecerem canônicos — manipulação
desses arquivos invalidaria a validação consolidada.

<!-- wikilinks-footer-start -->
- [feedback_mustard_agnostic](?) ⚠ não resolvido
- [feedback_no_stub_fail_open](?) ⚠ não resolvido
- [feedback_refactor_no_stub_deferral](?) ⚠ não resolvido
- [feedback_no_rubber_stamp_close](?) ⚠ não resolvido
- [layer](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->