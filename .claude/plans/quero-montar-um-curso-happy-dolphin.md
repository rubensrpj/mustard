# Curso Mustard — Guia Completo de Uso

> Documento de referência prática para ensinar Mustard a desenvolvedores. Cada comando: o que faz, quando usar, como usar, exemplos, armadilhas, dicas.

---

## Contexto

Mustard é um CLI que transforma Claude Code em um pipeline estruturado de desenvolvimento (ANALYZE → PLAN → EXECUTE → QA → CLOSE), com 17 slash-commands, 23 hooks de enforcement, 6 skills bundled e economia de tokens via RTK. Este curso cobre o uso prático: do primeiro `mustard init` até pipelines maduros com QA gates.

**Pré-requisitos do aluno:** Node.js ≥18, Claude Code CLI ou IDE, conhecimento básico de Git.

**Filosofia:** o orquestrador (sessão principal) não implementa código — ele delega via Task. Toda escrita de código acontece em subagentes com contexto isolado.

---

## Módulo 1 — Setup & Fundamentos

### 1.1 Instalação

```bash
npm install -g mustard-claude
cd meu-projeto
mustard init
```

`mustard init` copia `.claude/` para o projeto, instala RTK automaticamente e gera `mustard.json`. **É one-shot** — toda inteligência subsequente vive em hooks/skills.

**Atualização:**
- `mustard update` — preserva `CLAUDE.md`, `prompts/`, `context/*.md`, `mustard.json`
- `mustard auto-update` — modo não-interativo

**Armadilha:** rode `mustard init` numa pasta com Git já inicializado. Se rodar fora, `sync-detect.js` falha em descobrir subprojetos.

### 1.2 Primeira execução: `/scan`

Após `mustard init`, abra Claude Code e rode `/scan`. É o passo de bootstrap — sem ele, os pipelines não têm registry para validar entidades.

```
/scan                 → escaneia tudo (incremental por hash)
/scan <subproject>    → só um subprojeto (ex: /scan api)
/scan --force         → ignora cache, regenera tudo
```

**O que `/scan` produz:**
- `entity-registry.json` (entidades, enums, padrões)
- `commands/{stack,patterns,guards,recipes,notes}.md`
- Skills auto-geradas em `.claude/skills/{nome}/`
- Agents `{name}-impl.md` e `{name}-explorer.md`

**Dica:** rode `/scan --force` após mudanças grandes de schema/arquitetura. No dia a dia, o incremental basta.

**Armadilha:** se você não rodar `/scan` antes do primeiro `/feature`, o hook `enforce-registry.js` bloqueia o pipeline.

### 1.3 `mustard.json`

Configuração do git flow e provider. Estrutura típica:

```json
{
  "git": { "provider": "github", "main": "main", "dev": "dev" },
  "subprojects": [...]
}
```

Pode editar manualmente. Lido por `/git`.

---

## Módulo 2 — Pipeline Feature

### 2.1 Conceito

`/feature <nome>` é o pipeline padrão para **qualquer mudança em código de produção** (schema, API, UI, novo CRUD, novo endpoint). O pipeline detecta o escopo automaticamente.

**Escopo Light** — pula PLAN:
- 1-2 layers, ≤5 arquivos, padrão já conhecido no registry
- Fluxo: ANALYZE → EXECUTE → QA → CLOSE

**Escopo Extended Light:**
- Entidade já existe no registry, modificação ≤8 arquivos

**Escopo Full** — exige PLAN + `/approve`:
- 3+ layers, nova entidade, novo padrão
- Fluxo: ANALYZE → PLAN → `/approve` → EXECUTE → QA → CLOSE

### 2.2 Uso passo a passo

```
/feature linked-services-card
```

1. **SPEC HYGIENE** (auto) — audita specs em `active/`
2. **ANALYZE** — auto-sync, detecta layers afetadas (DB/Backend/Frontend)
3. **PLAN** (Full apenas) — gera spec com `## Acceptance Criteria`
4. Você revisa → `/approve` (ou `/approve --resume` para Light)
5. **EXECUTE** — Task agents implementam por wave
6. **QA** — executa cada AC; bloqueia CLOSE se algum falhar
7. **CLOSE** — fecha pipeline, move spec para `completed/`

### 2.3 Exemplos práticos

