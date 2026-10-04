//! O registro dos ganchos: qual gancho roda em qual evento e em qual
//! ferramenta.
//!
//! Acrescentar um gancho é só registrar um [`Module`] aqui; o despachante lê o
//! registro e não muda. Cada gancho diz os pares `(Trigger, ToolMatch)` em que
//! roda, e uma chamada que não casa com nenhum deles nem o executa.
//!
//! São quatorze, e só eles: a trava de comandos, o portão de escrita, o pedido
//! do subagente, a testemunha da aprovação, a testemunha da cópia da página,
//! a entrada da mensagem, o início da sessão, o conserto da barra de status,
//! o sinal de vida da onda, a testemunha do glossário do mapa, o aviso antes
//! de compactar, o aviso de tamanho da conversa, que também recusa o agente
//! de onda que gastou a folga depois do limite, a faxina do fim da sessão e
//! a conferência do fim da resposta.

use crate::hooks::bash::command_guard::CommandGuard;
use crate::hooks::observe::approval_witness::ApprovalWitness;
use crate::hooks::observe::copy_witness::CopyWitness;
use crate::hooks::observe::glossary_witness::GlossaryWitness;
use crate::hooks::observe::wave_alive_observer::WaveAliveObserver;
use crate::hooks::session::conversation_size::{PrecompactNotice, SizeNotice};
use crate::hooks::session::prompt_entry::PromptEntry;
use crate::hooks::session::session_cleanup_observer::SessionCleanupObserver;
use crate::hooks::session::session_start_inject::SessionStartInject;
use crate::hooks::session::statusline_heal_observer::StatuslineHealObserver;
use crate::hooks::task::end_of_turn_check::EndOfTurnCheck;
use crate::hooks::task::subagent_inject::SubagentInject;
use crate::hooks::write::write_gate::WriteGate;
use mustard_core::domain::model::contract::{Check, Observer, Trigger};
use mustard_core::platform::project_seed::PAGE_DATABASE_TOOL;

/// Em que ferramenta uma entrada do registro roda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMatch {
    /// Toda ferramenta, e também os eventos que não têm ferramenta.
    Any,
    /// Uma ferramenta.
    Named(&'static str),
    /// Qualquer uma destas ferramentas.
    OneOf(&'static [&'static str]),
}

impl ToolMatch {
    /// `true` quando a entrada vale para uma chamada da ferramenta `tool`.
    #[must_use]
    fn matches(self, tool: Option<&str>) -> bool {
        match self {
            Self::Any => true,
            Self::Named(name) => tool == Some(name),
            Self::OneOf(names) => tool.is_some_and(|tool| names.contains(&tool)),
        }
    }
}

/// Um gancho: um `Check`, um `Observer`, ou os dois.
pub struct Module {
    /// O nome do gancho, que o despachante grava quando ele age.
    pub id: &'static str,
    /// Os pares `(Trigger, ToolMatch)` em que o gancho roda.
    pub applies_to: &'static [(Trigger, ToolMatch)],
    /// O que o gancho decide; `None` num gancho que só observa.
    pub check: Option<Box<dyn Check>>,
    /// O que o gancho faz sem decidir nada; `None` num gancho que só decide.
    pub observer: Option<Box<dyn Observer>>,
}

impl Module {
    /// `true` quando o gancho roda neste evento e nesta ferramenta.
    #[must_use]
    pub fn matches(&self, trigger: Trigger, tool: Option<&str>) -> bool {
        self.applies_to.iter().any(|(t, want_tool)| *t == trigger && want_tool.matches(tool))
    }
}

/// As ferramentas que escrevem, leem ou buscam arquivo, que o portão de
/// escrita confere: a busca por palavra (`Grep`) e a busca por nome
/// (`Glob`).
const FILE_TOOLS: &[&str] = &["Read", "Write", "Edit", "MultiEdit", "NotebookEdit", "Grep", "Glob"];

/// As ferramentas que editam arquivo de texto, cuja edição ensina o
/// glossário do mapa.
const EDIT_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit"];

/// As ferramentas que despacham um subagente.
const AGENT_TOOLS: &[&str] = &["Task", "Agent"];

/// Os ganchos registrados.
pub struct Registry {
    modules: Vec<Module>,
}

