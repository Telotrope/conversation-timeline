//! The read-after-write experiment as a Lambda (plan
//! `docs/plans/2026-10-02-upload-processing-failures.md` §0b; stack
//! `infra/experiments/read-after-write.yaml`). Wiring only; the loop is
//! `timeline_experiments::read_after_write::run`.
//!
//! Input: `{"rows": 4000}`. Output: the report as JSON. The table comes from
//! `TIMELINE_EXPERIMENT_TABLE`; a missing setting stops start-up naming it.

use lambda_runtime::{service_fn, LambdaEvent};
use serde_json::Value;
use timeline_experiments::read_after_write::run;

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    let table = std::env::var("TIMELINE_EXPERIMENT_TABLE")
        .unwrap_or_else(|e| panic!("cannot start: TIMELINE_EXPERIMENT_TABLE: {e}"));
    let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = aws_sdk_dynamodb::Client::new(&sdk_config);
    let (client, table) = (&client, table.as_str());
    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| async move {
        let rows = event
            .payload
            .get("rows")
            .and_then(Value::as_u64)
            .ok_or("input must be {\"rows\": <a whole number>}")?;
        let report = run(client, table, rows as usize).await;
        serde_json::to_value(report).map_err(lambda_runtime::Error::from)
    }))
    .await
}
