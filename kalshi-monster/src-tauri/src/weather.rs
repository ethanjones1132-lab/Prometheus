#![allow(dead_code)]
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Weather data for a game location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameWeather {
    pub game: String,
    pub location: String,
    pub temperature_f: Option<f64>,
    pub wind_speed_mph: Option<f64>,
    pub wind_gust_mph: Option<f64>,
    pub precipitation_chance: Option<f64>,
    pub condition: String,
    pub is_dome: bool,
    pub impact_summary: String,
}

impl GameWeather {
    /// Format weather as a compact string for LLM context injection
    pub fn to_delta_string(&self) -> String {
        if self.is_dome {
            return format!("Weather in {}: Dome (no weather impact)", self.game);
        }

        let mut parts = Vec::new();
        if let Some(temp) = self.temperature_f {
            parts.push(format!("{:.0}°F", temp));
        }
        if let Some(wind) = self.wind_speed_mph {
            parts.push(format!("wind {:.0} mph", wind));
            if let Some(gust) = self.wind_gust_mph {
                if gust > wind + 5.0 {
                    parts.push(format!("gusting {:.0}", gust));
                }
            }
        }
        if let Some(precip) = self.precipitation_chance {
            if precip > 10.0 {
                parts.push(format!("{:.0}% precip", precip));
            }
        }
        if !self.condition.is_empty() {
            parts.push(self.condition.clone());
        }

        let weather_desc = if parts.is_empty() {
            "clear".to_string()
        } else {
            parts.join(", ")
        };

        format!(
            "Weather in {}: {} — {}",
            self.game, weather_desc, self.impact_summary
        )
    }
}

/// Weather API client
/// Uses Open-Meteo (free, no API key needed) as primary source
/// Falls back to OpenWeatherMap if API key is provided
pub struct WeatherClient {
    client: Client,
    openweathermap_api_key: String,
    /// Cache: city/team abbreviation -> (weather, timestamp_secs)
    cache: HashMap<String, (GameWeather, u64)>,
    /// Cache TTL in seconds (default: 300 = 5 minutes)
    cache_ttl: u64,
}

impl WeatherClient {
    pub fn new(openweathermap_api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self {
            client,
            openweathermap_api_key,
            cache: HashMap::new(),
            cache_ttl: 300,
        }
    }

    /// Set cache TTL in seconds
    pub fn set_cache_ttl(&mut self, secs: u64) {
        self.cache_ttl = secs;
    }

    /// Get weather for a game location by city name or team abbreviation
    pub async fn get_weather(
        &mut self,
        game: &str,
        location: &str,
    ) -> Result<GameWeather, String> {
        // Check cache
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some((weather, ts)) = self.cache.get(location) {
            if now - *ts < self.cache_ttl {
                return Ok(weather.clone());
            }
        }

        // Try Open-Meteo first (free, no key needed)
        let result = self.fetch_openmeteo(location).await;

        let weather = match result {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("Open-Meteo failed for {}: {}", location, e);
                // Try OpenWeatherMap as fallback
                if !self.openweathermap_api_key.is_empty() {
                    match self.fetch_openweathermap(location).await {
                        Ok(w) => w,
                        Err(e2) => {
                            tracing::warn!("OpenWeatherMap also failed for {}: {}", location, e2);
                            // Return a default "unknown" weather
                            GameWeather {
                                game: game.into(),
                                location: location.into(),
                                temperature_f: None,
                                wind_speed_mph: None,
                                wind_gust_mph: None,
                                precipitation_chance: None,
                                condition: "Unknown".into(),
                                is_dome: false,
                                impact_summary: "Weather data unavailable — assume neutral conditions".into(),
                            }
                        }
                    }
                } else {
                    GameWeather {
                        game: game.into(),
                        location: location.into(),
                        temperature_f: None,
                        wind_speed_mph: None,
                        wind_gust_mph: None,
                        precipitation_chance: None,
                        condition: "Unknown".into(),
                        is_dome: false,
                        impact_summary: "Weather data unavailable — assume neutral conditions".into(),
                    }
                }
            }
        };

