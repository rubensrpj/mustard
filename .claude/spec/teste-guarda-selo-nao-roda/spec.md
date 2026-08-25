---
id: spec.teste-guarda-selo-nao-roda
---

# o teste da guarda do selo nao roda no Windows: Command::new(bash) cai no lancador do WSL, o canal de erro volta vazio e os tres testes bump_guard_* falham

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Os tres testes `bump_guard_*` conferem se a guarda do selo, alem de recusar um arquivo de travas atrasado, DIZ qual pacote ficou para tras. Para isso eles rodam o script de verdade e leem o canal de erro dele. No runner `windows-latest` esse canal volta vazio, e a asercao acaba comparando contra uma string vazia.

Por que agora: a integracao continua (CI) esta vermelha em toda execucao desde 23/08 — os dois pull requests que fecharam ontem entraram com ela vermelha. Um sinal que esta sempre vermelho deixou de ser sinal, e a v0.1.46 foi publicada com ele aberto.

A causa foi MEDIDA, nao deduzida, na execucao 32735943086: no runner do Windows a busca por `bash` nao encontra o Git Bash, encontra o lancador do Windows Subsystem for Linux. Sem distribuicao instalada ele nao executa nada — imprime a propria reclamacao em UTF-16 na saida PADRAO e sai com codigo 1. Os dois sintomas se explicam de uma vez: o codigo diferente de zero faz a primeira asercao passar pelo motivo errado, e o canal de erro vazio faz a segunda falhar.

## Usuários/Stakeholders

Quem mantem o repositorio e quem publica release. A guarda em si continua funcionando em producao (o fluxo `bump-on-main` roda em Ubuntu e ficou verde); o que quebrou foi a capacidade de confiar na matriz de tres sistemas.

## Métrica de sucesso

As tres verificacoes `bump_guard_*` passam nos tres sistemas da matriz, e nenhuma delas passa por ter sido PULADA — no Windows elas exercitam um shell real.

## Não-Objetivos

- Mexer na logica da guarda do selo: ela esta correta, medida nos runners Ubuntu e macOS da mesma execucao.
- Instalar uma distribuicao WSL no runner.
- Tirar o `windows-latest` da matriz, ou marcar os tres testes como ignorados nele.

## Critérios de Aceitação

- **AC-1** — when o `bash` encontrado responde como o lancador do WSL (fala so na saida padrao e sai com codigo diferente de zero), then a resolucao do shell o REJEITA em vez de trata-lo como shell
  Command: `cargo test --locked -p mustard-core --test version_line -- a_stub_that_only_speaks_on_stdout_is_not_a_shell --exact 2>&1 | grep -q "1 passed"`
  Control: `cargo test --locked -p mustard-core --test version_line -- the_dashboard_lock_pins_this_repositorys_crates_at_this_version --exact`
- **AC-2** — when a unidade fecha, then o teste-sonda temporario nao existe mais na arvore
  Command: `! grep -q "probe_what_bash_does_on_this_runner" packages/core/tests/version_line.rs`
  Control: `test -f packages/core/tests/version_line.rs`
- **AC-3** — a suite do arquivo passa verde
  Command: `cargo test --locked -p mustard-core --test version_line`

## Checklist

- [x] T1 — resolver um shell de verdade em `run_lock_guard`: candidatos explicitos do Git Bash no Windows, `bash` do PATH nos demais, cada candidato validado por uma chamada de controle antes de ser usado.
- [x] T2 — cobrir a rejeicao com um teste proprio, forjando um falso `bash` que so fala na saida padrao e sai com codigo 1.
- [x] T3 — remover o teste-sonda `probe_what_bash_does_on_this_runner`.
- [x] T4 — disparar o CI no branch da unidade e confirmar, no log do `windows-latest`, que as tres verificacoes passaram sem pular.

## Definitions

- **guarda do selo** — o script .github/scripts/check-lock-pins.sh, que recusa um Cargo.lock cujos pacotes locais nao receberam o numero da versao nova e nomeia quais ficaram para tras
- **stub do WSL** — C:\Windows\System32\bash.exe, o lancador do Windows Subsystem for Linux. Sem distribuicao instalada ele nao e um shell: imprime a propria reclamacao em UTF-16 na saida padrao e sai com codigo 1

## Decisions

- run_lock_guard passa a resolver um bash de verdade e a VALIDA-LO com uma chamada de controle antes de usa-lo, em vez de pular o Windows
  Reason: esses tres testes sao a unica cobertura da logica da guarda; pular a plataforma os deixaria verdes medindo nada. O runner do Windows tem Git Bash instalado, entao ha um shell real para achar
- o teste-sonda probe_what_bash_does_on_this_runner sai antes do merge
  Reason: ele estoura de proposito nos tres sistemas para que o relatorio alcance o log do CI; mante-lo deixaria a integracao continua vermelha para sempre

## Evidence

- No windows-latest, Command::new("bash") resolve para o lancador do WSL: codigo de saida 1, canal de erro VAZIO e a reclamacao em UTF-16 na saida padrao. O primeiro assert (a guarda recusou) passa pelo motivo errado e o segundo (a guarda nomeou o crate) compara contra string vazia. Medido na execucao 32735943086
  Evidence: `packages/core/tests/version_line.rs:345`
- A guarda em si esta correta: nos runners ubuntu e macos da mesma execucao ela sai com codigo 1 e nomeia mustard-cli@0.1.44 e mustard-core@0.1.44
  Evidence: `.github/scripts/check-lock-pins.sh:131`
- Hipotese REFUTADA: quebra de linha CRLF no script. O .gitattributes ja fixa *.sh em eol=lf, e o mesmo script responde certo quando rodado a mao numa arvore Linux
  Evidence: `.gitattributes:8`
- O contrato de run_lock_guard ja preve devolver None quando nenhum bash pode ser lancado, mas o stub do WSL lanca com sucesso: a porta de escape existe e nao cobre este caso
  Evidence: `packages/core/tests/version_line.rs:338`
