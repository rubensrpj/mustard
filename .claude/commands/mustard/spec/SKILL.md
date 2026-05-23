---
name: mustard-spec
description: "Use when the user wants to approve a planned spec or continue an in-progress spec — delegates discovery to mustard-rt run active-specs, prints the pre-rendered table, then routes the user's letter selection to approve-only or resume flow."
source: manual
---
<!-- mustard:generated -->
# /mustard:spec — Comando único de spec

## Trigger

`/mustard:spec [letra[r]]`

## Description

Comando único que substitui `/approve` (aprovar spec em estágio PLAN — planejar) e `/resume` (continuar spec em estágio EXEC — executar). Sem argumento, renderiza tabela com TODAS as specs ativas (estágio Plan ou Execute, resultado Active) e fica esperando uma letra de seleção. Letra sozinha = aprovar (se PLAN) ou continuar (se EXEC). Letra + `r` = aprovar + executar inline na mesma sessão (PLAN→EXEC sem trocar de sessão).

O `mustard-rt run active-specs` lê o filesystem como fonte de verdade e devolve a tabela pronta — o LLM só imprime + roteia a escolha do usuário. A renderização da tabela + legenda de siglas (abreviações) + modo de seleção é **sempre obrigatória**, mesmo quando há só 1 spec ativa, para que o usuário enxergue o catálogo e nenhuma sigla apareça sem tradução.

## Action

### Step 1: AUTO-SYNC (obrigatório)

Antes de qualquer leitura:

```bash
mustard-rt run sync-registry
```

### Step 2: Descobrir + renderizar (delegado ao mustard-rt)

`mustard-rt run active-specs --format table` faz toda a descoberta nativa: glob filesystem, parse de header, filtro Plan/Execute/Active, contagem de waves, extração de resumo, resolução de aliases de parent, e backfill SQLite (para specs que vieram via git pull e não têm eventos no event-store local). Devolve a tabela markdown já pronta.

```bash
rtk mustard-rt run active-specs --format table
```

Imprima a saída do comando verbatim — é a tabela pronta. Depois imprima os blocos estáticos abaixo (Siglas + Modo de seleção).

---

**Siglas (legenda — todas as siglas usadas acima)**

- Headers de coluna:
  - `#` = letra de seleção (a-z)
  - `Esc` = Escopo (tamanho da spec)
  - `Prog` = Progresso (waves completas / totais, só em planos de wave)
- Estágio (Stage — fase do pipeline):
  - `PLAN` = planejar — spec escrita, aguardando aprovação para executar
  - `EXEC` = executar — em execução pelos agentes
- Escopo (Scope — tamanho da spec):
  - `lt` = light — ≤5 arquivos, padrão conhecido
  - `fl` = full — >5 arquivos ou plano de wave (onda de execução)
  - `-` = não declarado no header
- Progresso (Prog):
  - `X/Y` = X waves (ondas) completas de Y totais
  - `-` = não aplicável (spec simples sem waves)
- Status:
  - `TF` = Tactical Fix — correção tática derivada de outra spec
  - `TF→{alias}` = TF cujo parent (spec pai) é `{alias}` (ver "Parents referenciados" no final da tabela quando houver)
  - `W{N}` = Wave (onda) N
  - `BLOCK` = wave bloqueada — depende de algo externo
  - `em exec` = wave já despachada / agentes rodando
  - `-` = sem flag relevante

(Se a tabela tiver linhas com `TF→{alias}`, o `mustard-rt` inclui no final do output a lista `Parents referenciados:` mapeando cada alias para o slug completo do parent.)

**Modo de seleção**

- `a-z` (uma letra sozinha) — agir sobre aquela linha:
  - Se estiver em **PLAN** → aprova e para (você abre outra sessão e roda `/mustard:spec {letra}` para começar a executar).
  - Se estiver em **EXEC** → continua de onde parou, na mesma sessão.
- `a-z + r` (letra seguida de `r`, ex.: `ar`) — aprova **e** executa inline (PLAN → EXEC sem trocar de sessão). Se a spec já estiver em EXEC, o `r` é ignorado.
- Outra entrada → erro + re-render da tabela.

---

A tabela vem pré-renderizada do `mustard-rt`; siglas vêm sempre acompanhadas da legenda estática acima. Auditoria não é mais necessária.

### Step 3: Stop & Aguardar input do usuário

Após imprimir os três blocos, **parar**. Não auto-selecionar nem em 1-spec. O próximo turno do usuário traz a letra escolhida.

### Step 4: Parsing do input

O usuário responde com algo como `a`, `br`, `c`, `dr`. Regras:

- `^[a-z]$` → modo `act-only` (aprovar ou continuar conforme estágio).
- `^[a-z]r$` → modo `act+execute` (aprovar inline + executar).
- Qualquer outra coisa → imprimir `Letra inválida. Use a-z (ex.: a) ou letra+r (ex.: ar) para aprovar e executar inline.` e re-renderizar a tabela completa.

### Step 5: Roteamento por estágio + sufixo (via `resume-bootstrap`)

Após parseado o input, identifique a spec selecionada pela letra (`{specName}`). Em UM único passo, chame o bootstrapper que consolida todo o trabalho que antes era 4-5 comandos sequenciais (event-projections, wave-tree, stub-detect, diff/slice decision, model lookup, resume_mode emit):

```bash
rtk mustard-rt run resume-bootstrap --spec {specName} --json
```

Parse o JSON retornado. Os campos relevantes para rotear:

