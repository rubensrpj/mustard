//! Os recibos de custo das sessões do anfitrião, um arquivo por sessão, e a
//! soma deles, no total e por spec. O total da sessão e a parte de cada spec
//! são contas separadas, e nenhuma entra duas vezes no uso das ondas.
//!
//! O módulo só depende do núcleo: o painel mostra estas somas, e o resumo do
//! projeto, que a gravação de eventos chama na entrega, lê aqui o custo de
//! cada spec sem chegar ao painel nem, por ele, ao fechamento.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::io::fs::lock::{LockedFile, read_shared};
use mustard_core::io::sha256::Sha256;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// O recibo de uma sessão: o último custo acumulado que o anfitrião mostrou
/// e as somas dos intervalos, com a parte de cada spec.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Receipt {
    last_micro_usd: Option<u64>,
    total_micro_usd: u64,
    unattributed_micro_usd: u64,
    by_spec: BTreeMap<String, u64>,
    last_spec: Option<String>,
    measured_at: String,
    first_at: String,
    model: Option<String>,
}

/// A pasta dos recibos das sessões do projeto `root`.
fn folder(root: &Path) -> PathBuf {
    root.join(".claude/mustard/usage/sessions")
}

fn read(text: &str) -> Result<Receipt, String> {
    if text.trim().is_empty() {
        Ok(Receipt::default())
    } else {
        serde_json::from_str(text).map_err(|_| "usage-receipt-unreadable".into())
    }
}

fn apply(
    receipt: &mut Receipt,
    measurement: &Value,
    spec: Option<&str>,
    at: &str,
) -> Result<(), String> {
    let Some(cost) = measurement["cost"]["usd"].as_f64() else {
        return Ok(());
    };
    if !cost.is_finite() || cost < 0.0 || cost > 1_000_000.0 {
        return Err("usage-invalid-cost".into());
    }
    let cost = (cost * 1_000_000.0).round() as u64;
    let delta = match receipt.last_micro_usd {
        Some(previous) if cost >= previous => cost - previous,
        Some(_) => return Err("usage-out-of-order-or-reset".into()),
        None => cost,
    };
    receipt.total_micro_usd = receipt.total_micro_usd.saturating_add(delta);
    // O intervalo que atravessa ramos e o primeiro custo, que já chega acima
    // de zero, não têm dono seguro. Olhar outra spec nunca muda o dono.
    if receipt.last_micro_usd.is_some()
        && receipt.last_spec.as_deref() == spec
        && let Some(spec) = spec
    {
        let total = receipt.by_spec.entry(spec.into()).or_default();
        *total = total.saturating_add(delta);
    } else {
        receipt.unattributed_micro_usd = receipt.unattributed_micro_usd.saturating_add(delta);
    }
    receipt.last_micro_usd = Some(cost);
    receipt.last_spec = spec.map(str::to_string);
    if receipt.first_at.is_empty() {
        receipt.first_at = at.into();
    }
    receipt.measured_at = at.into();
    receipt.model = measurement["model"]
        .as_str()
        .map(str::to_string)
        .or_else(|| receipt.model.clone());
    Ok(())
}

/// Soma a medição `measurement` da sessão `session` ao recibo dela no
/// projeto `root`, com a spec `spec` dona do intervalo, sob a trava do
/// arquivo do recibo.
///
/// # Errors
///
/// O recibo que não se lê ou não se grava, e a medição que não vale, com o
/// motivo; o recibo fica como estava.
pub(crate) fn add(root: &Path, session: &str, measurement: &Value, spec: Option<&str>) -> Result<(), String> {
    let mut hash = Sha256::new();
    hash.update(session.as_bytes());
    let path = folder(root).join(format!("{}.json", hash.hex_digest()));
    let mut lock = LockedFile::exclusive(&path).map_err(|e| e.to_string())?;
    let mut receipt = read(&lock.read_to_string().map_err(|e| e.to_string())?)?;
    apply(&mut receipt, measurement, spec, &chrono::Utc::now().to_rfc3339())?;
    let mut bytes = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    lock.replace(&bytes).map_err(|e| e.to_string())
}

/// A soma dos recibos das sessões do projeto `root`: o total, a parte sem
/// dono, a de cada spec e quantos recibos se leram.
pub(crate) fn costs(root: &Path) -> Value {
    let mut by_spec = BTreeMap::<String, u64>::new();
    let mut total = 0u64;
    let mut unattributed = 0u64;
    let mut known = 0;
    let mut unreadable = 0;
    let mut newest = String::new();
    let mut model = None;
    if let Ok(entries) = std::fs::read_dir(folder(root)) {
        for path in entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        {
            let receipt = read_shared(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| read(&text));
            match receipt {
                Ok(receipt) => {
                    known += 1;
                    total = total.saturating_add(receipt.total_micro_usd);
                    unattributed = unattributed.saturating_add(receipt.unattributed_micro_usd);
                    for (name, cost) in receipt.by_spec {
                        let sum = by_spec.entry(name).or_default();
                        *sum = sum.saturating_add(cost);
                    }
                    if receipt.measured_at > newest {
                        newest = receipt.measured_at;
                        model = receipt.model;
                    }
                }
                Err(_) => unreadable += 1,
            }
        }
    }
    json!({"known_micro_usd":if known>0{Some(total)}else{None},"unattributed_micro_usd":if known>0{Some(unattributed)}else{None},
        "by_spec":by_spec,"observed_sessions":known,"unreadable_sessions":unreadable,"measured_at":if newest.is_empty(){None}else{Some(newest)},
        "model":model,"basis":"host-api-equivalent-estimate","coverage":"observed-sessions-only","billed_micro_usd":null})
}

/// O custo de cada spec que os recibos das sessões do projeto `root`
/// conhecem, em milionésimos de dólar.
pub(crate) fn by_spec(root: &Path) -> BTreeMap<String, u64> {
    costs(root)["by_spec"].as_object().into_iter().flatten().filter_map(|(spec, cost)| Some((spec.clone(), cost.as_u64()?))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_totals_and_branch_switches_never_double_count_or_misattribute() {
        let mut receipt = Receipt::default();
        for (cost, spec) in [
            (1.0, Some("a")),
            (1.2, Some("a")),
            (1.2, Some("a")),
            (1.4, Some("b")),
            (1.6, Some("b")),
        ] {
            apply(&mut receipt, &json!({"cost":{"usd":cost}}), spec, "now").unwrap();
        }
        assert_eq!(receipt.total_micro_usd, 1_600_000);
        assert_eq!(receipt.by_spec["a"], 200_000);
        assert_eq!(receipt.by_spec["b"], 200_000);
        assert_eq!(receipt.unattributed_micro_usd, 1_200_000);
        assert!(
            apply(
                &mut receipt,
                &json!({"cost":{"usd":0.1}}),
                Some("b"),
                "later"
            )
            .is_err()
        );
        assert_eq!(receipt.total_micro_usd, 1_600_000);
    }
    #[test]
    fn missing_and_invalid_cost_do_not_become_zero_observations() {
        let mut receipt = Receipt::default();
        apply(&mut receipt, &json!({}), Some("a"), "now").unwrap();
        assert_eq!(receipt.last_micro_usd, None);
        assert!(apply(&mut receipt, &json!({"cost":{"usd":-1}}), None, "now").is_err());
    }
}
