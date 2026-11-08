//! Integration tests for the embedded VelocityDB Studio HTTP & WebSocket Server (Axum + Tokio)

use std::sync::Arc;
use tokio::sync::RwLock;
use velocitydb::studio_api::{
    create_studio_router, KeyMetadataDto, StudioKeyStore, StudioState, TelemetryData,
};

#[tokio::test]
async fn test_studio_telemetry_defaults_and_mutation() {
    let telemetry = Arc::new(RwLock::new(TelemetryData::default()));
    let key_store = Arc::new(RwLock::new(StudioKeyStore::new_with_defaults()));

    let state = StudioState {
        telemetry: telemetry.clone(),
        key_store: key_store.clone(),
    };

    // Verify initial telemetry
    {
        let t = state.telemetry.read().await;
        assert_eq!(t.cluster_state, "ok");
        assert_eq!(t.cluster_slots_assigned, 16384);
        assert_eq!(t.rps, 1642890);
        assert_eq!(t.cpu_cores.len(), 16);
    }

    // Mutate telemetry state (simulate 1.6M+ RPS engine load)
    {
        let mut t = state.telemetry.write().await;
        t.rps = 1685420;
        t.p99_latency_micros = 17.4;
        t.eviction_count += 100;
    }

    // Assert mutation visible through shared state
    {
        let t = state.telemetry.read().await;
        assert_eq!(t.rps, 1685420);
        assert_eq!(t.p99_latency_micros, 17.4);
        assert_eq!(t.eviction_count, 43840);
    }
}

#[tokio::test]
async fn test_studio_key_store_crud_lifecycle() {
    let key_store = Arc::new(RwLock::new(StudioKeyStore::new_with_defaults()));

    // 1. Initial defaults
    {
        let store = key_store.read().await;
        assert!(store.keys.contains_key("{tenant:101}session:user_token:9941"));
        assert_eq!(store.keys.len(), 6);
    }

    // 2. Insert new key with CRC16 slot calculation
    let test_key = "{tenant:999}payment:tx_4821";
    let slot = velocitydb::db::key_slot(test_key.as_bytes());
    let target_node = if slot < 5462 {
        "node-01:6379"
    } else if slot < 10923 {
        "node-02:6380"
    } else {
        "node-03:6381"
    };

    let record = KeyMetadataDto {
        key: test_key.to_string(),
        key_type: "string".to_string(),
        slot,
        target_node: target_node.to_string(),
        ttl_seconds: Some(3600),
        size_bytes: 48,
        value: "{\"status\":\"approved\"}".to_string(),
        last_accessed: "Just now".to_string(),
    };

    {
        let mut store = key_store.write().await;
        store.keys.insert(test_key.to_string(), record);
        assert_eq!(store.keys.len(), 7);
    }

    // 3. Update key value and TTL
    {
        let mut store = key_store.write().await;
        let entry = store.keys.get_mut(test_key).expect("Key must exist");
        entry.value = "{\"status\":\"settled\"}".to_string();
        entry.ttl_seconds = None; // Make persistent
    }

    // 4. Assert update
    {
        let store = key_store.read().await;
        let entry = store.keys.get(test_key).expect("Key must exist");
        assert_eq!(entry.value, "{\"status\":\"settled\"}");
        assert_eq!(entry.ttl_seconds, None);
    }

    // 5. Delete key
    {
        let mut store = key_store.write().await;
        let removed = store.keys.remove(test_key);
        assert!(removed.is_some());
        assert_eq!(store.keys.len(), 6);
    }
}

#[tokio::test]
async fn test_studio_router_construction() {
    let telemetry = Arc::new(RwLock::new(TelemetryData::default()));
    let key_store = Arc::new(RwLock::new(StudioKeyStore::new_with_defaults()));

    let state = StudioState {
        telemetry,
        key_store,
    };

    // Verify router builds cleanly without panic
    let _router = create_studio_router(state);
}
