# Modelo de sentido embutido

Estes arquivos são o modelo estático que o Mustard carrega de dentro do próprio
programa (`packages/core/src/io/map_meaning.rs`), sem internet e sem serviço pago.

## Origem

- Modelo: `minishlab/potion-multilingual-128M`, um modelo estático do Model2Vec
  destilado do `BAAI/bge-m3`. Licença MIT, como os dois modelos de origem.
- Formato: uma tabela de vetores por pedaço de palavra. O vetor de um texto é a
  média dos vetores dos pedaços, normalizada. Não há rede neural na leitura.
- Arquivos: `model.safetensors` (vetores em int8, 256 números por pedaço),
  `tokenizer.json` (só os pedaços mantidos) e `config.json`.

## O que foi cortado

O `build_model.py` desta pasta gerou os arquivos a partir do modelo original:

1. Manteve os pedaços que aparecem nas palavras gerais das línguas (as listas de
   frequência do `wordfreq`) e num conjunto de pedaços de nomes de código que
   valem em qualquer projeto. O texto de um projeto nunca entra na poda.
2. Guardou os vetores em int8. A escala não importa, porque o vetor final é
   normalizado.
3. As 18 línguas do `normalize/languages/` deram 60 MB, acima do teto de 30 MB.
   O script então refaz só com português e inglês: 82.313 pedaços, 24 MB.

Para gerar de novo, com o modelo já baixado do Hugging Face:

```
python build_model.py <pasta de saída> [língua ...]
```

Trocar o modelo pede subir a versão do bloco `meaning` em `map_meaning.rs`, para
o próximo scan recalcular os vetores.
