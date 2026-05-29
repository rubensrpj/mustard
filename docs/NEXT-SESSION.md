# Prompt para a próxima sessão (execução autônoma)

> Cole o bloco abaixo como a primeira mensagem de uma sessão nova no diretório
> `C:\Atiz\mustard`. Ele roda o backlog restante do refactor sozinho, em
> sequência, com gate verde a cada commit. Pode sair — volte com tudo pronto.

---

Continue a refatoração SOLID/dedup do workspace Rust mustard (packages/core,
apps/cli, apps/rt). Branch: `dev_rubens`. **Trabalhe de forma autônoma até o fim
do backlog**, sem me pedir confirmação — as decisões de design já estão tomadas e
gravadas. Faça commit a cada fase/família **somente com a árvore verde**.

## ANTES DE TUDO (leia, nesta ordem)
1. A memória do projeto, em especial `project-mustard-remaining.md` (backlog +
   convenção de hooks), `project-mustard-refactor.md` (fases feitas),
   `feedback-no-facade-consolidation.md` e `feedback-engineer-decide-verify.md`
   (regras). Elas são auto-carregadas via MEMORY.md.
2. `REFACTOR-AUDIT.md` na raiz (catálogo). Note que os itens tardios da auditoria
   (A7/B3/B4/B6) **superestimam** a duplicação — veja as notas de "deferido".

## REGRAS INEGOCIÁVEIS
- **Sem facade / sem wrapper delegador.** Deletar a duplicata e fazer cada
  call-site chamar o dono canônico direto.
- **Mapear por COMPORTAMENTO, não por nome.** Varrer exaustivamente os 3 crates.
  Provar com varredura final que zerou. Ser criterioso: **se for variante e não
  cópia, NÃO force** (documente por que deixou).
- **Imports módulo-qualificados:** `use crate::util::json_io;` → `json_io::read_json(...)`.
  NUNCA importar o nome puro da função. Tipos podem ser por nome.
- **Engenheiro decide + verifica empiricamente.** Sem abstração por hipótese.
- **GATE verde antes de cada commit:** `rtk cargo build -p mustard-core` +
  `rtk cargo build -p mustard-rt --bin mustard-rt` (o BIN é o sinal real de
  dead-code; o `#![allow(dead_code,…)]` em rt/lib.rs é intencional) +
  `rtk cargo test`. **1 commit por fase/família.** Use `rtk` em todos os comandos.
- **3 falhas são AMBIENTAIS, não regressões** (faltam fixtures `.claude/spec`):
  `gate_regression_check::wave_7_review_w6_fixture`, `touched_functions::ac_a_15…`,
  `spec_invariants`; e `io::events::reader::bench_stream_10k` floca sob carga
  (passa isolado). Confirme que qualquer falha é uma dessas antes de seguir.
- Mensagem de commit termina com:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
- Atualize a memória ao concluir cada fase.

## MULTI-AGENTE
- Use **agentes Explore em paralelo** para MAPEAR cada família/fase antes de
  editar (ex.: levantar file→struct→id→role de cada família de hook a partir de
  `apps/rt/src/registry.rs`; varrer call-sites). Mapeamento é paralelizável.
- **A EXECUÇÃO é sequencial**, não paralela: os renames/dedups compartilham
  arquivos (`registry.rs`, `status.rs`, `*/mod.rs`, `util/mod.rs`) e o Rust exige
  a árvore compilando entre commits. Worktrees paralelos colidiriam nesses
  arquivos. Então: mapeie em paralelo, edite + builde + commite em série.

## BACKLOG (execute em ordem)

### FASE 1 — Rename de TODOS os hooks (convenção `<assunto>_<papel>`)
O user autorizou mexer em id/env (ninguém usa mustard ainda). Papel = contrato:
`_gate` (Check que dá `Deny`), `_observer` (Observer), `_inject` (verdict Inject),
`_counter`/`_budget` (limites). Cada rename toca TODAS as identidades: arquivo
(`git mv`), struct, id de wire (`Registry.id` + listas em registry.rs ~511/648/772),
env `MUSTARD_<ID>_MODE` (status.rs), descrição (status.rs), `mod.rs`, doc-links,
testes. **Já feito:** `enforce_entity_registry → entity_registry_gate`.

**ARQUIVO ≠ STRUCT ≠ ID (muitos-pra-um).** `task/tracker.rs` registra 5 hooks
(`ToolUseCounter`, `MainContextCounter`, `SubagentTracker`, `MetricsTracker`,
`SkillUsageTracker`) → explodir em 5 arquivos. ~27 ids no total.

Para CADA família: (1) Explore mapeia file→struct→id→role do `registry.rs`;
(2) aplique a convenção; (3) `git mv` + renomeie struct + id + env + registry +
status + mod.rs + testes; (4) build BIN + test; (5) **1 commit verde por família**.
Ordem: **task → session → observe → bash → write restante** (path_guard→path_gate,
pre_edit_intent_check→pre_edit_intent_gate; size_gate/close_gate/post_edit já ok).
Nomes propostos (ajuste ao papel real): tracker→`*_counter`/`*_tracker_observer`,
`budget`→`token_budget_gate`, `stop`→`subagent_stop_gate`, `stop_observer`→
`subagent_stop_observer`, session `knowledge`→`session_knowledge_observer`,
`bash_guard`→`bash_command_gate`. Varredura final: nenhum id/struct/arquivo antigo
fora de `REFACTOR-AUDIT.md`.

### FASE 2 — is_kebab duplicado (decisão pequena)
`skills.rs::is_kebab` (`s.len()>=2` bytes) vs `core::skill::frontmatter::is_kebab`
privado (`chars().count()>=2`). Expor o do core e fazer skills.rs usá-lo (o do
core é mais correto p/ multi-byte); atualizar testes se mudar borda. 1 commit.

### FASE 3 — B4/B6: REAVALIAR e provavelmente PULAR
Varra para confirmar que são variantes (não cópias idênticas). B4: parsers de
seção diferentes (wave_lib bulleted vs wave_files fence-aware vs
dependency_precheck próprio vs wave_tree tabela). B6: stack/signals. **Só
consolide o que for cópia idêntica comprovada;** documente o que deixou e por quê.
Não force (evita drift). Provavelmente nenhum commit, ou 1 pequeno.

### FASE 4 — C: migrações de bypass (mecânico, grande; faça por categoria)
Cada categoria = vários commits incrementais verdes (não um big-bang):
- **C2** joins inline `.claude`/spec → `ClaudePaths`/`SpecPaths` (~195 sites).
- **C1** `std::fs::` → `mustard_core::io::fs` (~661 sites; concentrados em hooks+scan).
- **C3** `Command::new` solto → `process::rtk_command` ou `util::platform`
  (~83 sites).
- **C4** remover `#![allow(dead_code, unused_imports, unused_variables, unused_mut)]`
  do `apps/rt/src/lib.rs` e limpar tudo que aparecer (use o build do BIN).
Faça C2→C1→C3→C4. Commit por lote coeso e verde. Se um site for legítimo (ex.:
`std::fs::remove_dir_all` sem equivalente no core), deixe e comente.

## AO TERMINAR
Atualize `project-mustard-remaining.md` e `project-mustard-refactor.md` com o que
foi feito, deixe a árvore verde, e me dê um resumo dos commits por fase.
