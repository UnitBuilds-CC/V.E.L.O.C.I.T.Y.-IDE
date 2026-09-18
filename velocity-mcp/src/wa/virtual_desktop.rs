//! Virtual Desktop management for Windows 10/11.
//!
//! Provides detection, enumeration, creation, removal, and switching of
//! Windows virtual desktops via the IVirtualDesktopManager COM interface
//! through PowerShell.

// ─── Virtual Desktop Model ───────────────────────────────────────────────────

/// A Windows virtual desktop.
#[derive(Debug, Clone)]
pub struct VirtualDesktop {
    /// Desktop GUID identifier.
    pub id: String,
    /// Desktop name (Windows 11 supports named desktops).
    pub name: Option<String>,
    /// 0-based index in the desktop list.
    pub index: u32,
    /// Whether this is the currently active desktop.
    pub is_current: bool,
    /// Number of windows on this desktop (if available).
    pub window_count: Option<u32>,
}

/// State of the virtual desktop system.
#[derive(Debug, Clone)]
pub struct VirtualDesktopState {
    pub desktops: Vec<VirtualDesktop>,
    pub current_index: u32,
    pub total_count: u32,
    pub supports_named_desktops: bool,
    /// Whether the desktop flagged as current was positively identified.
    /// Windows keeps `CurrentVirtualDesktop` only in some sessions, so
    /// `current_index` alone can be a placeholder rather than a measurement
    /// (bug #21).
    pub current_desktop_known: bool,
}

impl VirtualDesktopState {
    pub fn current(&self) -> Option<&VirtualDesktop> {
        self.desktops.iter().find(|d| d.is_current)
    }

    pub fn by_index(&self, index: u32) -> Option<&VirtualDesktop> {
        self.desktops.iter().find(|d| d.index == index)
    }

    pub fn by_name(&self, name: &str) -> Option<&VirtualDesktop> {
        self.desktops.iter().find(|d| {
            d.name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case(name))
                .unwrap_or(false)
        })
    }
}

// ─── Virtual Desktop Operations ──────────────────────────────────────────────

/// Operation to perform on virtual desktops.
#[derive(Debug, Clone)]
pub enum VDesktopOperation {
    /// Switch to desktop by index.
    SwitchTo(u32),
    /// Switch to desktop by name (Windows 11).
    SwitchToNamed(String),
    /// Create a new virtual desktop.
    Create { name: Option<String> },
    /// Remove a virtual desktop by index (windows moved to adjacent).
    Remove(u32),
    /// Move a window (by HWND) to a specific desktop.
    MoveWindow { hwnd: u64, desktop_index: u32 },
    /// Pin a window so it appears on all desktops.
    PinWindow(u64),
    /// Unpin a window.
    UnpinWindow(u64),
}

/// Result of a virtual desktop operation.
#[derive(Debug, Clone)]
pub struct VDesktopOpResult {
    pub success: bool,
    pub operation: String,
    pub detail: String,
    pub new_state: Option<VirtualDesktopState>,
}

/// Measured answer to "which desktop is this window on?".
#[derive(Debug, Clone)]
pub struct WindowDesktopProbe {
    pub hwnd: u64,
    /// False when `hwnd` is not a live top-level window at all.
    pub window_exists: bool,
    /// Index into the enumerated desktop list, when Windows would say.
    pub desktop_index: Option<u32>,
    /// The desktop GUID Windows reported for the window, when it reported one.
    pub desktop_id: Option<String>,
    /// Tri-state: `None` means Windows did not answer, not "no".
    pub on_current_desktop: Option<bool>,
    /// Why a lookup failed, when it did.
    pub reason: Option<String>,
}

// ─── Virtual Desktop Manager ─────────────────────────────────────────────────

/// Say concretely what an operation would do to the person at the keyboard.
///
/// Kept separate from [`session_guard::refusal`] so the gate's wording stays
/// uniform while each caller describes its own effect.
fn session_change_effect(op: &VDesktopOperation) -> String {
    match op {
        VDesktopOperation::SwitchTo(idx) => format!(
            "it sends the Ctrl+Win+Arrow shell hotkey to move you off the desktop you are looking at onto desktop {idx}"
        ),
        VDesktopOperation::SwitchToNamed(name) => {
            format!("it sends the Ctrl+Win+Arrow shell hotkey to move you onto the desktop named '{name}'")
        }
        VDesktopOperation::Create { name } => match name {
            Some(n) => format!(
                "it adds a new desktop named '{n}' and moves you onto it, hiding the windows on the one you are using"
            ),
            None => "it adds a new desktop and moves you onto it, hiding the windows on the one you are using"
                .to_string(),
        },
        VDesktopOperation::Remove(idx) => {
            format!("it closes desktop {idx} and relocates every window open on it, possibly the ones you are working in")
        }
        VDesktopOperation::MoveWindow {
            hwnd,
            desktop_index,
        } => format!(
            "it moves window {hwnd:#x} onto desktop {desktop_index}, so it disappears from the desktop you are looking at"
        ),
        VDesktopOperation::PinWindow(_) | VDesktopOperation::UnpinWindow(_) => {
            "it changes which desktops the window is visible on".to_string()
        }
    }
}

/// What a refusal looks like from this module.
///
/// Separated from [`VirtualDesktopManager::apply`] so the gate can be asserted
/// without building - let alone running - a script that would move a real
/// session (bug #40).
fn session_refusal(
    op: &VDesktopOperation,
    cached: &Option<VirtualDesktopState>,
) -> VDesktopOpResult {
    VDesktopOpResult {
        success: false,
        operation: format!("{op:?}"),
        detail: super::session_guard::refusal(
            "virtual desktop operation",
            &session_change_effect(op),
        ),
        new_state: cached.clone(),
    }
}