```
/feature add-email-field-to-user           → Light (1 campo, 2 files)
/feature notifications-module              → Full (nova entidade)
/feature fix-pagination-on-orders-list     → Light (UI ajuste)
/feature multi-tenant-billing              → Full (cross-cutting)
```

### 2.4 Acceptance Criteria

Toda spec Full deve ter 3-8 AC com comando executável:

```markdown
## Acceptance Criteria
- [ ] AC-1: build passes — Command: `npm run build`
- [ ] AC-2: e-mail field saved — Command: `node scripts/test-email.js`
- [ ] AC-3: type-check clean — Command: `npm run typecheck`
```

**Armadilha Windows:** AC rodam via `cmd.exe`. Evite `for/test/$()/[ ]` — quebram. Use `node -e "..."` ou `bash -c '...'`.

### 2.5 Dicas

- **Nomes em kebab-case** descritivos. Evite `/feature fix-stuff`.
- **Uma feature, um pipeline.** Não amontoe escopos.
- **Confie no auto-detect.** Não force Full em coisas pequenas — desperdiça tokens.

### 2.6 Armadilhas

- Rodar `/feature` em branch errada (main/master): `/git` recusa commits depois. Use `dev_<seu-nome>`.
- Esquecer `/scan` inicial → registry vazio → enforcement bloqueia.
- Não responder `/approve` em Full scope → pipeline trava em PLAN.

---

## Módulo 3 — Pipeline Bugfix

### 3.1 Conceito

`/bugfix <descrição>` faz diagnóstico e correção autônoma. **Zero context-switch** — não pergunta nada, descobre via Grep/Glob.

### 3.2 Uso

```
/bugfix Login button não responde no mobile
/bugfix NullReferenceException em OrderService.Calculate
/bugfix Migration 042 quebra em ambiente staging
```

### 3.3 Fases

1. **SPEC HYGIENE** — audita specs ativas
2. **DIAGNOSE** — Explore agent localiza root cause (orçamento ≤20 tool uses)
3. **FIX** — implementação iterativa com cache de root cause (retry inteligente)
4. **QA** — valida correção
5. **CLOSE** — finaliza

### 3.4 Dicas

- **Descreva o sintoma**, não a solução. Bug bom: "carrinho zera ao trocar de aba". Bug ruim: "adicione localStorage no carrinho".
- Inclua **mensagem de erro literal** quando houver — o cache de root cause funciona melhor.

### 3.5 Armadilhas

- `/bugfix` esgotando 20 tool uses em DIAGNOSE: o bug está mal descrito ou é multi-causal. Reformule.
- Tentar usar `/bugfix` para refactor: use `/task refactor` em vez disso.

---

## Módulo 4 — Aprovação, Resumo e Finalização

### 4.1 `/approve`

Aprova spec após PLAN. Dois modos:

```
/approve              → instrui /resume em sessão nova (Full)
/approve --resume     → executa EXECUTE na mesma sessão (Light)
```

Atualiza header da spec (`Status: approved`, `Phase: PLAN`), cria `.pipeline-states/{spec}.json`, captura decisões em memória.

**Armadilha:** `/approve` sem `--resume` em Light desperdiça uma sessão inteira de contexto.

### 4.2 `/resume`

Retoma pipeline interrompido. Pergunta:
- **Continuar (leve)** — reusa pipeline-state, pula sync-registry
- **Reanalisar (completo)** — sync-registry + diff-context fresh

Auto-recupera dispatch failures recentes (<10min).

**Quando usar:**
- Após `/approve` em nova sessão (Full)
- Sessão derrubada no meio de EXECUTE
- Crash do Claude Code

### 4.3 `/complete`

Fecha o pipeline com checklist obrigatório:
1. Review APPROVED
2. Build verde
3. Mudanças batem com spec
4. Zero CRITICAL issues
5. Sem regressões

Move spec `active/` → `completed/`, captura patterns para knowledge base.

**Armadilha:** `/complete` antes de `/qa` passar é bloqueado pelo `close-gate.js` (modo strict por default).

### 4.4 `/qa`

Executa Acceptance Criteria manualmente:

```
/qa                            → spec mais recente
/qa --spec linked-services     → spec específica
```

Retry limit: 3 tentativas. Falha após isso → intervenção manual. Controle por env: `MUSTARD_QA_GATE_MODE=strict|warn|off`.

---

## Módulo 5 — Tasks Delegadas (sem pipeline)

`/task` para análise, refactor, docs e auditorias rápidas — não exige pipeline.

