// Twin of `i18n_code_pt/src/livro_faturas.rs`. Second entity in the fixture so
// a query has to CHOOSE, rather than land on the only module present.

pub struct InvoiceLedger {
    pub entries: usize,
}

pub struct InvoiceEntry {
    pub number: String,
}

pub fn post_invoice(ledger: &mut InvoiceLedger, entry: InvoiceEntry) {
    let _ = entry;
    ledger.entries += 1;
}

pub fn cancel_invoice(ledger: &mut InvoiceLedger) -> bool {
    if ledger.entries == 0 {
        return false;
    }
    ledger.entries -= 1;
    true
}
