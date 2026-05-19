import i18n from "i18next";
import { initReactI18next } from "react-i18next";

i18n.use(initReactI18next).init({
  resources: {
    pt: {
      common: {
        "nav.home": "Home",
        "nav.activity": "Atividade",
        "nav.telemetry": "Telemetria",
        "nav.quality": "Qualidade",
        "nav.promptEconomy": "Prompt Economy",
        "nav.knowledge": "Knowledge",
        "nav.commands": "Comandos",
        "nav.prd": "PRD",
        "nav.settings": "Configurações",
        "group.workspace": "Workspace",
        "group.tools": "Ferramentas",
        "tooltip.selectWorkspace": "Selecione um workspace no topo",
      },
    },
    en: {
      common: {
        "nav.home": "Home",
        "nav.activity": "Activity",
        "nav.telemetry": "Telemetry",
        "nav.quality": "Quality",
        "nav.promptEconomy": "Prompt Economy",
        "nav.knowledge": "Knowledge",
        "nav.commands": "Commands",
        "nav.prd": "PRD",
        "nav.settings": "Settings",
        "group.workspace": "Workspace",
        "group.tools": "Tools",
        "tooltip.selectWorkspace": "Select a workspace at the top",
      },
    },
  },
  lng: "pt",
  fallbackLng: "pt",
  defaultNS: "common",
  interpolation: { escapeValue: false },
});

export function setLanguage(lng: "pt" | "en") {
  i18n.changeLanguage(lng);
}

export default i18n;
