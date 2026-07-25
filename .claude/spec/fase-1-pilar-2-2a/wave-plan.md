---
id: wave.fase-1-pilar-2-2a.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.fase-1-pilar-2-2a.1-core]] | core | — | Janela de tempo no EconomyScope + filtro por ts nos readers de economia |
| 2 | [[wave.fase-1-pilar-2-2a.2-tauri]] | tauri | [[wave.fase-1-pilar-2-2a.1-core]] | EconomyScopeDto + os 6 comandos dashboard_economy_* repassam a janela ao core |
| 3 | [[wave.fase-1-pilar-2-2a.3-frontend]] | frontend | [[wave.fase-1-pilar-2-2a.2-tauri]] | Seletor de janela (1d/7d/15d/30d) na pagina Economia, compondo com o escopo |
