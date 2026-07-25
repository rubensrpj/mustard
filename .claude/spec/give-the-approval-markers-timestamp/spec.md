---
id: spec.give-the-approval-markers-timestamp
---

# give the approval markers a timestamp and one writer, and make approve-spec and status read the provenance they already record

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**Hoje.** Quando você aprova um plano, o harness deixa um arquivo-marca dentro da pasta da spec. Ele
não está vazio — grava quem aprovou por qual porta. Este é o conteúdo real do marcador que esta
própria sessão gravou:

```
spec=scope-scan-generated-role-pattern
via=AskUserQuestion
session=e3f4cee2-ecf4-4095-a2b1-05f558c9e9b8
```

**Por que isso é um problema.** O registro existe e ninguém o usa. Três coisas, todas verificadas
lendo os arquivos:

1. **Não há data.** Nenhum dos três escritores grava um instante. A única data que existe é a do
   sistema de arquivos, que qualquer cópia, `checkout` ou sincronização reescreve. Quem for auditar
   *quando* um plano foi aprovado não tem onde olhar.
2. **Ninguém lê o corpo.** O `approve-spec` só pergunta se o arquivo existe
   ([approve_spec.rs:303](apps/rt/src/commands/spec/approve_spec.rs:303)); o mesmo vale para o
   `resume-bootstrap`. O `/status` não mostra nada disso. Um dado gravado que nenhum leitor consome
   é custo sem retorno — e, pior, ninguém percebe quando ele para de ser gravado.
3. **O mesmo texto é montado em três lugares**, e eles já divergiram:

| escritor | porta | grava `session=`? |
|---|---|---|
| [plan_approval_observer.rs:83](apps/rt/src/hooks/observe/plan_approval_observer.rs:83) | `ExitPlanMode` | sim |
| [approval_marker_observer.rs:274](apps/rt/src/hooks/observe/approval_marker_observer.rs:274) | `AskUserQuestion` | sim |
| [grill_capture.rs:220](apps/rt/src/commands/grill_capture.rs:220) | `grill-finalize` | **não** |

A terceira linha não é hipótese: o `.clarified` desta sessão tem duas linhas, e os outros dois têm
três. É a divergência que sempre acontece quando o mesmo formato é escrito em três lugares.

**Onde isso deve morar.** A casa já existe e está documentada como tal:
[context.rs:544](apps/rt/src/shared/context.rs:544) e :569 dizem, no comentário, que são
*"a casa única deste caminho, para que seus dois consumidores não divirjam"*. O caminho já é único;
o **corpo** não é. Falta o par: quem escreve o texto e quem o lê de volta, ao lado de quem já compõe
o caminho.

**Por que agora.** É pequeno — cerca de vinte linhas — e fecha um laço que já está 80% construído.
Não é reescrever os observadores: é dar a eles uma função em comum e um leitor.

## Usuários/Stakeholders

Quem audita uma spec depois: passa a ver por qual porta e quando ela foi aprovada, sem abrir arquivo
oculto. E quem mantém o harness: um formato só, num lugar só.

## Métrica de sucesso

Os três escritores passam a produzir o mesmo formato, agora com data, e ele aparece na saída do
`approve-spec` e do `/status` sem que ninguém precise abrir o arquivo à mão.

## Não-Objetivos

- **Fazer o `approve-spec` falhar por corpo ilegível.** A existência do arquivo continua governando
  o portão. Um corpo que não parseia degrada para "sem proveniência", nunca para "reprovado" — senão
  esta unidade transformaria um registro informativo num novo modo de falha.
- **Reescrever os observadores.** Eles continuam decidindo *quando* gravar; só param de montar o
  texto por conta própria.
- **Mudar o formato para JSON.** O arquivo é lido por humanos em `cat`; `chave=valor` por linha é o
  que já existe e basta.
- **H5 (a memória).** É trabalho fora do repositório, não consome spec, e segue como recomendação
  separada.

## Critérios de Aceitação

- **AC-1** — when qualquer uma das três portas grava seu marcador, then o corpo sai da mesma função
  e carrega `spec`, `via`, `session` e um instante em formato ISO-8601
  Command: `cargo test -p mustard-rt marker_body_is_the_single_writer`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when um marcador gravado é lido de volta, then a proveniência volta em campos tipados,
  e um corpo ilegível ou truncado devolve "sem proveniência" em vez de erro
  Command: `cargo test -p mustard-rt read_marker_provenance_round_trips_and_degrades`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — when o `approve-spec` aprova uma spec, then ele ecoa por qual porta e quando a
  aprovação foi dada
  Command: `cargo test -p mustard-rt approve_spec_echoes_provenance`
  Expect: `[1-9][0-9]* passed`
- **AC-4** — when o marcador existe mas seu corpo está ilegível, then o `approve-spec` continua
  aprovando, porque a existência é que governa o portão
  Command: `cargo test -p mustard-rt unreadable_marker_body_still_approves`
  Expect: `[1-9][0-9]* passed`
- **AC-5** — when o `/status` mostra uma spec aprovada, then a linha traz a porta e a data
  Command: `cargo test -p mustard-rt status_shows_approval_provenance`
  Expect: `[1-9][0-9]* passed`

## Arquivos

- `apps/rt/src/shared/context.rs` — o par `marker_body` + `read_marker_provenance`, ao lado dos dois
  compositores de caminho que já moram lá
- `apps/rt/src/hooks/observe/plan_approval_observer.rs` — passa a usar o escritor comum
- `apps/rt/src/hooks/observe/approval_marker_observer.rs` — idem
- `apps/rt/src/commands/grill_capture.rs` — idem, e ganha o `session=` que hoje lhe falta
- `apps/rt/src/commands/spec/approve_spec.rs` — ecoa a proveniência ao aprovar
- `apps/rt/src/commands/pipeline/status.rs` — mostra porta e data na linha da spec

## Checklist

- [ ] T1 — `marker_body(spec, via, session, ts)` e `read_marker_provenance(path)` em `context.rs`.
- [ ] T2 — os três escritores passam a chamar `marker_body`; o `grill-finalize` ganha `session=`.
- [ ] T3 — `approve-spec` e `/status` ecoam, com degradação silenciosa em corpo ilegível.
- [ ] T4 — testes dos cinco comportamentos, incluindo o caminho degradado.