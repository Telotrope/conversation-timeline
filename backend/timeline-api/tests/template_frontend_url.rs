//! The template's `FrontendUrl` (migration plan §V2e, E10): the page's full
//! address, so a page served under a path (VS Code's forwarding puts it at
//! `/proxy/8000/timeline.html`) can be Cognito's return address. Cognito
//! gets the address exactly; CORS on the API and the bucket gets its origin,
//! cut out of it by one expression written identically in every place.
//!
//! A stack with `HostPage=on` (docs/plans/2026-10-02-page-hosting.md §1)
//! accepts the hosted page at `https://<host>/` instead, and `FrontendUrl`
//! too only with `AlsoAllowLocalPage=on`. CloudFormation has no variables,
//! so each address list is written out in full in each place; these checks
//! keep the copies identical.
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

/// The hosted page's address: `PageDomain` when set, otherwise the
/// distribution's own domain. `HOSTED_ORIGIN` is the same without the `/`.
const HOSTED_URL: &str = "!Sub [\"https://${Host}/\", {Host: !If [HasPageDomain, !Ref PageDomain, \
                          !GetAtt PageDistribution.DomainName]}]";
const HOSTED_ORIGIN: &str =
    "!Sub [\"https://${Host}\", {Host: !If [HasPageDomain, !Ref PageDomain, \
                             !GetAtt PageDistribution.DomainName]}]";

/// `key`'s value at `indent` spaces: not hosted, only `local`; hosted, only
/// `hosted`, plus `local` with `AlsoAllowLocalPage`.
fn address_list(indent: usize, key: &str, hosted: &str, local: &str) -> String {
    let pad = " ".repeat(indent);
    format!(
        "{pad}{key}: !If\n\
         {pad}  - HostingPage\n\
         {pad}  - !If\n\
         {pad}    - AllowsLocalPage\n\
         {pad}    - - {hosted}\n\
         {pad}      - {local}\n\
         {pad}    - - {hosted}\n\
         {pad}  - - {local}\n"
    )
}

#[test]
fn hosting_is_off_unless_asked_for() {
    assert!(block("HostPage").contains("Default: \"off\""));
    assert!(block("AlsoAllowLocalPage").contains("Default: \"off\""));
}

#[test]
fn cognito_returns_to_exactly_the_page_address() {
    let client = block("UserPoolClient");
    for key in ["CallbackURLs", "LogoutURLs"] {
        let expected = address_list(6, key, HOSTED_URL, "!Ref FrontendUrl");
        assert!(
            client.contains(&expected),
            "{key}:\n{expected}\nin:\n{client}"
        );
    }
}

#[test]
fn the_api_and_the_bucket_allow_the_same_origin_cut_from_the_page_address() {
    let api = address_list(8, "AllowOrigins", HOSTED_ORIGIN, ORIGIN);
    assert!(block("HttpApi").contains(&api), "API:\n{api}");
    let bucket = address_list(12, "AllowedOrigins", HOSTED_ORIGIN, ORIGIN);
    assert!(
        block("RawUploadsBucket").contains(&bucket),
        "bucket:\n{bucket}"
    );
    // Two copies in each of the two lists, and none anywhere else.
    assert_eq!(TEMPLATE.matches(ORIGIN).count(), 4);
    assert_eq!(TEMPLATE.matches(HOSTED_ORIGIN).count(), 4);
}

#[test]
fn the_hosted_address_is_written_identically_everywhere() {
    // Two copies in each of Cognito's two lists, plus the PageUrl output.
    assert_eq!(TEMPLATE.matches(HOSTED_URL).count(), 5);
    assert!(block("PageUrl").contains(&format!("Value: {HOSTED_URL}")));
}

#[test]
fn a_custom_domain_uses_the_certificate_it_is_given_and_makes_none() {
    // The hosting plan's C14: one certificate, made by hand, for every
    // redeploy and stack.
    assert!(!TEMPLATE.contains("AWS::CertificateManager::Certificate"));
    assert!(block("PageDistribution").contains("AcmCertificateArn: !Ref PageCertificateArn\n"));
    assert!(block("PageCertificateArn").contains(
        r"AllowedPattern: '^$|^arn:aws:acm:us-east-1:[0-9]{12}:certificate/[0-9a-f-]+$'"
    ));
    // Neither setting without the other.
    let domain_rule = block("PageDomainNeedsHostingAndCertificate");
    assert!(domain_rule.contains("RuleCondition: !Not [!Equals [!Ref PageDomain, \"\"]]"));
    assert!(domain_rule.contains("Assert: !Not [!Equals [!Ref PageCertificateArn, \"\"]]"));
    assert!(domain_rule.contains("Assert: !Equals [!Ref HostPage, \"on\"]"));
    let certificate_rule = block("PageCertificateNeedsDomain");
    assert!(
        certificate_rule.contains("RuleCondition: !Not [!Equals [!Ref PageCertificateArn, \"\"]]")
    );
    assert!(certificate_rule.contains("Assert: !Not [!Equals [!Ref PageDomain, \"\"]]"));
}
