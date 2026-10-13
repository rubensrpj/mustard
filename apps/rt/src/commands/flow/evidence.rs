//! Attach a measured tool receipt without closing or manually re-versioning
//! a point. Receipt provenance and the interpretation remain distinct.
use mustard_core::{
    domain::spec_state::SpecState,
    io::{observations, spec_events as store},
};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) fn run(
    start: &Path,
    named: Option<&str>,
    wave: Option<u64>,
    point: Option<&str>,
    receipt: Option<&str>,
    text: Option<&str>,
) -> Value {
    let result = (|| -> Result<Value, String> {
        let root = store::spec_root(start);
        let session = crate::shared::spec_state::session_from_env();
        let spec = named
            .map(str::to_string)
            .or_else(|| {
                crate::shared::spec_state::DiskSpecState::new(&root).active(session.as_deref())
            })
            .ok_or("no-current-spec")?;
        let path = store::spec_file(&root, &spec).map_err(|e| format!("{e:?}"))?;
        let log = store::read(&path)
            .map_err(|e| format!("{e:?}"))?
            .ok_or("no-spec-file")?;
        let dispatch = if let Some(wave) = wave {
            let send = log
                .last_by_wave("send")
                .get(&wave)
                .copied()
                .ok_or("wave-not-sent")?;
            log.dispatch_position(send)
        } else {
            0
        };
        let Some(point) = point else {
            return observations::page(&root,&spec,dispatch,None).or_else(|reason| {
                if !root.join(".claude/mustard/observations.db").exists() {Ok(json!({"receipts":[],"note":"No native tool result recorded; no measurement inferred."}))} else {Err(reason)}
            }).map(|mut report|{report["ok"]=json!(true);report["spec"]=json!(spec);report});
        };
        let receipt_id = receipt.ok_or("receipt-required")?;
        let observed = observations::read(&root, receipt_id)?;
        if observed.spec != spec {
            return Err("evidence-belongs-to-another-spec".into());
        }
        let text = crate::shared::secret::without_secrets(text.ok_or("measurement-text-required")?);
        if text.trim().is_empty() || text.len() > 2000 {
            return Err("measurement-text-must-be-1-to-2000-bytes".into());
        }
        store::with_locked_writer(&path,|locked|->Result<Value,String>{
            let log=locked.log();
            let codes=log.codes();
            let target=super::answer::reference(point).ok_or("point-not-found")?;
            let id=super::answer::newest(log,&target).ok_or("point-not-found")?;
            let id=codes.get(&id).and_then(|code|super::answer::newest(log,&mustard_core::domain::spec_events::EventRef::Code(code.clone()))).unwrap_or(id);
            let current=log.current(id).filter(|event|event.event_type=="point").ok_or("point-not-found")?;
            let previous=current.fields.get("facts").or_else(||log.visible().into_iter().rev().find(|event|event.event_type=="point" && codes.get(&event.id)==codes.get(&current.id) && event.fields.contains_key("facts")).and_then(|event|event.fields.get("facts")));
            let mut facts=previous.and_then(Value::as_array).cloned().unwrap_or_default();
            if facts.iter().any(|fact|fact["receipt"]==receipt_id && fact["text"]==text) {
                return Ok(json!({"ok":true,"changed":false,"point":point,"receipt":receipt_id}));
            }
            facts.push(json!({"text":text,"source":format!("tool result: {}",observed.source),"receipt":receipt_id,
                "classification":"measured-tool-result","verification":"tool-result-observed; interpretation-unverified",
                "input_sha256":observed.input_sha256,"output_sha256":observed.output_sha256,"observed_at":observed.observed_at,
                "exit_code":observed.exit_code,"result_status":observed.result_status}));
            let mut draft=current.fields.iter().filter(|(key,_)|["block","gap","from","status","closes","result","reused_results","reason","reminders","origin"].contains(&key.as_str())).map(|(key,value)|(key.clone(),value.clone())).collect::<serde_json::Map<_,_>>();
            draft.insert("replaces".into(),json!(current.id));draft.insert("facts".into(),json!(facts));
            let written=crate::commands::spec_events::write::record_locked_by_model(locked,start,&spec,"point",draft).map_err(|e|format!("{e:?}"))?.written;
            Ok(json!({"ok":true,"changed":true,"point":written.code,"id":written.id,"receipt":receipt_id,"claim_verified":false}))
        }).map_err(|e|format!("{e:?}"))?.ok_or("no-spec-file")?
    })();
    result.unwrap_or_else(|reason| json!({"ok":false,"reason":reason}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measurements_attach_once_without_closing_the_point_or_crossing_specs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::commands::spec_events::write::record_open(root, "x", "fix/x", "dev").unwrap();
        let origin = crate::shared::spec_state::seed_event(
            root,
            "x",
            "message",
            json!({"author":"user","text":"Verificar a leitura."}),
        );
        let point = crate::shared::spec_state::seed_event(
            root,
            "x",
            "point",
            json!({"block":"context","gap":"Comprovar a leitura","from":"gap","status":"open","origin":origin,"author":"binary"}),
        );
        let receipt = observations::Receipt {
            version: 1,
            spec: "x".into(),
            wave: 0,
            dispatch: 0,
            host: "claude".into(),
            actor: "main".into(),
            call: "measured".into(),
            tool: "Bash".into(),
            source: "node measure.js input.xlsx".into(),
            input_sha256: observations::digest(b"node measure.js input.xlsx"),
            output_sha256: observations::digest(b"42 ms"),
            exit_code: Some(0),
            result_status: "returned".into(),
            observed_at: "2026-10-13T12:00:00Z".into(),
        };
        let (id, _) = observations::record(root, &receipt).unwrap();
        let first = run(
            root,
            Some("x"),
            None,
            Some(&point.to_string()),
            Some(&id),
            Some("42 ms for this input; other inputs not checked."),
        );
        assert_eq!(first["ok"], true, "{first}");
        let code = first["point"].as_str().unwrap();
        let again = run(
            root,
            Some("x"),
            None,
            Some(code),
            Some(&id),
            Some("42 ms for this input; other inputs not checked."),
        );
        assert_eq!(again["changed"], false, "{again}");
        let log = store::read(&store::spec_file(root, "x").unwrap())
            .unwrap()
            .unwrap();
        let current = log.current(point).unwrap();
        assert_eq!(current.str_field("status"), Some("open"));
        assert_eq!(current.fields["facts"][0]["receipt"], id);
        assert_eq!(
            current.fields["facts"][0]["verification"],
            "tool-result-observed; interpretation-unverified"
        );
        crate::shared::spec_state::seed_event(
            root,
            "x",
            "point",
            json!({"block":"context","gap":"Comprovar a leitura","from":"gap","status":"not_applicable","closes":point,"reason":"Already inspected.","origin":origin}),
        );
        let attached = run(
            root,
            Some("x"),
            None,
            Some(&point.to_string()),
            Some(&id),
            Some("Additional measured limit."),
        );
        assert_eq!(attached["ok"], true, "{attached}");
        let closed_log = store::read(&store::spec_file(root, "x").unwrap())
            .unwrap()
            .unwrap();
        let updated = closed_log.get(attached["id"].as_u64().unwrap()).unwrap();
        assert_eq!(updated.str_field("status"), Some("not_applicable"));
        assert_eq!(updated.str_field("reason"), Some("Already inspected."));
        assert_eq!(
            updated.fields["facts"].as_array().unwrap().len(),
            2,
            "closed-point evidence retains earlier facts"
        );
        assert_eq!(attached["point"], code);
        let mut other = receipt.clone();
        other.spec = "other".into();
        other.call = "another-call".into();
        let (other_id, _) = observations::record(root, &other).unwrap();
        assert_eq!(
            run(
                root,
                Some("x"),
                None,
                Some(code),
                Some(&other_id),
                Some("another result")
            )["reason"],
            "evidence-belongs-to-another-spec"
        );
        crate::shared::context::session::bind_session_spec(&root.to_string_lossy(), "survey", "x");
        let input = mustard_core::domain::model::contract::HookInput {
            session_id: Some("survey".into()),
            tool_name: Some("Bash".into()),
            tool_input: json!({"command":"node measure.js input.xlsx"}),
            raw: json!({"tool_use_id":"native-main-call","tool_response":{"stdout":"42 ms","exit_code":0}}),
            ..Default::default()
        };
        assert!(
            crate::shared::tool_observations::capture(root, "claude", "PostToolUse", &input)
                .unwrap()
                .is_ok()
        );
        let listed = run(root, Some("x"), None, None, None, None);
        assert_eq!(
            listed["receipts"].as_array().unwrap().len(),
            2,
            "native survey tools are captured before any wave exists"
        );
    }
}
