---
id: wave.harness-ve-toda-branch-trabalho.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.harness-ve-toda-branch-trabalho.1-enumerator]] | enumerator | — | Uma única fonte de verdade sobre branches: o enumerador varre refs locais e remotas por prefixo de base, o classificador cruza ancestralidade local com a consulta de PR atrás de uma porta, e o relatório sai por uma flag do ritual de saída — sem poder apagar nada. |
| 2 | [[wave.harness-ve-toda-branch-trabalho.2-surfacing]] | surfacing | [[wave.harness-ve-toda-branch-trabalho.1-enumerator]] | O harness passa a FALAR: o inventário de specs enxerga a branch que existe só no remoto, a statusline informa quantas unidades devem poda, o início de sessão avisa quando há pendência, e as duas varreduras antigas morrem convergindo no enumerador. |
