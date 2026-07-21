#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: String,
}

pub struct HttpClient;

impl HttpClient {
    pub fn new() -> Self {
        Self
    }

    pub fn get(&self, url: &str) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(HttpResponse {
            status_code: 200,
            body: format!("<html><body><h1>Native Content from {}</h1></body></html>", url),
        })
    }
}
