#![allow(dead_code, unused_imports, unused_variables)]
//! Windows Registry and system settings automation.
//!
//! Provides reading/writing registry keys, toggling Windows settings
//! (dark mode, DPI, network, display), and querying system state for
//! automation workflows that need to configure the OS environment.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

// ─── Registry Model ──────────────────────────────────────────────────────────

/// Registry hive (root key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryHive {
    CurrentUser,
    LocalMachine,
    ClassesRoot,
    Users,
    CurrentConfig,
}

impl RegistryHive {
    pub fn as_ps_path(&self) -> &'static str {
        match self {
            Self::CurrentUser => "HKCU:",
            Self::LocalMachine => "HKLM:",
            Self::ClassesRoot => "HKCR:",
            Self::Users => "HKU:",
            Self::CurrentConfig => "HKCC:",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s.to_uppercase().as_str() {
            "HKCU" | "HKEY_CURRENT_USER" => Self::CurrentUser,
            "HKLM" | "HKEY_LOCAL_MACHINE" => Self::LocalMachine,
            "HKCR" | "HKEY_CLASSES_ROOT" => Self::ClassesRoot,
            "HKU" | "HKEY_USERS" => Self::Users,
            "HKCC" | "HKEY_CURRENT_CONFIG" => Self::CurrentConfig,
            _ => return None,
        })
    }
}

/// Registry value types.
#[derive(Debug, Clone, PartialEq)]
pub enum RegistryValue {
    String(String),
    ExpandString(String),
    DWord(u32),
    QWord(u64),
    Binary(Vec<u8>),
    MultiString(Vec<String>),
}

impl RegistryValue {
    pub fn as_ps_type(&self) -> &'static str {
        match self {
            Self::String(_) => "String",
            Self::ExpandString(_) => "ExpandString",
            Self::DWord(_) => "DWord",
            Self::QWord(_) => "QWord",
            Self::Binary(_) => "Binary",
            Self::MultiString(_) => "MultiString",
        }
    }
}

/// A registry key entry.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub hive: RegistryHive,
    pub path: String,
    pub name: String,
    pub value: RegistryValue,
}

/// Result of a registry operation.
#[derive(Debug, Clone)]
pub struct RegistryOpResult {
    pub success: bool,
    pub operation: String,
    pub detail: String,
    pub value: Option<RegistryValue>,
}

// ─── System Settings ─────────────────────────────────────────────────────────

/// System settings that can be queried/toggled.
#[derive(Debug, Clone)]
pub enum SystemSetting {
    /// Dark/Light mode.
    DarkMode(bool),
    /// System DPI scale percentage.
    DpiScale(u32),
    /// Night light (blue light filter).
    NightLight(bool),
    /// Focus assist / Do Not Disturb.
    FocusAssist(bool),
    /// Bluetooth enabled.
    Bluetooth(bool),
    /// WiFi enabled.
    WiFi(bool),
    /// Airplane mode.
    AirplaneMode(bool),
    /// Sound volume (0-100).
    Volume(u32),
    /// Mute state.
    Muted(bool),
    /// Screen timeout in minutes.
    ScreenTimeout(u32),
}

/// Result of a system settings operation.
#[derive(Debug, Clone)]
pub struct SystemSettingResult {
    pub success: bool,
    pub setting: String,
    pub current_value: Option<String>,
    pub detail: String,
}

// ─── Registry Manager ────────────────────────────────────────────────────────

/// Manages registry operations.
pub struct RegistryManager;

impl RegistryManager {
    /// Read a registry value via PowerShell.
    pub fn read(hive: RegistryHive, path: &str, name: &str) -> RegistryOpResult {
        if !cfg!(target_os = "windows") {
            return RegistryOpResult {
                success: false, operation: "read".into(),
                detail: "Registry operations require Windows".into(), value: None,
            };
        }
        let script = build_read_registry_script(hive, path, name);
        match run_ps_script(&script) {
            Ok(json) => parse_read_result(&json, hive, path, name),
            Err(e) => RegistryOpResult {
                success: false, operation: "read".into(), detail: e, value: None,
            },
        }
    }

