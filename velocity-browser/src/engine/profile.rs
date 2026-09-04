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
        self.user_agent.contains("Mobile")
            || self.user_agent.contains("iPhone")
            || self.user_agent.contains("Android")
    }

    /// Effective screen size accounting for pixel ratio.
    pub fn effective_pixels(&self) -> (u32, u32) {
        (
            (self.viewport_width as f32 * self.pixel_ratio) as u32,
            (self.viewport_height as f32 * self.pixel_ratio) as u32,
        )
    }

    /// Aspect ratio of the viewport (width / height).
    pub fn viewport_aspect_ratio(&self) -> f32 {
        if self.viewport_height == 0 {
            return 0.0;
        }
        self.viewport_width as f32 / self.viewport_height as f32
    }

    /// Whether this profile represents a tablet device.
    pub fn is_tablet(&self) -> bool {
        // Tablets typically have touch support, mobile UA strings, but larger viewports
        self.is_touch_device()
            && (self.user_agent.contains("Android") && !self.user_agent.contains("Mobile"))
            || (self.viewport_width >= 600 && self.viewport_width <= 1400 && self.is_touch_device())
    }

    /// Parse a user-agent string into a DeviceProfile with best-effort detection.
    pub fn from_user_agent(ua: &str) -> Self {
        let lower = ua.to_lowercase();
        let is_mobile =
            lower.contains("mobile") || lower.contains("iphone") || lower.contains("android");
        let is_touch = is_mobile || lower.contains("ipad") || lower.contains("tablet");

        let (platform, viewport_w, viewport_h, pixel_ratio) =
            if lower.contains("windows") || lower.contains("win64") {
                ("Win32", 1920u32, 1080u32, 1.0f32)
            } else if lower.contains("iphone") {
                ("iPhone", 390, 844, 3.0)
            } else if lower.contains("ipad") {
                ("iPad", 810, 1080, 2.0)
            } else if lower.contains("macintosh") || lower.contains("mac os") {
                ("MacIntel", 1440, 900, 2.0)
            } else if lower.contains("android") {
                if lower.contains("mobile") {
                    ("Linux armv8l", 412, 915, 2.625)
                } else {
                    ("Linux armv8l", 800, 1280, 2.0)
                }
            } else if lower.contains("linux") {
                ("Linux x86_64", 1920, 1080, 1.0)
            } else {
                ("unknown", 1024, 768, 1.0)
            };

        let touch_points = if is_touch { 5 } else { 0 };

        Self {
            user_agent: ua.to_string(),
            viewport_width: viewport_w,
            viewport_height: viewport_h,
            timezone: "UTC".to_string(),
            locale: "en-US".to_string(),
            platform: platform.to_string(),
            hardware_concurrency: 8,
            device_memory_gb: if is_mobile { 6.0 } else { 8.0 },
            max_touch_points: touch_points,
            color_depth: 24,
            pixel_ratio,
            languages: vec!["en-US".to_string(), "en".to_string()],
            cookies_enabled: true,
            do_not_track: false,
            webdriver: false,
        }
    }

    pub fn export_profile_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(session_id, 110, &self.user_agent),
            NdaTriple::new(
                session_id,
                111,
                &format!("{}x{}", self.viewport_width, self.viewport_height),
            ),
            NdaTriple::new(session_id, 112, &self.timezone),
            NdaTriple::new(session_id, 113, &self.locale),
            NdaTriple::new(session_id, 114, &self.platform),
            NdaTriple::new(
                session_id,
                115,
                &format!(
                    "cores:{}:mem:{:.0}gb",
                    self.hardware_concurrency, self.device_memory_gb
                ),
            ),
            NdaTriple::new(session_id, 116, &self.languages.join(",")),
            NdaTriple::new(
                session_id,
                117,
                &format!(
                    "touch:{}:dnt:{}:wd:{}",
                    self.max_touch_points, self.do_not_track as u8, self.webdriver as u8
                ),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_velocity_native_defaults() {
        let p = DeviceProfile::velocity_native();
        assert_eq!(p.viewport_width, 1920);
        assert_eq!(p.viewport_height, 1080);
        assert_eq!(p.platform, "Win64");
        assert!(!p.webdriver);
        assert!(p.cookies_enabled);
        assert!(!p.is_mobile());
        assert!(!p.is_touch_device());
    }

    #[test]
    fn test_chrome_windows_profile() {
        let p = DeviceProfile::chrome_windows();
        assert!(p.user_agent.contains("Chrome"));
        assert!(p.user_agent.contains("Windows"));
        assert_eq!(p.platform, "Win32");
        assert!(!p.is_mobile());
        assert!(!p.is_touch_device());
    }

    #[test]
    fn test_firefox_macos_profile() {
        let p = DeviceProfile::firefox_macos();
        assert!(p.user_agent.contains("Firefox"));
        assert!(p.user_agent.contains("Macintosh"));
        assert_eq!(p.platform, "MacIntel");
        assert!(p.do_not_track);
        assert_eq!(p.pixel_ratio, 2.0);
    }

    #[test]
    fn test_chrome_android_is_mobile() {
        let p = DeviceProfile::chrome_android();
        assert!(p.is_mobile());
        assert!(p.is_touch_device());
        assert_eq!(p.max_touch_points, 5);
        assert!(p.user_agent.contains("Android"));
    }

    #[test]
    fn test_safari_iphone_is_mobile() {
        let p = DeviceProfile::safari_iphone();
        assert!(p.is_mobile());
        assert!(p.is_touch_device());
        assert_eq!(p.viewport_width, 390);
        assert_eq!(p.pixel_ratio, 3.0);
    }

    #[test]
    fn test_set_viewport() {
        let mut p = DeviceProfile::velocity_native();
        p.set_viewport(800, 600);
        assert_eq!(p.viewport_width, 800);
        assert_eq!(p.viewport_height, 600);
    }

    #[test]
    fn test_set_locale_updates_languages() {
        let mut p = DeviceProfile::velocity_native();
        p.set_locale("fr-FR");
        assert_eq!(p.locale, "fr-FR");
        assert_eq!(p.languages, vec!["fr-FR".to_string(), "fr".to_string()]);
    }

    #[test]
    fn test_set_timezone() {
        let mut p = DeviceProfile::velocity_native();
        p.set_timezone("Asia/Tokyo");
        assert_eq!(p.timezone, "Asia/Tokyo");
    }

    #[test]
    fn test_effective_pixels() {
        let p = DeviceProfile::safari_iphone();
        let (w, h) = p.effective_pixels();
        assert_eq!(w, (390.0 * 3.0) as u32);
        assert_eq!(h, (844.0 * 3.0) as u32);
    }

    #[test]
    fn test_viewport_aspect_ratio() {
        let p = DeviceProfile::velocity_native();
        let ratio = p.viewport_aspect_ratio();
        assert!((ratio - 16.0 / 9.0).abs() < 0.01);
    }

    #[test]
    fn test_viewport_aspect_ratio_zero_height() {
        let mut p = DeviceProfile::velocity_native();
        p.viewport_height = 0;
        assert_eq!(p.viewport_aspect_ratio(), 0.0);
    }

    #[test]
    fn test_is_tablet_android() {
        let mut p = DeviceProfile::chrome_android();
        // Android tablet: has touch, has "Android" but NOT "Mobile"
        p.user_agent = "Mozilla/5.0 (Linux; Android 14; SM-X710) AppleWebKit/537.36".to_string();
        assert!(p.is_tablet());
    }

    #[test]
    fn test_is_not_tablet_for_phone() {
        let p = DeviceProfile::safari_iphone();
        assert!(!p.is_tablet());
    }

    #[test]
    fn test_from_user_agent_windows() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
        let p = DeviceProfile::from_user_agent(ua);
        assert_eq!(p.platform, "Win32");
        assert_eq!(p.viewport_width, 1920);
        assert!(!p.is_mobile());
        assert!(!p.is_touch_device());
    }

    #[test]
    fn test_from_user_agent_iphone() {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X)";
        let p = DeviceProfile::from_user_agent(ua);
        assert_eq!(p.platform, "iPhone");
        assert!(p.is_mobile());
        assert!(p.is_touch_device());
        assert_eq!(p.pixel_ratio, 3.0);
    }

    #[test]
    fn test_from_user_agent_android_mobile() {
        let ua = "Mozilla/5.0 (Linux; Android 14; Pixel 8) Mobile Safari/537.36";
        let p = DeviceProfile::from_user_agent(ua);
        assert!(p.is_mobile());
        assert!(p.is_touch_device());
        assert_eq!(p.viewport_width, 412);
    }

    #[test]
    fn test_from_user_agent_unknown() {
        let p = DeviceProfile::from_user_agent("SomeUnknownBot/1.0");
        assert_eq!(p.platform, "unknown");
        assert_eq!(p.viewport_width, 1024);
    }

    #[test]
    fn test_export_profile_nda() {
        let p = DeviceProfile::velocity_native();
        let triples = p.export_profile_nda("sess-1");
        assert_eq!(triples.len(), 8);
        assert_eq!(triples[0].predicate_id, 110);
        assert_eq!(triples[7].predicate_id, 117);
    }
}