/// Manages virtual desktop operations via COM/PowerShell.
pub struct VirtualDesktopManager {
    /// Cached state (refreshed on enumerate).
    cached_state: Option<VirtualDesktopState>,
}

impl VirtualDesktopManager {
    pub fn new() -> Self {
        Self { cached_state: None }
    }

    /// Get the current state (uses cache if available).
    pub fn state(&self) -> Option<&VirtualDesktopState> {
        self.cached_state.as_ref()
    }

    /// Enumerate all virtual desktops (refreshes cache).
    pub fn enumerate(&mut self) -> &VirtualDesktopState {
        if cfg!(target_os = "windows") {
            let script = build_enumerate_desktops_script();
            if let Ok(json) = run_ps_script(&script) {
                if let Some(state) = parse_enumerate_result(&json) {
                    self.cached_state = Some(state);
                    return self.cached_state.as_ref().unwrap();
                }
            }
        }
        // Fallback: single desktop. Nothing was measured, so the current desktop
        // is only "known" where there is genuinely just one by construction.
        self.cached_state = Some(VirtualDesktopState {
            desktops: vec![VirtualDesktop {
                id: "default".to_string(),
                name: Some("Desktop 1".to_string()),
                index: 0,
                is_current: true,
                window_count: None,
            }],
            current_index: 0,
            total_count: 1,
            supports_named_desktops: cfg!(target_os = "windows"),
            current_desktop_known: !cfg!(target_os = "windows"),
        });
        self.cached_state.as_ref().unwrap()
    }

    /// Apply an operation.
    ///
    /// Every operation here rearranges the *interactive session* rather than a
    /// target the caller named: switching walks the desktop you are looking at,
    /// and creating one at the rightmost desktop is what a stray Ctrl+Win+Right
    /// does, which is how an unattended sweep ends up moving the user's own
    /// window set out of sight (bug #40). Refuse unless the operator opted in.
    pub fn apply(&mut self, op: &VDesktopOperation) -> VDesktopOpResult {
        if !cfg!(target_os = "windows") {
            return VDesktopOpResult {
                success: false,
                operation: format!("{:?}", op),
                detail: "Virtual desktop operations require Windows 10/11".to_string(),
                new_state: self.cached_state.clone(),
            };
        }
        if !super::session_guard::consent_given() {
            return session_refusal(op, &self.cached_state);
        }
        let script = match op {
            VDesktopOperation::SwitchTo(idx) => build_switch_desktop_script(*idx),
            VDesktopOperation::SwitchToNamed(name) => {
                // Resolve name to index via enumeration then switch
                self.enumerate();
                match self
                    .cached_state
                    .as_ref()
                    .and_then(|s| s.by_name(name))
                    .map(|d| d.index)
                {
                    Some(idx) => build_switch_desktop_script(idx),
                    None => {
                        return VDesktopOpResult {
                            success: false,
                            operation: format!("{op:?}"),
                            detail: format!(
                                "no desktop named '{name}' (this build exposes desktop names only where \
                                 Windows stores them; retry with index)"
                            ),
                            new_state: self.cached_state.clone(),
                        }
                    }
                }
            }
            VDesktopOperation::Create { name } => build_create_desktop_script(name.as_deref()),
            VDesktopOperation::Remove(idx) => build_remove_desktop_script(*idx),
            VDesktopOperation::MoveWindow {
                hwnd,
                desktop_index,
            } => return self.move_window(*hwnd, *desktop_index),
            VDesktopOperation::PinWindow(_) | VDesktopOperation::UnpinWindow(_) => {
                // Pinning needs IVirtualDesktopPinnedApps. The previous scripts set
                // WS_EX_TOOLWINDOW, which only hides a window from Alt+Tab, so every
                // "pinned" answer was wrong (bug #28).
                return VDesktopOpResult {
                    success: false,
                    operation: format!("{op:?}"),
                    detail:
                        "pin/unpin is not supported: the IVirtualDesktopPinnedApps coclass is not \
                             registered on this Windows build"
                            .to_string(),
                    new_state: self.cached_state.clone(),
                };
            }
        };
        // The script reports what it measured; trust that instead of assuming that
        // a clean exit means the desktop actually changed (bug #29).
        let (success, detail) = match run_ps_script(&script) {
            Ok(json) => describe_op_json(&json),
            Err(e) => (false, e),
        };
        self.enumerate();
        VDesktopOpResult {
            success,
            operation: format!("{op:?}"),
            detail,
            new_state: self.cached_state.clone(),
        }
    }

    /// Outcome of asking Windows which desktop a window lives on.
    ///
    /// The lookup goes through `IVirtualDesktopManager`, whose coclass is not
    /// registered on every Windows 11 build (bug #22: the previous code ignored
    /// `hwnd` entirely and answered "desktop 0, on the current desktop" for any
    /// input, including `hwnd = 0`). Where the coclass is missing, the fields
    /// stay `None` and `reason` explains what was unavailable.
    pub fn probe_window_desktop(&self, hwnd: u64) -> WindowDesktopProbe {
        if !cfg!(target_os = "windows") {
            return WindowDesktopProbe {
                hwnd,
                window_exists: false,
                desktop_index: None,
                desktop_id: None,
                on_current_desktop: None,
                reason: Some("virtual desktop lookup requires Windows".to_string()),
            };
        }
        let script = build_window_desktop_probe_script(hwnd);
        let json = match run_ps_script(&script) {
            Ok(json) => json,
            Err(e) => {
                return WindowDesktopProbe {
                    hwnd,
                    window_exists: false,
                    desktop_index: None,
                    desktop_id: None,
                    on_current_desktop: None,
                    reason: Some(e),
                }
            }
        };
        parse_window_desktop_probe(hwnd, &json)
    }

