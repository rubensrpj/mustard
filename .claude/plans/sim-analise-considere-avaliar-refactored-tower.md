# Plano: simplificar `/resume` e `/bugfix`, decomposição em ondas em `/feature`

## Contexto

Métricas do projeto sialia (40 pipelines, 808 eventos hook) revelaram dois problemas concretos:

1. **Pipeline monstro repetitiva** — `tenant-credit-ledger-downgrade` consumiu 14h, 309 API calls, 45 retries em PLAN. O padrão `heavy-pipeline` já estava capturado no `knowledge.json` desde 09/04. Mustard **observou** mas não **interveio**.
2. **Processo burocrático** — `/resume` re-analisa tudo sempre (registry + spec + diffs + memory + possível Explore Gate dispatch), mesmo quando o usuário só quer continuar de onde parou. `/bugfix` Fast Path re-DIAGNOSE em retry loop mesmo quando a causa raiz já foi identificada.

Exploração confirmou:

- `/resume` (`templates/commands/mustard/resume/SKILL.md:85`) roda `sync-registry` + pode disparar Explore Gate (Haiku, ≤10 tool uses) a cada invocação. Tokens reais em jogo: ~2-5k por resume desnecessariamente pesado.
- `/bugfix` (`templates/commands/mustard/bugfix/SKILL.md`) redispatcha Explore em retry loop mesmo com diff inalterado nos arquivos afetados. ~3-8k tokens por retry redundante.
- `/approve` recompila registry sempre, **mas** o ganho de pular é marginal (compute overhead, não tokens de modelo) e o risco de registry velho é real. Não vale a troca.
- `/review` já é mínimo (delega pra `code-review`, stateless). Nada a fazer.
- **Wave decomposition em specs pesadas** é a alavanca grande: uma pipeline monstro evitada vale 50-150k tokens. Economia de 2 ordens de magnitude maior que slim-downs de script.

Resultado esperado: retomadas idle ficam ~2-5k tokens mais leves; bugfix em retry economiza ~3-8k; pipelines que hoje caem no padrão `heavy-pipeline` são decompostas automaticamente em ondas sequenciais.

---

## Revisões vs primeira versão do plano

Após pressão crítica do usuário:

- **Removido:** A.1 (skip `sync-registry` em `/approve`). Ganho em tokens de modelo é ~zero; risco de registry velho é real. Não vale.
- **Removido:** heurística de `git diff` pra decidir modo em `/resume`. No meio de pipeline sempre há diff — o caso "zero diff" é estreito demais pra ser útil. Substituído por prompt explícito.
- **Elevado:** Fase B (waves) de "prioridade média" pra **prioridade principal**. É onde a economia de verdade mora.

---

## Mudanças

### A.1 — `/resume`: modo "continuar" vs "reanalisar" via prompt explícito

**Arquivo:** `templates/commands/mustard/resume/SKILL.md`

Passo 0 novo, antes de qualquer leitura pesada:

1. Ler `.claude/.pipeline-states/{spec}.json` (único arquivo — <1k tokens).
2. Se `lastDispatchFailure` existe e <10min: **forçar modo reanalisar** (cenário de recovery já merece o custo, ignora prompt).
3. Caso contrário, prompt inline:
   ```
   Pipeline {spec} pausada em {phaseName} (onda {N}/{total}).
   Continuar de onde parou ou reanalisar contexto? [C/r]
   ```
   - `C` (default) → modo `continuar`: pula `sync-registry`, pula `diff-context`, pula Explore Gate. Vai direto pra próximo agente/onda pendente usando `pipeline-state` como fonte de verdade.
   - `r` → comportamento atual (relê tudo).

**Fallback de segurança:** se em modo `continuar` o próximo agente dispatchado retornar erro de contexto stale (ex.: registry desatualizado, spec referenciando arquivo inexistente), `/resume` escala automaticamente pra modo `reanalisar` e redispatcha. Fail-open, com log explícito em `pipeline-state.resumeMode: "continued" | "escalated-to-reanalyze"`.

**Economia real:** ~2-5k tokens quando Explore Gate é pulado. Zero quando usuário escolhe `r` (intencional).

### A.2 — `/bugfix` Fast Path: cache de root-cause em retry loop