        self.cache.insert(location.to_string(), (weather.clone(), now));
        Ok(weather)
    }

    /// Fetch weather from Open-Meteo API (free, no API key required)
    async fn fetch_openmeteo(&self, location: &str) -> Result<GameWeather, String> {
        // First, geocode the location to get lat/lon
        let geo_url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1",
            location
        );

        let geo_resp: Value = self
            .client
            .get(&geo_url)
            .send()
            .await
            .map_err(|e| format!("Geocoding request failed: {}", e))?
            .json()
            .await
            .map_err(|e| format!("Geocoding parse failed: {}", e))?;

        let results = geo_resp
            .get("results")
            .and_then(|r| r.as_array())
            .ok_or("No geocoding results")?;

        let first = results.first().ok_or("Empty geocoding results")?;
        let lat = first
            .get("latitude")
            .and_then(|l| l.as_f64())
            .ok_or("No latitude")?;
        let lon = first
            .get("longitude")
            .and_then(|l| l.as_f64())
            .ok_or("No longitude")?;

        // Now fetch weather data
        let weather_url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,wind_speed_10m,wind_gusts_10m,weather_code,precipitation_probability&temperature_unit=fahrenheit&wind_speed_unit=mph&timezone=auto",
            lat, lon
        );

        let weather_resp: Value = self
            .client
            .get(&weather_url)
            .send()
            .await
            .map_err(|e| format!("Weather request failed: {}", e))?
            .json()
            .await
            .map_err(|e| format!("Weather parse failed: {}", e))?;

        let current = weather_resp
            .get("current")
            .ok_or("No current weather data")?;

        let temp = current.get("temperature_2m").and_then(|t| t.as_f64());
        let wind = current.get("wind_speed_10m").and_then(|w| w.as_f64());
        let gust = current.get("wind_gusts_10m").and_then(|g| g.as_f64());
        let precip_prob = current
            .get("precipitation_probability")
            .and_then(|p| p.as_f64());
        let weather_code = current
            .get("weather_code")
            .and_then(|c| c.as_i64())
            .unwrap_or(0);

        let condition = weather_code_to_string(weather_code);
        let impact = assess_weather_impact(temp, wind, precip_prob, false);

        Ok(GameWeather {
            game: location.into(),
            location: location.into(),
            temperature_f: temp,
            wind_speed_mph: wind,
            wind_gust_mph: gust,
            precipitation_chance: precip_prob,
            condition,
            is_dome: false,
            impact_summary: impact,
        })
    }

    /// Fetch weather from OpenWeatherMap API (requires API key)
    async fn fetch_openweathermap(&self, location: &str) -> Result<GameWeather, String> {
        let url = format!(
            "https://api.openweathermap.org/data/2.5/weather?q={}&appid={}&units=imperial",
            location, self.openweathermap_api_key
        );

        let resp: Value = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("OWM request failed: {}", e))?
            .json()
            .await
            .map_err(|e| format!("OWM parse failed: {}", e))?;

        let main = resp.get("main");
        let wind = resp.get("wind");
        let weather = resp.get("weather").and_then(|w| w.as_array()).and_then(|a| a.first());

        let temp = main.and_then(|m| m.get("temp")).and_then(|t| t.as_f64());
        let wind_speed = wind.and_then(|w| w.get("speed")).and_then(|s| s.as_f64());
        let wind_gust = wind.and_then(|w| w.get("gust")).and_then(|g| g.as_f64());
        let condition = weather
            .and_then(|w| w.get("description"))
            .and_then(|d| d.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let impact = assess_weather_impact(temp, wind_speed, None, false);

        Ok(GameWeather {
            game: location.into(),
            location: location.into(),
            temperature_f: temp,
            wind_speed_mph: wind_speed,
            wind_gust_mph: wind_gust,
            precipitation_chance: None,
            condition,
            is_dome: false,
            impact_summary: impact,
        })
    }

    /// Build a weather context string for multiple games
    pub async fn build_weather_context(
        &mut self,
        games: &[(String, String)], // (game_desc, location)
    ) -> String {
        let mut out = String::from("🌤️ LIVE WEATHER DELTA:\n");

        for (game, location) in games {
            match self.get_weather(game, location).await {
                Ok(weather) => {
                    out.push_str(&format!("  {}\n", weather.to_delta_string()));
                }
                Err(e) => {
                    out.push_str(&format!("  {}: Weather unavailable ({})\n", game, e));
                }
            }
        }

        out
    }
}

/// Convert WMO weather code to human-readable string
fn weather_code_to_string(code: i64) -> String {
    match code {
        0 => "Clear sky".into(),
        1 => "Mainly clear".into(),
        2 => "Partly cloudy".into(),
        3 => "Overcast".into(),
        45 | 48 => "Foggy".into(),
        51 | 53 | 55 => "Drizzle".into(),
        61 | 63 | 65 => "Rain".into(),
        66 | 67 => "Freezing rain".into(),
        71 | 73 | 75 => "Snow".into(),
        77 => "Snow grains".into(),
        80 | 81 | 82 => "Rain showers".into(),
        85 | 86 => "Snow showers".into(),
        95 => "Thunderstorm".into(),
        96 | 99 => "Thunderstorm with hail".into(),
        _ => "Unknown".into(),
    }
}

