---
name: add-hook-rule
description: Use quando for preciso acrescentar uma regra a um gancho feito de regras, como uma conferência nova no fim da resposta (o `end_of_turn_check`), sem criar um gancho novo.
---

# Acrescentar uma regra a um gancho feito de regras

Um gancho feito de regras roda uma lista de regras e junta o que elas acham num veredito só. Hoje o molde é o `end_of_turn_check` (o `Stop`), com o tipo `TurnRule`. Os próximos ganchos feitos de regras (o portão de escrita, com `WriteRule`) seguem o mesmo desenho: o tipo da regra mora no módulo do assunto, a lista `RULES` mora no gancho, e `run_rules` roda uma regra sozinha nos testes.

## Passos

1. **A regra, no módulo do assunto** (`apps/rt/src/hooks/task/<assunto>.rs`). Crie `pub struct <Assunto>Rule;` e implemente `TurnRule` (definido em `apps/rt/src/hooks/task/end_of_turn_check.rs`), que tem um método só:
   - `check(&self, turn: &Turn<'_>) -> Option<Finding>` devolve `None` quando não há o que dizer, `Finding::Block(texto)` para barrar (o texto vai ao assistente, que reescreve) e `Finding::Warn(texto)` para só avisar o usuário.
   - O `Turn` já traz `message` (o texto final), `project_dir`, `session`, `retry` (o `stop_hook_active`) e `lang` (o idioma das mensagens). Não releia o `HookInput`.
2. **O texto, no catálogo** (`packages/core/src/platform/i18n.rs`). Uma chave nova com o texto exato em pt-BR e en-US, e as vagas `{x}` que o chamador troca. Monte o texto com `mustard_core::translate(chave, turn.lang)`.
3. **O registro, na lista** (`apps/rt/src/hooks/task/end_of_turn_check.rs`). Acrescente `&<Assunto>Rule` em `RULES`, na posição em que o texto dela deve aparecer no bloqueio. O `registry.rs` e o `hooks.json` não mudam: o gancho já está registrado.
4. **O teste da regra**, no módulo dela, rodando só a regra por `run_rules(&[&<Assunto>Rule], &input, &ctx)`.
5. **O teste do gancho inteiro**, em `end_of_turn_check.rs`, por `EndOfTurnCheck.evaluate`, quando a regra divide o bloqueio com as outras. Se a responsabilidade veio de outro gancho, escreva também o teste lado a lado: a regra sozinha e o gancho inteiro dão o mesmo veredito.
6. **O catálogo no teste**: acrescente a chave na lista de `i18n_translates_clarity_defect_keys` (ou no teste do catálogo do assunto), com as vagas.

## Exemplo completo

A regra das pendências (`apps/rt/src/hooks/task/pending_gate.rs`):

```rust
use crate::hooks::task::end_of_turn_check::{Finding, Turn, TurnRule};

/// Quantas vezes, no máximo, a regra bloqueia por fechamento.
const MAX_BLOCKS: u32 = 2;

/// A regra das pendências do fim da resposta.
pub struct PendingRule;

impl TurnRule for PendingRule {
    fn check(&self, turn: &Turn<'_>) -> Option<Finding> {
        let project = Path::new(turn.project_dir);
        let armed = armed_charges(project);
        if armed.is_empty() {
            return None;
        }
        let disk = DiskSpecState::new(project);
        let mut still_armed = Vec::new();
        let mut reasons = Vec::new();
        for mut charge in armed {
            let omitted = disk
                .log(&charge.spec)
                .map(|log| omitted_items(turn.message, project, &log))
                .unwrap_or_default();
            if omitted.is_empty() || charge.blocks >= MAX_BLOCKS {
                continue;
            }
            charge.blocks += 1;
            reasons.push(block_reason(&charge.spec, &omitted, turn.lang));
            still_armed.push(charge);
        }
        if !save_charges(project, &still_armed) || reasons.is_empty() {
            return None;
        }
        Some(Finding::Block(reasons.join("\n\n")))
    }
}
```

Quem arma a cobrança é a ponte do fechamento e do merge (`record_phase`), com um contador por spec e por número do fechamento no checkout principal (`.claude/pending/charges.json`). A regra lê os contadores sem perguntar qual é a spec atual, e nenhum gravador de eventos marca a sessão por ela.

E a lista, em `apps/rt/src/hooks/task/end_of_turn_check.rs`:

```rust
pub(crate) const RULES: &[&dyn TurnRule] = &[&PendingRule, &ClarityRule];
```

## O teste a escrever

Da regra sozinha (`apps/rt/src/hooks/task/clarity_check.rs`):

```rust
fn check(root: &Path, input: &HookInput) -> Verdict {
    run_rules(&[&ClarityRule], input, &ctx(root, Trigger::Stop))
}

#[test]
fn a_failing_reply_blocks_and_its_rewrite_only_warns() {
    let dir = project(Some("didactic"));
    let root = dir.path();
    match check(root, &stop("s1", FAILING)) {
        Verdict::Deny { reason } => {
            assert!(reason.contains("\n- CI sem as palavras por extenso"), "{reason}");
        }
        other => panic!("a failing reply blocks, got {other:?}"),
    }
    match check(root, &rewrite("s1", FAILING)) {
        Verdict::Inject { context } => {
            assert!(context.contains("\n- CI sem as palavras por extenso"), "{context}");
        }
        other => panic!("the rewrite only warns, got {other:?}"),
    }
}
```

Lado a lado (`apps/rt/src/hooks/task/end_of_turn_check.rs`, `the_pending_rule_blocks_the_same_inside_the_end_of_turn_check`): o mesmo fechamento, em dois projetos iguais, pela regra sozinha e pelo gancho inteiro; os dois vereditos são iguais.

## Armadilhas

- **Bloqueio sem fim.** Um `Finding::Block` que volta toda vez prende o turno: o Claude Code para depois de 8 bloqueios seguidos. Ou a regra vira `Finding::Warn` quando `turn.retry` é verdadeiro (a clareza), ou ela conta os bloqueios num contador próprio (as pendências, `MAX_BLOCKS`).
- **Um bloqueio só.** Os textos de todas as regras vão juntos, separados por uma linha em branco. Cada texto se sustenta sozinho e começa por `[Mustard]`.
- **Código interno na conversa.** A clareza barra "R8", "C-15" e "P-3". Não peça ao assistente para citar um número: peça o nome ou o título.
- **Tempo.** O `Stop` tem 30 segundos (`plugin/hooks/hooks.json`). A regra lê arquivos pequenos e mede texto; nada de processo externo nem rede.
- **Projeto sem Mustard.** Sem `mustard.json` a regra se cala (`mustard_core::ProjectConfig::exists`).
- **Nunca falhar.** Erro de disco vira `None` (`?`, `unwrap_or`); `unwrap` e `expect` são proibidos fora de teste pelo Clippy.
- **Sem parênteses** nos textos que passam pelo tom técnico: ele os apaga.

## Exemplos usados

- `apps/rt/src/hooks/task/end_of_turn_check.rs`
- `apps/rt/src/hooks/task/pending_gate.rs`
- `apps/rt/src/hooks/task/clarity_check.rs`
- `packages/core/src/platform/i18n.rs`