| Action | Modelo | Quando |
|--------|--------|--------|
| `/task analyze <escopo>` | Haiku/Explore | Exploração rápida + padrões |
| `/task audit <escopo>` | Sonnet | QA com checklist (a11y, copy, design, i18n) |
| `/task review <escopo>` | Opus | Code quality (SOLID, security, perf) |
| `/task refactor <escopo>` | Sonnet/Opus | Plan → approve → implement |
| `/task docs <escopo>` | Sonnet | Geração de docs |
| `/task implement <escopo>` | Sonnet | Implementação cirúrgica + build |
| `/task compare <a> <b>` | Haiku+Sonnet | Cross-subproject |

**Exemplos:**

```
/task analyze "uso de useEffect em components/"
/task audit "fluxo de checkout"
/task review src/services/billing/
/task refactor "extrair PaymentGateway de OrderService"
/task docs "API pública do módulo auth"
```

**Dica:** `/task refactor` sempre apresenta o plan verbatim para você aprovar antes de implementar. Não pula etapa.

**Armadilha:** usar `/task implement` para mudança cross-layer. Vira pipeline `/feature` por baixo dos panos com menos governança. Prefira `/feature`.

---

## Módulo 6 — Git Operations

`/git <action>` — opera em cima de `mustard.json`.

| Comando | Efeito |
|---------|--------|
| `/git sync` | Pull rebase do parent na branch atual |
| `/git commit` | Commit (sem push) |
| `/git push` | sync + commit + push |
| `/git merge` | Promove ff-only para parent local |
| `/git merge main` | Cascade: branch → dev → main → branch |

**Regras críticas:**
- Recusa commit/push em `main` direto. Recusa em `dev` exceto `merge main`.
- History sempre linear (sem merge commits).
- Stashes preexistentes preservados (sentinel).
- **Caminhos efêmeros nunca commitados:** `.claude/.agent-state/`, `.claude/.metrics/`, `.claude/.pipeline-states/`, `.claude/.detect-cache.json`, `.claude/.knowledge-seen.json`.

**Fluxo recomendado (memória do projeto):** trabalhe sempre em `dev_<seu-nome>`; use `/git merge main` para propagar tudo de uma vez.

**Armadilha:** rodar `git push --force` direto sem `/git` — bloqueado pelo `permissions.deny` do `settings.json`.

---

## Módulo 7 — Knowledge & Memória

### 7.1 `/knowledge`

Gerencia `.claude/knowledge.json` — patterns, conventions, entities capturadas pelo pipeline.

```
/knowledge list                       → tudo agrupado por tipo
/knowledge search <termo>             → busca em name/description/tags
/knowledge add                        → adiciona interativamente
/knowledge notes [target]             → observações livres
/knowledge audit                      → detecta duplicatas
/knowledge report daily|weekly        → progresso
/knowledge evolve                     → recomendações por cluster
/knowledge export                     → JSON dated
/knowledge import <file>              → importa de export
```

**Quando usar:** após pipeline grande, rode `/knowledge evolve` — sugere consolidar patterns recorrentes em recipes.

### 7.2 Auto-memory pessoal (`~/.claude/projects/.../memory/`)

Diferente de `/knowledge` (project-level). É memória do Claude por usuário+projeto, persiste entre sessões. Carregada automaticamente no SessionStart.

---

## Módulo 8 — Skills

### 8.1 `/skill`

| Action | Uso |
|--------|-----|
| `/skill install <source>` | Local path ou `github:owner/repo[/path]` |
| `/skill create <name>` | Cria via skill-creator (intent → entrevista → SKILL.md) |
| `/skill list` | Lista instaladas |
| `/skill remove <name>` | Remove |
| `/skill optimize <name>` | Otimiza description para triggering |
| `/skill eval <name>` | Eval loop |
| `/skill update skill-creator` | Atualiza do anthropics/skills |

**Exemplo:**

```
/skill install github:anthropics/skills/skills/pdf
/skill create our-graphql-pattern
```

### 8.2 Skills bundled

`design-craft`, `react-best-practices`, `senior-architect`, `skill-creator`, `commit-workflow`, `pipeline-execution`. Carregam quando triggers do `description` casam com a tarefa.

### 8.3 Armadilha

Skills sem 3+ arquivos reais referenciados são rejeitadas pelo `skill-validate.js`. Não invente APIs.

---

