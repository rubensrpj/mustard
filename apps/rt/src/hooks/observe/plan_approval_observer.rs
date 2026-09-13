//! `plan_approval_observer` — o `PostToolUse` do modo de plano (`ExitPlanMode`).
//!
//! Aceitar um plano no modo de plano não aprova spec nenhuma: aceitar um plano
//! pode ser sobre qualquer coisa. A aprovação tem uma porta só, a testemunha da
//! pergunta com opções ([`super::approval_witness`]). Esta porta fica
//! registrada, sem nada a fazer, até sair do registro junto com as outras
//! portas antigas.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};

/// O observador do modo de plano, que não grava nada.
pub struct PlanApprovalObserver;

impl Observer for PlanApprovalObserver {
    fn observe(&self, _input: &HookInput, _ctx: &Ctx) {}
}
