//! The template's `LogS3Events` switch (migration plan §V2e, E9): off by
//! default, only `"off"` or `"on"`, and wired to the processing function
//! alone, so the API function, which receives login tokens, never logs
//! requests through it.
//!
//! Plain text checks, crude in the way the plan's C16 describes. The values
//! must stay quoted: unquoted `off` and `on` are booleans to some YAML
//! readers, which the Lambda would then receive as `false` or `true` and
//! refuse to start with.

const TEMPLATE: &str = include_str!("../../../infra/template.yaml");

/// The indented block under `  <name>:` (two-space resources and
/// parameters), up to the next line at that indentation or less.
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

#[test]
fn the_switch_is_off_by_default_and_takes_only_quoted_off_or_on() {
    let parameter = block("LogS3Events");
    assert!(parameter.contains("Default: \"off\""), "{parameter}");
    assert!(
        parameter.contains("AllowedValues: [\"off\", \"on\"]"),
        "{parameter}"
    );
}

#[test]
fn only_the_processing_function_reads_the_switch() {
    let setting = "TIMELINE_LOG_S3_EVENTS: !Ref LogS3Events";
    assert!(block("ProcessUploadFunction").contains(setting));
    assert!(!block("ApiFunction").contains("TIMELINE_LOG_S3_EVENTS"));
    assert_eq!(TEMPLATE.matches("TIMELINE_LOG_S3_EVENTS").count(), 1);
}