impl Registry {
    /// O registro com os ganchos que o Mustard entrega.
    #[must_use]
    pub fn new() -> Self {
        let modules = vec![
            // A trava de comandos: recusa o comando que destrói trabalho e
            // corrige, sem recusar, o redirecionamento para um caminho do
            // Windows.
            Module {
                id: "command_guard",
                applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Bash"))],
                check: Some(Box::new(CommandGuard)),
                observer: None,
            },
            // O portão de escrita, nas cinco ferramentas de arquivo e nas
            // duas buscas (por palavra e por nome). As regras, em ordem: segredo, a chave do Jev, arquivos
            // que só o binário escreve, aprovação, a branch da spec (só
            // aviso), a base do `git.flow`, a leitura inteira grande e a
            // leitura cortada. A primeira que responde decide.
            Module {
                id: "write_gate",
                applies_to: &[(Trigger::PreToolUse, ToolMatch::OneOf(FILE_TOOLS))],
                check: Some(Box::new(WriteGate)),
                observer: None,
            },
            // O pedido do subagente, no despacho de um agente: troca o bilhete
            // da onda pelo pedido montado, ou barra com o motivo.
            Module {
                id: "subagent_inject",
                applies_to: &[(Trigger::PreToolUse, ToolMatch::OneOf(AGENT_TOOLS))],
                check: Some(Box::new(SubagentInject)),
                observer: None,
            },
            // A testemunha da aprovação: na resposta à pergunta com opções,
            // grava a resposta na conversa e, quando é o "Aprovar" da
            // pergunta de aprovação com a spec em plano, a aprovação. Nunca
            // barra: devolve `Inject` para falar com o assistente.
            Module {
                id: "approval_witness",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Named("AskUserQuestion"))],
                check: Some(Box::new(ApprovalWitness)),
                observer: None,
            },
            // A testemunha da cópia: no resultado de um lote mandado ao banco
            // da página da spec, guarda as versões que o banco devolveu e,
            // quando o último lote volta, grava a cópia. Nunca barra.
            Module {
                id: "copy_witness",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Named(PAGE_DATABASE_TOOL))],
                check: Some(Box::new(CopyWitness)),
                observer: None,
            },
            // A entrada da mensagem, numa chamada só: a trava de instalação,
            // a mensagem gravada e a correção da escrita, quando houver.
            Module {
                id: "prompt_entry",
                applies_to: &[(Trigger::UserPromptSubmit, ToolMatch::Any)],
                check: Some(Box::new(PromptEntry)),
                observer: None,
            },
            // O início da sessão, que roda de novo depois de `/clear` e da
            // compactação.
            Module {
                id: "session_start_inject",
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: Some(Box::new(SessionStartInject)),
                observer: None,
            },
            // O conserto da barra de status no início da sessão.
            Module {
                id: "statusline_heal_observer",
                applies_to: &[(Trigger::SessionStart, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(StatuslineHealObserver)),
            },
            // O sinal de vida da onda: depois de cada ferramenta, grava a
            // hora da última ação quando a pasta de trabalho é a cópia de uma
            // onda. Nunca barra.
            Module {
                id: "wave_alive_observer",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(WaveAliveObserver)),
            },
            // A testemunha do glossário do mapa: depois de cada edição, a
            // declaração mudada que a última busca da sessão entregou ganha a
            // marca das palavras da pergunta. Nunca barra.
            Module {
                id: "glossary_witness",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::OneOf(EDIT_TOOLS))],
                check: None,
                observer: Some(Box::new(GlossaryWitness)),
            },
            // O aviso antes de compactar: em toda compactação, manual ou
            // automática, injeta o bloco de retomada pronto para colar.
            Module {
                id: "precompact_notice",
                applies_to: &[(Trigger::PreCompact, ToolMatch::Any)],
                check: Some(Box::new(PrecompactNotice)),
                observer: None,
            },
            // O tamanho da conversa nos dois lados de cada ferramenta. Depois
            // dela: a quem conduz, o aviso de limpar ou compactar; ao agente
            // de onda, o de parar no limite. Antes dela: só o agente de onda
            // que gastou a folga depois desse aviso é recusado, menos para
            // gravar na spec e compilar.
            Module {
                id: "size_notice",
                applies_to: &[(Trigger::PostToolUse, ToolMatch::Any), (Trigger::PreToolUse, ToolMatch::Any)],
                check: Some(Box::new(SizeNotice)),
                observer: None,
            },
            // A faxina do fim da sessão.
            Module {
                id: "session_cleanup_observer",
                applies_to: &[(Trigger::SessionEnd, ToolMatch::Any)],
                check: None,
                observer: Some(Box::new(SessionCleanupObserver)),
            },
            // A conferência do fim da resposta, o único gancho do `Stop`: a
            // regra das pendências bloqueia; a de clareza só bloqueia a
            // resposta fora do idioma do projeto, e os outros erros ela grava
            // para a mensagem seguinte.
            Module {
                id: "end_of_turn_check",
                applies_to: &[(Trigger::Stop, ToolMatch::Any)],
                check: Some(Box::new(EndOfTurnCheck)),
                observer: None,
            },
        ];
        Self { modules }
    }

    /// Os ganchos que rodam neste evento e nesta ferramenta, na ordem do
    /// registro.
    #[must_use]
    pub fn applicable(&self, trigger: Trigger, tool: Option<&str>) -> Vec<&Module> {
        self.modules.iter().filter(|m| m.matches(trigger, tool)).collect()
    }

    /// Os nomes dos ganchos registrados, na ordem do registro. Quem lê é o
    /// teste que confere o registro contra o manifesto do Claude Code.
    #[must_use]
    #[allow(dead_code)]
    pub fn ids(&self) -> Vec<&'static str> {
        self.modules.iter().map(|m| m.id).collect()
    }

    /// O gancho de nome `id`, em qualquer evento.
    #[cfg(test)]
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.id == id)
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os nomes dos ganchos que rodam neste evento e nesta ferramenta.
    fn applicable_ids(registry: &Registry, trigger: Trigger, tool: Option<&str>) -> Vec<&'static str> {
        registry.applicable(trigger, tool).iter().map(|m| m.id).collect()
    }

    /// O registro tem os quatorze ganchos que ficam, e só eles.
    #[test]
    fn the_registry_holds_exactly_the_fourteen_hooks() {
        let registry = Registry::new();
        let mut ids = registry.ids();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                "approval_witness",
                "command_guard",
                "copy_witness",
                "end_of_turn_check",
                "glossary_witness",
                "precompact_notice",
                "prompt_entry",
                "session_cleanup_observer",
                "session_start_inject",
                "size_notice",
                "statusline_heal_observer",
                "subagent_inject",
                "wave_alive_observer",
                "write_gate",
            ]
        );
    }

    /// Uma entrada com várias ferramentas casa com cada uma delas, e com
    /// nenhuma outra; sem ferramenta, não casa.
    #[test]
    fn one_of_matches_each_listed_tool_and_nothing_else() {
        let pair = ToolMatch::OneOf(&["Task", "Agent"]);
        assert!(pair.matches(Some("Task")));
        assert!(pair.matches(Some("Agent")));
        assert!(!pair.matches(Some("Bash")));
        assert!(!pair.matches(None));
        assert!(ToolMatch::Any.matches(None));
        assert!(ToolMatch::Named("Bash").matches(Some("Bash")));
        assert!(!ToolMatch::Named("Bash").matches(Some("bash")));
    }

    /// A trava de comandos roda só no `PreToolUse` do Bash, e o aviso de
    /// tamanho depois dela; o sinal de vida da onda, que roda depois de toda
    /// ferramenta, continua no `PostToolUse`.
    #[test]
    fn the_command_guard_runs_before_bash_only() {
        let registry = Registry::new();
        assert_eq!(applicable_ids(&registry, Trigger::PreToolUse, Some("Bash")), ["command_guard", "size_notice"]);
        assert_eq!(applicable_ids(&registry, Trigger::PostToolUse, Some("Bash")), ["wave_alive_observer", "size_notice"]);
        assert!(!applicable_ids(&registry, Trigger::PreToolUse, Some("Write")).contains(&"command_guard"));
    }

    /// O portão de escrita roda antes das cinco ferramentas de arquivo e das
    /// duas buscas (por palavra e por nome), e só delas, com o aviso de
    /// tamanho depois dele; o sinal de vida da
    /// onda segue rodando depois de cada uma, e a testemunha do glossário, só
    /// depois das três que editam texto: a leitura nunca ensina.
    #[test]
    fn the_write_gate_runs_on_the_file_tools_and_the_searches() {
        let registry = Registry::new();
        for tool in ["Read", "Write", "Edit", "MultiEdit", "NotebookEdit", "Grep", "Glob"] {
            assert_eq!(applicable_ids(&registry, Trigger::PreToolUse, Some(tool)), ["write_gate", "size_notice"], "{tool}");
            let after: &[&str] = if ["Write", "Edit", "MultiEdit"].contains(&tool) {
                &["wave_alive_observer", "glossary_witness", "size_notice"]
            } else {
                &["wave_alive_observer", "size_notice"]
            };
            assert_eq!(applicable_ids(&registry, Trigger::PostToolUse, Some(tool)), after, "{tool}");
        }
        let module = registry.by_id("glossary_witness").expect("registered");
        assert!(module.check.is_none() && module.observer.is_some());
        for tool in ["Bash", "Task", "Agent", "Skill", "WebFetch"] {
            assert!(!applicable_ids(&registry, Trigger::PreToolUse, Some(tool)).contains(&"write_gate"), "{tool}");
        }
        let module = registry.by_id("write_gate").expect("registered");
        assert!(module.check.is_some() && module.observer.is_none());
    }

    /// O pedido do subagente roda no despacho de um agente, com o aviso de
    /// tamanho depois dele, e o início e o fim de subagente não têm gancho
    /// nenhum.
    #[test]
    fn the_agent_dispatch_runs_only_the_subagent_inject() {
        let registry = Registry::new();
        for tool in ["Task", "Agent"] {
            assert_eq!(applicable_ids(&registry, Trigger::PreToolUse, Some(tool)), ["subagent_inject", "size_notice"], "{tool}");
            assert_eq!(applicable_ids(&registry, Trigger::PostToolUse, Some(tool)), ["wave_alive_observer", "size_notice"], "{tool}");
        }
        assert!(applicable_ids(&registry, Trigger::SubagentStart, None).is_empty());
        assert!(applicable_ids(&registry, Trigger::SubagentStop, None).is_empty());
        assert_eq!(applicable_ids(&registry, Trigger::PreToolUse, Some("Skill")), ["size_notice"]);
    }

    /// A testemunha roda só depois da pergunta com opções, e é uma trava que
    /// devolve veredito, não um observador; o sinal de vida da onda, que roda
    /// depois de toda ferramenta, roda ali também.
    #[test]
    fn ask_user_question_post_tool_use_runs_approval_witness() {
        let registry = Registry::new();
        assert_eq!(
            applicable_ids(&registry, Trigger::PostToolUse, Some("AskUserQuestion")),
            ["approval_witness", "wave_alive_observer", "size_notice"]
        );
        assert_eq!(applicable_ids(&registry, Trigger::PreToolUse, Some("AskUserQuestion")), ["size_notice"]);
        assert_eq!(applicable_ids(&registry, Trigger::PostToolUse, Some("ExitPlanMode")), ["wave_alive_observer", "size_notice"]);
        let module = registry.by_id("approval_witness").expect("registered");
        assert!(module.check.is_some() && module.observer.is_none());
    }

    /// O sinal de vida é um observador puro, sem veredito.
    #[test]
    fn wave_alive_observer_is_a_pure_observer() {
        let registry = Registry::new();
        let module = registry.by_id("wave_alive_observer").expect("registered");
        assert!(module.check.is_none() && module.observer.is_some());
    }

    /// O aviso de tamanho roda nos dois lados de qualquer ferramenta — depois
    /// dela, junto do sinal de vida, para avisar, e antes dela, para recusar o
    /// agente de onda que gastou a folga — e em nenhum outro evento, e é uma
    /// trava que devolve veredito, não um observador.
    #[test]
    fn size_notice_runs_after_every_tool() {
        let registry = Registry::new();
        for tool in ["Bash", "Write", "Task", "Grep"] {
            assert!(applicable_ids(&registry, Trigger::PostToolUse, Some(tool)).contains(&"size_notice"), "{tool}");
            assert!(applicable_ids(&registry, Trigger::PreToolUse, Some(tool)).contains(&"size_notice"), "{tool}");
        }
        for trigger in [Trigger::UserPromptSubmit, Trigger::PreCompact, Trigger::Stop, Trigger::SessionStart] {
            assert!(!applicable_ids(&registry, trigger, None).contains(&"size_notice"), "{trigger:?}");
        }
        let module = registry.by_id("size_notice").expect("registered");
        assert!(module.check.is_some() && module.observer.is_none());
    }

    /// O fim da resposta é uma conferência só, um `Check` puro.
    #[test]
    fn end_of_turn_check_is_the_only_module_on_stop() {
        let registry = Registry::new();
        assert_eq!(applicable_ids(&registry, Trigger::Stop, None), ["end_of_turn_check"]);
        let module = registry.by_id("end_of_turn_check").expect("registered");
        assert!(module.check.is_some() && module.observer.is_none());
    }

    /// A mensagem tem um gancho só; o início da sessão, dois, com o que
    /// coloca texto primeiro; o fim da sessão, a faxina; antes de compactar,
    /// o aviso com o bloco de retomada.
    #[test]
    fn the_session_hooks_apply_to_their_events() {
        let registry = Registry::new();
        assert_eq!(applicable_ids(&registry, Trigger::UserPromptSubmit, None), ["prompt_entry"]);
        assert_eq!(
            applicable_ids(&registry, Trigger::SessionStart, None),
            ["session_start_inject", "statusline_heal_observer"]
        );
        assert_eq!(applicable_ids(&registry, Trigger::SessionEnd, None), ["session_cleanup_observer"]);
        assert_eq!(applicable_ids(&registry, Trigger::PreCompact, None), ["precompact_notice"]);
    }
}
