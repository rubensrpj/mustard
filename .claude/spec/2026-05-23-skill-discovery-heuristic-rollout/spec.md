# Feature: Skill Discovery Heuristic — rollout em SKILLs + lint enforcement

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-23T19:30:00Z
### Lang: pt

## PRD

## Contexto

O fix `2026-05-23-spec-picker-perf-and-sqlite-backfill` (concluído nesta mesma sessão) provou que mover descoberta determinística (glob + parse + agregação de filesystem state) do LLM para o `mustard-rt` reduz consumo de tokens em ordem de magnitude (60k → ~2k no caso `/mustard:spec`). Após esse fix, um audit dos SKILLs em `.claude/commands/mustard/*/SKILL.md` revelou que pelo menos 6 outros comandos ainda fazem trabalho determinístico no LLM:

| Comando | O que faz no LLM hoje | Custo aproximado |
|---|---|---|
| `/mustard:status --harness` | Lê `settings.json`, agrupa hooks por evento de ciclo de vida, resolve env vars, renderiza tabela 4-col | 10-30k tokens |
| `/mustard:skill list` | Globa `.claude/skills/*/SKILL.md`, parseia YAML frontmatter, renderiza tabela | 5-15k tokens |
| `/mustard:knowledge glossary` | Itera `entity-registry.json`, filtra por termo, formata cada entidade | proporcional ao registry |
| `/mustard:knowledge list` | Chama `mustard-rt run memory list` (já delegado) mas o LLM faz agrupamento + render | 5-10k tokens |
| `/mustard:review` (prefetch) | Fetch de PR via `gh pr view` + parsing manual de comentários + diff | 10-30k tokens |
| `/mustard:bugfix` Step 2 | Itera todas as specs lendo header + checklist (duplica discovery já feita em `active-specs`) | 10-30k tokens |
| `/mustard:qa` Step 1 | Glob+filter+pick-recent (mesma duplicação) | 5-10k tokens |

Sem uma regra durável, o mesmo padrão vai voltar a aparecer em SKILLs futuros. A spec encerra três frentes:

1. **Documentar a heurística** em `.claude/pipeline-config.md` (e mirror em `apps/cli/templates/pipeline-config.md`) como regra invariante: "Se o SKILL faz glob + parse + agregação de filesystem state e devolve uma tabela determinística, isso é trabalho de `mustard-rt`, não do LLM." Critério decisor: **muda com estado do disco ou com decisão humana?** Se for só disco, mora no binário.
2. **Implementar enforcement** via `mustard-rt run doctor --check skill-discovery` que escaneia todos os SKILLs em `.claude/commands/mustard/` e `apps/cli/templates/commands/mustard/` em busca de telltale phrases (`Glob \`.claude/`, `Iterate \`entity-registry`, `parse YAML frontmatter`, `for each spec`, `for each skill`, etc.) e reporta violações com severidade WARN. Roda no `/mustard:maint doctor` e em CI futuro.
3. **Aplicar a heurística aos 7 comandos identificados** — cada um vira: SKILL chama `mustard-rt run <X>`, imprime saída verbatim, mantém só o que exige judgment humano ou interação multi-turno.

## Usuários/Stakeholders

Quem usa Mustard em projetos com >50 specs / >30 skills / >100 entidades — onde o custo linear vira insuportável. E qualquer dev escrevendo SKILLs novos no futuro: a heurística + o lint formam a rede de proteção contra regressão do padrão.

## Métrica de sucesso

- Os 6 SKILLs (`status`, `skill`, `knowledge`, `review`, `bugfix`, `qa`) consomem ≤5k tokens por invocação típica em projeto de tamanho médio (medido pelo `prompt-prefix-metrics` ou contagem manual de cada Bash/Read/Grep chamada).
- `mustard-rt run doctor --check skill-discovery` retorna `0 violations` quando rodado após o rollout.
- O bloco "Skill Discovery Heuristic" existe em `pipeline-config.md` (raiz + templates) e é referenciado por pelo menos 2 SKILLs (`feature` quando descreve criação de SKILL, e o doctor lint quando emite mensagem de violação).

## Não-Objetivos

- **Não** mover trabalho do LLM que envolve judgment (escrever spec, escolher modelo, julgar APPROVED/REJECTED em review).
- **Não** mexer em `/mustard:feature`, `/mustard:bugfix` (fluxo de criação), `/mustard:prd`, `/mustard:tactical-fix`, `/mustard:close`, `/mustard:git`, `/mustard:task`, `/mustard:stats`, `/mustard:scan` — todos já enxutos ou com LLM core indispensável.
- **Não** fazer auto-fix do lint (só WARN; humano decide).
- **Não** estender o `active-specs` com novos campos (checklist counter, etc.) — `/bugfix` e `/qa` consomem o output atual; se precisarem de mais, é TF separada.

## Plano de implementação (decomposto em 3 waves — ver `wave-plan.md`)

- **Wave 1 (rt + docs)**: heurística em pipeline-config.md + lint no doctor — fundação que define a regra antes do rollout.
- **Wave 2 (rt subcomandos)**: 5 novos/estendidos subcomandos paralelos — `status [--harness]`, `skills list`, `knowledge glossary`, `memory list --grouped`, `review-prefetch`.
- **Wave 3 (SKILLs)**: atualização dos 7 SKILLs para chamar binários — apaga trabalho determinístico do LLM, mantém só judgment + roteamento.

Wave 3 depende de Wave 2 (binários precisam existir). Wave 1 é independente das outras e pode rodar em paralelo com Wave 2, mas para evitar dispatch concorrente em arquivos próximos, executamos sequencialmente: 1 → 2 → 3.
