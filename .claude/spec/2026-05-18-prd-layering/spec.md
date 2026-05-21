# Feature: prd-layering

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-18T22:49:58Z
### Lang: pt

> Spec de backlog (Parte A, item A4). Rascunho criado em lote — passa por ANALYZE de refino quando for aprovada via `/approve`.

## Contexto

Numa ferramenta SDD, a spec é o artefato que dirige o código. Mas o `spec.md` atual do Mustard mistura duas coisas que têm audiências diferentes: o "o quê e por quê" (Contexto, Resumo, Critérios de Aceitação) — que é o que se alinha com o stakeholder — e o "como" (Arquivos, Tarefas, Waves, Agentes, Limites) — que é o que se alinha com os agentes. Hoje as duas camadas convivem no mesmo arquivo, sob um único gate de aprovação (`/approve`), cego à diferença entre aprovar a intenção e aprovar o plano de execução. Uma ferramenta SDD premium separa visivelmente a camada PRD (a intenção, legível por quem não vai codar) da camada de plano (a execução, legível pelo agente), porque são dois momentos de validação distintos. Esta mudança reorganiza o template de spec em duas camadas explícitas sem alterar o fluxo de pipeline.

## Resumo

Reestruturar o template de `spec.md` em duas camadas nomeadas: **PRD** (problema, usuários/stakeholders, métrica de sucesso, não-objetivos — no topo) e **Plano** (entidades, arquivos, tarefas, waves, limites — embaixo). Atualizar o comando `/feature` para escrever as duas camadas e o `/approve` para reconhecer a distinção (aprovar-o-quê versus aprovar-o-como).

## Entidades

N/A — refatoração de metodologia/template.

## Component Contract

N/A — sem trabalho de UI.

## Arquivos (~5)

- `templates/commands/mustard/feature/SKILL.md` — definição do template de spec (Full e Light)
- `templates/refs/feature/spec-language.md` — tabela de tradução de cabeçalhos + regras de narrativa
- `templates/commands/mustard/approve/SKILL.md` — reconhecer as duas camadas no gate
- `templates/pipeline-config.md` — descrição das fases/artefatos, se referenciar a estrutura da spec
- `templates/commands/mustard/bugfix/SKILL.md` — alinhar o template de spec de bugfix (se aplicável)

## Limites

- `templates/commands/mustard/{feature,approve,bugfix}/`, `templates/refs/feature/spec-language.md`, `templates/pipeline-config.md`
- **Fora dos limites:** specs já existentes em `active/`/`completed/` (não retroconvertidas — karpathy cirúrgico); hooks de gate; `src/`.

## Tarefas

### Templates Agent (Wave 1) — template em duas camadas

- [x] Em `feature/SKILL.md`, redefinir o template Full: bloco superior `## PRD` (Contexto, Usuários/Stakeholders, Métrica de sucesso, Não-Objetivos) e bloco inferior `## Plano` (Entidades, Arquivos, Component Contract, Tarefas, Dependências, Limites). Critérios de Aceitação ficam na fronteira (pertencem ao "o quê" verificável).
- [x] Ajustar o template Light de forma proporcional — uma camada PRD enxuta (Contexto + 1 métrica) e a camada Plano (Checklist + Arquivos).
- [x] Atualizar `refs/feature/spec-language.md`: tradução dos cabeçalhos novos (PRD, Plano, Usuários, Métrica de sucesso) e regras de narrativa por camada.
- [x] Rodar `npm run build`.

### Templates Agent (Wave 2) — gate de duas etapas

- [x] Em `approve/SKILL.md`: reconhecer as duas camadas — opcionalmente um gate leve "aprovar PRD" antes de "aprovar Plano", ou ao menos sinalizar na apresentação qual camada o usuário está aprovando.
- [x] Atualizar `pipeline-config.md` e `bugfix/SKILL.md` para refletir a estrutura de spec em duas camadas.
- [x] Rodar `npm run build`.

## Dependências

- Independente das demais specs da Parte A — pode rodar a qualquer momento.
- Wave 2 depende de Wave 1.

## Preocupações

- **Risco de inchaço:** duas camadas não podem virar mais burocracia. O template Light precisa permanecer curto. Definir no ANALYZE um teto de linhas por camada.
- **Compatibilidade:** o `qa-run.js`, o `close-gate.js` e `analyze-validation.js` fazem parsing de cabeçalhos da spec (ex.: `## Acceptance Criteria`, `## Contexto`). Verificar no ANALYZE que renomear/reagrupar cabeçalhos não quebra esses parsers — pode exigir tarefa adicional nos scripts.

### Concern pós-review (EXECUTE)

- **Latente — `spec-sections.js` chave `plan`:** o review apontou que `SECTIONS.plan = ["Plan", "Plano"]` agora casa o heading divisor `## Plano` do template em camadas, não mais uma seção de conteúdo. Nenhum caller atual de `findSection(spec, "plan")` foi encontrado, então não há quebra observável hoje — mas o risco era latente para callers futuros. **RESOLVIDO (follow-up pós-CLOSE):** a chave morta `plan` foi removida de `SECTIONS` (eliminação do risco por design, não mitigação por comentário) e `spec-language.md` ganhou cláusula de exceção na regra "update both" registrando que `## PRD`/`## Plano` são divisores narrativos sem entrada em `SECTIONS`. Build + 429 testes de hooks passam.
- **Nota:** o template Light omite `## Limites` (decisão deliberada de enxugamento — camada Plano Light = Summary + Checklist + Files). Confirmado intencional pelos implementadores.

## Critérios de Aceitação

- [x] AC-1: O template Full em `feature/SKILL.md` define os blocos `## PRD` e `## Plano` — Command: `bash -c 'grep -qE "## PRD" templates/commands/mustard/feature/SKILL.md && grep -qE "## Plano|## Plan" templates/commands/mustard/feature/SKILL.md'`
- [x] AC-2: A tabela de tradução em `spec-language.md` cobre os cabeçalhos novos — Command: `bash -c 'grep -qi "PRD" templates/refs/feature/spec-language.md'`
- [x] AC-3: Build e type-check passam — Command: `npm run build`

## Não-Objetivos

- Não retroconverter specs já existentes — só o template muda; specs antigas seguem no formato antigo.
- Não criar um documento PRD separado do `spec.md` — é uma reorganização interna em camadas, um arquivo só.
- Não integrar com trackers externos (GitHub/Linear Issues) — eventual escopo futuro.
