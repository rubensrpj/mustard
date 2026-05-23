# /mustard:spec — Approve-only flow

Loaded on demand by `commands/mustard/spec/SKILL.md` Step 7 quando a spec selecionada estiver em estágio PLAN (planejar). Conteúdo movido **verbatim** da antiga `commands/mustard/approve/SKILL.md` (deletada na TF `2026-05-23-tf-unify-spec-command`), com ajustes mínimos de costura para o novo entry-point.

## Description

Aprova a spec ativa selecionada pelo picker e prepara a fase de implementação.

Uma spec tem duas camadas nomeadas (ver `/feature` § Full Scope): `## PRD` — o *o quê & porquê* (intent) — e `## Plano` — o *como* (execução). Aprovar uma spec aprova **ambas as camadas de uma vez**: não há um portão separado para "aprovar PRD". A separação em duas camadas é uma ajuda de leitura, não um segundo checkpoint — mantenha assim.

- **Sem sufixo `r`** (`/mustard:spec {letra}` com estágio PLAN): prepara o estado do pipeline e PARA, instruindo o usuário a abrir nova sessão e rodar `/mustard:spec {letra}` novamente para continuar com contexto limpo. Recomendado para specs Full-scope (escopo cheio) com 5+ arquivos.
- **Com sufixo `r`** (`/mustard:spec {letra}r`): após a preparação, salta imediatamente para o fluxo `resume-flow.md` na mesma sessão (pula Step 0 e Step 1 do resume — sem check de falha de dispatch, sem handoff summary, sem reconfirmação). Use quando a spec acabou de ser aprovada e você quer evitar o hop de reiniciar sessão. Tradeoff: a fase EXECUTE herda o contexto ANALYZE+PLAN em vez de começar limpa — ok para specs pequenas/médias, menos eficiente para grandes.

## Prerequisites

- Spec ativa em `.claude/spec/{name}/` (layout flat — status lido do header da spec / projection do SQLite — banco de eventos)
- Spec foi apresentada ao usuário e ele escolheu a letra correspondente no picker do `/mustard:spec`

## Action

1. **Step 0: AUTO-SYNC (obrigatório)** — já rodado no Step 1 do `/mustard:spec`. Não re-executar.
2. **Read** `.claude/pipeline-config.md` — agents, model selection (seleção de modelo).
3. A spec já foi localizada pelo picker do `/mustard:spec` (filtrada por `### Stage:` + `### Outcome:` — só `Outcome: Active` AND `Stage ∈ {Plan, Execute}`).

### Step 3b: Wave Plan Detection (detecção de plano de wave)

Cheque se a spec localizada é um plano de wave: procure `.claude/spec/{specName}/wave-plan.md`.

**Se `wave-plan.md` existe:**

1. Carregue o estado do pipeline derivado do log de eventos SQLite (rode `mustard-rt run event-projections --view pipeline-state --spec {specName}` para obter o snapshot atual) — esperar `isWavePlan: true`, `totalWaves: N`, `currentWave: 1`, `completedWaves: []`.
2. Leia `wave-plan.md` e imprima o conteúdo INTEIRO dentro de um bloco markdown fenced (```` ```markdown ... ``` ````). Liste cada caminho de arquivo de wave-spec abaixo do bloco (uma linha cada).
2b. **Wave size audit (auditoria de tamanho da wave — apenas avisa):** rode `mustard-rt run wave-size-check --spec-dir .claude/spec/{specName}`.
   - Se o resultado for `action: "audited"` e `oversizedCount > 0`, imprima um bloco de aviso listando cada wave grande demais:
     `⚠ Wave {N} ({folder}) — {fileCount} arquivos, {layerCount} camada(s) — considere dividir ({reason})`
   - Diga explicitamente que isso é **avisativo** — NÃO bloqueia aprovação. Informa a opção **"Stop — re-plan with guidance"** do próximo `AskUserQuestion`: uma wave grande demais pode ser dividida antes do EXECUTE.
   - Se `oversizedCount === 0` ou `action: "skip"`, não imprima nada (silencioso).
3. `AskUserQuestion`:
   - **"Approve wave plan — start with wave 1"** → seguir para step 4 (atualiza header + state para dispatch da wave 1)
   - **"Reject decomposition — use single spec"** → unir todas as wave specs de volta em uma spec única em `.claude/spec/{specName}/spec.md` (concatenar `## Files`, `## Tasks`, `## Boundaries` de cada wave), deletar `wave-plan.md` e os subdirs `wave-N-*/`, setar `scopeOverride: "user-rejected-waves"` e `isWavePlan: false` no pipeline state, seguir para step 4 na spec única
   - **"Stop — re-plan with guidance"** → parar. Instruir usuário: `Delete .claude/spec/{specName}/ and re-run /feature {name} with explicit guidance (e.g., "keep wave 2 and wave 3 together").`
