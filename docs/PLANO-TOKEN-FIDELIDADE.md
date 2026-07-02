# Plano: ganhar qualidade + cortar token de runtime SEM ferir fidelidade

> Sucessor do `ENXUGAR-PAYLOAD-CLAUDE.md`. Aquele fez limpeza de disco + clareza; este ataca o que ficou: a **perda em aberto** (skill-creator) e o **token de runtime** (que mal se moveu) — pela única alavanca que não faz a IA pular processo.

## Princípio-régua (a lente, da doc oficial + reauditoria)

Os `.md` são o **programa** que a IA executa fielmente; ela pula prosa "under pressure / long session / ambiguity" (Steering Claude Code). Então toda mudança se classifica:

- 🔗 **REFERÊNCIA** (definição, tabela consultada reativamente) → pode virar link **um-nível-de-profundidade**.
- 📌 **AÇÃO / passo crítico** → fica **inline** ("Clear steps prevent Claude from skipping critical validation" — best-practices).
- 🔒 **NÃO-PODE-PULAR** → vira **hook/comando determinístico** ("an instruction is the wrong tool… a real guardrail must be deterministic" — Steering).

**Gate de TODA fase:** (1) zero perda de conteúdo; (2) todo must-do segue inline OU garantido por código; (3) **delta de token medido** (não estimado); (4) onde mexer em rt → `cargo test` verde + saída byte-estável.

Verdade que orienta a ordem: cortar `.md` em link **não** reduz token de runtime de um guia (skip-risk por ~0 ganho). Só duas coisas reduzem token sem ferir fidelidade: **apertar a prosa** (dizer o mesmo em menos palavras) e **mover mecanismo para o código** (aí a prosa encolhe porque o código garante o passo).

---

## Fase A — Fechar o loss-risk do skill-creator (segurança primeiro)

O único ponto onde a limpeza pode ter custado capacidade: `/skill create|optimize|eval` dependem do skill-creator, que removi afirmando "fetch on-demand" **não verificado**.

- **A1. Investigar o mecanismo real.** Grep rt+cli pelo handler de `/skill` `create`/`update`/`skill-fetch`; determinar se existe de fato um fetch on-demand de `skill-creator` (sparse-clone `anthropics/skills`) e onde ele instala.
- **A2. Decidir por evidência:**
  - fetch **funciona** → adicionar guard em `create`/`optimize`/`eval`: "se `skill-creator` ausente, rode `/skill update skill-creator` primeiro" + manter o corte.
  - fetch **não funciona** → escolher: **(a)** re-bundlar skill-creator (reverte o corte; aceita 252 KB de disco — mas é disco, não runtime), **(b)** implementar o fetch on-demand (mudança rt), ou **(c)** declarar create/optimize/eval removidos e deixar o SKILL honesto.
- **Gate A:** `/skill create` testado OU o SKILL é honesto sobre o que requer. Sem capacidade perdida em silêncio.

## Fase B — Medir o baseline real de token de runtime (antes de cortar às cegas)

"Apertar prosa" sem medir é chute. O custo que importa é o **re-injetado por turno**.

- **B1.** Token-count (aprox.) do **always-on**: `.claude/CLAUDE.md` raiz + um `{subproject}/CLAUDE.md` representativo (Guards). É o que volta todo turno.
- **B2.** Token-count dos **SKILLs quentes** (feature, task, bugfix) + a cadeia de refs que cada um puxa num run típico (um-nível).
- **B3.** Ranquear alvos por **(tokens × frequência-de-carga)**. Saída: lista priorizada de "onde 1 KB cortado rende mais".
- **Gate B:** baseline numérico em `docs/` (tabela). Define o alvo, não o achismo.

## Fase C — Apertar a prosa dos alvos quentes (qualidade, sem link, sem skip-risk)

Para cada alvo do ranking B3, reescrever **dizendo o mesmo em menos palavras** — cortar redundância e explicação que a IA já sabe (best-practices: *"Claude is already very smart; does this paragraph justify its token cost?"*), **sem remover nenhum passo/gatilho**.

- Candidatos óbvios: `feature/SKILL.md` (19 KB — o §1 tem parágrafos longos que repetem mecânica do digest), o always-on do CLAUDE.md.
- **Gate C (por arquivo):** diff de fidelidade — listar cada must-do/gatilho e confirmar que sobreviveu — **+** delta de token medido (B → depois). Aplica nos dois trees; commit por alvo.

## Fase D — min-IA/max-Rust (a alavanca durável: token **e** fidelidade)

A única forma de cortar prosa **sem** criar skip-risk: quando o **código garante** o passo, a prosa encolhe porque a IA deixa de ser quem o executa.

- **D1. Inventariar.** Para cada SKILL quente, listar os passos que a prosa NARRA mas são **determinísticos** (a IA hoje cumpre via prosa) — candidatos a virar comando `mustard-rt run …` (composite) ou hook (gate).
- **D2. Por candidato (maior prosa-economizada primeiro):** implementar o comando/hook (com testes byte-estáveis) → encolher a prosa para um gatilho de 1 linha + "o rt/hook garante isto".
- **D3. Gate D:** testes rt verdes; o passo agora é mecânico (hook bloqueia / comando faz); redução de prosa medida.
- **Sementes a confirmar no inventário:**
  - o gate "digest-validate obrigatório antes de Explore/implement num digest `strong` layer-incoerente" → **PreToolUse hook** (já levantado na reauditoria; fail-open; dispara uma vez). Vira 🔒 e a prosa do gatilho encolhe.
  - qualquer sequência "emita `pipeline.X` → rode `Y`" que um composite `mustard-rt run …` já poderia encadear (precedente: `close-orchestrate`, `plan-materialize`, `wave-advance`).

---

## Cross-cutting
- Princípio das 3 naturezas vira **checklist de edição** (evita reincidência do erro do `lexicon-feedback`: AÇÃO movida a link).
- Um-nível-de-profundidade: auditar a cadeia `SKILL → digest-validate → recall-index/locating-code` (2 níveis pré-existentes) — achatar ou aceitar conscientemente (best-practices: aninhado → leitura parcial).
- Cada fase = commits na branch; deploy nos alvos via `install.ps1` (`update --force` propaga SKILLs/refs; CLAUDE.md é preservado por-projeto).

## Ordem e por quê
1. **A** — segurança (fechar a única perda possível) antes de otimizar.
2. **B** — medir, senão C/D são chute.
3. **C** — aperto barato, ganho imediato de token, risco baixo.
4. **D** — a alavanca real e durável; mais cara (mexe em rt), mas é a que serve token **e** fidelidade ao mesmo tempo.
