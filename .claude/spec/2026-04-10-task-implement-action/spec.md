# Enhancement: task-implement-action

## Summary

Adicionar uma nova action `implement` ao `/mustard:task` que entrega código padronizado com baixo custo de token. Gap atual: `/task refactor` escreve código mas sem injeção de guards/patterns/recipes, e `/feature` Light scope carrega ~60-120K por causa do overhead de pipeline. O `implement` preenche o meio: single dispatch, guards/patterns/recipes injetados inline via Grep cirúrgico, sem spec, sem state, sem review gate. Alvo: ~25-50K por invocação.

## Boundaries

- `templates/commands/mustard/task/SKILL.md` — adicionar action `implement` na tabela + implementação + exemplo
- `templates/commands/mustard/task/references/implementation-examples.md` — adicionar exemplo de `implement` (se arquivo existir)

## Checklist

### general-purpose Agent

- [x] Adicionar linha `implement` na tabela "Actions" de `SKILL.md` logo após `refactor`: `| implement | general-purpose | sonnet | Single-dispatch implementation with inline guards/patterns/recipes (low-cost, standardized) |`
- [x] Adicionar seção "### implement" logo após "### refactor" em `SKILL.md § Flow` descrevendo os passos: (1) Grep cirúrgico em `{subproject}/.claude/commands/{guards,patterns,recipes}.md` pelo termo do scope — cap ~500 tokens por arquivo; (2) Dispatch único `Task(general-purpose, sonnet)` com guards/patterns/recipe injetados inline + naming conventions explícitas + return format cap de 30 linhas; (3) On return: validar via build/type-check (delegado a quem tem a ferramenta, não o orchestrator); (4) SEM spec, SEM state, SEM review gate — é operação cirúrgica.
- [x] Adicionar bloco de implementação javascript em `SKILL.md § Implementation` com o exemplo canônico: Grep calls → Task dispatch com prompt inline compondo guards + patterns + recipe + naming conventions + return format cap. Modelar após os blocos existentes de `refactor` e `audit`.
- [x] Adicionar à seção `## L0 Enforcement`: nota explícita de que `implement` NÃO lê código no orchestrator — os Greps são delegados via subagent ou são reads targetados de arquivos `.md` de contexto (não de código-fonte).
- [x] Adicionar exemplo CLI em `SKILL.md § Examples`: `/task implement "add logout button to header"` e `/task implement "create GET /api/users endpoint"`
- [x] Adicionar nota de quando usar `implement` vs `/feature` vs `refactor`: use `implement` para mudanças de 1-3 arquivos com pattern conhecido e resultado verificável por build; use `/feature` Light para mudanças estruturadas com spec auditável; use `refactor` para reorganização sem mudança funcional.
- [x] Se `references/implementation-examples.md` existir, adicionar um exemplo curto de uso do `implement` com o prompt composto mostrando guards+patterns+recipes inline. Se não existir, pular este item (não criar).
- [x] Rodar build + type-check: `rtk npm run build` na raiz do Mustard. Validar que `dist/` compila sem erros.

## Files (~2)

- `templates/commands/mustard/task/SKILL.md` (modify)
- `templates/commands/mustard/task/references/implementation-examples.md` (modify — condicional)

## Risks / Notes

- **Não adicionar lógica JS**: comandos `/mustard:*` são SKILL.md puros, não têm código executável. A "implementação" é a descrição procedural que o orchestrator segue. O exemplo javascript no SKILL.md é documentação, não código rodável.
- **Budget de Grep**: a instrução no SKILL.md deve deixar explícito que cada Grep em guards/patterns/recipes tem cap de output — não ler arquivos inteiros. `output_mode: content` + `-C 2` + `head_limit: 20` é um bom default.
- **Model default sonnet**: `implement` usa sonnet por default (não opus) porque é single-dispatch e padronização vem da injeção, não do modelo. Usuário pode forçar opus via sintaxe `/task implement --opus "..."` se precisar — mas isso é evolução futura, não escopo deste spec.
- **Sem review gate**: isso é intencional. Se o usuário quer review, roda `/task review` depois. Se quer o pipeline completo com gates, roda `/feature` Light. O `implement` é a opção "cirúrgica e rápida".
- **Validação via build**: o agent é instruído a rodar build/type-check no fim e reportar resultado. Se falhar, agent retorna `CONCERN` e orchestrator mostra pro usuário decidir.

## Concerns

- WARN missing-file: `templates/commands/mustard/task/references/implementation-examples.md` — referenced in checklist item but may not exist. Checklist explicitly marks this item as conditional (pular se não existir). Non-blocking.
