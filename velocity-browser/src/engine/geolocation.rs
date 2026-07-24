use crate::nda::NdaTriple;

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
