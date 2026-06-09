# wave-4-impl

## Resumo

Prosa: SKILL do /feature e refs do /spec passam a usar os comandos compostos + wave-dependency (local + templates)

## Rede

- Pai: [[menos-ia-mais-mustard-compor]]
- Depende de: [[wave-3-impl]]

## Tarefas

- [ ] - [ ] Em .claude/commands/mustard/feature/SKILL.md E apps/cli/templates/commands/mustard/feature/SKILL.md (manter os dois identicos nesse trecho): no PLAN passo 3, substituir a sequencia manual wave-scaffold->analyze-validation->emits por UMA chamada `mustard-rt run plan-materialize --spec-dir <dir> --plan <plan.json>`; adicionar instrucao de validar/derivar depends_on do plano com `mustard-rt run wave-dependency` (stdin = plan JSON) antes do materialize. NAO mudar a estrutura das fases nem o gate de aprovacao Full->/spec.
- [ ] - [ ] Em .claude/refs/spec/resume-flow.md E apps/cli/templates/refs/spec/resume-flow.md: o branch EXEC usa `mustard-rt run wave-advance --spec X` (prompts ja renderizados; despachar itens do mesmo nivel em UMA mensagem) no lugar de dispatch-plan + prompt_cmd por item; o CLOSE usa `mustard-rt run close-pipeline --spec X` no lugar da sequencia qa-run/complete-spec/pipeline-summary manual. Preservar as INVIOLABLE RULES existentes (nunca hand-craft, /spec aprova Full, etc.).
- [ ] - [ ] Revisar que NENHUMA outra prosa referencia os passos substituidos de forma conflitante (rg por wave-scaffold/dispatch-plan nos SKILL/refs locais e templates; ajuste citacoes obsoletas apenas onde contradizem o fluxo novo).
- [ ] - [ ] Verificar AC-7 com o rg do spec-pai. Prosa e runtime: sem rebuild necessario para este item, mas os comandos citados PRECISAM existir (onda 3).

## Arquivos

- `.claude/commands/mustard/feature/SKILL.md`
- `apps/cli/templates/commands/mustard/feature/SKILL.md`
- `.claude/refs/spec/resume-flow.md`
- `apps/cli/templates/refs/spec/resume-flow.md`