    /// Move `hwnd` to the desktop at `desktop_index`, verifying the result.
    pub fn move_window(&mut self, hwnd: u64, desktop_index: u32) -> VDesktopOpResult {
        if !super::session_guard::consent_given() {
            // Also reachable directly, not only through `apply`, so it gates for
            // itself rather than trusting its caller.
            return session_refusal(
                &VDesktopOperation::MoveWindow {
                    hwnd,
                    desktop_index,
                },
                &self.cached_state,
            );
        }
        self.enumerate();
        let target_id = match self
            .cached_state
            .as_ref()
            .and_then(|s| s.by_index(desktop_index))
            .map(|d| d.id.clone())
        {
            Some(id) => id,
            None => {
                return VDesktopOpResult {
                    success: false,
                    operation: format!(
                        "MoveWindow {{ hwnd: {hwnd}, desktop_index: {desktop_index} }}"
                    ),
                    detail: format!(
                        "no desktop at index {desktop_index} (known indices: {})",
                        self.cached_state
                            .as_ref()
                            .map(|s| s
                                .desktops
                                .iter()
                                .map(|d| d.index.to_string())
                                .collect::<Vec<_>>()
                                .join(", "))
                            .unwrap_or_default()
                    ),
                    new_state: self.cached_state.clone(),
                }
            }
        };
        let script = build_move_window_to_desktop_script(hwnd, &target_id, desktop_index);
        let detail = match run_ps_script(&script) {
            Ok(json) => describe_op_json(&json),
            Err(e) => (false, e),
        };
        self.enumerate();
        VDesktopOpResult {
            success: detail.0,
            operation: format!("MoveWindow {{ hwnd: {hwnd}, desktop_index: {desktop_index} }}"),
            detail: detail.1,
            new_state: self.cached_state.clone(),
        }
    }
}

impl Default for VirtualDesktopManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Shared PowerShell prelude: decode the shell's own virtual-desktop identifiers.
///
/// `HKCU\...\Explorer\VirtualDesktops` publishes two REG_BINARY values:
/// `VirtualDesktopIDs` (16 bytes per desktop, in task-view order) and
/// `CurrentVirtualDesktop` (the active one). Both are mixed-endian GUIDs, so
/// they must go through `[Guid]` before being compared with anything.
/// The `Desktops\{guid}` subkeys are keyed by a *different* GUID set on Windows
/// 11 25H2, which is why the old enumeration never matched the current desktop
/// and reported indices no other API recognised (bug #25).
const VD_PRELUDE: &str = r#"
$ErrorActionPreference = 'Stop'
$vdReg = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops"

function Format-VdGuid([byte[]]$bytes) {
    if ($null -eq $bytes -or $bytes.Length -lt 16) { return $null }
    $b = New-Object byte[] 16
    [Array]::Copy($bytes, $b, 16)
    try { return ([System.Guid]$b).ToString("B").ToUpperInvariant() } catch { return $null }
}

function Get-VdBlob([string]$name) {
    if (-not (Test-Path $vdReg)) { return $null }
    $item = Get-Item -Path $vdReg -ErrorAction SilentlyContinue
    if ($null -eq $item) { return $null }
    $v = $item.GetValue($name)
    if ($v -is [byte[]]) { return $v }
    return $null
}

function Get-VdDesktopIds {
    $blob = Get-VdBlob "VirtualDesktopIDs"
    $out = @()
    if ($null -eq $blob) { return , $out }
    for ($i = 0; ($i + 16) -le $blob.Length; $i += 16) {
        $b = New-Object byte[] 16
        [Array]::Copy($blob, $i, $b, 0, 16)
        $g = Format-VdGuid $b
        if ($null -ne $g) { $out += $g }
    }
    return , $out
}

function Get-VdCurrentId { return Format-VdGuid (Get-VdBlob "CurrentVirtualDesktop") }

function Get-VdNameForId([string]$id) {
    $p = "$vdReg\Desktops\$id"
    if (-not (Test-Path $p)) { return $null }
    return (Get-ItemProperty -Path $p -Name "Name" -ErrorAction SilentlyContinue).Name
}

Add-Type -MemberDefinition @'
[DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, int dwExtraInfo);
'@ -Name VdKeys -Namespace Velocity | Out-Null

function Send-VdHotkey([int]$vk) {
    [Velocity.VdKeys]::keybd_event(17, 0, 0, 0)
    [Velocity.VdKeys]::keybd_event(91, 0, 0, 0)
    [Velocity.VdKeys]::keybd_event($vk, 0, 0, 0)
    Start-Sleep -Milliseconds 40
    [Velocity.VdKeys]::keybd_event($vk, 0, 2, 0)
    [Velocity.VdKeys]::keybd_event(91, 0, 2, 0)
    [Velocity.VdKeys]::keybd_event(17, 0, 2, 0)
}
"#;

