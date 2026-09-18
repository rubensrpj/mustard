//! Os códigos dos itens, `MSTD-<sigla>-<NNNN>`: o de cada linha lida e o que
//! o próximo evento grava. Um número que saiu nunca volta.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::domain::mustard_id;

use super::{type_spec, SpecEvent, SpecLog};

impl SpecLog {
    /// O código de cada evento, `MSTD-<sigla>-<NNNN>`, pelo número do evento.
    ///
    /// O código gravado na linha vale como está. Uma linha sem código, de uma
    /// edição à mão ou de um gravador anterior, recebe o da versão que ela
    /// substitui (`replaces`) ou, senão, o próximo número livre do tipo: o
    /// seguinte ao maior visto até ela no arquivo, pulando os que outra linha
    /// já gravou. Assim os códigos gravados nunca mudam, e o de uma linha sem
    /// código não muda quando outra linha é gravada no fim. Um tipo que este
    /// binário não conhece fica sem código. Cada spec conta do zero.
    #[must_use]
    pub fn codes(&self) -> BTreeMap<u64, String> {
        let mut codes: BTreeMap<u64, String> = BTreeMap::new();
        let mut taken: BTreeSet<(&str, u64)> = BTreeSet::new();
        for event in &self.events {
            if let Some((kind, n)) = recorded_code(event) {
                codes.insert(event.id, mustard_id::format(kind, n));
                taken.insert((kind, n));
            }
        }
        let mut highest: BTreeMap<&str, u64> = BTreeMap::new();
        for event in &self.events {
            let Some(spec) = type_spec(&event.event_type) else {
                continue;
            };
            let top = highest.entry(spec.code).or_insert(0);
            if let Some((_, n)) = recorded_code(event) {
                *top = (*top).max(n);
                continue;
            }
            let inherited = event
                .int("replaces")
                .filter(|old| self.get(*old).is_some_and(|o| o.event_type == event.event_type))
                .and_then(|old| codes.get(&old).cloned());
            if let Some(code) = inherited {
                codes.insert(event.id, code);
                continue;
            }
            let mut n = *top + 1;
            while taken.contains(&(spec.code, n)) {
                n += 1;
            }
            *top = n;
            codes.insert(event.id, mustard_id::format(spec.code, n));
        }
        codes
    }
}

/// O código gravado numa linha, como sigla e número, quando ele tem o formato
/// e a sigla do tipo da linha. Um código de outro tipo ou fora do formato
/// conta como ausente.
fn recorded_code(event: &SpecEvent) -> Option<(&'static str, u64)> {
    let spec = type_spec(&event.event_type)?;
    let (kind, n) = mustard_id::parse(event.str_field("code")?.trim())?;
    (kind == spec.code).then_some((spec.code, n))
}