4. Se aprovou o wave plan, do step 4 em diante opere sobre a **wave 1 spec** (`.claude/spec/{specName}/wave-1-{role}/spec.md`) — atualize o header dela, não o do `wave-plan.md`.

**Se `wave-plan.md` NÃO existe:** seguir como spec única (comportamento abaixo).

4. **Spec Checkpoint — atualizar header da spec:**
   - `### Stage: Plan`
   - `### Outcome: Active`
   - `### Flags:`
   - `### Checkpoint: {ISO timestamp now}`
   (preservar linhas existentes `### Scope:`, `### Lang:`, `### Parent:`)
5. **Pipeline State — emitir transição de stage para Plan:**
   - Extrair `spec-name` do diretório da spec (ex.: basename do path → `2026-02-26-linked-services-card`)
   ```bash
   mustard-rt run emit-pipeline --kind pipeline.stage --spec {spec-name} --payload "{\"stage\":\"Plan\"}"
   mustard-rt run emit-pipeline --kind pipeline.status --spec {spec-name} --payload "{\"from\":\"draft\",\"to\":\"approved\"}"
   ```
   - Nenhum arquivo JSON é escrito aqui.
5b. **Memory Persist — registrar decisões arquiteturais:**
   - Para cada decisão significativa na spec (escolhas de tecnologia, padrões de design, trade-offs):
     ```bash
     echo '{"type":"decision","content":"<decision description>","source":"<spec-name>","context":"approved at PLAN phase"}' | mustard-rt run memory decision
     ```
   - Focar em: por que um padrão foi escolhido em vez de alternativas, restrições que moldaram o design
   - Pular decisões triviais ou óbvias (máx 3 entradas)
6. **Model selection (seleção de modelo)** — ler `Model Selection` de `.claude/pipeline-config.md` e registrar campo `"model"` no state:
   - Contar arquivos totais estimados na spec
   - Aplicar regra: ≤5 arquivos/padrões conhecidos → `"model": "sonnet"`, 5+ arquivos/padrões novos → `"model": "opus"`
7. **Task Tracking — criar TaskCreate para cada agente:**
   - 1 TaskCreate por agente identificado na spec
   - Subject: `"{Layer}: {brief description}"`
   - activeForm: `"Running {Layer} agent"`
8. **Output — feedback visual:**
   - Imprimir progress line: `[v] ANALYZE  [v] PLAN  [>] EXECUTE  [ ] CLOSE`
   - Imprimir uma linha de sinal de camada para o usuário saber o que foi aprovado:
     `Aprovado: camada PRD (o quê & porquê) + camada Plano (o como).` (Lang=en: `Approved: PRD layer (what & why) + Plano layer (how).`)
9. **Branch por sufixo `r`:**

   **Sem `r` (default) — PARAR e instruir usuário a abrir nova sessão:**
   - Não executar implementação nesta sessão (contexto já consumido por /feature + picker)
   - Output final:

     ```
     Spec approved and pipeline prepared.
     Open a new session and run /mustard:spec to start implementation with clean context.
     ```

   - **CRÍTICO**: NÃO dispatch Task agent, NÃO implementar código — apenas PARAR

   **Com `r` — salta para o fluxo de resume na mesma sessão:**
   - Informar usuário: `Spec approved. Resuming inline (sufixo r). Dispatching EXECUTE directly.`
   - Saltar para `resume-flow.md` **Step 2: Bootstrap**
   - **PULAR** Step 0 (Dispatch Failure Pre-Check — não se aplica, state foi criado acima) e Step 1 (Detect & Confirm — spec já é conhecida, usuário acabou de aprovar)
   - Do Step 2 em diante, seguir o fluxo completo do resume: AUTO-SYNC → Diff Context → Wave System → VALIDATE → REVIEW → QA → CLOSE
   - Aplicar todas as INVIOLABLE RULES do resume (main context IS the Pipeline Runner, wave dispatch in single message, etc.)

## Alternative Flow

Se a spec não está satisfatória:
- Forneça feedback textual para ajustes
- Use /mustard:close para cancelar
