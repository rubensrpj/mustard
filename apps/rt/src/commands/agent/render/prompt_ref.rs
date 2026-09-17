//! A marca do despacho e a chave estável que nomeia um arquivo de pedido.
//!
//! O gancho de injeção procura a marca no texto da tarefa para trocar o talão
//! pelo pedido inteiro; a página da spec e a lista de pendências usam a mesma
//! chave para carimbar o documento com o nome do arquivo de despacho.

/// Marker prefix of the stub's first line. `subagent_inject` greps the Task
/// prompt for this exact prefix to locate the file to expand.
pub const PROMPT_REF_MARKER: &str = "MUSTARD-PROMPT-REF:";

/// FNV-1a de 64 bits sobre as partes, com separador entre elas. Determinístico
/// — sem relógio e sem sorteio —, então as mesmas entradas dão sempre o mesmo
/// nome de arquivo.
pub(crate) fn fnv1a64(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for part in parts {
        for b in part.as_bytes() {
            eat(*b);
        }
        eat(0x1f); // unit separator — "ab","c" never collides with "a","bc"
    }
    h
}

#[cfg(test)]
mod tests {
    use super::fnv1a64;

    /// O separador entre as partes é o que impede que uma divisão diferente
    /// das mesmas letras caia na mesma chave.
    #[test]
    fn a_divisao_das_partes_muda_a_chave() {
        assert_ne!(fnv1a64(&["ab", "c"]), fnv1a64(&["a", "bc"]));
        assert_eq!(fnv1a64(&["ab", "c"]), fnv1a64(&["ab", "c"]));
    }
}
