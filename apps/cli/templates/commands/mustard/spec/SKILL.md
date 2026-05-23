---
name: mustard-spec
description: "Use when the user wants to approve a planned spec or continue an in-progress spec — renders a letter-keyed picker over all active specs (Plan/Execute) and routes to approve-only or resume flow."
source: manual
---
<!-- mustard:generated -->
# /mustard:spec — Comando único de spec

## Trigger

`/mustard:spec [letra[r]]`

## Description

Comando único que substitui `/approve` (aprovar spec em estágio PLAN — planejar) e `/resume` (continuar spec em estágio EXEC — executar). Sem argumento, renderiza tabela com TODAS as specs ativas (estágio Plan ou Execute, resultado Active) e fica esperando uma letra de seleção. Letra sozinha = aprovar (se PLAN) ou continuar (se EXEC). Letra + `r` = aprovar + executar inline na mesma sessão (PLAN→EXEC sem trocar de sessão).

A renderização da tabela + legenda de siglas (abreviações) + modo de seleção é **sempre obrigatória**, mesmo quando há só 1 spec ativa, para que o usuário enxergue o catálogo e nenhuma sigla apareça sem tradução.

## Action

### Step 1: AUTO-SYNC (obrigatório)

Antes de qualquer leitura:

```bash
mustard-rt run sync-registry
```

### Step 2: Discovery (varredura de specs ativas)

1. Glob `.claude/spec/*/spec.md` (specs simples) + `.claude/spec/*/wave-plan.md` (planos de wave — onda de execução).
2. Para cada match, leia as primeiras 12 linhas e extraia:
   - `### Stage:` (Analyze, Plan, Execute, QaReview, Close)
   - `### Outcome:` (Active, Completed, Abandoned)
   - `### Scope:` (light, full)
   - `### Parent:` (spec pai — preenchido em tactical-fixes e sub-specs)
3. **Filtros:**
   - Manter apenas `Outcome === "Active"` AND `Stage ∈ {Plan, Execute}`.
   - Pular sub-specs cujo basename do diretório começa com `review/`, `qa/` ou `wave-N-` (essas são detalhes internos do plano de wave; mostre apenas a raiz `{name}/`).
4. Para cada wave plan, rode `mustard-rt run event-projections --view pipeline-state --spec {name}` e capture `completedWaves.length / totalWaves` + `failedWaves[]`.
5. Para cada spec, extraia o Resumo: primeira frase da seção `## Resumo`, ou — se não existir — primeira frase de `## Contexto`, truncada em ≤70 caracteres.

### Step 3: Render (sempre — mesmo com 1 spec)

Imprima os TRÊS blocos juntos, na ordem abaixo, ainda que só exista uma spec na lista. **Não auto-selecionar.**

#### Tabela

```text
| #  | Spec                                          | Esc | Estágio | Prog | Status            | Resumo                                              |
|----|-----------------------------------------------|-----|---------|------|-------------------|-----------------------------------------------------|
| a  | 2026-05-23-dashboard-design-system            | fl  | EXEC    | 4/6  | W5 em exec        | Refit visual completo do dashboard                  |
| b  | 2026-05-23-tf-unify-spec-command              | lt  | EXEC    |  -   | TF→ds             | Unifica /approve + /resume em /mustard:spec        |
| c  | 2026-05-22-project-profiler                   | fl  | PLAN    | 0/3  |  -                | Profiler de projeto por wave                        |
```

Use UMA letra por linha começando em `a`. Coluna `#` = letra de seleção (alvo do comando `/mustard:spec X`).

#### Siglas (legenda — sempre renderizar todas as siglas usadas na tabela)

Bloco obrigatório toda vez que a tabela for impressa, sem exceção.

- **Headers de coluna:**
  - `#` = letra de seleção (a-z)
  - `Esc` = Escopo (tamanho da spec)
  - `Prog` = Progresso (em planos de wave)
- **Estágio (Stage):**
  - `PLAN` = planejar (spec escrita, aguarda aprovação)
  - `EXEC` = executar (em execução pelos agentes)
  - `CLOS` = fechar (filtrado da listagem — só mostra Plan/Execute)
