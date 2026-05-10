# Análise Hiper-Criteriosa: Mustard × Workshop "Desenvolvimento Assistido por IA" (PRD docs/prd.md)

> **Esta é a 3ª iteração** após confronto entre minha 1ª análise (otimista, baseada em docs), uma análise paralela (crítica) e validação web das afirmações canônicas do PRD. Esta versão é dimensionada para **produto vendável + curso de 2 dias**, com evidência arquivo:linha em cada afirmação e cross-reference com fontes públicas.

---

## 1. Contexto e premissa de qualidade

### 1.1 O produto a ser entregue

O workshop (imagem `docs/WhatsApp Image 2026-05-09 at 05.51.53.jpeg`, página `workshop-ia.techleads.club`) é um **curso de 2 dias** ("Em apenas dois dias") com 3 pilares:

1. **Refatorar 1.000+ linhas de código legado com confiança** — Dumb Zone (40% janela), SDD+RPI, compactação intencional, Vibe vs Assistido, code reviews otimizados.
2. **Onboard a IA em qualquer codebase complexo em minutos** — documentação efetiva, progressive disclosure em camadas, validação de suficiência, skills vs sub-agents, mental alignment.
3. **Construir features complexas de frontend sem parecer "código gerado"** — context engineering p/ componentes, IA + Browser, evitar estética AI-generated, fluxo FE, melhores MCPs.

O Mustard precisa ser **enabler do curriculum**: o aluno instala Mustard, e o curso ensina a usar IA via Mustard para entregar nas 3 trilhas. Logo, **Mustard precisa ter clareza, certeza e zero vaporware** em cada um dos 3 pilares.

### 1.2 Por que esta análise foi refeita

- 1ª passada: 85% cobertura — confiou em docs, perdeu vaporware.
- Análise paralela: ~30% — leu código, mas exagerou em overlap de hooks.
- Esta versão: **lê código + valida web + cita fonte + arbitra divergências**.

### 1.3 Padrão de evidência adotado

- **High confidence**: arquivo:linha lido + comportamento verificado por leitura ou teste.
- **Medium**: arquivo lido em parte, leve generalização.
- **Low / Assumido**: spot-check ou inferência. Marcado explicitamente.
- Web sources citadas para cada afirmação canônica do PRD.

---

## 2. Validação canônica do PRD (cross-reference web)

Antes de avaliar Mustard, valido se o PRD usa termos canônicos da indústria. Resultado: **PRD está alinhado com state-of-the-art 2025-2026** — não inventa terminologia.