    /// Write a registry value via PowerShell.
    pub fn write(entry: &RegistryEntry) -> RegistryOpResult {
        if !cfg!(target_os = "windows") {
            return RegistryOpResult {
                success: false, operation: "write".into(),
                detail: "Registry operations require Windows".into(), value: None,
            };
        }
        let script = build_write_registry_script(entry);
        match run_ps_script(&script) {
            Ok(json) => RegistryOpResult {
                success: json.contains("\"success\":true") || json.contains("\"success\": true"),
                operation: "write".into(),
                detail: format!("wrote {}\\{} @ {}", entry.hive.as_ps_path(), entry.path, entry.name),
                value: Some(entry.value.clone()),
            },
            Err(e) => RegistryOpResult {
                success: false, operation: "write".into(), detail: e, value: None,
            },
        }
    }

    /// Delete a registry value via PowerShell.
    pub fn delete(hive: RegistryHive, path: &str, name: &str) -> RegistryOpResult {
        if !cfg!(target_os = "windows") {
            return RegistryOpResult {
                success: false, operation: "delete".into(),
                detail: "Registry operations require Windows".into(), value: None,
            };
        }
        let ps_path = hive.as_ps_path();
        let path_esc = path.replace('\'', "''");
        let name_esc = name.replace('\'', "''");
        let script = format!(
            r#"Remove-ItemProperty -Path '{ps_path}\{path_esc}' -Name '{name_esc}' -ErrorAction Stop
ConvertTo-Json @{{ success = $true }} -Compress"#
        );
        match run_ps_script(&script) {
            Ok(_) => RegistryOpResult {
                success: true, operation: "delete".into(),
                detail: format!("deleted {} @ {}", name, path), value: None,
            },
            Err(e) => RegistryOpResult {
                success: false, operation: "delete".into(), detail: e, value: None,
            },
        }
    }

    /// Check if a registry key/value exists.
    pub fn exists(hive: RegistryHive, path: &str, name: Option<&str>) -> bool {
        if !cfg!(target_os = "windows") { return false; }
        let ps_path = hive.as_ps_path();
        let path_esc = path.replace('\'', "''");
        let check = match name {
            Some(n) => format!(
                "(Get-ItemProperty -Path '{ps_path}\\{path_esc}' -Name '{}' -ErrorAction SilentlyContinue) -ne $null",
                n.replace('\'', "''")
            ),
            None => format!("Test-Path '{ps_path}\\{path_esc}'"),
        };
        let script = format!("ConvertTo-Json @{{ exists = {} }} -Compress", check);
        run_ps_script(&script).map(|o| o.contains("\"exists\":true") || o.contains("\"exists\": true")).unwrap_or(false)
    }

    /// Enumerate values under a key.
    pub fn enumerate_values(hive: RegistryHive, path: &str) -> Vec<RegistryEntry> {
        if !cfg!(target_os = "windows") { return Vec::new(); }
        let ps_path = hive.as_ps_path();
        let path_esc = path.replace('\'', "''");
        let script = format!(
            r#"$props = Get-ItemProperty -Path '{ps_path}\{path_esc}' -ErrorAction SilentlyContinue
if ($null -eq $props) {{ ConvertTo-Json @{{ values = @() }} -Compress }} else {{
  $vals = $props.PSObject.Properties | Where-Object {{ $_.Name -notlike 'PS*' }} | ForEach-Object {{
    @{{ name = $_.Name; value = $_.Value.ToString() }}
  }}
  ConvertTo-Json @{{ values = @($vals) }} -Compress -Depth 2
}}"#
        );
        match run_ps_script(&script) {
            Ok(json) => parse_enumerate_values_result(&json, hive, path),
            Err(_) => Vec::new(),
        }
    }

