//! Integration tests against a real PostgreSQL.
//!
//! Each test runs in a uniquely-named **ephemeral schema** dropped on teardown, so
//! runs are isolated and repeatable even in parallel.
//!
//! When no database is reachable the tests **skip with a notice** rather than
//! fail, so `cargo test` stays green on a machine without Postgres. That is a
//! deliberate trade: a suite that fails without infrastructure gets ignored, and
//! an ignored suite catches nothing. CI runs a Postgres service, so these do
//! execute where it matters.
//!
//! Point `DATABASE_URL` (or the `DB_*` variables) at a live server to run them
//! locally; `docker/docker-compose.yml` brings one up.

use superquery_chain_api::{FinalityState, Header};
use superquery_config::DbConfig;
use superquery_store::{
    ddl, metadata::keys, parse_entities, CheckpointStore, Database, InitOutcome, MetadataStore,
    PlainModel,
};

const SCHEMA: &str = r#"
    type Transfer @entity {
      id: ID!
      amount: BigInt!
      recipient: String
      blockHeight: Int!
      tags: [String!]
    }
"#;

/// Connect if possible, else print why and skip.
async fn try_db(schema: &str) -> Option<Database> {
    let mut cfg = match std::env::var("DATABASE_URL") {
        Ok(url) => match DbConfig::from_url(&url) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("[skip] DATABASE_URL is not usable ({e})");
                return None;
            }
        },
        Err(_) => DbConfig::from_env(),
    };
    cfg.schema = schema.to_string();

    match Database::connect(&cfg) {
        Ok(db) => match db.ping().await {
            Ok(()) => Some(db),
            Err(e) => {
                eprintln!("[skip] postgres not reachable ({e})");
                None
            }
        },
        Err(e) => {
            eprintln!("[skip] could not build pool ({e})");
            None
        }
    }
}

/// A collision-resistant ephemeral schema name.
fn unique_schema(tag: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("sq_it_{tag}_{nanos}")
}

#[tokio::test]
async fn creates_entity_tables_matching_the_graphql_schema() {
    let schema = unique_schema("ddl");
    let Some(db) = try_db(&schema).await else {
        return;
    };

    db.ensure_schema().await.unwrap();
    let models = parse_entities(SCHEMA).unwrap();
    for statement in ddl::create_tables(&models, &schema).unwrap() {
        db.batch_execute(&statement).await.unwrap();
    }

    let info = db.introspect_schema(&schema).await.unwrap();
    let table = info
        .tables
        .get("transfers")
        .expect("Transfer -> transfers table");

    let columns: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        columns,
        vec!["amount", "block_height", "id", "recipient", "tags"],
        "column names are underscored and introspection is sorted"
    );

    let by = |name: &str| table.columns.iter().find(|c| c.name == name).unwrap();
    assert_eq!(by("id").data_type, "text");
    assert_eq!(by("amount").data_type, "numeric");
    assert_eq!(by("block_height").data_type, "int4");
    assert_eq!(by("tags").data_type, "jsonb");
    assert!(!by("id").is_nullable);
    assert!(by("recipient").is_nullable);

    db.drop_schema(&schema).await.unwrap();
}

