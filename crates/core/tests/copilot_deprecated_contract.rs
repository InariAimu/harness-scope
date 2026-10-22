//! Existing-schema compatibility vectors, independent of the native format reader.
use harness_scope_core::{model::*, query, store::Store, sync};

fn records(device: &str) -> Vec<UsageRecord> {
    let mut records: Vec<UsageRecord> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/copliot-deprecated/compatible-records.json"
    ))
    .unwrap();
    for record in &mut records {
        record.device_id = device.into();
    }
    records
}

fn schema(store: &Store) -> Vec<(String, String, String)> {
    store
        .conn
        .prepare("SELECT type,name,sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type,name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn zero_token_requests_keep_counts_and_unknown_optional_fields() {
    let records = records("synthetic-device");
    for record in &records {
        sync::validate_record(record).unwrap();
        assert_eq!(record.harness, "copliot-deprecated");
        assert!(record.timing.is_none());
        assert!(record.request_metadata.is_none());
        assert_eq!(record.provider.as_deref(), Some("github-copliot"));
        assert!(record.status_code.is_none());
        assert!(record.cost.amount_usd.is_none());
        assert_eq!(record.cost.method, "unknown");
    }
    assert_eq!(records[0].tokens.total(), 0);
    let summary = query::summary(&records);
    assert_eq!(summary["totalRequests"], 3);
    assert_eq!(summary["totalInputTokens"], 0);
    assert_eq!(summary["totalOutputTokens"], 20);
    assert_eq!(summary["realTotalTokens"], 20);
    assert_eq!(summary["totalCost"], "0");
    assert_eq!(summary["unpricedCount"], 3);
    assert_eq!(summary["knownStatusCount"], 0);
    assert!(summary["successRate"].is_null());
}

#[test]
fn existing_store_preserves_schema_and_repeat_import_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("compatibility.db");
    let store = Store::open(&path).unwrap();
    let original_schema = schema(&store);
    let original_version = store.schema_version().unwrap();
    let original_device = store.device_id().unwrap();
    let records = records(&original_device);
    for record in &records {
        assert!(store.upsert(record.clone(), "synthetic-source").unwrap());
        assert!(!store.upsert(record.clone(), "synthetic-source").unwrap());
    }
    assert_eq!(store.local_records().unwrap().len(), 3);
    assert_eq!(store.pending_count().unwrap(), 3);
    let (count, input, output): (u64, u64, u64) = store
        .conn
        .query_row(
            "SELECT SUM(request_count),SUM(input_tokens),SUM(output_tokens) FROM usage_index WHERE harness='copliot-deprecated'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((count, input, output), (3, 0, 20));
    assert_eq!(store.schema_version().unwrap(), original_version);
    assert_eq!(schema(&store), original_schema);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.device_id().unwrap(), original_device);
    for record in &records {
        assert!(!reopened.upsert(record.clone(), "synthetic-source").unwrap());
        assert_eq!(reopened.record(&record.id).unwrap().unwrap().revision, 1);
    }
    assert_eq!(reopened.pending_count().unwrap(), 3);
    assert_eq!(schema(&reopened), original_schema);
}

#[test]
fn existing_v4_sync_roundtrips_compatible_records_without_new_wire_fields() {
    let dir = tempfile::tempdir().unwrap();
    let server = Store::open(&dir.path().join("server.db")).unwrap();
    let client = Store::open(&dir.path().join("client.db")).unwrap();
    let records = records("synthetic-source-device");
    let original_schema = schema(&server);
    let request = || PushRequest {
        protocol_version: 4,
        device_id: "synthetic-source-device".into(),
        device_name: "synthetic-source".into(),
        records: records.clone(),
    };
    assert_eq!(sync::push(&server, request()).unwrap().accepted.len(), 3);
    let first = sync::changes(&server, 0, 500).unwrap();
    assert_eq!(first.changes.len(), 3);
    assert_eq!(sync::push(&server, request()).unwrap().accepted.len(), 3);
    assert_eq!(
        sync::changes(&server, 0, 500).unwrap().next_cursor,
        first.next_cursor
    );
    sync::apply_changes(&client, &first).unwrap();
    sync::apply_changes(&client, &first).unwrap();
    let received = client.records("all").unwrap();
    assert_eq!(received.len(), 3);
    for record in records {
        assert_eq!(received.iter().find(|r| r.id == record.id), Some(&record));
    }
    assert_eq!(query::summary(&received)["totalRequests"], 3);
    assert_eq!(schema(&server), original_schema);
}
