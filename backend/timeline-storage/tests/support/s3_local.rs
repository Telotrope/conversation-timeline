//! A local S3 stand-in for tests: `s3s-fs` (Apache-2.0) served over real
//! HTTP on a free port inside the test process, storing objects in a
//! temporary folder. See the migration plan's §V2b for why this one was
//! chosen: it checks request signatures, including presigned URLs, so a
//! wrongly-signed URL from `S3ObjectStore` fails here instead of only on
//! real S3.
//!
//! Bucket names go in the URL path (`http://127.0.0.1:port/bucket/key`),
//! because there's no DNS for `bucket.127.0.0.1`. Real S3 normally puts the
//! bucket in the host name, so that addressing form is only exercised by
//! the real-AWS run in the plan's §V2.

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnBuilder;
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use s3s_fs::FileSystem;
use timeline_storage::s3::S3ObjectStore;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub const ACCESS_KEY: &str = "test-access-key";
pub const SECRET_KEY: &str = "test-secret-key";
pub const REGION: &str = "us-east-1";
pub const BUCKET: &str = "timeline-test";

/// Keeps the server and its storage folder alive; both go away on drop.
pub struct LocalS3 {
    pub client: aws_sdk_s3::Client,
    _root: tempfile::TempDir,
    server: JoinHandle<()>,
}

impl Drop for LocalS3 {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl LocalS3 {
    /// Starts a fresh server with one empty bucket, [`BUCKET`].
    pub async fn start() -> Self {
        let root = tempfile::tempdir().expect("create temp folder for s3s-fs");
        let fs = FileSystem::new(root.path()).expect("open s3s-fs root");
        let mut builder = S3ServiceBuilder::new(fs);
        builder.set_auth(SimpleAuth::from_single(ACCESS_KEY, SECRET_KEY));
        let service = builder.build();

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a free local port");
        let addr = listener.local_addr().expect("read bound address");
        let server = tokio::spawn(async move {
            let http = ConnBuilder::new(TokioExecutor::new());
            loop {
                let (socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(e) => panic!("local S3 stand-in stopped accepting connections: {e}"),
                };
                let conn = http
                    .serve_connection(TokioIo::new(socket), service.clone())
                    .into_owned();
                tokio::spawn(async move {
                    if let Err(e) = conn.await {
                        eprintln!("local S3 stand-in connection error: {e}");
                    }
                });
            }
        });

        let client = client_for(&format!("http://{addr}"));
        client
            .create_bucket()
            .bucket(BUCKET)
            .send()
            .await
            .expect("create the test bucket");

        Self {
            client,
            _root: root,
            server,
        }
    }

    pub fn object_store(&self) -> S3ObjectStore {
        S3ObjectStore::new(self.client.clone(), BUCKET)
    }
}

/// A client with the test credentials, pointed at any endpoint -- used
/// directly to aim at an address where nothing is listening.
pub fn client_for(endpoint: &str) -> aws_sdk_s3::Client {
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(ACCESS_KEY, SECRET_KEY, None, None, "test"))
        .region(Region::new(REGION))
        .endpoint_url(endpoint)
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(config)
}
