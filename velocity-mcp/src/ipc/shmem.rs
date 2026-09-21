use hmac::{Hmac, Mac};
use memmap2::MmapMut;
use sha2::Sha256;
use std::error::Error;
use std::fs::OpenOptions;
use std::path::Path;

// Shared Memory layout specs (v2 - authenticated):
// Offset 0: State byte (0 = Idle, 1 = Host Request, 2 = Server Processing, 3 = Host Response Ready, 4 = Error)
// Offset 1..5: Input buffer length (u32, little endian)
// Offset 5..9: Output buffer length (u32, little endian)
// Offset 9..13: Sequence number (u32, little endian) - replay protection
// Offset 13..17: Flags (u32, little endian) - bit 0 = HMAC enabled
// Offset 17..49: HMAC-SHA256 tag (32 bytes) - message authentication
// Offset 49..81: Reserved (32 bytes) - future use
// Offset 81..4128: Input request buffer (4047 bytes)
// Offset 4128..65536: Output response buffer (61408 bytes)

const STATE_OFFSET: usize = 0;
const INPUT_LEN_OFFSET: usize = 1;
const OUTPUT_LEN_OFFSET: usize = 5;
const SEQ_OFFSET: usize = 9;
const FLAGS_OFFSET: usize = 13;
const HMAC_OFFSET: usize = 17;
const RESERVED_OFFSET: usize = 49;
/// Size of the reserved region between the HMAC tag and the input buffer.
const RESERVED_SIZE: usize = INPUT_BUFFER_OFFSET - RESERVED_OFFSET; // 32 bytes
const INPUT_BUFFER_OFFSET: usize = 81;
const OUTPUT_BUFFER_OFFSET: usize = 4128;
const TOTAL_BUFFER_SIZE: usize = 65536;

const FLAG_HMAC_ENABLED: u32 = 0x1;

pub const STATE_IDLE: u8 = 0;
pub const STATE_REQ_READY: u8 = 1;
pub const STATE_PROCESSING: u8 = 2;
pub const STATE_RES_READY: u8 = 3;
pub const STATE_ERROR: u8 = 4;

type HmacSha256 = Hmac<Sha256>;

// SAFETY: CreateEventW/SetEvent/WaitForSingleObject/CloseHandle are Windows
// kernel32 synchronization primitives. We create events with valid parameters,
// signal/wait on them within their documented lifetime, and close handles to
// avoid resource leaks.
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

/// How long [`SharedMemoryBuffer::wait_for_request`] watches for a request
/// before handing control back to `listen`, which re-checks and loops.
#[cfg(not(target_os = "windows"))]
const REQUEST_WAIT_BUDGET: std::time::Duration = std::time::Duration::from_millis(50);

/// How long a client waits for the server to move off the request side. The
/// handler does real work between those two states, so this has to outlast a
/// slow one rather than assume a fixed slice of time has passed.
#[cfg(not(target_os = "windows"))]
const RESPONSE_WAIT_BUDGET: std::time::Duration = std::time::Duration::from_millis(2_000);

/// Poll `pending` until it goes false or the budget runs out. The Windows build
/// blocks on named events instead; both mappings of one file back onto the same
/// pages, so the state word is already the source of truth here and all that was
/// missing is a reader that waits for it rather than sleeping and hoping.
#[cfg(not(target_os = "windows"))]
fn poll_while(budget: std::time::Duration, mut pending: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + budget;
    while pending() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_micros(500));
    }
}

