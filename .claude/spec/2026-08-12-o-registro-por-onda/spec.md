# Tactical Fix: o registro por onda distingue as ondas: o portao de fronteira resolve a onda pelo papel do agente que escreve e o retorno do agente chega ao registro da propria onda

## Context

Tactical fix derived from [[worktree-isolation-becomes-usable-it]].

Uma rodada de despacho dispara TODAS as ondas do menor nivel de dependencia ao
mesmo tempo. Na rodada 1 daquele spec, as ondas 1 e 2 escreveram juntas — e o
registro por onda nao soube separar as duas, em dois lugares.

### Defeito A — o portao acusava a onda errada

`boundary_gate.rs:180` lia `view.current_wave`, um ESCALAR cuja definicao e
`max(completedWaves) + 1`. Com nada completo, ele responde `1` para todo mundo.

```
ANTES                                    DEPOIS
agente da onda 1 ─┐                      agente da onda 1 ─┐
                  ├─> current_wave = 1                     ├─> carimbo do proprio
agente da onda 2 ─┘    (escalar)         agente da onda 2 ─┘   transcript (por filho)
                        │                                        │
                        v                                        v
              wave-1-carry/spec.md                    a onda de QUEM escreve
                        │                             (ou a UNIAO das ondas
                        v                              da rodada em voo)
   WARN em todo Edit da onda 2, inclusive
   em worktree_gc.rs e work_removed.rs, que
   wave-2-reap/spec.md declara em `## Files`
```

O sinal que separa irmaos em voo e o carimbo `<!-- mustard:wave=N -->` que o
despacho deixa na primeira linha do transcript do proprio filho — verificado nos
tres transcripts daquela rodada (waves 1, 2 e 3, corretos). O `agent_id` chega no
`PreToolUse` de dentro do subagente (contrato oficial de hooks: "present only
when the hook fires inside a subagent call") e, com o `transcript_path`, localiza
o transcript do filho. Quando nada identifica o escritor, o portao passa a usar a
UNIAO dos `## Files` das ondas do nivel em voo — mais estreito do que liberar
tudo, e nunca acusa arquivo corretamente declarado.

### Defeito B — o retorno da onda nao chegava ao registro dela

1. `wave-done --wave 1` acusou `RO-1.1` sem prestacao de contas, embora o retorno
   do agente da onda 1 comecasse com `RO-1.1 — verified on this install...`. O
   unico canal lido era `agent.stop`, emitido no `PostToolUse(Task)`: num
   despacho em background o `tool_response` e o ACK de lancamento
   (`{"isAsync":true,"status":"async_launched",...}`), produzido quando o filho
   COMECA. O retorno nunca esteve la.
2. A mesma chamada reportou `memoriesWritten: [".../memory/shared-proc-...-wave2.md"]`
   — a memoria do agente da ONDA 2, colhida pelo fecho da onda 1 porque a colheita
   era por spec, nao por onda.

## Acceptance Criteria

- AC-1 — quando duas ondas estao no mesmo nivel de dependencia e o escalar
  `currentWave` da projecao vale 1, entao uma escrita em arquivo declarado no
  `## Files` da onda 2 NAO produz aviso de fronteira, e um arquivo que nenhuma
  onda da rodada declarou continua avisando, nomeando a fronteira conferida.
  Command: `cargo test -p mustard-rt a_parallel_siblings_declared_file_is_not_accused`
  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt boundary_gate`
- AC-2 — quando duas ondas deixam cada uma o proprio retorno registrado, entao a
  finalizacao da onda 1 presta contas apenas das obrigacoes da onda 1 e colhe
  apenas as memorias da onda 1.
  Command: `cargo test -p mustard-rt each_waves_finalisation_reads_only_its_own_record`
  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt wave_done`
- AC-3 — o workspace continua compilando. Command: `cargo build --workspace`

## Files

| File | What changes |
|---|---|
| `apps/rt/src/hooks/write/boundary_gate.rs` | `resolve_spec_file` vira `resolve_boundary_files`: resolve a onda de QUEM escreve pelo carimbo do transcript do filho e, sem esse sinal, usa a uniao dos `## Files` das ondas do nivel em voo. O aviso nomeia a fronteira conferida. |
| `apps/rt/src/hooks/task/subagent_inject.rs` | `wave_from_child_transcript` passa a `pub(crate)` e ganha o candidato derivado (`transcript_path` + `agent_id`), que e o unico que resolve dentro de um `PreToolUse`. Novo `capture_return_report`: o retorno do filho vira evento `agent.return` carimbado com a onda dele; sem onda estabelecida, nada e gravado. |
| `apps/rt/src/commands/pipeline/wave_done.rs` | `recorded_return_text` passa a ler `agent.return` e a filtrar por onda; `materialize_wave_memory` recebe a onda e colhe so o que e dela (mais as licoes sem onda, que ninguem reclama). |
| `apps/rt/src/commands/pipeline/dispatch_plan.rs` | `wave_spec_path` passa a `pub(crate)` — o portao resolve o `spec.md` da onda pelo MESMO scanner que o despacho usou. |
| `.claude/spec/2026-08-12-o-registro-por-onda/spec.md` | Esta spec. |

## Boundaries

IN: qual onda o portao de fronteira confere; como o retorno de um agente chega ao
registro da onda dele; a quem pertence cada memoria colhida no fecho de onda.

OUT: silenciar o portao; mudar o formato de saida de `wave-done` (as chaves do
JSON seguem as mesmas); transformar `realityUnaccounted` em bloqueio — segue
relatorio, nunca portao; qualquer subcomando novo de `run`.

<!-- wikilinks-footer-start -->
- [worktree-isolation-becomes-usable-it](?) ⚠ unresolved
<!-- wikilinks-footer-end -->