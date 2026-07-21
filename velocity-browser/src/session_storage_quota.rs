#[derive(Debug, Clone)]
pub struct StorageQuotaEstimate {
    pub usage_bytes: usize,
    pub quota_bytes: usize,
}

pub struct StorageQuotaManager {
    pub quota_bytes: usize,
    pub current_usage: usize,
}

impl StorageQuotaManager {
    pub fn new(quota_bytes: usize) -> Self {
        Self {
            quota_bytes,
            current_usage: 0,
        }
    }

    pub fn estimate(&self) -> StorageQuotaEstimate {
        StorageQuotaEstimate {
            usage_bytes: self.current_usage,
            quota_bytes: self.quota_bytes,
        }
    }

    pub fn reserve(&mut self, bytes: usize) -> Result<(), String> {
        if self.current_usage + bytes > self.quota_bytes {
            Err("StorageQuotaExceededError: Quota limit hit".to_string())
        } else {
            self.current_usage += bytes;
            Ok(())
        }
    }
}
