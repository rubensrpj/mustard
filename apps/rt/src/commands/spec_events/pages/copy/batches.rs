//! Os lotes da cópia: as escritas de uma página divididas nas chamadas da
//! ferramenta do banco.
//!
//! Cada lote vai numa chamada só, com o corpo de cada documento lido do
//! arquivo dele, e o banco recusa a chamada que passa de 1.048.576 bytes.
//! Por isso o lote fecha em [`BATCH_MAX`] escritas ou antes da escrita que
//! faria a soma dele passar de [`BATCH_MAX_BYTES`]: a soma conta a linha de
//! cada escrita e o arquivo que ela manda. A ordem das escritas não muda, e
//! o documento que sozinho passa do teto vai sozinho num lote, sem lote
//! vazio antes dele.

use std::path::Path;

use mustard_core::domain::spec_events::Refusal;
use serde_json::Value;

use super::{write, BATCH_MAX};
use crate::commands::spec_events::pages::relative;

/// Quantos bytes um lote soma no máximo, contando o arquivo de cada
/// documento: abaixo do teto de 1.048.576 bytes do banco, com folga para o
/// que a ferramenta acrescenta ao montar a chamada.
pub(crate) const BATCH_MAX_BYTES: u64 = 800_000;

/// Grava as escritas em lotes, `<nome>-1.json`, `<nome>-2.json`…, uma
/// escrita por linha, dentro de `folder`, e devolve o caminho de cada lote,
/// relativo a `root`, e as escritas de cada um, como foram gravadas. O
/// `file_path` de cada escrita é lido a partir de `root`.
pub(crate) fn batches_in(
    root: &Path,
    folder: &Path,
    name: &str,
    writes: &[Value],
) -> Result<(Vec<String>, Vec<Vec<Value>>), Refusal> {
    let mut files = Vec::new();
    let mut sent = Vec::new();
    for (n, chunk) in split(root, writes).into_iter().enumerate() {
        let path = folder.join(format!("{name}-{}.json", n + 1));
        let lines: Vec<String> = chunk.iter().map(Value::to_string).collect();
        write(&path, &format!("[\n{}\n]\n", lines.join(",\n")))?;
        files.push(relative(root, &path));
        sent.push(chunk.to_vec());
    }
    Ok((files, sent))
}

/// As escritas de `writes` em lotes, na ordem: o lote fecha em
/// [`BATCH_MAX`] escritas ou antes da escrita que faria a soma dele
/// ([`weight`]) passar de [`BATCH_MAX_BYTES`].
fn split<'a>(root: &Path, writes: &'a [Value]) -> Vec<&'a [Value]> {
    let mut out = Vec::new();
    let (mut start, mut bytes) = (0, 0);
    for (at, write) in writes.iter().enumerate() {
        let size = weight(root, write);
        let count = at - start;
        if count > 0 && (count == BATCH_MAX || bytes + size > BATCH_MAX_BYTES) {
            out.push(&writes[start..at]);
            (start, bytes) = (at, 0);
        }
        bytes += size;
    }
    if start < writes.len() {
        out.push(&writes[start..]);
    }
    out
}

/// O que uma escrita pesa na chamada: a linha dela e o arquivo do documento
/// que ela manda, pelo `file_path` lido a partir de `root`. A escrita que
/// tira um documento não tem arquivo.
fn weight(root: &Path, write: &Value) -> u64 {
    let file = write["file_path"].as_str().and_then(|file| std::fs::metadata(root.join(file)).ok());
    write.to_string().len() as u64 + file.map_or(0, |meta| meta.len())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::set_in;
    use super::*;

    /// Um documento de `bytes` bytes no arquivo, escrito como a cópia o
    /// escreve, na faixa `start`.
    fn document(root: &Path, folder: &Path, start: u64, bytes: usize) -> Value {
        let body = json!({ "text": "x".repeat(bytes - r#"{"text":""}"#.len()) });
        set_in(root, folder, 7, "ranges", &start.to_string(), &body).expect("document written")
    }

    /// O tamanho que a ferramenta do banco monta para cada lote, lido do
    /// disco: a linha de cada escrita e o arquivo do documento dela.
    fn sizes_on_disk(root: &Path, sent: &[Vec<Value>]) -> Vec<u64> {
        sent.iter()
            .map(|batch| {
                batch
                    .iter()
                    .map(|w| {
                        let file = std::fs::read(root.join(w["file_path"].as_str().unwrap_or_default()));
                        w.to_string().len() as u64 + file.map_or(0, |bytes| bytes.len() as u64)
                    })
                    .sum()
            })
            .collect()
    }

    /// Quarenta e um documentos de 70 mil bytes, quase 3 megabytes juntos,
    /// saem em lotes de onze, que ficam abaixo do teto do banco, na mesma
    /// ordem em que entraram, e cada arquivo de lote traz as escritas dele.
    #[test]
    fn forty_one_documents_of_seventy_thousand_bytes_go_in_batches_under_the_ceiling_in_order() {
        let dir = tempdir().expect("tempdir");
        let (root, folder) = (dir.path(), dir.path().join("copy"));
        let writes: Vec<Value> = (0..41).map(|n| document(root, &folder, n * 100, 70_000)).collect();
        let body = std::fs::metadata(root.join(writes[0]["file_path"].as_str().unwrap_or_default())).unwrap();
        assert_eq!(body.len(), 70_000);

        let (files, sent) = batches_in(root, &folder, "spec", &writes).expect("batches written");

        assert_eq!(sent.iter().map(Vec::len).collect::<Vec<_>>(), vec![11, 11, 11, 8]);
        for size in sizes_on_disk(root, &sent) {
            assert!(size <= BATCH_MAX_BYTES, "{size} bytes in one batch");
        }
        assert_eq!(sent.concat(), writes, "the same writes, in the same order");
        assert_eq!(files, vec!["copy/spec-1.json", "copy/spec-2.json", "copy/spec-3.json", "copy/spec-4.json"]);
        for (file, batch) in files.iter().zip(&sent) {
            let on_disk: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(root.join(file)).unwrap()).unwrap();
            assert_eq!(&on_disk, batch, "{file}");
        }
    }

    /// O documento que sozinho passa do teto vai sozinho num lote, entre os
    /// vizinhos, sem lote vazio e sem perder escrita; o apagamento, sem
    /// arquivo, segue no lote do vizinho.
    #[test]
    fn a_document_over_the_ceiling_goes_alone_without_an_empty_batch() {
        let dir = tempdir().expect("tempdir");
        let (root, folder) = (dir.path(), dir.path().join("copy"));
        let small = document(root, &folder, 0, 1_000);
        let big = document(root, &folder, 100, 900_000);
        let after = document(root, &folder, 200, 1_000);
        let delete = json!({ "op": "delete", "collection": "ranges", "doc_id": "300" });
        let writes = vec![small.clone(), big.clone(), after.clone(), delete.clone()];

        let (files, sent) = batches_in(root, &folder, "spec", &writes).expect("batches written");

        assert_eq!(sent, vec![vec![small], vec![big], vec![after, delete]]);
        assert_eq!(files.len(), 3);
    }
}
