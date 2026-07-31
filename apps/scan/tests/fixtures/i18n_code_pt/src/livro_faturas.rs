// Gemeo de `i18n_code_en/src/invoice_ledger.rs`. Segunda entidade do fixture
// para que a consulta tenha de ESCOLHER, em vez de cair no unico modulo.

pub struct LivroFaturas {
    pub lancamentos: usize,
}

pub struct LancamentoFatura {
    pub numero: String,
}

pub fn lancar_fatura(livro: &mut LivroFaturas, lancamento: LancamentoFatura) {
    let _ = lancamento;
    livro.lancamentos += 1;
}

pub fn cancelar_fatura(livro: &mut LivroFaturas) -> bool {
    if livro.lancamentos == 0 {
        return false;
    }
    livro.lancamentos -= 1;
    true
}