- **Escopo (Scope):**
  - `lt` = light (≤5 arquivos, padrão conhecido)
  - `fl` = full (>5 arquivos ou plano de wave)
- **Progresso (Prog):**
  - `X/Y` = X waves completas de Y totais
  - `-` = não aplicável (spec simples, sem waves)
- **Status (coluna Status):**
  - `TF` = Tactical Fix (correção tática derivada de outra spec; setinha aponta para o parent)
  - `TF→{parent}` = TF cujo parent é `{parent}` (alias curto; ver bloco "Parents referenciados" abaixo se houver truncamento)
  - `W{N}` = Wave N (onda N do plano)
  - `BLOCK` = bloqueada (ver wave failures no plano)
  - `em exec` = wave já despachada / agentes rodando
  - `-` = sem flag relevante
- **Parents referenciados (quando houver TF→alias):** Listar em sub-lista o mapeamento alias → nome completo (ex.: `ds = 2026-05-23-dashboard-design-system`). Só imprimir se a tabela mostrou pelo menos um `TF→alias` abreviado.
- **Inline no Resumo:** Caso a frase do Resumo contenha sigla específica (`AC` = Acceptance Criteria — critério de aceitação, `AC-W{N}.M` = AC M da Wave N, `BLOCKED` etc.), incluir a expansão também na legenda.

#### Modo de seleção

- `a-z` (uma letra sozinha) → agir sobre a spec daquela linha:
  - Se estiver em `PLAN` → aprovar e parar (a sessão atual termina; abra outra e rode `/mustard:spec {letra}` para executar).
  - Se estiver em `EXEC` → continuar de onde parou (mesma sessão).
- `a-z + r` (letra seguida da letra `r`, ex.: `ar`, `br`) → aprovar **e** executar inline (PLAN → EXEC sem trocar de sessão). Para spec já em EXEC, o `r` é ignorado (comportamento igual a só a letra).
- Qualquer outra entrada → mensagem de erro + re-renderizar a tabela inteira (com siglas + modo).

### Step 4: Auditoria obrigatória de siglas (antes de imprimir)

A renderização do bloco "Siglas" é obrigatória em **toda** invocação — independente de número de specs, contexto, ou se as siglas parecem óbvias. Política `feedback_siglas_always_with_legend`.

Antes de imprimir a tabela, varra cada célula e cada header procurando abreviações. Para cada sigla encontrada, garanta que ela tem entrada correspondente no bloco "Siglas". Regra:

- Toda sigla nova que aparecer **e** não estiver coberta deve, ou (a) ser substituída pela forma estendida, ou (b) ser adicionada à legenda.
- Nunca imprimir sigla sem legenda correspondente.

Cobertura mínima esperada (caso a sigla apareça): `#`, `Esc`, `Prog`, `PLAN`, `EXEC`, `CLOS`, `lt`, `fl`, `TF`, `W{N}`, `BLOCK`, `em exec`, `AC`, `AC-W{N}.M`, `BLOCKED`. Truncamentos de nome de parent (ex.: `ds` por `dashboard-design-system`) vão no sub-bloco "Parents referenciados".

### Step 5: Stop & Aguardar input do usuário

Após imprimir os três blocos, **parar**. Não auto-selecionar nem em 1-spec. O próximo turno do usuário traz a letra escolhida.

### Step 6: Parsing do input

O usuário responde com algo como `a`, `br`, `c`, `dr`. Regras:

- `^[a-z]$` → modo `act-only` (aprovar ou continuar conforme estágio).
- `^[a-z]r$` → modo `act+execute` (aprovar inline + executar).
- Qualquer outra coisa → imprimir `Letra inválida. Use a-z (ex.: a) ou letra+r (ex.: ar) para aprovar e executar inline.` e re-renderizar a tabela completa.

### Step 7: Roteamento por estágio + sufixo

Após parseado o input, identifique a spec selecionada pela letra. Depois:

