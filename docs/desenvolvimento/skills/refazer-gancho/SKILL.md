---
name: refazer-gancho
description: Refazer um hook do Mustard alinhando contrato, registro, despacho e integração com o host.
---
# Refazer um hook

Leia o módulo afetado e seus consumidores antes de editar. A autoridade é o código vigente: `apps/rt/src/registry.rs`, `apps/rt/src/dispatch.rs`, `apps/rt/src/hook_output.rs`, `packages/core/src/domain/model/contract.rs` e `plugin/hooks/hooks.json`. Não há uma lista fechada de hooks permitidos. Preserve as responsabilidades observadas, o contexto e a autoria; remover sincronização automática de páginas não elimina observadores independentes.

1. Localize usos e provas com `rg`. Leia o contrato `Check`/`Observer`, `HookInput`, `Ctx`, `Verdict` e o evento realmente registrado. Confirme limites e serialização em `hook_output`; não imponha um teto genérico ao texto de todos os eventos.
2. Se juntar módulos, transfira responsabilidades e provas antes de retirar o anterior. Um `Check` permite eventos alheios; um `Observer` observa sem decidir autorização. `PreCompact` pode ter conferência: preserve o contrato atual do registro.
3. Atualize o registro, o despacho e o manifesto quando o evento mudar. Preserve a integração JavaScript de Mods em `plugin/hooks/register.js`; painel não gera turno do modelo, scan ou Jev por atualização.
4. Use as portas de `commands/spec_events/conversation` para conversa/medição e `write::record` com a autoria correta para estado. Não monte aprovação nem eventos binários por um caminho paralelo.
5. Busca literal original conserva texto, opções e execução. Recuperação conceitual explícita pode usar o mapa; compactação de resultado só ocorre após execução verificável. Jev julga ambiguidades pertinentes, sem autorizar alterações ou provar cobertura/ausência de uso.
6. Atualize pt-BR/en-US no catálogo e nos templates que descrevem a mudança. Publicar projeto/spec exige ação explícita; atualização de estado local não prepara banco externo.
7. Prove o comportamento na porta afetada, incluindo passagem/fallback, fase, autoria e leitura. Confira registro/manifesto pelos testes existentes e `claude plugin validate ./plugin`; mudanças de Mods usam `claude plugin test ./plugin`. Nenhuma prova escreve na configuração pessoal.

Para acrescentar uma regra ao hook existente, use `add-hook-rule`. Exemplos compiláveis devem vir da implementação atual, não de uma cópia de código nesta receita.
