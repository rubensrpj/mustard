---
id: spec.apertar-prosa-skills-quentes-medir
---

# apertar prosa dos SKILLs quentes + medir baseline de token + emissao via mecanismo depende da porta unica

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

A reauditoria mostrou que as Fases 1-4 deram limpeza de DISCO + clareza, mas **quase nada de token de RUNTIME** — o custo que pesa é a re-injeção, todo turno, do always-on (`CLAUDE.md`) + dos SKILLs quentes (`feature` ~19 KB). Esta spec corta token de verdade **sem ferir fidelidade**, por caminhos que NÃO viram link-que-a-IA-pula:

- **Fase B — medir o baseline:** token-count do always-on + dos SKILLs quentes + refs que cada um puxa; ranquear por (tokens × frequência de carga). Sem baseline, apertar é chute. (Precedente: já existe `apps/rt/src/commands/economy/economy_capture_baseline.rs`.)
- **Fase C — apertar a prosa:** reescrever os alvos do ranking dizendo o mesmo em MENOS palavras (cortar redundância e explicação que a IA já sabe), **sem remover nenhum passo/gatilho** — régua: linha de fluxo/emit/gate é INTOCÁVEL.
- **Emissão via mecanismo:** generalizar o padrão que o #3 inaugura (evento como side-effect determinístico) onde a prosa hoje narra "emita X" — encolhe a prosa porque o código garante o passo.

**DEPENDE do #2 (porta única) executar primeiro:** o #2 reescreve o `CLAUDE.md` (Intent Routing) e as descrições dos SKILLs; apertar esses arquivos ANTES seria refazer trabalho. Os alvos concretos da Fase C só se fixam pós-#2 — por isso esta spec fica em fila atrás do #2.

## Usuários/Stakeholders

- **Quem paga o custo de token por turno** (o usuário, em sessões longas).
- **IA orquestradora** — contexto mais enxuto, mesma fidelidade.

## Métrica de sucesso

- Redução **medida** de token de runtime nos alvos do ranking, com **todo passo/gatilho preservado** (diff de fidelidade por arquivo).
- `cargo test` verde; nenhum link novo substituindo ação; nenhum hook/gate novo.

## Não-Objetivos

- **Não** mover ação / passo crítico para link (fere fidelidade).
- **Não** adicionar hook/gate novo (endurece, burocratiza).
- **Não** mexer no que #2 (roteador/descrições) ou #4 (dashboard) já tratam — esta entra DEPOIS do #2.

## Critérios de Aceitação

- **AC-1** — Build verde
  Command: `cargo build`
- **AC-2** — Testes verdes
  Command: `cargo test`
- **AC-3** — Baseline de token documentado (saída da Fase B)
  Command: `test -f docs/TOKEN-BASELINE.md`

> Fidelidade-preservada + token-reduzido são verificados por **diff por arquivo** no EXECUTE (cada passo/gatilho confirmado presente) — não por um único comando.

## Checklist

- [ ] T1 — Fase B: medir token-count do always-on + SKILLs quentes + refs; ranquear por (tokens × frequência); gravar `docs/TOKEN-BASELINE.md`.
- [ ] T2 — (pós-#2) Fase C: apertar a prosa dos alvos do topo do ranking, com diff de fidelidade (todo passo/gatilho preservado).
- [ ] T3 — Generalizar emissão-via-mecanismo onde a prosa narra "emita X" (padrão do #3).
- [ ] T4 — Re-medir e registrar a redução vs baseline.