#[tokio::test]
async fn entities_round_trip_through_the_store() {
    let schema = unique_schema("entity");
    let Some(db) = try_db(&schema).await else {
        return;
    };

    db.ensure_schema().await.unwrap();
    let models = parse_entities(SCHEMA).unwrap();
    for statement in ddl::create_tables(&models, &schema).unwrap() {
        db.batch_execute(&statement).await.unwrap();
    }

    let model = PlainModel::new(&schema, &models[0]);
    let entity = serde_json::json!({
        "id": "tx-1",
        "amount": "1000000000000000000000000",
        "recipient": "0xalice",
        "blockHeight": 100,
        "tags": ["mint"],
    });
    model.upsert(&db, &[entity]).await.unwrap();

    let row = model.get(&db, "tx-1").await.unwrap().expect("row exists");
    // A u256-scale value must survive exactly; an i64 bind would have overflowed.
    assert_eq!(
        row.get("amount").unwrap().as_deref(),
        Some("1000000000000000000000000")
    );
    assert_eq!(row.get("recipient").unwrap().as_deref(), Some("0xalice"));

    // Upserting the same id updates rather than conflicting — what a handler
    // setting an entity twice in one block relies on.
    let updated = serde_json::json!({
        "id": "tx-1",
        "amount": "42",
        "recipient": serde_json::Value::Null,
        "blockHeight": 101,
        "tags": [],
    });
    model.upsert(&db, &[updated]).await.unwrap();

    let rows = model.dump_canonical(&db).await.unwrap();
    assert_eq!(rows.len(), 1, "upsert must update, not insert a second row");
    assert_eq!(rows[0].get("amount").unwrap().as_deref(), Some("42"));
    assert_eq!(rows[0].get("recipient").unwrap(), &None, "null round-trips");

    model.remove(&db, &["tx-1".to_string()]).await.unwrap();
    assert!(model.get(&db, "tx-1").await.unwrap().is_none());

    db.drop_schema(&schema).await.unwrap();
}

#[tokio::test]
async fn metadata_detects_a_project_or_chain_mismatch() {
    let schema = unique_schema("meta");
    let Some(db) = try_db(&schema).await else {
        return;
    };

    db.ensure_schema().await.unwrap();
    let metadata = MetadataStore::new(&schema).unwrap();
    metadata.ensure_table(&db).await.unwrap();

    // First run stamps identity.
    assert_eq!(
        metadata
            .verify_or_initialize(&db, "erc20-indexer", "1")
            .await
            .unwrap(),
        InitOutcome::Initialized
    );
    // Second run recognises it.
    assert_eq!(
        metadata
            .verify_or_initialize(&db, "erc20-indexer", "1")
            .await
            .unwrap(),
        InitOutcome::Resumed
    );

    // Pointing a different chain at the same schema must fail loudly: silently
    // interleaving two chains' data is the worst outcome available here.
    let wrong_chain = metadata
        .verify_or_initialize(&db, "erc20-indexer", "137")
        .await;
    assert!(wrong_chain.is_err(), "chain mismatch must be refused");

    let wrong_project = metadata
        .verify_or_initialize(&db, "other-project", "1")
        .await;
    assert!(wrong_project.is_err(), "project mismatch must be refused");

    db.drop_schema(&schema).await.unwrap();
}

#[tokio::test]
async fn a_block_and_its_checkpoint_commit_atomically() {
    let schema = unique_schema("ckpt");
    let Some(db) = try_db(&schema).await else {
        return;
    };

    db.ensure_schema().await.unwrap();
    let checkpoints = CheckpointStore::new(&schema).unwrap();
    checkpoints.ensure_tables(&db).await.unwrap();

    assert!(
        checkpoints.load(&db).await.unwrap().is_none(),
        "a fresh schema has no checkpoint"
    );

    let header = Header {
        height: 100,
        hash: "0x64".into(),
        parent_hash: Some("0x63".into()),
        timestamp: None,
    };

    db.transaction(|tx| {
        let checkpoints = CheckpointStore::new(&schema).unwrap();
        let header = header.clone();
        Box::pin(async move {
            checkpoints
                .commit_tx(tx, &header, FinalityState::Final, 100)
                .await
        })
    })
    .await
    .unwrap();

    let checkpoint = checkpoints.load(&db).await.unwrap().expect("committed");
    assert_eq!(checkpoint.indexed.height, 100);
    assert_eq!(checkpoint.indexed.hash, "0x64");
    assert_eq!(checkpoint.finalized_height, 100);

    // A failing transaction must leave no trace — this is Milestone 10's
    // guarantee that a killed process never leaves a half-indexed block.
    let failed: Result<(), _> = db
        .transaction(|tx| {
            let checkpoints = CheckpointStore::new(&schema).unwrap();
            Box::pin(async move {
                let header = Header {
                    height: 101,
                    hash: "0x65".into(),
                    parent_hash: Some("0x64".into()),
                    timestamp: None,
                };
                checkpoints
                    .commit_tx(tx, &header, FinalityState::Unfinalized, 100)
                    .await?;
                Err(superquery_store::StoreError::Decode(
                    "simulated failure".into(),
                ))
            })
        })
        .await;
    assert!(failed.is_err());

    let after = checkpoints.load(&db).await.unwrap().unwrap();
    assert_eq!(
        after.indexed.height, 100,
        "a rolled-back block must not advance the checkpoint"
    );
    assert!(
        checkpoints.header_at(&db, 101).await.unwrap().is_none(),
        "a rolled-back block must leave no header row"
    );

    db.drop_schema(&schema).await.unwrap();
}

