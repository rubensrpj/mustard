# Plano: Specs mais didáticas e bilíngues consistentes

## Contexto

Auditoria das specs geradas pelo Mustard hoje (`/feature`, `/bugfix`) revelou três problemas reais:

1. **Excessivamente técnicas, pouco didáticas.** O `## Summary` é uma frase compacta de implementação ("Enriquecer o placeholder `{retry_context}` do template..."). Falta contexto narrativo (problema, motivação, restrições) que ajude o leitor humano a reentender o que está em jogo numa retomada.
2. **Mistura PT/EN sem critério.** Headers em inglês (`## Summary`, `## Files`, `## Boundaries`, `## Acceptance Criteria`) coexistem com corpo em português ("Enriquecer o placeholder...", "rewrite passo 3"). Code/comandos ficam em inglês (correto), mas labels e prosa se misturam — fica feio e dificulta leitura.
3. **`karpathy-guidelines` depende do agente lembrar.** A diretiva em `templates/CLAUDE.md:85` diz "agent SHOULD invoke Skill(karpathy-guidelines)" antes do primeiro Edit/Write em tarefas de código. É opcional. Skills não-mandatórias dependem do agente ler CLAUDE.md, lembrar da regra, e auto-invocar. Há gap consistente.

**Observação importante (após análise honesta do agent-prompt template existente):**

O dispatch template (`templates/commands/mustard/templates/agent-prompt/SKILL.md`) **já entrega per-agent context** ao agente via slots `{recommended_skills}`, `{recipe_context}`, `{task_steps}`. O orchestrator já filtra tasks por agente e passa skills relevantes. Não há gap real de "agent recebe spec.md inteira" — esse problema imaginado não existe. **Não vou adicionar per-agent files, wave-decomp em Light, sync-back, regeneração de derivados** — todos esses estavam resolvendo problemas inexistentes ou marginais. O escopo fica em 3 mudanças cirúrgicas.

---

## As 3 mudanças

### 1) `## Contexto` no topo do spec.md (didatismo)

**Onde:** `templates/commands/mustard/feature/SKILL.md` (e bugfix, e espelhos `.claude/`).

**O que:** PLAN, ao escrever `spec.md` (Light, Extended Light, ou Full), adiciona como primeira seção do corpo:

```markdown
## Contexto

{2-4 linhas em prose explicando:
 - Problema/situação atual
 - Por que importa (motivação, impacto)
 - Restrições conhecidas (quando aplicável)}
```

**Por que funciona sem riscos:**
- Adicionar 2-4 linhas no topo é uma operação aditiva isolada — não interage com nenhum outro mecanismo do pipeline.
- Spec-size-gate continua warnando em 200L; o Contexto não consome budget significativo.
- Specs históricas em `completed/` não são afetadas.

### 2) `### Lang: pt|en` no header + propagação ao dispatch

**Onde:** `templates/commands/mustard/feature/SKILL.md`, `templates/commands/mustard/bugfix/SKILL.md`, `templates/commands/mustard/templates/agent-prompt/SKILL.md` (e espelhos `.claude/`).

**Resolução de idioma (cascata sem heurística textual):**

1. **Header explícito** `### Lang: pt` ou `### Lang: en` no spec.md (re-runs/edits manuais) → respeita.
2. **Preferência do projeto** `specLang: "pt" | "en"` em `.claude/mustard.json` → usa.
3. **Caso contrário** (sem header e sem preferência): `AskUserQuestion` ÚNICA `"Spec language: pt | en?"`. A resposta é gravada em `mustard.json#specLang` para o projeto inteiro. **Nenhuma heurística com stopwords/diacríticos** — evitamos violar a regra "Mustard 100% agnóstico" (memory `feedback_mustard_agnostic`).

**Aplicação:**
- Headers de spec.md em PT (`## Contexto / Resumo / Limites / Arquivos / Tarefas / Critérios de Aceitação / Não-Objetivos / Concerns / Decisões / Dependências`) ou EN (`## Context / Summary / Boundaries / Files / Tasks / Acceptance Criteria / Non-Goals / Concerns / Decisions / Dependencies`).
- Prose, descrições, labels de AC.
- **Exceções (sempre EN):** Status/Phase/Scope values (`draft | implementing | completed`, `PLAN | EXECUTE | QA | CLOSE`, `light | extended-light | full`), comandos shell, identificadores de código, AC `Command:` field, header `### Lang:`.

