---
id: wave.harness-ve-toda-branch-trabalho.2-surfacing
---

# wave-2-surfacing

## Summary

O harness passa a FALAR: o inventário de specs enxerga a branch que existe só no remoto, a statusline informa quantas unidades devem poda, o início de sessão avisa quando há pendência, e as duas varreduras antigas morrem convergindo no enumerador.

## Network

- Parent: [[spec.harness-ve-toda-branch-trabalho]]
- Depends on: [[wave.harness-ve-toda-branch-trabalho.1-enumerator]]

## Tasks

- [ ] active_specs.rs: scan_work_branches (linha 429) enumera hoje APENAS refs/heads/ — a enumeração de refs/remotes e origin/ no arquivo retorna zero ocorrências. Consequência medida: uma spec que vive só numa branch remota é invisível ao inventário. Ele passa a consumir o enumerador da Onda 1, e a coluna de localização ganha um terceiro valor: só-no-remoto.
- [ ] active_specs.rs + git_settle.rs: com os dois consumindo o enumerador, NENHUMA das duas varreduras anteriores sobrevive. AC-2 afirma isso. Um terceiro varredor seria o defeito, não a solução — é a razão registrada da decisão principal desta spec.
- [ ] statusline/segment.rs: um segmento novo informando a contagem de unidades pendentes de poda, alimentado pelo classificador. A barra tem dez segmentos hoje e nenhum diz nada sobre estado de branch ou de PR. A checagem de ancestralidade é local mas não é gratuita — use cache curto, como os segmentos vizinhos já fazem.
- [ ] statusline/mod.rs: liga o segmento novo na composição da barra, na ordem que o tema já define.
- [ ] hooks/session/session_start_inject.rs: uma linha no início de sessão quando houver pendência. Esta é a correção da causa de campo: o comando de poda existe, funciona, e ninguém o chamou por seis unidades seguidas. Não faltou comando, faltou avisar.
- [ ] packages/core/src/platform/i18n.rs: as chaves de catálogo de todo texto novo voltado ao usuário. NENHUM literal de idioma no código — a lei de agnosticismo do projeto, que o work_branch_gate acabou de ser corrigido por violar.
- [ ] plugin/commands/review.md: remove a instrução que manda o usuário rodar uma ação de merge do fluxo git quando não encontra PR. Essa ação NÃO existe e o próprio fluxo de git a declara inexistente — é instrução morta apontando para o nada.

## Files

- `apps/rt/src/commands/spec/active_specs.rs`
- `apps/rt/src/commands/statusline/segment.rs`
- `apps/rt/src/commands/statusline/mod.rs`
- `apps/rt/src/hooks/session/session_start_inject.rs`
- `packages/core/src/platform/i18n.rs`
- `plugin/commands/review.md`