/// O código que o evento `event` grava ao entrar no fim de `log`: o da versão
/// que ele substitui, quando `replaces` aponta um item do mesmo tipo; senão, o
/// maior número que o tipo já tem na spec, mais 1. Nunca a posição: um número
/// que saiu do meio do arquivo não volta. `None` para um tipo desconhecido.
#[must_use]
pub fn code_after(log: &SpecLog, event: &Map<String, Value>) -> Option<String> {
    let spec = type_spec(event.get("type").and_then(Value::as_str)?)?;
    let codes = log.codes();
    let inherited = event
        .get("replaces")
        .and_then(Value::as_u64)
        .filter(|old| log.get(*old).is_some_and(|o| o.event_type == spec.name))
        .and_then(|old| codes.get(&old).cloned());
    if inherited.is_some() {
        return inherited;
    }
    let top = codes
        .values()
        .filter_map(|code| mustard_id::parse(code))
        .filter(|(kind, _)| *kind == spec.code)
        .map(|(_, n)| n)
        .max()
        .unwrap_or(0);
    Some(mustard_id::format(spec.code, top + 1))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::spec_events::tests::{line, obj};
    use crate::domain::spec_events::{parse_log, TYPES};

    /// Cada tipo tem uma sigla própria, só de letras maiúsculas, e o código
    /// montado com ela tem o formato do identificador do Mustard.
    #[test]
    fn every_type_has_its_own_code_letters() {
        let codes: BTreeSet<&str> = TYPES.iter().map(|t| t.code).collect();
        assert_eq!(codes.len(), TYPES.len(), "two types share a code");
        for t in TYPES {
            assert!(!t.code.is_empty() && t.code.bytes().all(|b| b.is_ascii_uppercase()), "{}", t.name);
            assert!(mustard_id::is_id(&mustard_id::format(t.code, 1)), "{}", t.name);
        }
    }

    /// Os códigos contam por tipo, na ordem do arquivo; a versão nova herda o
    /// código da antiga; um item removido ou expurgado segue contando, e o
    /// número dele nunca é dado de novo.
    #[test]
    fn codes_count_per_type_and_a_number_never_returns() {
        let content = [
            line(1, "rule", ",\"text\":\"a\""),
            line(2, "criterion", ",\"when\":\"w\""),
            line(3, "rule", ",\"text\":\"b\""),
            line(4, "remove", ",\"targets\":[3],\"reason\":\"x\""),
            line(5, "rule", ",\"text\":\"a2\",\"replaces\":1"),
            line(6, "purge", ",\"targets\":[2],\"reason\":\"secret\""),
            line(7, "rule", ",\"text\":\"c\""),
            line(8, "criterion", ",\"when\":\"w2\""),
            line(9, "future_kind", ""),
        ]
        .concat();
        let log = parse_log(&content);
        let codes = log.codes();
        assert_eq!(codes[&1], "MSTD-RULE-0001");
        assert_eq!(codes[&2], "MSTD-CRIT-0001");
        assert_eq!(codes[&3], "MSTD-RULE-0002");
        assert_eq!(codes[&4], "MSTD-RMV-0001");
        assert_eq!(codes[&5], "MSTD-RULE-0001", "the new version is the same item");
        assert_eq!(codes[&7], "MSTD-RULE-0003", "the removed number 2 never returns");
        assert_eq!(codes[&8], "MSTD-CRIT-0002", "the purged number 1 never returns");
        assert!(!codes.contains_key(&9), "an unknown type has no code");

        // O código do próximo evento sai do mesmo cálculo.
        let next = obj(json!({"id": 10, "type": "rule", "text": "d"}));
        assert_eq!(code_after(&log, &next).as_deref(), Some("MSTD-RULE-0004"));
        let revised = obj(json!({"id": 10, "type": "rule", "text": "c2", "replaces": 7}));
        assert_eq!(code_after(&log, &revised).as_deref(), Some("MSTD-RULE-0003"));

        // Outra spec conta do zero.
        let other = parse_log(&line(1, "rule", ",\"text\":\"z\""));
        assert_eq!(other.codes()[&1], "MSTD-RULE-0001");
    }

    /// O código gravado na linha vale como está. Uma linha sem código recebe
    /// o próximo número livre do tipo, pulando os que outra linha gravou, e
    /// esse número não muda quando outra linha é gravada no fim. Um código de
    /// outra sigla não vale.
    #[test]
    fn recorded_codes_stand_and_a_line_without_one_takes_the_next_free_number() {
        let coded = |id: u64, event_type: &str, code: &str, extra: &str| {
            line(id, event_type, &format!(",\"code\":\"{code}\"{extra}"))
        };
        let mut content = [
            coded(1, "rule", "MSTD-RULE-0001", ""),
            line(2, "rule", ",\"text\":\"inserida à mão\""),
            coded(3, "rule", "MSTD-RULE-0002", ""),
            coded(4, "rule", "MSTD-RULE-0003", ""),
            coded(5, "criterion", "MSTD-CRIT-0007", ""),
            line(6, "rule", ",\"text\":\"revista à mão\",\"replaces\":3"),
            coded(7, "rule", "MSTD-CRIT-0009", ""),
        ]
        .concat();
        let codes = parse_log(&content).codes();
        let got: Vec<&str> = (1..=7).map(|id| codes[&id].as_str()).collect();
        assert_eq!(
            got,
            [
                "MSTD-RULE-0001",
                "MSTD-RULE-0004",
                "MSTD-RULE-0002",
                "MSTD-RULE-0003",
                "MSTD-CRIT-0007",
                "MSTD-RULE-0002",
                "MSTD-RULE-0005",
            ]
        );
        let next = obj(json!({"type": "rule", "text": "nova"}));
        assert_eq!(code_after(&parse_log(&content), &next).as_deref(), Some("MSTD-RULE-0006"));
        assert_eq!(
            code_after(&parse_log(&content), &obj(json!({"type": "criterion"}))).as_deref(),
            Some("MSTD-CRIT-0008"),
            "the next number follows the largest, never the position"
        );

        content.push_str(&coded(8, "rule", "MSTD-RULE-0006", ""));
        let after = parse_log(&content).codes();
        assert_eq!((after[&2].as_str(), after[&7].as_str()), ("MSTD-RULE-0004", "MSTD-RULE-0005"));
    }
}
