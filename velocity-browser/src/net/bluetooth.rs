/// A GATT characteristic on a Bluetooth device.
#[derive(Debug, Clone)]
pub struct GattCharacteristic {
    pub uuid: String,
    pub properties: Vec<String>, // "read", "write", "notify", "indicate"
    pub value: Vec<u8>,
}

/// A GATT service on a Bluetooth device.
#[derive(Debug, Clone)]
pub struct GattService {
    pub uuid: String,
    pub is_primary: bool,
    pub characteristics: Vec<GattCharacteristic>,
}

/// A discovered Bluetooth LE device.
#[derive(Debug, Clone)]
pub struct BluetoothDevice {
    pub id: String,
    pub name: String,
    pub gatt_services: Vec<String>,
    pub rssi: i16,
    pub is_connected: bool,
    pub services: Vec<GattService>,
}

/// Web Bluetooth transport with GATT service/characteristic interaction.
pub struct WebBluetoothTransport {
    pub discovered_devices: Vec<BluetoothDevice>,
    pub active_connection: Option<String>,
}

impl Default for WebBluetoothTransport {
    fn default() -> Self { Self::new() }
}

impl WebBluetoothTransport {
    pub fn new() -> Self {
        Self { discovered_devices: Vec::new(), active_connection: None }
    }

    /// Request a device by name filter.
    pub fn request_device(&mut self, name_filter: &str) -> Option<BluetoothDevice> {
        let dev = BluetoothDevice {
            id: format!("bt_dev_{}", self.discovered_devices.len() + 1),
            name: name_filter.to_string(),
            gatt_services: vec!["0000180d-0000-1000-8000-00805f9b34fb".to_string()],
            rssi: -50,
            is_connected: false,
            services: vec![GattService {
                uuid: "0000180d-0000-1000-8000-00805f9b34fb".to_string(),
                is_primary: true,
                characteristics: vec![GattCharacteristic {
                    uuid: "00002a37-0000-1000-8000-00805f9b34fb".to_string(),
                    properties: vec!["read".to_string(), "notify".to_string()],
                    value: Vec::new(),
                }],
            }],
        };
        self.discovered_devices.push(dev.clone());
        Some(dev)
    }

    /// Request a device with service filter.
    pub fn request_device_with_services(&mut self, service_uuids: &[&str]) -> Option<BluetoothDevice> {
        let dev = BluetoothDevice {
            id: format!("bt_dev_{}", self.discovered_devices.len() + 1),
            name: "Filtered Device".to_string(),
            gatt_services: service_uuids.iter().map(|s| s.to_string()).collect(),
            rssi: -60,
            is_connected: false,
            services: service_uuids.iter().map(|uuid| GattService {
                uuid: uuid.to_string(), is_primary: true, characteristics: Vec::new(),
            }).collect(),
        };
        self.discovered_devices.push(dev.clone());
        Some(dev)
    }

    /// Connect to a device by ID.
    pub fn connect(&mut self, device_id: &str) -> Result<(), &'static str> {
        let dev = self.discovered_devices.iter_mut().find(|d| d.id == device_id)
            .ok_or("Device not found")?;
        dev.is_connected = true;
        self.active_connection = Some(device_id.to_string());
        Ok(())
    }

    /// Disconnect from the active device.
    pub fn disconnect(&mut self) -> Result<(), &'static str> {
        if let Some(ref conn_id) = self.active_connection {
            if let Some(dev) = self.discovered_devices.iter_mut().find(|d| d.id == *conn_id) {
                dev.is_connected = false;
            }
        }
        self.active_connection = None;
        Ok(())
    }

    /// Read a characteristic value.
    pub fn read_characteristic(&self, device_id: &str, service_uuid: &str, char_uuid: &str) -> Result<Vec<u8>, &'static str> {
        let dev = self.discovered_devices.iter().find(|d| d.id == device_id)
            .ok_or("Device not found")?;
        if !dev.is_connected { return Err("Not connected"); }
        for svc in &dev.services {
            if svc.uuid == service_uuid {
                for ch in &svc.characteristics {
                    if ch.uuid == char_uuid {
                        return Ok(ch.value.clone());
                    }
                }
            }
        }
        Err("Characteristic not found")
    }

    /// Write a characteristic value.
    pub fn write_characteristic(&mut self, device_id: &str, service_uuid: &str, char_uuid: &str, data: &[u8]) -> Result<(), &'static str> {
        let dev = self.discovered_devices.iter_mut().find(|d| d.id == device_id)
            .ok_or("Device not found")?;
        if !dev.is_connected { return Err("Not connected"); }
        for svc in &mut dev.services {
            if svc.uuid == service_uuid {
                for ch in &mut svc.characteristics {
                    if ch.uuid == char_uuid {
                        if !ch.properties.contains(&"write".to_string()) {
                            return Err("Characteristic not writable");
                        }
                        ch.value = data.to_vec();
                        return Ok(());
                    }
                }
            }
        }
        Err("Characteristic not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_device() {
        let mut bt = WebBluetoothTransport::new();
        let dev = bt.request_device("HeartRate").unwrap();
        assert_eq!(dev.name, "HeartRate");
        assert!(!dev.gatt_services.is_empty());
    }

    #[test]
    fn test_connect_disconnect() {
        let mut bt = WebBluetoothTransport::new();
        let dev = bt.request_device("Device1").unwrap();
        bt.connect(&dev.id).unwrap();
        assert!(bt.discovered_devices[0].is_connected);
        bt.disconnect().unwrap();
        assert!(!bt.discovered_devices[0].is_connected);
    }

    #[test]
    fn test_read_write() {
        let mut bt = WebBluetoothTransport::new();
        let dev = bt.request_device("Device1").unwrap();
        let dev_id = dev.id.clone();
        bt.connect(&dev_id).unwrap();
        // Write
        let result = bt.write_characteristic(&dev_id, "0000180d-0000-1000-8000-00805f9b34fb", "00002a37-0000-1000-8000-00805f9b34fb", &[0x01, 0x02]);
        assert!(result.is_err()); // not writable (only read+notify)
    }

    #[test]
    fn test_request_with_services() {
        let mut bt = WebBluetoothTransport::new();
        let dev = bt.request_device_with_services(&["heart_rate", "battery"]).unwrap();
        assert_eq!(dev.gatt_services.len(), 2);
    }

    #[test]
    fn test_connect_nonexistent() {
        let mut bt = WebBluetoothTransport::new();
        assert!(bt.connect("nonexistent").is_err());
    }
}
