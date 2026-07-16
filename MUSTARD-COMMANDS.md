# Mustard — Comandos e Fluxos

Referência visual de **cada comando do Mustard** e seu fluxo de execução.
Os diagramas usam [Mermaid](https://mermaid.js.org/) — renderizam direto no GitHub, no VS Code (com extensão Mermaid) e no dashboard.

> **Convenções dos diagramas**
> - **AI** = passo de raciocínio que o orquestrador (Claude) faz.
> - **rust** = trabalho determinístico delegado ao binário `mustard-rt` (sem AI).
> - **Task** = subagente despachado em contexto isolado.
> - **gate** = portão bloqueante (só passa se a condição for satisfeita).
> - Termos técnicos (nomes de comandos, fases, eventos, arquivos) ficam no original.

---

## Mapa do ecossistema

Como os comandos se encaixam. O eixo central é o **pipeline orientado a especificação** (SDD): tudo nasce de uma varredura determinística (`/scan`) e converge para o fechamento auditável (`/close`).

```mermaid
flowchart TD
    scan["/scan<br/>(rust, sem AI)"] -->|grain.model.json| feat["/feature"]
    scan -->|grain.model.json| bug["/bugfix"]
    scan -->|grain.model.json| prd["/prd"]

    feat -->|spec.md + meta.json| spec["/spec<br/>(approve / resume)"]
    bug -->|fast path: inline| exec
    bug -->|full path: spec| spec

    spec -->|EXECUTE| exec["EXECUTE<br/>(Task: agentes por onda)"]
    exec --> review["/review"]
    review --> qa["/qa"]
    qa -->|gate: pass| close["/close"]

    review -. candidato .-> tf["/tactical-fix<br/>(sub-spec ligada ao pai)"]
    qa -. candidato .-> tf
    tf --> spec

    close -->|se código mudou| scan

    subgraph apoio["Apoio / fora do pipeline"]
        task["/task<br/>(delegação spec-less)"]
        git["/git"]
        maint["/maint"]
        status["/status"]
        stats["/stats"]
        knowledge["/knowledge"]
        skill["/skill"]
        unhook["/unhook"]
        rehook["/rehook"]
    end
```

**Princípio central:** o código-fonte **nunca é lido em massa**. O `/scan` minera o repositório uma vez para `grain.model.json`; os comandos de pipeline consomem esse modelo via *digest* (`mustard-rt run feature`, `scan spec`) e leem apenas as ~12 *anchors* (arquivos-âncora) que o digest aponta. É assim que o Mustard economiza contexto.

---

## Pipeline canônico

Vocabulário único de fases (fonte: `refs/canonical-phases.md`):

```mermaid
flowchart LR
    A["ANALYZE"] --> P["PLAN"]
    P -->|/approve| E["EXECUTE"]
    E --> R["REVIEW"]
    R --> Q["QA"]
    Q -->|gate| C["CLOSE"]
```

- **Light scope** (1-2 camadas, ≤5 arquivos, padrão conhecido): pula o **PLAN** → `ANALYZE → EXECUTE → REVIEW → QA → CLOSE`.
- **Full scope** (3+ camadas, entidade nova): pipeline completo com aprovação humana entre PLAN e EXECUTE.

---

# Comandos do pipeline (core)

## `/scan` — Modelo do código-base

Minera o repositório para `grain.model.json` (determinístico, agnóstico de linguagem, **sem AI**). É o produto durável que `/feature` e `/bugfix` consomem.

| | |
|---|---|
| **Trigger** | `/scan`, `/scan --root <dir>`, `/scan --out <path>` |
| **Backend** | `mustard-rt run scan` |
| **Produz** | `.claude/grain.model.json` |
| **Regra** | Não escreve nada nos subprojetos; não gera skills/agentes; sem confirmação (o próprio `/scan` é a aprovação) |

```mermaid
flowchart TD
    start(["/scan"]) --> run["mustard-rt run scan<br/>(rust, sem leitura de fonte)"]
    run --> model[("grain.model.json<br/>módulos · declarações · grafo de deps<br/>roles · slices · contratos · touchpoints")]
    model --> report["AI: parseia { ok, model }<br/>reporta o caminho do modelo"]
    report --> done(["pronto — consumido por<br/>/feature e /bugfix via digest"])
```

---

## `/feature` — Pipeline de feature

Entende o cliente, pesquisa o repositório via *digest* do scan (nunca lendo fonte à mão), planeja e implementa. É o pipeline mais completo.

| | |
|---|---|
| **Trigger** | `/feature <request>` |
| **Fases** | `ANALYZE → DECOMPOSE → PLAN → (/approve) → EXECUTE → REVIEW → QA → CLOSE` |
| **Escopo** | light / extended-light / full (auto-detectado) |
| **Materializa** | `.claude/spec/{slug}/spec.md` + `meta.json` (apenas via `spec-draft`) |

```mermaid
flowchart TD
    start(["/feature &lt;request&gt;"]) --> hyg["spec-hygiene + emit pipeline.stage: Analyze"]

    subgraph an["1. ANALYZE"]
        hyg --> fresh{"grain.model.json<br/>existe / fresco?"}
        fresh -->|não| sc["mustard-rt run scan"]
        fresh -->|sim| dig
        sc --> dig["mustard-rt run feature --intent<br/>(digest do scan → insumos)"]
        dig --> miss{"miss?"}
        miss -->|sim| requery["AI: re-query com<br/>vocabulário do repo"]
        requery --> dig
        miss -->|não| anchors["AI: lê SÓ as anchors (~12 arquivos)"]
        anchors --> scope["detecta escopo:<br/>light / extended-light / full"]
    end

    scope --> decomp["2. DECOMPOSE (AI)<br/>unidades c/ precedente · invariantes · gaps net-new"]

    subgraph pl["3. PLAN"]
        decomp --> lap["por unidade: mustard-rt run scan spec<br/>→ draft; AI lapida no idioma do projeto"]
        lap --> draft["mustard-rt run spec-draft<br/>(materializa spec.md + meta.json)"]
        draft --> fold["AI: Edit dobra o corpo lapidado<br/>nas seções do Plano"]
        fold --> wave{"full scope?"}
        wave -->|sim| wsc["wave-scaffold<br/>(specs por onda + review/qa)"]
        wave -->|não| val
        wsc --> val["analyze-validation<br/>(WARN → ## Concerns)"]
        val --> audit["Concern Coverage Audit"]
    end

    audit --> ask{"AskUserQuestion:<br/>aprovar?"}
    ask -->|"salvar p/ depois"| stop(["para — retomar via /spec"])
    ask -->|"ajustar"| lap
    ask -->|"aprovar (light/ext-light)"| exec

    subgraph ex["4. EXECUTE (inline p/ light)"]
        exec["emit Execute → exec-rewave-check<br/>→ dependency-precheck"] --> disp["agent-prompt-render → dispatch Task<br/>(todos agentes da onda em 1 msg)"]
        disp --> valw["valida por onda"]
        valw --> rev["REVIEW por subprojeto<br/>(re-reviews em sonnet, máx 2 loops)"]
        rev --> qa2["QA: qa-run"]
    end

    qa2 -->|pass| close2(["→ CLOSE"])
    qa2 -->|fail| valw
```

---

## `/bugfix` — Pipeline de correção

Diagnóstico + correção autônomos, sem troca de contexto. **Consome** o scan (não roda varredura interativa).

| | |
|---|---|
| **Trigger** | `/bugfix <error-description>` |
| **Caminhos** | Fast Path (1-2 arquivos, causa clara, pula PLAN) · Full Path (3+ arquivos, spec enxuta) |
| **Usa o scan** | Entrada (consome `grain.model.json` via digest) e saída (re-scan se o código mudou) |

```mermaid
flowchart TD
    start(["/bugfix &lt;bug&gt;"]) --> hyg["hygiene + emit Analyze"]

    subgraph an["1. ANALYZE"]
        hyg --> ensure["garante grain.model.json (scan se ausente)"]
        ensure --> research["mustard-rt run feature --intent<br/>(digest — sem ler fonte)"]
        research --> diag["DIAGNOSE: Task(Explore) + skill 'diagnose'<br/>(≤20 tool uses, ≤3 reads) → causa raiz"]
        diag --> cache["root-cause cache (hash em memória)"]
    end

    cache --> assess{"2. ASSESS<br/>quantos arquivos?"}
    assess -->|"1-2, causa clara"| fast["Fast Path (pula PLAN)"]
    assess -->|"3+, cross-layer"| full["3. Full Path Spec<br/>(Contexto + AC + Causa raiz + Plano + Limites)"]
    full --> approve["print spec → /mustard:spec p/ aprovar"]
    approve --> exec
    fast --> exec

    subgraph ex["4. EXECUTE"]
        exec["agent-prompt-render → dispatch"] --> validate["valida: build/type-check<br/>sem regressão (máx 3 iter)"]
    end

    validate --> route{"5. Failure routing"}
    route -->|"transient"| retry["retry 1x"]
    route -->|"resolvable ≤3 linhas"| patch["patch + retry"]
    route -->|"structural"| reexp["checa cache / re-Explore"]
    route -->|"BLOCKED"| blocked["STOP + AskUserQuestion"]
    retry --> validate
    patch --> validate
    reexp --> validate

    validate --> qa["6. QA: emit QaReview → qa-run"]
    qa -->|pass| close["CLOSE"]
    qa -->|fail| validate
    close --> rescan["mustard-rt run scan<br/>(se código mudou materialmente)"]
    rescan --> done(["pronto"])
```

---

## `/spec` — Seletor unificado de specs

Substitui `/approve` (PLAN) e `/resume` (EXEC). Um único *picker*: a letra aprova (PLAN) ou continua (EXEC); letra + `r` aprova e executa inline na mesma sessão.

| | |
|---|---|
| **Trigger** | `/mustard:spec [letra[r]]` |
| **Backend** | `active-specs` (render) · `resume-bootstrap` (rota) · `wave-advance` (despacho renderizado) |
| **Regra** | A ordem das ondas é decidida pelo Rust (`wave-advance`), nunca pela AI |

```mermaid
flowchart TD
    start(["/mustard:spec [letra[r]]"]) --> render["mustard-rt run active-specs --format table<br/>(print verbatim + Siglas + Modo de seleção)"]
    render --> parse{"parse da letra"}
    parse -->|inválida| err["'Letra inválida.' + re-render"]
    parse -->|"^[a-z]$ ou ^[a-z]r$"| boot["mustard-rt run resume-bootstrap --spec --json<br/>(stage · mode · operationalSpecPath ...)"]

    boot --> stage{"stage?"}
    stage -->|"Plan (sem r)"| approveOnly["approve-only-flow<br/>(aprova, para)"]
    stage -->|"Plan + r"| approveResume["approve + execute inline"]
    stage -->|"Execute / Analyze / QaReview / Close"| resume["resume-flow (continua; ignora r)"]

    approveResume --> dp
    resume --> dp["mustard-rt run wave-advance --spec<br/>(nível pendente, prompts já renderizados)"]
    dp --> loop["por item {wave, role, subproject, subagent_type, prompt}:<br/>relay do prompt → Task(prompt verbatim)"]
    loop --> note["mesma 'level' → despacha em 1 msg<br/>pós-dispatch → resume-flow"]
    note --> done(["pronto"])
    approveOnly --> done
```

---

## `/qa` — Fase de QA (Wave 10)

Roda cada Critério de Aceitação (AC) e reporta pass/fail. **Bloqueia o CLOSE** em caso de falha. Read-only — nunca modifica código.

| | |
|---|---|
| **Trigger** | `/mustard:qa [--spec <name>]` |
| **Backend** | `qa-run` (emite `qa.result`) · `tactical-fix-detect` |
| **Gate** | `close-gate` exige `qa.result.overall=pass` (`MUSTARD_QA_GATE_MODE=strict\|warn\|off`) |

```mermaid
flowchart TD
    start(["/mustard:qa"]) --> id["identifica spec (--spec ou active-specs[0])"]
    id --> hasAC{"tem ## Acceptance Criteria<br/>com ≥1 AC + Command?"}
    hasAC -->|não| stop(["'Spec has no Acceptance Criteria.'"])
    hasAC -->|sim| run["emit QaReview → mustard-rt run qa-run<br/>(roda cada AC)"]

    run --> branch{"qa.result.overall"}
    branch -->|pass| pass["emit pipeline.stage: Close<br/>'QA passed.'"]
    branch -->|fail| fail["lista ACs que falharam"]
    branch -->|skip| skip["warn: 'No AC — QA skipped.'"]

    fail --> iter{"3ª falha?"}
    iter -->|não| run
    iter -->|sim| ask["AskUserQuestion:<br/>(a) fix+retry (b) relax AC (c) abort"]

    pass --> tf["Tactical-fix discovery (pós-pass)<br/>tactical-fix-detect → tactical_fix.proposed"]
    tf --> gate["CLOSE gate: exige overall=pass"]
    gate --> done(["→ /close"])
    skip --> done
```

---

## `/close` — Finalizar pipeline

Verifica build/review/QA, arquiva a spec (semântico, sem mover diretório) e emite o banner de conclusão. A finalização é **automática e determinística**.

| | |
|---|---|
| **Trigger** | `/close` |
| **Backend** | `close-orchestrate` (1 relatório JSON; encadeia `complete-spec` em processo) |
| **Gates** | build+tests · QA · review-spans · docs audit · checklist/concerns |
| **Regra** | Nunca chamar `complete-spec` à mão; nunca mover o diretório da spec |

```mermaid
flowchart TD
    start(["/close"]) --> locate["localiza spec; estado vem do meta.json + eventos"]
    locate --> rescan["mustard-rt run scan<br/>(se ## Files mexeu no código)"]
    rescan --> orch["mustard-rt run close-orchestrate --spec"]

    subgraph gates["Gates (dentro do close-orchestrate)"]
        orch --> g1["1. verify-pipeline (build + tests)"]
        g1 --> g2["2. qa-run (fail → bloqueia)"]
        g2 --> g3["3. review-spans (span vermelho → bloqueia)"]
        g3 --> g4["4. docs-stale-check"]
        g4 --> g5["5. pipeline-summary (advisory)"]
    end

    g5 --> overall{"overall?"}
    overall -->|fail| reportonly["report-only (chained: false)<br/>corrige gate → re-roda"]
    reportonly --> orch
    overall -->|pass| chain["encadeia complete-spec IN-PROCESS<br/>spec → closed-followup<br/>emite pipeline.complete + auto-verifica"]

    chain --> stamp["emit Stage: Close · Outcome: Completed · flag followup_open"]
    stamp --> know["emit-event decision/lesson (máx 3 cada)"]
    know --> metrics["arquiva métricas → .claude/metrics/{spec}.json"]
    metrics --> banner["pipeline-summary → wave-tree → banner PIPELINE COMPLETE"]
    banner --> epic["fold por épico (in-process no close-orchestrate)"]
    epic --> done(["pronto"])
```

---

## `/tactical-fix` — Sub-spec para correção tática

Cria uma sub-spec ligada a um pai quando REVIEW ou QA descobre um ajuste adjacente pequeno. Preserva a pureza SDD: o pai fica congelado após o approve.

| | |
|---|---|
| **Trigger** | `/mustard:tactical-fix <parent> "<descrição>" [--scope touch\|light\|full]` |
| **Backend** | `tactical-fix-create` (slug, dir, spec.md narrativo, meta.json, evento `spec.link`) |
| **Qualifica** | ≤100 LOC · sem mudança de contrato público · sem decisão de design pendente · sem nova dependência |

```mermaid
flowchart TD
    start(["/mustard:tactical-fix &lt;parent&gt; '&lt;desc&gt;'"]) --> qual{"qualifica?<br/>≤100 LOC · sem contrato público<br/>sem design pendente · sem nova dep"}
    qual -->|não| route["follow-up normal OU /mustard:feature"]
    qual -->|sim| create["mustard-rt run tactical-fix-create<br/>--parent --description --scope"]

    create --> gen["rust gera:<br/>slug YYYY-MM-DD-kebab · dir (aborta se existe)<br/>spec.md narrativo ([[parent]] link)<br/>meta.json (parent + lang + stage Analyze)<br/>evento spec.link"]
    gen --> print["print: 'Sub-spec created ... edit + /mustard:spec'"]
    print --> done(["usuário edita → /mustard:spec<br/>(mesmo pipeline, mesmos gates)"])
```

---

# Comandos de delegação / revisão

## `/task` — Execução delegada (spec-less)

Delega cada ação em contexto Task isolado (L0 Universal Delegation). Sem spec, sem gates de higiene — modo vibe/prototype.

| Ação | Agente | Modelo | Descrição |
|---|---|---|---|
| `analyze` | Explore | sonnet | Exploração / análise de padrões |
| `audit` | general-purpose | sonnet | Auditoria de qualidade com checklist |
| `compare` | explorers paralelos → Plan | sonnet | Alinhamento entre subprojetos |
| `review` | general-purpose | opus | SOLID / segurança / performance |
| `docs` | general-purpose | sonnet | Geração de documentação |
| `refactor` | Plan → general-purpose | sonnet/opus | Plano + approve + implementa |
| `implement` | general-purpose | sonnet | Despacho único com slices inline |

```mermaid
flowchart TD
    start(["/task &lt;action&gt; &lt;scope&gt;"]) --> slice["mustard-rt run context-slice<br/>(guards + patterns do escopo)"]
    slice --> render["mustard-rt run agent-prompt-render<br/>--role {action} --subproject {scope}<br/>(NUNCA prompt à mão)"]
    render --> action{"action?"}

    action -->|analyze| a1["Task(Explore, sonnet) → report"]
    action -->|review| a2["Task(general-purpose, opus) → report"]
    action -->|docs| a3["Task(general-purpose, sonnet) → report"]
    action -->|audit| a4["load improve-codebase-architecture<br/>Task(general-purpose, sonnet) → report classificado"]
    action -->|compare| a5["1 explorer/subprojeto em PARALELO<br/>→ Task(Plan) funde + aponta divergências"]
    action -->|refactor| a6["Plan → print plano → AskUserQuestion<br/>→ implement (opus) → valida"]
    action -->|implement| a7["implement (sonnet, cap 30 linhas)<br/>agent roda build/type-check"]

    a4 --> sev["parse severidade → mapeia p/ /task refactor ou pipeline"]
    a5 --> sev
    a7 --> concern{"CONCERN?"}
    concern -->|sim| promote["oferece /feature Light"]
```

---

## `/review` — Revisão de Pull Request

Detecta o PR, invoca a revisão e reporta. ZERO confirmações.

| | |
|---|---|
| **Trigger** | `/review [pr-number-or-url]` |
| **Backend** | `review-prefetch` · `diff-context` · skill `code-review` (fallback Task opus) |
| **Provider** | `mustard.json#git.provider` (github/gitlab) |

```mermaid
flowchart TD
    start(["/review [pr]"]) --> resolve{"argumento?"}
    resolve -->|numérico/URL| ref["usa direto"]
    resolve -->|nenhum| detect["gh pr view --json (branch atual)"]
    detect --> noPR{"PR aberto?"}
    noPR -->|não| stop(["'No open PR found. Run /git merge first.'"])
    noPR -->|sim| ref

    ref --> prefetch["mustard-rt run review-prefetch (JSON)<br/>+ diff-context — fonte da verdade"]
    prefetch --> emit1["emit review.start"]
    emit1 --> invoke["cola diff como ## DIFF<br/>→ Skill(code-review)<br/>(fallback: Task general-purpose opus)"]
    invoke --> emit2["emit review.complete → resultados verbatim"]

    emit2 --> tf["Tactical-fix discovery:<br/>tactical-fix-detect → tactical_fix.proposed"]
    tf --> verdict{"verdito?"}
    verdict -->|APPROVED| done(["pronto"])
    verdict -->|REJECTED| fixloop["fix-loop normal (re-review em sonnet)"]
```

---

# Comandos de git e manutenção

## `/git` — Operações de git

Lê `mustard.json` para o fluxo de branches. Apenas operações **reversíveis** — nunca reescreve histórico ou apaga arquivos.

| Ação | Descrição |
|---|---|
| `sync` | Puxa a branch-pai para a atual (rebase) |
| `commit` | Cria commit (sem push); aceita `--scope=all\|staged\|<pattern>` |
| `push` | Sync, depois commit + push |
| `merge` | Sync + fast-forward para a pai (sempre até `dev`) |
| `merge main` | Cascata: branch → dev → main → volta à branch |

```mermaid
flowchart TD
    start(["/git &lt;action&gt;"]) --> s0["Step 0: resolve $PARENT do mustard.json"]
    s0 --> prot{"Step 0b: proteção de branch"}
    prot -->|"main / dev (commit/push/sync)"| refuse(["recusa"])
    prot -->|ok| sub["Step 0c: checa HEAD de submódulos (monorepo)"]

    sub --> action{"action?"}
    action -->|sync| sync["auto-stash → fetch + rebase origin/$PARENT → stash pop"]
    action -->|commit| commit["analisa → exclui efêmeros → resolve scope<br/>→ commit submódulos (paralelo) → commit pai"]
    action -->|push| push["sync (para em conflito) → commit + push submódulos → push pai"]
    action -->|merge| merge["sync → garante pushed → merge --ff-only → push → volta"]
    action -->|"merge main"| mergemain["se não em dev: merge antes<br/>→ dev → main (ff-only) → volta à origem"]

    sync --> report["Final Status Report"]
    commit --> report
    push --> report
    merge --> report
    mergemain --> report
    report --> done(["pronto — aborta em QUALQUER conflito"])
```

---

## `/maint` — Utilitários de manutenção

| Ação | Descrição |
|---|---|
| `deps` | Instala dependências de todos os subprojetos |
| `validate` | Build + type-check entre subprojetos |
| `sync` | `mustard-rt run scan` — refresca o `grain.model.json` |
| `doctor` | Health check da instalação (wiring, drift, state + OTEL) |

```mermaid
flowchart TD
    start(["/maint &lt;action&gt;"]) --> action{"action?"}
    action -->|deps| deps["lê pipeline-config § Agents<br/>→ instala deps (paralelo)"]
    action -->|validate| val["build + type-check (paralelo)"]
    action -->|sync| sync["mustard-rt run scan → grain.model.json"]
    action -->|doctor| doc["doctor (wiring+drift+state)<br/>+ doctor --residue<br/>+ diagnose-otel"]
    doc --> consol["relatório consolidado:<br/>wiring · drift · state-health · residue<br/>(OK / WARN / FAIL — nunca bloqueia)"]
```

---

## `/status` — Status consolidado

| | |
|---|---|
| **Trigger** | `/status [--harness]` |
| **Backend** | `mustard-rt run status --format table` |
| **Regra** | Sempre delega ao binário; `--harness` é estritamente read-only |

```mermaid
flowchart TD
    start(["/status [--harness]"]) --> mode{"--harness?"}
    mode -->|não| st["mustard-rt run status --format table<br/>(git · specs ativas/órfãs · build · entity registry)"]
    mode -->|sim| hn["mustard-rt run status --harness<br/>(lê settings.json, agrupa hooks por evento)"]
    st --> print["print verbatim"]
    hn --> print
    print --> orphan{"pipelines órfãos?"}
    orphan -->|sim| suggest["sugere /mustard:close ou /mustard:maint"]
```

---

## `/stats` — Métricas do pipeline

| | |
|---|---|
| **Trigger** | `/stats [--hooks] [--since] [--event] [--compare] [--pr] [--days]` |
| **Backend** | `metrics collect` (default) · `metrics report` (--hooks) · `event-projections --view pr-metrics` (--pr) |

```mermaid
flowchart TD
    start(["/stats [flags]"]) --> flag{"flag?"}
    flag -->|"(default)"| coll["metrics collect<br/>(superset: pipelines + hooks + RTK)"]
    flag -->|--hooks| hooks["metrics report --since/--event/--compare"]
    flag -->|--pr| pr["event-projections --view pr-metrics --wave {days}<br/>(DORA-style)"]
    coll --> print["print verbatim"]
    hooks --> print
    pr --> print
    print --> sections["Summary → Active/Orphaned → Completed<br/>→ Last 7 Days → Enforcement Events → RTK gain"]
```

---

## `/knowledge` — Gestão de conhecimento

Conhecimento = memória nativa do Claude Code (prosa durável) + eventos `decision`/`lesson` no NDJSON por spec (emitidos no CLOSE via `emit-event`).

| Ação | Backend / propósito |
|---|---|
| `list [spec]` | `event-projections --view pipeline-state` — decisions[]/lessons[] da spec |
| `search <term>` | MCP `search_knowledge` — match em title/detail dos eventos |
| `add` | interativo → `emit-event --event decision`/`lesson` |
| `notes [target]` | edita `notes.md` (nunca sobrescrito por `/scan`) |
| `audit` | compara memória nativa vs CLAUDE.md/skills (report-only) |
| `report <period>` | relatórios de progresso via git |

```mermaid
flowchart TD
    start(["/knowledge &lt;action&gt;"]) --> action{"action?"}
    action -->|list| list["event-projections --view pipeline-state<br/>(decisions[] / lessons[])"]
    action -->|search| search["MCP search_knowledge &lt;term&gt;"]
    action -->|add| add["interativo → emit-event decision/lesson"]
    action -->|notes| notes["edita {subproject}/.claude/commands/notes.md<br/>(injetado no contexto dos agentes)"]
    action -->|audit| audit["compara memória nativa vs CLAUDE.md/skills<br/>(report-only, nunca auto-edita)"]
    action -->|report| rep["relatórios git (refs/knowledge/report.md)"]
    list --> print["print verbatim (sempre mostra contagem)"]
    search --> print
    add --> print
```

---

# Skills

## `/skill` — Gerenciador de skills

| Ação | Backend |
|---|---|
| `install <name>` | manual — cópia para `.claude/skills/<name>/` (sem fetch embutido) |
| `create <name>` | skill `skill-creator` (interativo) |
| `list` | listagem manual de `.claude/skills/` (sem comando dedicado) |
| `remove <name>` | apaga `.claude/skills/{name}/` (avisa se `source: scan`) |
| `optimize / eval` | loops do `skill-creator` (requer Python 3 + `claude` CLI) |
| `update skill-creator` | sparse-clone `anthropics/skills` |

```mermaid
flowchart TD
    start(["/skill &lt;action&gt;"]) --> action{"action?"}
    action -->|install| inst["manual: copiar para .claude/skills/&lt;name&gt;/<br/>→ source: manual"]
    action -->|create| create["skill-creator (interativo) → source: manual"]
    action -->|list| list["listagem manual de .claude/skills/"]
    action -->|remove| remove{"source: manual?"}
    remove -->|sim| confirm["pede confirmação"]
    remove -->|scan| warn["avisa que é gerado por /scan"]
    action -->|"optimize/eval"| opt["skill-creator (Python 3 + claude CLI)"]
    action -->|update| upd["sparse-clone anthropics/skills"]
```

---

# Harness (liga/desliga dos hooks)

## `/unhook` — Kill-switch do harness

Desabilita os hooks renomeando `settings.json` para `settings.json.disabled-<timestamp>` e limpa estado volátil. Reversível via `/rehook`.

| Scope | O que toca |
|---|---|
| `this` | só `<repo>/.claude/settings.json` (default) |
| `monorepo` | `<repo>/.claude/` + todos `apps/*` e `packages/*` |
| `all` | monorepo + `~/.claude/settings.json` global (requer `--confirm`) |

```mermaid
flowchart TD
    start(["/mustard:unhook [--scope] [--confirm]"]) --> run["mustard-rt run unhook --scope"]
    run --> rename["renomeia settings.json → settings.json.disabled-&lt;ts&gt;<br/>limpa .agent-state/ · .cluster-cache.json · .worktrees/"]
    rename --> scopeChk{"scope all sem --confirm?"}
    scopeChk -->|sim| skip["global: state: skipped (não toca)"]
    scopeChk -->|não| apply["aplica no scope"]
    skip --> report
    apply --> report["print verbatim (state por entrada:<br/>disabled/missing/skipped/error)<br/>+ campo revert_with"]
    report --> done(["sugere /mustard:rehook --scope &lt;same&gt;"])
```

---

## `/rehook` — Restaurar o harness

Reverte o `/unhook`: acha o snapshot `settings.json.disabled*` mais recente em cada `.claude/` do escopo e renomeia de volta.

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
    state -->|disabled encontrado| restore["renomeia de volta → settings.json"]
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
| `/scan` | core | `scan` | **produz** |
| `/feature` | core | `feature`, `scan spec`, `spec-draft`, `wave-scaffold` | consome (digest) |
| `/bugfix` | core | `feature`, `qa-run`, `scan` | consome (digest) + refresca |
| `/spec` | core | `active-specs`, `resume-bootstrap`, `wave-advance` | indireto |
| `/qa` | core | `qa-run`, `tactical-fix-detect` | não |
| `/close` | core | `close-orchestrate` (+ `scan`) | refresca se mudou |
| `/tactical-fix` | core | `tactical-fix-create` | não |
| `/task` | delegação | `context-slice`, `agent-prompt-render` | indireto |
| `/review` | revisão | `review-prefetch`, `diff-context` | não |
| `/git` | git | (git nativo via `rtk`) | não |
| `/maint` | manutenção | `scan`, `doctor`, `diagnose-otel` | refresca (sync) |
| `/status` | observabilidade | `status` | não |
| `/stats` | observabilidade | `metrics collect/report`, `event-projections` | não |
| `/knowledge` | conhecimento | `memory list/search/knowledge` | não |
| `/skill` | skills | manual (copiar para `.claude/skills/`; sem backend `run`) | não |
| `/unhook` | harness | `unhook` | não |
| `/rehook` | harness | `rehook` | não |

---

*Gerado a partir dos comandos do plugin em `plugin/commands/`. Quando um fluxo mudar, re-derive deste diretório — ele é a fonte da verdade.*
