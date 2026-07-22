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

unsafe impl Send for ExecPage {}
unsafe impl Sync for ExecPage {}

impl ExecPage {
    #[cfg(windows)]
    pub fn allocate(size: usize) -> Option<Self> {
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
        assert!(offset + data.len() <= self.size);
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
        unsafe {
            VirtualFree(self.ptr as *mut _, 0, MEM_RELEASE);
        }
    }

    #[cfg(not(windows))]
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut _, self.size);
        }
    }
}
