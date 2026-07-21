use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct DeviceProfile {
    pub user_agent: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub device_scale_factor: f32,
    pub timezone_id: String,
    pub locale: String,
    pub has_touch: bool,
}

impl DeviceProfile {
    pub fn desktop_chrome() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36".to_string(),
            viewport_width: 1920,
            viewport_height: 1080,
            device_scale_factor: 1.0,
            timezone_id: "America/New_York".to_string(),
            locale: "en-US".to_string(),
            has_touch: false,
        }
    }

    pub fn mobile_safari() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1".to_string(),
            viewport_width: 390,
            viewport_height: 844,
            device_scale_factor: 3.0,
            timezone_id: "America/Los_Angeles".to_string(),
            locale: "en-US".to_string(),
            has_touch: true,
        }
    }

    pub fn export_profile_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(session_id, 110, &self.user_agent),
            NdaTriple::new(session_id, 111, &format!("{}x{}", self.viewport_width, self.viewport_height)),
            NdaTriple::new(session_id, 112, &self.timezone_id),
            NdaTriple::new(session_id, 113, &self.locale),
        ]
    }
}
