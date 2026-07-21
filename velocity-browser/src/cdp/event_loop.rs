pub struct CdpEventLoop {
    pub console_logs: Vec<String>,
    pub network_events: Vec<String>,
    pub download_guid: Option<String>,
    pub file_chooser_opened: bool,
}

impl CdpEventLoop {
    pub fn new() -> Self {
        Self {
            console_logs: Vec::new(),
            network_events: Vec::new(),
            download_guid: None,
            file_chooser_opened: false,
        }
    }

    /// Process incoming CDP JSON notification message string without external crates
    pub fn handle_raw_event(&mut self, json_str: &str) {
        if json_str.contains("\"method\":\"Console.messageAdded\"") {
            if let Some(text) = extract_json_field(json_str, "text") {
                self.console_logs.push(text);
            }
        } else if json_str.contains("\"method\":\"Network.responseReceived\"") {
            if let Some(url) = extract_json_field(json_str, "url") {
                self.network_events.push(format!("GET {}", url));
            }
        } else if json_str.contains("\"method\":\"Page.fileChooserOpened\"") {
            self.file_chooser_opened = true;
        } else if json_str.contains("\"method\":\"Browser.downloadWillBegin\"") {
            if let Some(guid) = extract_json_field(json_str, "guid") {
                self.download_guid = Some(guid);
            }
        }
    }
}

fn extract_json_field(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{}\":\"", field);
    if let Some(start) = json.find(&key) {
        let val_start = start + key.len();
        if let Some(end) = json[val_start..].find('"') {
            return Some(json[val_start..val_start + end].to_string());
        }
    }
    None
}
