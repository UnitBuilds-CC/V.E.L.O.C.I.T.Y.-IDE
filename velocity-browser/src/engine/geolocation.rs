use crate::nda::NdaTriple;
use std::path::Path;

/// Per-session geolocation configuration loaded from workspace settings.
#[derive(Debug, Clone)]
pub struct GeolocationConfig {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_meters: f64,
}

impl Default for GeolocationConfig {
    fn default() -> Self {
        // Default: San Francisco (used as fallback when no config is present).
        Self {
            latitude: 37.7749,
            longitude: -122.4194,
            accuracy_meters: 5.0,
        }
    }
}

impl GeolocationConfig {
    /// Load geolocation config from the workspace's .velocity/session_geo.json.
    /// Falls back to default San Francisco coordinates if not present.
    pub fn load_from_workspace(workspace_root: &Path) -> Self {
        let config_path = workspace_root.join(".velocity").join("session_geo.json");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                let lat = parsed.get("latitude").and_then(|v| v.as_f64()).unwrap_or(37.7749);
                let lon = parsed.get("longitude").and_then(|v| v.as_f64()).unwrap_or(-122.4194);
                let acc = parsed.get("accuracy_meters").and_then(|v| v.as_f64()).unwrap_or(5.0);
                return Self {
                    latitude: lat,
                    longitude: lon,
                    accuracy_meters: acc,
                };
            }
        }
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct Geocoordinates {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_meters: f64,
}

pub struct GeolocationProvider {
    pub current_coords: Geocoordinates,
}

impl GeolocationProvider {
    /// Create a provider with specific coordinates (configurable per-session).
    pub fn new(latitude: f64, longitude: f64, accuracy_meters: f64) -> Self {
        Self {
            current_coords: Geocoordinates {
                latitude,
                longitude,
                accuracy_meters,
            },
        }
    }

    /// Default mock provider at San Francisco (for testing/fallback).
    pub fn mock_sf() -> Self {
        Self::new(37.7749, -122.4194, 5.0)
    }

    /// Create from a GeolocationConfig (per-session configuration).
    pub fn from_config(config: &GeolocationConfig) -> Self {
        Self::new(config.latitude, config.longitude, config.accuracy_meters)
    }

    /// Create from coordinate pair with default accuracy.
    pub fn from_coords(latitude: f64, longitude: f64) -> Self {
        Self::new(latitude, longitude, 10.0)
    }

    /// Update the provider's coordinates (e.g., from automation config).
    pub fn set_position(&mut self, latitude: f64, longitude: f64, accuracy: f64) {
        self.current_coords = Geocoordinates {
            latitude,
            longitude,
            accuracy_meters: accuracy,
        };
    }

    pub fn get_current_position(&self) -> Geocoordinates {
        self.current_coords.clone()
    }

    pub fn export_geolocation_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        vec![NdaTriple::new(
            session_id,
            241,
            &format!("lat:{},lon:{}", self.current_coords.latitude, self.current_coords.longitude),
        )]
    }
}
