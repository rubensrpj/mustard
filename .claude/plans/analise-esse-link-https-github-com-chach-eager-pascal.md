# PRD — Aproveitamento competitivo: claude-code-harness → Mustard

## Context

Análise comparativa do repositório [Chachamaru127/claude-code-harness](https://github.com/Chachamaru127/claude-code-harness)
(framework "Plan→Work→Review→Ship" para Claude Code, engine em Go) contra o Mustard.

O objetivo **não** é copiar features. É identificar onde o harness resolve um problema
que o Mustard hoje **não resolve** — e onde o Mustard já tem fricção documentada que uma
ideia do harness elimina. Princípio condutor (memória de projeto): *subtrair > adicionar*,
*resolver fricções antigas primeiro*, *uma direção principal — não cinco*, *tudo agnóstico*.

### Premissa Rust (decisiva)

O harness fez a jogada arquitetural certa: engine **Go nativo, binário único, zero Node.js**.
O Mustard está no **mesmo caminho** — `mustard-rt` (crate `apps/rt`) já é o binário Rust
que despacha todos os hooks de enforcement. Porém a migração **não está completa**: parte dos
`templates/scripts/*.js` ainda é JavaScript e o `pipeline-config.md` ainda descreve "hooks JS".

Consequências para este PRD — **toda proposta abaixo é Rust-first**:
- Nada de novo script `.js`. Todo subcomando novo é um módulo no crate `apps/rt`,
  exposto via `mustard-rt run <cmd>` e registrado em `apps/cli/src/cli.rs`.
- O próprio `doctor` (P0) ganha uma checagem extra: **completude da migração JS→Rust** —
  listar scripts/hooks ainda em `.js` que já têm equivalente Rust (resíduo de migração).
- Paridade competitiva: ao terminar a migração, Mustard iguala o harness em
  "binário único nativo, sem runtime externo" — e o supera em economia de tokens e SDD.

### Estado atual confirmado (exploração)

| Área | Mustard hoje | Evidência |
|---|---|---|
| Guardrails de Bash | Tabela declarativa `DANGER_RULES` (13 regras) com predicados — **sem IDs estáveis, sem catálogo doc** | `apps/rt/src/hooks/bash_guard.rs:101-167` |
| Self-diagnóstico | **Inexistente** — só validadores pontuais (`verify-pipeline`, `skill-validate`) | `templates/scripts/` |
| Scripts stale | `.claude/scripts/` (cópia instalada) diverge de `templates/scripts/` (fonte) | memória `feedback_mustard_self_scripts_stale` |
| Review de PR | **Passada única genérica** — checklist SOLID/Security/Perf/Patterns num só agente | `templates/commands/mustard/review/SKILL.md:75` |
| Crítico de plano | **Inexistente** — PLAN → /approve → EXECUTE, sem desafio adversarial | `feature/SKILL.md` |
| Changelog/release | **Inexistente** — `/close` arquiva spec; `/git` é ff-only sem release | `close/SKILL.md`, `git/SKILL.md` |

## O que o harness tem e o Mustard não (síntese)

1. **`harness doctor` / `doctor --residue`** — diagnóstico de saúde dos hooks + detecção de
   referências a código deletado (stale).
2. **Guardrails como catálogo numerado (R01–R13)** — IDs estáveis, auditáveis, com break-glass por TOML.
3. **Review em 4 perspectivas paralelas** (security, performance, quality, accessibility).
4. **Agente crítico** que desafia o plano antes de codar ("Breezing" Phase 0).
5. **`/harness-release`** — changelog + tag + GitHub Release automáticos.
6. (Descartados — fora do escopo/filosofia do Mustard: geração de slides/vídeo, delegação a
   OpenAI Codex, modo 2-agentes Cursor=PM. Engine Go → Mustard já migra para Rust, N/A.)

---

## Proposta — direção priorizada

### P0 — `mustard doctor` (direção PRINCIPAL)

**Problema que resolve:** o Mustard tem fricção *documentada* — `.claude/scripts/` instalado
fica stale vs `templates/scripts/`; hooks falham silenciosamente (memória
`feedback_mustard_performance`). Não há comando que diga "sua instalação está saudável".
Esta é a única proposta que ataca uma fricção **já registrada**, por isso é a direção principal.

**Escopo (Rust-first, agnóstico, sem novos hooks):**
- Novo módulo Rust `apps/rt/src/commands/doctor.rs`, exposto como `mustard-rt run doctor`
  + wrapper `/mustard:maint doctor`. **Zero JavaScript.**
- Checagens:
  1. **Wiring de hooks** — cada `mustard-rt on <event>` em `settings.json` resolve para um módulo existente.
  2. **Drift de instalação** — compara hash de `.claude/` instalado vs `templates/` (a fonte): lista arquivos stale. Resolve diretamente a fricção do "scripts stale".
  3. **Resíduo (`--residue`)** — referências a scripts/hooks/comandos que não existem mais (varre `settings.json`, SKILL.md, refs por nomes de arquivo mortos).
  4. **Migração JS→Rust** — lista `templates/scripts/*.js` / hooks JS que já têm equivalente em `mustard-rt` (resíduo de migração a remover).
  5. **Saúde de estado** — `.pipeline-states/` órfãos, specs `closed-followup` vencidas, registry ausente/desatualizado.
- Saída: relatório compacto `OK / WARN / FAIL` por categoria. **Fail-open, read-only, zero confirmações.**

**Arquivos:** `apps/rt/src/commands/doctor.rs` (novo); `apps/rt/src/cli.rs` ou
`apps/cli/src/cli.rs` (registrar subcomando); `templates/commands/mustard/maint/SKILL.md`
(adicionar ação `doctor`).

### P1 — IDs estáveis no catálogo de guardrails (custo baixo, alto valor)

**Problema:** `DANGER_RULES` já é declarativo, mas regras não têm ID — impossível referenciar
"por que fui bloqueado pela regra X" em telemetria, testes ou docs.

**Escopo:** adicionar campo `id: &'static str` (ex.: `"BG01".."BG13"`) ao struct `DangerRule`
e às regras de redirect/commit-gate. Incluir o ID na mensagem de bloqueio e no evento de
telemetria. Gerar/atualizar uma tabela-catálogo em `pipeline-config.md`. Sem mudança de
comportamento — só rastreabilidade. **Não** adotar o TOML break-glass do harness agora
(Mustard já tem env vars de modo; evitar segunda fonte de config).

**Arquivos:** `apps/rt/src/hooks/bash_guard.rs` (Rust puro — só mexe na tabela já
existente); `templates/pipeline-config.md`.

### P2 — itens com valor real, porém maior custo (decidir depois do P0)

- **Review multi-perspectiva agnóstico.** A versão do harness inclui "accessibility", que só
  faz sentido em UI — adotar verbatim violaria a regra "Mustard 100% agnóstico". Proposta
  agnóstica: `/review` deriva as lentes do `entity-registry.json` / `sync-detect` —
  *Security* e *Quality* sempre; *Performance* quando há camada de dados/API; *Accessibility*
  só quando há subprojeto de UI detectado. Lentes viram seções do prompt do agente único
  (não N agentes — preserva orçamento de tokens).
- **Crítico de plano.** O Mustard já recomenda o skill `grill-me` (interview adversarial de
  plano). Em vez de novo agente, tornar o passo `/approve` capaz de disparar uma auto-crítica
  via `grill-me` antes de liberar EXECUTE. Reusa o que já existe — não adiciona superfície.
- **Changelog no CLOSE.** `/close` já coleta `affectedFiles` + diff vs parent. Adicionar
  geração opcional de bloco de changelog (agrupado por tipo de commit) no banner de conclusão.
  Release/tag no GitHub fica fora — conflita com o design ff-only-sem-PR do `/git`.

---

## Não-objetivos

- Geração de slides/vídeo, delegação a Codex, modo Cursor 2-agentes — fora da filosofia do Mustard.
- Reescrever guardrails em formato TOML — duplicaria a config de modo já existente.
- Criar novos hooks de enforcement — a direção é diagnóstico (read-only), não mais bloqueio.
- Escrever qualquer lógica nova em JavaScript — tudo novo nasce em Rust (`apps/rt`).

## Verificação (end-to-end)

1. **P0** — `cargo build -p mustard-rt` então `mustard-rt run doctor` num repo com hook
   removido de `settings.json` e um script stale forçado → relatório deve listar ambos como
   `FAIL`/`WARN`. `mustard-rt run doctor --residue` num SKILL.md com referência a script morto
   → resíduo detectado. Rodar em repo limpo → tudo `OK`, exit 0.
2. **P1** — `cargo test -p mustard-rt` (bash_guard); disparar `rm -rf` e confirmar que a
   mensagem de bloqueio inclui o ID. Conferir a tabela em `pipeline-config.md`.
3. **P2** — após aprovação, escopo e ACs próprios por item.

## Sequência recomendada

Implementar **somente o P0** como primeira spec (`/mustard:feature mustard-doctor`).
P1 entra como spec curta em seguida. P2 só após P0 validado em uso real.