**Propagação ao dispatch:**
- 1 linha nova no template `templates/commands/mustard/templates/agent-prompt/SKILL.md` (no bloco CONTEXT ou ROLE):
  ```
  Spec language is {spec_lang}. Use {spec_lang} for any prose, labels, and Concerns you add. Code/commands stay EN.
  ```
- Adicionar `{spec_lang}` à lista de placeholders documentados.
- Orchestrator preenche `{spec_lang}` lendo o header `### Lang:` da spec atual.

**Por que funciona sem riscos:**
- O header é a **única fonte de idioma** durante todo o pipeline. Nada lê de outro lugar — drift entre fases é impossível.
- Sem detecção heurística → sem caso patológico de detecção errada. Se user não definir, é perguntado uma vez, persistido pra sempre no projeto.
- 1 linha no template de dispatch tem custo zero em tokens (cabe na cache do prompt); benefício: agente reforça idioma certo nas suas adições.

### 3) `karpathy-guidelines` automático em `{recommended_skills}` para code edits

**Onde:** `templates/commands/mustard/feature/SKILL.md` EXECUTE phase + `templates/commands/mustard/bugfix/SKILL.md` EXECUTE + `templates/commands/mustard/resume/SKILL.md` (dispatch logic) + espelhos `.claude/`.

**O que:** quando orchestrator preenche `{recommended_skills}` no dispatch template, **prepende** `karpathy-guidelines` automaticamente para todos os agentes que vão editar código. Ou seja:
- Agentes que editam código (impl, backend, frontend, database, bugfix, etc.): `{recommended_skills} = "karpathy-guidelines, {outros relevantes ao subprojeto}"`
- Explore agents: NÃO recebem (são read-only).
- Review agents: NÃO recebem (review não edita).

**Implementação concreta:**
- Adicionar instrução no `## SKILLS` step de cada SKILL.md de pipeline (feature/bugfix/resume): "When filling `{recommended_skills}` for code-editing agents, prepend `karpathy-guidelines` first. Skip for Explore and Review agents."
- Atualizar `templates/commands/mustard/templates/agent-prompt/SKILL.md` § "How to fill `{recommended_skills}`" (linhas 126-138 hoje) para documentar a regra.

**Por que funciona sem riscos:**
- `{recommended_skills}` já é o slot canônico. Não cria mecanismo novo.
- Skill `karpathy-guidelines` já existe em `templates/skills/karpathy-guidelines/SKILL.md` — não muda.
- Diretiva atual em `templates/CLAUDE.md:85` continua válida; este é o reforço prático: quando `karpathy-guidelines` aparece em `{recommended_skills}`, o agente vê na primeira leitura do prompt e invoca a skill (description é triggerable).
- Sem hook validador, sem linha extra no agent-prompt template body.

---

## Arquivos a modificar

> Atualizar AMBAS as localizações (`templates/` é fonte da verdade para `mustard init`; `.claude/` é a sessão atual deste repo).

| Arquivo | Mudança |
|---------|---------|
| `templates/commands/mustard/feature/SKILL.md` (+ espelho) | (1) `## Contexto` na lista de seções de Light, Extended Light, Full; (2) Spec Language Resolution sub-seção (refer ao novo ref); (3) instrução "prepend karpathy-guidelines em `{recommended_skills}` para code editors" |
| `templates/commands/mustard/bugfix/SKILL.md` (+ espelho) | Mesmas 3 mudanças, adaptadas ao Full Path. Fast Path (sem spec) não muda. |
| `templates/commands/mustard/resume/SKILL.md` (+ espelho) | Mudança 3 apenas (instrução de prepend ao despachar) |
| `templates/commands/mustard/templates/agent-prompt/SKILL.md` (+ espelho) | (2) 1 linha extra `Spec language is {spec_lang}. Use {spec_lang} for prose...`; documentar placeholder `{spec_lang}`; atualizar §"How to fill `{recommended_skills}`" para mencionar karpathy automático |
| `templates/refs/feature/spec-language.md` (NEW + espelho) | Cascata de 3 layers (header > mustard.json > AskUserQuestion); tabela de headers PT↔EN; lista de exceções EN |

