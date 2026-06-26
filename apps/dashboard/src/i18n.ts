// SPEC LANG: pt-allowed — i18next translation catalogue. pt-BR strings are data, not narrative.
import i18n from "i18next";
import { initReactI18next } from "react-i18next";

// BCP-47 locale keys (see memory `project_locale_codes`).
i18n.use(initReactI18next).init({
  resources: {
    "pt-BR": {
      common: {
        "nav.home": "Home",
        "nav.workspace": "Visão Geral",
        "nav.specs": "Specs",
        "nav.economy": "Economia",
        "nav.activity": "Atividade",
        "nav.telemetry": "Telemetria",
        "nav.quality": "Qualidade",
        "nav.promptEconomy": "Prompt Economy",
        "nav.knowledge": "Knowledge",
        "nav.commands": "Comandos",
        "nav.settings": "Configurações",
        "nav.preferences": "Preferences",
        "group.workspace": "Workspace",
        "group.tools": "Ferramentas",
        "tooltip.selectWorkspace": "Selecione um workspace no topo",
        "sidebar.addProject": "Adicionar projeto",
        "sidebar.addProjectDesktopOnly": "Disponível no app desktop",
        "sidebar.tools": "Ferramentas",
        "sidebar.empty.title": "Nenhum projeto",
        "sidebar.empty.description": "Adicione uma pasta para começar",
        "sidebar.projectMenu.update": "Atualizar",
        "sidebar.projectMenu.uninstall": "Remover Mustard",
        "sidebar.projectMenu.removeFromRegistry": "Remover do registry",
        "sidebar.status.installed": "Instalado",
        "sidebar.status.updateAvailable": "Atualização disponível",
        "sidebar.status.notInstalled": "Não instalado",
        "sidebar.status.checking": "Verificando…",
        "sidebar.status.versionUnknown": "Instalado · versão desconhecida",
        "projects.statusChecking": "Verificando…",
        "projects.statusNotInstalled": "Não instalado",
        "projects.statusUpdateAvailable": "Atualização disponível",
        "projects.statusInstalled": "Instalado",
        "projects.actionInstall": "Instalar",
        "projects.actionUpdate": "Atualizar",
        "projects.actionRemove": "Remover {{name}}",
        "projects.confirmCancel": "Cancelar",
        "projects.confirmDelete": "Confirmar",
        "projects.addDialogTitle": "Selecionar pasta do projeto",
        "projects.toastInstalled": "Mustard instalado em {{name}}",
        "projects.toastUpdated": "{{name}} atualizado",
        "projects.toastUninstalled": "Mustard removido de {{name}}",
        "projects.toastActionFailed": "Falha: {{msg}}",
        // Artifact drift (B6 Wave 3). `staleCount` uses i18next's `_one` /
        // `_other` plural suffix; the badge is only rendered when count > 0
        // so the zero case is unused but kept for symmetry.
        "artifact.staleCount_one": "{{count}} artefato defasado",
        "artifact.staleCount_other": "{{count}} artefatos defasados",
        "artifact.updateAction": "Atualizar artefatos",
        "artifact.updateRunning": "Atualizando artefatos…",
        "artifact.updateSuccess_one": "{{count}} artefato atualizado",
        "artifact.updateSuccess_other": "{{count}} artefatos atualizados",
        "artifact.updateError": "Erro ao atualizar artefatos: {{msg}}",
        "settings.title": "Settings",
        "settings.envTitle": "Environment",
        "settings.envDescriptionBefore": "Variáveis de ambiente do Mustard para o projeto selecionado. Gravadas em ",
        "settings.envDescriptionAfter": ". Cada entrada mostra o título legível em cima e o nome técnico da variável logo abaixo.",
        "settings.envSelectProject": "Selecione um projeto na sidebar para editar variáveis MUSTARD_*.",
        "settings.envHelpTextBefore": "Variáveis ",
        "settings.envHelpTextSep": " / ",
        "settings.envHelpTextAfter": " persistidas em ",
        "settings.envHelpTextEnd": ".",
        "settings.envPending": "pendentes",
        "settings.saveChanges": "Salvar mudanças",
        "settings.discardChanges": "Descartar",
        "preferences.title": "Preferences",
        "preferences.description": "Configurações globais do dashboard",
        "preferences.language": "Idioma",
        "preferences.languagePt": "Português",
        "preferences.languageEn": "Inglês",
        // KPI tooltip footer label that the `KPICard` primitive renders above
        // every `caption` block. Kept outside the `economy.*` namespace so any
        // future KPI consumer in another page can reuse the same key.
        "kpi.captionLabel": "o que isso significa",
        // ── Economia page (spec 2026-05-23-economia-i18n-migration) ──
        // Empty / config states reused from the main `Economia` page.
        "economy.empty.noRoot.title": "Diretório de projetos não configurado",
        "economy.empty.noRoot.description":
          "Vá em Configurações e aponte para a pasta onde estão seus repos.",
        "economy.empty.noWorkspace.title": "Selecione um workspace",
        "economy.empty.noWorkspace.description":
          "Use o seletor na sidebar para escolher um projeto.",
        // KPI: project cost (measured).
        "economy.kpi.cost.label": "Custo do projeto (medido)",
        "economy.kpi.cost.hint":
          "{{dispatches}} execuções · {{tokens}} tokens",
        "economy.kpi.cost.caption": "cobrado pela Anthropic, somado por sessão",
        "economy.kpi.cost.statusLive": "ao vivo",
        "economy.kpi.cost.statusStale": "parado",
        "economy.kpi.cost.statusOff": "desligado",
        "economy.kpi.cost.updatedAgo": "atualizado {{ago}}",
        // KPI: total savings (tokens).
        "economy.kpi.savings.label": "Economia total (tokens)",
        "economy.kpi.savings.hint": "abaixo, o detalhe por origem",
        "economy.kpi.savings.caption":
          "tokens que a ferramenta evitou de gastar — abaixo, o detalhe por origem",
        // KPI: cache hit ratio.
        "economy.kpi.cache.label": "Cache hit",
        "economy.kpi.cache.caption":
          "tokens servidos do cache ÷ (cache + escrita no cache + input novo). Acima de 80% é ótimo — a Anthropic cobra só 10% do preço normal nesses tokens.",
        "economy.kpi.cache.tier.optimal": "ótimo · cache funcionando",
        "economy.kpi.cache.tier.warm": "morno · prefixo mudando",
        "economy.kpi.cache.tier.cold": "frio · pouco reuso",
        "economy.kpi.cache.tier.empty": "sem reuso medido",
        "economy.kpi.cache.noData": "sem dados nesta janela",
        "economy.kpi.cache.collapseWave":
          "ⓘ no filtro de Wave, o número é da spec inteira — o cache da Anthropic não distingue waves dentro de uma mesma spec.",
        // Stale banner: estimated path froze vs measured path.
        "economy.staleBanner.label_hours_one": "{{count}} hora",
        "economy.staleBanner.label_hours_other": "{{count}} horas",
        "economy.staleBanner.label_days_one": "{{count}} dia",
        "economy.staleBanner.label_days_other": "{{count}} dias",
        "economy.staleBanner.title":
          "A tabela de custo estimado por spec/onda parou de receber dados há {{label}}.",
        "economy.staleBanner.bodyBefore":
          'O custo medido (do card "Custo do projeto") continua atualizado — só a quebra por feature está congelada. Para retomar a estimação por spec: verifique que o collector do ',
        "economy.staleBanner.bodyBetween": " está rodando e que o Claude Code está exportando OTEL via ",
        "economy.staleBanner.bodyAfter": ".",
        // Sections.
        "economy.byAgent.title_one": "Por agente (top {{count}})",
        "economy.byAgent.title_other": "Por agente (top {{count}})",
        "economy.byAgent.titleFallback": "Por agente",
        "economy.byAgent.caption":
          "agentes que mais consumiram tokens nesta janela",
        // Humanized agent labels — fallback to raw id when the key is missing.
        // Listed alphabetically so adding a new role stays a one-line change.
        "economy.agents.core-impl": "Núcleo (biblioteca)",
        "economy.agents.core-explorer": "Núcleo (exploração)",
        "economy.agents.rt-impl": "Runtime",
        "economy.agents.rt-explorer": "Runtime (exploração)",
        "economy.agents.dashboard-impl": "Dashboard",
        "economy.agents.dashboard-explorer": "Dashboard (exploração)",
        "economy.agents.cli-impl": "CLI",
        "economy.agents.cli-explorer": "CLI (exploração)",
        "economy.agents.templates-impl": "Templates",
        "economy.agents.templates-explorer": "Templates (exploração)",
        "economy.agents.general-purpose": "Geral",
        "economy.agents.Explore": "Explorador",
        "economy.agents.Plan": "Planejador",
        // Aggregate rows below the top-3.
        "economy.byAgent.others_one": "Outros ({{count}} agente)",
        "economy.byAgent.others_other": "Outros ({{count}} agentes)",
        "economy.byAgent.total": "Total estimado",
        "economy.byAgent.matchMeasured": "≈ medido {{cost}}",
        // Inline badges used by the per-spec table and per-session rows.
        "economy.estimated.noWaveBadge": "sem onda",
        "economy.bySession.noSpecChip": "sem spec",
        "economy.bySession.title": "Por sessão",
        "economy.bySession.captionBefore":
          "uma linha por sessão do Claude Code — compare o custo com ",
        "economy.bySession.captionAfter": " para conferir",
        "economy.bySession.unavailable.title": "Não disponível neste filtro",
        "economy.bySession.unavailable.description":
          "A Anthropic atribui custo medido só por sessão — sessão não tem dimensão de spec nem onda. Para ver as sessões, volte ao filtro Projeto.",
        "economy.bySession.empty.title": "Sem sessões registradas",
        "economy.bySession.empty.description":
          "As sessões aparecem aqui depois que o Claude Code rodar com telemetria ligada.",
        "economy.bySession.noSpec": "sem spec registrada",
        // Summary error wrapper.
        "economy.summaryError.title": "Falha ao ler os dados de economia",
        // Savings section.
        "economy.savings.title": "O que a ferramenta evitou de gastar",
        "economy.savings.caption":
          "cada linha é uma estratégia que poupa tokens",
        "economy.savings.total_one":
          "total: {{tokens}} tok · {{count}} ocorrência",
        "economy.savings.total_other":
          "total: {{tokens}} tok · {{count}} ocorrências",
        "economy.savings.estimatedSuffix": "(estimado)",
        "economy.savings.source.rtk_rewrite": "Reescrita de comando shell",
        "economy.savings.source.rtk_rewrite.hint":
          "encurtou o comando antes de rodar, mantendo o resultado",
        "economy.savings.source.model_routing_downgrade":
          "Modelo mais barato quando seguro",
        "economy.savings.source.model_routing_downgrade.hint":
          "trocou para um modelo mais barato quando a tarefa permitia",
        "economy.savings.source.bash_guard_block":
          "Comando bloqueado por segurança",
        "economy.savings.source.bash_guard_block.hint":
          "barrou um comando destrutivo ou ruidoso antes da execução",
        "economy.savings.source.budget_output_cut":
          "Resposta cortada por orçamento",
        "economy.savings.source.budget_output_cut.hint":
          "cortou uma resposta muito longa antes de devolver ao pai",
        // Estimated per spec/wave table.
        "economy.estimated.title": "Custo estimado por spec / onda",
        "economy.estimated.badge": "estimado",
        "economy.estimated.caption":
          "soma do custo de cada execução atribuída à spec — uma estimativa interna por dispatch, útil para comparar features. Não é o valor cobrado pela Anthropic.",
        "economy.estimated.loading": "carregando…",
        "economy.estimated.empty.title": "Sem execuções atribuídas neste escopo",
        "economy.estimated.empty.description":
          "As linhas aparecem aqui assim que dispatches forem registrados com a spec correspondente.",
        "economy.estimated.noWaveAttributed": "(sem onda atribuída)",
        "economy.estimated.unattributed_one":
          "{{count}} execução sem spec registrada — não aparecem na tabela porque não dá pra atribuir a uma feature específica.",
        "economy.estimated.unattributed_other":
          "{{count}} execuções sem spec registrada — não aparecem na tabela porque não dá pra atribuir a uma feature específica.",
        "economy.estimated.col.specWave": "Spec / onda",
        "economy.estimated.col.dispatches": "Execuções",
        "economy.estimated.col.tokens": "Tokens",
        "economy.estimated.col.cost": "Custo",
        // Per-agent table headers.
        "economy.table.agent": "Agente",
        "economy.table.dispatches": "Execuções",
        "economy.table.tokens": "Tokens",
        "economy.table.cost": "Custo",
        "economy.table.empty": "Nenhum agente custou nada neste escopo ainda.",
        // ScopeBar tabs + dropdown labels.
        "economy.scope.project": "Projeto",
        "economy.scope.spec": "Spec",
        "economy.scope.wave": "Wave",
        "economy.scope.compare": "Comparar projetos",
        "economy.scope.specLabel": "Spec:",
        "economy.scope.waveLabel": "Wave:",
        "economy.scope.compareLabel": "Projetos a comparar:",
        "economy.scope.selectPlaceholder": "— selecione —",
        "economy.scope.noProjects": "Nenhum projeto descoberto.",
      },
    },
    "en-US": {
      common: {
        "nav.home": "Home",
        "nav.workspace": "Overview",
        "nav.specs": "Specs",
        "nav.economy": "Economy",
        "nav.activity": "Activity",
        "nav.telemetry": "Telemetry",
        "nav.quality": "Quality",
        "nav.promptEconomy": "Prompt Economy",
        "nav.knowledge": "Knowledge",
        "nav.commands": "Commands",
        "nav.settings": "Settings",
        "nav.preferences": "Preferences",
        "group.workspace": "Workspace",
        "group.tools": "Tools",
        "tooltip.selectWorkspace": "Select a workspace at the top",
        "sidebar.addProject": "Add project",
        "sidebar.addProjectDesktopOnly": "Available in desktop app",
        "sidebar.tools": "Tools",
        "sidebar.empty.title": "No projects",
        "sidebar.empty.description": "Add a folder to get started",
        "sidebar.projectMenu.update": "Update",
        "sidebar.projectMenu.uninstall": "Uninstall Mustard",
        "sidebar.projectMenu.removeFromRegistry": "Remove from registry",
        "sidebar.status.installed": "Installed",
        "sidebar.status.updateAvailable": "Update available",
        "sidebar.status.notInstalled": "Not installed",
        "sidebar.status.checking": "Checking…",
        "sidebar.status.versionUnknown": "Installed · version unknown",
        "projects.statusChecking": "Checking…",
        "projects.statusNotInstalled": "Not installed",
        "projects.statusUpdateAvailable": "Update available",
        "projects.statusInstalled": "Installed",
        "projects.actionInstall": "Install",
        "projects.actionUpdate": "Update",
        "projects.actionRemove": "Remove {{name}}",
        "projects.confirmCancel": "Cancel",
        "projects.confirmDelete": "Confirm",
        "projects.addDialogTitle": "Select project folder",
        "projects.toastInstalled": "Mustard installed in {{name}}",
        "projects.toastUpdated": "{{name}} updated",
        "projects.toastUninstalled": "Mustard removed from {{name}}",
        "projects.toastActionFailed": "Failed: {{msg}}",
        // Artifact drift (B6 Wave 3) — see the PT block for plural handling.
        "artifact.staleCount_one": "{{count}} stale artifact",
        "artifact.staleCount_other": "{{count}} stale artifacts",
        "artifact.updateAction": "Update artifacts",
        "artifact.updateRunning": "Updating artifacts…",
        "artifact.updateSuccess_one": "{{count}} artifact updated",
        "artifact.updateSuccess_other": "{{count}} artifacts updated",
        "artifact.updateError": "Failed to update artifacts: {{msg}}",
        "settings.title": "Settings",
        "settings.envTitle": "Environment",
        "settings.envDescriptionBefore": "Mustard environment variables for the selected project. Persisted to ",
        "settings.envDescriptionAfter": ". Each row shows the readable title on top and the technical variable name right below.",
        "settings.envSelectProject": "Select a project from the sidebar to edit MUSTARD_* variables.",
        "settings.envHelpTextBefore": "",
        "settings.envHelpTextSep": " / ",
        "settings.envHelpTextAfter": " variables persisted in ",
        "settings.envHelpTextEnd": ".",
        "settings.envPending": "pending",
        "settings.saveChanges": "Save changes",
        "settings.discardChanges": "Discard",
        "preferences.title": "Preferences",
        "preferences.description": "Dashboard-wide settings",
        "preferences.language": "Language",
        "preferences.languagePt": "Portuguese",
        "preferences.languageEn": "English",
        "kpi.captionLabel": "what this means",
        // ── Economy page (spec 2026-05-23-economia-i18n-migration) ──
        "economy.empty.noRoot.title": "Projects directory not configured",
        "economy.empty.noRoot.description":
          "Go to Settings and point Mustard at the folder where your repos live.",
        "economy.empty.noWorkspace.title": "Select a workspace",
        "economy.empty.noWorkspace.description":
          "Use the sidebar picker to choose a project.",
        "economy.kpi.cost.label": "Project cost (measured)",
        "economy.kpi.cost.hint":
          "{{dispatches}} dispatches · {{tokens}} tokens",
        "economy.kpi.cost.caption": "billed by Anthropic, totalled per session",
        "economy.kpi.cost.statusLive": "live",
        "economy.kpi.cost.statusStale": "stalled",
        "economy.kpi.cost.statusOff": "off",
        "economy.kpi.cost.updatedAgo": "updated {{ago}}",
        "economy.kpi.savings.label": "Total savings (tokens)",
        "economy.kpi.savings.hint": "see the per-source breakdown below",
        "economy.kpi.savings.caption":
          "tokens the tool avoided spending — see the per-source breakdown below",
        "economy.kpi.cache.label": "Cache hit",
        "economy.kpi.cache.caption":
          "cache reads ÷ (cache reads + cache writes + fresh input). Above 80% is great — Anthropic only charges 10% of the base price on those tokens.",
        "economy.kpi.cache.tier.optimal": "great · cache working",
        "economy.kpi.cache.tier.warm": "warm · prefix drifting",
        "economy.kpi.cache.tier.cold": "cold · little reuse",
        "economy.kpi.cache.tier.empty": "no reuse measured",
        "economy.kpi.cache.noData": "no data in this window",
        "economy.kpi.cache.collapseWave":
          "ⓘ under the Wave filter, the number is for the whole spec — Anthropic's cache doesn't distinguish waves inside the same spec.",
        "economy.staleBanner.label_hours_one": "{{count}} hour",
        "economy.staleBanner.label_hours_other": "{{count}} hours",
        "economy.staleBanner.label_days_one": "{{count}} day",
        "economy.staleBanner.label_days_other": "{{count}} days",
        "economy.staleBanner.title":
          "The estimated cost-per-spec/wave table stopped receiving data {{label}} ago.",
        "economy.staleBanner.bodyBefore":
          'The measured cost (from the "Project cost" card) is still up to date — only the per-feature breakdown is frozen. To resume per-spec estimation: check that the ',
        "economy.staleBanner.bodyBetween":
          " collector is running and that Claude Code is exporting OTEL via ",
        "economy.staleBanner.bodyAfter": ".",
        "economy.byAgent.title_one": "By agent (top {{count}})",
        "economy.byAgent.title_other": "By agent (top {{count}})",
        "economy.byAgent.titleFallback": "By agent",
        "economy.byAgent.caption":
          "agents that consumed the most tokens in this window",
        // Humanized agent labels — fallback to raw id when the key is missing.
        "economy.agents.core-impl": "Core (library)",
        "economy.agents.core-explorer": "Core (exploration)",
        "economy.agents.rt-impl": "Runtime",
        "economy.agents.rt-explorer": "Runtime (exploration)",
        "economy.agents.dashboard-impl": "Dashboard",
        "economy.agents.dashboard-explorer": "Dashboard (exploration)",
        "economy.agents.cli-impl": "CLI",
        "economy.agents.cli-explorer": "CLI (exploration)",
        "economy.agents.templates-impl": "Templates",
        "economy.agents.templates-explorer": "Templates (exploration)",
        "economy.agents.general-purpose": "General",
        "economy.agents.Explore": "Explorer",
        "economy.agents.Plan": "Planner",
        // Aggregate rows below the top-3.
        "economy.byAgent.others_one": "Others ({{count}} agent)",
        "economy.byAgent.others_other": "Others ({{count}} agents)",
        "economy.byAgent.total": "Estimated total",
        "economy.byAgent.matchMeasured": "≈ measured {{cost}}",
        // Inline badges used by the per-spec table and per-session rows.
        "economy.estimated.noWaveBadge": "no wave",
        "economy.bySession.noSpecChip": "no spec",
        "economy.bySession.title": "By session",
        "economy.bySession.captionBefore":
          "one row per Claude Code session — cross-check the cost against ",
        "economy.bySession.captionAfter": " to confirm",
        "economy.bySession.unavailable.title": "Not available under this filter",
        "economy.bySession.unavailable.description":
          "Anthropic only attributes measured cost per session — and sessions don't have a spec or wave dimension. To see sessions, switch back to the Project filter.",
        "economy.bySession.empty.title": "No sessions recorded",
        "economy.bySession.empty.description":
          "Sessions show up here after Claude Code runs with telemetry enabled.",
        "economy.bySession.noSpec": "no spec recorded",
        "economy.summaryError.title": "Failed to read economy data",
        "economy.savings.title": "What the tool avoided spending",
        "economy.savings.caption":
          "each row is a strategy that saves tokens",
        "economy.savings.total_one":
          "total: {{tokens}} tok · {{count}} occurrence",
        "economy.savings.total_other":
          "total: {{tokens}} tok · {{count}} occurrences",
        "economy.savings.estimatedSuffix": "(estimated)",
        "economy.savings.source.rtk_rewrite": "Shell command rewrite",
        "economy.savings.source.rtk_rewrite.hint":
          "shortened the command before running it, keeping the result",
        "economy.savings.source.model_routing_downgrade":
          "Cheaper model when safe",
        "economy.savings.source.model_routing_downgrade.hint":
          "switched to a cheaper model when the task allowed it",
        "economy.savings.source.bash_guard_block":
          "Command blocked for safety",
        "economy.savings.source.bash_guard_block.hint":
          "stopped a destructive or noisy command before execution",
        "economy.savings.source.budget_output_cut":
          "Response trimmed by budget",
        "economy.savings.source.budget_output_cut.hint":
          "trimmed an overly long response before returning to the parent",
        "economy.estimated.title": "Estimated cost per spec / wave",
        "economy.estimated.badge": "estimated",
        "economy.estimated.caption":
          "sum of the cost of every dispatch attributed to the spec — an internal per-dispatch estimate, useful to compare features. Not the amount billed by Anthropic.",
        "economy.estimated.loading": "loading…",
        "economy.estimated.empty.title": "No attributed dispatches in this scope",
        "economy.estimated.empty.description":
          "Rows appear here as soon as dispatches are recorded with the matching spec.",
        "economy.estimated.noWaveAttributed": "(no wave attributed)",
        "economy.estimated.unattributed_one":
          "{{count}} dispatch with no spec recorded — not shown in the table because it can't be attributed to a specific feature.",
        "economy.estimated.unattributed_other":
          "{{count}} dispatches with no spec recorded — not shown in the table because they can't be attributed to a specific feature.",
        "economy.estimated.col.specWave": "Spec / wave",
        "economy.estimated.col.dispatches": "Dispatches",
        "economy.estimated.col.tokens": "Tokens",
        "economy.estimated.col.cost": "Cost",
        "economy.table.agent": "Agent",
        "economy.table.dispatches": "Dispatches",
        "economy.table.tokens": "Tokens",
        "economy.table.cost": "Cost",
        "economy.table.empty": "No agent has spent anything in this scope yet.",
        "economy.scope.project": "Project",
        "economy.scope.spec": "Spec",
        "economy.scope.wave": "Wave",
        "economy.scope.compare": "Compare projects",
        "economy.scope.specLabel": "Spec:",
        "economy.scope.waveLabel": "Wave:",
        "economy.scope.compareLabel": "Projects to compare:",
        "economy.scope.selectPlaceholder": "— select —",
        "economy.scope.noProjects": "No projects discovered.",
      },
    },
  },
  lng: "pt-BR",
  fallbackLng: "pt-BR",
  defaultNS: "common",
  interpolation: { escapeValue: false },
});

export function setLanguage(lng: "pt-BR" | "en-US") {
  i18n.changeLanguage(lng);
}

// Keep `<html lang>` in sync with the active language so screen readers,
// browser spellcheck and `:lang(pt)` CSS selectors all stay coherent. We
// listen on i18next's `languageChanged` instead of plumbing into zustand
// directly because `store.ts::setLanguage` already calls
// `i18n.changeLanguage(...)`, so this single hook covers every code path
// (interactive Preferences toggle, persisted-state rehydration, programmatic
// calls). Guarded for SSR/test envs where `document` is undefined.
if (typeof document !== "undefined") {
  const apply = (lng: string) => {
    document.documentElement.lang = lng;
  };
  apply(i18n.language);
  i18n.on("languageChanged", apply);
}

export default i18n;
