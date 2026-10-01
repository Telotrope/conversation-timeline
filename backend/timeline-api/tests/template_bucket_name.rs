//! The uploads bucket's name is written out as text in several places in
//! `infra/template.yaml`, not referred to with `!Ref`: a `!Ref` from the
//! processing function's permissions to the bucket that triggers it is a
//! circular dependency CloudFormation refuses (migration plan §V2e, E2;
//! cfn-lint reports it as E3004). Nothing links those copies, and
//! `sam validate --lint` can't tell if one drifts: the deployment would
//! succeed and uploads would then fail with "access denied". This checks
//! every copy matches the bucket's own name.
//!
//! A plain text check, crude in the way the plan's C16 describes
//! (CloudFormation's `!Sub` tags stop ordinary YAML readers).

const TEMPLATE: &str = include_str!("../../../infra/template.yaml");

/// The value after `key:` on every line that has it, in order.
fn values_of(key: &str) -> Vec<&'static str> {
    TEMPLATE
        .lines()
        .filter_map(|line| line.trim().strip_prefix(key))
        .filter_map(|rest| rest.strip_prefix(':'))
        .map(str::trim)
        .collect()
}

/// The bucket resource's own `BucketName`, from the `RawUploadsBucket`
/// block.
fn bucket_name() -> &'static str {
    let block = TEMPLATE
        .split("\n  RawUploadsBucket:\n")
        .nth(1)
        .expect("template declares RawUploadsBucket");
    block
        .lines()
        .find_map(|line| line.trim().strip_prefix("BucketName:"))
        .map(str::trim)
        .expect("RawUploadsBucket has a BucketName")
}

#[test]
fn the_bucket_name_is_the_fixed_pattern() {
    assert_eq!(
        bucket_name(),
        r#"!Sub "timeline-uploads-${Stage}-${AWS::AccountId}""#
    );
}

#[test]
fn every_function_setting_names_the_bucket_exactly_as_the_bucket_does() {
    let settings = values_of("TIMELINE_UPLOADS_BUCKET");
    // The API function and the processing function.
    assert_eq!(settings.len(), 2, "{settings:?}");
    for value in settings {
        assert_eq!(value, bucket_name());
    }
}

#[test]
fn every_permission_names_the_bucket_exactly_as_the_bucket_does() {
    // The bucket's own BucketName plus the API function's S3CrudPolicy and
    // the processing function's S3ReadPolicy.
    let names = values_of("BucketName");
    assert_eq!(names.len(), 3, "{names:?}");
    for value in names {
        assert_eq!(
            value,
            bucket_name(),
            "a permission names a different bucket"
        );
    }
}
