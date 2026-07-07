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

- [ ] T1 — auditar `session_knowledge_observer`: em que evento dispara, que condição filtra, por que nunca escreveu no sialia (hipóteses: evento não emitido em projeto consumidor; filtro estreito demais; caminho de escrita).
- [ ] T2 — reproduzir num fixture: uma sessão simulada que DEVERIA capturar um padrão; provar o defeito ou provar que o desenho nunca dispara.
- [ ] T3 — veredito: consertar (com teste de regressão) OU remover o lado patterns inteiro (observador, seção "Project Knowledge" da injeção, prosa) — decisão registrada na spec.