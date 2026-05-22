# Feature: skills-absorption

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-18T20:35:00Z
### Lang: pt

> Spec de backlog (Parte B... não — Parte A, item A6). Rascunho ajustado em 2026-05-18: a diretiva passou a ser **adicionar as skills do Matt verbatim, sem adaptar**.

## Contexto

A análise do repositório `mattpocock/skills` identificou skills de engenharia valiosas para o Mustard. A diretiva de execução é clara: **adicionar essas skills sem modificá-las**. Copiá-las verbatim para `templates/skills/`, exatamente como o Matt as escreveu — SKILL.md, `references/` e recursos —, do jeito que estão. O Mustard não adapta, não traduz, não reescreve e não "enriquece" o conteúdo. Adaptar quebraria a integridade do design do Matt (por exemplo, no `grill-with-docs` a entrevista e a construção do `CONTEXT.md` são um único ato integrado — separar isso destruiria o efeito) e criaria uma bifurcação impossível de manter em dia com o upstream. O trabalho do Mustard é puramente aditivo: shipar as skills e deixar o mecanismo de auto-load por descrição acioná-las.

## Resumo

Copiar verbatim, de `github.com/mattpocock/skills`, para `templates/skills/`: `grill-me`, `grill-with-docs`, `diagnose`, `improve-codebase-architecture`. Conteúdo inalterado. Registrar as skills no mecanismo de auto-load do Mustard. Avaliar (sem adaptar) a dependência delas em `setup-matt-pocock-skills`.

## Entidades

N/A — adição de skills de fundação.

## Component Contract

N/A.

## Arquivos

- `templates/skills/grill-me/` — novo; cópia verbatim de `skills/productivity/grill-me/`
- `templates/skills/grill-with-docs/` — novo; cópia verbatim de `skills/engineering/grill-with-docs/`
- `templates/skills/diagnose/` — novo; cópia verbatim de `skills/engineering/diagnose/`
- `templates/skills/improve-codebase-architecture/` — novo; cópia verbatim de `skills/engineering/improve-codebase-architecture/`
- `templates/skills/setup-matt-pocock-skills/` — possível (avaliar no ANALYZE)
- `templates/CLAUDE.md` — registrar na lista "Recommended Skills"
- `templates/pipeline-config.md` — registrar, se houver índice de skills

## Limites

- `templates/skills/` (apenas pastas novas), `templates/CLAUDE.md`, `templates/pipeline-config.md`
- **Fora dos limites:** o conteúdo das skills do Matt (NÃO editado); `/bugfix`, `/scan`, `/feature` (NÃO adaptados — as skills auto-carregam por descrição); demais comandos.

## Tarefas

### Templates Agent (Wave 1) — cópia verbatim

- [x] Clonar (sparse) `github.com/mattpocock/skills` e copiar as pastas inteiras `skills/productivity/grill-me/`, `skills/engineering/grill-with-docs/`, `skills/engineering/diagnose/`, `skills/engineering/improve-codebase-architecture/` para `templates/skills/{nome}/` — SKILL.md + `references/` + recursos, byte a byte. **NÃO editar o conteúdo.** Usar as URLs raw do GitHub para garantir cópia fiel, não markdown convertido.
- [x] Validar o frontmatter YAML de cada SKILL.md contra o `skill-validate-gate` do Mustard. Se o validador exigir um campo ausente, adicionar APENAS esse campo mínimo no frontmatter — nunca tocar o corpo da skill.
- [x] Rodar o validador de skills (`bun .claude/scripts/skills.js validate`) — todas devem passar.

### Templates Agent (Wave 2) — integração aditiva

- [x] Registrar as skills onde o Mustard lista skills disponíveis (`templates/CLAUDE.md` § Recommended Skills e `pipeline-config.md`), para o auto-load por descrição alcançá-las.
- [x] Documentar — sem resolver por adaptação — a dependência das skills de engenharia do Matt em `setup-matt-pocock-skills` e nas estruturas que assumem (`docs/adr/`, `CONTEXT.md`, `LANGUAGE.md`, tracker de issues).
- [x] Rodar `npm run build`.

