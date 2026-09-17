//! `tool_result_observer` — o observador do `PostToolUse`.
//!
//! O gravador velho de eventos saiu nesta onda, e com ele o `tool.result` que
//! este observador era o único a escrever: o recorte da saída de cada
//! ferramenta (a saída do Bash, o antes e o depois de uma edição, o trecho de
//! uma leitura) não tem mais para onde ir, e saiu junto. O gancho em si sai com
//! os ganchos, na onda deles; enquanto isso ele fica registrado e não faz nada.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};

/// The `PostToolUse` lifecycle observer.
pub struct ToolResultObserver;

impl Observer for ToolResultObserver {
    fn observe(&self, _input: &HookInput, _ctx: &Ctx) {}
}