#[tokio::test]
async fn stored_headers_support_the_reorg_walk() {
    let schema = unique_schema("reorg");
    let Some(db) = try_db(&schema).await else {
        return;
    };

    db.ensure_schema().await.unwrap();
    let checkpoints = CheckpointStore::new(&schema).unwrap();
    checkpoints.ensure_tables(&db).await.unwrap();

    for height in 100..=104u64 {
        let header = Header {
            height,
            hash: format!("0x{height:x}"),
            parent_hash: Some(format!("0x{:x}", height - 1)),
            timestamp: None,
        };
        db.transaction(|tx| {
            let checkpoints = CheckpointStore::new(&schema).unwrap();
            let header = header.clone();
            Box::pin(async move {
                checkpoints
                    .commit_tx(tx, &header, FinalityState::Unfinalized, 99)
                    .await
            })
        })
        .await
        .unwrap();
    }

    // Highest first — the order the common-ancestor search walks.
    let descending = checkpoints.headers_descending(&db, 104, 100).await.unwrap();
    assert_eq!(
        descending.iter().map(|h| h.height).collect::<Vec<_>>(),
        vec![104, 103, 102, 101, 100]
    );
    assert_eq!(descending[0].parent_hash.as_deref(), Some("0x67"));

    // Truncating above the ancestor is the store half of a rewind.
    db.transaction(|tx| {
        let checkpoints = CheckpointStore::new(&schema).unwrap();
        Box::pin(async move { checkpoints.truncate_above_tx(tx, 101).await })
    })
    .await
    .unwrap();

    let remaining = checkpoints.headers_descending(&db, 104, 100).await.unwrap();
    assert_eq!(
        remaining.iter().map(|h| h.height).collect::<Vec<_>>(),
        vec![101, 100]
    );

    db.drop_schema(&schema).await.unwrap();
}

#[tokio::test]
async fn metadata_keys_are_readable_as_typed_values() {
    let schema = unique_schema("typed");
    let Some(db) = try_db(&schema).await else {
        return;
    };

    db.ensure_schema().await.unwrap();
    let metadata = MetadataStore::new(&schema).unwrap();
    metadata.ensure_table(&db).await.unwrap();

    metadata
        .set(&db, keys::INDEXED_HEIGHT, "18000000")
        .await
        .unwrap();
    assert_eq!(
        metadata.get_u64(&db, keys::INDEXED_HEIGHT).await.unwrap(),
        Some(18_000_000)
    );
    assert_eq!(metadata.get_u64(&db, "never_written").await.unwrap(), None);

    // A non-numeric value must surface as an error, not a silent zero.
    metadata
        .set(&db, keys::FINALIZED_HEIGHT, "not-a-number")
        .await
        .unwrap();
    assert!(metadata.get_u64(&db, keys::FINALIZED_HEIGHT).await.is_err());

    db.drop_schema(&schema).await.unwrap();
}
