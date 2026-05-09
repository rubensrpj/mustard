# PRD — Fluxo de pesquisa e adoção (IA + refatoração + FE)

## Contexto e objetivo

Este PRD descreve um **fluxo de pesquisa + operacionalização** para adoção de IA em:

- refatoração de legado em escala,
- onboarding rápido de IA em codebases complexas,
- desenvolvimento de frontend com alta qualidade (sem aparência de “código gerado”).

O documento foi estruturado para você conseguir **cruzar** com o seu sistema atual de desenvolvimento (processo, templates, gates de qualidade, ferramentas) e também para servir como **input para outro agente/sistema** analisar o projeto (com fontes e critérios claros).

## Problema

Times e indivíduos que usam IA em engenharia normalmente enfrentam:

- degradação de qualidade ao colocar “código demais” no prompt (contexto longo e não estruturado);
- refatorações arriscadas sem especificação de comportamento (regressões);
- PRs difíceis de revisar (mudanças grandes, sem racional, sem plano/testes);
- frontend inconsistente com design system e com aparência de “boilerplate gerado”;
- falta de um sistema para decidir quando usar prototipação rápida vs. engenharia robusta.

## Metas (outcomes)

1. **Refatorar com segurança** (menos regressão, mais previsibilidade).
2. **Onboard IA rapidamente** em projetos grandes com documentação mínima e escalável.
3. **Aumentar throughput** sem perder qualidade (reviews mais rápidos, PRs menores).
4. **Manter consistência de FE** (arquitetura, padrões e design system).

## Não-metas

- Definir uma stack específica (frameworks/linguagens) — o PRD é agnóstico.
- Substituir revisão humana/QA — a IA complementa, não assume responsabilidade.

## Personas / usuários

- Devs (FE/BE) atuando em legado e/ou features novas.
- Reviewer(s) responsáveis por qualidade/segurança.
- Tech lead/Staff definindo padrões do time.
- “Sistema/Agente” que vai ingerir este PRD para comparar com seu processo atual.

## Escopo

O escopo está organizado em 3 trilhas, espelhando os tópicos do curso no print:

1. **Refatorar legado com confiança**
2. **Onboard IA em codebases complexas**
3. **Construir features complexas de frontend sem parecer ‘código gerado’**

Cada trilha tem:

- Hipóteses
- Plano (Research → Plan → Implement)
- Critérios de suficiência de contexto
- Entregáveis (templates/playbooks)
- Métricas

---

# Trilha 01 — Refatorar 1.000+ linhas de código legado com confiança

## 01.1 Hipótese: “Dumb Zone” e degradação com contexto longo

**Hipótese:** prompts longos e pouco estruturados degradam qualidade por “perda de atenção” (especialmente no meio do contexto), levando a refatorações inconsistentes.

### Requisitos / abordagem

- Contexto deve ser fornecido em **camadas** e **compactado** (resumos + índices) em vez de dump.
- Sempre manter explícitos: objetivo, invariantes, restrições, interfaces públicas, casos críticos.

### Entregável

- Guia “Contexto para Refatoração” (limites práticos + quando resumir + templates).

### Validação

- Exercício A/B/C (curto vs médio vs longo) para o mesmo trecho + medir consistência.

## 01.2 Spec Driven Development (SDD) + Research-Plan-Implement (RPI)

**Objetivo:** reduzir risco de regressão ao criar “contrato do comportamento” antes de mudar.

### Requisitos

- Definir comportamento atual com:
    - characterization tests ("golden master") quando aplicável,
    - invariantes explícitas,
    - casos de borda.
- Todo trabalho relevante deve seguir **RPI**:
    
    1) Research: entender e mapear riscos
    
    2) Plan: sequenciar passos e rollback
    
    3) Implement: executar com gates
    

### Entregáveis

- Template de Spec (SDD)
- Template RPI
- Exemplo preenchido em um trecho real do seu repo

## 01.3 Compactação intencional + gerenciamento sistemático de contexto

**Objetivo:** reduzir custo cognitivo e ruído, mantendo a IA “alinhada”.

