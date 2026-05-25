# Memory — Mustard Deep Refactor

Princípios técnicos formalizados durante esta spec. Vivem junto da spec (não em user-global) para serem versionados no git, sobreviverem ao arquivamento e poderem ser linkados por specs futuras via wirelink.

## Princípios (6)

| Arquivo | Conteúdo | Wave de origem |
|---------|----------|----------------|
| [[scan_rust_first]] | Tudo estrutural do `/scan` em Rust; IA só interpretação semântica nomeável | [[wave-3-mixed]] |
| [[no_hardcoded_stack_patterns]] | Mustard é ferramenta agnóstica; zero catálogo de padrões esperados por stack; tudo emerge do filesystem | [[wave-3-mixed]] |
| [[recipes_from_scan]] | Recipes geradas pelo `/scan` por subprojeto com paths/convenções reais; nunca hardcoded em templates/ | [[wave-3-mixed]] |
| [[graph_pipeline_knowledge]] | `.claude/graph/` aceita só `spec/skill/command/ref/recipe/conv`; entity/enum vivem no registry, não no graph | [[wave-3-mixed]] |
| [[templates_md_moat]] | `apps/cli/templates/*.md` alimentam scan + entity-registry + contexto IA; manter enxuto, sem refs legadas, coerente com Rust atual | [[wave-6-cli]] |
| [[claude_dir_audit]] | `.claude/` raiz pode acumular zumbi; cruzar com uso real de rt/cli/dashboard antes de declarar feature pronta | [[wave-2-mixed]] |

## Carregamento

Estes princípios **não** são auto-injetados em `SessionStart`. Em vez disso:

- **Per-dispatch**: o hook `subagent_inject` (W8.T8.3 + W8.T8.10) consulta `skill-resolve` (W1.T1.4) para filtrar princípios relevantes à tarefa do agente — não carrega todos.
- **Per-wave**: o `agent-prompt-render` (W1.T1.5) injeta apenas princípios cujo wirelink case com o cluster/skill da tarefa.
- **Sob demanda**: Grep/Read continuam funcionais a qualquer momento.

Memory user-global (`~/.claude/projects/C--Atiz-mustard/memory/`) carrega só preferências de comportamento do user — não princípios técnicos.
