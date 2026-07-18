# Plano: enxugar o payload `.claude/` do mustard

> Análise prospectiva. Aprovar este plano ≠ executá-lo. Execução acontece em branch, commit por fase, com run de verificação entre fases (git é a rede).

## Régua de decisão (3 naturezas) — fundamento na doc oficial

Toda mudança de "reduzir/criar link" é classificada por natureza, porque a doc da Anthropic diz que **o destino certo depende do que a regra É**:

- 🔗 **REFERÊNCIA** (como o comando funciona, schema, exemplo) → vai para ref linkado, **um nível de profundidade**, com TOC se >100 linhas. *Seguro de-duplicar.*
- 📌 **AÇÃO** (o passo que deve rodar agora) → fica **inline** como step proeminente. *"Clear steps prevent Claude from skipping critical validation."*
- 🔒 **REGRA QUE NÃO PODE SER PULADA** → vira **hook** (determinístico). *"When there's something that absolutely must not happen, an instruction is the wrong tool… a real guardrail needs to be deterministic — hooks and permissions."*

Salvaguardas transversais (anti-pattern documentado de "IA pula o link"):
1. **Um nível de profundidade** a partir de cada SKILL — refs aninhados causam leitura parcial (`head -100`).
2. **TOC no topo** de todo ref >100 linhas.
3. **Links proeminentes** — link fraco é "missed connection".
4. **Nome descritivo** — sem nome vago (`helper`/`nudge`/anuncia-fluxo-morto).
5. Working `.claude/` **e** `apps/cli/templates/` mudam juntos; deploy nos alvos via `install.ps1` (`update --force`).
6. Cada fase = 1 commit em branch; run de verificação (instância limpa) entre fases; reverter se regredir.

---

## Fase 0 — Setup de segurança (mata o "medo de perder")

- Branch: `chore/payload-slim` a partir de `dev_rubens`.
- Commit isolado por fase (cada um revertível sozinho).
- Verificação entre fases: rodar um `/bugfix` + um `/task` representativos numa sessão NOVA e confirmar comportamento inalterado (método "Claude B fresh instance" da doc).
- **Re-confirmar cada órfão por grep no momento do corte** (não confiar só na auditoria).

---

## Fase 1 — Cortes puros (maior ganho de deploy, zero lógica de pipeline tocada)

| Item | Ação | Sinal / re-confirmação |
|---|---|---|
| `skills/skill-creator/` (252 KB) | **CUT-FROM-PAYLOAD** | Vendorado (anthropics/skills), inbound:1 (só `/mustard:skill`), Python-dependente (viola "Nunca Python"). **Decisão:** se `/mustard:skill create` NÃO é recurso de projeto-alvo → remover subtree do template; se É → no mínimo tirar cruft não-`.md` (`.pyc`, `__pycache__`, viewers HTML, LICENSE ≈180 KB) e fazer o `/skill` instalar sob demanda. |
| `refs/resume/handoff-summary.md` (2.1 KB) | **CUT** | inbound:0 repo-wide; aponta para `/resume` SKILL inexistente. Doc: "Ignored content → unnecessary". |
| `refs/concern-judge.md` (4.8 KB) | **CUT** (ou dobrar nota Haiku em digest-validate) | digest-validate diz "SUBSUMES the older concern-split judge". **Re-confirmar antes:** grep por dispatch de `concern-judge-render` em prosa de SKILL/ref — o comando rt existir é OK; um fluxo chamá-lo não é. |
| `context/qa/README.md` (1.2 KB) | **CUT-FROM-PAYLOAD** | Doc "como estender" p/ dev; nada carrega; poluiria o concat do agente QA — confirmar que o sync de qa globa `*.md` e excluí-lo. |

Ganho: **~258 KB** fora de todo `mustard init`, zero impacto de pipeline.
Commit: `chore(payload): cut dead/dev-only md (skill-creator, orphans, qa README)`

---

## Fase 2 — Enxugar `CLAUDE.md` always-on (maior ganho POR TURNO, Tier 0)

| Bloco | Natureza | Destino |
|---|---|---|
| `## Spec Layout` | 🔗 referência (quase-dup de pipeline-config) | mover p/ pipeline-config; ponteiro 1-linha |
| `### QA Phase` / `### Mid-pipeline change requests` | misto | 🔗 detalhe (paths NDJSON, nomes de hook) → pipeline-config; 📌 gatilho do gate fica inline |
| `## Context Loading` | 🔗 referência | mover p/ pipeline-config; ponteiro |
| `## Knowledge Capture` (`<MEMORY>`) | 📌/🔒 ação de fim-de-run | **NÃO mover sem verificar.** Se já há hook (Stop/SubagentStop) capturando memória → trim p/ ponteiro. Se NÃO → manter inline (mover = para de disparar). |
| Role, Response Style, Intent Routing, Routing economy, When-to-delegate, Efficiency, Locating-code ptr, Pipeline Phases | 📌 roteamento por-turno | **manter inline** |

