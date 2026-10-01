//! The Lambda's AWS settings, read once at startup into typed values. See
//! the migration plan's §V2d.
//!
//! Each name gets its own small type, so a table name can't be passed where
//! a bucket name belongs. A missing or empty variable is an error naming it,
//! and every problem is reported at once rather than one per restart.
//!
//! The variables are set by `infra/template.yaml`, except `AWS_REGION`,
//! which Lambda sets itself. The upload-processing Lambda reads only
//! [`StorageSettings`]; the API Lambda reads [`AwsSettings`] (migration plan
//! §V2e).

use std::fmt;

macro_rules! name_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

name_type!(
    /// The S3 bucket holding raw uploads and generated exports.
    BucketName
);
name_type!(
    /// A DynamoDB table's name.
    TableName
);
name_type!(
    /// A Cognito user pool's id, such as `us-east-1_AbCdEf123`.
    UserPoolId
);
name_type!(
    /// The Cognito app client id that tokens must be issued for.
    ClientId
);
name_type!(
    /// An AWS region, such as `us-east-1`.
    Region
);

pub const UPLOADS_BUCKET_VAR: &str = "TIMELINE_UPLOADS_BUCKET";
pub const CONVERSATIONS_TABLE_VAR: &str = "TIMELINE_CONVERSATIONS_TABLE";
pub const MESSAGE_FLAGS_TABLE_VAR: &str = "TIMELINE_MESSAGE_FLAGS_TABLE";
pub const USER_POOL_ID_VAR: &str = "TIMELINE_COGNITO_USER_POOL_ID";
pub const CLIENT_ID_VAR: &str = "TIMELINE_COGNITO_CLIENT_ID";
pub const REGION_VAR: &str = "AWS_REGION";

/// The three storage names, which are all the upload-processing Lambda
/// needs. The API Lambda reads these plus the Cognito settings
/// ([`AwsSettings`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSettings {
    pub uploads_bucket: BucketName,
    pub conversations_table: TableName,
    pub message_flags_table: TableName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsSettings {
    pub uploads_bucket: BucketName,
    pub conversations_table: TableName,
    pub message_flags_table: TableName,
    pub user_pool_id: UserPoolId,
    pub client_id: ClientId,
    pub region: Region,
}

/// Every variable that was missing or empty, in the order they're read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingSettings(pub Vec<&'static str>);

impl fmt::Display for MissingSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "missing or empty environment variables: {}",
            self.0.join(", ")
        )
    }
}

impl std::error::Error for MissingSettings {}

/// Reads variables through a lookup function, remembering every one that
/// was missing or empty so they can all be reported together.
struct Reader<F> {
    lookup: F,
    missing: Vec<&'static str>,
}

impl<F: Fn(&str) -> Option<String>> Reader<F> {
    fn new(lookup: F) -> Self {
        Reader {
            lookup,
            missing: Vec::new(),
        }
    }

    fn read(&mut self, name: &'static str) -> String {
        match (self.lookup)(name) {
            Some(value) if !value.trim().is_empty() => value,
            _ => {
                self.missing.push(name);
                String::new()
            }
        }
    }

    fn storage(&mut self) -> StorageSettings {
        StorageSettings {
            uploads_bucket: BucketName(self.read(UPLOADS_BUCKET_VAR)),
            conversations_table: TableName(self.read(CONVERSATIONS_TABLE_VAR)),
            message_flags_table: TableName(self.read(MESSAGE_FLAGS_TABLE_VAR)),
        }
    }

    fn finish<T>(self, value: T) -> Result<T, MissingSettings> {
        if self.missing.is_empty() {
            Ok(value)
        } else {
            Err(MissingSettings(self.missing))
        }
    }
}

impl StorageSettings {
    /// Reads the bucket and table names through `lookup`, as
    /// [`AwsSettings::from_lookup`] does.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, MissingSettings> {
        let mut reader = Reader::new(lookup);
        let storage = reader.storage();
        reader.finish(storage)
    }
}

impl AwsSettings {
    /// Reads every setting through `lookup` -- `std::env::var` in the
    /// Lambda; any function in tests, so they never touch the real
    /// environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, MissingSettings> {
        let mut reader = Reader::new(lookup);
        let storage = reader.storage();
        let settings = AwsSettings {
            uploads_bucket: storage.uploads_bucket,
            conversations_table: storage.conversations_table,
            message_flags_table: storage.message_flags_table,
            user_pool_id: UserPoolId(reader.read(USER_POOL_ID_VAR)),
            client_id: ClientId(reader.read(CLIENT_ID_VAR)),
            region: Region(reader.read(REGION_VAR)),
        };
        reader.finish(settings)
    }

    /// The storage names alone.
    pub fn storage(&self) -> StorageSettings {
        StorageSettings {
            uploads_bucket: self.uploads_bucket.clone(),
            conversations_table: self.conversations_table.clone(),
            message_flags_table: self.message_flags_table.clone(),
        }
    }

    /// The `iss` claim Cognito puts in this pool's tokens.
    pub fn issuer(&self) -> String {
        format!(
            "https://cognito-idp.{}.amazonaws.com/{}",
            self.region, self.user_pool_id
        )
    }

    /// Where Cognito publishes this pool's public signing keys.
    pub fn jwks_url(&self) -> String {
        format!("{}/.well-known/jwks.json", self.issuer())
    }
}