/// Build a PowerShell script that enumerates virtual desktops.
/// Reads the shell's ordered desktop IDs, so indices line up with what Task
/// View shows and with the IDs the move/pin APIs expect.
pub fn build_enumerate_desktops_script() -> String {
    String::from(VD_PRELUDE)
        + r#"
$ids = Get-VdDesktopIds
$current = Get-VdCurrentId
$desktops = @()
$idx = 0
foreach ($id in $ids) {
    $desktops += @{
        id = $id
        name = (Get-VdNameForId $id)
        index = $idx
        is_current = ($null -ne $current -and $current -eq $id)
    }
    $idx++
}
$matched = @($desktops | Where-Object { $_.is_current })
$result = @{
    desktops = $desktops
    current_index = $(if ($matched.Count -eq 1) { $matched[0].index } else { $null })
    total_count = $desktops.Count
    current_desktop_known = ($matched.Count -eq 1)
    registry_available = (Test-Path $vdReg)
}
ConvertTo-Json $result -Compress -Depth 4
"#
}

/// Assemble a virtual-desktop script: shared prelude, then the body, with
/// `@@TOKEN@@` placeholders substituted. Placeholder substitution avoids
/// doubling every brace in a `format!` string, which is where PowerShell
/// scripts in this file used to pick up stray `{{`.
fn vd_script(body: &str, tokens: &[(&str, &str)]) -> String {
    let mut script = String::from(VD_PRELUDE);
    script.push_str(body);
    for (token, value) in tokens {
        script = script.replace(token, value);
    }
    script
}

/// `IVirtualDesktopManager` interop plus the `IsWindow` check, wrapped in C#
/// helpers that return JSON. PowerShell 5.1 cannot pass `out`/`ref` arguments
/// to interop methods, so the marshalling has to live on the .NET side.
const VD_COM_INTEROP: &str = r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

[ComImport, Guid("A5CD92FF-29BE-454C-8D04-D82879FB3A15"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IVirtualDesktopManager {
    [PreserveSig] int IsWindowOnCurrentVirtualDesktop(IntPtr topLevelWindow, out bool onCurrentDesktop);
    [PreserveSig] int GetWindowDesktopId(IntPtr topLevelWindow, out Guid desktopId);
    [PreserveSig] int MoveWindowToDesktop(IntPtr topLevelWindow, ref Guid desktopId);
}

public static class VdApi {
    const string CoClsid = "FF72BABB-21EC-411D-9249-53D1A7B4008F";

    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr hWnd);

    static string Esc(string s) {
        if (s == null) return "";
        return s.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\r", " ").Replace("\n", " ");
    }

    static IVirtualDesktopManager Manager() {
        return (IVirtualDesktopManager)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid(CoClsid)));
    }

    public static string Probe(long hwndValue) {
        IntPtr hwnd = new IntPtr(hwndValue);
        bool exists = IsWindow(hwnd);
        if (!exists) return "{\"window_exists\":false,\"error\":\"hwnd is not a window\"}";
        try {
            IVirtualDesktopManager m = Manager();
            Guid id;
            int hrId = m.GetWindowDesktopId(hwnd, out id);
            bool onCur;
            int hrCur = m.IsWindowOnCurrentVirtualDesktop(hwnd, out onCur);
            return "{\"window_exists\":true,\"hr_getid\":" + hrId
                + ",\"desktop\":\"" + id.ToString("B").ToUpperInvariant() + "\""
                + ",\"hr_iscurrent\":" + hrCur
                + ",\"on_current\":" + (hrCur == 0 ? (onCur ? "true" : "false") : "null") + "}";
        } catch (Exception e) {
            return "{\"window_exists\":true,\"error\":\"" + Esc(e.Message) + "\"}";
        }
    }

    public static string Move(long hwndValue, string desktopIdText) {
        IntPtr hwnd = new IntPtr(hwndValue);
        if (!IsWindow(hwnd)) return "{\"verified\":false,\"error\":\"hwnd is not a window\"}";
        try {
            IVirtualDesktopManager m = Manager();
            Guid target = new Guid(desktopIdText);
            int hrMove = m.MoveWindowToDesktop(hwnd, ref target);
            Guid now;
            int hrRead = m.GetWindowDesktopId(hwnd, out now);
            bool ok = hrMove == 0 && hrRead == 0 && now.ToString("B").ToUpperInvariant() == target.ToString("B").ToUpperInvariant();
            return "{\"verified\":" + (ok ? "true" : "false") + ",\"hr_move\":" + hrMove
                + ",\"hr_read\":" + hrRead
                + ",\"desktop\":\"" + now.ToString("B").ToUpperInvariant() + "\"}";
        } catch (Exception e) {
            return "{\"verified\":false,\"error\":\"" + Esc(e.Message) + "\"}";
        }
    }
}
"@
"#;

