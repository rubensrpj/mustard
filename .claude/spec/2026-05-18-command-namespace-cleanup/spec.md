# Feature: command-namespace-cleanup

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-18T19:34:04Z
### Lang: pt

## Contexto

O Mustard expõe seus comandos como subpastas de `templates/commands/mustard/`, e o Claude Code registra automaticamente cada subpasta como um slash command. Hoje existem 18 subpastas, mas duas delas não são comandos de usuário: `scan-format` e `templates/agent-prompt` são instruções internas consumidas por agentes, e mesmo assim aparecem na lista de comandos como ruído. Além disso, `/metrics` e `/stats` se sobrepõem ao ponto de suas descrições precisarem se referenciar mutuamente para o usuário descobrir qual usar — sinal claro de que deveriam ser um só comando. E `/complete` tem um nome que não diz o que completa, divergindo da fase canônica `CLOSE` do pipeline. O resultado é uma superfície de comandos inflada e ambígua, que contradiz o objetivo de tornar o Mustard uma ferramenta enxuta. Esta limpeza reduz de 18 para 15 comandos reais sem alterar nenhum comportamento de pipeline.

## Resumo

Quatro movimentações estruturais e a reconciliação de uma inconsistência de placeholder, todas confinadas ao subprojeto `templates` mais os docs raiz:

- Fundir `/metrics` em `/stats` (flags `--hooks`/`--since`/`--event`/`--compare`/`--pr`/`--days`); remover a pasta `metrics/`.
- Renomear `/complete` → `/close` para alinhar ao vocabulário canônico da fase `CLOSE`.
- Mover `scan-format/` e `templates/agent-prompt/` de `commands/` para `refs/` (não são comandos de usuário).
- Reconciliar o placeholder fantasma `{spec_slice}` em `resume/SKILL.md` com o `{task_steps}` real do template.

A CLI não hardcoda nomes de comando — `init.ts`/`update.ts` copiam `templates/` inteiro (verificado) —, logo nenhuma mudança em `src/` é necessária.

## Entidades

N/A — refatoração de tooling. Não há entidades de domínio, schema, endpoint ou UI envolvidos.

## Arquivos (~20)

Renomeações / movimentações:
- `templates/commands/mustard/complete/` → `templates/commands/mustard/close/`
- `templates/commands/mustard/metrics/` → removida
- `templates/commands/mustard/scan-format/SKILL.md` → `templates/refs/scan/scan-format.md`
- `templates/commands/mustard/templates/agent-prompt/SKILL.md` → `templates/refs/agent-prompt/agent-prompt.md`

Edições de conteúdo:
- `templates/commands/mustard/close/SKILL.md` (ex-complete — frontmatter, título, trigger)
- `templates/commands/mustard/stats/SKILL.md` (absorve flags + seção DORA de metrics)
- `templates/commands/mustard/qa/SKILL.md`, `approve/SKILL.md`, `status/SKILL.md`, `review/SKILL.md`
- `templates/commands/mustard/feature/SKILL.md`, `bugfix/SKILL.md`, `resume/SKILL.md`
- `templates/hooks/spec-hygiene.js`
- `templates/refs/scan/scan-protocol.md`, `refs/scan/evidence-rules.md`
- `templates/refs/resume/fix-loop-wave.md`, `refs/feature/spec-language.md`, `refs/agent-prompt/prefix-order.md`
- `README.md`, `TUTORIAL.md`, `CHANGELOG.md`, `curso-mustard.html`

## Limites

- `templates/commands/mustard/` — renomeações e remoções internas
- `templates/refs/scan/`, `templates/refs/agent-prompt/` — destinos das movimentações
- `templates/hooks/spec-hygiene.js` — apenas a string de mensagem ao usuário
- `README.md`, `TUTORIAL.md`, `CHANGELOG.md`, `curso-mustard.html` — docs raiz
- **Fora dos limites:** `src/`, `dist/`, o espelho `.claude/` do próprio repo, `templates/scripts/` (o script `complete-spec.js` NÃO é renomeado), `templates/spec/` (specs históricas).

## Tarefas

### Templates Agent (Wave 1) — movimentações estruturais