**Arquivo:** `templates/commands/mustard/bugfix/SKILL.md`

No fim do DIAGNOSE, gravar em `pipeline-state.{spec}.json`:
- `rootCauseHash`: SHA256 de `{descrição canônica do bug} + {sorted array de arquivos afetados}`
- `rootCauseSummary`: resumo curto (<500 chars) pro fix-loop consumir sem re-diagnosticar
- `affectedFilesHash`: SHA256 do conteúdo atual dos arquivos afetados (pra invalidar cache se mudaram)

Em fix-loop (após REJECTED), antes de re-DIAGNOSE:
1. Recomputar `affectedFilesHash` dos mesmos arquivos.
2. Se bate com cache → pular DIAGNOSE, injetar `rootCauseSummary` direto no prompt do FIX. Log: `root-cause cached (retry {N}/2), skipping diagnose`.
3. Se não bate (arquivos mudaram) → invalidar cache, rodar DIAGNOSE normal.

**Salvaguarda contra bugs com sintoma igual mas causa diferente:** o hash inclui descrição do bug + arquivos. Se o REVIEW rejeita com feedback que sugere causa diferente (parser de `decision: "changes_requested"` com `rationale`), invalidar cache mesmo se arquivos não mudaram.

**Limite:** cache vale por no máximo 2 retries (alinhado com `max-retries` atual do fix-loop).

**Economia real:** ~3-8k tokens por retry evitado. Com ~1-2 retries/bug típicos, ~3-15k por bugfix pesado.

### B — Decomposição em ondas em `/feature` (PRINCIPAL)

**Arquivos:**
- `templates/commands/mustard/feature/SKILL.md` — novo passo pós-PLAN
- `templates/commands/mustard/approve/SKILL.md` — handling de wave plan
- `templates/commands/mustard/resume/SKILL.md` — execução onda por onda
- `templates/scripts/scope-decompose.js` — novo
- `templates/scripts/wave-dependency.js` — novo (grafo de dependência)

#### B.1 — PLAN detecta escopo pesado

No fim do PLAN do `/feature`, antes de escrever o spec final:

1. Computar sinais que o PLAN já tem:
   - `fileCount` — arquivos em `## Files`
   - `layerCount` — camadas distintas (derivadas de `sync-detect.js` role detection — agnóstico, não hardcode)
   - `newEntityCount` — entidades novas
   - `estimatedTouchPoints` — imports/refs cruzados (via Grep)
2. Ler `.claude/knowledge.json`, extrair entradas `heavy-pipeline`, `high-hook-retry`.
3. Rodar `scope-decompose.js` com esses sinais.

**`scope-decompose.js`:**
- Input (stdin JSON): `{fileCount, layerCount, newEntityCount, estimatedTouchPoints, knowledgeMatches}`
- Regras (agnósticas, baseadas em dados do próprio projeto — sem hardcode):
  - Sem match histórico + `fileCount ≤ 5` → `{decompose: false}`
  - Match histórico relevante **OU** `layerCount ≥ 3` **OU** `fileCount > 10` com `newEntityCount ≥ 2` → `{decompose: true, reason}`
- Output (stdout JSON): `{decompose: bool, reason: string, signals: {...}}`
- Fail-open: em erro, `{decompose: false}` (mantém fluxo atual).

#### B.2 — Grafo de dependência e corte

**`wave-dependency.js`:**
- Input: lista de arquivos + subprojeto detectado
- Usa `sync-detect.js` roles + análise de imports/refs (via Grep em `import`, `require`, `using`, `from`) pra construir DAG direcionado
- Agrupa arquivos em ondas por ordenação topológica: cada onda contém arquivos que só dependem de ondas anteriores
- **Número de ondas = profundidade topológica do DAG** — não é constante, deriva da estrutura real do código
- Output: `[{wave: 1, files: [...], roles: [...]}, {wave: 2, files: [...], dependsOn: [1]}, ...]` com N variável

**Casos limite:**
- DAG com 1 nível (arquivos sem dependência cruzada) → retorna 1 onda. `scope-decompose.js` descarta decomposição nesse caso (`{decompose: false}`) — não adianta criar wave plan com onda única.
- DAG circular (ciclo de imports) → retorna `{error: "cyclic-dependency"}`. Escopo volta pra single spec com warning pro usuário investigar o ciclo (problema de arquitetura preexistente).
- DAG muito largo (uma onda com 20+ arquivos) → warning, mas não bloqueia. Usuário decide no `/approve` se quer aceitar ou pedir edição (opção `e`).

