---
id: wave.base-do-branch-escolhida-numa.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.base-do-branch-escolhida-numa.1-core]] | core | — | separa no modelo o que é ponto-de-corte do que é branch protegido |
| 2 | [[wave.base-do-branch-escolhida-numa.2-runtime]] | runtime | [[wave.base-do-branch-escolhida-numa.1-core]] | os portões param de recusar branch real, o tipo abre e nasce o comando que lista as candidatas |
| 3 | [[wave.base-do-branch-escolhida-numa.3-installer]] | installer | [[wave.base-do-branch-escolhida-numa.1-core]] | o init para de perguntar qual é a produção e qual é o desenvolvimento |
| 4 | [[wave.base-do-branch-escolhida-numa.4-docs]] | docs | [[wave.base-do-branch-escolhida-numa.2-runtime]], [[wave.base-do-branch-escolhida-numa.3-installer]] | a prosa ensina o seletor no lugar da pergunta de duas opções |

## Acceptance Criteria
- AC-6 — when uma instalação antiga com git.flow preenchido abre uma unidade a partir de um branch que o flow NÃO declara, then o portão de base aceita a abertura e a base declarada aparece apenas como pré-seleção Command: `cargo test -p mustard-rt --lib commands::event::base_gate::tests::a_declared_flow_preselects_without_refusing_others -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
- AC-1 — o portão de base aceita um branch real não declarado. Command: `cargo test -p mustard-rt --lib commands::event::base_gate::tests::accepts_any_real_branch_as_base -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
- AC-2 — só o branch padrão do remoto é protegido. Command: `cargo test -p mustard-rt --lib commands::event::work_branch::tests::only_the_remote_default_branch_is_protected -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
- AC-3 — um tipo fora da lista sugerida é aceito. Command: `cargo test -p mustard-rt --lib shared::work_kind::tests::accepts_a_type_outside_the_suggested_list -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
- AC-4 — run base-candidates devolve os branches reais. Command: `cargo run -p mustard-rt --quiet -- run base-candidates 2>&1 | grep -q '"branches"'`
- AC-5 — o init não pergunta branches e não grava git.flow. Command: `cargo test -p mustard-cli --lib commands::git_flow::tests::init_does_not_ask_for_branches -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
- AC-7 — o build do workspace passa verde. Command: `cargo build --workspace`
