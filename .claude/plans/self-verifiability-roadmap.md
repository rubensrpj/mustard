# Roadmap — Mustard verificável sobre si mesmo

> Epic de direção única, derivado da análise competitiva `claude-code-harness` → Mustard
> (2026-05-19). Substitui qualquer leitura das "perdas" como cinco problemas separados.

## Causa raiz

O placar competitivo mostrou o Mustard perdendo em três frentes — self-diagnóstico,
maturidade/medição e rastreabilidade de guardrails. **Não são três problemas: são um.**
O Mustard *se descreve* (tabelas no `CLAUDE.md`, contagens como "28 scripts", validadores
pontuais espalhados) em vez de *se medir*. O harness roda 17 testes de regressão para as
regras R01–R13, tem `benchmarks/`, mede latência de hook. O Mustard afirma que funciona.
É por isso que o harness *parece* mais confiável fazendo menos.

A direção fecha esse gap: tornar o Mustard capaz de **provar a própria saúde**.

## Movimentos → specs

Três movimentos, entregues como **duas specs** — os movimentos 2 e 3 mexem no mesmo
arquivo (`apps/rt/src/hooks/bash_guard.rs`) e o teste de regressão depende do ID existir;
separá-los seria um handoff inútil.

| Fase | Spec | Movimento(s) | Status |
|---|---|---|---|
| 1 | `2026-05-19-mustard-doctor` | Comando que prova a saúde da instalação | spec pronta (PLAN) |
| 2 | `guardrail-catalog` (a redigir) | IDs estáveis de guardrail + testes de regressão por regra | queued |

### Fase 1 — `mustard-doctor`

`mustard-rt run doctor [--residue]` + wrapper `/mustard:maint doctor`. Diagnóstico único,
read-only: wiring de hooks, resíduo, drift de instalação, saúde de estado. Reporta
OK/WARN/FAIL por categoria. Spec detalhada em `.claude/spec/active/2026-05-19-mustard-doctor/`.

**Critério de saída:** `doctor` reporta OK numa instalação saudável e FAIL/WARN num hook
removido ou referência morta; absorve a antiga ação `/maint audit`.

### Fase 2 — `guardrail-catalog`

Reframe honesto: o valor não são os IDs (isso é polimento) — é a **cobertura de regressão**
que fecha o gap de medição. Os IDs são o meio: não se escreve "teste de regressão para
BG05" sem BG05 existir.

- Adicionar `id: &'static str` ao struct `DangerRule` e às regras de redirect/commit-gate
  → `BG01`..`BG13`. Incluir o ID na mensagem de bloqueio e no evento de telemetria.
- Um teste de regressão por regra em `bash_guard.rs` — confirma que o bloqueio dispara e
  que a variante segura passa. Espelha os 17 testes R01–R13 do harness.
- Tabela-catálogo das regras em `pipeline-config.md`.
- **Rejeitado:** break-glass por TOML — o Mustard já tem modos por env var; segunda fonte
  de config seria regressão.
- Arquivos: `apps/rt/src/hooks/bash_guard.rs`, `apps/cli/templates/pipeline-config.md`.

**Critério de saída:** todo `DANGER_RULE` tem ID estável e um teste de regressão;
`cargo test -p mustard-rt` cobre cada guardrail.

A Fase 2 só é redigida em detalhe **após a Fase 1 validada em uso real** — premissa do
próprio PRD ("P2 só após P0 validado").

## Fora deste epic (perdas descartadas ou de outra campanha)

- **Review multi-perspectiva** — descartada. Zero fricção registrada com o review atual;
  copiar `a11y` viola o agnosticismo; 4 agentes paralelos queimam tokens.
- **Superfície 18→5 comandos** — perda real, conserto subtrativo (na filosofia), mas é
  campanha própria, ortogonal à verificabilidade. Tratar separada, depois.
- **Distribuição / onboarding** — coberta pela spec `b6-dashboard-projects` (já em
  `active/`): o `mustard-dashboard` passa a ser o instalador — o usuário baixa o dashboard,
  seleciona o diretório, instala o Mustard por dentro dele e só então registra os repos /
  monorepos. O modelo atual de "mapear um workspace e carregar tudo" é descontinuado. A b6
  precisa de um ajuste de enunciado para refletir isso explicitamente.
