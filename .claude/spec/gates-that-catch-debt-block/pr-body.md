# Portões que travam dívida de verdade; os que travam plano incompleto continuam só avisando

Este PR fecha uma dívida encontrada nesta mesma sessão, logo depois de mergear a unidade da instalação privada (PR #161): os portões de tamanho do Mustard avisavam quando um documento passava do orçamento, mas deixavam passar. E o comando `/mustard:spec` tinha um defeito de campo — pedia confirmação exatamente onde a própria regra do projeto dizia que não devia.

## Por quê

O usuário pediu, em suas palavras: *"o Mustard precisa ser sem atrito, tem que fluir, mas fluir dentro daquilo que não quebre o Mustard e o seu princípio."*

Isso vira uma regra concreta assim que se pergunta: quando um portão trava, o que ele está protegendo? Duas respostas possíveis, e são diferentes:

- **Dívida** — algo que o próprio autor resolve sem sair de onde está (documento grande demais, item de checklist esquecido). Travar aqui custa uma edição e evita que todo mundo depois pague o excesso.
- **Plano incompleto** — o agente precisou escrever um arquivo que a spec não previu. Travar aqui não corrige nada; só impede o trabalho de continuar até alguém editar a spec e redespachar.

Isso não era hipotético. **Aconteceu dentro desta própria sessão**, na unidade anterior: o implementador da onda 2 precisou criar `apps/rt/src/shared/context.rs`, que eu tinha esquecido de listar no plano. O portão de fronteira (`boundary_gate`) avisou quatro vezes, deixou o trabalho seguir, e eu corrigi a spec depois. Se esse portão travasse por padrão, a onda teria morrido ali — não por erro do agente, mas por erro do meu plano.

## O que mudou

**Três portões de tamanho passam a bloquear por padrão** (não é mais preciso configurar nada):

| Portão | O que verifica |
|---|---|
| `MUSTARD_SPEC_SIZE_MODE` | uma spec passou do orçamento de linhas |
| `MUSTARD_SKILL_SIZE_MODE` | uma skill passou do orçamento |
| `MUSTARD_SKILL_VALIDATE_GATE_MODE` | uma skill está malformada |

**Um portão continua avisando, de propósito** — `boundary_gate` (arquivo fora do que a spec listou). A razão agora está escrita no próprio código-fonte, ao lado da decisão, para que o próximo autor não "corrija" isso por engano:

```
ANTES                                    DEPOIS
todos os portões de tamanho    →   3 travam (dívida, resolvível
avisavam                            na hora), 1 continua avisando
                                     (plano incompleto, não é
                                     erro do autor)
```

**E o `/mustard:spec` foi corrigido.** A regra já existia — "dentro da própria branch da unidade, retomar não custa nada" — mas só valia a partir do passo 3 do fluxo. O passo 1 ainda dizia "vazio → mostra a tabela", sem exceção. Resultado: chamar `/mustard:spec` sem nada, já dentro da branch de uma unidade, mostrava a tabela mesmo assim — pedindo para você escolher a linha em que já estava.

## Como validar

```bash
cargo test -p mustard-rt --test gates_block_debt
```

Os três testes leem o código e a prosa reais — nenhum é simulado:

```bash
grep -A3 "MUSTARD_SPEC_SIZE_MODE" apps/rt/src/hooks/write/size_gate.rs
# deve mostrar GateMode::Strict como padrão

grep -A3 "MUSTARD_BOUNDARY_MODE" apps/rt/src/hooks/write/boundary_gate.rs
# NÃO deve mostrar GateMode::Strict — continua avisando

grep -B1 -A1 "Empty, and the checkout IS" plugin/commands/spec.md
# deve mostrar a nova regra do passo 1
```

## Testes

| Critério | O que garante | Comando |
|---|---|---|
| AC-1 | documento acima do orçamento é recusado sem configuração | `cargo test -p mustard-rt --test gates_block_debt ac1_…` |
| AC-2 | arquivo fora do plano avisa e deixa seguir | `… ac2_…` |
| AC-3 | `/mustard:spec` dentro da branch não pergunta | `… ac3_…` |
| AC-4 | o workspace compila | `cargo build --workspace` |

Suíte completa: **2943 testes, exit 0.**

## Decisões que valem explicar

**A linha entre travar e avisar não é "o que o portão vê", é "o que o autor consegue resolver sem sair do lugar".** Documento grande demais: uma edição resolve. Plano incompleto: só resolve saindo do trabalho para editar a spec. Tratar os dois igual — travando tudo — parecia mais rigoroso e na prática é atrito sem proteção.

**A correção do `/mustard:spec` é só prosa**, porque o arquivo de comando *é* a implementação — não existe um "código Rust" separado para esse comportamento. Por isso o teste do AC-3 lê o texto do arquivo `.md` diretamente, em vez de rodar um binário.

**Um segundo achado nesta sessão, tratado à parte** (arquivo `plugin/output-styles/mustard-didactic.md`, sem AC formal porque é sobre estilo de resposta, não comportamento medível): o usuário apontou que minha explicação sobre esses mesmos portões ficou complicada demais. O arquivo de estilo mandava "ser didático" mas não proibia o que deu errado — usar termo interno sem traduzir, e oferecer três opções densas em vez de uma recomendação. As duas regras foram escritas no estilo da casa.

## Fora de escopo

- **Os outros portões que já bloqueavam** (fechamento, dívida, checklist, achados, QA) não mudaram — já nasciam estritos, e eu tinha informado o usuário do contrário por engano durante a conversa. Corrigido verbalmente, sem necessidade de código.
- **`mainBudget`** continua avisando — mede se a sessão principal está fazendo trabalho que devia ser delegado, e travar isso no meio de uma sessão é uma decisão de escopo maior do que esta unidade.