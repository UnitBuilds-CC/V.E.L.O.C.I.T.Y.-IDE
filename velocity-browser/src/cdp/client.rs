use crate::cdp::ws_client::NativeWsClient;
use crate::nda::NdaTriple;

pub struct NativeCdpClient {
    ws: NativeWsClient,
    seq: u64,
}

impl NativeCdpClient {
    pub fn connect(host: &str, port: u16, path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let ws = NativeWsClient::connect(host, port, path)?;
        Ok(Self { ws, seq: 1 })
    }

    pub fn send_command(&mut self, method: &str, params_json: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let id = self.seq;
        self.seq += 1;

        let req = format!(
            "{{\"id\":{},\"method\":\"{}\",\"params\":{}}}",
            id, method, params_json
        );

        self.ws.send_text(&req)?;
        let resp = self.ws.read_text()?;
        Ok(resp)
    }

    /// Convert page metadata directly to zero-allocation binary NDA triples
    pub fn page_to_nda_triples(&mut self, url: &str, title: &str) -> Vec<NdaTriple> {
        vec![
            NdaTriple::new(url, 1, "page"),
            NdaTriple::new(url, 2, title),
        ]
    }
}
