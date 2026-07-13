#![allow(unused_imports)]

use crate::chat::kalshi_context;
use crate::edge_engine;
use crate::kalshi::KalshiClient;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;

pub type KalshiState = Arc<KalshiClient>;
pub type SharedCacheState = Arc<RwLock<Option<crate::kalshi::KalshiCache>>>;

pub fn emit_chat_kalshi_context(
    app: &tauri::AppHandle<tauri::Wry>,
    session_id: &str,
    client: &crate::kalshi::KalshiClient,
) {
    let status = kalshi_context::assess_kalshi_chat_context(client);
    let _ = app.emit(
        "chat-kalshi-context",
        serde_json::json!({
            "session_id": session_id,
            "status": status,
        }),
    );
}

pub async fn edge_config_for_pool(db_pool: &Pool<Sqlite>) -> crate::edge_engine::EdgeConfig {
    match crate::edge_engine::persistence::load_edge_config(db_pool).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("edge config load failed, using defaults: {e}");
            crate::edge_engine::EdgeConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KalshiState;
    use crate::kalshi::{KalshiClient, KalshiConfig};
    use std::sync::Arc;

    /// Regression: Tauri state is keyed by TypeId. The alias MUST match the
    /// concrete type passed to `.manage(...)` in `lib.rs` (currently
    /// `Arc<KalshiClient>` — `KalshiClient` uses internal `Arc<RwLock<…>>`).
    /// If someone re-introduces an outer `Mutex<>`, the assignment below
    /// fails to compile, surfacing the mismatch before runtime.
    #[test]
    fn kalshi_state_is_arc_of_client() {
        let client: Arc<KalshiClient> =
            Arc::new(KalshiClient::new(KalshiConfig::default(), None));
        let _: KalshiState = client;
    }
}
