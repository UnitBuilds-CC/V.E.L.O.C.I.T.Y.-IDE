#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_initializes_correctly() {
        let manager = StorageQuotaManager::new(1024);
        let estimate = manager.estimate();
        assert_eq!(estimate.usage_bytes, 0);
        assert_eq!(estimate.quota_bytes, 1024);
    }

    #[test]
    fn reserve_within_quota_updates_usage() {
        let mut manager = StorageQuotaManager::new(1000);
        assert!(manager.reserve(500).is_ok());
        assert_eq!(manager.estimate().usage_bytes, 500);
        
        assert!(manager.reserve(300).is_ok());
        assert_eq!(manager.estimate().usage_bytes, 800);
    }

    #[test]
    fn reserve_exceeding_quota_returns_error() {
        let mut manager = StorageQuotaManager::new(500);
        assert!(manager.reserve(400).is_ok());
        
        let result = manager.reserve(150);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "StorageQuotaExceededError: Quota limit hit");
        assert_eq!(manager.estimate().usage_bytes, 400);
    }

    #[test]
    fn exact_quota_allocation_succeeds() {
        let mut manager = StorageQuotaManager::new(1000);
        assert!(manager.reserve(1000).is_ok());
        assert_eq!(manager.estimate().usage_bytes, 1000);
    }

    #[test]
    fn cumulative_reservations_enforce_quota() {
        let mut manager = StorageQuotaManager::new(1500);
        let allocations = vec![200, 500, 300, 400, 200];
        let mut successful = 0;
        
        for bytes in allocations {
            if manager.reserve(bytes).is_ok() {
                successful += bytes;
            }
        }
        
        assert_eq!(successful, 200 + 500 + 300 + 400); // 1400
        assert_eq!(manager.estimate().usage_bytes, 1400);
    }

    #[test]
    fn zero_allocation_does_nothing() {
        let mut manager = StorageQuotaManager::new(100);
        assert!(manager.reserve(0).is_ok());
        assert_eq!(manager.estimate().usage_bytes, 0);
    }

    #[test]
    fn error_preserves_state() {
        let mut manager = StorageQuotaManager::new(200);
        assert!(manager.reserve(150).is_ok());
        let pre_error_state = manager.estimate().usage_bytes;
        
        let result = manager.reserve(100);
        assert!(result.is_err());
        assert_eq!(manager.estimate().usage_bytes, pre_error_state);
    }

    #[test]
    fn quota_boundary_handling() {
        let mut manager = StorageQuotaManager::new(usize::MAX);
        assert!(manager.reserve(usize::MAX - 1).is_ok());
        assert!(manager.reserve(1).is_ok());
        assert!(manager.reserve(1).is_err());
    }
}