## Dependências

- Independente das demais specs da Parte A (não toca mais `refs/scan/` — a dependência de A3 foi removida).
- A spec A1 (`context-glossary`) depende **desta**: o `grill-with-docs` adicionado aqui é quem produz o `CONTEXT.md` que a A1 injeta.

## Preocupações

- **Dependência de setup:** as skills de engenharia do Matt assumem o `setup-matt-pocock-skills` e estruturas de repo (`docs/adr/`, `CONTEXT.md`, tracker). Adicionadas verbatim, essas suposições vêm junto. Decisão para o ANALYZE: adicionar também `setup-matt-pocock-skills` verbatim, ou apenas documentar a dependência — em nenhum caso adaptar as skills.
- **Sobreposição com `mustard init`:** o `setup-matt-pocock-skills` sobrepõe parte do que o `mustard init` faz. NÃO resolver isso adaptando — apenas registrar a sobreposição.

### Registro pós-EXECUTE (2026-05-18) — apenas registrado, não resolvido

As 4 skills foram copiadas verbatim. Confirmadas, sem adaptar, as seguintes dependências e sobreposições:

- **`grill-with-docs` e `improve-codebase-architecture` assumem `setup-matt-pocock-skills` e estruturas de repo:** ambas referenciam `CONTEXT.md`, `CONTEXT-MAP.md`, `docs/adr/`, `LANGUAGE.md` (esta última copiada junto, em `improve-codebase-architecture/LANGUAGE.md`) e os formatos `ADR-FORMAT.md`/`CONTEXT-FORMAT.md` (copiados em `grill-with-docs/`). As skills criam `CONTEXT.md` e `docs/adr/` de forma preguiçosa (lazy), então funcionam num repo "vazio", mas a leitura inicial dessas estruturas pressupõe a convenção do Matt. O `setup-matt-pocock-skills` (NÃO copiado nesta spec) é o que normalmente prepara essa convenção.
- **`diagnose` referencia `/improve-codebase-architecture` como handoff** e o script `scripts/hitl-loop.template.sh` (copiado junto). A referência cruzada está intacta — a skill alvo existe agora em `templates/skills/`.
- **`improve-codebase-architecture` referencia caminhos relativos cross-skill** (`../grill-with-docs/CONTEXT-FORMAT.md`, `../grill-with-docs/ADR-FORMAT.md`): esses caminhos resolvem corretamente porque ambas as pastas vivem lado a lado em `templates/skills/`.
- **Sobreposição `setup-matt-pocock-skills` vs `mustard init`:** o `mustard init` já cria a estrutura `.claude/` e pode gerar skills/registry. O `setup-matt-pocock-skills` faria um setup paralelo de `CONTEXT.md`/`docs/adr/`. Decisão futura (fora desta spec): unificar ou deixar coexistir — NÃO resolvido aqui.

## Critérios de Aceitação

- [x] AC-1: As quatro skills existem em `templates/skills/` — Command: `node -e "const fs=require('fs'),b='templates/skills/';['grill-me','grill-with-docs','diagnose','improve-codebase-architecture'].forEach(s=>{if(!fs.existsSync(b+s+'/SKILL.md')){console.error('ausente: '+s);process.exit(1)}})"`
- [x] AC-2: As skills do Matt não carregam o marcador `mustard:generated` (prova de que não foram adaptadas/regeneradas) — Command: `bash -c '! grep -rl "mustard:generated" templates/skills/grill-me templates/skills/grill-with-docs templates/skills/diagnose templates/skills/improve-codebase-architecture'`
- [x] AC-3: Build e type-check passam — Command: `npm run build`

## Não-Objetivos

- Não editar, traduzir, reescrever nem "enriquecer" o conteúdo das skills do Matt — cópia verbatim.
- Não transformar as skills em comandos nem em refs.
- Não adaptar `/bugfix`, `/scan` ou `/feature` — as skills auto-carregam por descrição; os comandos ficam intocados.
- Não construir sincronização automática com o upstream do Matt — cópia pontual por ora.
