//! Amazon's DynamoDB Local, started once per test binary and shared by
//! every test in it. Each test creates its own uniquely-named tables, so
//! tests never see each other's data. See the migration plan's §V2b.
//!
//! **Missing Java or a missing JAR fails the test; it is never skipped.**
//! Skipping quietly is how these adapters went untested for weeks (plan
//! C17). The panic message names the setup steps.
//!
//! The JVM is wrapped in `sh` so it dies with the test binary: the wrapper
//! starts java in the background, then blocks reading its own stdin, which
//! is a pipe held open by this process. When the test binary exits for any
//! reason, the OS closes that pipe, `cat` returns, and the wrapper kills
//! java. A `static` is never dropped, so a `Drop` impl couldn't do this.
//!
//! Launched with `-disableTelemetry`, since DynamoDB Local sends usage data
//! by default (plan C15).

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_dynamodb::types::{
    AttributeDefinition, BillingMode, KeySchemaElement, KeyType, ScalarAttributeType,
};

const SETUP_HELP: &str = "DynamoDB tests need Java 17+ and DynamoDB Local. One-time setup:\n  \
    sudo apt install -y openjdk-21-jre-headless\n  \
    scripts/fetch-dynamodb-local.sh\n\
    See docs/plans/2026-09-09-rust-aws-backend-migration.md, section V2b.";

struct Server {
    addr: SocketAddr,
    // Held only so the wrapper's stdin pipe stays open until this process exits.
    _wrapper: Mutex<Child>,
}

static SERVER: OnceLock<Server> = OnceLock::new();

fn jar_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.tools/dynamodb-local")
}

fn free_port() -> u16 {
    // Bind-then-release: another process could take the port in the gap
    // before java binds it. Startup then fails loudly via the readiness
    // timeout below, rather than tests silently talking to something else.
    let listener = TcpListener::bind("127.0.0.1:0").expect("find a free local port");
    listener.local_addr().expect("read bound address").port()
}

fn start() -> Server {
    match Command::new("java").arg("-version").output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => panic!(
            "`java -version` failed ({}): {}\n{SETUP_HELP}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(e) => panic!("Java is not installed or not on PATH ({e}).\n{SETUP_HELP}"),
    }
    let dir = jar_dir();
    if !dir.join("DynamoDBLocal.jar").is_file() {
        panic!(
            "DynamoDB Local is missing from {}.\n{SETUP_HELP}",
            dir.display()
        );
    }

    let port = free_port();
    let script = format!(
        "java -Djava.library.path=./DynamoDBLocal_lib -jar DynamoDBLocal.jar \
         -inMemory -disableTelemetry -port {port} >/dev/null 2>&1 & \
         j=$!; cat >/dev/null; kill $j"
    );
    let wrapper = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("could not start DynamoDB Local: {e}\n{SETUP_HELP}"));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_err() {
        if Instant::now() > deadline {
            panic!("DynamoDB Local did not start listening on {addr} within 20 seconds");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Server {
        addr,
        _wrapper: Mutex::new(wrapper),
    }
}

/// A client pointed at the shared DynamoDB Local, starting it if needed.
pub fn client() -> aws_sdk_dynamodb::Client {
    let server = SERVER.get_or_init(start);
    let config = aws_sdk_dynamodb::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .region(Region::new("us-east-1"))
        .endpoint_url(format!("http://{}", server.addr))
        .build();
    aws_sdk_dynamodb::Client::from_conf(config)
}

/// Creates a new, empty table with a unique name and returns that name.
///
/// The key layout is copied from infra/template.yaml's `ConversationsTable`
/// and `MessageFlagsTable` (both `pk` HASH + `sk` RANGE, both strings), not
/// read from it: the template uses CloudFormation tags such as `!Sub` that
/// ordinary YAML readers reject. If the template's key layout changes,
/// change it here too (plan C16).
pub async fn create_table(client: &aws_sdk_dynamodb::Client) -> String {
    let name = format!("t-{}", uuid::Uuid::new_v4());
    let string_attr = |n: &str| {
        AttributeDefinition::builder()
            .attribute_name(n)
            .attribute_type(ScalarAttributeType::S)
            .build()
            .expect("valid attribute definition")
    };
    let key = |n: &str, t: KeyType| {
        KeySchemaElement::builder()
            .attribute_name(n)
            .key_type(t)
            .build()
            .expect("valid key schema element")
    };
    client
        .create_table()
        .table_name(&name)
        .attribute_definitions(string_attr("pk"))
        .attribute_definitions(string_attr("sk"))
        .key_schema(key("pk", KeyType::Hash))
        .key_schema(key("sk", KeyType::Range))
        .billing_mode(BillingMode::PayPerRequest)
        .send()
        .await
        .unwrap_or_else(|e| panic!("create test table {name}: {e:?}"));
    name
}
