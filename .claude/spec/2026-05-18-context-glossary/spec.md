# Feature: context-glossary

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-18T21:32:49Z
### Lang: pt

> Spec de backlog (Parte A, item A1). Reescrita em 2026-05-18: injeção **filtrada por relevância** (padrão do entity-registry) + wiring do `grill-with-docs` na PLAN do `/feature`. A6 concluída — as skills do Matt já existem em `templates/skills/`.

## Contexto

A skill `grill-with-docs` (instalada pela A6, já concluída) constrói e mantém o `CONTEXT.md` — o glossário de linguagem compartilhada do projeto. Mas hoje ela está dormente nas duas pontas: nada a aciona, então o `CONTEXT.md` nunca é produzido; e nada o injeta nos agentes, então ele nunca é consumido. A A1 fecha as duas pontas. Como produtor: liga o `grill-with-docs` como modo opt-in da fase PLAN do `/feature` Full, onde alinhar antes de codar é o comportamento esperado. Como consumidor: injeta o `CONTEXT.md` nos prompts de agente — mas **filtrado por relevância**, não despejando o arquivo inteiro com um teto posicional burro. O Mustard já tem esse padrão: a regra da casa é "Grep o `entity-registry.json` pela entidade específica, NUNCA leia o JSON inteiro". A A1 aplica o mesmo princípio ao `CONTEXT.md`.

## Resumo

(1) Ligar o `grill-with-docs` como opt-in da fase PLAN do `/feature` (escopo Full apenas). (2) Criar `context-slice.js`, que dado o `CONTEXT.md` + a spec ativa retorna só os blocos de termo relevantes às entidades/arquivos da spec. (3) Injetar essa fatia no bloco PREFIX-STABLE do template de prompt. Filtrar supera despejar; o teto vira backstop.

## Entidades

N/A — integração de contexto.

## Component Contract

N/A.

## Arquivos (~5)

- `templates/commands/mustard/feature/SKILL.md` — opt-in de grill na PLAN + preenchimento de `{context_md}`
- `templates/commands/mustard/resume/SKILL.md` — preenchimento de `{context_md}` no dispatch
- `templates/refs/agent-prompt/agent-prompt.md` — placeholder `{context_md}` no bloco PREFIX-STABLE
- `templates/scripts/context-slice.js` — novo; filtro de relevância (espelha `spec-extract.js`)
- `templates/pipeline-config.md` — registrar `CONTEXT.md` como fonte de contexto

## Limites

- `templates/commands/mustard/{feature,resume}/`, `templates/refs/agent-prompt/`, `templates/scripts/context-slice.js`, `templates/pipeline-config.md`
- **Fora dos limites:** o `CONTEXT.md` em si (produzido pela skill — NÃO tocado); a skill `grill-with-docs` (verbatim, NÃO adaptada); `entity-registry.json` e `knowledge.json`.

## Tarefas

### Templates Agent (Wave 1) — produtor: grill-with-docs na PLAN

- [x] Em `feature/SKILL.md`, no início da fase PLAN e **só em escopo Full**: uma `AskUserQuestion` — "Escrever a spec direto, ou grelhar o plano antes (`grill-with-docs`)?". Se "grelhar" → invocar `Skill(grill-with-docs)` antes de redigir a spec (a skill atualiza o `CONTEXT.md` por conta própria). Escopo Light: nunca grelha, sem a pergunta.
- [x] Documentar que a skill NÃO é adaptada — o `/feature` apenas a aciona; o conteúdo verbatim do Matt fica intocado.
- [x] Rodar `npm run build`.

### Templates Agent (Wave 2) — consumidor: injeção filtrada

- [x] Criar `templates/scripts/context-slice.js`: dado `--context {CONTEXT.md}` e `--spec {spec.md}`, retorna só os blocos de termo cujo termo ou definição casa com entidades, arquivos ou termos-chave da spec. Espelha o formato de `spec-extract.js` (CLI + programático, fail-graceful). Backstop: teto `MUSTARD_GLOSSARY_MAX_LINES` (~250), com aviso acionável em stderr se a fatia ainda exceder.
- [x] Adicionar o placeholder `{context_md}` ao bloco `<!-- PREFIX-STABLE -->` de `refs/agent-prompt/agent-prompt.md`. A fatia é estável dentro do pipeline (a spec não muda no meio) — então cacheia entre dispatches.
- [x] Em `feature/SKILL.md` e `resume/SKILL.md`: no snapshot por wave, rodar `context-slice.js`, gravar a fatia em `.claude/.pipeline-states/{specName}.context-md.md`, e preencher `{context_md}` com ela. Se `CONTEXT.md` ausente → `{context_md}` vazio (degrade gracioso). Snapshot refeito só na transição de wave.
- [x] Registrar `CONTEXT.md` em `pipeline-config.md` como fonte de contexto, documentando o slice por relevância.
- [x] Rodar `npm run build`.

## Dependências

- **A3** (`command-namespace-cleanup`) ✓ — caminho de `agent-prompt`.
- **A6** (`skills-absorption`) ✓ — `grill-with-docs` instalada.
- Wave 2 não depende rígido da Wave 1 (degrade gracioso com `CONTEXT.md` vazio).

## Preocupações

- **Heurística do filtro:** casar termo↔spec por substring pode gerar falso-positivo/negativo. O ANALYZE define a heurística exata (nomes de entidade exatos + tokens significativos da spec).
- **Multi-contexto:** o Matt suporta `CONTEXT-MAP.md` para repos multi-contexto. O `context-slice.js` precisa decidir, no ANALYZE, como lidar com múltiplos `CONTEXT.md` num monorepo.

## Critérios de Aceitação

- [x] AC-1: O `context-slice.js` existe — Command: `node -e "const fs=require('fs');if(!fs.existsSync('templates/scripts/context-slice.js'))process.exit(1)"`
- [x] AC-2: O placeholder `{context_md}` está no template de prompt — Command: `bash -c 'grep -q "{context_md}" templates/refs/agent-prompt/agent-prompt.md'`
- [x] AC-3: O `feature/SKILL.md` oferece o grill opt-in na PLAN — Command: `bash -c 'grep -q "grill-with-docs" templates/commands/mustard/feature/SKILL.md'`
- [x] AC-4: Build e type-check passam — Command: `npm run build`

## Não-Objetivos

- Não despejar o `CONTEXT.md` inteiro — a injeção é filtrada por relevância.
- Não criar glossário próprio do Mustard — é o `CONTEXT.md` do `grill-with-docs`.
- Não adaptar nenhuma skill do Matt — a A1 só aciona e consome.
- Não grelhar em escopo Light.
