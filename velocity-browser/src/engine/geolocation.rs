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
    pub fn mock_sf() -> Self {
        Self {
            current_coords: Geocoordinates {
                latitude: 37.7749,
                longitude: -122.4194,
                accuracy_meters: 5.0,
            },
        }
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
