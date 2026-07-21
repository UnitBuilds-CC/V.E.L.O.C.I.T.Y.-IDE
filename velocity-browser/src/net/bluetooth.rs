#[derive(Debug, Clone)]
pub struct BluetoothDevice {
    pub id: String,
    pub name: String,
    pub gatt_services: Vec<String>,
}

pub struct WebBluetoothTransport {
    pub discovered_devices: Vec<BluetoothDevice>,
}

impl WebBluetoothTransport {
    pub fn new() -> Self {
        Self { discovered_devices: Vec::new() }
    }

    pub fn request_device(&mut self, name_filter: &str) -> Option<BluetoothDevice> {
        let dev = BluetoothDevice {
            id: format!("bt_dev_{}", self.discovered_devices.len() + 1),
            name: name_filter.to_string(),
            gatt_services: vec!["0000180d-0000-1000-8000-00805f9b34fb".to_string()],
        };
        self.discovered_devices.push(dev.clone());
        Some(dev)
    }
}
