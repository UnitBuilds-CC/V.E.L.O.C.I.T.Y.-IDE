#[derive(Debug, Clone)]
pub enum ProxyType {
    Direct,
    Http(String, u16),
    Socks5(String, u16),
}

pub struct ProxyResolver {
    pub proxy: ProxyType,
    pub bypass_list: Vec<String>,
}

impl ProxyResolver {
    pub fn direct() -> Self {
        Self {
            proxy: ProxyType::Direct,
            bypass_list: Vec::new(),
        }
    }

    pub fn set_http_proxy(&mut self, host: &str, port: u16) {
        self.proxy = ProxyType::Http(host.to_string(), port);
    }

    pub fn resolve_proxy_for_url(&self, url: &str) -> ProxyType {
        for domain in &self.bypass_list {
            if url.contains(domain) {
                return ProxyType::Direct;
            }
        }
        self.proxy.clone()
    }
}
