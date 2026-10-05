//! The template's part of recording what the user did and what reached AWS
//! (docs/plans/completed/2026-10-02-activity-instrumentation.md §2, §5, §6): the five
//! log groups and how long they keep records, each function writing to its
//! own group, API Gateway's request log, the activity function and its
//! route, the CORS headers the page needs, and the two settings.
//!
//! Plain text checks, crude in the way the migration plan's C16 describes.
//! Whether AWS accepts them is checked by deploying.

const TEMPLATE: &str = include_str!("../../../infra/template.yaml");

/// The indented block under `  <name>:`, up to the next line at that
/// indentation or less (as in `template_event_logging.rs`).
fn block(name: &str) -> String {
    let header = format!("\n  {name}:\n");
    let rest = TEMPLATE
        .split(&header)
        .nth(1)
        .unwrap_or_else(|| panic!("template declares {name}"));
    rest.lines()
        .take_while(|l| l.is_empty() || l.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n")
}

const GROUPS: [(&str, &str); 5] = [
    ("ApiLogGroup", "/timeline/${Stage}/api"),
    ("ActivityLogGroup", "/timeline/${Stage}/activity"),
    ("ProcessUploadLogGroup", "/timeline/${Stage}/process-upload"),
    (
        "RecordFailedUploadLogGroup",
        "/timeline/${Stage}/record-failed-upload",
    ),
    ("ApiAccessLogGroup", "/timeline/${Stage}/api-access"),
];

#[test]
fn five_log_groups_keep_records_for_the_retention_setting() {
    for (resource, name) in GROUPS {
        let group = block(resource);
        assert!(group.contains("Type: AWS::Logs::LogGroup"), "{group}");
        assert!(
            group.contains(&format!("LogGroupName: !Sub \"{name}\"")),
            "{group}"
        );
        assert!(
            group.contains("RetentionInDays: !Ref LogRetentionDays"),
            "{group}"
        );
    }
    let retention = block("LogRetentionDays");
    assert!(retention.contains("Default: 7"), "{retention}");
}

#[test]
fn each_function_writes_to_its_own_group() {
    for (function, group) in [
        ("ApiFunction", "ApiLogGroup"),
        ("ActivityFunction", "ActivityLogGroup"),
        ("ProcessUploadFunction", "ProcessUploadLogGroup"),
        ("RecordFailedUploadFunction", "RecordFailedUploadLogGroup"),
    ] {
        let body = block(function);
        assert!(
            body.contains(&format!("LoggingConfig:\n        LogGroup: !Ref {group}")),
            "{function}: {body}"
        );
    }
}

#[test]
fn api_gateway_logs_every_request_as_json_without_addresses() {
    let api = block("HttpApi");
    assert!(
        api.contains("DestinationArn: !GetAtt ApiAccessLogGroup.Arn"),
        "{api}"
    );
    for field in [
        "\"requestId\":\"$context.requestId\"",
        "\"routeKey\":\"$context.routeKey\"",
        "\"status\":\"$context.status\"",
        "\"authorizerError\":\"$context.authorizer.error\"",
    ] {
        assert!(api.contains(field), "{field} missing: {api}");
    }
    assert!(
        !api.contains("sourceIp") && !api.contains("userAgent"),
        "{api}"
    );
}

#[test]
fn the_activity_function_answers_post_activity_with_login_settings_only() {
    let function = block("ActivityFunction");
    assert!(
        function.contains("CodeUri: ../backend/target/lambda/record_activity/"),
        "{function}"
    );
    assert!(
        function.contains("Path: /activity\n            Method: POST"),
        "{function}"
    );
    assert!(
        function.contains("TIMELINE_COGNITO_USER_POOL_ID: !Ref UserPool"),
        "{function}"
    );
    assert!(
        function.contains("TIMELINE_COGNITO_CLIENT_ID: !Ref UserPoolClient"),
        "{function}"
    );
    assert!(
        !function.contains("Policies"),
        "needs no storage: {function}"
    );
}

#[test]
fn cors_lets_the_page_send_its_session_and_read_the_request_id() {
    let api = block("HttpApi");
    assert!(
        api.contains("AllowHeaders: [authorization, content-type, x-timeline-session]"),
        "{api}"
    );
    assert!(api.contains("ExposeHeaders: [apigw-requestid]"), "{api}");
}

#[test]
fn recording_is_on_by_default_but_only_ever_on_the_dev_stage() {
    let parameter = block("RecordActivity");
    assert!(parameter.contains("Default: \"on\""), "{parameter}");
    assert!(
        parameter.contains("AllowedValues: [\"off\", \"on\"]"),
        "{parameter}"
    );
    assert!(TEMPLATE.contains(
        "RecordsActivity: !And [!Condition IsDev, !Equals [!Ref RecordActivity, \"on\"]]"
    ));
    let outputs = TEMPLATE
        .split("\nOutputs:\n")
        .nth(1)
        .expect("template has outputs");
    assert!(
        outputs.contains("  RecordActivity:\n    Description: Whether the page records activity; read by scripts/write-deploy-config.sh.\n    Value: !If [RecordsActivity, \"on\", \"off\"]"),
        "{outputs}"
    );
}
