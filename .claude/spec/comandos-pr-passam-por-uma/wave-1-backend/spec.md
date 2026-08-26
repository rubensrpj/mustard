---
id: wave.comandos-pr-passam-por-uma.1-backend
---

# wave-1-backend

## Summary

a porta PrProvider e seus dois adaptadores: GitHub real, Azure esqueleto honesto

## Network

- Parent: [[spec.comandos-pr-passam-por-uma]]

## Tasks

- [ ] criar apps/rt/src/shared/pr_provider.rs espelhando o padrão porta/adaptador de branch_state.rs (PrLookup): trait PrProvider com as operações open/edit_body/ready/view, cada uma devolvendo tipos normalizados
- [ ] normalização de refs NA PORTA: o Azure devolve sourceRefName como ref completa (refs/heads/x) e o GitHub headRefName curto (x); a porta fala SEMPRE o nome curto, com helper testável short_ref()
- [ ] mapa de estados NA PORTA: active→OPEN, completed→MERGED, abandoned→CLOSED, notSet→OPEN — o vocabulário canônico é o de PrStatus; o mergeStatus do Azure viaja verbatim num campo próprio Option<String>, sem mapeamento
- [ ] adaptador GithubPrCli sobre gh (mesmo formato de gh_out de pr_door.rs: cwd explícito + desvio cmd /C no Windows; erro degrada para Err(String), nunca panic)
- [ ] esqueleto AzurePrRest que responde provider-unsupported em toda operação — honesto como PR_UNSUPPORTED de branch_state.rs, nunca fingindo ausência medida
- [ ] fábrica provider_for(root) -> Box<dyn PrProvider> escolhendo pelo provedor em vigor: core::platform::git_provider::resolve_provider(root, mustard.json#git.provider)
- [ ] registrar o módulo em apps/rt/src/shared/mod.rs

## Files

- `apps/rt/src/shared/pr_provider.rs`
- `apps/rt/src/shared/mod.rs`

## Reality Obligations

- **RO-1.1** — confirmar no contrato REST GitPullRequest do Azure DevOps (documentação oficial) os campos sourceRefName (ref completa), status (active/completed/abandoned/notSet) e mergeStatus (seis valores) antes de fixar os tipos da porta
