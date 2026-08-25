# O `bash` do runner do Windows não era um shell, e o CI ficou vermelho por isso

A integração contínua estava vermelha em toda execução desde 23/08 — os dois pull requests que fecharam ontem entraram com ela vermelha, e a v0.1.46 foi publicada com o sinal aberto. Só o `windows-latest` falhava, sempre nos mesmos três testes, e a mensagem que eles deixavam no log não dizia nada: `the guard must name the crate that stayed behind:` e o vazio depois dos dois-pontos.

O vazio era a pista. Ele estava correto.

## Por quê

Os três testes `bump_guard_*` são a única cobertura da lógica de `check-lock-pins.sh`, a guarda que recusa um `Cargo.lock` cujos pacotes locais não receberam o número da versão nova. Eles cobram duas coisas dela: que recuse, e que **diga qual** pacote ficou para trás. Para isso rodam o script de verdade, por `Command::new("bash")`, e leem o canal de erro.

No `windows-latest` esse `bash` não é o Git Bash. É `C:\Windows\System32\bash.exe`, o lançador do Windows Subsystem for Linux — e sem distribuição instalada ele não executa nada. Imprime a própria reclamação em UTF-16 na saída **padrão** e sai com código 1.

Os dois sintomas caem disso de uma vez:

| o que o teste esperava | o que o lançador faz |
|---|---|
| sair com código ≠ 0 porque a guarda recusou | sai com código 1 — parece que recusou |
| nomear o pacote no canal de erro | canal de erro vazio |

A primeira asserção passava **pelo motivo errado**. Só a segunda quebrava, e por isso o log não continha nenhuma explicação: não havia erro nenhum para reportar, porque nada tinha rodado.

Descartado no caminho, com evidência: quebra de linha CRLF (o `.gitattributes` já fixa `*.sh` em `eol=lf`) e defeito na guarda (nos runners Ubuntu e macOS da mesma execução ela recusa e nomeia os dois pacotes).

## Como foi medido

Nenhuma árvore Linux consegue dizer o que o `bash` do Windows faz, então a causa não foi deduzida — foi medida. Um teste-sonda temporário, que estourava de propósito nos três sistemas para que o relatório alcançasse o log (a saída de um teste que passa é engolida), fez cinco perguntas ao runner. A primeira já respondeu tudo:

```
pergunta de controle:  echo out; echo err 1>&2; exit 7

Linux    │ código 3 │ saída padrão "OUT" │ canal de erro "ERR"
macOS    │ código 3 │ saída padrão "OUT" │ canal de erro "ERR"
Windows  │ código 1 │ "Windows Subsystem for Linux has
         │          │  no installed distributions." (UTF-16)
         │          │ canal de erro ""
```

Execução 32735943086. A sonda saiu da árvore junto com este conserto.

## O que mudou

O critério de confiança. Um candidato deixa de ser aceito por ser **lançável** e passa a ser **perguntado**.

```mermaid
flowchart TB
    subgraph Antes
      A1["Command::new(\"bash\")"] --> A2{"deu para lançar?"}
      A2 -->|sim| A3["confio"]
    end
    subgraph Depois
      B1["candidatos, melhor primeiro"] --> B2["Windows: Git for Windows"]
      B1 --> B3["todos: o bash do PATH"]
      B2 --> B4{"responde nos DOIS canais<br/>e preserva o código de saída?"}
      B3 --> B4
      B4 -->|sim| B5["confio"]
      B4 -->|não| B6["próximo candidato"]
    end
```

No Windows os caminhos do Git for Windows vêm **antes** do PATH, porque é o PATH que responde errado ali. Nos demais sistemas a ordem é a de sempre.

A decisão de aceitar ou recusar ficou separada do lançamento (`answer_is_a_shell`), o que permite reexecutar a resposta medida do lançador como três valores simples, sem depender da plataforma. É o que faz `a_stub_that_only_speaks_on_stdout_is_not_a_shell` — junto com o controle positivo, porque uma checagem que recusasse tudo satisfaria a primeira asserção e não provaria nada.

## O esconderijo que fechei junto

O conserto criava um lugar para se esconder. `run_lock_guard` já devolvia `None` quando nenhum shell podia ser lançado, e um `None` faz o teste **pular** e sair verde. Se o resolvedor não achasse o Git Bash no runner, os três guardas passariam sem ter rodado nada — e a mensagem `[skip]` nem chegaria ao log, porque a saída de um teste que passa é engolida. Verde por não ter medido, exatamente na plataforma que produziu o defeito.

