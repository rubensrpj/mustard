---
id: spec.qa-run-com-build-quente
---

# qa-run com build quente e timeout configurável por critério

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O runner de QA (`qa-run`) executa cada Critério de Aceitação (AC) com timeout fixo de 10 minutos. Quando um AC é uma suíte de testes (`cargo test -p mustard-rt`), a primeira execução paga a compilação inteira num target frio e estoura o limite: em 2026-07-07, o AC-3 da spec `templates-md-enxutos-separar-lei` foi marcado `skip` por timeout (600.009 ms) sendo que o mesmo comando, rodado direto com o target quente, passa em 39 segundos. É resíduo já observado antes ("verify >600s em build frio"). O sintoma engana: um `skip` por timeout parece problema do AC, quando é problema de aquecimento do runner — e vai atingir toda spec cujo AC envolva compilação.

## Usuários/Stakeholders

Toda spec com AC de build/teste; o orquestrador (deixa de interpretar `skip` falso); o close-gate (o `qa.result` fica fiel).

## Métrica de sucesso

Uma spec cujo AC roda a suíte completa passa no `qa-run` em target frio, sem `skip` por timeout.

## Não-Objetivos

Não mudar a semântica de `pass`/`fail`/`skip`; não paralelizar ACs; não tocar no close-gate.

## Critérios de Aceitação

- **AC-1** — Testes do qa-run cobrem o aquecimento e o timeout configurável
  Command: `cargo test -p mustard-rt qa_run`
- **AC-2** — Suíte do rt permanece verde
  Command: `cargo test -p mustard-rt`
- **AC-3** — Lint limpo no crate
  Command: `cargo clippy -p mustard-rt`

## Checklist

- [ ] T1 — aquecer o target antes dos ACs: `verify-pipeline`/`qa-run` roda o build declarado do projeto (ex.: `cargo build`) UMA vez antes de iterar os critérios, fora do timeout por AC.
- [ ] T2 — timeout por critério configurável (campo opcional no AC ou env `MUSTARD_QA_TIMEOUT_MS`), mantendo 600s como padrão.
- [ ] T3 — `skip` por timeout carrega a causa no payload (dica de build frio quando o primeiro AC de build estourar).