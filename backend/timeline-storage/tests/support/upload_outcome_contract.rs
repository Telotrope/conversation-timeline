//! What any correct `UploadOutcomeStore` must do, run against the in-memory
//! fake and `DynamoConversationsTable`. See the migration plan's §V2b.
//!
//! `$make` is an async fn returning `(store, keep_alive)`.

macro_rules! upload_outcome_contract {
    ($make:path) => {
        use timeline_core::model::ConversationId as ContractConversationId;
        use timeline_core::ports::ids::{UploadId as ContractUploadId, UserId as ContractUserId};
        use timeline_core::ports::uploads::{
            UploadOutcome as ContractUploadOutcome,
            UploadOutcomeStore as ContractUploadOutcomeStore,
        };

        fn contract_user(name: &str) -> ContractUserId {
            ContractUserId(name.to_string())
        }

        fn contract_upload() -> ContractUploadId {
            ContractUploadId(uuid::Uuid::from_u128(100))
        }

        #[tokio::test]
        async fn outcome_before_any_write_is_none() {
            let (store, _keep) = $make().await;
            let got = store
                .get_outcome(&contract_user("alice"), contract_upload())
                .await
                .unwrap();
            assert_eq!(got, None);
        }

        #[tokio::test]
        async fn a_ready_outcome_round_trips_with_ids_in_order() {
            let (store, _keep) = $make().await;
            let outcome = ContractUploadOutcome::Ready {
                conversation_ids: vec![
                    ContractConversationId(uuid::Uuid::from_u128(3)),
                    ContractConversationId(uuid::Uuid::from_u128(1)),
                    ContractConversationId(uuid::Uuid::from_u128(2)),
                ],
            };
            store
                .record_outcome(&contract_user("alice"), contract_upload(), outcome.clone())
                .await
                .unwrap();
            let got = store
                .get_outcome(&contract_user("alice"), contract_upload())
                .await
                .unwrap();
            assert_eq!(got, Some(outcome));
        }

        #[tokio::test]
        async fn a_ready_outcome_with_no_conversations_round_trips() {
            let (store, _keep) = $make().await;
            let outcome = ContractUploadOutcome::Ready {
                conversation_ids: vec![],
            };
            store
                .record_outcome(&contract_user("alice"), contract_upload(), outcome.clone())
                .await
                .unwrap();
            let got = store
                .get_outcome(&contract_user("alice"), contract_upload())
                .await
                .unwrap();
            assert_eq!(got, Some(outcome));
        }

        #[tokio::test]
        async fn a_failed_outcome_round_trips_with_its_reason() {
            let (store, _keep) = $make().await;
            let outcome = ContractUploadOutcome::Failed {
                reason: "not a conversations export: unexpected \"token\" at line 1 — ✗"
                    .to_string(),
            };
            store
                .record_outcome(&contract_user("alice"), contract_upload(), outcome.clone())
                .await
                .unwrap();
            let got = store
                .get_outcome(&contract_user("alice"), contract_upload())
                .await
                .unwrap();
            assert_eq!(got, Some(outcome));
        }

        #[tokio::test]
        async fn recording_again_replaces_the_earlier_outcome() {
            let (store, _keep) = $make().await;
            let first = ContractUploadOutcome::Failed {
                reason: "first".to_string(),
            };
            let second = ContractUploadOutcome::Ready {
                conversation_ids: vec![ContractConversationId(uuid::Uuid::from_u128(9))],
            };
            store
                .record_outcome(&contract_user("alice"), contract_upload(), first)
                .await
                .unwrap();
            store
                .record_outcome(&contract_user("alice"), contract_upload(), second.clone())
                .await
                .unwrap();
            let got = store
                .get_outcome(&contract_user("alice"), contract_upload())
                .await
                .unwrap();
            assert_eq!(got, Some(second));
        }

        #[tokio::test]
        async fn outcomes_are_isolated_per_user() {
            let (store, _keep) = $make().await;
            store
                .record_outcome(
                    &contract_user("alice"),
                    contract_upload(),
                    ContractUploadOutcome::Failed {
                        reason: "x".to_string(),
                    },
                )
                .await
                .unwrap();
            let got = store
                .get_outcome(&contract_user("bob"), contract_upload())
                .await
                .unwrap();
            assert_eq!(got, None);
        }
    };
}
