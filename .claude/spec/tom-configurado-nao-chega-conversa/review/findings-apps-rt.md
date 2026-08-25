## Segunda revisao: APPROVED — 0 criticos

Os quatro ACs e o controle re-rodados pelo revisor: verdes. Regressao 2.159 testes. Clippy limpo (o unico aviso e pre-existente, conferido contra a base).

### Verificado com o binario real, em caixa de areia
- mensagem comum num projeto que declara didactic -> 7.098 caracteres injetados, terminando na regra. Sob o teto de 10.000, com 2.900 de folga.
- SEGUNDA mensagem da mesma sessao -> so a regra (o injetavel `once:true` ja foi gasto). Confirma "toda mensagem", nao uma vez.
- `/mustard:pr merge` e `/mustard:upsert` -> a regra chega. O AC-3 e real, nao so no teste.
- `tone: technical`, config sem `tone`, e SEM `mustard.json` -> saida vazia. A decisao "como foi escrito" vale em campo.
- sem `mustard.json` + `/mustard:pr merge` -> o portao de instalacao continua bloqueando.

Moldes e guards: sem violacao. O revisor tambem conferiu o risco de dois `Inject` na mesma invocacao — nenhum irmao emite verdito nesse evento.

### Nao-bloqueantes — os tres corrigidos
1. MAIOR: a documentacao de `evaluate` ainda dizia que um comando `/mustard:*` nunca injeta. Minha correcao anterior nao pegou porque o texto quebrava linha noutro ponto. Agora descreve o que o codigo faz.
2. MENOR: o AC-2 nomeia tres casos e o teste cobria dois; o projeto SEM `mustard.json` estava provado so a mao. Agora esta no teste.
3. MENOR: linha em branco dupla, e a funcao do tom estava encravada no meio do grupo de predicados de prompt. Movida para depois do grupo, com a faixa de secao propria.

### Nota para o operador
`mustard.json` marca 0.1.42 enquanto o `Cargo.toml` esta em 0.1.43 — e o selo que o proximo upsert reescreve, agora automaticamente.