Ganho: ~30% do always-on, re-injetado todo turno.
**Bloqueio:** verificar o hook de captura de memória ANTES de tocar Knowledge Capture.
Commit: `refactor(claude-md): move reference blocks to pipeline-config; keep triggers inline`

---

## Fase 3 — De-dup cross-SKILL (a parte que você temia) — por natureza

| Cluster duplicado | Natureza → destino |
|---|---|
| **Lapidação** (feature:24/26/29, task:33-42, bugfix:17-19) | 🔗 → consolidar em `locating-code.md` (já existe, já linkado por bugfix/task; add feature). 📌 a linha `mustard-rt run feature --intent` fica inline. Um nível de profundidade ✓ |
| **Invocação digest-validate** (feature:35-38, task:54-55, bugfix:21) | 🔗 mecanismo → cortar inline, fica em `digest-validate.md`. 📌 gatilho "rodar agora, obrigatório em strong" inline. 🔒 **e este é o candidato a HOOK** (Fase 5) — pulá-lo foi o defeito de campo |
| **Lexicon-feedback** (close:41-52 ≈ task:96-104) | 🔗 → novo `refs/lexicon-feedback.md` (um nível); 📌 gatilho por-SKILL inline |
| **Escalation statuses** (feature:56 inline) | 🔗 → feature linka `pipeline-config §Escalation Statuses` (bugfix já linka); remover cópia |
| **"passe o stub verbatim" do agent-prompt-render** (≥5 SKILLs) | 🔗 explicação → `agent-prompt.md`; 📌 a INVIOLABLE "NEVER hand-craft, always render" continua inline (é must-follow) |

Guardrail: cada ref consolidado fica **um nível** a partir de cada SKILL; não criar nesting novo.
Commit: `refactor(skills): de-dup reference prose into one-level-deep refs; keep action triggers inline`

---

## Fase 4 — Fragmentar refs grandes + renomear + merge

**Fragmentar** (🔗 referência on-demand; um nível + TOC):
- `feature/spec-language.md` (11.4) → tirar `## Component Contract` + `## Contexto narrative rules` → `refs/feature/narrative-and-contract.md`; spec-language linka. +TOC.
- `spec/resume-flow.md` (12.8) → cauda Escalation + Dependency Precheck (dup pipeline-config) → `refs/resume/escalation.md`. +TOC.
- `feature/wave-decomposition.md` (9.5) → seção COORDINATE → `refs/feature/coordinate.md` (opcional). +TOC.
- **TOC no topo de todo ref >100 linhas** (resume-flow, spec-language, full-plan, approve-only-flow, pipeline-config). *(Nota: approve-only-flow.md e resume-flow.md — colapsados em `refs/spec/resume-loop.md`; layout atual.)*

**Renomear** (nome descritivo; atualizar TODOS os inbound por grep):
- `refs/scan-enrich-purpose.md` → `recall-index.md` (inbound: digest-validate, scan SKILL).
- `refs/feature/glossary-nudge.md` → `glossary-grill.md` (inbound: feature SKILL).
- dir `refs/resume/` → avaliar mover sob `refs/spec/` (carregado por `/spec`).

**Merge** (baixa prioridade):
- `unhook` + `rehook` → 1 SKILL (toggle reversível, mesma tabela `--scope`).

⚠️ Passo mais arriscado: todo rename/move **quebra link** se um inbound não for atualizado (anti-pattern "missed connection"). Após cada rename: `grep` o basename antigo na árvore inteira → 0 ocorrências.
Commit: `refactor(refs): fragment large refs (one-level-deep+TOC), rename vague refs, merge unhook/rehook`

---

## Fase 5 — Promover a regra must-not-skip a HOOK (CONDICIONAL)

🔒 Regra "digest-validate obrigatório em `strong`; sem Explore/implement antes dele" → PreToolUse hook (block exit-2), fail-open, dispara uma vez, distingue o Task-do-validate do Task-bloqueado por subagent_type/model. Definição (porquê/como) fica em `digest-validate.md`; o hook é só o enforcement.

- É mudança Rust (`apps/rt`) → rebuild + testes, separada das fases markdown.
- **Gatada por reincidência:** as Fases 2-3 sobem a saliência inline; só construir o hook se o pulo RECORRER após isso (prosa primeiro, hook se a prosa falhar). Mustard já enforça assim nos gates existentes (close-gate, scope_guard, qa-gate).

---

## Ordem e ganho

1. **Fase 1** (cortes ~258 KB) — maior ganho de deploy, risco ~zero, diff fácil de revisar.
2. **Fase 2** (CLAUDE.md ~30%) — maior ganho por-turno. *Bloqueio: verificar hook de memória.*
3. **Fase 3** (de-dup) — encolhe SKILLs quentes; régua "🔗 como vs 📌 faça agora".
4. **Fase 4** (fragment + rename + merge) — limpeza; cuidado com inbound links.
5. **Fase 5** (hook) — condicional à reincidência do pulo.