### Artefatos de contexto (mínimos)

- Project Context (L0/L1)
- Module Briefs (L2)
- Open Questions
- Assumptions
- Decision Log

### Entregável

- “Context Pack” padrão do time (estrutura + checklist).

## 01.4 Vibe Coding vs Desenvolvimento Assistido por IA

**Objetivo:** decidir quando usar velocidade vs rigor.

### Política proposta

- **Vibe (exploratório):** spikes, protótipos, rascunhos, experimentos de UI.
- **Assistido (robusto):** refatoração, migração, pagamentos, auth, segurança, produção.

### Entregável

- Matriz de decisão (risco × impacto × reversibilidade).

## 01.5 Fluxo de desenvolvimento para code reviews otimizados

**Objetivo:** PRs menores, mais fáceis de revisar e com racional explícito.

### Requisitos

- PR template com:
    - objetivo, contexto, abordagem,
    - riscos/rollback,
    - evidências (testes, screenshots, métricas),
    - “como revisar”.

### Métricas

- Tempo médio de review
- Tamanho médio de PR
- Bugs pós-merge

---

# Trilha 02 — Onboard a IA em qualquer codebase complexo em minutos

## 02.1 Documentação efetiva (contexto inicial)

**Requisitos mínimos:**

- como rodar, como testar, como debugar
- arquitetura macro + boundaries
- glossário do domínio

**Entregável:** checklist de onboarding + template “Project Overview”.

## 02.2 Progressive disclosure (contexto em camadas)

**Estrutura de camadas:**

- L0: objetivo do produto + restrições
- L1: arquitetura (componentes)
- L2: módulos (responsabilidades + APIs)
- L3: arquivos/funções sob demanda

**Entregável:** índice navegável do projeto.

## 02.3 Checklist prático de suficiência de contexto

**Teste de suficiência (prompts de verificação):**

- explique o fluxo E2E
- proponha plano em etapas
- liste riscos e pontos cegos
- proponha testes

**Entregável:** checklist + perguntas padrão de gap analysis.

## 02.4 Skills/rules vs sub-agents

**Política proposta:**

- Use **rules/skills** para rotinas repetíveis (checklists, templates, padrões).
- Use **sub-agents** para tarefas:
    - paralelizáveis,
    - com especialização (segurança, FE, QA),
    - com investigação e síntese.

**Entregável:** guia de decisão + exemplos de “contrato” de agente.

## 02.5 Mental alignment para times AI-assisted

**Requisitos:**

- rastreabilidade: “por que mudou”
- responsabilidade humana
- padrões de segurança/compliance

**Entregável:** política de review AI-assisted + checklist de aprovação.

---

# Trilha 03 — Features complexas de frontend sem parecer “código gerado”

## 03.1 Context engineering para componentes

**Requisitos de spec do componente:**

- props + estados + variantes
- responsividade
- a11y
- performance
- integração com design system/tokens

**Entregável:** template de spec + exemplo.

## 03.2 IA e o Browser

**Requisitos:**

- fluxo padrão: reproduzir → isolar → instrumentar → corrigir → prevenir
- testes e2e e/ou regressão visual quando necessário

**Entregável:** playbook de debugging FE assistido por IA.

## 03.3 Evitar estética “AI-generated”

**Heurísticas:**

- consistência com DS, nomes e padrões do repo
- edge cases e estados vazios
- microinterações e acessibilidade

**Entregável:** checklist “anti-AI-look”.

## 03.4 Fluxo de desenvolvimento Frontend

**Pipeline recomendado:**

Spec → protótipo → implementação → testes → observabilidade → release

**Entregável:** SOP do time para FE (gates e responsabilidades).

## 03.5 “Melhores MCPs para Frontend” (conectores/ferramentas)

**Critérios de escolha:**

- segurança/permissões
- utilidade real (fluxos ponta a ponta)
- custo/fricção

**Entregável:** tabela comparativa + recomendação por caso de uso.

---

# Requisitos funcionais (para cruzar com seu sistema)

