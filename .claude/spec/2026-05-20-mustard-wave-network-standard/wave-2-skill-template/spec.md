# Wave 2 — SKILL /feature força wave-files + agent-prompt injeta cross-wave memory

### Parent: [[2026-05-20-mustard-wave-network-standard]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full (wave)
### Checkpoint: 2026-05-20T21:00:00Z
### Lang: pt

## PRD

## Contexto

Com `mustard-rt run wave-scaffold`, `wikilink-extract` e `memory cross-wave` prontos (de [[wave-1-rt-infra]]), esta wave atualiza três artefatos textuais que definem o comportamento do `/mustard:feature` e `/mustard:resume`:

1. **SKILL `/feature`**: hoje "Wave Decomposition Pre-Check" é opt-in via `scope-decompose` com `roadmap signal`. Vira regra explícita: Full scope com sinais reais de dependência (`file_count≥6` OR `layer_count≥3` OR `independent_subbehaviors≥3`) **OBRIGATORIAMENTE** roda `mustard-rt run wave-scaffold` passando um plan JSON derivado da análise. O scaffold cria wave-plan.md + wave-N-{role}/spec.md + review/spec.md + qa/spec.md. Single spec.md neste cenário vira erro de scaffold detectável em QA.
2. **SKILL `/resume`**: antes do dispatch de cada wave N>1, chama `mustard-rt run memory cross-wave --spec <s> --wave N` e capta o stdout no placeholder `{cross_wave_memory}` do agent prompt. Para N=1 fica vazio.
3. **Agent prompt template**: `{cross_wave_memory}` documentado entre `{recipe_context}` e `{your_task}`.

Esta wave é puramente edição de markdown (SKILLs + ref) — nenhuma compilação de código. Validação por grep nos arquivos editados.

## Métrica de sucesso

- SKILL `/feature` tem seção "Wave Decomposition (mandatory)" com a regra explícita.
- Agent prompt template tem placeholder `{cross_wave_memory}` documentado.
- SKILL `/resume` (que dispara waves) sabe interpolar `{cross_wave_memory}` chamando o subcomando.

## Não-Objetivos

- Não alterar `/mustard:bugfix` (escopo bugfix continua o mesmo).
- Não tocar Light scope nem PLAN do Full pre-decomposição (a regra incide só na geração dos wave-files).
- Não migrar specs ativas existentes — regra incide só em /feature novas.

## Acceptance Criteria

- [ ] AC-1: SKILL `/feature` contém regra wave-files obrigatório E invoca `wave-scaffold` — Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/commands/mustard/feature/SKILL.md','utf8');if(!/Wave Decomposition.*mandatory|wave-files.*OBRIGAT/i.test(t))throw new Error('mandatory rule missing');if(!t.includes('wave-scaffold'))throw new Error('wave-scaffold invocation missing')"`
- [ ] AC-2: SKILL `/feature` referencia `mustard-rt run memory cross-wave` — Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/commands/mustard/feature/SKILL.md','utf8');if(!t.includes('memory cross-wave'))throw new Error('cross-wave reference missing')"`
- [ ] AC-3: SKILL `/resume` referencia `memory cross-wave` E lê `Modelo` do wave-plan — Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/commands/mustard/resume/SKILL.md','utf8');if(!t.includes('memory cross-wave'))throw new Error('cross-wave reference missing in resume');if(!/wave-plan.*Modelo|Modelo.*wave-plan/.test(t))throw new Error('Modelo-from-wave-plan rule missing')"`
- [ ] AC-4: Agent prompt ref documenta `{cross_wave_memory}` — Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/refs/agent-prompt/agent-prompt.md','utf8');if(!t.includes('{cross_wave_memory}'))throw new Error('placeholder not documented')"`

## Plano

## Arquivos (~3)

```
apps/cli/templates/commands/mustard/feature/SKILL.md      (modify — Wave Decomposition mandatory section + cross-wave hook)
apps/cli/templates/commands/mustard/resume/SKILL.md       (modify — chamar memory cross-wave antes do dispatch de wave N>1)
apps/cli/templates/refs/agent-prompt/agent-prompt.md      (modify — documentar {cross_wave_memory} placeholder)
```

## Tarefas

### General Agent

- [ ] Em `apps/cli/templates/commands/mustard/feature/SKILL.md`:
  - Renomear "Wave Decomposition Pre-Check" para "Wave Decomposition (mandatory for Full+deps)"
  - Substituir a heurística atual por: "Full scope com `file_count≥6` OR `layer_count≥3` OR `independent_subbehaviors≥3` → OBRIGATÓRIO rodar `mustard-rt run wave-scaffold --spec-dir <dir> --plan <plan.json>`. O scaffold cria wave-plan.md + wave-N-{role}/spec.md + review/spec.md + qa/spec.md. Single spec.md neste cenário é erro de scaffold."
  - Documentar formato do `plan.json` (referência ao schema do `wave-scaffold` subcomando)
  - Adicionar nota: "Antes do dispatch de cada wave N>1, SKILL `/resume` chama `mustard-rt run memory cross-wave --spec <spec> --wave N` para preencher `{cross_wave_memory}` no agent prompt."
- [ ] Em `apps/cli/templates/commands/mustard/resume/SKILL.md`:
  - Acrescentar passo "Cross-wave memory injection" no dispatch loop: antes de preencher o agent prompt, rodar `mustard-rt run memory cross-wave --spec <spec> --wave <N>` (se N>1) e capturar o stdout no `{cross_wave_memory}` placeholder do template.
  - Para N=1 (primeira wave): `{cross_wave_memory}` fica vazio.
  - Acrescentar passo "Model selection": ler a coluna `Modelo` da linha da wave no `wave-plan.md` do parent e passar como `model` no Task dispatch. Agente NUNCA escolhe; orquestrador (SKILL) é fonte de verdade. `model_routing` module continua bloqueando upgrades.
- [ ] Em `apps/cli/templates/refs/agent-prompt/agent-prompt.md`:
  - Adicionar entrada `{cross_wave_memory}` na tabela de placeholders com descrição "Markdown gerado por `mustard-rt run memory cross-wave` resumindo memórias das waves anteriores deste spec; vazio na primeira wave".
  - Posicionar entre `{recipe_context}` e `{your_task}` no exemplo de template.

## Dependências

- [[wave-1-rt-infra]]: precisa do subcomando `mustard-rt run memory cross-wave` existir.

## Network

- Parent: [[2026-05-20-mustard-wave-network-standard]]
- Depende de: [[wave-1-rt-infra]]
- Paralela a: [[wave-3-dashboard-graph]] (não compartilham arquivos)
- Recebe memória: [[wave-1-rt-infra]] (signatures dos subcomandos novos).
- Grava memória: `{skill_sections_added: [...], placeholder: '{cross_wave_memory}', notes: '...'}`.

## Limites

Em escopo: SKILLs `/feature` e `/resume`, ref `agent-prompt.md`.

Fora de escopo: código (mustard-rt já foi), dashboard, outras SKILLs.