**Não modificar:**
- `templates/skills/karpathy-guidelines/SKILL.md` — sem alterações.
- `templates/hooks/spec-size-gate.js` — continua warn @200 / strict @500 como rede de segurança.
- `templates/CLAUDE.md:85` (diretiva karpathy SHOULD invoke) — mantida.
- `templates/refs/feature/wave-decomposition.md` — mantém comportamento atual (Full scope only). Caso patológico (Light >200L) é raro e não justifica mudança.
- Specs históricas em `.claude/spec/completed/` — não retroconvertidas.

---

## Verificação (end-to-end)

1. **Idioma PT, spec pequena:**
   - Disparar `/feature adicionar campo email no usuário` em projeto sem `mustard.json#specLang`
   - Esperado: AskUserQuestion única `"Spec language: pt | en?"`. Após "pt", spec.md tem header `### Lang: pt`, headers PT (`## Contexto / Resumo / Tarefas`), corpo PT.
   - `mustard.json#specLang = "pt"` gravado.

2. **Idioma EN, spec pequena:**
   - Em outro projeto-teste com `mustard.json#specLang = "en"` pré-configurado, disparar `/feature add email field`
   - Esperado: SEM AskUserQuestion (preferência já existe). Spec headers EN, header `### Lang: en`.

3. **Header explícito sobrepõe preferência:**
   - Em projeto com `specLang: "pt"`, criar spec.md manual com `### Lang: en`, rodar `/resume`
   - Esperado: dispatch usa `{spec_lang} = "en"`.

4. **karpathy-guidelines em code-edit dispatch:**
   - Em pipeline com agente backend/impl, capturar prompt do dispatch (via log ou inspeção)
   - Esperado: `{recommended_skills}` começa com `karpathy-guidelines`.
   - Para Explore agent: NÃO contém `karpathy-guidelines`.

5. **Contexto presente em spec gerada:**
   - Qualquer pipeline novo: spec.md tem `## Contexto` (ou `## Context`) como primeira seção do corpo após o header, com 2-4 linhas em prose.

6. **Tests existentes não quebram:**
   - `node --test templates/hooks/__tests__/hooks.test.js` (103/103 pass)
   - `node --test templates/hooks/__tests__/size-gates.test.js` (22/22 pass)
   - `npm run build` na raiz

---

## Decisões de design (riscos eliminados por construção)

### D1 — Drift de idioma entre fases → impossível
Single-source: `### Lang:` no header do spec.md é a única fonte. Dispatch lê dali, dispatcher não recompoe, agente não decide. Sem janela para divergir.

### D2 — Detecção de idioma errada → eliminada
Sem heurística textual (memory `feedback_mustard_agnostic` proíbe stopwords fixas). User decide explicitamente quando não há header/preferência: AskUserQuestion única, persiste em mustard.json. Caso patológico não existe.

### D3 — `## Contexto` inflando spec → contido pelo gate existente
spec-size-gate.js já warna @200 / strict @500. 2-4 linhas extras não movem a agulha em specs típicas (<100L). Em specs grandes, gate existente capture.

### D4 — karpathy duplicado em context (já em CLAUDE.md + agora em recommended_skills) → token cost zero
karpathy aparece como nome de skill no slot existente `{recommended_skills}`. SKILL.md só é carregado quando o agente de fato invoca via Skill tool — descrição triggerável faz a maioria invocar. CLAUDE.md continua sendo a diretiva de alto nível. Reforçar via slot é "uma direção, não 5".

---

## Por que NÃO está aqui

Itens descartados após análise honesta:

- **Per-agent files (`agents/{type}-wave-{N}.md`)** — duplicaria o que `{recommended_skills} + {recipe_context} + {task_steps}` já fazem inline. Adicionaria geração + regeneração + sync-back sem ganho mensurável.
- **Wave-decomp automático em Light/Extended Light >200L** — caso patológico raro nas specs reais do Mustard (a maioria <100L). Estender o trigger adicionaria complexidade para um cenário que praticamente não dispara.
- **Heurística de detecção de idioma** — viola "Mustard agnóstico". User explícito > adivinhação textual.
- **Hook validador de idioma/contexto** — "subtrair > adicionar". O dispatch já carrega a regra; sem enforcement extra.
