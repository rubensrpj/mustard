//! O que a rodada, a retomada, o painel e o fechamento leem do mesmo jeito,
//! sem importar nenhum deles: a linha de comando de cada passo, a marca do
//! fechamento recusado, a soltura das marcas de conserto sem prova, os testes
//! sem dono, o pedido de revisão final que ficou para trás e as contas do
//! andamento que não dependem da rodada. Os quatro importam daqui, e nada
//! daqui importa os quatro: o que depende da leitura da rodada chega como
//! argumento de quem chama.

use mustard_core::domain::spec_state::State;

pub(crate) mod outdated_request;
pub(crate) mod ownerless;
pub(crate) mod progress;
pub(crate) mod stale_marks;

/// A etapa em que o fechamento recusado fica gravado, para a retomada.
pub(crate) const REFUSED_STAGE: &str = "close-refused";

/// A linha inteira do passo `name` da spec `spec`, com as opções que o passo
/// exige tiradas do estado: o pull request sai da branch da spec para a base
/// dela. `None` quando o estado não traz o que o passo exige — uma linha sem
/// uma opção obrigatória é recusada pelo binário antes de fazer qualquer coisa.
pub fn step_command(name: &str, spec: &str, state: &State) -> Option<String> {
    match name {
        "round" => Some(format!("mustard-rt run round --dispatch --spec {spec}")),
        "pr-open" => {
            let base = state.base.as_deref().map(str::trim).filter(|b| !b.is_empty())?;
            let head = state.branch.as_deref().map(str::trim).filter(|b| !b.is_empty())?;
            Some(format!("mustard-rt run pr-open --base {base} --head {head} --spec {spec}"))
        }
        _ => Some(format!("mustard-rt run {name} --spec {spec}")),
    }
}
