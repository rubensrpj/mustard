# Wave 3 (rt + templates) — Grafo de wirelinks + vault Obsidian

## PRD

## Contexto

Os artefatos que o Mustard gera (skills, recipes, convenções) são hoje arquivos isolados, de nome repetido (todo skill é `SKILL.md`, todo subprojeto tem `guards.md`) e sem relação explícita entre si. Não dá para navegar o conhecimento do projeto, nem para o motor saber qual convenção depende de qual. Esta wave transforma esses artefatos em um grafo: cada conceito vira um nó com `id` único e namespaceado, e as relações de significado viram links `[[id]]` no estilo nativo do Obsidian — os mesmos que um humano navega no graph view e que a máquina parseia para resolver injeção. A pasta `.claude/` passa a ser um vault: nós-conceito centralizados em `.claude/graph/`, uma pasta `.obsidian/` só de configuração, um índice MOC gerado, e os `SKILL.md` pesados ganham um `alias` igual ao seu `id` para serem alcançáveis sem colisão. Um índice `id→path` é construído no sync para a máquina dereferenciar.

## Métrica de sucesso

O `.claude/` abre como vault no Obsidian sem colisão de nome; o graph view mostra convenções, skills e entidades conectadas; o índice `id→path` resolve todo `[[id]]` existente; arestas órfãs e ciclos são detectados no sync.

## Critérios de Aceitação

- [x] AC-1: vault presente — `.claude/.obsidian/` + `.claude/graph/index.md` (MOC) existem — Command: `node -e "const fs=require('fs');process.exit(fs.existsSync('.claude/.obsidian')&&fs.existsSync('.claude/graph/index.md')?0:1)"`
- [x] AC-2: índice id→path resolve todas as arestas; órfã/ciclo viram warning, não panic — Command: `cargo test -p mustard-rt graph_validation`
- [x] AC-3: ids únicos — nenhum `id`/alias duplicado no grafo — Command: `cargo test -p mustard-rt graph_ids_unique`

## Plano

## Summary

Definir o schema do nó-conceito (frontmatter `id`, `provides`, corpo com `[[id]]`) e a convenção `id = {sub}.{kind}.{slug}`. A interpretação da W2 passa a emitir nós-conceito em `.claude/graph/` e a anotar `aliases:[id]` nos `SKILL.md`. Novo `mustard-rt run graph-index` constrói o `.context-graph.json` (adjacência + tabela `id→path`) parseando `[[ ]]`, valida (órfã/ciclo) e gera o MOC `index.md`. Templates do CLI ganham a pasta `.obsidian/` de config padrão.

## Arquivos

- `apps/rt/src/run/scan/graph.rs` — novo: parse de `[[id]]`, construção da adjacência + `id→path`, validação órfã/ciclo, geração do MOC.
- `apps/rt/src/run/scan/interpret.rs` — emitir nós-conceito + `aliases` nos SKILL.md (estende W2).
- `apps/rt/src/run/mod.rs` / dispatcher de subcomandos — registrar `run graph-index`.
- `apps/cli/templates/.obsidian/` — config padrão do vault (app.json/graph.json mínimos) copiada por `mustard init`.
- `apps/cli/templates/CLAUDE.md` — documentar o vault e a fronteira conhecimento×plumbing.

## Tarefas

### rt Agent (Wave 3)

- [x] Definir schema do nó-conceito e a convenção de `id` (`{sub}.{kind}.{slug}`, kebab).
- [x] `interpret` emite nós em `.claude/graph/` e injeta `aliases:[id]` no frontmatter dos `SKILL.md`.
- [x] `graph.rs`: parser `[[id]]` (regex trivial), adjacência, tabela `id→path`, validação (aresta órfã → warning; ciclo → corta), geração do `index.md` (MOC).
- [x] `mustard-rt run graph-index` (JSON byte-estável) + registro no dispatcher.
- [x] Testes: `graph_validation`, `graph_ids_unique`.

### templates Agent (Wave 3)

- [x] Adicionar `apps/cli/templates/.obsidian/` mínima (config, sem links) copiada no `init`/`update`.
- [x] Atualizar `templates/CLAUDE.md`: vault, layout `.claude/graph/`, fronteira conhecimento×plumbing.

## Limites

- `.claude/spec/2026-05-22-project-profiler/wave-3-rt/`
- `apps/rt/src/run/scan/**`, dispatcher de `run`
- `apps/cli/templates/.obsidian/**`, `apps/cli/templates/CLAUDE.md`
- NÃO implementar o resolver de injeção aqui (W4) — esta wave só constrói e valida o grafo + vault.
