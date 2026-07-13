pub mod shared;
pub use shared::*;

pub mod config;
pub mod chat;
pub mod sports;
pub mod predictions;
pub mod kalshi_markets;
pub mod kalshi_analysis;
pub mod kalshi_paper;
pub mod notifications;
pub mod bot;
pub mod analysis_engine;
pub mod fincept;
pub mod breakers;

pub use config::*;
pub use chat::*;
pub use sports::*;
pub use predictions::*;
pub use kalshi_markets::*;
pub use kalshi_analysis::*;
pub use kalshi_paper::*;
pub use notifications::*;
pub use bot::*;
pub use analysis_engine::*;
pub use fincept::*;
pub use breakers::*;

/// Input for generating bet recommendations from the frontend
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct PickInput {
    pub player_name: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub win_probability: f64,
    pub confidence_score: Option<u8>,
}

#[derive(Debug, serde::Serialize)]
pub struct KalshiDashboardBootstrap {
    pub markets: Vec<crate::kalshi::KalshiMarketSummary>,
    pub categories: Vec<crate::kalshi::KalshiCategoryStat>,
    pub cache_status: String,
    pub cache_age_secs: Option<u64>,
    pub partial_catalog: bool,
    pub last_refresh_at: Option<String>,
    pub market_count: usize,
    pub category_count: usize,
    pub dashboard_generated_at: String,
    pub data_quality_notes: Vec<String>,
    #[serde(default)]
    pub ml_phase3: Option<crate::ml_predictor::MLPhase3DashboardSummary>,
}
