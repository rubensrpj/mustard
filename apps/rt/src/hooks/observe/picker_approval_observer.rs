//! `picker_approval_observer` — o `UserPromptSubmit` do comando de barra da
//! spec.
//!
//! Digitar `/mustard:spec a`, ou `/mustard:spec` dentro da branch da spec, só
//! escolhe qual spec abrir; não aprova nada. A aprovação tem uma porta só, a
//! testemunha da pergunta com opções ([`super::approval_witness`]). Esta porta
//! fica registrada, sem nada a fazer, até sair do registro junto com as outras
//! portas antigas.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};

/// O observador do comando de barra da spec, que não grava nada.
pub struct PickerApprovalObserver;

impl Observer for PickerApprovalObserver {
    fn observe(&self, _input: &HookInput, _ctx: &Ctx) {}
}