| Estágio detectado | Sufixo | Fluxo aplicado                                                            |
|-------------------|--------|---------------------------------------------------------------------------|
| PLAN              | nenhum | Fluxo "approve sem `--resume`" — ver `../../../refs/spec/approve-only-flow.md` |
| PLAN              | `r`    | Fluxo "approve + execute inline" — ver `../../../refs/spec/approve-only-flow.md` § Branch `--resume` |
| EXEC              | nenhum | Fluxo "continuar pipeline" — ver `../../../refs/spec/resume-flow.md`         |
| EXEC              | `r`    | Igual a EXEC sem sufixo; imprimir inline `Spec já em EXEC — sufixo r ignorado.` e continuar |

**Resumo do que cada fluxo faz:**

- **approve-only-flow** (`refs/spec/approve-only-flow.md`): detecta wave plan, atualiza header da spec para `Stage: Plan` + `Outcome: Active` + Checkpoint atual, emite `pipeline.stage` + `pipeline.status` no SQLite (banco de eventos), seleciona modelo, cria TaskCreate por agente. Sem sufixo `r`, para e instrui `Abra nova sessão e rode /mustard:spec {letra}`. Com `r`, salta para o **Step 2: Bootstrap** do fluxo de resume.
- **resume-flow** (`refs/spec/resume-flow.md`): pré-checa falha de dispatch (≤10min re-despacha), escolhe modo `continued` vs `reanalyzed`, expande stub de wave N≥2 se necessário, atualiza header para `Stage: Execute`, emite stage transition, monta prompts dos agentes via template, despacha em uma única mensagem por wave, roda VALIDATE → REVIEW → QA → CLOSE.

Os refs contêm a sequência completa de passos, eventos a emitir e regras invioláveis dos fluxos legados, MOVIDA verbatim das antigas `commands/mustard/approve/SKILL.md` e `commands/mustard/resume/SKILL.md` (deletadas).

### Step 8: Edge cases

- **0 specs ativas** → imprimir `Nenhuma spec ativa. Crie uma via /mustard:feature ou /mustard:bugfix.` e parar (não imprimir tabela vazia).
- **>26 specs ativas** (improvável) → mostrar as 26 primeiras letradas `a-z` + nota: `(N specs adicionais — refine via /mustard:status para detalhes)`. Sem paginação real nesta TF.
- **Letra inválida** → erro inline + re-render completo (tabela + siglas + modo).
- **Spec EXEC com sufixo `r`** → comportamento idêntico a só letra; informar inline `Spec já em EXEC; sufixo r ignorado.` e continuar.
- **`/mustard:spec` com argumento direto pré-seleção** (`/mustard:spec ar`) → pular Step 3 e Step 5 (sem re-render), ir direto ao Step 6 (parsing). A tabela ainda deve ser impressa rapidamente para que o usuário veja qual spec foi selecionada antes de seguir.

## INVIOLABLE RULES

- Renderização da tabela + bloco Siglas + bloco Modo de seleção é **obrigatória** em toda invocação; nunca auto-skipar — nem em 1-spec, nem em re-render por erro.
- Nenhuma sigla pode aparecer sem entrada correspondente no bloco Siglas (Auditoria do Step 4).
- O picker NÃO mora no `mustard-rt`; é texto + LLM. O binário é chamado apenas para sync-registry, event-projections e emissão de eventos.
- `/mustard:spec` substitui completamente `/approve` e `/resume`; ambos foram deletados (sem alias). Tentar invocá-los retorna "comando não encontrado".
- Fluxos pós-seleção (approve-only / resume) seguem idênticos aos SKILLs antigos — moveram-se para `refs/spec/*.md`, não foram re-escritos.

## Alternative Flow

Se o usuário não quer agir sobre nenhuma spec listada:
- Para descartar uma spec: `/mustard:close` (cancela o pipeline, marca outcome=Abandoned).
- Para criar nova spec: `/mustard:feature` ou `/mustard:bugfix`.
- Para inspecionar uma spec sem agir: `/mustard:status` (resumo) ou abrir `.claude/spec/{name}/spec.md` direto.