/// Build a PowerShell script to switch to a virtual desktop by index.
/// Sends the shell's own Ctrl+Win+Arrow shortcut, then waits for
/// `CurrentVirtualDesktop` to name the target rather than assuming the keys
/// landed (bug #26: the old script compared a raw GUID byte string against a
/// key name, so it always believed it was on desktop 0 and always answered
/// success).
pub fn build_switch_desktop_script(target_index: u32) -> String {
    vd_script(
        r#"
$ids = Get-VdDesktopIds
$current = Get-VdCurrentId
$total = $ids.Count
$targetIdx = @@TARGET@@
if ($total -eq 0) {
    Write-Output (ConvertTo-Json @{ success = $false; detail = "no virtual desktops are registered on this session" } -Compress)
    exit
}
if ($targetIdx -ge $total) {
    Write-Output (ConvertTo-Json @{ success = $false; detail = "desktop index $targetIdx is out of range (0..$($total - 1))" } -Compress)
    exit
}
$fromIdx = -1
for ($i = 0; $i -lt $total; $i++) { if ($ids[$i] -eq $current) { $fromIdx = $i } }
if ($fromIdx -lt 0) {
    Write-Output (ConvertTo-Json @{ success = $false; detail = "Windows did not report an active desktop, so the number of switches cannot be computed" } -Compress)
    exit
}
if ($fromIdx -eq $targetIdx) {
    Write-Output (ConvertTo-Json @{ success = $true; detail = "already on desktop $targetIdx of $total" } -Compress)
    exit
}
$direction = if ($targetIdx -gt $fromIdx) { 0x27 } else { 0x25 }
$steps = [Math]::Abs($targetIdx - $fromIdx)
for ($i = 0; $i -lt $steps; $i++) {
    Send-VdHotkey $direction
    Start-Sleep -Milliseconds 250
}
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$now = Get-VdCurrentId
while ($sw.ElapsedMilliseconds -lt 6000 -and $now -ne $ids[$targetIdx]) {
    Start-Sleep -Milliseconds 150
    $now = Get-VdCurrentId
}
$ok = ($now -eq $ids[$targetIdx])
$detail = if ($ok) { "switched from desktop $fromIdx to $targetIdx (verified)" } else { "sent $steps switch(es) but the active desktop is still $(if ($null -eq $now) { 'unknown' } else { $now })" }
Write-Output (ConvertTo-Json @{ success = $ok; detail = $detail; from = $fromIdx; to = $targetIdx; steps = $steps; total = $total } -Compress)
"#,
        &[("@@TARGET@@", &target_index.to_string()), ("@@COM@@", "")],
    )
}

/// Build a PowerShell script to create a new virtual desktop.
pub fn build_create_desktop_script(name: Option<&str>) -> String {
    let name_token = match name {
        // Strip the characters that would break out of the quoted literal
        // below - one pass, since chained `replace` calls allocate twice.
        Some(n) => n.chars().filter(|c| *c != '"' && *c != '\\').collect(),
        None => String::new(),
    };
    vd_script(
        r#"
$ids = Get-VdDesktopIds
$before = $ids.Count
Send-VdHotkey 0x44
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$after = (Get-VdDesktopIds).Count
while ($sw.ElapsedMilliseconds -lt 5000 -and $after -le $before) {
    Start-Sleep -Milliseconds 150
    $after = (Get-VdDesktopIds).Count
}
$ok = ($after -gt $before)
$requested = '@@NAME@@'
$detail = "created desktop (count $before -> $after)"
if (-not $ok) { $detail = "Ctrl+Win+D did not add a desktop (count still $before)" }
if ($requested -ne '') {
    $detail = $detail + "; the requested name '$requested' could not be applied: this build exposes no desktop-name API"
}
Write-Output (ConvertTo-Json @{ success = $ok; detail = $detail; name_applied = $false; count_before = $before; count_after = $after } -Compress)
"#,
        &[("@@NAME@@", &name_token)],
    )
}

/// Build a PowerShell script to remove a virtual desktop by index.
/// Ctrl+Win+F4 closes the *current* desktop, so the target has to be switched
/// to first - the old script skipped that step and reported whichever index it
/// was handed.
pub fn build_remove_desktop_script(target_index: u32) -> String {
    vd_script(
        r#"
$ids = Get-VdDesktopIds
$total = $ids.Count
$targetIdx = @@TARGET@@
if ($total -le 1) {
    Write-Output (ConvertTo-Json @{ success = $false; detail = "refusing to remove the only desktop ($total present)" } -Compress)
    exit
}
if ($targetIdx -ge $total) {
    Write-Output (ConvertTo-Json @{ success = $false; detail = "desktop index $targetIdx is out of range (0..$($total - 1))" } -Compress)
    exit
}
$current = Get-VdCurrentId
$curIdx = -1
for ($i = 0; $i -lt $total; $i++) { if ($ids[$i] -eq $current) { $curIdx = $i } }
if ($curIdx -ne $targetIdx) {
    $direction = if ($targetIdx -gt $curIdx) { 0x27 } else { 0x25 }
    $steps = [Math]::Abs($targetIdx - $curIdx)
    for ($i = 0; $i -lt $steps; $i++) { Send-VdHotkey $direction; Start-Sleep -Milliseconds 250 }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 6000 -and (Get-VdCurrentId) -ne $ids[$targetIdx]) { Start-Sleep -Milliseconds 150 }
    if ((Get-VdCurrentId) -ne $ids[$targetIdx]) {
        Write-Output (ConvertTo-Json @{ success = $false; detail = "could not switch onto desktop $targetIdx before removing it" } -Compress)
        exit
    }
}
$before = (Get-VdDesktopIds).Count
Send-VdHotkey 0x73
$sw2 = [System.Diagnostics.Stopwatch]::StartNew()
$after = (Get-VdDesktopIds).Count
while ($sw2.ElapsedMilliseconds -lt 5000 -and $after -ge $before) {
    Start-Sleep -Milliseconds 150
    $after = (Get-VdDesktopIds).Count
}
$ok = ($after -lt $before)
$detail = if ($ok) { "removed desktop $targetIdx (count $before -> $after)" } else { "Ctrl+Win+F4 did not remove desktop $targetIdx (count still $after)" }
Write-Output (ConvertTo-Json @{ success = $ok; detail = $detail; count_before = $before; count_after = $after } -Compress)
"#,
        &[("@@TARGET@@", &target_index.to_string())],
    )
}

