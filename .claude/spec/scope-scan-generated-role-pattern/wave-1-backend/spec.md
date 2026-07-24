---
id: wave.scope-scan-generated-role-pattern.1-backend
---

# wave-1-backend

## Summary

O glob do cluster sai do censo e chega ao molde como paths:

## Network

- Parent: [[spec.scope-scan-generated-role-pattern]]

## Tasks

- [ ] Em scan_patterns/list.rs: derivar o glob de cada candidato a molde APENAS dos diretorios que o censo ja registrou para o cluster (dirs / common_dir / exemplars). Nunca um caminho inventado, nunca o nome do papel. Barra para frente sempre. Expor o glob como campo novo da worklist JSON, ao lado de moldPath e exemplars.
- [ ] Em render/role.rs: o contrato canonico do molde no prompt do papel patterns passa a exigir paths: com o valor que a worklist entregou, ao lado das chaves ja exigidas. Nao remover tags/appliesTo/scope: o skill-resolve as consome.
- [ ] Em core/domain/skill/frontmatter.rs: paths vira campo tipado do SkillFrontmatter (aceitando lista YAML ou string separada por virgula, como a plataforma documenta), em vez de cair no extra flatten. Manter o parse leniente e fail-open.
- [ ] Em scan_patterns/apply.rs: garantir que paths: sobrevive a gravacao. A regra create-only e a de nao inventar frontmatter que o agente nao escreveu continuam valendo.
- [ ] Testes: um por AC-1..AC-4, nos modulos correspondentes. O de list.rs prova que o glob sai dos dirs do censo e nao de literal escrito no codigo.
- [ ] EXTENSAO DE ESCOPO (AC-7), achada durante o EXECUTE: dependency_precheck.rs responde ok:true e sai 0 quando nao consegue ler a spec (verificado contra caminho inexistente). O fluxo o trata como portao. Fazer o caminho spec-not-readable responder ok:false, preservando o campo de erro e sem entrar em panico. Teste dos dois lados: spec legivel segue ok:true, spec ilegivel da ok:false.

## Files

- `apps/rt/src/commands/scan_patterns/list.rs`
- `apps/rt/src/commands/agent/render/role.rs`
- `apps/rt/src/commands/scan_patterns/apply.rs`
- `apps/rt/src/commands/review/dependency_precheck.rs`
- `packages/core/src/domain/skill/frontmatter.rs`

<!-- wikilinks-footer-start -->
- [spec.scope-scan-generated-role-pattern](spec.md)
<!-- wikilinks-footer-end -->