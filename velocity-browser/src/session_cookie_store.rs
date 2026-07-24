use crate::nda::NdaTriple;

#[derive(Debug, Clone, PartialEq)]
pub enum SameSitePolicy {
    Strict,
    Lax,
    None,
}

#[derive(Debug, Clone)]
pub struct CookieRecord {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires_timestamp: f64,
    pub samesite: SameSitePolicy,
    pub secure: bool,
    pub http_only: bool,
}

pub struct CookieStore {
    pub cookies: Vec<CookieRecord>,
}

impl Default for CookieStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CookieStore {
    pub fn new() -> Self {
        Self { cookies: Vec::new() }
    }

    pub fn set_cookie(&mut self, cookie: CookieRecord) {
        if let Some(existing) = self.cookies.iter_mut().find(|c| c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path) {
            *existing = cookie;
        } else {
            self.cookies.push(cookie);
        }
    }

    pub fn get_cookies_for_url(&self, domain: &str, path: &str, is_secure: bool) -> Vec<&CookieRecord> {
        self.cookies.iter().filter(|c| {
            if c.secure && !is_secure {
                return false;
            }
            if !domain.ends_with(&c.domain) && domain != c.domain {
                return false;
            }
            if !path.starts_with(&c.path) {
                return false;
            }
            true
        }).collect()
    }

    pub fn export_cookies_nda(&self) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for c in &self.cookies {
            let key = format!("{}:{}", c.domain, c.name);
            triples.push(NdaTriple::new(&key, 170, &c.value));
        }
        triples
    }
}
