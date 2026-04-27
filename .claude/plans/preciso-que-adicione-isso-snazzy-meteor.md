# Plano: Integrar Karpathy Guidelines ao Mustard

## Contexto

Usuário pediu para adicionar ao Mustard os princípios comportamentais do repo [forrestchang/andrej-karpathy-skills](https://github.com/forrestchang/andrej-karpathy-skills) (MIT) — derivados de observações de Andrej Karpathy sobre erros comuns de LLMs programando. São 4 princípios:

1. **Think Before Coding** — explicitar assunções, levantar ambiguidades antes de implementar
2. **Simplicity First** — código mínimo que resolve; nada especulativo
3. **Surgical Changes** — tocar só o necessário; não refatorar adjacente
4. **Goal-Driven Execution** — critérios de sucesso verificáveis; loop até passar

## Requisitos do usuário (bloqueantes)

1. **Otimizar tokens** — implementação deve reduzir, não inflar o orçamento
2. **Ler por completo** durante implementação ou qualquer alteração de código — sem condensados
3. **Carregado uma única vez** por contexto — zero duplicação

## Estratégia (atende os 3 requisitos)

**Skill-only**: conteúdo full fica **apenas** em `templates/skills/karpathy-guidelines/SKILL.md`. CLAUDE.md **não** contém o conteúdo, apenas uma linha de referência na seção `## Recommended Skills`.

### Como cada requisito é atendido:

| Requisito | Como |
|-----------|------|
| Otimizar tokens | Zero bytes no CLAUDE.md sempre-carregado além de 1 linha de index (~15 tok). Skill só carrega em Tasks de código. |
| Lido por completo | Skill tem os 4 princípios integrais (65 linhas). Auto-triggered por description matching em qualquer Task que mexa em código. |
| Carregado 1x | (a) Skill só entra no contexto do agent que bate trigger — orchestrator não vê. (b) Entre agents do mesmo pipeline, **Anthropic prompt caching** (5-min TTL) reaproveita — 1º agent paga ~500 tok, demais pagam ~50 tok (cache hit ~10% do custo). |

### Custo real por fluxo

| Fluxo | Tokens adicionais |
|-------|-------------------|
| `/scan`, `/metrics`, `/status`, `/stats` (não-coding) | **+15** (só linha de index) |
| `/feature` Full (EXECUTE com N agents) | **~500 + 50·(N-1)** via cache |
| `/feature` Light (1 agent EXECUTE) | **+500** |
| `/bugfix` | **+500** |
| `/task refactor`, `/task review` | **+500** |
| `/approve`, `/resume`, `/complete` (orchestrator-only) | **+15** (sem conteúdo) |

## Decisões resolvidas

| Questão | Decisão |
|---------|---------|
| Nome do skill | `karpathy-guidelines` — preserva atribuição MIT, autoridade do nome, consistente com skills existentes |
| Profundidade | **Só no skill**, full (revisado vs versão anterior que propunha condensado no CLAUDE.md) |
| Branding | Crédito/atribuição MIT + link no topo do SKILL.md |

## Arquivos a modificar

### 1. **NOVO** — [templates/skills/karpathy-guidelines/SKILL.md](templates/skills/karpathy-guidelines/SKILL.md)

Skill foundation seguindo padrão dos 6 existentes:

- Frontmatter YAML:
  - `name: karpathy-guidelines`
  - `description:` com triggers amplos cobrindo **qualquer alteração de código**: `implement`, `write code`, `edit code`, `modify code`, `change code`, `refactor`, `fix`, `bugfix`, `review`, `add feature`, `add logic`. Garante auto-load em 100% dos fluxos de coding.
  - **Não** usar `disable-model-invocation: true` — queremos auto-trigger
- `<!-- mustard:generated -->` **após** frontmatter (guard [templates/CLAUDE.md:71](templates/CLAUDE.md:71))
- Conteúdo: 4 princípios integrais (65 linhas originais preservadas)
- Rodapé: "Derivado de [forrestchang/andrej-karpathy-skills](https://github.com/forrestchang/andrej-karpathy-skills) (MIT)"

### 2. **EDITAR** — [templates/CLAUDE.md](templates/CLAUDE.md)

**Mudança mínima**: adicionar 1 linha à seção existente `## Recommended Skills` (linha 83-89), no topo da lista:

```markdown
- `karpathy-guidelines` — 4 princípios anti-slop (carrega em toda alteração de código)
```

**NÃO** adicionar seção "Core Coding Principles" no CLAUDE.md (revisão vs plano v1). Isso evita duplicação e mantém orchestrator enxuto.

### 3. **EDITAR** — [.claude/CLAUDE.md](.claude/CLAUDE.md)

Mesma edição mínima (1 linha) — este repo roda o próprio Mustard. `init.ts` preserva `.claude/CLAUDE.md` em re-updates, então edição manual é durável.

### 4. **NÃO MEXER** — demais arquivos

- `.claude/skills/` → regenerado automaticamente via `mustard update` (copia `templates/skills/`)
- `templates/context/{agent}/*.core.md` → skill auto-load via description já cobre
- Zero hooks novos, zero código TS — respeita memória `feedback_analysis_pattern` ("subtrair > adicionar")

## Garantias de "1x por contexto"

1. **Intra-agent** (natural): cada system prompt só inclui skill uma vez (auto-trigger não duplica)
2. **Inter-agent mesmo pipeline** (prompt cache): Anthropic SDK faz cache hit se o conteúdo aparece na mesma posição do system prompt em 5min. Skills auto-loaded entram sempre após CLAUDE.md (posição estável) → cache hit garantido para agents spawned dentro da janela
3. **Orchestrator nunca carrega** o skill full — só a linha de index em CLAUDE.md

## Padrões reutilizados

- **Estrutura skill**: [templates/skills/commit-workflow/SKILL.md:1-5](templates/skills/commit-workflow/SKILL.md:1)
- **Guard do header**: [templates/CLAUDE.md:71](templates/CLAUDE.md:71)
- **Audit skills listados**: [templates/hooks/recommended-skills-audit.js:50](templates/hooks/recommended-skills-audit.js:50) — avisa se >10; adicionar 1 entrada leva de 5 → 6, ok
- **Copy templates→.claude**: [src/commands/init.ts:53-64](src/commands/init.ts:53) — wholesale copy, nenhuma mudança TS necessária

## Verificação end-to-end

1. **Sintaxe**: `node .claude/scripts/skill-validate.js` — skill novo passa validação
2. **Audit**: `node .claude/scripts/metrics-collect.js` pré/pós sessão de feature — delta em CLAUDE.md system prompt ≤20 tokens
3. **Auto-trigger**: spawn `Task(general-purpose)` com description "implement new endpoint" — confirmar que `karpathy-guidelines` aparece no system prompt do agent
4. **Não-trigger**: spawn `Task(Explore)` com description "analyze project structure" — skill **não** deve ser carregado (não é alteração de código)
5. **Cache dedup**: rodar `/mustard:feature` Full com 2+ agents em EXECUTE — inspecionar API usage (`rtk gain --history`) para confirmar cache hits nos agents subsequentes
6. **Comportamento**: pipeline de teste onde agent é tentado a adicionar try/catch desnecessário ou refactor adjacente — verificar que respeita princípios (diffs enxutos)

## Fora de escopo (consciente)

- Sem EXAMPLES.md/CURSOR.md — Mustard agnóstico (memória `feedback_mustard_agnostic`)
- Sem hook `karpathy-check.js` — "subtrair > adicionar" (memória `feedback_analysis_pattern`)
- Sem edição em `.core.md` dos agents — skill auto-load é suficiente
- Sem seção "Core Coding Principles" no CLAUDE.md — evita duplicação e atende req "1x"