Numa máquina qualquer, a ausência de shell continua sendo evidência faltando. Dentro do CI ela virou **falha declarada**: todo runner que a integração contínua e o `bump-on-main` usam traz um shell. A asserção nomeia os candidatos tentados, para que o próximo leitor não precise adivinhar onde se procurou.

## Segundo passe — o que a revisão devolveu

A primeira versão deste PR foi revisada e **reprovada**, com três achados críticos. Ficam registrados porque cada um mostra o conserto anterior fazendo menos do que a própria documentação dele afirmava.

**A pergunta de controle não cobrava a propriedade certa.** Um `bash` do WSL **com** distribuição instalada responde perfeitamente — roda o texto, fala nos dois canais, preserva o código de saída — e ainda assim não consegue abrir `C:/…/check-lock-pins.sh`, porque esse caminho não significa nada dentro da raiz dele. Aceitá-lo levaria de volta ao diagnóstico vazio que este PR existe para eliminar. A pergunta passou a incluir `test -f` sobre o próprio script que o candidato vai receber: uma pergunta, duas propriedades.

**O resolvedor já existia, e o que existia é melhor.** `find_posix_shell`, em `apps/rt/src/util/platform.rs:36`, ancora no `git.exe` do PATH e pega o `bash` ao lado — acha o Git onde quer que ele esteja, inclusive instalação por usuário (`winget`, `scoop`, instalador não elevado), que uma varredura fixa em `Program Files` não alcança. O comentário dele já registrava a mesma descoberta sobre o lançador do WSL. esta continua sendo uma SEGUNDA cópia dessa caminhada, e a dependência não a desculpa: `apps/rt/Cargo.toml:34` já declara `mustard-core`, então o resolvedor podia morar no core e o `rt` consumi-lo. As duas já divergiram — a do `rt` olha só `bin/bash.exe` e confia em conseguir lançar, esta olha também `usr/bin` e faz a pergunta de controle. Unificar move código entre duas crates e é unidade própria; o comentário nomeia a dívida em vez de inventar um motivo para ela. Os caminhos fixos ficam como segunda chance, e no Windows o PATH vai por último — é ali que ele responde errado. Fora do Windows o PATH vem primeiro, com os caminhos absolutos atrás, porque PATH podado não é o mesmo que máquina sem `bash`.

**`CI` era lido pela presença, não pelo valor.** `CI=false`, `CI=0` e `CI` vazio são grafias de "não estou no CI"; lê-las como CI transformaria um pulo local honesto em falha cuja mensagem culpa um runner onde o desenvolvedor não está. É a única checagem de `CI` da árvore, então essa leitura é o contrato inteiro.

Mais quatro achados menores da mesma revisão: os tokens da pergunta e as expectativas da resposta viraram constantes compartilhadas; cada canal passa a **conter** seu token em vez de igualá-lo, porque `bash` não interativo carrega o `BASH_ENV` e um banner de perfil não desqualifica shell nenhum; a resolução é memorizada uma vez por processo, em vez de cinco sondagens repetidas; e as **três** portas de pulo de `run_lock_guard` passam pelo mesmo ponto que estoura no CI — antes só uma fazia isso.

## Fora de escopo

Os pulos mais antigos deste arquivo continuam silenciosos no CI: raiz de workspace ausente, lock do dashboard ilegível, workflow ausente. São lacunas anteriores a esta unidade, com zero ocorrências medidas, e ampliar o conserto até elas seria consertar por achado em vez de por medida. O comentário de `skip_lock_guard` declara esse limite em vez de deixá-lo implícito.

## Verificação

Execução 32741481116, no branch desta unidade, nos três sistemas da matriz:

```
Test (ubuntu-latest)   success
Test (macos-latest)    success
Test (windows-latest)  success
```

E no log do `windows-latest`, executados — não pulados. A pergunta de controle agora exige que o shell **enxergue** o script, então este verde também prova que o Git Bash foi encontrado no runner:

```
test ci_is_read_by_value_and_not_by_presence ... ok
test a_stub_that_only_speaks_on_stdout_is_not_a_shell ... ok
test bump_guard_rejects_a_lock_whose_local_crates_did_not_move ... ok
test bump_guard_rejects_a_lock_that_lost_one_of_our_crates ... ok
test bump_guard_checks_every_local_crate_of_each_lock ... ok
```

Localmente: 11 testes verdes, nenhum `[skip]`, e nenhum aviso novo do clippy (`version_line.rs:324` e `:326` são pré-existentes, em `forge_lock`, que este PR não toca).