## Módulo 9 — Métricas & Stats

### 9.1 `/stats` (superset)

Dashboard completo: pipelines + hooks + RTK + Pass@1.

**Seções:**
- Summary (5-8 linhas com ✓/⚠/→)
- Active / Orphaned por spec (duration, API calls, retries, top tools)
- Completed pipelines
- Last 7 Days (delta semana atual vs anterior)
- Enforcement Events
- RTK Token Economy

### 9.2 `/metrics` (focado)

Só hook events e compare windows.

```
/metrics                                      → tudo desde sempre
/metrics --since 2026-04-09                   → recente
/metrics --event budget-check                 → 1 tipo
/metrics --compare v3.1.21 v3.1.22            → delta entre releases
/metrics --compare 2026-04-09 2026-04-20      → delta entre datas
```

### 9.3 Quando usar

- `/stats` semanal — saúde geral
- `/metrics --compare` antes de release — comparar com baseline
- `/stats` pós-pipeline — checar Pass@1, gate saves, retries

**Dica:** logs em `.claude/.metrics/*.jsonl` rotacionam a 10MB. Para reset: `rm .claude/.metrics/*.jsonl`.

---

## Módulo 10 — Status, Maint & Review

### 10.1 `/status`

Snapshot 4-em-1: Git + Pipeline + Build + Registry. Zero argumentos. Use antes de sessão nova ou deploy.

### 10.2 `/maint`

```
/maint deps        → install em todos subprojetos (paralelo)
/maint validate    → build + type-check
/maint sync        → regenera entity-registry.json
```

### 10.3 `/review [pr|url]`

Review automático via skill `code-review`.

```
/review              → auto-detecta PR da branch atual
/review 123          → PR número 123
/review https://...  → URL completa (GitHub/GitLab/Bitbucket)
```

Checklist: SOLID, Security, Performance, Patterns, Integration. Zero confirmações.

---

## Módulo 11 — Hooks de Enforcement

23 hooks ativos. Os que mais impactam o uso diário:

| Hook | Bloqueio | Como contornar |
|------|----------|----------------|
| `bash-native-redirect.js` | grep/ls/cat/head/tail/find no Bash | Use Grep/Glob/Read |
| `tool-use-counter.js` | Explore agent >15-20 tools | Reformule prompt do agent |
| `enforce-registry.js` | Skill sem registry | Rode `/scan` primeiro |
| `model-routing-gate.js` | Modelo errado para tarefa | Respeite a tabela (Haiku=Explore, Sonnet=bugfix/análise, Opus=feature) |
| `close-gate.js` | `/complete` sem QA pass | Rode `/qa` antes |
| `review-gate.js` | Edits sem review aprovado em phase certa | Pipeline natural |
| `spec-size-gate.js` | Spec gigante | Modo `warn` por default |
| `skill-size-gate.js` | Skill gigante | Modo `warn` por default |

**Env vars de override** (em `settings.json`):

```json
{
  "MUSTARD_DUPLICATION_MODE": "off|warn|strict",
  "MUSTARD_CONVENTION_MODE": "off|warn|strict",
  "MUSTARD_REGRESSION_MODE": "off|warn|strict",
  "MUSTARD_SPEC_SIZE_MODE": "warn",
  "MUSTARD_SKILL_SIZE_MODE": "warn",
  "MUSTARD_QA_GATE_MODE": "strict|warn|off",
  "CONTEXT_BUDGET_MODE": "strict|warn|observe",
  "MUSTARD_DISABLED_HOOKS": "hook-a.js,hook-b.js"
}
```

**Filosofia (memória do projeto):** hooks heurísticos default off/warn; só sensores reais (build, QA) podem bloquear. Não force `strict` em hook heurístico — gera fricção sem ganho.

---

## Módulo 12 — Token Economy (RTK)

**Regra de ouro:** prefixe **todo** comando shell com `rtk`. Mesmo em `&&`:

```bash
rtk git add . && rtk git commit -m "msg" && rtk git push
rtk npm run build
rtk vitest run
rtk gh pr view 123
```

**Savings típicas:**
- Tests: 90-99%
- Build: 70-87%
- Git: 59-80%
- pnpm/npm: 70-90%

**Comandos meta:**
```
rtk gain               → estatísticas
rtk gain --history     → histórico
rtk discover           → analisa sessões para uso perdido
```

