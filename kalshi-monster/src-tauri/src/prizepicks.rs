use serde::{Deserialize, Serialize};

pub mod models {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct PrizePicksProp {
        pub external_id: String,
        pub player_name: String,
        pub team: String,
        pub opponent: String,
        pub stat_category: String,
        pub line: f64,
        pub league: String,
        pub projection: Option<f64>,
        pub source: String,
        pub game_time: Option<String>,
    }
}

pub struct PrizePicksFetcher;

impl PrizePicksFetcher {
    pub async fn fetch_props(
        &mut self,
        _league: Option<&str>,
        _cache_only: bool,
    ) -> Result<PropsResponse, String> {
        Ok(PropsResponse {
            props: vec![],
            source: "Mock".to_string(),
        })
    }
}

pub struct PropsResponse {
    pub props: Vec<models::PrizePicksProp>,
    pub source: String,
}
