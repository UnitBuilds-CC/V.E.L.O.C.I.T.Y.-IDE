use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

pub struct HttpClient {
    pub cookie_jar: HashMap<String, String>,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            cookie_jar: HashMap::new(),
        }
    }

    /// Perform a native HTTP GET request over TCP socket
    pub fn get(&mut self, url: &str) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let (host, port, path) = parse_url(url)?;
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr)?;

        let mut req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: VelocityAgent/1.0\r\nConnection: close\r\n", path, host);
        if !self.cookie_jar.is_empty() {
            let cookie_header: Vec<String> = self.cookie_jar.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            req.push_str(&format!("Cookie: {}\r\n", cookie_header.join("; ")));
        }
        req.push_str("\r\n");

        stream.write_all(req.as_bytes())?;
        stream.flush()?;

        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer)?;

        parse_http_response(&buffer, &mut self.cookie_jar)
    }
}

fn parse_url(url: &str) -> Result<(String, u16, String), &'static str> {
    let s = url.trim();
    let without_scheme = if let Some(stripped) = s.strip_prefix("http://") {
        stripped
    } else if let Some(stripped) = s.strip_prefix("https://") {
        stripped
    } else {
        s
    };

    let parts: Vec<&str> = without_scheme.splitn(2, '/').collect();
    let host_port = parts[0];
    let path = if parts.len() > 1 {
        format!("/{}", parts[1])
    } else {
        "/".to_string()
    };

    let hp_parts: Vec<&str> = host_port.splitn(2, ':').collect();
    let host = hp_parts[0].to_string();
    let port = if hp_parts.len() > 1 {
        hp_parts[1].parse::<u16>().unwrap_or(80)
    } else {
        80
    };

    Ok((host, port, path))
}

fn parse_http_response(raw: &[u8], cookie_jar: &mut HashMap<String, String>) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
    let response_str = String::from_utf8_lossy(raw);
    let parts: Vec<&str> = response_str.splitn(2, "\r\n\r\n").collect();

    if parts.is_empty() {
        return Err("Invalid HTTP response".into());
    }

    let header_lines: Vec<&str> = parts[0].lines().collect();
    let status_line = header_lines.first().cloned().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(200);

    let mut headers = HashMap::new();
    for line in &header_lines[1..] {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_lowercase();
            let val = v.trim().to_string();
            if key == "set-cookie" {
                if let Some((ck, cv)) = val.split_once('=') {
                    cookie_jar.insert(ck.trim().to_string(), cv.split(';').next().unwrap_or("").trim().to_string());
                }
            }
            headers.insert(key, val);
        }
    }

    let body = if parts.len() > 1 { parts[1].to_string() } else { String::new() };

    Ok(HttpResponse {
        status_code,
        headers,
        body,
    })
}
