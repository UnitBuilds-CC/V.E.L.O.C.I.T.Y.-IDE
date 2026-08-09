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