- [x] Renomear `templates/commands/mustard/complete/` → `close/` (git mv). Em `close/SKILL.md`, atualizar frontmatter `name`/`description`, título, `## Trigger` para `/close`, e toda menção interna a "/complete".
- [x] Fundir `/metrics` em `/stats`: remover `templates/commands/mustard/metrics/`. Em `stats/SKILL.md`, ampliar o Trigger para `[--hooks] [--since <ISO>] [--event <type>] [--compare <from> <to>] [--pr] [--days <n>]`, adicionar a seção de Flags e a tabela "DORA event sources" vindas de `metrics/SKILL.md`. A flag `--hooks` roteia para `metrics.js report` (o `/stats` padrão usa `metrics.js collect`).
- [x] Mover `scan-format/SKILL.md` → `templates/refs/scan/scan-format.md`; remover frontmatter de comando, se houver (refs são markdown puro).
- [x] Mover `templates/agent-prompt/SKILL.md` → `templates/refs/agent-prompt/agent-prompt.md`; remover a pasta vazia `templates/commands/mustard/templates/`.
- [x] Rodar `npm run build` — deve passar.

### Templates Agent (Wave 2) — integridade de referências

- [x] Atualizar `/complete` → `/close` em: `qa/SKILL.md` (2 ocorrências), `approve/SKILL.md`, `status/SKILL.md`, e a mensagem "Run /mustard:complete" em `hooks/spec-hygiene.js`.
- [x] Atualizar `/metrics` → `/stats` em: `stats/SKILL.md` (remover a prosa de cross-reference mútua) e `review/SKILL.md` (`/mustard:metrics --view pr-metrics` → `/mustard:stats --pr`).
- [x] Atualizar caminhos de `scan-format` em `refs/scan/scan-protocol.md` e `refs/scan/evidence-rules.md` — como o destino é o mesmo diretório `refs/scan/`, links relativos `scan-format.md` já resolvem; ajustar apenas caminhos absolutos. Atualizar `scan/SKILL.md` se referenciar o caminho antigo.
- [x] Atualizar caminhos de `agent-prompt` em `feature/SKILL.md`, `bugfix/SKILL.md`, `resume/SKILL.md`, `refs/resume/fix-loop-wave.md`, `refs/feature/spec-language.md`, `refs/agent-prompt/prefix-order.md` — de `commands/mustard/templates/agent-prompt/SKILL.md` para `refs/agent-prompt/agent-prompt.md`.
- [x] Reconciliar o placeholder fantasma em `resume/SKILL.md` (§ Wave Slice Injection e Step 12c.3): o output de `spec-extract.js` popula `{task_steps}` — remover toda menção a `{spec_slice}`, que não existe no template `agent-prompt`.
- [x] Atualizar docs raiz: `README.md`, `TUTORIAL.md`, `CHANGELOG.md`, `curso-mustard.html` — substituir `/complete`→`/close`, remover `/metrics`, corrigir os caminhos da árvore de arquivos.
- [x] Rodar `npm run build` final — deve passar.

## Dependências

- Wave 2 depende de Wave 1 (as referências apontam para os caminhos novos).
- Nenhuma mudança em `src/` — `init.ts`/`update.ts` copiam `templates/` inteiro, sem hardcode de nomes de comando (verificado durante ANALYZE).

## Critérios de Aceitação

Critérios testáveis e binários (pass/fail). Cada um executável e independente, a partir da raiz do projeto.

- [x] AC-1: As pastas antigas sumiram e `close/` existe — Command: `node -e "const fs=require('fs'),b='templates/commands/mustard/';['complete','metrics','scan-format','templates'].forEach(d=>{if(fs.existsSync(b+d)){console.error('ainda existe: '+d);process.exit(1)}});if(!fs.existsSync(b+'close')){console.error('close/ ausente');process.exit(1)}"`
- [x] AC-2: Os refs reposicionados existem nos destinos — Command: `node -e "const fs=require('fs');['templates/refs/scan/scan-format.md','templates/refs/agent-prompt/agent-prompt.md'].forEach(p=>{if(!fs.existsSync(p)){console.error('ausente: '+p);process.exit(1)}})"`
- [x] AC-3: Zero referências obsoletas a comandos/caminhos antigos — Command: `bash -c '! grep -rEn "mustard:complete|mustard:metrics|commands/mustard/metrics|commands/mustard/scan-format|commands/mustard/templates/agent-prompt" templates/commands templates/refs templates/hooks README.md TUTORIAL.md CHANGELOG.md curso-mustard.html'`
- [x] AC-4: Build e type-check passam — Command: `npm run build`

## Não-Objetivos

- Não renomear o script `templates/scripts/complete-spec.js` — mantém o nome para minimizar o raio de impacto; `/close` apenas o invoca.
- Não alterar o comportamento de `/stats` além de absorver as flags de `/metrics`.
- Não editar à mão o espelho `.claude/` do próprio repositório — ele é regenerado por `mustard update`.
- Não tocar specs históricas em `templates/spec/completed/`.
- Itens A1 (glossário), A4 (camada PRD), A5 (harness legível) e A6 (absorções do Matt) da análise — specs separadas, fora desta.