/// Assess weather impact on football game
fn assess_weather_impact(
    temp: Option<f64>,
    wind: Option<f64>,
    precip: Option<f64>,
    is_dome: bool,
) -> String {
    if is_dome {
        return "Dome — no weather impact".into();
    }

    let mut impacts = Vec::new();

    if let Some(w) = wind {
        if w >= 25.0 {
            impacts.push("severe wind — deep passing game heavily suppressed, favor rushing props and unders");
        } else if w >= 15.0 {
            impacts.push("moderate wind — some suppression of passing/kicking, slight lean toward unders");
        } else if w >= 10.0 {
            impacts.push("light wind — minimal impact on most props");
        }
    }

    if let Some(t) = temp {
        if t <= 20.0 {
            impacts.push("extreme cold — passing efficiency drops, favor rushing and unders");
        } else if t <= 32.0 {
            impacts.push("freezing — slight suppression of passing game");
        } else if t >= 95.0 {
            impacts.push("extreme heat — fatigue factor, pace may slow in 2nd half");
        }
    }

    if let Some(p) = precip {
        if p >= 70.0 {
            impacts.push("high precipitation chance — favor rushing, lean unders on passing props");
        } else if p >= 40.0 {
            impacts.push("moderate precip chance — slight lean toward run-heavy game script");
        }
    }

    if impacts.is_empty() {
        "Neutral conditions — no significant weather impact expected".into()
    } else {
        impacts.join("; ")
    }
}

/// Map NFL team abbreviations to city names for weather lookup
pub fn team_to_city(team_abbr: &str) -> &str {
    match team_abbr.to_uppercase().as_str() {
        "ARI" => "Phoenix",
        "ATL" => "Atlanta",
        "BAL" => "Baltimore",
        "BUF" => "Buffalo",
        "CAR" => "Charlotte",
        "CHI" => "Chicago",
        "CIN" => "Cincinnati",
        "CLE" => "Cleveland",
        "DAL" => "Dallas",
        "DEN" => "Denver",
        "DET" => "Detroit",
        "GB" => "Green Bay",
        "HOU" => "Houston",
        "IND" => "Indianapolis",
        "JAX" => "Jacksonville",
        "KC" => "Kansas City",
        "LAR" => "Los Angeles",
        "LVR" | "LV" => "Las Vegas",
        "MIA" => "Miami",
        "MIN" => "Minneapolis",
        "NE" => "Boston",
        "NO" => "New Orleans",
        "NYG" => "New York",
        "NYJ" => "New York",
        "PHI" => "Philadelphia",
        "PIT" => "Pittsburgh",
        "SEA" => "Seattle",
        "SF" => "San Francisco",
        "TB" => "Tampa",
        "TEN" => "Nashville",
        "WAS" => "Washington",
        _ => team_abbr,
    }
}

/// Check if a team plays in a dome
pub fn is_dome_team(team_abbr: &str) -> bool {
    matches!(
        team_abbr.to_uppercase().as_str(),
        "ARI" | "ATL" | "DAL" | "DET" | "HOU" | "IND" | "MIN" | "NO" | "LAR" | "LV" | "LVR"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weather_code_to_string() {
        assert_eq!(weather_code_to_string(0), "Clear sky");
        assert_eq!(weather_code_to_string(61), "Rain");
        assert_eq!(weather_code_to_string(95), "Thunderstorm");
    }

    #[test]
    fn test_assess_weather_impact() {
        let impact = assess_weather_impact(Some(75.0), Some(5.0), Some(5.0), false);
        assert!(impact.contains("Neutral"));

        let impact = assess_weather_impact(Some(75.0), Some(20.0), Some(5.0), false);
        assert!(impact.contains("moderate wind"));

        let impact = assess_weather_impact(Some(15.0), Some(30.0), Some(80.0), false);
        assert!(impact.contains("severe wind"));
        assert!(impact.contains("extreme cold"));
        assert!(impact.contains("high precipitation"));
    }

    #[test]
    fn test_team_to_city() {
        assert_eq!(team_to_city("KC"), "Kansas City");
        assert_eq!(team_to_city("GB"), "Green Bay");
        assert_eq!(team_to_city("NE"), "Boston");
    }

    #[test]
    fn test_is_dome_team() {
        assert!(is_dome_team("DET"));
        assert!(is_dome_team("MIN"));
        assert!(!is_dome_team("GB"));
        assert!(!is_dome_team("CHI"));
    }

    #[test]
    fn test_game_weather_delta_string() {
        let weather = GameWeather {
            game: "KC vs BUF".into(),
            location: "Kansas City".into(),
            temperature_f: Some(35.0),
            wind_speed_mph: Some(20.0),
            wind_gust_mph: Some(28.0),
            precipitation_chance: Some(15.0),
            condition: "Partly cloudy".into(),
            is_dome: false,
            impact_summary: "moderate wind — some suppression of passing".into(),
        };

        let delta = weather.to_delta_string();
        assert!(delta.contains("KC vs BUF"));
        assert!(delta.contains("35°F"));
        assert!(delta.contains("wind 20 mph"));
        assert!(delta.contains("gusting 28"));
    }
}
