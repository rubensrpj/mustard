<!-- mustard:generated -->
# Ordem canônica do prompt — PREFIX-STABLE / VARIABLE

## Por que um prefixo estável importa

A API da Anthropic faz cache automático de prefixos de prompt que sejam **byte-idênticos** entre chamadas próximas no tempo. Quando o cache acerta, a parte cacheada é cobrada a **10% do custo normal** de input. O limiar mínimo é de **~1024 tokens** (na prática, 1024 caracteres é uma aproximação conservadora). Sem um bloco estável bem demarcado, cada dispatch é único em bytes — o cache nunca ativa e o pipeline paga input cheio em conteúdo que se repete (skills, recipe, role rules, pipeline-config snippet). A reordenação para `[PREFIX-STABLE] → [VARIABLE]` resolve isso colocando 100% do conteúdo dinâmico (spec slice, diff, retry context, TASK) **depois** do marcador, garantindo que o prefixo permaneça idêntico entre waves e entre dispatches do mesmo template.

## Ordem canônica

O template `agent-prompt/SKILL.md` produz, após interpolação, um arquivo no formato:

```text
<!-- PREFIX-STABLE -->

## CONTEXT
...links para skills/recipes (apenas IDs/nomes, sem inline do conteúdo)...

## REFERENCE
...arquivos a serem lidos pelo agent (apenas paths)...

## SKILLS
...lista de skills disponíveis (apenas nomes; o agent invoca Skill tool para carregar)...

## RECIPE
...nome do recipe a aplicar (não o conteúdo)...

## ROLE
...regras de papel (estáticas para o template)...

## EFFICIENCY
...regras de eficiência (estáticas)...

<!-- VARIABLE -->

## RETRY CONTEXT
...só presente em re-dispatches; texto varia a cada chamada...

## TASK
...spec slice, diff da wave anterior, lista de arquivos, AC inline...
```

Tudo que vier **antes** de `<!-- VARIABLE -->` precisa ser textualmente idêntico entre dispatches do mesmo template para o cache acertar.

## Regras

- **Interpolação dentro de PREFIX-STABLE só pode usar valores estáveis.** Skill IDs (`karpathy-guidelines`), nomes de recipe (`feature.entity-crud`), nomes de role (`Implementation Agent`) — nunca os corpos. O agent é responsável por carregar o corpo via Skill tool quando precisar.
- **Os marcadores são comentários HTML, preservados verbatim.** `<!-- PREFIX-STABLE -->` e `<!-- VARIABLE -->` aparecem literais no prompt final. Não envolva em código, não traduza, não reformate.
- **Qualquer interpolação de spec text, diff, ou retry context dentro do PREFIX-STABLE invalida o cache.** Se você precisa injetar conteúdo dinâmico, faça isso depois do `<!-- VARIABLE -->` marker. Se descobrir um caso onde isso parece impossível, abra issue antes de violar a regra — provavelmente é um sinal de que o template precisa ser dividido.
- **Tamanho mínimo do prefixo: 1024 caracteres** (aprox. 1024 tokens). Prefixos menores ainda são válidos textualmente, mas não ativam cache — o ganho fica em 0.

## Como verificar

Renderize um prompt do template para stdin do script abaixo:

```bash
node -e "const {analyzePrompt}=require('./templates/hooks/_lib/prompt-cache-detect.js'); console.log(analyzePrompt(require('fs').readFileSync(0,'utf8')))"
```

Saída esperada:

```json
{ "prefix_len": 2814, "prefix_hash": "a1b2c3...", "variable_len": 4120, "prefix_cacheable": true }
```

Se `prefix_cacheable` vier `false`, ou o prefixo está abaixo de 1024 chars, ou o marcador `<!-- PREFIX-STABLE -->` está ausente. Se `prefix_hash` mudar entre dois dispatches do mesmo template, alguma interpolação dinâmica vazou para o bloco estável — revise as variáveis injetadas antes do `<!-- VARIABLE -->`.
