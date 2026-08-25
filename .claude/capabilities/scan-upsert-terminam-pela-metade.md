---
id: cap.scan-upsert-terminam-pela-metade
status: active
---

# scan upsert terminam pela metade

### Requirement: The system SHALL satisfy the acceptance criteria of spec scan-upsert-terminam-pela-metade.

#### Scenario: AC-1
- when: o modelo tem `projects[]` só com a unidade-raiz e `skeleton[]` com casas
- then: a resolução de dono usa as casas do esqueleto e a worklist sai com `subproject` real (`src/sira`) e `moldPath` sob `src/sira/.claude/skills/`
- command: `cargo test -p mustard-rt skeleton_houses_own_clusters_when_no_manifest_unit_exists 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-2
- when: o modelo tem ao menos uma unidade de manifesto com `dir` não-vazio
- then: o esqueleto não é consultado e a saída é a de hoje
- command: `cargo test -p mustard-rt skeleton_fallback_stays_out_when_manifest_units_exist 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-3
- when: não há unidade de manifesto E o modelo não traz `skeleton[]` (modelo antigo)
- then: a worklist é `[]` e o comando sai 0
- command: `cargo test -p mustard-rt no_skeleton_degrades_to_empty_worklist 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-4
- when: `init` roda sobre uma árvore de git limpa e o selo de versão muda
- then: ao fim da execução a árvore está limpa de novo, sem ação do operador
- command: `cargo test -p mustard-cli install_leaves_the_git_tree_clean 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-5
- when: o plugin carregado está atrás da versão registrada em `installed_plugins.json`
- then: o início de sessão diz em UMA linha que a sessão roda prosa antiga e que recarregar é preciso
- command: `cargo test -p mustard-rt stale_plugin_is_announced_at_session_start 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-6
- when: 
- then: o build do workspace passa verde
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.scan-upsert-terminam-pela-metade]]

## Related

