use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct DeviceProfile {
    pub user_agent: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub timezone: String,
    pub locale: String,
}

impl DeviceProfile {
    pub fn velocity_native() -> Self {
        Self {
            user_agent: "VelocityEngine/1.0 (Native Pure-Rust Zero-Alloc Platform)".to_string(),
            viewport_width: 1920,
            viewport_height: 1080,
            timezone: "UTC".to_string(),
            locale: "en-US".to_string(),
        }
    }

    pub fn export_profile_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(session_id, 110, &self.user_agent),
            NdaTriple::new(session_id, 111, &format!("{}x{}", self.viewport_width, self.viewport_height)),
            NdaTriple::new(session_id, 112, &self.timezone),
            NdaTriple::new(session_id, 113, &self.locale),
        ]
    }
}