- `stage` — `Plan` | `Execute` | `Analyze` | `QaReview` | `Close`
- `mode` — `continued` | `reanalyzed` | `ask` (auto-decidido pelo binário; `ask` só quando env `MUSTARD_RESUME_MODE=ask`)
- `operationalSpecPath` — caminho da spec da wave atual (já resolvido entre `spec.md` raiz e `wave-N-{role}/spec.md`)
- `isWavePlan`, `currentWave`, `totalWaves`, `isStub`, `waveModel`
- `lastDispatchFailure` — não-nulo só se houver falha fresca (≤10min); re-despache com o `prompt` armazenado
- `needsDiff`, `needsContextSlice` — apenas `true` se uma wave completou desde o último resume (orquestrador roda `diff-context` / `context-slice` só nesses casos)

Roteie com base em `stage` + sufixo:

| `stage` retornado | Sufixo | Fluxo aplicado |
|-------------------|--------|----------------|
| `Plan`            | nenhum | Fluxo "approve sem `--resume`" — ver `../../../refs/spec/approve-only-flow.md`. Não chamar `resume-bootstrap` denovo (ele só roda em EXEC). |
| `Plan`            | `r`    | Fluxo "approve + execute inline" — `approve-only-flow.md § Branch --resume`. Após aprovar e gravar header `Stage: Execute`, reinvocar `resume-bootstrap` para pegar o snapshot fresco e cair no branch EXEC abaixo. |
| `Execute`         | (qualquer) | Branch EXECUTE abaixo. Sufixo `r` em spec EXEC é no-op — imprima `Spec já em EXEC — sufixo r ignorado.` e siga. |
| `Analyze` / `QaReview` / `Close` | (qualquer) | Branch EXECUTE abaixo (o ref `resume-flow.md § Escalation Statuses` cuida das transições). |

#### Branch EXECUTE — dispatch via `agent-prompt-render`

Para cada agente da wave atual, NÃO monte o prompt manualmente. Delegue a renderização ao binário, que lê o template embedded (`apps/rt/src/run/agent_prompt_template.md`), expande todos os `{placeholders}` (guards, entity, recipe, skills, cached diff, context-slice, cross-wave memory, retry context) e devolve a string final pronta para a Task tool:

```bash
rtk mustard-rt run agent-prompt-render \
  --spec {specName} \
  --wave {currentWave} \
  --role {ui|backend|database|review|...} \
  --subproject {subproject_path} \
  [--mode first|granular|fix-loop] \
  [--retry-context-file .claude/.pipeline-states/{specName}.retry-{agent}.md]
```

Capture stdout e passe **verbatim** como `prompt` da Task tool. Stderr lista placeholders deixados em branco (degrade graceful — não bloqueia). TODOS os agentes da mesma wave em uma única mensagem (múltiplas invocações Task na mesma message), `model` vindo de `waveModel` do JSON do bootstrap.

Para regras pós-dispatch (wave transitions, VALIDATE → REVIEW → QA → CLOSE, escalation statuses, wave failure handling, dependency precheck) ver `../../../refs/spec/resume-flow.md` — agora reduzido a invariantes + tabela de estados, sem repetir a lógica que o `resume-bootstrap` já encapsula.

### Step 6: Edge cases

- **0 specs ativas** → imprimir `Nenhuma spec ativa. Crie uma via /mustard:feature ou /mustard:bugfix.` e parar (não imprimir tabela vazia).
- **>26 specs ativas** (improvável) → mostrar as 26 primeiras letradas `a-z` + nota: `(N specs adicionais — refine via /mustard:status para detalhes)`. Sem paginação real nesta TF.
- **Letra inválida** → erro inline + re-render completo (tabela + siglas + modo).
- **Spec EXEC com sufixo `r`** → comportamento idêntico a só letra; informar inline `Spec já em EXEC; sufixo r ignorado.` e continuar.
- **`/mustard:spec` com argumento direto pré-seleção** (`/mustard:spec ar`) → pular Step 2 e Step 3 (sem re-render), ir direto ao Step 4 (parsing). A tabela ainda deve ser impressa rapidamente para que o usuário veja qual spec foi selecionada antes de seguir.

## INVIOLABLE RULES

- Renderização da tabela + bloco Siglas + bloco Modo de seleção é **obrigatória** em toda invocação; nunca auto-skipar — nem em 1-spec, nem em re-render por erro.
- Os blocos Siglas + Modo de seleção são literais — copie-os verbatim após a saída de `active-specs`. Não regenerar dinamicamente.
- O picker NÃO mora no `mustard-rt`; é texto + LLM. O binário é chamado para sync-registry, active-specs (descoberta + backfill SQLite), `resume-bootstrap` (decisão de modo + caminho operacional + needsDiff/Slice), `agent-prompt-render` (montagem do prompt final), e emissão de eventos.
- `/mustard:spec` substitui completamente `/approve` e `/resume`; ambos foram deletados (sem alias). Tentar invocá-los retorna "comando não encontrado".
- **NUNCA construir prompt de agente à mão** — sempre via `agent-prompt-render`. O template literal não vive mais em `refs/agent-prompt/agent-prompt.md`; está embedded no binário.
- **NUNCA reimplementar a decisão de `continued` vs `reanalyzed` no orquestrador** — `resume-bootstrap` é a fonte de verdade. Se o JSON disser `mode: ask`, então e só então AskUserQuestion.

## Alternative Flow

Se o usuário não quer agir sobre nenhuma spec listada:
- Para descartar uma spec: `/mustard:close` (cancela o pipeline, marca outcome=Abandoned).
- Para criar nova spec: `/mustard:feature` ou `/mustard:bugfix`.
- Para inspecionar uma spec sem agir: `/mustard:status` (resumo) ou abrir `.claude/spec/{name}/spec.md` direto.
