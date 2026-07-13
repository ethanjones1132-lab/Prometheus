pub mod models;
pub mod client;
pub mod grading;
pub mod portfolio_risk;
pub mod price_tracker;
pub mod market_cache_store;
pub mod forecast;

pub use models::*;
pub use client::{KalshiClient, KalshiCategoryStat, kalshi_config_from_app};

pub use grading::{evaluate_bet, grade_pending_predictions, spawn_auto_grade_task};
pub use portfolio_risk::{
    compute_stake_adjustment, exposures_from_positions, exposures_from_predictions,
    StakeAdjustment, PortfolioExposure,
};
pub use price_tracker::{get_price_history, snapshot_markets, KalshiPriceHistory, KalshiSnapshotBatch};
pub use market_cache_store::{
    init_market_cache_table, load_persisted_cache, save_persisted_cache,
    PERSISTED_REHYDRATE_MAX_AGE_SECS,
};
