// Gemeo de `i18n_code_en/src/payment_processor.rs`: mesmo dominio, mesmas
// formas, identificadores em portugues. O par existe para que a recuperacao
// seja medida no cruzamento idioma-da-pergunta x idioma-do-codigo.
//
// O vocabulario que discrimina mora SO em nome de declaracao. Prosa e pista
// sobre intencao e pode estar desatualizada ou errada, entao o instrumento
// nunca a pontua.

pub struct ProcessadorPagamento {
    pub gateway: String,
}

pub struct ReciboPagamento {
    pub autorizacao: String,
}

impl ProcessadorPagamento {
    pub fn novo(gateway: String) -> Self {
        Self { gateway }
    }
}

pub fn autorizar_pagamento(processador: &ProcessadorPagamento, valor: u64) -> ReciboPagamento {
    let _ = (processador, valor);
    ReciboPagamento { autorizacao: String::new() }
}

pub fn estornar_pagamento(recibo: &ReciboPagamento) -> bool {
    !recibo.autorizacao.is_empty()
}

pub fn liquidar_pagamento(recibo: &ReciboPagamento) -> bool {
    estornar_pagamento(recibo)
}