#### B.3 — Estrutura de arquivos do wave plan

```
.claude/spec/active/{date}-{name}/
  ├── wave-plan.md                    # visão geral do plano
  ├── wave-1-{role}/spec.md           # onda 1 (sem dependência)
  ├── wave-2-{role}/spec.md           # onda 2 (dependsOn: wave-1)
  └── wave-N-{role}/spec.md           # N depende da profundidade do DAG
```

N é variável — pode ser 2, 3, 4, ou mais, conforme a estrutura de dependências do código tocado pela spec.

`wave-plan.md` contém:
- Resumo do que o PLAN original ia fazer
- Lista das ondas com arquivos, roles, dependências
- Rationale da decomposição (qual entrada do knowledge disparou, quais sinais)

Cada `wave-N/spec.md` é uma spec completa e atômica, referenciando `../wave-plan.md` como contexto.

`pipeline-state.{date-name}.json` tem novos campos:
- `isWavePlan: true`
- `currentWave: 1`
- `totalWaves: 3`
- `completedWaves: []`
- `failedWaves: []`

#### B.4 — `/approve` handling de wave plan

**Novo comportamento:**

1. Se spec ativa tem `wave-plan.md` → exibir plano inteiro (todas as ondas com preview de arquivos).
2. Usuário responde:
   - `a` — aprovar plano inteiro (EXECUTE começa pela wave 1).
   - `r` — rejeitar decomposição → PLAN vira single spec, flag `scopeOverride: user-rejected-waves` no pipeline-state. Fluxo continua como hoje.
   - `e` — editar (ex.: "junte wave 2 e 3") → PLAN reexecuta com hint do usuário. **Evita o risco principal do corte ruim.**
3. Se aprovado, `pipeline-state.currentWave = 1`, `status = "approved"`, dispatch da wave 1.

#### B.5 — `/resume` execução onda por onda

Quando `pipeline-state.isWavePlan === true`:

1. EXECUTE dispatcha só a wave atual (`currentWave`).
2. Ao terminar uma wave:
   - Commit automático via `/mustard:git commit` (já existe) com mensagem `feat(wave-N/{role}): ...`.
   - `completedWaves.push(currentWave)`, `currentWave++`.
   - Atualizar pipeline-state.
3. Próxima iteração do EXECUTE: carrega só o contexto da próxima wave (specs das ondas já completas ficam como "pano de fundo" — reference, não re-execução).
4. Entre ondas: contexto reset (session limpa) mas pipeline-state persiste — próxima wave começa com contexto enxuto.
5. CLOSE só depois que `completedWaves.length === totalWaves`.

#### B.6 — Tratamento de falha parcial

**Cenário:** wave 1 commitou, wave 2 quebra (REJECTED após 2 retries).

Comportamento:
- Wave 1 **fica commitada** (não é rollback — é progresso real).
- `pipeline-state.failedWaves.push(2)`, `status = "failed"`.
- Usuário roda `/mustard:resume` → detecta estado `failed`, prompt:
  ```
  Wave 2 falhou após 2 retries. Opções:
  [f] corrigir wave 2 manualmente e retomar
  [r] reescrever wave 2 (re-PLAN dessa wave só)
  [a] abortar pipeline (waves 1 commitadas, 2-3 abandonadas)
  ```
- Log completo do fail em `wave-2-{role}/failure.md` pra debug.

**Risco residual:** wave 1 commit pode estar incompleto semanticamente sem wave 2 (ex.: schema criado mas API não). Documentar claramente na UI do `/approve` que "ondas são incrementais — wave N sozinha pode não ser deployable".

**Economia real:** se uma pipeline monstro histórica (309 calls, 14h) for decomposta em N ondas menores (N derivado do DAG), mesmo se todas forem executadas sequencialmente, o custo total pode subir 10-20% em tokens brutos por conta do overhead de commits e context resets entre ondas. **Ganho vem quando decomposição evita retries** — cada onda tem surface area menor, PLAN oscila menos, REVIEW rejeita menos. Estimativa conservadora: 30-50% de economia líquida em pipelines que hoje seriam monstro, após compensar o overhead.

