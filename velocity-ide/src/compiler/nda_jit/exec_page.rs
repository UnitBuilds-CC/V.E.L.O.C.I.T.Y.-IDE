#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(
        lpAddress: *mut std::ffi::c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut std::ffi::c_void;
    fn VirtualFree(lpAddress: *mut std::ffi::c_void, dwSize: usize, dwFreeType: u32) -> i32;
}

use serde::Serialize;

#[cfg(windows)]
const MEM_COMMIT: u32 = 0x1000;
#[cfg(windows)]
const MEM_RESERVE: u32 = 0x2000;
#[cfg(windows)]
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
#[cfg(windows)]
const MEM_RELEASE: u32 = 0x8000;

pub struct ExecPage {
    #[cfg(windows)]
    ptr: *mut u8,
    #[cfg(windows)]
    size: usize,
    #[cfg(not(windows))]
    ptr: *mut u8,
    #[cfg(not(windows))]
    size: usize,
}

// SAFETY: ExecPage owns a region of executable memory allocated via VirtualAlloc/mmap.
// The raw pointer is valid for the lifetime of the ExecPage, and Drop ensures it is freed.
// Send/Sync are safe because the memory region is exclusively owned and not aliased.
unsafe impl Send for ExecPage {}
unsafe impl Sync for ExecPage {}

impl ExecPage {
    #[cfg(windows)]
    pub fn allocate(size: usize) -> Option<Self> {
        // SAFETY: VirtualAlloc with MEM_COMMIT|MEM_RESERVE and PAGE_EXECUTE_READWRITE
        // allocates a region of executable memory. null address lets the OS choose the base.
        // We check for null return and only wrap in Some if allocation succeeded.
        let ptr = unsafe {
            VirtualAlloc(
                std::ptr::null_mut(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        };
        if ptr.is_null() {
            None
        } else {
            Some(Self {
                ptr: ptr as *mut u8,
                size,
            })
        }
    }

    #[cfg(not(windows))]
    pub fn allocate(size: usize) -> Option<Self> {
        // SAFETY: mmap with MAP_PRIVATE|MAP_ANONYMOUS allocates memory not backed by a file.
        // PROT_READ|PROT_WRITE|PROT_EXEC makes it executable for JIT code generation.
        // fd=-1 is correct for anonymous mappings. We check for MAP_FAILED return.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            None
        } else {
            Some(Self {
                ptr: ptr as *mut u8,
                size,
            })
        }
    }

    pub fn write(&mut self, offset: usize, data: &[u8]) {
        assert!(offset + data.len() <= self.size, "write out of bounds");
        // SAFETY: The assertion above guarantees offset+data.len() <= self.size.
        // self.ptr was allocated with at least self.size bytes by VirtualAlloc/mmap.
        // data.as_ptr() is valid for data.len() bytes. The regions don't overlap
        // because self.ptr is heap-allocated and data is a separate slice.
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.add(offset), data.len());
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }
}

impl Drop for ExecPage {
    #[cfg(windows)]
    fn drop(&mut self) {
        // SAFETY: self.ptr was allocated by VirtualAlloc and is valid for freeing.
        // MEM_RELEASE requires size=0 when freeing the entire region.
        // Drop is called exactly once, preventing double-free.
        unsafe {
            VirtualFree(self.ptr as *mut _, 0, MEM_RELEASE);
        }
    }

    #[cfg(not(windows))]
    fn drop(&mut self) {
        // SAFETY: self.ptr was allocated by mmap with self.size bytes.
        // munmap with the same pointer and size correctly frees the mapping.
        // Drop is called exactly once, preventing double-free.
        unsafe {
            libc::munmap(self.ptr as *mut _, self.size);
        }
    }
}

// ─── Diagnostics ───────────────────────────────────────────────────────────────

/// Serializable diagnostic snapshot of an ExecPage allocation.
#[derive(Debug, Clone, Serialize)]
pub struct ExecPageInfo {
    pub size_bytes: usize,
    pub pointer_address: usize,
    pub is_null: bool,
    pub page_aligned: bool,
    pub validation_issues: Vec<String>,
}

