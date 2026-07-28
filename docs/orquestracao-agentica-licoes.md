# Orquestração agêntica: o que um harness entrega e o que não

Documento de lições transferíveis sobre executar trabalho de software em múltiplas etapas com agentes, sob um harness de orquestração — o componente que decide a ordem das etapas, materializa prompts, guarda estado entre sessões e roda os portões de qualidade.

Confiança: alta nas observações; média nas generalizações. Base empírica: seis etapas concluídas de um plano de onze, com correções fora de escopo ao longo do caminho.

## O modelo mental

> Um harness de orquestração é um **trilho**, não um **detector**.

Ele garante que o trabalho aconteça na ordem certa, sobreviva ao fim da sessão e deixe rastro auditável. Ele **não descobre** que o código está errado. Confundir as duas funções é a origem da maioria das frustrações — e de quase todo defeito que escapa.

## O que ele entrega

**Roteamento com autoridade sobre o julgamento do agente.** O caso mais instrutivo foi um em que o modelo afirmou, com convicção, qual seria a próxima etapa — e o roteador determinístico devolveu outra. A etapa que o modelo pularia estava parcialmente feita e teria sido abandonada em silêncio. A lição não é "o roteador é esperto"; é que **a decisão dele precisa ter autoridade, não ser conselho**. Um harness cuja saída o modelo pode reinterpretar não protege de nada.

**Persistência entre sessões.** Plano, critérios de aceitação, progresso e marcadores de aprovação gravados em disco e versionados sobrevivem a pausas longas, trocas de contexto e sessões novas. Retomar sem replanejar e sem repetir a aprovação humana é o que torna trabalho de múltiplos dias viável.

**Trilha de desvio.** Um guarda que avisa a cada edição fora do escopo declarado converte "mexi em algo que não estava no plano" em registro reconstituível. Sem ele, o desvio existe do mesmo jeito — só que invisível.

**Propagação de achado entre etapas.** Um canal em que descobertas feitas numa etapa entram no prompt das seguintes. Sem esse canal, cada descoberta morre na conversa onde nasceu, e a etapa seguinte repete o erro.

**Prompt materializado pela máquina.** Prompts de implementador escritos à mão divergem entre si a partir do terceiro. Renderizá-los a partir do plano é o que mantém as etapas comparáveis.

## O que ele não entrega

**Detecção.** Os defeitos de verdade encontrados no período — navegação para rota inexistente, campo de configuração zerado a cada salvamento, validação frouxa habilitando uma ação que o servidor recusa, vocabulário inconsistente entre telas — **nenhum veio de um portão**. Todos vieram de agentes lendo código e de verificação humana do relato deles. O harness organizou, preservou e propagou; não encontrou.

**Visibilidade de trabalho pausado.** Trabalho interrompido antes de integrar tende a ficar invisível de fora do seu próprio ramo. Um listador de trabalho em andamento mostra o que está integrado, não o que está em voo. Isso é consequência do controle de versão, não defeito do harness — mas é um ponto cego caro.

**Coerência entre os próprios comandos.** Executando com o contexto errado, um comando respondeu com dados plausíveis montados de rascunho abandonado, enquanto outro, na mesma condição, respondeu honestamente "não existe". A resposta plausível é mais perigosa que o erro: ela induz afirmação confiante e errada.

**Bloqueio.** Guarda que só avisa depende inteiramente da disciplina de quem lê o aviso.

## Armadilhas recorrentes

**Campo de posição confundido com próxima ação.** Um estado chamado "etapa atual" quase nunca significa "a etapa a executar agora" — costuma ser um marcador de posição. Nomes assim produzem erro de leitura mesmo em quem conhece o sistema. Regra: **quem decide a próxima ação é o roteador, não um escalar do estado**.

**Fadiga de aviso.** Se o guarda de fronteira dispara para os mesmos arquivos indefinidamente porque a lista de escopo nunca é atualizada, ele deixa de ser sinal e vira ruído. Um aviso que sempre dispara não avisa nada.

**Registro que mente por omissão.** Itens de checklist deixados como "não feito" quando na verdade foram **deliberadamente descartados** produzem a leitura oposta da verdade meses depois. Descartar é uma decisão; precisa ser registrada como decisão, não como pendência.

**Correção sem antígeno.** Corrigir um comportamento sem criar o critério que reprova o retorno dele significa corrigir uma vez. É o padrão mais comum e o mais silencioso.

**Falso diagnóstico por reflexo.** Um sintoma conhecido — um arquivo de trava, um cache corrompido — dispara a receita memorizada. Vale diagnosticar antes: uma trava que reaparece a cada tentativa mas não existe entre elas é **disputa concorrente**, não trava obsoleta, e a receita memorizada (remover o arquivo) seria a ação errada.

**Verificação com escopo enganoso.** Uma busca que exclui um diretório para responder a uma pergunta específica não pode ter seu resultado reaproveitado como total. Foi assim que uma estimativa de treze itens virou mais de trinta na execução.

## A regra da dupla escrita

O padrão por trás da maioria das pendências acumuladas:

> **Toda correção que muda comportamento observável termina em duas escritas, não uma:** o código, e o critério que reprova se o comportamento voltar.

E o corolário útil: **se escrever o segundo é caro demais para compensar, a correção provavelmente era cosmética** — o que também é informação que vale ter.

## Checklist reutilizável

**Antes de despachar uma etapa**
- [ ] A ordem veio do roteador, não da minha leitura do estado
- [ ] O escopo da etapa lista os arquivos que ela realmente vai tocar
- [ ] Achados das etapas anteriores estão no canal que alimenta este prompt

**Ao receber o retorno de um agente**
- [ ] Verifiquei por leitura própria toda afirmação que muda decisão — relato de agente é hipótese, não fato
- [ ] Preservei o trabalho antes de discutir o que fazer com ele
- [ ] Defeitos relatados fora do escopo viraram registro, não só texto de conversa

**Ao corrigir algo fora do plano**
- [ ] O desvio está declarado onde o guarda de fronteira lê, não só descrito em prosa
- [ ] Existe critério que reprova a regressão — ou está registrado por que não vale a pena
- [ ] Procurei o **mesmo padrão** em outros lugares: um defeito causado por padrão de código raramente é único

**Ao fechar uma etapa**
- [ ] Itens descartados estão marcados como decisão, não como pendência
- [ ] O checklist reflete a realidade que alguém sem contexto vai ler daqui a meses

**Ao pausar**
- [ ] O trabalho está publicado, não só local
- [ ] Onde ele vive está anotado em lugar que a próxima sessão lê de graça
- [ ] O próximo passo está escrito como **uma** ação, não como um menu

## Glossário

| Termo | O que significa aqui |
|---|---|
| **harness** | componente que decide ordem, materializa prompts, guarda estado e roda portões |
| **etapa / onda** | unidade de trabalho despachada a um agente, com escopo e critérios próprios |
| **roteador** | parte determinística que decide qual etapa vem agora |
| **guarda de fronteira** | verificação pré-edição contra o escopo declarado da etapa |
| **trilha** | registro append-only que alimenta o prompt das etapas seguintes |
| **portão de qualidade** | execução dos critérios de aceitação antes do fechamento |
| **marcador de aprovação** | arquivo-sentinela que impede execução sem decisão humana |

---

**A frase para levar:** a cobertura de verificação de qualquer harness é exatamente do tamanho dos critérios que você escreveu. Classes inteiras de defeito ficam de fora por **omissão sua**, não por falha da ferramenta — e é por isso que a disciplina de escrever o critério junto com a correção vale mais que qualquer recurso do harness.
