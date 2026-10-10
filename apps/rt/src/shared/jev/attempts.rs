//! Physical HTTP attempts, including retries and failed responses. No state,
//! response body or authentication is written to the ledger.
use super::{json, JevFilter, JudgementRequest, Value, FilterError, Duration, JudgementService};

impl JevFilter {
    pub(super) fn record_attempt(
        &self,
        request: &JudgementRequest<'_>,
        attempt: u64,
        status: Option<u16>,
        result: &Result<Value, FilterError>,
        elapsed: Duration,
    ) {
        let Some(cache) = &self.cache else { return };
        let service = JudgementService::new(self, None);
        let value = result.as_ref().ok();
        let row = json!({"request_id":service.key(request),"purpose":request.purpose.key(),
            "spec":self.spec,"attempt":attempt,"at":chrono::Utc::now().to_rfc3339(),"ms":elapsed.as_millis(),
            "status":status,"result":if result.is_ok(){"received"}else{"failed"},
            "error":result.as_ref().err().map(FilterError::reason),
            "input_tokens":value.and_then(|doc|doc.pointer("/usage/input_tokens")).and_then(Value::as_u64),
            "output_tokens":value.and_then(|doc|doc.pointer("/usage/output_tokens")).and_then(Value::as_u64),
            "model":value.and_then(|doc|doc.get("model"))});
        let wrote = (|| -> Result<(), mustard_core::platform::error::Error> {
            std::fs::create_dir_all(cache)?;
            let mut file = mustard_core::io::fs::lock::LockedFile::exclusive(&cache.join("attempts.ndjson"))?;
            file.append_line(&row.to_string())
        })();
        if wrote.is_err() {
            eprintln!("mustard: Jev attempt metrics unavailable; usage may be incomplete");
        }
    }
}
