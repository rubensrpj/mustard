# Medição A/B (gate de processo)

> Gate exigido pelo `## Limites` do `spec.md`: "medição A/B contra as consultas GRAVADAS (loop de outcome), com o resultado registrado neste spec — esta classe de truque de agregação no digest já resistiu a 4 tentativas e reverts". Registrado aqui como sidecar (o `spec.md` é narrativa congelada após approve).

Binário fresco (`target/debug/scan.exe`), modelo `.claude/grain.model.json`, idioma declarado do projeto (sem forçar `--lang en` — isso inflava splits com glue PT como "qual"/"para"). Correlação: cada `feature.outcome` atribuído à `feature.query` anterior na mesma sessão.

- Queries gravadas: 41 (14 sessões). Splits em ≥2 concerns: 22/41.
- Controle não-regressão: 19 queries single-concern → 0 emitiram `concerns` de 1 elemento; lista flat inalterada por construção. **0 regressões.**
- Queries multi-concern com outcomes correlacionados E ranqueáveis: N=8 (16 arquivos-alvo mensuráveis).
- **WINS (enterrado no flat → top-5 no seu concern): 1.** REGRESSÕES: 0.

O único win (Q34 "add a supplier to a card"): o flat top-12 foi dominado pelo concern `card` (9/12 = `*Card*.tsx`); `apps/scan/src/model.rs` (alvo do lado `contract`, lido pelo operador) caiu fora do flat top-12 mas surgiu em #5 no concern `contract`. Mecanismo confirmado.

Ressalvas que tornam o resultado **INCONCLUSIVO**: (a) 33/41 queries não têm outcomes capturados; (b) a única sessão rica (supplier/card) editava o PRÓPRIO tooling do Mustard, não o domínio consultado — ground-truth fraco; (c) a query de dogfood (Q40) teve ganho ZERO (todos os alvos no mesmo concern grande, e "occurrence" foi separado espuriamente).

**Antes/depois:** a feature NÃO regride; o ganho existe mas é 1 arquivo, insuficiente para provar valor sobre o corpus gravado. Bar da spec ("measurably helps") NÃO foi cleanly atingido — a feature sobrevive em "harmless + occasionally helps", não em "measurably helps".

**Defeito secundário a endereçar antes de confiar no split:** a aresta de co-ocorrência às vezes separa um conceito relacionado (Q40 "occurrence"); apertar a aresta antes de confiar na contagem de concerns.

## Reprodução
```
cargo build -p scan -p mustard-rt
target/debug/scan.exe digest .claude/grain.model.json --query "card,supplier,contract"
```