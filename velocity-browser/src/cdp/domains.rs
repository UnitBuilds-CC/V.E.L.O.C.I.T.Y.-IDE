pub struct CdpRequest {
    pub id: u64,
    pub method: String,
    pub params_json: Option<String>,
}

pub struct CdpResponse {
    pub id: Option<u64>,
    pub method: Option<String>,
    pub result_json: Option<String>,
    pub error_message: Option<String>,
}

pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: f64,
    pub http_only: bool,
    pub secure: bool,
    pub session: bool,
    pub same_site: Option<String>,
}

pub struct AxNode {
    pub node_id: String,
    pub role: String,
    pub name: String,
    pub value: String,
    pub description: String,
}
