# Mustard — Comandos e Fluxos

Referência visual de **cada comando do Mustard** e seu fluxo de execução.
Os diagramas usam [Mermaid](https://mermaid.js.org/) — renderizam direto no GitHub, no VS Code (com extensão Mermaid) e no dashboard.

> **Convenções dos diagramas**
> - **AI** = passo de raciocínio que o orquestrador (Claude) faz.
> - **rust** = trabalho determinístico delegado ao binário `mustard-rt` (sem AI).
> - **Task** = subagente despachado em contexto isolado.
> - **gate** = portão bloqueante (só passa se a condição for satisfeita).
> - Termos técnicos (nomes de comandos, fases, eventos, arquivos) ficam no original.

Instalado como plugin do Claude Code, todo comando vive no namespace **`/mustard:`**. A entrada do dia a dia é a **porta única** (`/mustard`, ou simplesmente descrever o pedido em linguagem natural).

> **Fluxos internos:** `feature`, `bugfix`, `task` e `tactical-fix` são despachados pelo **roteador** (a porta única) — você descreve o que quer e ele escolhe o fluxo. Invocá-los direto (`/mustard:feature …`) continua valendo como atalho de força; não é necessário no dia a dia.

---

## Mapa do ecossistema

Como os comandos se encaixam. Tudo entra pela **porta única**, nasce de uma varredura determinística (`/mustard:scan`) e converge para o fechamento auditável (`/mustard:close`).

```mermaid
flowchart TD
    door["/mustard — porta única<br/>(classifica a intenção e roteia)"] -->|"feature (≥2 camadas / entidade nova)"| feat["/mustard:feature"]
    door -->|"erro / quebrado"| bug["/mustard:bugfix"]
    door -->|"1 camada / análise"| task["/mustard:task<br/>(delegação spec-less)"]

    scan["/mustard:scan<br/>(rust, sem AI)"] -->|grain.model.json| feat
    scan -->|grain.model.json| bug

    feat -->|spec.md + meta.json| spec["/mustard:spec<br/>(aprova / retoma)"]
    bug -->|"fast path: inline"| exec
    bug -->|"full path: spec"| spec

    spec -->|EXECUTE| exec["EXECUTE<br/>(Task: agentes por onda)"]
    exec --> review["/mustard:review"]
    review --> qa["/mustard:qa"]
    qa -->|"gate: pass"| close["/mustard:close"]

    review -. candidato .-> tf["/mustard:tactical-fix<br/>(sub-spec ligada ao pai)"]
    qa -. candidato .-> tf
    tf --> spec

    close -->|se código mudou| scan

    subgraph apoio["Apoio / fora do pipeline"]
        git["/mustard:git"]
        maint["/mustard:maint"]
        status["/mustard:status"]
        stats["/mustard:stats"]
        knowledge["/mustard:knowledge"]
        skills["/mustard:skills"]
        unhook["/mustard:unhook"]
        rehook["/mustard:rehook"]
    end
```

**Princípio central:** o código-fonte **nunca é lido em massa**. O `/mustard:scan` minera o repositório uma vez para `grain.model.json`; os fluxos de pipeline consomem esse modelo via *digest* (`mustard-rt run feature`) e leem apenas as *anchors* (arquivos-âncora) que o digest aponta. É assim que o Mustard economiza contexto.

---

## Pipeline canônico

Vocabulário único de fases (fonte: `plugin/refs/canonical-phases.md`):

```mermaid
flowchart LR
    A["ANALYZE"] --> P["PLAN"]
    P -->|"/mustard:spec aprova"| E["EXECUTE"]
    E --> R["REVIEW"]
    R --> Q["QA"]
    Q -->|"gate: pass"| C["CLOSE"]
```

Sequência canônica: `ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE` (+ `COORDINATE` para roadmaps com specs-filhas).

| Escopo | Orientação | Fluxo |
|---|---|---|
| **Light** | 1-2 camadas, ≤5 arquivos, espelha um *slice* existente | Pula o PLAN: `ANALYZE → EXECUTE → REVIEW → QA → CLOSE` |
| **Extended-light** | *slice* casado + modifica existente, 6-8 arquivos | Igual ao Light (execução inline) |
| **Full** | 3+ camadas, entidade nova, ≥2 slices ou >8 arquivos | Completo, com **clarify + aprovação humana** entre PLAN e EXECUTE (via `/mustard:spec`) |

O escopo é decidido **deterministicamente** (`plan-prepare` sobre o censo da spec), nunca só pelo olho da AI. Cada fase emite eventos; os *gates* bloqueiam o avanço. O **close-gate** não deixa fechar sem `qa.result.overall=pass`; editar a spec depois de um QA aprovado marca o pass como *stale* e re-bloqueia até o QA rodar de novo.

---

# A porta única

## `/mustard` — Roteamento por intenção

Descreva o que quer em linguagem natural — o roteador classifica (funcionalidade / mudança / correção / investigação + escopo), **narra como leu o pedido** e despacha o fluxo interno certo. Só pergunta em ambiguidade genuína.

| | |
|---|---|
| **Trigger** | `/mustard` — ou simplesmente descreva o trabalho ("adiciona importação de CSV", "tá com erro ao importar") |
| **Backend** | nenhum — roteia via `CLAUDE.md § Intent Routing` |
| **Regra** | Nunca edita produção sem rotear; `/mustard:feature`, `/mustard:bugfix`, `/mustard:task`, `/mustard:tactical-fix` seguem disponíveis como atalhos de força |

```mermaid
flowchart TD
    start(["pedido em linguagem natural<br/>(ou /mustard)"]) --> desc{"descreveu trabalho?"}
    desc -->|não| help["página de ajuda"]
    desc -->|sim| classify["AI: classifica intenção + escopo<br/>e NARRA a leitura"]
    classify --> amb{"ambiguidade genuína?"}
    amb -->|sim| ask["UMA AskUserQuestion<br/>(opções inferíveis)"]
    amb -->|não| route
    ask --> route{"intenção?"}
    route -->|"criar / implementar<br/>≥2 camadas ou entidade nova"| f["/mustard:feature"]
    route -->|"erro / bug / quebrado"| b["/mustard:bugfix"]
    route -->|"melhorar 1 camada ·<br/>analisar / auditar"| t["/mustard:task"]
    route -->|"ajuste pequeno ligado<br/>a uma spec-pai"| tfx["/mustard:tactical-fix"]
```

---

# Comandos do pipeline (core)

## `/mustard:scan` — Modelo do código-base

Minera o repositório para `grain.model.json` (determinístico, agnóstico de linguagem, **sem AI**) e enriquece os mapas por subprojeto — Guards (prosa do/don't) e moldes de padrão. O enriquecimento é **padrão**: roda em silêncio ou pula em silêncio (fail-open), **nunca** pede confirmação de custo.

| | |
|---|---|
| **Trigger** | `/mustard:scan [--root <dir>] [--out <path>]` |
| **Backend** | `scan --full` · `scan-guards-list/apply` · `scan-patterns-sweep/list/apply/decline` · `agent-prompt-render --role guards` |
| **Produz** | `.claude/grain.model.json` · `.claude/scan-map.md` por unidade (+ a linha `@.claude/scan-map.md` no topo do `CLAUDE.md` do projeto) · blocos `## Guards` · moldes `{role}-pattern/SKILL.md` frescos |
| **Regra** | O passo determinístico nunca lê fonte; a AI do enriquecimento escreve SÓ Guards (~6 linhas) e moldes — todo molde `source: scan` é varrido e re-autorado do zero a cada scan (adoção = `source: manual`); recusa vale UMA rodada |

```mermaid
flowchart TD
    start(["/mustard:scan"]) --> full["mustard-rt run scan --full<br/>(rust — sem AI, sem ler fonte)"]
    full --> model[("grain.model.json<br/>+ .claude/scan-map.md por unidade<br/>(CLAUDE.md do projeto: só a linha @import;<br/>## Guards preservados)")]

    subgraph enrich["Enriquecimento padrão (fail-open)"]
        model --> sw["scan-patterns-sweep<br/>(apaga moldes source:scan +<br/>ledger de recusas — tudo fresco)"]
        sw --> gl["scan-guards-list<br/>(subprojetos com Guards pending)"]
        gl --> gag["Task: 1 agente mustard-guards<br/>por subprojeto (read-only, 1 msg)"]
        gag --> gap["scan-guards-apply (stdin)<br/>~6 linhas do/don't"]
        gap --> pl["scan-patterns-list<br/>(clusters de role ≥3, sem teto)"]
        pl --> pag["Task: 1 agente mustard-patterns<br/>por subprojeto (read-only, 1 msg)"]
        pag --> pap["scan-patterns-apply<br/>(create-only, atômico, etiqueta EN)"]
        pag --> pd["scan-patterns-decline<br/>(recusa registrada — vale 1 rodada)"]
    end

    pap --> done(["consumido por /mustard:feature e<br/>/mustard:bugfix via digest"])
    pd --> done
```

> Um Guard pode abrir com `[critical]` na forma checável `never <proibido> in <glob>` — vira gate de edição (`MUSTARD_GUARD_GATE_MODE=strict|warn`, default `warn`). Guards sem marca são consultivos.

---

## `/mustard:feature` — Pipeline de feature *(fluxo interno)*

Entende o pedido, pesquisa o repositório via *digest* do scan (nunca lendo fonte à mão), roteia o escopo deterministicamente e implementa. Este fluxo é o caminho Light + ANALYZE compartilhado; a maquinaria de PLAN do escopo Full vive em `refs/feature/full-plan.md`.

| | |
|---|---|
| **Despacho** | pelo roteador; atalho: `/mustard:feature <request>` |
| **Fases** | `ANALYZE → (rota/escopo) → PLAN (só Full) → EXECUTE → REVIEW → QA → CLOSE` |
| **Backend** | `feature` (digest) · `spec-draft` · `plan-prepare` · `analyze-validation` · `emit-pipeline`/`emit-phase` · `exec-rewave-check` · `dependency-precheck` · `agent-prompt-render` · `qa-run` |
| **Lei** | Nenhum código antes da spec aprovada (o hook `scope_guard` recusa de qualquer forma); Full para no PLAN — só `/mustard:spec` destrava o EXECUTE |

```mermaid
flowchart TD
    start(["router despacha feature"]) --> hyg["spec-hygiene (audita specs velhas)"]
    hyg --> fresh{"grain.model.json fresco?"}
    fresh -->|não| sc["mustard-rt run scan"]
    fresh -->|sim| lap
    sc --> lap["AI lapida a intenção para<br/>vocabulário de código"]

    subgraph an["1. ANALYZE"]
        lap --> dig["mustard-rt run feature --intent<br/>(digest — chamado UMA vez)"]
        dig --> res{"cobertura?"}
        res -->|weak / none| requery["lê o menu vocabulary<br/>→ re-query afiada"]
        requery --> dig
        res -->|strong| sel["seleciona 5-10 anchors<br/>(nunca todas)"]
        sel --> unc["uncovered → resolve CADA um<br/>com Grep/Glob (existence gate)"]
        unc --> read["Task(Explore) consolidado<br/>lê as anchors sobreviventes"]
        read --> grill["grill seletivo: pedido vago →<br/>UMA AskUserQuestion batched"]
    end

    grill --> route2{"2. rota + escopo<br/>(determinístico)"}
    route2 -->|"1 camada, sem entidade nova"| totask(["vira /mustard:task — para aqui"])
    route2 -->|senão| draft["spec-draft — ÚNICO escrevedor do scaffold<br/>(spec.md + meta.json)"]
    draft --> prep["plan-prepare (autoridade do scope)<br/>+ analyze-validation (WARN → ## Concerns)"]
    prep --> scope{"scope?"}
    scope -->|full| fullp(["abre refs/feature/full-plan.md:<br/>PLAN por ondas + clarify<br/>→ /mustard:spec aprova"])
    scope -->|"light / extended-light"| approve

    subgraph ex["3. EXECUTE inline (Light)"]
        approve["spec anexada como preview da<br/>AskUserQuestion: aprovar / ajustar / salvar"] -->|aprovar| pre["emit-phase Execute → exec-rewave-check<br/>→ dependency-precheck (bloqueia dep externa ausente)"]
        pre --> disp["agent-prompt-render --emit ref<br/>→ Task (onda inteira em 1 msg)"]
        disp --> val["valida por onda"]
        val --> rev["REVIEW por subprojeto<br/>(review-result, máx 2 fix-loops)"]
        rev --> qa2["QA: qa-run"]
    end
    qa2 -->|pass| c(["CLOSE"])
    qa2 -->|fail| val
```

> Digest com ≥2 `concerns` → cada concern vira sua própria unidade, com suas próprias anchors (no Full: uma onda; no light/task: um despacho). Ponte de vocabulário confirmada → `equivalence-learn` persiste o aprendizado (sobrevive a re-scans).

---

## `/mustard:bugfix` — Pipeline de correção *(fluxo interno)*

Diagnóstico + correção autônomos. Lei de ferro: **nenhum fix antes de localizar e reproduzir a causa**. A triagem decide a localização: sintoma com token literal → `grep` direto; só conceito → digest.

| | |
|---|---|
| **Despacho** | pelo roteador; atalho: `/mustard:bugfix <descrição-do-erro>` |
| **Caminhos** | Fast Path (1-2 arquivos, causa clara, pula PLAN) · Full Path (3+ arquivos, spec enxuta) · **Promote** → vira `/mustard:feature` se o escopo real for de feature |
| **Backend** | `feature` (digest, só conceito) · `agent-prompt-render` · `digest-adherence-finalize` · `qa-run` · `scan` (pós-CLOSE) |

```mermaid
flowchart TD
    start(["router despacha bugfix"]) --> hyg["spec-hygiene + garante grain.model.json"]
    hyg --> triage{"sintoma tem token LITERAL?<br/>(msg de erro, campo, file:line, status HTTP)"}
    triage -->|sim| grep["grep/glob direto<br/>(pula o digest)"]
    triage -->|"não — só conceito"| dig["digest: mustard-rt run feature --intent<br/>→ LÊ as anchors apontadas"]
    grep --> diag["DIAGNOSE: Task(Explore) + skill diagnose<br/>(≤20 tool uses, ≤3 reads) → causa raiz"]
    dig --> diag
    diag --> cache["root-cause cache (hash em memória)"]

    cache --> assess{"2. ASSESS"}
    assess -->|"1-2 arquivos, causa clara"| fast["Fast Path (pula PLAN)"]
    assess -->|"3+ / cross-layer"| full["spec enxuta: Contexto + AC<br/>(repro: exit ≠0 antes, 0 depois)<br/>+ Causa raiz + Plano + Limites"]
    assess -->|"virou feature"| promote(["PROMOTE → /mustard:feature<br/>(pode disparar no meio do caminho;<br/>change-log.md registra)"])
    full --> appr["print da spec →<br/>/mustard:spec aprova"]
    appr --> exec
    fast --> exec

    subgraph ex["4. EXECUTE"]
        exec["agent-prompt-render --emit ref → Task"] --> validate["valida: build/type-check,<br/>sem regressão (máx 3 iter)"]
    end

    validate --> routef{"5. roteamento de falha"}
    routef -->|transient| retry["retry 1x"] --> validate
    routef -->|"resolvable (patch ≤3 linhas)"| patch["patch + retry"] --> validate
    routef -->|structural| reexp["cache bate? reusa resumo<br/>: re-Explore"] --> validate
    routef -->|BLOCKED| blocked["STOP + AskUserQuestion"]

    validate --> qa["6. emit QaReview → qa-run (máx 3 iter)"]
    qa -->|pass| close["CLOSE"]
    qa -->|fail| validate
    close --> rescan["mustard-rt run scan<br/>(se o código mudou materialmente)"]
    rescan --> done(["pronto"])
```

---

## `/mustard:spec` — Seletor unificado de specs

Substituiu `/approve` (PLAN) e `/resume` (EXEC). Um único *picker*: letra age na linha; letra + `r` aprova **e** executa inline; um **nome de spec** vai direto (modo focado, sem tabela).

| | |
|---|---|
| **Trigger** | `/mustard:spec [alvo]` — vazio (tabela) · `a`-`z` · `<letra>r` · nome da spec |
| **Backend** | `active-specs --format table` (só picker/letra) · `resume-bootstrap --spec --json` · downstream: `approve-spec`, `wave-advance`, `wave-tree` |
| **Regra** | Ordem das ondas e prompts decididos pelo Rust (`wave-advance`) — a AI só faz o *relay*; nome de spec NUNCA passa pela tabela |

```mermaid
flowchart TD
    start(["/mustard:spec [alvo]"]) --> parse{"alvo?"}
    parse -->|vazio| table["active-specs --format table<br/>+ blocos Siglas e Modo de seleção"]
    parse -->|"letra ou letra+r"| table2["render tabela → mapeia letra → spec"]
    parse -->|"nome de spec"| focused["modo focado: SEM tabela<br/>header de 1 linha + 1 confirmação"]
    table --> wait["espera a letra"]
    wait --> boot
    table2 --> boot
    focused --> boot["resume-bootstrap --spec --json"]

    boot --> stage{"stage?"}
    stage -->|Plan| clar{"Full sem .clarified?"}
    clar -->|sim| refuse["approve-spec RECUSA<br/>(clarify antes da aprovação — F6)"]
    clar -->|não| approve["resume-loop §A: aprovação<br/>(letra+r pré-responde:<br/>aprovar + implementar inline)"]
    stage -->|"Execute / Analyze /<br/>QaReview / Close"| loop["resume-loop §B: relay do wave-advance<br/>(mesma 'level' → 1 msg com todos os Task)"]
    approve --> done(["pronto"])
    loop --> done
```

> Casos de borda: 0 specs → "Nenhuma spec ativa."; >26 → 26 primeiras + contagem; nome desconhecido → erro + tabela como fallback.

---

## `/mustard:qa` — Fase de QA

Roda cada Critério de Aceitação (AC) e reporta pass/fail. **Bloqueia o CLOSE** em falha. Read-only — um pass é um *exit code observado*, nunca uma inferência.

| | |
|---|---|
| **Trigger** | `/mustard:qa [--spec <name>]` |
| **Backend** | `qa-run` (emite `qa.result`) · `tactical-fix-detect` |
| **Gate** | `close-gate` exige `qa.result.overall=pass` (`MUSTARD_QA_GATE_MODE=strict\|warn\|off`); editar a spec após um pass → QA **stale** |

```mermaid
flowchart TD
    start(["/mustard:qa"]) --> id["identifica spec (--spec ou active-specs[0])"]
    id --> hasAC{"## Acceptance Criteria<br/>com ≥1 AC?"}
    hasAC -->|não| stop(["'Spec has no Acceptance Criteria.'"])
    hasAC -->|sim| run["emit stage QaReview → qa-run<br/>(arquivo operativo: spec.md,<br/>ou wave-plan.md após decompose)"]

    run --> branch{"qa.result.overall"}
    branch -->|pass| pass["emit stage Close — 'QA passed.'"]
    branch -->|fail| fail["lista os ACs que falharam"]
    branch -->|skip| skip["sem Command: ou todos em timeout<br/>(120s por AC) → warn; não bloqueia o CLOSE"]

    fail --> iter{"3ª falha?"}
    iter -->|não| run
    iter -->|sim| ask["AskUserQuestion:<br/>(a) fix+retry (b) relaxar AC (c) abortar"]

    pass --> tf["Tactical-fix discovery (pós-pass):<br/>tactical-fix-detect → tactical_fix.proposed<br/>(propõe, NUNCA cria sozinho)"]
    tf --> gate["close-gate: exige overall=pass"]
    gate --> done(["→ /mustard:close"])
    skip --> done
```

---

## `/mustard:close` — Finalizar pipeline

Roda todos os gates num comando só e, se tudo passa, **finaliza em-processo automaticamente** — a spec vira `completed` sem janela de carência; follow-up vai numa sub-spec ligada (`/mustard:tactical-fix`), nunca numa flag desta spec.

| | |
|---|---|
| **Trigger** | `/mustard:close` (gate de docs aceita `--skip-docs` para spec não-arquitetural) |
| **Backend** | `close-orchestrate --spec` (encadeia a finalização em-processo) · `scan` condicional · `emit-event` (decision/lesson) |
| **Pré-condição** | `BLOCKED` aberto ou item `- [ ]` no Checklist → ABORTA antes de qualquer gate |
| **Regra** | NUNCA chamar `complete-spec` à mão, NUNCA emitir `pipeline.stage`/`outcome` à mão, NUNCA mover o diretório da spec (arquivamento é só evento) |

```mermaid
flowchart TD
    start(["/mustard:close"]) --> pre{"pré-condições: BLOCKED aberto?<br/>checklist com item não marcado?"}
    pre -->|sim| abortx(["ABORTA e reporta os itens"])
    pre -->|não| rescan["mustard-rt run scan<br/>(se ## Files mexeu em código)"]
    rescan --> orch["mustard-rt run close-orchestrate --spec<br/>(1 relatório JSON)"]

    subgraph gates["Gates (dentro do close-orchestrate)"]
        orch --> g1["1. build + tests (verify-pipeline)"]
        g1 --> g2["2. QA (qa-run) — fail bloqueia, skip passa"]
        g2 --> g3["3. review-spans — span vermelho bloqueia"]
        g3 --> g4["4. docs-stale-check (--skip-docs opcional)"]
        g4 --> g5["5. pipeline-summary (advisory)"]
    end

    g5 --> overall{"overall?"}
    overall -->|fail| report["report-only (chained: false)<br/>corrige o gate → re-roda"]
    report --> orch
    overall -->|pass| chain["finaliza IN-PROCESS (chained: true):<br/>spec → completed · pipeline.complete<br/>auto-verificado · meta.json Close/Completed"]

    chain --> know["emit-event decision/lesson<br/>(máx 3 cada; prosa durável → memória nativa)"]
    know --> metrics["arquiva métricas →<br/>.claude/.metrics/{spec}.json"]
    metrics --> banner["pipeline-summary → wave-tree →<br/>banner PIPELINE COMPLETE"]
    banner --> epic["épico: auto-fold em-processo<br/>(filhas todas fechadas → dobra)"]
    epic --> done(["pronto"])
```

> Cancelamento: emite `pipeline.stage: Close` + `pipeline.outcome: Cancelled` — também sem mover nada no filesystem.

---

## `/mustard:tactical-fix` — Sub-spec para correção tática *(fluxo interno)*

Cria uma sub-spec ligada a um pai quando REVIEW ou QA descobre um ajuste adjacente pequeno. Preserva a pureza SDD: o pai fica congelado após o approve; o vínculo é unidirecional (filha → pai).

| | |
|---|---|
| **Despacho** | pelo roteador; atalho: `/mustard:tactical-fix <parent> "<descrição>" [--scope touch\|light\|full]` (default `light` ≤100 LOC; `touch` ≤30 LOC) |
| **Backend** | `tactical-fix-create --parent --description --scope` |
| **Qualifica** | ≤100 LOC · sem mudança de contrato público · sem decisão de design pendente · sem nova dependência |

```mermaid
flowchart TD
    start(["/mustard:tactical-fix &lt;parent&gt; '&lt;desc&gt;'"]) --> qual{"qualifica?<br/>≤100 LOC · sem contrato público<br/>sem design pendente · sem nova dep"}
    qual -->|não| route["follow-up normal OU /mustard:feature"]
    qual -->|sim| create["mustard-rt run tactical-fix-create"]

    create --> gen["rust gera:<br/>slug YYYY-MM-DD-kebab · dir (aborta se existe)<br/>spec.md narrativo (link [[parent]])<br/>meta.json (parent + lang + stage Analyze)<br/>evento spec.link"]
    gen --> print["print: sub-spec criada —<br/>edite e rode /mustard:spec"]
    print --> done(["mesmo pipeline, mesmos gates<br/>(sem 'modo light' de gate)"])
```

> Fail-open na existência do pai: a sub-spec é criada mesmo se `<parent>` não existir (só a navegação do dashboard degrada). Nunca auto-aprova — o usuário revisa a semente e roda `/mustard:spec`.

---

# Delegação e revisão

## `/mustard:task` — Execução delegada (spec-less) *(fluxo interno)*

Delega cada ação em contexto Task isolado. Lei de ferro: **UMA camada** — no momento em que crescer para duas, é `/mustard:feature`. O orquestrador nunca lê fonte nem implementa; localiza primeiro, despacha depois.

| Ação | `--role` | `subagent_type` |
|---|---|---|
| `analyze` | `explore` | Explore (read-only) |
| `audit` | `audit` | general-purpose |
| `compare` | `explore` ×N → `plan` | Explore em paralelo → Plan |
| `review` | `review` | mustard-review (read-only) |
| `docs` | `docs` | general-purpose |
| `refactor` | `plan` → `implement` | Plan → general-purpose |
| `implement` | `implement` | general-purpose |

```mermaid
flowchart TD
    start(["router despacha task"]) --> locate{"LOCATE primeiro:<br/>token literal conhecido?"}
    locate -->|sim| grep["grep/glob"]
    locate -->|conceito| dig["digest: feature --intent<br/>→ LÊ as anchors"]
    grep --> render
    dig --> render["agent-prompt-render --role {ação}<br/>--task-text '…anchors…' --emit ref<br/>(prompt NUNCA à mão)"]
    render --> disp["Task com o stub verbatim<br/>(≥2 concerns → 1 despacho por concern)"]

    disp --> acts{"especificidades"}
    acts -->|refactor| two["2 fases: Plan → print →<br/>AskUserQuestion → implement"]
    acts -->|compare| par["1 explore por subprojeto em paralelo<br/>→ Plan funde + aponta divergências"]
    acts -->|"audit"| chk["checklist (copy·design·a11y·i18n·<br/>consistency·api-contract) via --task-text<br/>→ CRITICAL/WARNING viram opções — user escolhe"]
    acts -->|implement| impl["retorna ≤30 linhas + roda build/type-check<br/>CONCERN → oferece /mustard:feature Light"]

    two --> lex
    par --> lex
    chk --> lex
    impl --> lex["fim da run: equivalence-learn<br/>(SÓ ponte de vocabulário confirmada)"]
```

> Sem spec e sem close por design — precisa de rastro? Promova para `/mustard:feature` Light ou `/mustard:tactical-fix`.

---

## `/mustard:review` — Revisão de Pull Request

Detecta o PR, invoca a revisão e reporta. ZERO confirmações. Ao final, **emite o veredito** — sem `review.result` a spec fica presa em `ReviewPending`.

| | |
|---|---|
| **Trigger** | `/mustard:review [nº-ou-URL do PR]` (sem arg: auto-detecta o PR da branch) |
| **Backend** | `review-prefetch` · `diff-context` · `emit-event review.start/complete` · `review-result --verdict --critical` · `tactical-fix-detect` |
| **Provider** | `mustard.json#git.provider` (github/gitlab) |
| **Budget** | ≤1 Bash p/ detecção · ≤1 Skill/Task · ≤4 chamadas de API |

```mermaid
flowchart TD
    start(["/mustard:review [pr]"]) --> resolve{"argumento?"}
    resolve -->|"número / URL"| ref["usa direto"]
    resolve -->|nenhum| detect["gh pr view --json (branch atual)"]
    detect --> noPR{"PR aberto?"}
    noPR -->|não| stop(["'No open PR found. Run /git pr first.'"])
    noPR -->|sim| ref

    ref --> prefetch["review-prefetch --format json + diff-context<br/>(fonte da verdade — não re-buscar)"]
    prefetch --> emit1["emit review.start"]
    emit1 --> invoke["cola o diff como ## DIFF<br/>→ Skill(code-review)<br/>(fallback: Task general-purpose)"]
    invoke --> emit2["emit review.complete → resultados verbatim"]

    emit2 --> verdict["review-result --verdict approved|rejected<br/>--critical N (obrigatório — o resume gate lê isto;<br/>nunca gravar approved só p/ destravar)"]
    verdict --> tf["Tactical-fix discovery:<br/>tactical-fix-detect → tactical_fix.proposed"]
    tf --> out{"veredito?"}
    out -->|APPROVED| done(["pronto"])
    out -->|REJECTED| fixloop["fix-loop normal → re-review"]
```

---

# Git e manutenção

## `/mustard:git` — Operações de git

Lê o *git flow* do `mustard.json`. **PR é o único caminho de integração** — uma branch de trabalho chega à base via `pr`, nunca por push local na base. Apenas operações reversíveis; aborta em QUALQUER conflito.

| Ação | Descrição |
|---|---|
| `sync` | Rebase da branch atual sobre `origin/<base>` (base derivada do prefixo `{base}_`) |
| `commit` | Commit sem push; `--scope` default `all` (`add -A` — nunca escopo parcial silencioso) |
| `push` | Sync → commit + push SÓ da branch atual (com upstream) |
| `pr [<target>]` | Abre/atualiza PR (idempotente) — um por repo, submódulo antes do pai; cada `push`/`pr` atualiza o MESMO PR até o `pr close`. Base pura `B` → promove/backporta via `flow[B]` |
| `pr close [<worktree>]` | Ritual de saída pós-merge: confirma o merge, volta à base, remove worktree + branch local e remota. Não mergeado → só avisa |

Não existe ação `merge` — a integração acontece no provedor, via PR.

| | |
|---|---|
| **Backend** | `git-settle` (+ `git-settle --unit <branch>`) no `pr close`; todo git/gh cru via `rtk git` / `rtk gh` |
| **Regras de ferro** | Sobe TUDO (`add -A`); nunca operar numa base pura (exceto `pr`); `rtk` prefixa todo `git` (até em `&&` e `$(…)`); submódulos antes do pai, cada um na sua branch `{base}_{slug}` com PR próprio |

```mermaid
flowchart TD
    start(["/mustard:git &lt;ação&gt;"]) --> s0["Step 0: resolve $BASE do<br/>prefixo {base}_ da branch"]
    s0 --> prot{"base pura (ex.: dev, main)?"}
    prot -->|"sim, ação de escrita"| refuse(["recusa — na base pura<br/>só /git pr é permitido"])
    prot -->|ok| sub["Step 0c: checa HEAD de submódulos"]

    sub --> action{"ação?"}
    action -->|sync| sync["auto-stash → fetch +<br/>rebase origin/$BASE → stash pop<br/>(aborta em conflito)"]
    action -->|commit| commit["analisa → exclui efêmeros → add -A<br/>→ commit submódulos (paralelo) → commit pai"]
    action -->|push| push["sync (para em conflito) →<br/>commit + push só a branch atual<br/>(submódulo na base corta {base}_{slug} ANTES)"]
    action -->|pr| pr["push → 1 PR por repo<br/>(submódulo antes do pai) na base do prefixo<br/>PR existente → imprime a URL do MESMO"]
    action -->|"pr close"| settle["git-settle (confirma merge, avança a base)<br/>→ ExitWorktree → git-settle --unit &lt;branch&gt;<br/>(pull, remove worktree, apaga branch local+remota)"]

    sync --> reportx["Final Status Report"]
    commit --> reportx
    push --> reportx
    pr --> reportx
    settle --> reportx
```

---

## `/mustard:maint` — Utilitários de manutenção

| Ação | Backend | Descrição |
|---|---|---|
| `deps [--dry-run]` | `maint-deps` | Instala dependências de todos os subprojetos (comando por tipo: `pnpm install`, `cargo fetch`, `dotnet restore`…) |
| `validate [--dry-run]` | `maint-validate` | Build + type-check por subprojeto (`pnpm typecheck`, `cargo check`…) |
| `sync` | `scan` | Refresca o `grain.model.json` |
| `doctor [--residue]` | `doctor` + `diagnose-otel` | Health check: wiring, drift, state-health, residue + telemetria OTEL — nunca bloqueia |

```mermaid
flowchart TD
    start(["/mustard:maint &lt;ação&gt;"]) --> action{"ação?"}
    action -->|deps| deps["mustard-rt run maint-deps<br/>(auto-descobre subprojetos do grain.model.json)"]
    action -->|validate| val["mustard-rt run maint-validate<br/>(JSON: overall + validates[])"]
    action -->|sync| sync["mustard-rt run scan → grain.model.json"]
    action -->|doctor| doc["doctor (+ --residue) + diagnose-otel"]
    doc --> consol["relatório consolidado:<br/>wiring · drift · state-health · residue<br/>(OK / WARN / FAIL — nunca bloqueia)"]
```

> O binário resolve os comandos por subprojeto sozinho — nunca ler a tabela de Agents ou o `CLAUDE.md` do subprojeto à mão para isso.

---

# Observabilidade e conhecimento

## `/mustard:status` — Status consolidado

| | |
|---|---|
| **Trigger** | `/mustard:status [--harness]` |
| **Backend** | `status --format table` · `status --harness --format table` |
| **Regra** | Sempre delega ao binário (nunca parsear NDJSON à mão); `--harness` é estritamente read-only |

```mermaid
flowchart TD
    start(["/mustard:status [--harness]"]) --> mode{"--harness?"}
    mode -->|não| st["mustard-rt run status --format table<br/>(git · specs ativas/órfãs · build · entity registry)"]
    mode -->|sim| hn["mustard-rt run status --harness<br/>(lê settings.json, agrupa hooks por evento,<br/>resolve o modo de cada módulo)"]
    st --> print["print verbatim"]
    hn --> print
    print --> orphan{"pipelines órfãos?"}
    orphan -->|sim| suggest["sugere /mustard:close ou /mustard:maint"]
```

---

## `/mustard:stats` — Métricas do pipeline

| | |
|---|---|
| **Trigger** | `/mustard:stats [--hooks] [--since] [--event] [--compare] [--pr] [--days <n>]` |
| **Backend** | `metrics collect` (default) · `metrics report` (--hooks) · `event-projections --view pr-metrics` (--pr, estilo DORA) |

```mermaid
flowchart TD
    start(["/mustard:stats [flags]"]) --> flag{"flag?"}
    flag -->|"(default)"| coll["metrics collect<br/>(superset: pipelines + hooks + RTK)"]
    flag -->|--hooks| hooks["metrics report --since/--event/--compare"]
    flag -->|--pr| pr["event-projections --view pr-metrics<br/>(pr.opened/merged + review.start/complete,<br/>pareados por spec ou branch)"]
    coll --> print["print verbatim"]
    hooks --> print
    pr --> print
    print --> sections["Summary → Active/Orphaned → Completed<br/>→ Last 7 Days → Enforcement → RTK gain"]
```

---

## `/mustard:knowledge` — Gestão de conhecimento

Conhecimento = memória nativa do Claude Code (prosa durável) + eventos `decision`/`lesson` no NDJSON por spec (emitidos no CLOSE via `emit-event`).

| Ação | Backend / propósito |
|---|---|
| `list [spec]` | `event-projections --view pipeline-state` — decisions[]/lessons[] da spec |
| `search <term>` | MCP `search_knowledge` — match em title/detail dos eventos |
| `add` | interativo → `emit-event --event decision`/`lesson` |
| `notes [target]` | edita `notes.md` (injetado nos agentes; nunca sobrescrito pelo `/mustard:scan`) |
| `audit` | compara memória nativa vs CLAUDE.md/skills (report-only) |
| `report <period>` | relatórios de progresso via git |

```mermaid
flowchart TD
    start(["/mustard:knowledge &lt;ação&gt;"]) --> action{"ação?"}
    action -->|list| list["event-projections --view pipeline-state<br/>(decisions[] / lessons[])"]
    action -->|search| search["MCP search_knowledge &lt;term&gt;"]
    action -->|add| add["interativo → emit-event decision/lesson<br/>(append-only, nunca editado à mão)"]
    action -->|notes| notes["edita notes.md do subprojeto"]
    action -->|audit| audit["memória nativa vs CLAUDE.md/skills<br/>(report-only, nunca auto-edita)"]
    action -->|report| rep["relatórios git (refs/knowledge/report.md)"]
    list --> print["print verbatim (sempre com contagem)"]
    search --> print
    add --> print
```

---

# Skills

## `/mustard:skills` — Gerenciador de skills

| Ação | Backend |
|---|---|
| `install <name-or-path>` | manual — cópia para `.claude/skills/<name>/` + validação do frontmatter (sem fetch embutido) |
| `create <name>` | skill `skill-creator` (não vem no pacote — instalar à parte) |
| `list` | listagem de `.claude/skills/*/SKILL.md` + frontmatter |
| `remove <name>` | apaga `.claude/skills/{name}/` (avisa se `source: scan`; `source: manual` exige confirmação) |
| `optimize` / `eval` | loops do `skill-creator` (requer Python 3 + `claude` CLI) |
| `update` | skills embutidas atualizam com o plugin (marketplace); as manuais são suas |

O campo `source:` é territorial: `/mustard:scan` escreve só `source: scan`; `install`/`create` escrevem só `source: manual`; ausente → tratado como `manual` (conservador).

```mermaid
flowchart TD
    start(["/mustard:skills &lt;ação&gt;"]) --> action{"ação?"}
    action -->|install| inst["copiar p/ .claude/skills/&lt;name&gt;/<br/>→ valida frontmatter → source: manual"]
    action -->|create| create["skill-creator (interativo)<br/>(inerte até instalá-lo à parte)"]
    action -->|list| list["lista .claude/skills/ + frontmatter"]
    action -->|remove| remove{"source?"}
    remove -->|manual| confirm["pede confirmação"]
    remove -->|scan| warn["avisa que é gerado pelo /mustard:scan"]
    action -->|"optimize / eval"| opt["skill-creator (Python 3 + claude CLI)"]
    action -->|update| upd["plugin via marketplace<br/>(ou mustard init, idempotente)"]
```

> Curiosidade que virou regra: o arquivo do comando chama-se `skills.md` (plural) porque `skill.md` colide com o marcador `SKILL.md` em filesystems case-insensitive (Windows/macOS) — e o plugin inteiro perderia a pasta `commands/`.

---

# Harness (liga/desliga dos hooks)

## `/mustard:unhook` — Kill-switch do harness

Desabilita os hooks renomeando `settings.json` para `settings.json.disabled-<timestamp>` e limpa estado volátil (`.agent-state/`, `.cluster-cache.json`, `.worktrees/`). Reversível via `/mustard:rehook`.

| Scope | O que toca |
|---|---|
| `this` | só `<repo>/.claude/settings.json` (default) |
| `monorepo` | `<repo>/.claude/` + todos `apps/*` e `packages/*` |
| `all` | monorepo + `~/.claude/settings.json` global (requer `--confirm`) |

```mermaid
flowchart TD
    start(["/mustard:unhook [--scope] [--confirm]"]) --> run["mustard-rt run unhook --scope<br/>(nunca renomear à mão — o binário<br/>é dono do formato do timestamp)"]
    run --> scopeChk{"scope all sem --confirm?"}
    scopeChk -->|sim| skip["global: state skipped (não toca)"]
    scopeChk -->|não| apply["aplica no scope"]
    skip --> report
    apply --> report["print verbatim (state por entrada:<br/>disabled / missing / skipped / error)<br/>+ campo revert_with"]
    report --> done(["sugere /mustard:rehook --scope &lt;mesmo&gt;"])
```

---

## `/mustard:rehook` — Restaurar o harness

Reverte o `/mustard:unhook`: acha o snapshot `settings.json.disabled*` mais recente em cada `.claude/` do escopo e renomeia de volta. Diretórios voláteis não são recriados — o runtime os regenera.

| | |
|---|---|
| **Trigger** | `/mustard:rehook [--scope this\|monorepo\|all] [--confirm]` |
| **Backend** | `mustard-rt run rehook --scope` |
| **States** | restored · already-active · no-snapshot · missing · skipped · error |

```mermaid
flowchart TD
    start(["/mustard:rehook [--scope] [--confirm]"]) --> run["mustard-rt run rehook --scope"]
    run --> find["por .claude/ no scope:<br/>acha settings.json.disabled* mais recente"]
    find --> state{"estado?"}
    state -->|encontrado| restore["renomeia de volta → settings.json"]
    state -->|já ativo| active["already-active"]
    state -->|sem snapshot| nosnap["no-snapshot"]
    state -->|sem .claude/| missing["missing"]
    restore --> report["print verbatim (state por entrada)"]
    active --> report
    nosnap --> report
    missing --> report
    report --> allActive{"tudo already-active?"}
    allActive -->|sim| hint["sugere: talvez quisesse /mustard:unhook"]
```

---

## Tabela-resumo de todos os comandos

| Comando | Categoria | Backend principal (`mustard-rt run …`) | Usa `grain.model.json`? |
|---|---|---|---|
| `/mustard` | porta única | — (roteia via `CLAUDE.md § Intent Routing`) | não |
| `/mustard:scan` | core | `scan --full`, `scan-guards-*`, `scan-patterns-*` | **produz** |
| `/mustard:feature` | core · fluxo interno | `feature`, `spec-draft`, `plan-prepare`, `analyze-validation`, `agent-prompt-render` | consome (digest) |
| `/mustard:bugfix` | core · fluxo interno | `feature`, `agent-prompt-render`, `qa-run`, `scan` | consome (digest) + refresca |
| `/mustard:spec` | core | `active-specs`, `resume-bootstrap`, `wave-advance` | indireto |
| `/mustard:qa` | core | `qa-run`, `tactical-fix-detect` | não |
| `/mustard:close` | core | `close-orchestrate` (+ `scan`) | refresca se mudou |
| `/mustard:tactical-fix` | core · fluxo interno | `tactical-fix-create` | não |
| `/mustard:task` | delegação · fluxo interno | `agent-prompt-render`, `feature` (digest), `equivalence-learn` | indireto |
| `/mustard:review` | revisão | `review-prefetch`, `diff-context`, `review-result`, `tactical-fix-detect` | não |
| `/mustard:git` | git | `git-settle` (+ git nativo via `rtk`) | não |
| `/mustard:maint` | manutenção | `maint-deps`, `maint-validate`, `scan`, `doctor`, `diagnose-otel` | refresca (sync) |
| `/mustard:status` | observabilidade | `status` | não |
| `/mustard:stats` | observabilidade | `metrics collect/report`, `event-projections` | não |
| `/mustard:knowledge` | conhecimento | `event-projections`, `emit-event`, MCP `search_knowledge` | não |
| `/mustard:skills` | skills | manual (sem backend `run`) | não |
| `/mustard:unhook` | harness | `unhook` | não |
| `/mustard:rehook` | harness | `rehook` | não |

---

*Derivado dos comandos do plugin em `plugin/commands/`. Quando um fluxo mudar, re-derive deste diretório — ele é a fonte da verdade.*
