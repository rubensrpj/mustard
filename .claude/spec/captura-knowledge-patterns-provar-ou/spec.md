---
id: spec.captura-knowledge-patterns-provar-ou
---

# captura de knowledge patterns: provar ou remover

<!-- drafter:tone=didactic — didactic tone; expand abbreviations on first use. -->

<!-- PRD -->

## Contexto

O mustard tem dois lados de memória automática: decisões (`.claude/memory/decisions/`) e padrões de conhecimento (`.claude/knowledge/`, escritos pelo `session_knowledge_observer`). A sonda de 2026-07-07 no sialia mediu: 11 decisões capturadas (boas — regras de reajuste/recorrência) e **zero** padrões de knowledge, mesmo após semanas de uso com pipelines reais. No repo do mustard os 21 padrões existentes datam de junho. Ou o observador tem um defeito/condição que nunca dispara em projeto consumidor, ou a peça não se paga. Pela régua do enxuto (mesma aplicada ao palpite por prompt, removido após medir 1 útil em 17): peça que não prova valor é consertada uma vez ou removida — nunca mantida no talvez.

## Usuários/Stakeholders

Sessões futuras (que herdariam padrões úteis na injeção de memória); quem mantém o mustard (menos código morto se a resposta for remover).

## Métrica de sucesso

Decisão binária tomada com evidência: (a) o observador captura padrões reais num projeto consumidor após o conserto, OU (b) o lado knowledge-patterns é removido (observador + injeção + docs), com a injeção de memória mantendo só o que prova valor.

## Não-Objetivos

Não tocar no lado decisions (provado); não redesenhar a memória; não criar curadoria manual de padrões.

## Critérios de Aceitação

- **AC-1** — Testes do observador cobrem o caminho de captura (ou a remoção completa, conforme o veredito)
  Command: `cargo test -p mustard-rt session_knowledge`
- **AC-2** — Suíte do rt permanece verde
  Command: `cargo test -p mustard-rt`
- **AC-3** — Lint limpo
  Command: `cargo clippy -p mustard-rt`

## Checklist

- [x] T1 — auditar `session_knowledge_observer`: **o observador NÃO é o escritor de knowledge-patterns**. Ele escreve friction telemetry + `retry.attempt`, e `memory-auto-extract` grava Decisions/Lessons em `.claude/memory/{decisions,lessons}/` (não em `knowledge/`). `extractPatternsFromStates` é vazio por design. A pasta `.claude/knowledge/` só recebia escrita do comando MANUAL `mustard-rt run memory knowledge` (`Kind::Principle`/`Scope::Global`), invocado por prosa no `/close` e `/knowledge add` — nunca por hook determinístico. Por isso fica vazia em consumidor (sialia: 0 padrões, confirmado por `ls`).
- [x] T2 — provado estaticamente: nenhum caminho automático escreve em `knowledge/`; o único construtor de `Kind::Principle/Global` era `run_knowledge`. Campo (2 btws sialia 07-12): os "ganhos de memória" atribuídos a knowledge eram, na verdade, comentário de código lido por Explore genérico — não injeção do mustard. A camada não pagou em nenhum lugar mensurável.
- [x] T3 — **VEREDITO: REMOVER.** Removidos: (a) a seção "Project Knowledge" da injeção do SessionStart (`load_knowledge_md`, `KB_MAX_ENTRIES`, o bloco em `build_memory_context`) — parava de injetar padrões "(unverified)" e código morto em todo consumidor; (b) o escritor `run_knowledge` + subcomando `memory knowledge` + linhas "pattern" do `run_list`; (c) prosa em `/close`, `/knowledge`, `evolve-report`, `pipeline-config`. **Mantido** (lado provado): Decisions/Lessons via `memory decision` + `memory-auto-extract`, e o `KnowledgeStore` genérico. Redundância adicional: os moldes `{role}-pattern` autorados pelo `/scan` já cobrem o papel de "como se escreve X aqui" e SIM disparam em consumidor.