/// Build a PowerShell script to move a window to a known desktop GUID.
/// The previous version used `New-Object -ComObject VirtualDesktopManager`, a
/// ProgID that has never been registered, so the call could not have worked
/// (bug #22 - it only looked like it did because the runner discarded the
/// script output entirely).
pub fn build_move_window_to_desktop_script(
    hwnd: u64,
    desktop_id: &str,
    desktop_index: u32,
) -> String {
    vd_script(
        r#"
@@COM@@
$raw = [VdApi]::Move(@@HWND@@, '@@DESKTOP@@')
$r = $raw | ConvertFrom-Json
$detail = if ($r.PSObject.Properties.Name -contains 'error') {
    "move unavailable: " + $r.error
} elseif ($r.verified) {
    "window is on desktop @@INDEX@@ ($raw)"
} else {
    "move reported hr=" + $r.hr_move + " but the window is on " + $r.desktop
}
Write-Output (ConvertTo-Json @{ success = [bool]$r.verified; detail = $detail } -Compress)
"#,
        &[
            ("@@COM@@", VD_COM_INTEROP),
            ("@@HWND@@", &hwnd.to_string()),
            ("@@DESKTOP@@", desktop_id),
            ("@@INDEX@@", &desktop_index.to_string()),
        ],
    )
}

/// Build a PowerShell script that answers, for one window: does it exist, and
/// which virtual desktop is it on?
pub fn build_window_desktop_probe_script(hwnd: u64) -> String {
    vd_script(
        r#"
@@COM@@
$hwnd = [int64]@@HWND@@
$raw = [VdApi]::Probe($hwnd) | ConvertFrom-Json
$ids = Get-VdDesktopIds
$current = Get-VdCurrentId
$index = $null
$desktopId = $null
$reason = $null
if ($raw.PSObject.Properties.Name -contains 'error') { $reason = $raw.error }
if ($raw.PSObject.Properties.Name -contains 'desktop') {
    $desktopId = $raw.desktop
    for ($i = 0; $i -lt $ids.Count; $i++) { if ($ids[$i] -eq $desktopId) { $index = $i } }
    if ($null -eq $index -and $null -eq $reason) {
        $reason = "the window belongs to a desktop that is not in the registered list"
    }
}
$onCurrent = $null
if ($raw.PSObject.Properties.Name -contains 'on_current') { $onCurrent = $raw.on_current }
elseif ($null -ne $desktopId -and $null -ne $current) { $onCurrent = ($desktopId -eq $current) }
Write-Output (ConvertTo-Json @{
    window_exists = [bool]$raw.window_exists
    desktop_index = $index
    desktop_id = $desktopId
    on_current_desktop = $onCurrent
    reason = $reason
} -Compress)
"#,
        &[("@@COM@@", VD_COM_INTEROP), ("@@HWND@@", &hwnd.to_string())],
    )
}

// ─── Runtime Helpers ─────────────────────────────────────────────────────────

fn run_ps_script(script: &str) -> Result<String, String> {
    crate::wa::ps::run_ps_script(script)
}

/// Interpret the `{success, detail}` verdict a virtual-desktop script printed.
/// A clean exit is not evidence that the desktop actually changed, so each
/// script measures the before/after state and reports it (bug #29: `apply`
/// hard-coded `success: true` whenever PowerShell ran without error).
fn describe_op_json(json: &str) -> (bool, String) {
    #[derive(serde::Deserialize)]
    struct PsOpResult {
        success: Option<bool>,
        detail: Option<String>,
    }
    match serde_json::from_str::<PsOpResult>(json) {
        Ok(r) => (
            r.success.unwrap_or(false),
            r.detail
                .unwrap_or_else(|| "the script reported no detail".to_string()),
        ),
        Err(_) => {
            let snippet: String = json.chars().take(200).collect();
            (false, format!("unexpected script output: {snippet:?}"))
        }
    }
}

/// Parse the per-window desktop probe into measured fields, keeping "Windows
/// did not answer" distinct from "the window is not on that desktop".
fn parse_window_desktop_probe(hwnd: u64, json: &str) -> WindowDesktopProbe {
    #[derive(serde::Deserialize)]
    struct PsProbe {
        #[serde(default)]
        window_exists: bool,
        desktop_index: Option<u32>,
        desktop_id: Option<String>,
        on_current_desktop: Option<bool>,
        reason: Option<String>,
    }
    match serde_json::from_str::<PsProbe>(json) {
        Ok(p) => WindowDesktopProbe {
            hwnd,
            window_exists: p.window_exists,
            desktop_index: p.desktop_index,
            desktop_id: p.desktop_id,
            on_current_desktop: p.on_current_desktop,
            reason: p.reason,
        },
        Err(_) => WindowDesktopProbe {
            hwnd,
            window_exists: false,
            desktop_index: None,
            desktop_id: None,
            on_current_desktop: None,
            reason: Some(format!(
                "unexpected probe output: {:?}",
                json.chars().take(200).collect::<String>()
            )),
        },
    }
}