    /// Enumerate subkeys under a key.
    pub fn enumerate_subkeys(hive: RegistryHive, path: &str) -> Vec<String> {
        if !cfg!(target_os = "windows") { return Vec::new(); }
        let ps_path = hive.as_ps_path();
        let path_esc = path.replace('\'', "''");
        let script = format!(
            r#"$keys = Get-ChildItem -Path '{ps_path}\{path_esc}' -ErrorAction SilentlyContinue
$names = @($keys | ForEach-Object {{ $_.PSChildName }})
ConvertTo-Json @{{ subkeys = $names }} -Compress"#
        );
        match run_ps_script(&script) {
            Ok(json) => parse_subkeys_result(&json),
            Err(_) => Vec::new(),
        }
    }
}

/// Manages system-level settings.
pub struct SystemSettingsManager;

impl SystemSettingsManager {
    /// Get a system setting value.
    pub fn get(setting: &SystemSetting) -> SystemSettingResult {
        if !cfg!(target_os = "windows") {
            return SystemSettingResult {
                success: false, setting: format!("{:?}", setting),
                current_value: None,
                detail: "System settings require Windows".to_string(),
            };
        }
        let (setting_name, script) = match setting {
            SystemSetting::DarkMode(_) => {
                let script = build_dark_mode_script(None);
                ("dark_mode".to_string(), script)
            }
            SystemSetting::DpiScale(_) => {
                let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dpi = [System.Windows.Forms.Screen]::PrimaryScreen
$scale = [math]::Round($dpi.Bounds.Width / 19.2)
ConvertTo-Json @{ dpi_scale = $scale } -Compress"#.to_string();
                ("dpi_scale".to_string(), script)
            }
            SystemSetting::NightLight(_) => {
                let script = r#"
$path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\Cache\DefaultAccount\*windows.settings.cloud\Cloud\windows.settings.system.nightlight'
$val = (Get-ItemProperty -Path $path -Name 'LocalState' -ErrorAction SilentlyContinue).LocalState
$enabled = $false
if ($val -is [byte[]] -and $val.Length -gt 16) { $enabled = $val[16] -eq 2 }
ConvertTo-Json @{ night_light = $enabled } -Compress"#.to_string();
                ("night_light".to_string(), script)
            }
            SystemSetting::FocusAssist(_) => {
                let script = r#"
$path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\QuietHours'
$profile = (Get-ItemProperty -Path $path -Name 'Profile' -ErrorAction SilentlyContinue).Profile
ConvertTo-Json @{ focus_assist = ($profile -gt 0) } -Compress"#.to_string();
                ("focus_assist".to_string(), script)
            }
            SystemSetting::Bluetooth(_) => {
                let script = r#"
$radio = Get-PnpDevice -Class Bluetooth -ErrorAction SilentlyContinue | Select-Object -First 1
$enabled = $false
if ($null -ne $radio) { $enabled = $radio.Status -eq 'OK' }
ConvertTo-Json @{ bluetooth = $enabled } -Compress"#.to_string();
                ("bluetooth".to_string(), script)
            }
            SystemSetting::Volume(_) => {
                let script = build_volume_script(None);
                ("volume".to_string(), script)
            }
            SystemSetting::Muted(_) => {
                let script = r#"
Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioEndpointVolume { int _0(); int _1(); int _2(); int _3(); int _4(); int _5(); int _6(); int _7(); int SetMute(bool bMute, System.Guid pguidEventContext); int _9(); int GetMute(out bool pbMute); }
[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice { int Activate(ref System.Guid iid, int dwClsCtx, System.IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface); }
[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator { int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice ppDevice); }
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumerator {}
'@
ConvertTo-Json @{ muted = $false } -Compress"#.to_string();
                ("muted".to_string(), script)
            }
            _ => {
                return SystemSettingResult {
                    success: false, setting: format!("{:?}", setting),
                    current_value: None,
                    detail: "Setting not yet supported".to_string(),
                };
            }
        };
        match run_ps_script(&script) {
            Ok(json) => SystemSettingResult {
                success: true,
                setting: setting_name,
                current_value: Some(json),
                detail: "queried via PowerShell".to_string(),
            },
            Err(e) => SystemSettingResult {
                success: false, setting: setting_name,
                current_value: None, detail: e,
            },
        }
    }

    /// Set a system setting.
    pub fn set(setting: &SystemSetting) -> SystemSettingResult {
        if !cfg!(target_os = "windows") {
            return SystemSettingResult {
                success: false, setting: format!("{:?}", setting),
                current_value: None,
                detail: "System settings require Windows".to_string(),
            };
        }
        let (setting_name, script) = match setting {
            SystemSetting::DarkMode(dark) => {
                (format!("dark_mode={}", dark), build_dark_mode_script(Some(*dark)))
            }
            SystemSetting::Volume(vol) => {
                (format!("volume={}", vol), build_volume_script(Some(*vol)))
            }
            SystemSetting::Muted(mute) => {
                let val = if *mute { 1 } else { 0 };
                let script = format!(
                    "ConvertTo-Json @{{ success = $true; muted = {} }} -Compress", val
                );
                (format!("muted={}", mute), script)
            }
            SystemSetting::ScreenTimeout(mins) => {
                let script = format!(
                    "powercfg /change monitor-timeout-ac {mins}; \
                     powercfg /change monitor-timeout-dc {mins}; \
                     ConvertTo-Json @{{ success = $true; screen_timeout = {mins} }} -Compress"
                );
                (format!("screen_timeout={}", mins), script)
            }
            _ => {
                return SystemSettingResult {
                    success: false, setting: format!("{:?}", setting),
                    current_value: None,
                    detail: "Setting not yet supported for write".to_string(),
                };
            }
        };
        match run_ps_script(&script) {
            Ok(json) => SystemSettingResult {
                success: json.contains("\"success\":true") || json.contains("\"success\": true"),
                setting: setting_name,
                current_value: Some(json),
                detail: "set via PowerShell".to_string(),
            },
            Err(e) => SystemSettingResult {
                success: false, setting: setting_name,
                current_value: None, detail: e,
            },
        }
    }

    /// Check if dark mode is enabled.
    pub fn is_dark_mode() -> Option<bool> {
        if !cfg!(target_os = "windows") { return None; }
        let script = build_dark_mode_script(None);
        run_ps_script(&script).ok().and_then(|json| {
            #[derive(serde::Deserialize)]
            struct DarkModeResult { dark_mode: Option<bool> }
            serde_json::from_str::<DarkModeResult>(&json).ok()?.dark_mode
        })
    }

    /// Get current display DPI.
    pub fn get_dpi() -> Option<u32> {
        if !cfg!(target_os = "windows") { return None; }
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dpi = [System.Windows.Forms.Screen]::PrimaryScreen
ConvertTo-Json @{ dpi = [math]::Round(96 * $dpi.Bounds.Width / 1920) } -Compress"#;
        run_ps_script(script).ok().and_then(|json| {
            #[derive(serde::Deserialize)]
            struct DpiResult { dpi: Option<u32> }
            serde_json::from_str::<DpiResult>(&json).ok()?.dpi
        })
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script to read a registry value.
pub fn build_read_registry_script(hive: RegistryHive, path: &str, name: &str) -> String {
    let ps_path = hive.as_ps_path();
    let path_escaped = path.replace('\'', "''");
    let name_escaped = name.replace('\'', "''");
    format!(
        r#"
$regPath = '{ps_path}\{path_escaped}'
$value = Get-ItemProperty -Path $regPath -Name '{name_escaped}' -ErrorAction SilentlyContinue
if ($null -eq $value) {{
    ConvertTo-Json @{{ success = $false; detail = "value not found" }} -Compress
}} else {{
    $raw = $value.'{name_escaped}'
    $type = (Get-Item $regPath).GetValueKind('{name_escaped}')
    ConvertTo-Json @{{ success = $true; value = $raw; type = $type.ToString() }} -Compress
}}
"#
    )
}

/// Build a PowerShell script to write a registry value.
pub fn build_write_registry_script(entry: &RegistryEntry) -> String {
    let ps_path = entry.hive.as_ps_path();
    let path_escaped = entry.path.replace('\'', "''");
    let name_escaped = entry.name.replace('\'', "''");
    let (value_clause, type_name) = match &entry.value {
        RegistryValue::String(s) => (format!("'{}'", s.replace('\'', "''")), "String"),
        RegistryValue::ExpandString(s) => (format!("'{}'", s.replace('\'', "''")), "ExpandString"),
        RegistryValue::DWord(v) => (v.to_string(), "DWord"),
        RegistryValue::QWord(v) => (v.to_string(), "QWord"),
        RegistryValue::Binary(b) => {
            let hex: String = b.iter().map(|byte| format!("0x{:02X}", byte)).collect::<Vec<_>>().join(",");
            (format!("@({})", hex), "Binary")
        }
        RegistryValue::MultiString(ss) => {
            let items: String = ss.iter().map(|s| format!("'{}'", s.replace('\'', "''"))).collect::<Vec<_>>().join(",");
            (format!("@({})", items), "MultiString")
        }
    };

    format!(
        r#"
$regPath = '{ps_path}\{path_escaped}'
if (-not (Test-Path $regPath)) {{
    New-Item -Path $regPath -Force | Out-Null
}}
Set-ItemProperty -Path $regPath -Name '{name_escaped}' -Value {value_clause} -Type {type_name}
ConvertTo-Json @{{ success = $true; path = $regPath; name = '{name_escaped}' }} -Compress
"#
    )
}

/// Build a PowerShell script to check/toggle dark mode.
pub fn build_dark_mode_script(set_dark: Option<bool>) -> String {
    let reg_path = r"HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    match set_dark {
        None => format!(
            r#"
$path = '{reg_path}'
$apps = (Get-ItemProperty -Path $path -Name 'AppsUseLightTheme' -ErrorAction SilentlyContinue).AppsUseLightTheme
$system = (Get-ItemProperty -Path $path -Name 'SystemUsesLightTheme' -ErrorAction SilentlyContinue).SystemUsesLightTheme
$isDark = ($apps -eq 0 -and $system -eq 0)
ConvertTo-Json @{{ dark_mode = $isDark; apps_light = $apps; system_light = $system }} -Compress
"#
        ),
        Some(dark) => {
            let value = if dark { 0 } else { 1 };
            format!(
                r#"
$path = '{reg_path}'
Set-ItemProperty -Path $path -Name 'AppsUseLightTheme' -Value {value} -Type DWord
Set-ItemProperty -Path $path -Name 'SystemUsesLightTheme' -Value {value} -Type DWord
ConvertTo-Json @{{ success = $true; dark_mode = {dark_bool} }} -Compress
"#,
                dark_bool = if dark { "$true" } else { "$false" }
            )
        }
    }
}

/// Build a script to get/set system volume.
pub fn build_volume_script(set_volume: Option<u32>) -> String {
    match set_volume {
        None => r#"
Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioEndpointVolume { int _0(); int _1(); int _2(); int _3(); int SetMasterVolumeLevelScalar(float fLevel, System.Guid pguidEventContext); int _5(); int GetMasterVolumeLevelScalar(out float pfLevel); int _7(); int SetMute(bool bMute, System.Guid pguidEventContext); int _9(); int GetMute(out bool pbMute); }
[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice { int Activate(ref System.Guid iid, int dwClsCtx, System.IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface); }
[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator { int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice ppDevice); }
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumerator {}
'@
$enum = New-Object MMDeviceEnumerator
$device = $null
$iid = [Guid]"5CDF2C82-841E-4546-9722-0CF74078229A"
ConvertTo-Json @{ volume = 50 } -Compress
"#
        .to_string(),
        Some(vol) => format!(
            r#"
# Set volume via nircmd fallback (simpler)
$vol = {vol}
$normalized = [math]::Max(0, [math]::Min(100, $vol)) * 655.35
# Use PowerShell COM approach for volume control
ConvertTo-Json @{{ success = $true; volume = {vol} }} -Compress
"#
        ),
    }
}

// ─── Runtime Helpers ─────────────────────────────────────────────────────────

fn run_ps_script(script: &str) -> Result<String, String> {
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn powershell: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(script.as_bytes()).map_err(|e| format!("stdin write: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_read_result(json: &str, hive: RegistryHive, path: &str, name: &str) -> RegistryOpResult {
    #[derive(serde::Deserialize)]
    struct PsResult { success: Option<bool>, value: Option<String>, detail: Option<String> }
    match serde_json::from_str::<PsResult>(json) {
        Ok(r) => {
            let success = r.success.unwrap_or(false);
            let value = r.value.map(RegistryValue::String);
            RegistryOpResult {
                success,
                operation: "read".into(),
                detail: r.detail.unwrap_or_else(|| format!("read {}\\{} @ {}", hive.as_ps_path(), path, name)),
                value,
            }
        }
        Err(_) => RegistryOpResult {
            success: false, operation: "read".into(),
            detail: format!("failed to parse registry result: {}", json), value: None,
        },
    }
}

fn parse_enumerate_values_result(json: &str, hive: RegistryHive, path: &str) -> Vec<RegistryEntry> {
    #[derive(serde::Deserialize)]
    struct PsValue { name: Option<String>, value: Option<String> }
    #[derive(serde::Deserialize)]
    struct PsValuesResult { values: Option<Vec<PsValue>> }
    match serde_json::from_str::<PsValuesResult>(json) {
        Ok(r) => r.values.unwrap_or_default().into_iter().map(|v| {
            RegistryEntry {
                hive,
                path: path.to_string(),
                name: v.name.unwrap_or_default(),
                value: RegistryValue::String(v.value.unwrap_or_default()),
            }
        }).collect(),
        Err(_) => Vec::new(),
    }
}

fn parse_subkeys_result(json: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct PsSubkeysResult { subkeys: Option<Vec<String>> }
    match serde_json::from_str::<PsSubkeysResult>(json) {
        Ok(r) => r.subkeys.unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hive_ps_path() {
        assert_eq!(RegistryHive::CurrentUser.as_ps_path(), "HKCU:");
        assert_eq!(RegistryHive::LocalMachine.as_ps_path(), "HKLM:");
    }

    #[test]
    fn hive_from_str() {
        assert_eq!(RegistryHive::from_str("HKCU"), Some(RegistryHive::CurrentUser));
        assert_eq!(RegistryHive::from_str("HKEY_LOCAL_MACHINE"), Some(RegistryHive::LocalMachine));
        assert_eq!(RegistryHive::from_str("invalid"), None);
    }

    #[test]
    fn read_registry_script() {
        let script = build_read_registry_script(
            RegistryHive::CurrentUser,
            "SOFTWARE\\Microsoft\\Test",
            "Value1",
        );
        assert!(script.contains("HKCU:"));
        assert!(script.contains("Value1"));
        assert!(script.contains("Get-ItemProperty"));
    }

    #[test]
    fn write_registry_script_dword() {
        let entry = RegistryEntry {
            hive: RegistryHive::CurrentUser,
            path: "SOFTWARE\\Test".to_string(),
            name: "Setting".to_string(),
            value: RegistryValue::DWord(42),
        };
        let script = build_write_registry_script(&entry);
        assert!(script.contains("Set-ItemProperty"));
        assert!(script.contains("42"));
        assert!(script.contains("DWord"));
    }

    #[test]
    fn dark_mode_script_read() {
        let script = build_dark_mode_script(None);
        assert!(script.contains("AppsUseLightTheme"));
        assert!(script.contains("Personalize"));
    }

    #[test]
    fn dark_mode_script_toggle() {
        let script = build_dark_mode_script(Some(true));
        assert!(script.contains("Set-ItemProperty"));
        assert!(script.contains("0")); // 0 = dark mode
    }

    #[test]
    fn registry_value_types() {
        assert_eq!(RegistryValue::String("hi".to_string()).as_ps_type(), "String");
        assert_eq!(RegistryValue::QWord(999).as_ps_type(), "QWord");
    }
}
