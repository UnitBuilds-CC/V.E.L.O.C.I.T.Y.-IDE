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

/// Permission state for geolocation access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoPermission {
    /// Permission not yet requested.
    Prompt,
    /// User granted permission.
    Granted,
    /// User denied permission.
    Denied,
}

/// Geolocation permission manager.
#[derive(Debug, Clone)]
pub struct GeoPermissionState {
    pub state: GeoPermission,
    /// Whether the site has been asked before.
    pub previously_asked: bool,
}

impl Default for GeoPermissionState {
    fn default() -> Self {
        Self { state: GeoPermission::Prompt, previously_asked: false }
    }
}

impl GeoPermissionState {
    pub fn grant(&mut self) {
        self.state = GeoPermission::Granted;
        self.previously_asked = true;
    }

    pub fn deny(&mut self) {
        self.state = GeoPermission::Denied;
        self.previously_asked = true;
    }

    pub fn reset(&mut self) {
        self.state = GeoPermission::Prompt;
        self.previously_asked = false;
    }

    pub fn is_granted(&self) -> bool {
        self.state == GeoPermission::Granted
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

/// Distance between two coordinate pairs using the Haversine formula (meters).
pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0; // Earth radius in meters
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    r * c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_lifecycle() {
        let mut perm = GeoPermissionState::default();
        assert_eq!(perm.state, GeoPermission::Prompt);
        assert!(!perm.previously_asked);

        perm.grant();
        assert!(perm.is_granted());
        assert!(perm.previously_asked);

        perm.deny();
        assert!(!perm.is_granted());

        perm.reset();
        assert_eq!(perm.state, GeoPermission::Prompt);
        assert!(!perm.previously_asked);
    }

    #[test]
    fn haversine_same_point() {
        let d = haversine_distance(37.7749, -122.4194, 37.7749, -122.4194);
        assert!(d.abs() < 0.01);
    }

    #[test]
    fn haversine_known_distance() {
        // SF to LA ~559km
        let d = haversine_distance(37.7749, -122.4194, 34.0522, -118.2437);
        assert!(d > 500_000.0 && d < 600_000.0);
    }
}
