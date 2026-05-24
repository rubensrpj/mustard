# Onda de Revisão — meta-sidecar

## Resumo

Revisão final agregando o trabalho das 4 ondas. Confere que nenhuma spec ficou sem `meta.json`, que nenhuma fonte de leitura ainda depende dos headers do `.md`, e que o parser foi simplificado de verdade.

## Tarefas

### Review Agent

- [ ] Conferir que `mustard-core::meta` expõe `read_meta` e `write_meta` com tratamento fail-open documentado.
- [ ] Conferir que `pipeline_state_ingest` prefere `meta.json` e usa fallback antigo só quando o JSON não existe.
- [ ] Conferir que `wave-scaffold`, `emit-pipeline` e o caminho de `tactical-fix` escrevem `meta.json` em toda spec nova.
- [ ] Conferir que o dashboard consome `read_spec_meta` e não depende mais do parser de headers.
- [ ] Conferir que após a Onda 4 nenhum `spec.md` em `.claude/spec/**` contém linhas `### Stage:`/etc.
- [ ] Conferir que `spec_sections.rs` perdeu as entradas redundantes (só seções de conteúdo restam).
- [ ] Conferir que o subcomando `migrate-to-meta` é idempotente (rodar duas vezes não duplica nada nem corrompe).
- [ ] Conferir que o subcomando `cleanup-spec-md` é idempotente.
- [ ] Conferir que o `migrate_spec_headers.rs` foi deletado OU redirecionado pra `write_meta`, e nada chama o caminho antigo.
- [ ] Build, lint e test de todo o workspace passam.

## Limites

Não escreve código novo. Aponta o que precisa voltar.