**Armadilha:** comandos sem filtro dedicado em RTK ainda passam through — então `rtk` é sempre seguro.

---

## Módulo 13 — Armadilhas Globais

1. **Não rodar `/scan` antes do primeiro pipeline** — bloqueio em registry.
2. **Trabalhar em `main`** — `/git` recusa. Use `dev_<nome>`.
3. **Ignorar QA** — `/complete` bloqueia se `qa.result.overall != pass`.
4. **Forçar modelo barato** (Haiku em feature) — qualidade cai. Memória do projeto é explícita: nunca downgrade.
5. **Editar `.claude/.pipeline-states/`** manualmente — corrompe state. Use `/resume`.
6. **Commits em arquivos efêmeros** (`.metrics/`, `.agent-state/`) — `.gitignore` cobre, mas `git add -A` cego pode escapar.
7. **Specs gigantes** — quebre em pipelines menores. Hook `spec-size-gate` avisa.
8. **AC com sintaxe POSIX no Windows** — use `node -e` ou `bash -c`.
9. **Confundir `/knowledge` (projeto) com auto-memory (usuário)** — são camadas distintas.
10. **Rodar `/feature` para refactor puro** — use `/task refactor`. Pipeline gasta mais tokens.

---

## Módulo 14 — Receita Prática (workshop end-to-end)

**Cenário:** adicionar campo `phone` à entidade `User` em projeto Next.js + Drizzle.

```
1. /scan                                            (bootstrap, se ainda não rodou)
2. /status                                          (sanity check)
3. /feature add-phone-to-user
   → ANALYZE detecta: 1 schema, 1 API, 1 form = Light scope
   → EXECUTE direto, sem PLAN
4. /qa                                              (executa AC: build, type-check, migration)
5. /complete                                        (fecha pipeline)
6. /git push                                        (sync + commit + push)
7. /review                                          (auto-detecta PR)
8. Quando aprovado: /git merge main                 (cascade dev_user → dev → main)
```

**Telemetria pós-execução:**
```
/stats          → ver Pass@1, retries, tokens
/metrics --since <data>
```

---

## Módulo 15 — Cheat Sheet

```
SETUP
  mustard init / update / auto-update
  /scan [sub] [--force]

PIPELINES
  /feature <name>
  /bugfix <descrição>
  /approve [--resume]
  /resume
  /qa [--spec <name>]
  /complete

TAREFAS DELEGADAS
  /task {analyze|audit|review|refactor|docs|implement|compare} <escopo>

GIT
  /git {sync|commit|push|merge|merge main}

KNOWLEDGE & SKILLS
  /knowledge {list|search|add|notes|audit|report|evolve|export|import}
  /skill {install|create|list|remove|optimize|eval|update}

OBSERVABILIDADE
  /status
  /stats
  /metrics [--since|--event|--compare]

MANUTENÇÃO
  /maint {deps|validate|sync}
  /review [pr|url]

TOKEN ECONOMY
  rtk <qualquer comando shell>
  rtk gain
```

---

## Verificação do curso

Para validar que o aluno aprendeu:

1. **Setup:** rodar `mustard init` em projeto novo, `/scan`, abrir `/status` e ver registry populado.
2. **Light feature:** `/feature add-<campo>-to-<entidade>` → completar até `/complete` sem intervenção.
3. **Full feature:** feature cross-layer, validar PLAN gerado, `/approve`, `/resume` em sessão nova, `/qa`, `/complete`.
4. **Bugfix:** introduzir bug deliberado, `/bugfix <sintoma>`, validar correção.
5. **Task:** `/task review <pasta>` e interpretar output.
6. **Git flow:** trabalhar em `dev_<nome>`, `/git push`, `/review`, `/git merge main`.
7. **Métricas:** abrir `/stats`, identificar Pass@1, retries, top tools usados.
8. **Hooks:** tentar rodar `cat file.txt` no Bash → ver bloqueio do `bash-native-redirect`.

Quem completa esses 8 passos sem ajuda externa domina Mustard em uso prático.

---

## Referências internas

- `templates/CLAUDE.md` — regras do orquestrador
- `templates/commands/mustard/*/SKILL.md` — fonte de verdade de cada comando
- `templates/settings.json` — registro de hooks e permissões
- `.claude/pipeline-config.md` — regras de dispatch e naming
- `.claude/knowledge.json` — knowledge base do projeto
- `README.md` raiz — quick start oficial
