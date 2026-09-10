//! Real DynamoDB-backed adapters. Compile-checked and unit-tested for their
//! own key/expression-building logic (no live connection needed for that),
//! but not yet run against LocalStack or real DynamoDB -- no Docker or AWS
//! credentials were available in this environment. See the migration plan
//! V2 test list.

pub mod conversations_table;
pub mod message_flags_table;
