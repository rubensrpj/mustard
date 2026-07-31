// Twin of `i18n_code_pt/src/processador_pagamento.rs`: same domain, same
// shapes, identifiers in English. The pair exists so retrieval can be measured
// across prompt-language x code-language without depending on finding a real
// non-English repository.
//
// Discriminating vocabulary lives in DECLARATION NAMES only. Prose is a hint
// about intent and can be stale or wrong, so the instrument never scores it.

pub struct PaymentProcessor {
    pub gateway: String,
}

pub struct PaymentReceipt {
    pub authorization: String,
}

impl PaymentProcessor {
    pub fn new(gateway: String) -> Self {
        Self { gateway }
    }
}

pub fn authorize_payment(processor: &PaymentProcessor, amount: u64) -> PaymentReceipt {
    let _ = (processor, amount);
    PaymentReceipt { authorization: String::new() }
}

pub fn refund_payment(receipt: &PaymentReceipt) -> bool {
    !receipt.authorization.is_empty()
}

pub fn settle_payment(receipt: &PaymentReceipt) -> bool {
    refund_payment(receipt)
}
