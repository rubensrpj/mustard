## Quarta revisao: APPROVED — 0 criticos

AC-1, AC-2, AC-3 e controle: PASS. `cargo test --workspace` 3049 passed, 6 ignored. Clippy 0 erros.

### Verificacao independente (nao pela palavra do implementador)
O revisor recusou aceitar o booleano entregue na mao como prova da medida e dirigiu o BINARIO REAL num repositorio descartavel com instalacao privada:
- mold rastreado -> a linha estrita, pedindo unidade propria
- `git rm --cached` no mesmo mold -> a linha relaxada, "dispatch it right here, now"
Ambas por `emit-pipeline --kind pipeline.kind`, uma linha, stderr, exit 0. O texto versionado bate byte a byte com o de antes da onda.

Duas premissas confirmadas por experimento: `git check-ignore` e ciente do index (caminho rastreado-mas-ignorado sai 1, logo conta como versionado — a direcao perigosa e inalcancavel), e `guards_file_name` resolve o mesmo nome que o censo usa. Checagem de relaxamento vazio: lacuna nao-vazia sempre tem ao menos um alvo medido.

Impressao digital recalculada em FNV-1a/64 sobre o template superado: bate exatamente.

### Nao-bloqueantes — todos corrigidos
1. Link de doc apontava para `measure`, renomeado para `measure_with_targets`; rustdoc nao pega porque o item e privado.
2. A impressao digital foi PREposta, enquanto a doc do catalogo diz APPEND, e sem o comentario que as duas irmas mais novas carregam.
3. Campos de `EnrichmentGapPaths` eram `pub(crate)` e lidos so dentro do proprio modulo.

Custo medido: ~34 ms de caminhada mais ~1,2 ms por spawn de check-ignore; pior caso ~100 ms por emit de abertura.