| Conceito PRD | Status canônico | Fonte primária |
|---|---|---|
| **"Dumb Zone" 40% threshold** | **Cunhado por Dex Horthy (HumanLayer)**. Consenso community: ≥40% janela = degradação. Mikko Ohtamaa (Twitter): "everyone agrees the boundary exists". | DEV Community ([Escaping the Dumbzone Part 1](https://dev.to/diggidydale/escaping-the-dumbzone-part-1-why-your-ai-gets-stupider-the-more-you-talk-to-it-4d8k)), Syntackle ([A Million Token Context Window Isn't What You Think It Is](https://syntackle.com/blog/long-context-window-ai-model-catch/)), [Mikko Ohtamaa on X](https://x.com/moo9000/status/2014656102290403810) |
| **Lost in the Middle (mecanismo)** | Liu et al. 2023, arXiv:2307.03172, **TACL 2024** (peer-reviewed). U-shaped attention, ≥30% degradação no meio. | [arXiv:2307.03172](https://arxiv.org/abs/2307.03172), [ACL Anthology TACL 2024](https://aclanthology.org/2024.tacl-1.9/) |
| **Spec-Driven Development (SDD)** | Formalizado em **GitHub Spec Kit** (open-source oficial GitHub). Compatível com Claude Code, Copilot, Gemini CLI. Microsoft Learn tem treinamento. Martin Fowler escreveu sobre. | [github/spec-kit](https://github.com/github/spec-kit), [GitHub Blog](https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/), [Martin Fowler "SDD-3-tools"](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html) |
| **Research-Plan-Implement (RPI)** | Loop canônico para evitar Dumb Zone. Adoção crescente community 2025-2026. | [Context Engineering — Medium](https://medium.com/@rajesh.godavarthi/context-engineering-what-ai-builders-know-that-you-dont-5-counter-intuitive-lessons-from-the-8435308183ca), [12 Factor Agents](https://paddo.dev/blog/12-factor-agents/) |
| **MCPs Frontend** | Dois principais: **Playwright MCP** (Microsoft, scripted interaction via accessibility tree) + **Chrome DevTools MCP** (Google, debugging/perf/network). Complementares, não alternativos. | [Webfuse 5 best MCP browser](https://www.webfuse.com/blog/the-top-5-best-mcp-servers-for-ai-agent-browser-automation), [ChromeDevTools/chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp), [Steve Kinney "Playwright vs DevTools MCP"](https://stevekinney.com/writing/driving-vs-debugging-the-browser) |

**Conclusão:** o PRD do Mustard é canônico. Cada um dos 5 conceitos centrais tem fonte pública citável. **Mustard ENTREGAR isso = entregar canon.** Falhar em qualquer um = ficar atrás de GitHub Spec Kit (oficial, gratuito).

---

## 3. Posicionamento estratégico (concorrência)

### 3.1 Concorrentes diretos identificados (web)

| Produto | Posicionamento | Diferenciação vs Mustard |
|---|---|---|
| **GitHub Spec Kit** | Open-source oficial GitHub. 4 fases (Research/Spec/Plan/Implement). Multi-tool: Copilot, Claude Code, Gemini. Microsoft Learn training. | **Concorrência mais séria**. Aluno do curso pode preferir por marca + multi-tool. Mustard precisa diferenciação. |
| **Kiro** | (Martin Fowler cita) | Menos info; provável foco enterprise. |
| **Tessl** | (Martin Fowler cita) | Idem. |

### 3.2 Diferenciadores de Mustard (verificados)

| Capacidade | Mustard | GitHub Spec Kit |
|---|---|---|
| Hooks JS de enforcement (Pre/PostToolUse, SessionStart, etc) | **31 hooks** (verificado em `templates/hooks/`) | Não nativo |
| RTK token economy integrada | **`hooks/rtk-rewrite.js`** + 60-90% redução em CLI outputs | Não nativo |
| Wave decomposition automática | **`scope-decompose.js` + `wave-dependency.js` + `exec-rewave-check.js`** (`commands/mustard/feature/SKILL.md:76-80, 144`) | Não nativo |
| QA gate executável (Wave 10) | **`qa-run.js` + close-gate** com 3 iter max + AskUserQuestion (`feature/SKILL.md:176-178`) | Manual |
| Existence Gate pré-EXECUTE | **Haiku Task verifica se trabalho ainda é necessário** (`feature/SKILL.md:135-139`) | Não |
| Cross-shell AC commands | **Pattern para Windows cmd.exe** (`feature/SKILL.md:196-200`) | Não |
| Multi-language spec (PT/EN) | **Resolution cascade + 14 header translations** (`spec-language.md`) | EN-only |
| Auto-checklist marking | **`checklist-auto-mark.js` + `mark-checklist-item.js`** | Não |
| Knowledge base ranked confidence × recency | **`session-memory.js` + `knowledge-update.js`** | Não |

**Conclusão:** Mustard tem **vantagens reais** sobre Spec Kit em automação Claude-Code-first, multilíngue PT/EN, e enforcement via hooks. Estes diferenciadores são vendáveis no curso.

### 3.3 Risco competitivo

- **Aluno descobre Spec Kit antes do curso** e questiona "por que não usar Spec Kit?" → curso precisa preparar resposta clara.
- **Spec Kit evolui rápido (oficial GitHub)** → Mustard precisa cadência de melhoria.
- **Vaporware no Mustard prejudica trust** — aluno percebe doc drift e questiona o resto do produto.

---

## 4. Auditoria rigorosa do estado atual

### 4.1 Inventário verificado (Glob + Bash count)

| Componente | Declarado em CLAUDE.md | Verificado | Drift |
|---|---|---|---|
| Hooks | 20 (`templates/CLAUDE.md:41`) | **31** (`templates/hooks/*.js`) | **+55%** |
| Scripts | 13 | **25** | +92% |
| Slash commands | 17 | **18** | +6% |
| Foundation skills | 6 | **7** | +17% |
| Test files | (não declarado) | **19** em `__tests__/` | n/a |
| `.core.md` agents | 6+1 (CLAUDE.md §Context Architecture v3.0) | **1** (apenas `qa.core.md`) | **−86%** |
| Recipes | "Recipe Engine" (`pipeline-config.md`) | **0** em `.claude/recipes/` | **vaporware** |
| `memory/decisions.json` | "Persistent projection" (`pipeline-config.md`) | **diretório não existe** | **vaporware** |
| `memory/lessons.json` | idem | idem | **vaporware** |

### 4.2 Hooks deletados ainda referenciados

Commit `129b73d` (refactor: remove deprecated hooks):
- `mcp-budget.js` — **referenciado em `CLAUDE.md` raiz §Enforcement Hooks**
- `regression-guard.js` — **referenciado em `pipeline-config.md` §Anti-slope hooks**
- `debug-loop-guard.js` — não verificado em docs
- `epic-detect.js` — não verificado em docs

### 4.3 Sistemas: funcional / parcial / vaporware

#### 4.3.1 ✅ Funcional (alta confiança, evidência arquivo:linha)

| Sistema | Evidência |
|---|---|
| **Pipeline phases ANALYZE→PLAN→EXECUTE→QA→CLOSE** | `commands/mustard/feature/SKILL.md` 217 linhas |
| **Spec language EN/PT cascade** | `refs/feature/spec-language.md:5-13` |
| **Spec hygiene SessionStart audit** | `refs/feature/spec-hygiene.md:1-23` + `templates/settings.json:260-263` |
| **Wave decomposition (Wave 7)** | `feature/SKILL.md:76-80` + `scope-decompose.js`, `wave-dependency.js`, `exec-rewave-check.js` (`scripts/`) |
| **QA gate (Wave 10)** | `feature/SKILL.md:176-178` + `scripts/qa-run.js` + `close-gate.js` |
| **Existence gate pre-EXECUTE** | `feature/SKILL.md:135-139` + `refs/feature/existence-gate.md` |
| **Diff context interpolation** | `feature/SKILL.md:25` + `scripts/diff-context.js` (cap 3000 chars, cached per phase) |
| **7-category review** | `pipeline-execution/SKILL.md:92-103` |
| **Acceptance Criteria com runnable commands** | `feature/SKILL.md:119` (formato `- [ ] AC-1: {desc} — Command: \`{cmd}\``) |
| **AC cross-shell Windows** | `refs/feature/ac-cross-shell.md` (referenced) |
| **31 hooks ativos com fail-open** | `templates/settings.json` 363 linhas; `bash-safety.js`, `file-guard.js`, `close-gate.js`, `enforce-registry.js`, `model-routing-gate.js`, `context-budget.js`, `output-budget.js`, `pre-compact.js`, `session-knowledge.js`, `session-knowledge-inc.js`, `session-memory.js` lidos integralmente ou em parte |
| **Token economy: RTK + budgets per-role + tool-use cap** | `context-budget.js:48-52` (Explore 10K chars / review 12K / general 30K hard block); `output-budget.js:25-30` (lines 30/40/60/80 advisory); `tool-use-counter.js` (cap 15-20) |
| **Knowledge.json ranked confidence × recency** | `session-memory.js:24-25, 41-44`; entries verificadas em `.claude/knowledge.json` |
| **harness-views.js** | 993 linhas, real, com `buildAgentVisibility`, `buildPipelineState` (verificado via head) |
| **events.jsonl (truth source)** | `harness-init.js` rotaciona; `metrics-tracker.js` emite |
| **Skills foundation: karpathy, design-craft, react-best-practices, senior-architect, skill-creator, commit-workflow, pipeline-execution** | `templates/skills/*/SKILL.md` (7 verificados) |
| **Auto-checklist marking** | `checklist-auto-mark.js` hook + `scripts/mark-checklist-item.js` |
| **Spec layout progressive disclosure** | `feature/SKILL.md:188-194` (≥200 linhas → spec-references/, hard block 500) |
| **Permission deny destructive ops** | `templates/settings.json:41-56` (rm -rf, force push, reset --hard, etc) |
| **enforce-registry.js gate** | `enforce-registry.js:35-44` (bloqueia /feature, /bugfix se registry missing) |
| **model-routing-gate** | `model-routing-gate.js:12-19, 32-33`: Explore→haiku, Plan/Feature/Bugfix→opus, default→sonnet; **upgrades blocked, downgrades allowed** |

#### 4.3.2 ⚠️ Parcial (existe mas com lacunas relevantes)

| Sistema | Lacuna |
|---|---|
| **Memory writer (decisions/lessons)** | `memory-write.js` referenciado em `feature/SKILL.md:154` (step 8b) e `memory-persist.js` existe em `scripts/`. **Não há hook automático**. Depende de Claude invocar manualmente. Diretório `.claude/memory/` **não existe** neste projeto → orquestrador não tem invocado. |
| **Pre-compact é snapshot, não compactação** | `pre-compact.js:64-137` cria snapshot (branch, status, commits, active pipelines, memory counts) e injeta via `additionalContext`. **Reativo**, não estratégico. PRD pede "compactação intencional" — Mustard tem cap de output budget (advisory) + per-role context budget (block). |
| **Dumb Zone threshold** | `context-budget.js:46`: `TOKEN_THRESHOLD = 50000` **absoluto**. Em janela 200K = 25% (abaixo do 40% canônico). Em janela 1M = 5% (totalmente prematuro). **Não calibra por modelo.** |
| **Compactação intencional** | `feature/SKILL.md:71-74`: "Compact Advisory" sugere `/compact` se ANALYZE foi pesado. Advisory only — usuário decide. |
| **Vibe vs Assistido** | `/mustard:task` é vibe-like (sem spec, sem hygiene). Light/Extended Light/Full são auto-detectados. **Não há flag explícita "vibe" vs "assistido"**. |

#### 4.3.3 ❌ Vaporware (referenciado mas inativo/inexistente)

| Sistema | Evidência da inatividade | Severidade p/ curso |
|---|---|---|
| **`.core.md` per-agent (backend/frontend/database/bugfix/review/orchestrator)** | `templates/CLAUDE.md` §Context Architecture v3.0 promete; **só `qa.core.md` existe** (Glob). Sistema real é skill+commands+CLAUDE.md-driven. | **Crítica** — aluno lê CLAUDE.md, procura `.core.md`, não acha → desconfia produto |
| **Recipe Engine (`.claude/recipes/*.json`)** | `pipeline-config.md` §Recipe Engine: "skeleton 90% complete". `recipe-match.js` real em scripts; **diretório `.claude/recipes/` não existe** | **Alta** — economia de tokens prometida não dispara |
| **`memory/decisions.json` + `lessons.json`** | `pre-compact.js:124-130` lê; `session-memory.js:36` aponta para memDir; **diretório não existe** | **Alta** — Decision Log do PRD §RF3 não materializa |
| **`mcp-budget.js`** referenciado em `CLAUDE.md` raiz | Hook deletado em commit 129b73d | Média — doc lying |
| **`regression-guard.js`** referenciado em `pipeline-config.md` | Idem deletado | Média — doc lying |

### 4.4 Análise de overlap de hooks (verificada por leitura, não conjectura)

A análise paralela alegou "31 hooks com overlap → fundir 31→15". **Verificação por código mostra que essa redução destruiria funcionalidade.**

| Suspeito | Verificação | Veredito |
|---|---|---|
| `session-knowledge` + `-inc` + `session-memory` | **Complementares**. Memory: SessionStart load (`templates/settings.json:254`); -inc: PostToolUse(Task) com throttle 3/h, idempotência 24h (`session-knowledge-inc.js:25-27`); -knowledge: SessionEnd extract (`templates/settings.json:293`). `session-knowledge.js:55-58` skipa se `-inc` rodou <5min antes. | **Manter os 3** |
| `output-budget` + `context-budget` | **Fases distintas**. context: PreToolUse(Task) hard block input (`context-budget.js:88-152`); output: PostToolUse(Task) advisory output (`output-budget.js:65-103`). | **Manter ambos** |
| `bash-safety` + `bash-native-redirect` | **Diferentes**. safety: bloqueia rm/mkfs/dd/credentials; redirect: força grep→Grep, ls→Glob, cat→Read | **Manter ambos** |
| `spec-size-gate` + `skill-size-gate` | Heurística similar (linhas → warn). | **Manter hooks; extrair `_lib/size-gate.js`** (S effort) |
| `subagent-tracker` reuso | Roda em PreToolUse(Task), PostToolUse(Task), SessionStart, SubagentStart, SubagentStop (5 wirings). Estado consistente exige presença em todos. | **OK** |
| `tool-use-counter` reuso | PreToolUse(.*), SessionStart, SubagentStart, SubagentStop. Idem. | **OK** |

**Conclusão:** dos suspeitos, apenas `spec-size-gate` + `skill-size-gate` têm extração legítima de lib comum (não merge dos hooks).

---

## 5. Mapeamento PRD item-a-item com confidence

### 5.1 Pilar 01 — Refatorar 1.000+ linhas de código legado

#### 01.1 — A "Dumb Zone": >40% janela degrada
- **PRD pede:** mitigação via camadas + compactação intencional.
- **Mustard tem:**
  - `tool-use-counter.js` cap 15-20 tool uses por agent.
  - `context-budget.js:48-52` budgets POR ROLE em chars (Explore 10K=2.5K tokens, review 12K=3K tokens, general-purpose 30K=7.5K tokens) — **hard block** em strict mode.
  - `output-budget.js:25-30` budgets de retorno por role (advisory).
  - RTK rewrite (60-90% redução em CLI outputs).
  - **Wave decomposition** (`scope-decompose.js`) quando >5 files / >3 layers detectados.
- **Gap real:** `context-budget.js:46` advisory threshold é **`TOKEN_THRESHOLD = 50000` absoluto** sobre tamanho de `.md` referenciado. Não calcula % de janela do modelo. **Não implementa "Dumb Zone 40%" canônico.**
- **Confidence:** **High** (lido código completo).
- **Coverage:** **75%** — multi-camada existe; threshold canônico não.
- **Severidade gap:** **P0 para curso** — Dumb Zone 40% é o slogan do pilar 1. Não ter == perder argumento de venda.

#### 01.2 — SDD + RPI
- **PRD pede:** Spec-Driven Development + Research-Plan-Implement com artefatos.
- **Mustard tem (mapeamento RPI):**
  - **Research = ANALYZE phase** (`feature/SKILL.md:21-74`): registry-first, Path A skip Explore se entity conhecida; Path B Explore "medium" para novo. Cap 5 reads, max 1 Explore com ≤10 tool uses (Light) ou ≤20 (Full).
  - **Plan = PLAN phase** (`feature/SKILL.md:87-134`): spec.md com Status/Phase/Scope/Lang/Context/Summary/Boundaries/Files/Plan/AC/Concerns/Decisions/Dependencies. AC com runnable commands.
  - **Implement = EXECUTE phase** (`feature/SKILL.md:141-174`): Wave 1 (DB+Backend), Wave 2 (FE+Mobile), per-spec recipe match, karpathy mandatory antes 1º Edit/Write.
- **Em comparação com GitHub Spec Kit (4 fases canônicas Research/Spec/Plan/Implement):** Mustard colapsou Spec+Plan em PLAN. AC executable é equivalente. **Coverage funcional: 100%; nomeação não-canônica.**
- **Confidence:** **High**.
- **Coverage:** **95%** — falta apenas marketing/documentação dos termos canônicos.
- **Gap:** doc não usa "RPI" / "SDD" como labels. Aluno do curso precisa traduzir mentalmente. **Trivial fix:** adicionar tabela em CLAUDE.md mapeando ANALYZE↔Research, PLAN↔Spec+Plan, EXECUTE↔Implement.

#### 01.3 — Compactação intencional + gerenciamento sistemático de contexto
- **PRD pede:** Project Context (L0/L1), Module Briefs (L2), Open Questions, Assumptions, Decision Log.
- **Mustard tem:**
  - **L0/L1:** `CLAUDE.md` raiz + `templates/CLAUDE.md` ✅
  - **L2:** `{subproject}/CLAUDE.md` + `commands/{stack,patterns,guards,recipes,notes}.md` (gerados por `/scan`) ✅
  - **L3:** `entity-registry.json` grep on-demand ✅
  - **Decision Log:** `memory/decisions.json` — **vaporware** (4.3.3)
  - **Lessons:** `memory/lessons.json` — **vaporware**
  - **Open Questions:** sem slot dedicado; `## Concerns` cobre parcialmente (`spec-language.md:43`)
  - **Assumptions:** sem slot
  - **knowledge.json:** real, ranked confidence × recência
  - **Compactação:** `feature/SKILL.md:71-74` Compact Advisory após ANALYZE pesado; `pre-compact.js` snapshot reativo
- **Confidence:** **High**.
- **Coverage:** **55%** — L0-L3 OK; Decision Log/Lessons vaporware; Open Q sem slot.

#### 01.4 — Vibe Coding vs Desenvolvimento Assistido
- **PRD pede:** flag explícita.
- **Mustard tem:** `/mustard:task` (vibe-like, sem spec) + Light/Extended Light/Full auto-detect (`feature/SKILL.md:42-47`). **Não há flag user-facing "vibe" vs "assistido"**.
- **Confidence:** **High**.
- **Coverage:** **70%** — capability existe, naming não.

#### 01.5 — Code reviews otimizados
- **PRD pede:** PRs menores, racional, "como revisar", evidências.
- **Mustard tem:**
  - 7-category review (`pipeline-execution/SKILL.md:92-103`): SOLID/DS/Patterns/i18n/Integration/Build/Elegance.
  - `/mustard:review` skill.
  - `review-gate.js` PreToolUse Bash `git commit` checa secrets.
  - `/mustard:git push` é **ff-only sem PR** (`commands/mustard/git/SKILL.md:16, 28-29`); externos via `gh pr create` manual.
  - **Sem `templates/.github/pull_request_template.md`**.
- **Confidence:** **High**.
- **Coverage interna:** **80%** / **Coverage PR externa:** **0%**.

### 5.2 Pilar 02 — Onboard a IA em codebase complexo

#### 02.1 — Documentação efetiva (rodar/testar/debugar + arquitetura + glossário)
- **PRD pede:** checklist + Project Overview template.
- **Mustard tem:** `CLAUDE.md` raiz (Build & Run, Structure, CLI Flow), `{subproject}/CLAUDE.md` por subproject, `entity-registry.json`, `commands/{stack,patterns,guards}.md` gerados por `/scan`.
- **Gap real:** **sem glossário humano de domínio**. Entity registry tem refs/subs sem descrições.
- **Confidence:** **Medium** (não li `commands/{stack,patterns,guards}.md` reais, só vi referências).
- **Coverage:** **80%**.

#### 02.2 — Progressive disclosure (contexto em camadas)
- **PRD pede:** L0/L1/L2/L3 com lazy loading.
- **Mustard tem:**
  - L0-L3 mapeados (5.1.01.3 acima)
  - **Skills auto-trigger** (capability lazy load por descrição) — superior a L0-L3 simples.
  - **Recipes** matched por entity+operation — vaporware (4.3.3).
  - **Spec layout progressive** (`feature/SKILL.md:188-194`): spec.md ≤200 linhas; >200 extrai para `spec-references/`; hard block 500.
  - **Refs progressive disclosure**: `templates/refs/{cmd}/*.md` — comandos referenciam refs em vez de inline.
- **Confidence:** **High**.
- **Coverage:** **80%** — funcional; recipes vaporware; `.core.md` agente identity vaporware.

#### 02.3 — Validação de suficiência (checklist prático)
- **PRD pede:** "explique fluxo E2E, proponha plano, liste riscos, propose tests".
- **Mustard tem:**
  - ANALYZE phase exige registry-first + ≤5 reads + escalation BLOCKED implícito (`feature/SKILL.md:21-74`).
  - PLAN exige spec.md com AC + Boundaries + Files (`feature/SKILL.md:111-134`).
  - **`scripts/analyze-validation.js`** rodado no fim de ANALYZE (`feature/SKILL.md:84-85`); issues vão para `## Concerns`.
  - Escalation statuses CONCERN/BLOCKED/PARTIAL/DEFERRED.
- **Gap:** validação é AUTOMATIZADA, não checklist user-facing. PRD pede aluno **fazer perguntas explícitas à IA**. Mustard automatiza, aluno pode não ver o checklist.
- **Confidence:** **High**.
- **Coverage:** **85%** — sistema valida; pedagogicamente, aluno do curso precisa do checklist visível.

#### 02.4 — Skills/rules vs sub-agents
- **PRD pede:** política.
- **Mustard tem:** 7 foundation skills + skill-generator dinâmico via `/scan`; Wave 1/2 dispatch; `model-routing-gate.js` enforça model vs scope; `recommended-skills-audit.js` warn >10 skills; `pipeline-config.md §Model Selection` table.
- **Confidence:** **High**.
- **Coverage:** **100%**.

#### 02.5 — Mental alignment (rastreabilidade, responsabilidade humana, segurança)
- **PRD pede:** "por que mudou", aprovação humana, padrões.
- **Mustard tem:** spec.md com `## Decisões não-óbvias`; `/mustard:approve`; `review-gate.js` pre-commit secrets; `bash-safety.js` + `permissions.deny`; 7-category review §i18n + §SOLID; escalation BLOCKED força input humano.
- **Confidence:** **High**.
- **Coverage:** **90%**.

### 5.3 Pilar 03 — Frontend sem estética "AI-generated"

#### 03.1 — Context engineering p/ componentes
- **PRD pede:** template de spec de componente (props/states/variants/responsividade/a11y).
- **Mustard tem:** `design-craft` skill (palettes/typography/principles/validation/ux-guidelines/critique/styles-catalog), `react-best-practices` skill (rerender/bundle/server/client/async/advanced); spec.md genérica.
- **Gap:** **sem sub-template "Component Contract"** estruturando props/states/variants/breakpoints/a11y/DS tokens.
- **Confidence:** **High**.
- **Coverage:** **45%**.

#### 03.2 — IA + Browser
- **PRD pede:** fluxo reproduce → isolate → instrument → fix → prevent.
- **Mustard tem:** Playwright MCP plugin disponível (system-reminder lista 25+ browser tools); `pipeline-config.md §Diagnostic Failure Routing` (Internal/Transient/Resolvable/Structural — genérico).
- **Gap:** **sem playbook FE-específico**. Routing genérico não orienta uso de browser MCP. **Falta documentação Playwright vs Chrome DevTools MCP** (canônico web 2026 — ambos complementares).
- **Confidence:** **High**.
- **Coverage:** **30%**.

#### 03.3 — Evitar estética "AI-generated"
- **PRD pede:** checklist consistência DS, edge cases, microinterações, a11y.
- **Mustard tem:** `design-craft` skill (princípios, validação), `karpathy-guidelines` mandatory pre Edit/Write, 7-category review §Design System.
- **Gap:** **sem checklist explícito anti-AI-look**. Material disperso em design-craft sub-references.
- **Confidence:** **High**.
- **Coverage:** **45%**.

#### 03.4 — Fluxo de Desenvolvimento Frontend
- **PRD pede:** SOP (Spec → protótipo → impl → testes → obs → release).
- **Mustard tem:** Wave 2 dispatcha FE depois de Wave 1, QA com AC, CLOSE com build/lint/test.
- **Gap:** "protótipo" não tem slot; "observabilidade" fora do escopo Mustard (depende de infra do projeto-alvo); "release" via `/mustard:git merge main`.
- **Confidence:** **High**.
- **Coverage:** **70%**.

#### 03.5 — Melhores MCPs Frontend
- **PRD pede:** tabela comparativa.
- **Mustard tem:** Playwright MCP via plugin. **Sem documentação Mustard sobre MCPs FE**. `mcp-budget.js` deletado.
- **Gap canônico:** PRD precisa dizer **Playwright (interação) + Chrome DevTools (debugging)** — fonte web 2026.
- **Confidence:** **High**.
- **Coverage:** **30%**.

### 5.4 Requisitos Funcionais

| RF | PRD | Mustard | Coverage |
|---|---|---|---|
| RF1 Vibe vs Assistido | flag + spec/plano/testes obrigatórios em Assistido | `/mustard:task` (vibe não-nomeado) + Light/Full auto | **70%** |
| RF2 RPI default | toda iniciativa via Research → Plan → Implement | ANALYZE → PLAN → EXECUTE → QA → CLOSE com artefatos | **95%** (rename) |
| RF3 Context Pack versionável | Project Ctx + Module Briefs + Decision Log + Open Q | L0-L3 ✅; Decision Log vaporware; Open Q sem slot | **55%** |
| RF4 PR template obrigatório | "como revisar", riscos, evidências | sem `.github/pull_request_template.md` | **0%** externo / **80%** interno |

### 5.5 Métricas

| PRD | Mustard | Status |
|---|---|---|
| Lead time PR | events.jsonl tem tool/agent/phase, **sem `pr.opened/merged`** | Não tem |
| Tempo review | sem `review.start/end` | Não tem |
| Tamanho PR | `diff-context.js` calcula mas não persiste | Parcial |
| Defeitos pós-merge | sem GH webhook | Fora de escopo legítimo |
| % tarefas com spec | derivável de events (`/feature` = spec, `/task` = sem) | Derivável |

**Coverage métricas:** **40%**.

### 5.6 Cobertura agregada (verificada)

| Pilar | Cobertura |
|---|---|
| 01 Refatoração legado | **75%** |
| 02 Onboard IA | **80%** |
| 03 Frontend qualidade | **40%** |
| RFs (1-4) | **65%** |
| Métricas | **40%** |
| **Total ponderado** | **65%** |

> **Crítica honesta:** minha 1ª análise (85%) era inflada. Análise paralela (~30%) era subestimada. Esta versão (65%) **arbitra com evidência**.

---

## 6. Gaps reais priorizados (ótica de produto/curso)

### Tier 0 — Higiene crítica (bloqueia confiança no produto)

#### G-0a: Doc drift severo em `templates/CLAUDE.md` e `CLAUDE.md` raiz
- **Problema:** "20 hooks" (real 31), "13 scripts" (real 25), "6 skills" (real 7), `.core.md` arquitetura promised vaporware, `mcp-budget.js`/`regression-guard.js` referenciados mas deletados.
- **Impacto curso:** aluno lê doc, procura `.core.md`, não acha → desconfia produto.
- **Fix:** sync contagens; deletar seção "Context Architecture v3.0" (subtração > adição); remover refs a hooks deletados.
- **Effort:** XS (1-2h).
- **Confidence:** High.

#### G-0b: Dumb Zone canônico (40% janela do modelo)
- **Problema:** `context-budget.js:46` usa `TOKEN_THRESHOLD = 50000` absoluto, não % janela.
- **Impacto curso:** "Dumb Zone 40%" é slogan do pilar 1 do workshop. Não implementar = perder claim diferenciador. Em janela 1M (Opus 1M, 200K Sonnet/Opus padrão), 50K não é o threshold canônico citado por Dex Horthy/Mikko Ohtamaa.
- **Fix:**
  ```js
  const WINDOW_BY_MODEL = {
    haiku: 200_000, sonnet: 200_000, opus: 200_000,
    'opus-4-7-1m': 1_000_000, // model id pattern from data.model
  };
  const WARN_PCT = 0.40;
  const COMPACT_PCT = 0.65;
  ```
- **Tests:** atualizar `__tests__/integration.test.js` para testar com diferentes model windows.
- **Effort:** S (~1h).
- **Confidence:** High.

#### G-0c: Vaporware de memory writer
- **Problema:** `memory-write.js` (script) referenciado em `feature/SKILL.md:154`. **Não há hook**. Diretório `.claude/memory/` não existe → orquestrador não tem invocado.
- **Impacto curso:** Decision Log e Lessons (PRD §RF3) não materializam. Cross-session learning fica só em `knowledge.json`.
- **Fix:** ou (A) wirar `memory-persist.js` em hook PostToolUse(Task) com throttle, ou (B) reforçar instrução em `feature/SKILL.md` step 8b com "MUST" e adicionar ao karpathy-guidelines.
- **Recomendação:** **(A) wirar via hook**. Mais robusto, menos dependente de disciplina de orquestrador.
- **Effort:** S (~1-2h).
- **Confidence:** High.

#### G-0d: Recipe Engine sem recipes (vaporware)
- **Problema:** `pipeline-config.md §Recipe Engine` promete "skeleton 90% completo". `.claude/recipes/` não existe.
- **Impacto curso:** se o curso quer ensinar economia de tokens via recipes, demonstração falha.
- **Fix:** seed mínimo (3-5 recipes para operações comuns: add-field, add-endpoint, add-component, add-migration, add-form-validation). Cada recipe ~20 linhas JSON.
- **Effort:** M (~6-10h).
- **Confidence:** High.

### Tier 1 — Diferenciadores do curso (alto valor / S effort)

#### G-1a: Anti-AI-look ref para FE
- Criar `templates/refs/feature/fe-craft-check.md` (~40 linhas): tokens DS (não cores literais), estados loading/empty/error/success, microinterações em ações (hover/focus/active), `prefers-reduced-motion`, sem Lorem Ipsum em UI-de-produto, a11y aria/contrast.
- Referenciar de `commands/mustard/templates/agent-prompt/SKILL.md` quando `role=ui`.
- **Token impacto:** +0.3K por dispatch FE; -2K em rework de review.
- **Effort:** S.
- **Course value:** **Alto** — pilar 3 slogan direto.

#### G-1b: Browser-debug playbook (Playwright + Chrome DevTools MCP)
- Criar `templates/refs/bugfix/browser-debug.md` (~40 linhas): fluxo reproduce → isolate → instrument → fix → prevent. **Mapeia para AMBOS** Playwright MCP (interação via accessibility tree) **e** Chrome DevTools MCP (debugging/perf/network) — alinhado com canon web 2026.
- Referenciar de `commands/mustard/bugfix/SKILL.md` quando `role=ui`.
- **Effort:** S (~2h).
- **Course value:** **Alto** — pilar 3 slogan "IA + Browser".

#### G-1c: Component Contract na spec UI
- Sub-template em `refs/feature/spec-language.md` (PT + EN): seção `## Component Contract (UI)` com props/estados/variants/breakpoints/a11y/DS tokens.
- Anexar condicionalmente em PLAN quando ANALYZE detecta criação/refatoração de componente.
- **Effort:** S.
- **Course value:** **Alto** — pilar 3 §03.1.

#### G-1d: PR template `.github/`
- `templates/.github/pull_request_template.md`: Contexto / O que mudou / Por que mudou / Como testar / Riscos e rollback / Como revisar / Evidências.
- Copy rule em `init.ts` quando GH remote detected.
- **Token impacto:** zero runtime; benefício humano em PR review.
- **Effort:** S.
- **Course value:** **Médio** — pilar 1 §01.5.

#### G-1e: Mapping table RPI/SDD ↔ Mustard
- Tabela em `CLAUDE.md` raiz mapeando ANALYZE↔Research, PLAN↔Spec+Plan, EXECUTE↔Implement. Cita `github/spec-kit` como referência canônica.
- **Effort:** XS (15 min).
- **Course value:** **Crítico** — diferencia produto e ensina aluno a traduzir entre Mustard/Spec Kit.

#### G-1f: Vibe mode docs
- Linha em `templates/CLAUDE.md` §Intent Routing: "Spike/protótipo → `/mustard:task`. Sem spec, sem hygiene gates."
- **Effort:** XS.

### Tier 2 — Métricas DORA + glossário (M effort)

#### G-2a: DORA events em `/mustard:git` + `/mustard:review`
- Emit `pr.opened`, `pr.merged`, `review.start`, `review.complete` via `_lib/metrics-emit.js`.
- Nova view `buildPRMetrics(events)` em `harness-views.js`.
- Seção em `commands/mustard/metrics/SKILL.md`.
- **Effort:** M.

#### G-2b: Glossário via `description?` no entity-registry
- Schema do `entity-registry.json` ganha campo `description?: string`. `/scan` extrai de docstrings/JSDoc/XML doc se existir.
- View `/mustard:knowledge glossary`.
- **Effort:** M.

### Tier 3 — Refinamentos opcionais

#### G-3a: Extrair `_lib/size-gate.js` compartilhado
- `spec-size-gate` + `skill-size-gate` extraem heurística comum.
- **Effort:** S.

#### G-3b: Decisão sobre `.core.md` (G-0a já cobre)
- Subtrair (recomendado, alinhado com `feedback_analysis_pattern.md`) — converter `qa.core.md` em skill ou exceção documentada.

---

## 7. Roadmap fase-a-fase para entrega de curso

### Fase 1 — Higiene (precede qualquer demo de curso)
**Duração:** 1 dia.
**Itens:** G-0a (doc sync) + G-0b (Dumb Zone 40%) + G-0c (memory writer wired) + G-0d (recipe seed mínimo).
**Gate de aceitação:**
- `grep -r "20 hooks\|6 skills\|mcp-budget\|regression-guard" templates/ CLAUDE.md` retorna 0
- `node templates/hooks/__tests__/integration.test.js` passa com testes novos de % janela
- após pipeline /feature de exemplo, `.claude/memory/decisions.json` tem ≥1 entry
- `/mustard:feature add-field user.email` matches recipe `add-field.json`

### Fase 2 — Diferenciadores de curso (1 dia)
**Itens:** G-1a (anti-AI-look) + G-1b (browser-debug Playwright+DevTools) + G-1c (Component Contract) + G-1d (PR template) + G-1e (RPI/SDD mapping) + G-1f (Vibe docs).
**Gate de aceitação:**
- demo `/mustard:feature add Button component` em projeto-teste UI gera spec com `## Component Contract`
- demo `/mustard:bugfix UI flicker` faz agente FE receber browser-debug.md
- `node bin/mustard.js init` em projeto com remote GH instala `.github/pull_request_template.md`

### Fase 3 — Métricas DORA (½ dia, opcional)
**Itens:** G-2a.
**Quando:** se time usa GitHub PRs.

### Fase 4 — Glossário (½ dia, opcional)
**Itens:** G-2b.
**Quando:** projetos-alvo com domínio rico (entidades não-óbvias).

### Fase 5 — Refinamentos (½ dia, opcional)
**Itens:** G-3a.

**Total para curso entregável:** 2-3 dias de trabalho (Fases 1+2). Fases 3-5 são post-launch.

---

## 8. Risco-residual e mitigação

| Risco | Probabilidade | Mitigação |
|---|---|---|
| Aluno descobre Spec Kit e prefere | Alta | G-1e (mapping table) + diferenciadores listados em §3.2 |
| Vaporware quebra trust no curso | Alta sem Fase 1 | Fase 1 não-opcional |
| Dumb Zone fix introduz regressões | Baixa | tests novos em `integration.test.js` + fallback se window não detectado |
| Memory writer hook desbalanceia performance | Baixa | throttle (3/h, idempotência 24h igual a `session-knowledge-inc`) |
| Recipes seed fica datado | Média | doc explícita "recipes são inicializadores; users devem extender" |

**O que NÃO fazer (engajamento honesto):**

- **Reduzir 31→15 hooks** (proposto pela análise paralela): destruiria funcionalidade. Verificação por leitura mostra que overlap é real só entre 2 hooks (size-gates).
- **Recriar 6 .core.md**: skills + commands + CLAUDE.md já cumprem o papel. Recriação duplica.
- **Vibe/Assistido como flag dedicada**: `/mustard:task` já é vibe; adicionar 3ª taxonomia paralela ao Light/Full = fricção.
- **Hook anti-AI-look bloqueante**: heurísticas de UI dão falso-positivo (ver memory `feedback_no_permission_loops.md`).
- **Pipeline FE com fase "protótipo" + "observabilidade"**: protótipo é parte natural de EXECUTE; observabilidade depende da infra do projeto-alvo.
- **Múltiplos modelos via routing agressivo**: `model-routing-gate.js` já bloqueia upgrades; downgrades são opt-in. Mexer aqui contraria `feedback_no_routing_downgrade.md`.

---

## 9. Decisões pendentes do usuário

A análise produziu **8 gaps reais** ranqueados (G-0a..d, G-1a..f, G-2a-b, G-3a). Decisões necessárias:

1. **Fase 1 (higiene) é não-opcional para curso?** Recomendo **sim**.
2. **Memory writer (G-0c):** wirar via hook (recomendado) ou reforçar instrução em SKILL.md?
3. **Recipe seed (G-0d):** investir 6-10h para 5 recipes ou marcar opcional?
4. **`.core.md`:** subtrair (recomendado) ou recriar 6 arquivos?
5. **Fase 2 inteira ou subset?** Subset cirúrgico é G-1a+b+c (FE craft).
6. **Fase 3 (DORA):** time usa GitHub PRs ativamente?

---

## 10. Sources (pesquisa web)

- [Liu et al. (2023) — Lost in the Middle: How Language Models Use Long Contexts (arXiv:2307.03172)](https://arxiv.org/abs/2307.03172)
- [TACL 2024 publication of Lost in the Middle](https://aclanthology.org/2024.tacl-1.9/)
- [GitHub Spec Kit — Toolkit for Spec-Driven Development](https://github.com/github/spec-kit)
- [GitHub Blog — Spec-driven development with AI: Get started with a new open source toolkit](https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/)
- [Microsoft for Developers — Diving Into Spec-Driven Development With GitHub Spec Kit](https://developer.microsoft.com/blog/spec-driven-development-spec-kit)
- [Martin Fowler — Understanding Spec-Driven Development: Kiro, spec-kit, and Tessl](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html)
- [DEV Community — Escaping the Dumbzone, Part 1 (Dale Diggs)](https://dev.to/diggidydale/escaping-the-dumbzone-part-1-why-your-ai-gets-stupider-the-more-you-talk-to-it-4d8k)
- [Syntackle — A Million Token Context Window Isn't What You Think It Is](https://syntackle.com/blog/long-context-window-ai-model-catch/)
- [Mikko Ohtamaa on X — "At around the 40% context mark, LLMs start entering the dumb zone"](https://x.com/moo9000/status/2014656102290403810)
- [Medium — Context Engineering: 5 Counter-Intuitive Lessons (Rajesh Godavarthi)](https://medium.com/@rajesh.godavarthi/context-engineering-what-ai-builders-know-that-you-dont-5-counter-intuitive-lessons-from-the-8435308183ca)
- [12 Factor Agents: Principles for AI That Actually Work](https://paddo.dev/blog/12-factor-agents/)
- [Webfuse — 5 Best MCP Servers for Browser Automation in 2026](https://www.webfuse.com/blog/the-top-5-best-mcp-servers-for-ai-agent-browser-automation)
- [ChromeDevTools/chrome-devtools-mcp — Chrome DevTools for coding agents](https://github.com/ChromeDevTools/chrome-devtools-mcp)
- [Steve Kinney — Playwright vs. Chrome DevTools MCP: Driving vs. Debugging](https://stevekinney.com/writing/driving-vs-debugging-the-browser)
- [MCP.Directory — Chrome DevTools vs Playwright Comparison](https://mcp.directory/compare/chrome-devtools-vs-playwright)

---

## Anexo: O que mudou desta para a iteração anterior

| Aspecto | 2ª análise | Esta (3ª, validada web) | Por quê mudou |
|---|---|---|---|
| Cobertura agregada | 60% | **65%** | Re-leitura de `feature/SKILL.md` (217 linhas) revelou Wave 7 + Wave 10 + Existence Gate + spec layout progressive — não estavam pesados antes |
| Memory writer status | "vaporware" | "**parcial — designed mas não enforced**" | `feature/SKILL.md:154` referencia `memory-write.js` (orchestrator-invoked, não hook) |
| Position vs Spec Kit | n/a | adicionada §3 com 9 diferenciadores verificados | Web search revelou Spec Kit como concorrente direto oficial |
| Dumb Zone fix | "trocar 50K por %" | "**trocar 50K por 40% canônico (Horthy)**" | Web confirma 40% é threshold consensual cunhado por Dex Horthy |
| MCP Frontend playbook | "Playwright" | "**Playwright + Chrome DevTools (complementares)**" | Web 2026 mostra ambos canônicos |
| Routing | "manter" | confirmado: upgrades blocked, downgrades allowed (opt-in) — alinhado com memory `feedback_no_routing_downgrade.md` | Leitura `model-routing-gate.js:32-33` |
| Recipe seed | "valor médio" | "**P0 para curso**" (G-0d) | Sem recipes, demo de economia de tokens falha |

A análise paralela ajudou a ver onde minha 1ª passada confiou em docs em vez de código. A web search ajudou a ver que o PRD usa termos canônicos (Dumb Zone, SDD, RPI) — o que **eleva a barra**: Mustard não pode entregar versão "fraca" desses termos. Esta versão é **anti-confiança-em-docs + pró-evidência-arquivo:linha + cross-reference-canônico**.
