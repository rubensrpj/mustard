//! `scan spec` — compile a deterministic spec draft for one entity via
//! `grain spec`. Thin passthrough: constructs a [`SpecRequest`] from CLI flags
//! and prints the Markdown verbatim to stdout. The heavy logic lives entirely
//! in [`mustard_core::domain::scan::Scan::spec`]; this module owns nothing.

use std::path::PathBuf;

use mustard_core::{Scan, SpecRequest};

pub struct ScanSpecOpts {
    pub entity: String,
    pub like: Option<String>,
    pub ops: Vec<String>,
    pub invariants: Vec<String>,
    pub root: PathBuf,
}

/// Run `grain spec` for `opts.entity` and print the resulting Markdown to
/// stdout. Exits with code `1` on failure (grain not installed, model missing,
/// non-zero exit from grain) so the caller can detect the error.
pub fn run(opts: ScanSpecOpts) {
    let model = opts.root.join(".claude").join("grain.model.json");
    let req = request_from(opts);
    match Scan::locate().spec(&model, &req) {
        Ok(md) => println!("{md}"),
        Err(err) => {
            eprintln!("scan spec: grain failed: {err}");
            std::process::exit(1);
        }
    }
}

/// A única conversão este módulo possui: as opções da linha de comando viram um
/// [`SpecRequest`]. Um `--like` ausente chega como `None` e sai como string
/// vazia, que é o que o motor lê como "sem filtro".
///
/// Existe como função para que os testes possam CHAMÁ-LA. Antes eles refaziam a
/// mesma conta por conta própria, e uma cópia da regra nunca reprova quando o
/// original muda — que é a única coisa que um teste de mapeamento precisa fazer.
fn request_from(opts: ScanSpecOpts) -> SpecRequest {
    SpecRequest {
        entity: opts.entity,
        like: opts.like.unwrap_or_default(),
        ops: opts.ops,
        invariants: opts.invariants,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that a `SpecRequest` is wired from opts correctly (no logic
    /// change — just the field mapping we own). We do NOT invoke grain (not
    /// installed in CI); the test is purely about argument plumbing.
    #[test]
    fn opts_wire_to_spec_request() {
        let opts = ScanSpecOpts {
            entity: "Order".to_string(),
            like: Some("Invoice".to_string()),
            ops: vec!["approve".to_string(), "cancel".to_string()],
            invariants: vec!["no-double-charge".to_string()],
            root: PathBuf::from("."),
        };
        let req = request_from(opts);
        assert_eq!(req.entity, "Order");
        assert_eq!(req.like, "Invoice");
        assert_eq!(req.ops, ["approve", "cancel"]);
        assert_eq!(req.invariants, ["no-double-charge"]);
    }

    /// Um `--like` ausente nao pode virar filtro.
    ///
    /// O teste antigo afirmava `None::<String>.unwrap_or_default() ==
    /// String::new()` — uma propriedade da biblioteca padrao, nao deste crate:
    /// verdadeira para sempre e cega para qualquer regressao daqui. Agora
    /// chama a conversao de verdade.
    #[test]
    fn absent_like_reaches_the_scan_as_an_empty_string() {
        let req = request_from(ScanSpecOpts {
            entity: "Order".to_string(),
            like: None,
            ops: Vec::new(),
            invariants: Vec::new(),
            root: PathBuf::from("."),
        });
        assert!(req.like.is_empty(), "um --like ausente nao pode virar filtro");
    }
}