impl SharedMemoryBuffer {
    #[cfg(target_os = "windows")]
    pub fn create_or_open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false) // preserve existing buffer content; set_len handles sizing
            .open(&path)?;

        file.set_len(TOTAL_BUFFER_SIZE as u64)?;

        // SAFETY: MmapMut::map_mut creates a mutable memory-mapped view of the file.
        // The file was opened with read+write+create and sized to TOTAL_BUFFER_SIZE.
        // The mapping is valid for the lifetime of the file handle (held by mmap).
        let mmap = unsafe { MmapMut::map_mut(&file)? };

        let file_name = path
            .as_ref()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default");

        let req_event_name = format!("Global\\VELOCITY_NMCP_REQ_{}", file_name);
        let res_event_name = format!("Global\\VELOCITY_NMCP_RES_{}", file_name);

        let w_req = to_wstring(&req_event_name);
        let w_res = to_wstring(&res_event_name);

        // SAFETY: CreateEventW with null security attributes and manual reset (0).
        // The event names are null-terminated wide strings from to_wstring().
        // We check for null handles and return Err if creation fails.
        let h_req_event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, w_req.as_ptr()) };
        // SAFETY: Same as h_req_event — CreateEventW with valid null-terminated name.
        let h_res_event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, w_res.as_ptr()) };

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
            .truncate(false) // preserve existing buffer content; set_len handles sizing
            .open(path)?;

        file.set_len(TOTAL_BUFFER_SIZE as u64)?;

        // SAFETY: MmapMut::map_mut creates a mutable memory-mapped view of the file.
        // The file was opened with read+write+create and sized to TOTAL_BUFFER_SIZE.
        // The mapping is valid for the lifetime of the file handle (held by mmap).
        let mmap = unsafe { MmapMut::map_mut(&file)? };

        let mut buffer = SharedMemoryBuffer { mmap };

        if buffer.get_state() == 0 && buffer.get_input_len() == 0 {
            buffer.set_state(STATE_IDLE);
        }

        Ok(buffer)
    }

    #[cfg(target_os = "windows")]
    pub fn wait_for_request(&self) {
        // SAFETY: self.h_req_event is a valid event handle created by CreateEventW.
        // INFINITE (0xFFFFFFFF) timeout blocks until the event is signaled.
        unsafe {
            WaitForSingleObject(self.h_req_event, 0xFFFFFFFF);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn wait_for_request(&self) {
        // There is no event object to block on here, so the state word in the
        // mapping is polled. Bounded, because `listen` treats a non-ready state
        // as "loop again" and has to keep iterating on its own.
        poll_while(REQUEST_WAIT_BUDGET, || self.get_state() != STATE_REQ_READY);
    }

    #[cfg(target_os = "windows")]
    pub fn signal_response(&self) {
        // SAFETY: self.h_res_event is a valid event handle created by CreateEventW.
        // SetEvent sets it to signaled state, unblocking any waiting threads.
        unsafe {
            SetEvent(self.h_res_event);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn signal_response(&self) {
        // Nothing to wake: the peer watches the state word, and the preceding
        // `set_state` plus `flush` has already published the transition.
    }

    #[cfg(target_os = "windows")]
    pub fn signal_request(&self) {
        // SAFETY: self.h_req_event is a valid event handle created by CreateEventW.
        unsafe {
            SetEvent(self.h_req_event);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn signal_request(&self) {
        // As above - the write of STATE_REQ_READY is the notification.
    }

    #[cfg(target_os = "windows")]
    pub fn wait_for_response(&self) {
        // SAFETY: self.h_res_event is a valid event handle created by CreateEventW.
        // INFINITE timeout blocks until the response is signaled.
        unsafe {
            WaitForSingleObject(self.h_res_event, 0xFFFFFFFF);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn wait_for_response(&self) {
        // The client has already published STATE_REQ_READY, so "answered" means
        // the state has left the request side: a response is ready, or the
        // server refused. A fixed sleep cannot stand in for that - the handler
        // does real work, and reading early returns whatever the previous
        // exchange left in the output buffer, which fails HMAC rather than
        // showing up as a timeout.
        poll_while(RESPONSE_WAIT_BUDGET, || {
            let state = self.get_state();
            state != STATE_RES_READY && state != STATE_ERROR
        });
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

    // ─── Sequence Number (Replay Protection) ──────────────────────────────────

    pub fn get_sequence(&self) -> u32 {
        u32::from_le_bytes([
            self.mmap[SEQ_OFFSET],
            self.mmap[SEQ_OFFSET + 1],
            self.mmap[SEQ_OFFSET + 2],
            self.mmap[SEQ_OFFSET + 3],
        ])
    }

    pub fn set_sequence(&mut self, seq: u32) {
        let bytes = seq.to_le_bytes();
        self.mmap[SEQ_OFFSET..SEQ_OFFSET + 4].copy_from_slice(&bytes);
    }

    // ─── Flags ────────────────────────────────────────────────────────────────

    pub fn get_flags(&self) -> u32 {
        u32::from_le_bytes([
            self.mmap[FLAGS_OFFSET],
            self.mmap[FLAGS_OFFSET + 1],
            self.mmap[FLAGS_OFFSET + 2],
            self.mmap[FLAGS_OFFSET + 3],
        ])
    }

    pub fn set_flags(&mut self, flags: u32) {
        let bytes = flags.to_le_bytes();
        self.mmap[FLAGS_OFFSET..FLAGS_OFFSET + 4].copy_from_slice(&bytes);
    }

    pub fn is_hmac_enabled(&self) -> bool {
        (self.get_flags() & FLAG_HMAC_ENABLED) != 0
    }

    // ─── HMAC (Message Authentication) ────────────────────────────────────────

    pub fn get_hmac(&self) -> [u8; 32] {
        let mut hmac = [0u8; 32];
        hmac.copy_from_slice(&self.mmap[HMAC_OFFSET..HMAC_OFFSET + 32]);
        hmac
    }

    pub fn set_hmac(&mut self, hmac: &[u8; 32]) {
        self.mmap[HMAC_OFFSET..HMAC_OFFSET + 32].copy_from_slice(hmac);
    }

    pub fn clear_hmac(&mut self) {
        self.mmap[HMAC_OFFSET..HMAC_OFFSET + 32].fill(0);
    }

    // ─── Reserved Region ────────────────────────────────────────────────────

    /// Read the 32-byte reserved region (offset 49..81).
    /// Reserved for future protocol extensions (e.g. capability flags, versioning).
    pub fn get_reserved(&self) -> [u8; RESERVED_SIZE] {
        let mut reserved = [0u8; RESERVED_SIZE];
        reserved.copy_from_slice(&self.mmap[RESERVED_OFFSET..RESERVED_OFFSET + RESERVED_SIZE]);
        reserved
    }

    /// Write to the 32-byte reserved region (offset 49..81).
    pub fn set_reserved(&mut self, data: &[u8; RESERVED_SIZE]) {
        self.mmap[RESERVED_OFFSET..RESERVED_OFFSET + RESERVED_SIZE].copy_from_slice(data);
    }

    /// Zero-fill the reserved region.
    pub fn clear_reserved(&mut self) {
        self.mmap[RESERVED_OFFSET..RESERVED_OFFSET + RESERVED_SIZE].fill(0);
    }

    /// Compute HMAC-SHA256 over sequence number + message bytes
    fn compute_hmac(key: &[u8], seq: u32, data: &[u8]) -> Result<[u8; 32], Box<dyn Error>> {
        let mut mac = HmacSha256::new_from_slice(key)
            .map_err(|e| -> Box<dyn Error> { format!("HMAC key error: {}", e).into() })?;
        mac.update(&seq.to_le_bytes());
        mac.update(data);
        let result = mac.finalize();
        let mut hmac = [0u8; 32];
        hmac.copy_from_slice(&result.into_bytes());
        Ok(hmac)
    }

    /// Verify HMAC-SHA256 tag over sequence number + message bytes
    fn verify_hmac(key: &[u8], seq: u32, data: &[u8], expected: &[u8; 32]) -> bool {
        if let Ok(computed) = Self::compute_hmac(key, seq, data) {
            // Constant-time comparison
            let mut diff = 0u8;
            for (a, b) in computed.iter().zip(expected.iter()) {
                diff |= a ^ b;
            }
            diff == 0
        } else {
            false
        }
    }

    // ─── Authenticated Message API ────────────────────────────────────────────

    /// Write authenticated message to input buffer with HMAC + sequence number
    pub fn write_authenticated_input(
        &mut self,
        request: &str,
        key: &[u8],
        seq: u32,
    ) -> Result<(), Box<dyn Error>> {
        let bytes = request.as_bytes();
        if bytes.len() > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Request length exceeds input buffer limit".into());
        }

        // Write sequence number
        self.set_sequence(seq);

        // Write message
        self.set_input_len(bytes.len() as u32);
        self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + bytes.len()].copy_from_slice(bytes);

        // Compute and write HMAC
        let hmac = Self::compute_hmac(key, seq, bytes)?;
        self.set_hmac(&hmac);

        // Enable HMAC flag
        self.set_flags(self.get_flags() | FLAG_HMAC_ENABLED);

        Ok(())
    }

    /// Read and verify authenticated message from input buffer
    pub fn read_authenticated_input(
        &self,
        key: &[u8],
        expected_seq: Option<u32>,
    ) -> Result<(String, u32), Box<dyn Error>> {
        let len = self.get_input_len() as usize;
        if len > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Input length exceeds buffer limit".into());
        }

        let seq = self.get_sequence();
        let bytes = &self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + len];
        let stored_hmac = self.get_hmac();

        // Verify sequence number if provided
        if let Some(expected) = expected_seq {
            if seq != expected {
                return Err(format!(
                    "Sequence number mismatch: expected {}, got {}",
                    expected, seq
                )
                .into());
            }
        }

        // Verify HMAC if enabled
        if self.is_hmac_enabled() && !Self::verify_hmac(key, seq, bytes, &stored_hmac) {
            return Err("HMAC verification failed - message authentication error".into());
        }

        let msg = String::from_utf8(bytes.to_vec())?;
        Ok((msg, seq))
    }

    /// Write authenticated message to output buffer with HMAC + sequence number
    pub fn write_authenticated_output(
        &mut self,
        response: &str,
        key: &[u8],
        seq: u32,
    ) -> Result<(), Box<dyn Error>> {
        let bytes = response.as_bytes();
        if bytes.len() > (TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET) {
            return Err("Response length exceeds output buffer limit".into());
        }

        // Write sequence number
        self.set_sequence(seq);

        // Write message
        self.set_output_len(bytes.len() as u32);
        self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + bytes.len()].copy_from_slice(bytes);

        // Compute and write HMAC
        let hmac = Self::compute_hmac(key, seq, bytes)?;
        self.set_hmac(&hmac);

        // Enable HMAC flag
        self.set_flags(self.get_flags() | FLAG_HMAC_ENABLED);

        Ok(())
    }

    /// Read and verify authenticated message from output buffer
    pub fn read_authenticated_output(
        &self,
        key: &[u8],
        expected_seq: Option<u32>,
    ) -> Result<(String, u32), Box<dyn Error>> {
        let len = self.get_output_len() as usize;
        if len > (TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET) {
            return Err("Output length exceeds buffer limit".into());
        }

        let seq = self.get_sequence();
        let bytes = &self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + len];
        let stored_hmac = self.get_hmac();

        // Verify sequence number if provided
        if let Some(expected) = expected_seq {
            if seq != expected {
                return Err(format!(
                    "Sequence number mismatch: expected {}, got {}",
                    expected, seq
                )
                .into());
            }
        }

        // Verify HMAC if enabled
        if self.is_hmac_enabled() && !Self::verify_hmac(key, seq, bytes, &stored_hmac) {
            return Err("HMAC verification failed - message authentication error".into());
        }

        let msg = String::from_utf8(bytes.to_vec())?;
        Ok((msg, seq))
    }

    // ─── Legacy Unauthenticated API (backward compatibility) ──────────────────

    pub fn read_input(&self) -> Result<String, Box<dyn Error>> {
        let len = self.get_input_len() as usize;
        if len > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Input length exceeds buffer limit".into());
        }
        let bytes = &self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + len];
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    pub fn write_input(&mut self, request: &str) -> Result<(), Box<dyn Error>> {
        let bytes = request.as_bytes();
        if bytes.len() > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Request length exceeds input buffer limit".into());
        }
        self.set_input_len(bytes.len() as u32);
        self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    pub fn read_output(&self) -> Result<String, Box<dyn Error>> {
        let len = self.get_output_len() as usize;
        if len > (TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET) {
            return Err("Output length exceeds buffer limit".into());
        }
        let bytes = &self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + len];
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
        // SAFETY: CloseHandle is called exactly once via Drop, preventing handle leaks.
        // The handles were created by CreateEventW and are checked for null before use.
        // Null checks prevent closing invalid handles.
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