fn parse_enumerate_result(json: &str) -> Option<VirtualDesktopState> {
    #[derive(serde::Deserialize)]
    struct PsDesktop {
        id: Option<String>,
        name: Option<String>,
        index: Option<u32>,
        is_current: Option<bool>,
    }
    #[derive(serde::Deserialize)]
    struct PsResult {
        desktops: Option<Vec<PsDesktop>>,
        current_index: Option<u32>,
        total_count: Option<u32>,
        current_desktop_known: Option<bool>,
    }
    let r: PsResult = serde_json::from_str(json).ok()?;
    let listed = r.desktops?;
    // An empty list means nothing was measured (the registry key was missing or
    // held no IDs), so let the caller fall back instead of reporting a state
    // with zero desktops.
    if listed.is_empty() {
        return None;
    }
    let desktops: Vec<VirtualDesktop> = listed
        .into_iter()
        .map(|d| VirtualDesktop {
            id: d.id.unwrap_or_default(),
            name: d.name,
            index: d.index.unwrap_or(0),
            is_current: d.is_current.unwrap_or(false),
            window_count: None,
        })
        .collect();
    let total = r.total_count.unwrap_or(desktops.len() as u32);
    // A missing current desktop must stay missing: defaulting it to index 0
    // reported a measurement that was never taken (bug #21).
    let current_known = r.current_desktop_known.unwrap_or(false);
    let current_index = r.current_index.unwrap_or(0);
    Some(VirtualDesktopState {
        desktops,
        current_index,
        total_count: total,
        current_desktop_known: current_known,
        supports_named_desktops: true,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Bug #40: the session-change gate ────────────────────────────────────
    //
    // These assert the refusal *value* rather than calling `apply`, because
    // calling `apply` with the gate broken would really send the Ctrl+Win+Arrow
    // hotkey and move the person reading this output to another desktop.

    /// Every operation has to explain itself concretely; a vague refusal is
    /// barely a refusal. Exhaustive list so a new variant shows up here.
    #[test]
    fn every_operation_names_its_effect_on_the_session() {
        let cases = [
            (VDesktopOperation::SwitchTo(1), "desktop 1"),
            (
                VDesktopOperation::SwitchToNamed("Work".into()),
                "named 'Work'",
            ),
            (
                VDesktopOperation::Create { name: None },
                "adds a new desktop",
            ),
            (
                VDesktopOperation::Create {
                    name: Some("Scratch".into()),
                },
                "named 'Scratch'",
            ),
            (VDesktopOperation::Remove(2), "closes desktop 2"),
            (
                VDesktopOperation::MoveWindow {
                    hwnd: 0x1234,
                    desktop_index: 1,
                },
                "0x1234",
            ),
            (VDesktopOperation::PinWindow(1), "which desktops the window"),
        ];
        for (op, expected) in cases {
            let effect = session_change_effect(&op);
            assert!(
                effect.contains(expected),
                "{op:?} explains itself as {effect:?}, which does not mention {expected:?}"
            );
            assert!(effect.starts_with("it "), "{op:?} -> {effect:?}");
        }
    }

    #[test]
    fn a_refusal_keeps_the_last_known_state_and_claims_nothing() {
        let known = VirtualDesktopState {
            desktops: vec![VirtualDesktop {
                id: "first".into(),
                name: Some("Desktop 1".into()),
                index: 0,
                is_current: true,
                window_count: None,
            }],
            current_index: 0,
            total_count: 1,
            supports_named_desktops: true,
            current_desktop_known: true,
        };
        let refused = session_refusal(&VDesktopOperation::Create { name: None }, &Some(known));
        assert!(!refused.success);
        assert_eq!(refused.operation, "Create { name: None }");
        assert!(
            refused
                .detail
                .contains(super::super::session_guard::ALLOW_ENV),
            "{}",
            refused.detail
        );
        for lie in ["verified", "succeeded", "switched"] {
            assert!(!refused.detail.contains(lie), "{}", refused.detail);
        }
        let echoed = refused
            .new_state
            .expect("the refusal reports the state it still has");
        assert_eq!(echoed.total_count, 1);
        assert_eq!(echoed.current_index, 0);
    }

    /// The guarantee that `wa_virtual_desktop_list` cannot move your session is
    /// structural rather than a code-review promise. The shared prelude defines
    /// the hotkey helper for every script in this module, so the assertion is on
    /// the enumeration *body*: it must never call the injection at all.
    #[test]
    fn the_enumeration_body_never_calls_the_shell_hotkey() {
        let script = build_enumerate_desktops_script();
        assert!(
            script.starts_with(VD_PRELUDE),
            "enumeration no longer starts from the shared prelude, so this test is checking the wrong span"
        );
        let body = &script[VD_PRELUDE.len()..];
        for injection in ["keybd_event", "Send-VdHotkey", "mouse_event"] {
            assert!(
                !body.contains(injection),
                "enumeration must not synthesise input, but its body uses {injection}: {body}"
            );
        }
    }

    /// Enumeration must return a coherent state. The desktop count is a property
    /// of the machine, not of this code, so it is bounded rather than pinned:
    /// bug #19 used to hide the real script output behind the single-desktop
    /// fallback, and this test only ever passed against that fallback.
    #[test]
    fn enumerate_returns_coherent_state() {
        let mut mgr = VirtualDesktopManager::new();
        let state = mgr.enumerate().clone();
        assert!(state.total_count >= 1, "got: {state:?}");
        assert_eq!(
            state.total_count as usize,
            state.desktops.len(),
            "got: {state:?}"
        );
        assert!(
            (state.current_index as usize) < state.desktops.len(),
            "got: {state:?}"
        );
        if state.current_desktop_known {
            assert!(state.current().is_some(), "got: {state:?}");
        }
        assert!(mgr.state().is_some(), "enumerate() must populate the cache");
    }

    /// Bug #21: the current desktop is only optional where the registry does not
    /// publish it, and the parser must not invent index 0 in that case.
    #[test]
    fn parse_keeps_unknown_current_desktop_flagged() {
        let json = r#"{"desktops":[{"id":"{A}","index":0,"is_current":false},{"id":"{B}","index":1,"is_current":false}],"total_count":2,"current_desktop_known":false}"#;
        let state = parse_enumerate_result(json).expect("parses");
        assert!(!state.current_desktop_known);
        assert!(state.current().is_none());
        assert_eq!(state.total_count, 2);

        let json = r#"{"desktops":[{"id":"{A}","index":0,"is_current":true}],"current_index":0,"total_count":1,"current_desktop_known":true}"#;
        let state = parse_enumerate_result(json).expect("parses");
        assert!(state.current_desktop_known);
        assert_eq!(state.current().unwrap().index, 0);
    }

    #[test]
    fn state_lookup_by_name() {
        let state = VirtualDesktopState {
            desktops: vec![
                VirtualDesktop {
                    id: "aaa".to_string(),
                    name: Some("Work".to_string()),
                    index: 0,
                    is_current: true,
                    window_count: Some(5),
                },
                VirtualDesktop {
                    id: "bbb".to_string(),
                    name: Some("Personal".to_string()),
                    index: 1,
                    is_current: false,
                    window_count: Some(3),
                },
            ],
            current_index: 0,
            total_count: 2,
            supports_named_desktops: true,
            current_desktop_known: true,
        };
        assert_eq!(state.by_name("personal").unwrap().index, 1);
        assert_eq!(state.by_index(0).unwrap().name.as_deref(), Some("Work"));
    }

    #[test]
    fn enumerate_script_reads_registry() {
        let script = build_enumerate_desktops_script();
        assert!(script.contains("VirtualDesktops"));
        assert!(script.contains("CurrentVirtualDesktop"));
    }

    #[test]
    fn switch_script_uses_keyboard() {
        let script = build_switch_desktop_script(2);
        assert!(script.contains("keybd_event"));
        assert!(script.contains("targetIdx = 2"));
    }

    /// Bug #25: the ordered list comes from `VirtualDesktopIDs`, not from the
    /// `Desktops\{guid}` subkeys, which are keyed by unrelated GUIDs.
    #[test]
    fn enumerate_script_reads_the_ordered_id_list() {
        let script = build_enumerate_desktops_script();
        assert!(script.contains("VirtualDesktopIDs"));
        assert!(!script.contains("Get-ChildItem -Path $desktopsPath"));
    }

    /// Shape captured from a real Windows 11 25H2 session: two desktops, the
    /// second one active.
    #[test]
    fn parse_reads_ordered_ids_and_the_active_index() {
        let json = r#"{"desktops":[{"id":"{B529C4F1-6660-4E0F-A53D-AF655088D42E}","name":null,"index":0,"is_current":false},{"id":"{3E1E9F43-2B70-4E24-A5BF-2223D08242BB}","name":null,"index":1,"is_current":true}],"current_index":1,"total_count":2,"current_desktop_known":true,"registry_available":true}"#;
        let state = parse_enumerate_result(json).expect("parses");
        assert_eq!(state.total_count, 2);
        assert!(state.current_desktop_known);
        assert_eq!(state.current_index, 1);
        assert_eq!(
            state.current().unwrap().id,
            "{3E1E9F43-2B70-4E24-A5BF-2223D08242BB}"
        );
    }

    /// An empty enumeration is "nothing measured", which the caller must be
    /// able to tell apart from a machine that genuinely has zero desktops.
    #[test]
    fn parse_rejects_empty_desktop_list() {
        let json = r#"{"desktops":[],"current_index":null,"total_count":0,"current_desktop_known":false,"registry_available":false}"#;
        assert!(parse_enumerate_result(json).is_none());
    }

    /// Bug #22: "Windows would not say" must stay null rather than becoming a
    /// confident desktop-0 answer.
    #[test]
    fn probe_keeps_unanswered_lookup_null() {
        let json = r#"{"window_exists":true,"desktop_index":null,"desktop_id":null,"on_current_desktop":null,"reason":"Retrieving the COM class factory failed: 80040154"}"#;
        let probe = parse_window_desktop_probe(4242, json);
        assert!(probe.window_exists);
        assert_eq!(probe.hwnd, 4242);
        assert!(probe.desktop_index.is_none());
        assert!(probe.on_current_desktop.is_none());
        assert!(probe.reason.unwrap().contains("80040154"));
    }

    #[test]
    fn probe_reports_a_window_that_does_not_exist() {
        let json = r#"{"window_exists":false,"desktop_index":null,"desktop_id":null,"on_current_desktop":null,"reason":"hwnd is not a window"}"#;
        let probe = parse_window_desktop_probe(0, json);
        assert!(!probe.window_exists);
        assert!(probe.reason.unwrap().contains("not a window"));
    }

    /// Bug #29: a clean PowerShell exit is not a success verdict.
    #[test]
    fn describe_op_json_follows_the_scripts_verdict() {
        let (ok, detail) = describe_op_json(
            r#"{"success":false,"detail":"Ctrl+Win+F4 did not remove desktop 1"}"#,
        );
        assert!(!ok);
        assert!(detail.contains("did not remove"));

        let (ok, _) = describe_op_json(
            r#"{"success":true,"detail":"switched from desktop 0 to 1 (verified)"}"#,
        );
        assert!(ok);

        let (ok, detail) = describe_op_json("not json at all");
        assert!(!ok);
        assert!(detail.contains("unexpected script output"));
    }
}
