//! Toda leitura do pedido da revisão final conta o pedido pela versão que o
//! gravou: o lançamento do revisor grava uma versão nova do envio, com o
//! modelo que valeu, e ela não muda quando o pedido saiu.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{ReviewSend, answered_request, review_send};
use mustard_core::io::spec_events as store;
use serde_json::{Value, json};
use tempfile::tempdir;

use crate::commands::flow::round::{awaited_review, open_review};
use crate::commands::flow::validation::review_elapsed;

/// A obra cujo pedido de revisão final (6) saiu antes da onda 2 (7) e da
/// entrega dela (8), e cujo revisor só foi lançado depois (9), na versão nova
/// do envio. Com `verdict`, o veredito final aprovado (10) vem por último.
fn lines(verdict: bool) -> Vec<Value> {
    let mut lines = vec![
        json!({"v":1,"id":1,"at":"2026-10-26T10:00:00-03:00","type":"state","phase":"survey","branch":"feature/x","base":"dev"}),
        json!({"v":1,"id":2,"at":"2026-10-26T10:01:00-03:00","type":"state","phase":"approved","witness":{"question":"q","answer":"a"}}),
        json!({"v":1,"id":3,"at":"2026-10-26T10:02:00-03:00","type":"state","phase":"running"}),
        json!({"v":1,"id":4,"at":"2026-10-26T10:10:00-03:00","type":"send","wave":1,"role":"wave","text":"Faça a onda 1."}),
        json!({"v":1,"id":5,"at":"2026-10-26T11:00:00-03:00","type":"delivered","wave":1,"text":"A onda 1 entregou.","files":["src/a.rs"]}),
        json!({"v":1,"id":6,"at":"2026-10-26T12:00:00-03:00","type":"send","role":"review","text":"Revise a obra.","read_items":[]}),
        json!({"v":1,"id":7,"at":"2026-10-26T12:05:00-03:00","type":"send","wave":2,"role":"wave","text":"Faça a onda 2."}),
        json!({"v":1,"id":8,"at":"2026-10-26T12:20:00-03:00","type":"delivered","wave":2,"text":"A onda 2 entregou.","files":["src/b.rs"]}),
        json!({"v":1,"id":9,"at":"2026-10-26T12:30:00-03:00","type":"send","role":"review","replaces":6,"model_used":"opus"}),
    ];
    if verdict {
        lines.push(json!({"v":1,"id":10,"at":"2026-10-26T13:00:00-03:00","type":"verdict","author":"review","final":true,
            "result":"approved","text":"Sem achados."}));
    }
    lines
}

/// Grava `lines` como o arquivo de eventos da spec `x` e o devolve lido.
fn written(root: &Path, lines: &[Value]) -> SpecLog {
    let path = store::spec_file(root, "x").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body: Vec<String> = lines.iter().map(Value::to_string).collect();
    std::fs::write(&path, body.join("\n") + "\n").unwrap();
    store::read(&path).unwrap().unwrap()
}

/// O pedido aberto, o que espera veredito, o que o veredito responde e o
/// tempo da revisão contam o pedido no número 6, que o gravou, e não no 9,
/// que o lançamento gravou depois da onda 2 e da entrega dela. Pelo 9, o
/// pedido feito antes da entrega esperaria veredito e a revisão duraria
/// meia hora em vez de uma.
#[test]
fn every_reading_of_the_final_review_request_counts_the_version_that_recorded_it() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let launched = written(root, &lines(false));
    assert_eq!(review_send(&launched, None), Some(ReviewSend { asked: 6, current: 9 }));
    assert_eq!(open_review(&launched), Some(6), "the open request is the one that recorded it");
    assert_eq!(awaited_review(&launched), None, "the request went out before the delivery of wave 2");

    let judged = written(root, &lines(true));
    assert_eq!(answered_request(&judged, 10), Some(6));
    review_elapsed(root, "x", &judged).unwrap();
    let after = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let elapsed = after.events.iter().find(|e| e.str_field("stage") == Some("independent-review-elapsed")).expect("the review time");
    assert_eq!((elapsed.int("attempt"), elapsed.int("ms")), (Some(6), Some(3_600_000)), "{:?}", elapsed.fields);
}