---

## Arquivos críticos

| Arquivo | Mudança |
|---------|---------|
| `templates/commands/mustard/resume/SKILL.md` | A.1 — prompt continuar/reanalisar + B.5 execução por onda + B.6 failure handling |
| `templates/commands/mustard/bugfix/SKILL.md` | A.2 — cache de root-cause |
| `templates/commands/mustard/feature/SKILL.md` | B.1 — scope-decompose pós-PLAN |
| `templates/commands/mustard/approve/SKILL.md` | B.4 — handling de wave plan (a/r/e) |
| `templates/scripts/scope-decompose.js` | **novo** — decide se decompor |
| `templates/scripts/wave-dependency.js` | **novo** — grafo + ordenação topológica |

## Utilidades existentes a reusar

- `templates/scripts/sync-detect.js` (linhas 1048-1063) — role detection já existe, alimenta `wave-dependency.js` sem duplicar.
- `.claude/.pipeline-states/{spec}.json` — estrutura atual suporta extensão; só adicionar campos novos.
- `templates/scripts/memory-persist.js` — gravação de agent memory por onda reusa infra existente.
- `.claude/knowledge.json` — consumido por `scope-decompose.js` sem nova estrutura.
- `/mustard:git commit` — usado entre ondas sem alteração.
- `rtk` — já cobre tudo.

## Verificação

### Teste automatizado

```bash
node --test templates/hooks/__tests__/hooks.test.js
npm run build
```

### Teste manual end-to-end

1. **A.1** — retomar pipeline pausada com `/mustard:resume`.
   - Esperado: prompt aparece. Escolher `C` → pula Explore Gate, resume completa em <10s.
   - Escolher `r` → comportamento atual preservado.
   - Simular `lastDispatchFailure` <10min → prompt é pulado, entra em reanalisar direto.

2. **A.2** — criar bug intencional, deixar REVIEW falhar.
   - Esperado: 2º ciclo do fix-loop pula DIAGNOSE com log `root-cause cached`.
   - Alterar arquivo afetado antes do retry → cache invalida, DIAGNOSE roda.
   - REVIEW rejeitar com rationale sugerindo causa diferente → cache invalida.

3. **B** — criar spec que toca 3+ camadas com 2+ entidades novas.
   - Esperado: PLAN gera `wave-plan.md` + N sub-specs com `dependsOn` (N deriva do DAG — pode ser 2, 3, 4+).
   - `/mustard:approve` exibe plano completo, aceitar → wave 1 roda.
   - Após wave 1 completar: commit automático, `currentWave++`.
   - Simular falha numa wave intermediária → prompt de failure aparece com [f/r/a].
   - Testar caso de DAG raso (1 nível): decomposição é descartada, spec única preservada.
   - Testar caso de DAG com ciclo: warning exibido, fluxo volta pra single spec.

4. **B controle** — criar spec simples (1 camada, 2 arquivos).
   - Esperado: `scope-decompose.js` retorna `{decompose: false}`. Fluxo single-spec preservado.

### Medição de tokens

Antes vs depois via `/mustard:stats`:
- Baseline: rodar `/resume` em pipeline pausada (atual).
- Pós A.1: mesmo cenário com modo `continuar` — esperar ~2-5k tokens a menos.
- Baseline bugfix: gerar bug que dispara 2 retries.
- Pós A.2: mesmo bug — 2º retry economiza ~3-8k.
- Baseline B: reproduzir cenário análogo ao `tenant-credit-ledger` (3+ camadas, 2+ entidades).
- Pós B: esperar decomposição em N ondas (N derivado do DAG) com total de tokens 30-50% menor que monólito equivalente.

## Escopo fora deste plano

- **Não** consolidar `/feature + /approve + /resume` em comando único (usuário rejeitou).
- **Não** mexer em `/review` (já é leve, stateless).
- **Não** criar novo hook de advisory pra knowledge no `/approve` (decomposição automática na Fase B substitui isso).
- **Não** pular `sync-registry` em `/approve` (ganho marginal, risco real).
- **Não** mudar comportamento default do `/resume` — usuário escolhe no prompt. Default `continuar` mas reversível.
