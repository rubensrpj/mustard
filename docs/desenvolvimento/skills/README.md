# Guias locais de desenvolvimento

As receitas de desenvolvimento do próprio Mustard moram em `apps/rt/.agents/skills/<nome>/SKILL.md`, no git. Não são templates instalados em todos os projetos.

Cada receita declara no cabeçalho, em `paths:`, as pastas que atende. O pedido da onda inclui a receita quando uma tarefa mexe numa dessas pastas, sem que a tarefa precise nomeá-la:

- `add-run-command`: `apps/rt/src/commands/**`;
- `add-hook-rule` e `refazer-gancho`: `apps/rt/src/hooks/**`.

As cópias antigas em `apps/rt/.claude/skills`, fora do git, não valem mais e podem ser apagadas.

As fontes de autoridade continuam sendo registro, enums, CLI, código e provas vigentes. Os guias evitam exemplos extensos que envelhecem e não substituem leitura do contrato afetado.
