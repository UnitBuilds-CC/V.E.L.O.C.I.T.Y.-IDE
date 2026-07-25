use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct DeviceProfile {
    pub user_agent: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub timezone: String,
    pub locale: String,
    pub platform: String,
    pub hardware_concurrency: u32,
    pub device_memory_gb: f32,
    pub max_touch_points: u32,
    pub color_depth: u32,
    pub pixel_ratio: f32,
    pub languages: Vec<String>,
    pub cookies_enabled: bool,
    pub do_not_track: bool,
    pub webdriver: bool,
}

impl DeviceProfile {
    pub fn velocity_native() -> Self {
        Self {
            user_agent: "VelocityEngine/1.0 (Native Pure-Rust Zero-Alloc Platform)".to_string(),
            viewport_width: 1920,
            viewport_height: 1080,
            timezone: "UTC".to_string(),
            locale: "en-US".to_string(),
            platform: "Win64".to_string(),
            hardware_concurrency: 8,
            device_memory_gb: 8.0,
            max_touch_points: 0,
            color_depth: 24,
            pixel_ratio: 1.0,
            languages: vec!["en-US".to_string(), "en".to_string()],
            cookies_enabled: true,
            do_not_track: false,
            webdriver: false,
        }
    }

    /// Chrome on Windows desktop profile — blends in with common traffic.
    pub fn chrome_windows() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36".to_string(),
            viewport_width: 1920,
            viewport_height: 1080,
            timezone: "America/New_York".to_string(),
            locale: "en-US".to_string(),
            platform: "Win32".to_string(),
            hardware_concurrency: 8,
            device_memory_gb: 8.0,
            max_touch_points: 0,
            color_depth: 24,
            pixel_ratio: 1.0,
            languages: vec!["en-US".to_string(), "en".to_string()],
            cookies_enabled: true,
            do_not_track: false,
            webdriver: false,
        }
    }

    /// Firefox on macOS profile.
    pub fn firefox_macos() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:126.0) Gecko/20100101 Firefox/126.0".to_string(),
            viewport_width: 1440,
            viewport_height: 900,
            timezone: "America/Los_Angeles".to_string(),
            locale: "en-US".to_string(),
            platform: "MacIntel".to_string(),
            hardware_concurrency: 10,
            device_memory_gb: 16.0,
            max_touch_points: 0,
            color_depth: 24,
            pixel_ratio: 2.0,
            languages: vec!["en-US".to_string(), "en".to_string()],
            cookies_enabled: true,
            do_not_track: true,
            webdriver: false,
        }
    }

    /// Mobile Chrome on Android — for responsive/mobile testing.
    pub fn chrome_android() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Mobile Safari/537.36".to_string(),
            viewport_width: 412,
            viewport_height: 915,
            timezone: "America/New_York".to_string(),
            locale: "en-US".to_string(),
            platform: "Linux armv8l".to_string(),
            hardware_concurrency: 8,
            device_memory_gb: 8.0,
            max_touch_points: 5,
            color_depth: 24,
            pixel_ratio: 2.625,
            languages: vec!["en-US".to_string(), "en".to_string()],
            cookies_enabled: true,
            do_not_track: false,
            webdriver: false,
        }
    }

    /// Safari on iPhone — for iOS-compatible testing.
    pub fn safari_iphone() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1".to_string(),
            viewport_width: 390,
            viewport_height: 844,
            timezone: "America/New_York".to_string(),
            locale: "en-US".to_string(),
            platform: "iPhone".to_string(),
            hardware_concurrency: 6,
            device_memory_gb: 6.0,
            max_touch_points: 5,
            color_depth: 24,
            pixel_ratio: 3.0,
            languages: vec!["en-US".to_string(), "en".to_string()],
            cookies_enabled: true,
            do_not_track: false,
            webdriver: false,
        }
    }

    /// Set viewport dimensions.
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.viewport_width = width;
        self.viewport_height = height;
    }

    /// Set timezone.
    pub fn set_timezone(&mut self, tz: &str) {
        self.timezone = tz.to_string();
    }

    /// Set locale and update languages accordingly.
    pub fn set_locale(&mut self, locale: &str) {
        self.locale = locale.to_string();
        let primary = locale.split('-').next().unwrap_or(locale).to_string();
        self.languages = vec![locale.to_string(), primary];
    }

    /// Whether this profile represents a touch device.
    pub fn is_touch_device(&self) -> bool {
        self.max_touch_points > 0
    }

    /// Whether this profile represents a mobile device.
    pub fn is_mobile(&self) -> bool {
        self.user_agent.contains("Mobile") || self.user_agent.contains("iPhone") || self.user_agent.contains("Android")
    }

    /// Effective screen size accounting for pixel ratio.
    pub fn effective_pixels(&self) -> (u32, u32) {
        (
            (self.viewport_width as f32 * self.pixel_ratio) as u32,
            (self.viewport_height as f32 * self.pixel_ratio) as u32,
        )
    }

    pub fn export_profile_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(session_id, 110, &self.user_agent),
            NdaTriple::new(session_id, 111, &format!("{}x{}", self.viewport_width, self.viewport_height)),
            NdaTriple::new(session_id, 112, &self.timezone),
            NdaTriple::new(session_id, 113, &self.locale),
            NdaTriple::new(session_id, 114, &self.platform),
            NdaTriple::new(session_id, 115, &format!("cores:{}:mem:{:.0}gb", self.hardware_concurrency, self.device_memory_gb)),
            NdaTriple::new(session_id, 116, &self.languages.join(",")),
            NdaTriple::new(session_id, 117, &format!("touch:{}:dnt:{}:wd:{}", self.max_touch_points, self.do_not_track as u8, self.webdriver as u8)),
        ]
    }
}