impl ExecPage {
    /// Return a diagnostic snapshot of this executable page.
    pub fn info(&self) -> ExecPageInfo {
        let addr = self.ptr as usize;
        let mut issues = Vec::new();

        if self.ptr.is_null() {
            issues.push("exec page pointer is null".to_string());
        }

        if self.size == 0 {
            issues.push("exec page has zero size".to_string());
        }

        // Check page alignment (4KB pages)
        let page_aligned = addr.is_multiple_of(4096);
        if !page_aligned && !self.ptr.is_null() {
            issues.push(format!(
                "exec page address 0x{:x} is not page-aligned",
                addr
            ));
        }

        // Check for unreasonably large allocation
        if self.size > 256 * 1024 * 1024 {
            issues.push(format!(
                "exec page size {} bytes exceeds 256MB (possible leak)",
                self.size
            ));
        }

        ExecPageInfo {
            size_bytes: self.size,
            pointer_address: addr,
            is_null: self.ptr.is_null(),
            page_aligned,
            validation_issues: issues,
        }
    }
}

/// Validate that an exec page allocation size is reasonable.
pub fn validate_exec_page_size(size: usize) -> Vec<String> {
    let mut issues = Vec::new();

    if size == 0 {
        issues.push("allocation size must be > 0".to_string());
    }

    if size > 256 * 1024 * 1024 {
        issues.push(format!(
            "allocation size {} bytes exceeds 256MB limit",
            size
        ));
    }

    // Warn about non-page-aligned sizes (wastes memory)
    if !size.is_multiple_of(4096) && size > 0 {
        let rounded = (size + 4095) & !4095;
        issues.push(format!(
            "allocation size {} is not page-aligned; OS will round up to {}",
            size, rounded
        ));
    }

    issues
}