## RF1 — Modo de trabalho (Vibe vs Assistido)

- O processo deve permitir marcar uma tarefa como **Vibe** ou **Assistido**.
- Tarefas “Assistido” exigem: spec, plano, testes mínimos e PR template completo.

## RF2 — RPI como pipeline padrão

- Toda iniciativa relevante passa por **Research → Plan → Implement**.
- Cada fase gera um artefato (mesmo curto):
    - Research notes (riscos, opções)
    - Plan (etapas, rollback)
    - Implement (diff + evidências)

## RF3 — Context Pack

- Deve existir um “pacote de contexto” versionável por projeto:
    - Project Context
    - Module Briefs
    - Decision Log
    - Open Questions

## RF4 — Review otimizado

- PR template obrigatório com “como revisar”, riscos e evidências.

---

# Requisitos não-funcionais (qualidade)

- Confiabilidade: reduzir regressões em refatoração.
- Manutenibilidade: código alinhado à arquitetura e padrões.
- Observabilidade: capacidade de detectar impacto pós-release.
- Segurança: não vazar segredos; respeitar controles e permissões.

---

# Métricas (sugestão)

- Lead time de PR (abertura → merge)
- Tempo de review
- Tamanho de PR (linhas alteradas) e número de commits
- Defeitos pós-merge (bugs/rollback)
- % de tarefas com spec/plano (Assistido)

---

# Riscos e mitigação

- **R1:** Overhead de processo → mitigação: aplicar gates só em “Assistido”.
- **R2:** Context pack desatualizado → mitigação: dono por módulo + checklist em PR.
- **R3:** Dependência excessiva de IA → mitigação: checklists e responsabilidade humana.

---

# Fontes e referências (para o seu sistema/agente investigar)

Abaixo estão fontes públicas recomendadas para embasar os tópicos (você pode anexar mais fontes internas do seu repo/processo):

## Context length / degradação / “lost in the middle”

- Liu, Nelson F. et al. (2023). *Lost in the Middle: How Language Models Use Long Contexts*. arXiv:2307.03172. [https://arxiv.org/abs/2307.03172](https://arxiv.org/abs/2307.03172)

## Refatoração e preservação de comportamento

- Martin Fowler — *Refactoring (book / catalog)*: [https://refactoring.com/](https://refactoring.com/)
- Michael Feathers — *Working Effectively with Legacy Code*: [https://www.oreilly.com/library/view/working-effectively-with/0131177052/](https://www.oreilly.com/library/view/working-effectively-with/0131177052/)
- Golden Master / characterization tests (conceito): [https://martinfowler.com/bliki/CharacterizationTest.html](https://martinfowler.com/bliki/CharacterizationTest.html)

## Code review e PRs pequenos

- Google Engineering Practices — Code Review: [https://google.github.io/eng-practices/review/](https://google.github.io/eng-practices/review/)

## Frontend qualidade, métricas e testes

- [web.dev](http://web.dev) — Core Web Vitals (LCP/CLS/INP): [https://web.dev/vitals/](https://web.dev/vitals/)
- Playwright (e2e): [https://playwright.dev/](https://playwright.dev/)
- Cypress (e2e): [https://www.cypress.io/](https://www.cypress.io/)

## Acessibilidade

- WAI/W3C — WCAG Overview: [https://www.w3.org/WAI/standards-guidelines/wcag/](https://www.w3.org/WAI/standards-guidelines/wcag/)

---

# Anexos (templates)

## Template — SDD (Spec)

- Objetivo
- Não-objetivos
- Invariantes (o que não pode mudar)
- Casos principais
- Casos de borda
- Critérios de aceite
- Testes (mínimos)

## Template — RPI

- Research
    - hipóteses
    - opções
    - riscos
- Plan
    - etapas
    - migração/rollout
    - rollback
- Implement
    - mudanças efetuadas
    - evidências
    - follow-ups

## Template — PR

- Contexto
- O que mudou
- Por que mudou
- Como testar
- Riscos e rollback
- Como revisar
- Evidências (prints, métricas, logs)