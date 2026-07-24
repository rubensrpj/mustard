---
id: wave.close-the-qa-verification-loop.1-gate
---

# wave-1-gate

## Summary

O Check de Stop: auto-restrição, execução via reuso do qa-run, contador próprio, texto via i18n

## Network

- Parent: [[spec.close-the-qa-verification-loop]]

## Tasks

- [ ] Criar apps/rt/src/hooks/task/stop_gate.rs — o Check de Stop. Auto-restrição: só age com spec ativa E aprovada (approval_marker_path existe) E com AC executavel (spec_has_executable_acs), e nunca num stop de subagente (HookInput::is_subagent). Fora disso, Verdict::Allow silencioso.
- [ ] Quando ha o que verificar, executa os criterios pelo caminho do qa-run — run_for_spec_with_options / parse_ac_items / gather_capability_acs. NUNCA um segundo parser de AC. Um teste de paridade prova que o veredito coincide com o do qa-run.
- [ ] Contador proprio de bloqueios CONSECUTIVOS por-spec: caminho do marcador em shared/context.rs, ao lado de approval_marker_path/clarified_marker_path. Incrementa a cada bloqueio; ZERA quando os criterios passam; honra stop_hook_active como sinal secundario; ao teto (constante documentada, nao um MUSTARD_*_MODE novo), libera.
- [ ] Chaves stopgate.* no catalogo i18n (packages/core/src/platform/i18n.rs) para o texto do bloqueio, na lingua do projeto; nenhuma prosa embarcada no gate.
- [ ] Se o veredito precisar carregar o reason ate a emissao, estender Verdict/Outcome em packages/core/src/domain/model/contract.rs (contrato publico — migracao, nao quebra de shape).

## Files

- `apps/rt/src/hooks/task/stop_gate.rs`
- `apps/rt/src/commands/review/qa_run/mod.rs`
- `apps/rt/src/shared/context.rs`
- `packages/core/src/platform/i18n.rs`
- `packages/core/src/domain/model/contract.rs`