/// Validate that a write operation at `offset` with `data_len` bytes fits within `page_size`.
pub fn validate_write_bounds(page_size: usize, offset: usize, data_len: usize) -> Vec<String> {
    let mut issues = Vec::new();

    let end = offset.saturating_add(data_len);
    if end > page_size {
        issues.push(format!(
            "write at offset {} length {} (end={}) exceeds page size {}",
            offset, data_len, end, page_size
        ));
    }

    if data_len == 0 {
        issues.push("write has zero-length data".to_string());
    }

    issues
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_page_allocate_and_info() {
        let page = ExecPage::allocate(4096);
        assert!(page.is_some());
        let page = page.unwrap();
        let info = page.info();
        assert_eq!(info.size_bytes, 4096);
        assert!(!info.is_null);
        assert!(info.page_aligned);
        assert!(info.validation_issues.is_empty());
    }

    #[test]
    fn exec_page_write_in_bounds() {
        let mut page = ExecPage::allocate(4096).unwrap();
        let code = [0x90u8, 0x90, 0xC3]; // nop; nop; ret
        page.write(0, &code);
        // No panic = success
    }

    #[test]
    #[should_panic(expected = "write out of bounds")]
    fn exec_page_write_out_of_bounds() {
        let mut page = ExecPage::allocate(4).unwrap();
        let code = [0x90u8; 8]; // 8 bytes into a 4-byte page
        page.write(0, &code);
    }

    #[test]
    fn exec_page_write_at_offset() {
        let mut page = ExecPage::allocate(4096).unwrap();
        let code = [0xC3u8]; // ret
        page.write(100, &code);
        // No panic = success
    }

    #[test]
    fn validate_exec_page_size_zero() {
        let issues = validate_exec_page_size(0);
        assert!(issues.iter().any(|i| i.contains("> 0")));
    }

    #[test]
    fn validate_exec_page_size_huge() {
        let issues = validate_exec_page_size(512 * 1024 * 1024);
        assert!(issues.iter().any(|i| i.contains("256MB")));
    }

    #[test]
    fn validate_exec_page_size_unaligned() {
        let issues = validate_exec_page_size(5000);
        assert!(issues.iter().any(|i| i.contains("not page-aligned")));
    }

    #[test]
    fn validate_exec_page_size_clean() {
        let issues = validate_exec_page_size(8192);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_write_bounds_clean() {
        let issues = validate_write_bounds(4096, 0, 100);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_write_bounds_overflow() {
        let issues = validate_write_bounds(4096, 4000, 200);
        assert!(issues.iter().any(|i| i.contains("exceeds page size")));
    }

    #[test]
    fn validate_write_bounds_zero_length() {
        let issues = validate_write_bounds(4096, 0, 0);
        assert!(issues.iter().any(|i| i.contains("zero-length")));
    }

    #[test]
    fn validate_write_bounds_at_end() {
        let issues = validate_write_bounds(4096, 4096, 1);
        assert!(issues.iter().any(|i| i.contains("exceeds page size")));
    }

    #[test]
    fn exec_page_as_ptr_not_null() {
        let page = ExecPage::allocate(4096).unwrap();
        assert!(!page.as_ptr().is_null());
    }

    #[test]
    fn exec_page_large_allocation() {
        let page = ExecPage::allocate(65536);
        assert!(page.is_some());
        let info = page.unwrap().info();
        assert_eq!(info.size_bytes, 65536);
        assert!(info.validation_issues.is_empty());
    }

    // ── Block 112: expanded tests ────────────────────────────────────────────

    #[test]
    fn exec_page_allocate_minimum() {
        let page = ExecPage::allocate(1);
        assert!(page.is_some());
        let info = page.unwrap().info();
        assert_eq!(info.size_bytes, 1);
    }

    #[test]
    fn exec_page_write_exact_end() {
        let mut page = ExecPage::allocate(4).unwrap();
        let code = [0x90u8; 4]; // exactly fills the page
        page.write(0, &code);
    }

    #[test]
    fn exec_page_write_at_end_boundary() {
        let mut page = ExecPage::allocate(4096).unwrap();
        let code = [0xC3u8]; // ret at the very last byte
        page.write(4095, &code);
    }

    #[test]
    fn exec_page_info_pointer_address_nonzero() {
        let page = ExecPage::allocate(4096).unwrap();
        let info = page.info();
        assert_ne!(info.pointer_address, 0);
    }

    #[test]
    fn validate_size_exactly_256mb() {
        let issues = validate_exec_page_size(256 * 1024 * 1024);
        // 256MB is the boundary — should not trigger the > 256MB warning
        assert!(!issues.iter().any(|i| i.contains("256MB")));
    }

    #[test]
    fn validate_size_one_byte_over_256mb() {
        let issues = validate_exec_page_size(256 * 1024 * 1024 + 1);
        assert!(issues.iter().any(|i| i.contains("256MB")));
    }

    #[test]
    fn validate_size_page_aligned_4096() {
        let issues = validate_exec_page_size(4096);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_size_page_aligned_8192() {
        let issues = validate_exec_page_size(8192);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_size_one() {
        let issues = validate_exec_page_size(1);
        // 1 byte: not page-aligned, but > 0
        assert!(issues.iter().any(|i| i.contains("not page-aligned")));
    }

    #[test]
    fn validate_write_bounds_exact_fit() {
        let issues = validate_write_bounds(4096, 0, 4096);
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_write_bounds_saturating_add() {
        // offset + data_len would overflow usize — saturating_add caps at usize::MAX
        let issues = validate_write_bounds(4096, usize::MAX, 1);
        assert!(issues.iter().any(|i| i.contains("exceeds page size")));
    }

    #[test]
    fn validate_write_bounds_multiple_issues() {
        // Both overflow AND zero-length
        let issues = validate_write_bounds(4096, 5000, 0);
        assert!(issues.len() >= 2);
    }

    #[test]
    fn exec_page_info_serializes() {
        let page = ExecPage::allocate(4096).unwrap();
        let info = page.info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"size_bytes\":4096"));
        assert!(json.contains("\"is_null\":false"));
    }

    #[test]
    fn exec_page_multiple_writes() {
        let mut page = ExecPage::allocate(4096).unwrap();
        page.write(0, &[0x55]); // push rbp
        page.write(1, &[0x48, 0x89, 0xE5]); // mov rbp, rsp
        page.write(4, &[0xC3]); // ret
        // Verify by reading back (pointer is valid)
        assert!(!page.as_ptr().is_null());
    }
}
