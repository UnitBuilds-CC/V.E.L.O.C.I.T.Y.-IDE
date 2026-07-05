use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::path::Path;
use std::error::Error;

// Shared Memory layout specs:
// Offset 0: State byte (0 = Idle, 1 = Host Request, 2 = Server Processing, 3 = Host Response Ready, 4 = Error)
// Offset 1..5: Input buffer length (u32, little endian)
// Offset 5..9: Output buffer length (u32, little endian)
// Offset 10..4096: Input request buffer
// Offset 4096..65536: Output response buffer (supports up to 61KB responses)

const STATE_OFFSET: usize = 0;
const INPUT_LEN_OFFSET: usize = 1;
const OUTPUT_LEN_OFFSET: usize = 5;
const INPUT_BUFFER_OFFSET: usize = 10;
const OUTPUT_BUFFER_OFFSET: usize = 4096;
const TOTAL_BUFFER_SIZE: usize = 65536;

pub const STATE_IDLE: u8 = 0;
pub const STATE_REQ_READY: u8 = 1;
pub const STATE_PROCESSING: u8 = 2;
pub const STATE_RES_READY: u8 = 3;
pub const STATE_ERROR: u8 = 4;

#[cfg(target_os = "windows")]
extern "system" {
    fn CreateEventW(
        lpEventAttributes: *mut std::ffi::c_void,
        bManualReset: i32,
        bInitialState: i32,
        lpName: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetEvent(hEvent: *mut std::ffi::c_void) -> i32;
    fn WaitForSingleObject(hHandle: *mut std::ffi::c_void, dwMilliseconds: u32) -> u32;
    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
}

#[cfg(target_os = "windows")]
fn to_wstring(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
pub struct SharedMemoryBuffer {
    mmap: MmapMut,
    h_req_event: *mut std::ffi::c_void,
    h_res_event: *mut std::ffi::c_void,
}

#[cfg(not(target_os = "windows"))]
pub struct SharedMemoryBuffer {
    mmap: MmapMut,
}

impl SharedMemoryBuffer {
    #[cfg(target_os = "windows")]
    pub fn create_or_open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;

        file.set_len(TOTAL_BUFFER_SIZE as u64)?;

        let mmap = unsafe { MmapMut::map_mut(&file)? };

        let file_name = path.as_ref()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default");

        let req_event_name = format!("Global\\VELOCITY_NMCP_REQ_{}", file_name);
        let res_event_name = format!("Global\\VELOCITY_NMCP_RES_{}", file_name);

        let w_req = to_wstring(&req_event_name);
        let w_res = to_wstring(&res_event_name);

        let h_req_event = unsafe {
            CreateEventW(std::ptr::null_mut(), 0, 0, w_req.as_ptr())
        };
        let h_res_event = unsafe {
            CreateEventW(std::ptr::null_mut(), 0, 0, w_res.as_ptr())
        };

        if h_req_event.is_null() || h_res_event.is_null() {
            return Err("Failed to create Win32 Event objects".into());
        }

        let mut buffer = SharedMemoryBuffer {
            mmap,
            h_req_event,
            h_res_event,
        };

        if buffer.get_state() == 0 && buffer.get_input_len() == 0 {
            buffer.set_state(STATE_IDLE);
        }

        Ok(buffer)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn create_or_open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        file.set_len(TOTAL_BUFFER_SIZE as u64)?;

        let mmap = unsafe { MmapMut::map_mut(&file)? };

        let mut buffer = SharedMemoryBuffer { mmap };

        if buffer.get_state() == 0 && buffer.get_input_len() == 0 {
            buffer.set_state(STATE_IDLE);
        }

        Ok(buffer)
    }

    #[cfg(target_os = "windows")]
    pub fn wait_for_request(&self) {
        unsafe {
            WaitForSingleObject(self.h_req_event, 0xFFFFFFFF);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn wait_for_request(&self) {
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    #[cfg(target_os = "windows")]
    pub fn signal_response(&self) {
        unsafe {
            SetEvent(self.h_res_event);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn signal_response(&self) {
        // No-op fallback
    }

    pub fn get_state(&self) -> u8 {
        self.mmap[STATE_OFFSET]
    }

    pub fn set_state(&mut self, state: u8) {
        self.mmap[STATE_OFFSET] = state;
    }

    pub fn get_input_len(&self) -> u32 {
        u32::from_le_bytes([
            self.mmap[INPUT_LEN_OFFSET],
            self.mmap[INPUT_LEN_OFFSET + 1],
            self.mmap[INPUT_LEN_OFFSET + 2],
            self.mmap[INPUT_LEN_OFFSET + 3],
        ])
    }

    pub fn set_input_len(&mut self, len: u32) {
        let bytes = len.to_le_bytes();
        self.mmap[INPUT_LEN_OFFSET..INPUT_LEN_OFFSET + 4].copy_from_slice(&bytes);
    }

    pub fn get_output_len(&self) -> u32 {
        u32::from_le_bytes([
            self.mmap[OUTPUT_LEN_OFFSET],
            self.mmap[OUTPUT_LEN_OFFSET + 1],
            self.mmap[OUTPUT_LEN_OFFSET + 2],
            self.mmap[OUTPUT_LEN_OFFSET + 3],
        ])
    }

    pub fn set_output_len(&mut self, len: u32) {
        let bytes = len.to_le_bytes();
        self.mmap[OUTPUT_LEN_OFFSET..OUTPUT_LEN_OFFSET + 4].copy_from_slice(&bytes);
    }

    pub fn read_input(&self) -> Result<String, Box<dyn Error>> {
        let len = self.get_input_len() as usize;
        if len > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Input length exceeds buffer limit".into());
        }
        let bytes = &self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + len];
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    pub fn write_output(&mut self, response: &str) -> Result<(), Box<dyn Error>> {
        let bytes = response.as_bytes();
        if bytes.len() > (TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET) {
            return Err("Response length exceeds output buffer limit".into());
        }

        self.set_output_len(bytes.len() as u32);
        self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    pub fn flush(&self) -> Result<(), Box<dyn Error>> {
        self.mmap.flush()?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
impl Drop for SharedMemoryBuffer {
    fn drop(&mut self) {
        unsafe {
            if !self.h_req_event.is_null() {
                CloseHandle(self.h_req_event);
            }
            if !self.h_res_event.is_null() {
                CloseHandle(self.h_res_event);
            }
        }
    }
}
