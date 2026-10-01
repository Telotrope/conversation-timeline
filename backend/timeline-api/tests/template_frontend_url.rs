//! The template's `FrontendUrl` (migration plan §V2e, E10): the page's full
//! address, so a page served under a path (VS Code's forwarding puts it at
//! `/proxy/8000/timeline.html`) can be Cognito's return address. Cognito
//! gets the address exactly; CORS on the API and the bucket gets its origin,
//! cut out of it by one expression written identically in both places.
//!
//! Plain text checks, crude in the way the plan's C16 describes. Whether
//! CloudFormation evaluates the expression as intended is checked only on
//! AWS (deployment checks D2 and D3).

const TEMPLATE: &str = include_str!("../../../infra/template.yaml");

/// The origin expression: the first piece of the address split on `/`
/// (`https:`), then `//`, then the third piece (the host and port; the
/// second is the empty piece between the two slashes).
const ORIGIN: &str = "!Join [\"\", [!Select [0, !Split [\"/\", !Ref FrontendUrl]], \"//\", \
                      !Select [2, !Split [\"/\", !Ref FrontendUrl]]]]";

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
fn the_page_address_defaults_to_localhost_and_must_end_in_timeline_html() {
    let parameter = block("FrontendUrl");
    assert!(
        parameter.contains("Default: http://localhost:8000/timeline.html"),
        "{parameter}"
    );
    assert!(
        parameter.contains(r"AllowedPattern: '^https?://[^/]+(/[^?#]*)?/timeline\.html$'"),
        "{parameter}"
    );
}

#[test]
fn the_old_origin_parameter_is_gone() {
    assert!(!TEMPLATE.contains("FrontendOrigin"));
}

#[test]
fn cognito_returns_to_exactly_the_page_address() {
    let client = block("UserPoolClient");
    assert!(
        client.contains("CallbackURLs:\n        - !Ref FrontendUrl\n"),
        "{client}"
    );
    assert!(
        client.contains("LogoutURLs:\n        - !Ref FrontendUrl\n"),
        "{client}"
    );
}

#[test]
fn the_api_and_the_bucket_allow_the_same_origin_cut_from_the_page_address() {
    assert!(
        block("HttpApi").contains(&format!("AllowOrigins:\n          - {ORIGIN}\n")),
        "API"
    );
    assert!(
        block("RawUploadsBucket").contains(&format!("AllowedOrigins:\n              - {ORIGIN}\n")),
        "bucket"
    );
    assert_eq!(TEMPLATE.matches(ORIGIN).count(), 2);
}
