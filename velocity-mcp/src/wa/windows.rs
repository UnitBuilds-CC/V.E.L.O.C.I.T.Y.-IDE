use std::error::Error;
use std::io::{Error as IoError, ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::wa::{WaNode, WaWindowsActionReport, WaWindowsCaptureReport, WaWindowsWaitReport};

#[derive(Debug, Deserialize)]
struct WindowsCapturePayload {
    #[serde(default)]
    window_title: String,
    #[serde(default)]
    process_id: Option<u32>,
    #[serde(default)]
    focus_node_id: Option<String>,
    #[serde(default)]
    nodes: Vec<WaNode>,
}

fn build_capture_script() -> &'static str {
    r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class WaNative {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
"@

$processIdFilter = $env:WA_CAPTURE_PROCESS_ID
$windowNameFilter = $env:WA_CAPTURE_WINDOW_NAME_CONTAINS
$maxDepth = 3
if (-not [string]::IsNullOrWhiteSpace($env:WA_CAPTURE_MAX_DEPTH)) {
    $maxDepth = [Math]::Max(0, [int]$env:WA_CAPTURE_MAX_DEPTH)
}
$maxChildren = 64
if (-not [string]::IsNullOrWhiteSpace($env:WA_CAPTURE_MAX_CHILDREN)) {
    $maxChildren = [Math]::Max(1, [int]$env:WA_CAPTURE_MAX_CHILDREN)
}

function Get-WaRole($element) {
    try {
        $programmatic = $element.Current.ControlType.ProgrammaticName
        if ([string]::IsNullOrWhiteSpace($programmatic)) {
            return 'unknown'
        }
        $parts = $programmatic.Split('.')
        return $parts[$parts.Length - 1].ToLowerInvariant()
    } catch {
        return 'unknown'
    }
}

function Get-WaValue($element) {
    try {
        $pattern = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        if ($null -ne $pattern) {
            return $pattern.Current.Value
        }
    } catch {}
    return ''
}

function Get-WaActions($element) {
    $actions = New-Object System.Collections.Generic.List[string]
    try { $null = $element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern); $actions.Add('click') } catch {}
    try { $null = $element.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern); $actions.Add('select') } catch {}
    try { $null = $element.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern); $actions.Add('expand'); $actions.Add('collapse') } catch {}
    try { $null = $element.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern); $actions.Add('toggle') } catch {}
    try { $null = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern); $actions.Add('type') } catch {}
    try { if ($element.Current.IsKeyboardFocusable) { $actions.Add('focus') } } catch {}
    return @($actions | Select-Object -Unique)
}

$nodeList = New-Object System.Collections.Generic.List[object]
$focusNodeId = $null
$nodeCounter = 0

function Add-WaNode($element, $depth, $maxDepth, $maxChildren) {
    if ($null -eq $element) {
        return
    }
    $automationId = ''
    try { $automationId = $element.Current.AutomationId } catch {}
    $runtimeId = ''
    try { $runtimeId = (($element.GetRuntimeId() | ForEach-Object { $_.ToString() }) -join '.') } catch {}
    $handleValue = 0
    try { $handleValue = $element.Current.NativeWindowHandle } catch {}
    $nodeId = if (-not [string]::IsNullOrWhiteSpace($automationId)) {
        $automationId
    } elseif (-not [string]::IsNullOrWhiteSpace($runtimeId)) {
        "rid:$runtimeId"
    } elseif ($handleValue -ne 0) {
        "hwnd:$handleValue"
    } else {
        $script:nodeCounter += 1
        "node:$script:nodeCounter"
    }
    $name = ''
    try { $name = $element.Current.Name } catch {}
    if ([string]::IsNullOrWhiteSpace($name)) {
        $name = $automationId
    }
    if ([string]::IsNullOrWhiteSpace($name)) {
        try { $name = $element.Current.ClassName } catch {}
    }
    $enabled = $true
    try { $enabled = [bool]$element.Current.IsEnabled } catch {}
    $visible = $true
    try { $visible = -not [bool]$element.Current.IsOffscreen } catch {}
    $hasFocus = $false
    try { $hasFocus = [bool]$element.Current.HasKeyboardFocus } catch {}
    if ($hasFocus -and [string]::IsNullOrWhiteSpace($script:focusNodeId)) {
        $script:focusNodeId = $nodeId
    }
    $script:nodeList.Add([PSCustomObject]@{
        id = $nodeId
        role = Get-WaRole $element
        name = if ([string]::IsNullOrWhiteSpace($name)) { $nodeId } else { $name }
        value = Get-WaValue $element
        actions = @(Get-WaActions $element)
        visible = $visible
        enabled = $enabled
        provenance = 'native'
        confidence = 1.0
    }) | Out-Null
    if ($depth -ge $maxDepth) {
        return
    }
    $children = $null
    try {
        $children = $element.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    } catch {
        return
    }
    $count = [Math]::Min($children.Count, $maxChildren)
    for ($i = 0; $i -lt $count; $i++) {
        Add-WaNode $children.Item($i) ($depth + 1) $maxDepth $maxChildren
    }
}

$root = [System.Windows.Automation.AutomationElement]::RootElement
$topLevel = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
$target = $null
if (-not [string]::IsNullOrWhiteSpace($processIdFilter)) {
    for ($i = 0; $i -lt $topLevel.Count; $i++) {
        $candidate = $topLevel.Item($i)
        if ($candidate.Current.ProcessId -eq [int]$processIdFilter) {
            $target = $candidate
            break
        }
    }
}
if ($null -eq $target -and -not [string]::IsNullOrWhiteSpace($windowNameFilter)) {
    for ($i = 0; $i -lt $topLevel.Count; $i++) {
        $candidate = $topLevel.Item($i)
        $name = $candidate.Current.Name
        if (-not [string]::IsNullOrWhiteSpace($name) -and $name.ToLowerInvariant().Contains($windowNameFilter.ToLowerInvariant())) {
            $target = $candidate
            break
        }
    }
}
if ($null -eq $target) {
    $foreground = [WaNative]::GetForegroundWindow()
    if ($foreground -ne [IntPtr]::Zero) {
        for ($i = 0; $i -lt $topLevel.Count; $i++) {
            $candidate = $topLevel.Item($i)
            if ($candidate.Current.NativeWindowHandle -eq $foreground.ToInt32()) {
                $target = $candidate
                break
            }
        }
    }
}
if ($null -eq $target) {
    for ($i = 0; $i -lt $topLevel.Count; $i++) {
        $candidate = $topLevel.Item($i)
        $name = $candidate.Current.Name
        if (-not [string]::IsNullOrWhiteSpace($name)) {
            $target = $candidate
            break
        }
    }
}
if ($null -eq $target -and $topLevel.Count -gt 0) {
    $target = $topLevel.Item(0)
}
if ($null -eq $target) {
    throw 'no Windows UIAutomation target window found'
}

Add-WaNode $target 0 $maxDepth $maxChildren

[PSCustomObject]@{
    window_title = ($target.Current.Name)
    process_id = ($target.Current.ProcessId)
    focus_node_id = $focusNodeId
    nodes = @($nodeList)
} | ConvertTo-Json -Depth 6 -Compress
"#
}

#[derive(Debug, Deserialize)]
struct WindowsActionPayload {
    #[serde(default)]
    window_title: String,
    #[serde(default)]
    process_id: Option<u32>,
    #[serde(default)]
    executed_node_id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    detail: String,
}

#[derive(Debug, Deserialize)]
struct WindowsWaitPayload {
    #[serde(default)]
    window_title: String,
    #[serde(default)]
    process_id: Option<u32>,
    #[serde(default)]
    observed_value: Option<String>,
    #[serde(default)]
    satisfied: bool,
    #[serde(default)]
    elapsed_ms: u64,
    #[serde(default)]
    detail: String,
}

fn build_action_report_from_payload(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    action: &str,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    input_value: Option<&str>,
    payload: WindowsActionPayload,
) -> Result<WaWindowsActionReport, Box<dyn Error>> {
    let plan = crate::wa::plan_action(
        root,
        session_id,
        snapshot_name,
        action,
        node_id,
        role,
        name,
        input_value,
    )?;
    Ok(WaWindowsActionReport {
        source: "windows-uia".to_string(),
        session_id: session_id.to_string(),
        snapshot_name: plan.snapshot_name,
        action: action.to_string(),
        requested_value: input_value.map(|value| value.to_string()),
        selector: plan.selector,
        matched: plan.matched,
        preconditions: plan.preconditions,
        target_process_id: payload.process_id,
        target_window_title: payload.window_title,
        executed_node_id: payload.executed_node_id,
        execution_status: payload.status,
        execution_detail: payload.detail,
        snapshot_nda_path: plan.snapshot_nda_path,
    })
}

fn build_wait_report_from_payload(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    condition: &str,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    expected_value: Option<&str>,
    timeout_ms: u64,
    poll_interval_ms: u64,
    payload: WindowsWaitPayload,
) -> Result<WaWindowsWaitReport, Box<dyn Error>> {
    let probe_action = match condition {
        "focused" => "focus",
        "value_equals" => "type",
        _ => "inspect",
    };
    let resolve = crate::wa::resolve_selector(
        root,
        session_id,
        snapshot_name,
        node_id,
        role,
        name,
        if probe_action == "inspect" { None } else { Some(probe_action) },
    )?;
    Ok(WaWindowsWaitReport {
        source: "windows-uia".to_string(),
        session_id: session_id.to_string(),
        snapshot_name: resolve.snapshot_name,
        condition: condition.to_string(),
        expected_value: expected_value.map(|value| value.to_string()),
        selector: resolve.selector,
        matched: resolve.matched,
        target_process_id: payload.process_id,
        target_window_title: payload.window_title,
        observed_value: payload.observed_value,
        satisfied: payload.satisfied,
        elapsed_ms: payload.elapsed_ms,
        timeout_ms,
        poll_interval_ms,
        detail: payload.detail,
        snapshot_nda_path: resolve.snapshot_nda_path,
    })
}

fn build_action_script() -> &'static str {
    r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$processIdFilter = $env:WA_ACTION_PROCESS_ID
$windowNameFilter = $env:WA_ACTION_WINDOW_NAME_CONTAINS
$nodeId = $env:WA_ACTION_NODE_ID
$actionName = $env:WA_ACTION_NAME
$inputValue = $env:WA_ACTION_VALUE

function Get-WaTargetWindow() {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $topLevel = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    if (-not [string]::IsNullOrWhiteSpace($processIdFilter)) {
        for ($i = 0; $i -lt $topLevel.Count; $i++) {
            $candidate = $topLevel.Item($i)
            if ($candidate.Current.ProcessId -eq [int]$processIdFilter) {
                return $candidate
            }
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($windowNameFilter)) {
        for ($i = 0; $i -lt $topLevel.Count; $i++) {
            $candidate = $topLevel.Item($i)
            $name = $candidate.Current.Name
            if (-not [string]::IsNullOrWhiteSpace($name) -and $name.ToLowerInvariant().Contains($windowNameFilter.ToLowerInvariant())) {
                return $candidate
            }
        }
    }
    return $null
}

function Get-WaElementNodeId($element) {
    $automationId = ''
    try { $automationId = $element.Current.AutomationId } catch {}
    if (-not [string]::IsNullOrWhiteSpace($automationId)) {
        return $automationId
    }
    $runtimeId = ''
    try { $runtimeId = (($element.GetRuntimeId() | ForEach-Object { $_.ToString() }) -join '.') } catch {}
    if (-not [string]::IsNullOrWhiteSpace($runtimeId)) {
        return "rid:$runtimeId"
    }
    $handleValue = 0
    try { $handleValue = $element.Current.NativeWindowHandle } catch {}
    if ($handleValue -ne 0) {
        return "hwnd:$handleValue"
    }
    return ''
}

function Find-WaNodeRecursive($element, $expectedNodeId) {
    if ($null -eq $element) {
        return $null
    }
    if ((Get-WaElementNodeId $element) -eq $expectedNodeId) {
        return $element
    }
    $children = $null
    try {
        $children = $element.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    } catch {
        return $null
    }
    for ($i = 0; $i -lt $children.Count; $i++) {
        $matched = Find-WaNodeRecursive $children.Item($i) $expectedNodeId
        if ($null -ne $matched) {
            return $matched
        }
    }
    return $null
}

$targetWindow = Get-WaTargetWindow
if ($null -eq $targetWindow) {
    throw 'no Windows UIAutomation target window found for action execution'
}
$targetElement = Find-WaNodeRecursive $targetWindow $nodeId
if ($null -eq $targetElement) {
    throw "target node '$nodeId' was not found in the target window"
}

$status = 'executed'
$detail = ''
switch ($actionName.ToLowerInvariant()) {
    'focus' {
        $targetElement.SetFocus()
        $detail = 'focus applied'
    }
    'click' {
        try {
            $pattern = $targetElement.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
            $pattern.Invoke()
            $detail = 'invoke pattern executed'
        } catch {
            $targetElement.SetFocus()
            [System.Windows.Forms.SendKeys]::SendWait(' ')
            $detail = 'invoke unavailable; sent keyboard activation'
        }
    }
    'type' {
        if ([string]::IsNullOrEmpty($inputValue)) {
            throw 'type action requires WA_ACTION_VALUE'
        }
        try {
            $pattern = $targetElement.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
            $pattern.SetValue($inputValue)
            $detail = 'value pattern set'
        } catch {
            $targetElement.SetFocus()
            [System.Windows.Forms.SendKeys]::SendWait('^a')
            [System.Windows.Forms.SendKeys]::SendWait($inputValue)
            $detail = 'value pattern unavailable; sent keyboard input'
        }
    }
    'select' {
        $pattern = $targetElement.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
        $pattern.Select()
        $detail = 'selection item selected'
    }
    'toggle' {
        $pattern = $targetElement.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
        $pattern.Toggle()
        $detail = 'toggle pattern toggled'
    }
    'expand' {
        $pattern = $targetElement.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
        $pattern.Expand()
        $detail = 'expand pattern expanded'
    }
    'collapse' {
        $pattern = $targetElement.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
        $pattern.Collapse()
        $detail = 'expand pattern collapsed'
    }
    default {
        throw "unsupported WA Windows action '$actionName'"
    }
}

[PSCustomObject]@{
    window_title = ($targetWindow.Current.Name)
    process_id = ($targetWindow.Current.ProcessId)
    executed_node_id = $nodeId
    status = $status
    detail = $detail
} | ConvertTo-Json -Depth 4 -Compress
"#
}

fn build_wait_script() -> &'static str {
    r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$processIdFilter = $env:WA_WAIT_PROCESS_ID
$windowNameFilter = $env:WA_WAIT_WINDOW_NAME_CONTAINS
$nodeId = $env:WA_WAIT_NODE_ID
$conditionName = $env:WA_WAIT_CONDITION
$expectedValue = $env:WA_WAIT_EXPECTED_VALUE
$timeoutMs = [Math]::Max(1, [int]$env:WA_WAIT_TIMEOUT_MS)
$pollMs = [Math]::Max(1, [int]$env:WA_WAIT_POLL_MS)

function Get-WaTargetWindow() {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $topLevel = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    if (-not [string]::IsNullOrWhiteSpace($processIdFilter)) {
        for ($i = 0; $i -lt $topLevel.Count; $i++) {
            $candidate = $topLevel.Item($i)
            if ($candidate.Current.ProcessId -eq [int]$processIdFilter) {
                return $candidate
            }
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($windowNameFilter)) {
        for ($i = 0; $i -lt $topLevel.Count; $i++) {
            $candidate = $topLevel.Item($i)
            $name = $candidate.Current.Name
            if (-not [string]::IsNullOrWhiteSpace($name) -and $name.ToLowerInvariant().Contains($windowNameFilter.ToLowerInvariant())) {
                return $candidate
            }
        }
    }
    return $null
}

function Get-WaElementNodeId($element) {
    $automationId = ''
    try { $automationId = $element.Current.AutomationId } catch {}
    if (-not [string]::IsNullOrWhiteSpace($automationId)) { return $automationId }
    $runtimeId = ''
    try { $runtimeId = (($element.GetRuntimeId() | ForEach-Object { $_.ToString() }) -join '.') } catch {}
    if (-not [string]::IsNullOrWhiteSpace($runtimeId)) { return "rid:$runtimeId" }
    $handleValue = 0
    try { $handleValue = $element.Current.NativeWindowHandle } catch {}
    if ($handleValue -ne 0) { return "hwnd:$handleValue" }
    return ''
}

function Find-WaNodeRecursive($element, $expectedNodeId) {
    if ($null -eq $element) { return $null }
    if ((Get-WaElementNodeId $element) -eq $expectedNodeId) { return $element }
    $children = $null
    try {
        $children = $element.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    } catch {
        return $null
    }
    for ($i = 0; $i -lt $children.Count; $i++) {
        $matched = Find-WaNodeRecursive $children.Item($i) $expectedNodeId
        if ($null -ne $matched) { return $matched }
    }
    return $null
}

function Get-WaObservedValue($element, $conditionName) {
    switch ($conditionName.ToLowerInvariant()) {
        'exists' { return 'present' }
        'focused' {
            try { return ([bool]$element.Current.HasKeyboardFocus).ToString().ToLowerInvariant() } catch { return 'false' }
        }
        'value_equals' {
            try {
                $pattern = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
                return [string]$pattern.Current.Value
            } catch {
                return ''
            }
        }
        default {
            throw "unsupported WA wait condition '$conditionName'"
        }
    }
}

function Test-WaCondition($conditionName, $observedValue, $expectedValue) {
    switch ($conditionName.ToLowerInvariant()) {
        'exists' { return $observedValue -eq 'present' }
        'focused' { return $observedValue -eq 'true' }
        'value_equals' { return $observedValue -eq $expectedValue }
        default { throw "unsupported WA wait condition '$conditionName'" }
    }
}

$startedAt = [Environment]::TickCount64
$windowTitle = ''
$processId = $null
$observedValue = $null
$satisfied = $false
$detail = ''

while (([Environment]::TickCount64 - $startedAt) -le $timeoutMs) {
    $targetWindow = Get-WaTargetWindow
    if ($null -ne $targetWindow) {
        $windowTitle = $targetWindow.Current.Name
        $processId = $targetWindow.Current.ProcessId
        $targetElement = Find-WaNodeRecursive $targetWindow $nodeId
        if ($null -ne $targetElement) {
            $observedValue = Get-WaObservedValue $targetElement $conditionName
            if (Test-WaCondition $conditionName $observedValue $expectedValue) {
                $satisfied = $true
                $detail = 'condition satisfied'
                break
            }
            $detail = "condition not yet satisfied (observed '$observedValue')"
        } else {
            $detail = "target node '$nodeId' not found yet"
        }
    } else {
        $detail = 'target window not found yet'
    }
    Start-Sleep -Milliseconds $pollMs
}

$elapsed = [Math]::Max(0, ([Environment]::TickCount64 - $startedAt))
if (-not $satisfied -and [string]::IsNullOrWhiteSpace($detail)) {
    $detail = 'timeout elapsed without satisfying condition'
}

[PSCustomObject]@{
    window_title = $windowTitle
    process_id = $processId
    observed_value = $observedValue
    satisfied = $satisfied
    elapsed_ms = $elapsed
    detail = $detail
} | ConvertTo-Json -Depth 4 -Compress
"#
}

fn parse_capture_payload(json_payload: &str) -> Result<WindowsCapturePayload, Box<dyn Error>> {
    let payload: WindowsCapturePayload = serde_json::from_str(json_payload)?;
    if payload.nodes.is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "Windows capture returned no accessible nodes",
        )
        .into());
    }
    Ok(payload)
}

fn save_windows_capture_payload(
    root: &Path,
    session_id: &str,
    snapshot_name: &str,
    title_override: Option<&str>,
    payload: WindowsCapturePayload,
) -> Result<WaWindowsCaptureReport, Box<dyn Error>> {
    let window_title = if payload.window_title.trim().is_empty() {
        "Windows UIA capture".to_string()
    } else {
        payload.window_title
    };
    let title = title_override.unwrap_or(&window_title);
    let url = match payload.process_id {
        Some(process_id) => format!("windows://uia/process/{process_id}"),
        None => "windows://uia/window".to_string(),
    };
    let save_report = crate::wa::save_snapshot_report(
        root,
        session_id,
        snapshot_name,
        &url,
        title,
        payload.focus_node_id.as_deref(),
        payload.nodes,
    )?;
    Ok(WaWindowsCaptureReport {
        source: "windows-uia".to_string(),
        target_process_id: payload.process_id,
        target_window_title: window_title,
        snapshot: save_report.snapshot,
        snapshot_nda_path: save_report.snapshot_nda_path,
        session_nda_path: save_report.session_nda_path,
    })
}

pub fn capture_windows_snapshot_report(
    root: &Path,
    session_id: &str,
    snapshot_name: &str,
    title_override: Option<&str>,
    process_id: Option<u32>,
    window_name_contains: Option<&str>,
    max_depth: u32,
    max_children_per_node: usize,
) -> Result<WaWindowsCaptureReport, Box<dyn Error>> {
    if !cfg!(target_os = "windows") {
        return Err(IoError::new(
            ErrorKind::Unsupported,
            "WA Windows capture is only supported on Windows hosts",
        )
        .into());
    }

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .env("WA_CAPTURE_MAX_DEPTH", max_depth.to_string())
        .env("WA_CAPTURE_MAX_CHILDREN", max_children_per_node.to_string())
        .env(
            "WA_CAPTURE_PROCESS_ID",
            process_id.map(|value| value.to_string()).unwrap_or_default(),
        )
        .env(
            "WA_CAPTURE_WINDOW_NAME_CONTAINS",
            window_name_contains.unwrap_or_default(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(build_capture_script().as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(IoError::new(
            ErrorKind::Other,
            format!("Windows UIAutomation capture failed: {detail}"),
        )
        .into());
    }

    let payload = parse_capture_payload(&String::from_utf8_lossy(&output.stdout))?;
    save_windows_capture_payload(root, session_id, snapshot_name, title_override, payload)
}

fn parse_action_payload(json_payload: &str) -> Result<WindowsActionPayload, Box<dyn Error>> {
    let payload: WindowsActionPayload = serde_json::from_str(json_payload)?;
    if payload.executed_node_id.trim().is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "Windows action returned no executed node id",
        )
        .into());
    }
    Ok(payload)
}

fn parse_wait_payload(json_payload: &str) -> Result<WindowsWaitPayload, Box<dyn Error>> {
    let payload: WindowsWaitPayload = serde_json::from_str(json_payload)?;
    Ok(payload)
}

pub fn execute_windows_action_report(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    action: &str,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    input_value: Option<&str>,
) -> Result<WaWindowsActionReport, Box<dyn Error>> {
    if !cfg!(target_os = "windows") {
        return Err(IoError::new(
            ErrorKind::Unsupported,
            "WA Windows action execution is only supported on Windows hosts",
        )
        .into());
    }

    let plan = crate::wa::plan_action(
        root,
        session_id,
        snapshot_name,
        action,
        node_id,
        role,
        name,
        input_value,
    )?;
    let snapshot = crate::wa::load_snapshot(root, session_id, &plan.snapshot_name)?;
    let process_id = snapshot
        .url
        .strip_prefix("windows://uia/process/")
        .and_then(|value| value.parse::<u32>().ok());

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .env(
            "WA_ACTION_PROCESS_ID",
            process_id.map(|value| value.to_string()).unwrap_or_default(),
        )
        .env("WA_ACTION_WINDOW_NAME_CONTAINS", snapshot.title.clone())
        .env("WA_ACTION_NODE_ID", plan.matched.id.clone())
        .env("WA_ACTION_NAME", action)
        .env("WA_ACTION_VALUE", input_value.unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(build_action_script().as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(IoError::new(
            ErrorKind::Other,
            format!("Windows UIAutomation action execution failed: {detail}"),
        )
        .into());
    }

    let payload = parse_action_payload(&String::from_utf8_lossy(&output.stdout))?;
    build_action_report_from_payload(
        root,
        session_id,
        snapshot_name,
        action,
        node_id,
        role,
        name,
        input_value,
        payload,
    )
}

pub fn wait_for_windows_condition_report(
    root: &Path,
    session_id: &str,
    snapshot_name: Option<&str>,
    condition: &str,
    node_id: Option<&str>,
    role: Option<&str>,
    name: Option<&str>,
    expected_value: Option<&str>,
    timeout_ms: u64,
    poll_interval_ms: u64,
) -> Result<WaWindowsWaitReport, Box<dyn Error>> {
    if !cfg!(target_os = "windows") {
        return Err(IoError::new(
            ErrorKind::Unsupported,
            "WA Windows wait execution is only supported on Windows hosts",
        )
        .into());
    }

    let resolve = crate::wa::resolve_selector(
        root,
        session_id,
        snapshot_name,
        node_id,
        role,
        name,
        None,
    )?;
    let snapshot = crate::wa::load_snapshot(root, session_id, &resolve.snapshot_name)?;
    let process_id = snapshot
        .url
        .strip_prefix("windows://uia/process/")
        .and_then(|value| value.parse::<u32>().ok());

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .env(
            "WA_WAIT_PROCESS_ID",
            process_id.map(|value| value.to_string()).unwrap_or_default(),
        )
        .env("WA_WAIT_WINDOW_NAME_CONTAINS", snapshot.title.clone())
        .env("WA_WAIT_NODE_ID", resolve.matched.id.clone())
        .env("WA_WAIT_CONDITION", condition)
        .env("WA_WAIT_EXPECTED_VALUE", expected_value.unwrap_or_default())
        .env("WA_WAIT_TIMEOUT_MS", timeout_ms.to_string())
        .env("WA_WAIT_POLL_MS", poll_interval_ms.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(build_wait_script().as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(IoError::new(
            ErrorKind::Other,
            format!("Windows UIAutomation wait failed: {detail}"),
        )
        .into());
    }

    let payload = parse_wait_payload(&String::from_utf8_lossy(&output.stdout))?;
    build_wait_report_from_payload(
        root,
        session_id,
        snapshot_name,
        condition,
        node_id,
        role,
        name,
        expected_value,
        timeout_ms,
        poll_interval_ms,
        payload,
    )
}

pub fn render_windows_wait_report(report: &WaWindowsWaitReport) -> String {
    format!(
        "Waited for Windows WA condition '{}' in session '{}' snapshot '{}'.\nTarget window: {}\nProcess id: {}\nNode: {} [{}] '{}'\nSatisfied: {}\nObserved: {}\nElapsed: {}ms / {}ms\nDetail: {}\nSnapshot NDA: {}",
        report.condition,
        report.session_id,
        report.snapshot_name,
        report.target_window_title,
        report
            .target_process_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        report.matched.id,
        report.matched.role,
        report.matched.name,
        report.satisfied,
        report.observed_value.as_deref().unwrap_or("unknown"),
        report.elapsed_ms,
        report.timeout_ms,
        report.detail,
        report.snapshot_nda_path,
    )
}

pub fn render_windows_action_report(report: &WaWindowsActionReport) -> String {
    format!(
        "Executed Windows WA action '{}' in session '{}' snapshot '{}'.\nTarget window: {}\nProcess id: {}\nNode: {} [{}] '{}'\nExecution: {} ({})\nSnapshot NDA: {}",
        report.action,
        report.session_id,
        report.snapshot_name,
        report.target_window_title,
        report
            .target_process_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        report.matched.id,
        report.matched.role,
        report.matched.name,
        report.execution_status,
        report.execution_detail,
        report.snapshot_nda_path,
    )
}

pub fn render_windows_capture_report(report: &WaWindowsCaptureReport) -> String {
    format!(
        "Captured Windows WA snapshot '{}' for session '{}'.\nTarget window: {}\nProcess id: {}\nNodes: {}\nFocused node: {}\nSnapshot NDA: {}",
        report.snapshot.snapshot_name,
        report.snapshot.session_id,
        report.target_window_title,
        report
            .target_process_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        report.snapshot.nodes.len(),
        report
            .snapshot
            .focus_node_id
            .as_deref()
            .unwrap_or("unknown"),
        report.snapshot_nda_path,
    )
}

#[cfg(test)]
pub(crate) fn save_windows_capture_report_from_json(
    root: &Path,
    session_id: &str,
    snapshot_name: &str,
    title_override: Option<&str>,
    json_payload: &str,
) -> Result<WaWindowsCaptureReport, Box<dyn Error>> {
    let payload = parse_capture_payload(json_payload)?;
    save_windows_capture_payload(root, session_id, snapshot_name, title_override, payload)
}

#[cfg(test)]
mod tests {
    use super::{
        build_action_report_from_payload, build_wait_report_from_payload,
        save_windows_capture_report_from_json, WindowsActionPayload, WindowsWaitPayload,
    };

    #[test]
    fn saves_windows_capture_payload_into_wa_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        crate::wa::create_session_report(&root, "desktop-auth").unwrap();

        let report = save_windows_capture_report_from_json(
            &root,
            "desktop-auth",
            "live-window",
            None,
            r#"{
                "window_title": "Sign in",
                "process_id": 4242,
                "focus_node_id": "email-field",
                "nodes": [
                    {
                        "id": "email-field",
                        "role": "edit",
                        "name": "Email",
                        "value": "",
                        "actions": ["focus", "type"],
                        "visible": true,
                        "enabled": true,
                        "provenance": "native",
                        "confidence": 1.0
                    },
                    {
                        "id": "continue-button",
                        "role": "button",
                        "name": "Continue",
                        "value": "",
                        "actions": ["click"],
                        "visible": true,
                        "enabled": true,
                        "provenance": "native",
                        "confidence": 1.0
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(report.source, "windows-uia");
        assert_eq!(report.target_process_id, Some(4242));
        assert_eq!(report.target_window_title, "Sign in");
        assert_eq!(report.snapshot.url, "windows://uia/process/4242");
        assert_eq!(report.snapshot.title, "Sign in");
        assert_eq!(report.snapshot.focus_node_id.as_deref(), Some("email-field"));
        assert_eq!(report.snapshot.nodes.len(), 2);
        assert!(report.snapshot_nda_path.contains(".velocity/wa-snapshots/desktop-auth--live-window.nda"));
        assert!(report.session_nda_path.contains(".velocity/wa-sessions/desktop-auth.nda"));

        let saved = crate::wa::read_snapshot_report(&root, "desktop-auth", "live-window").unwrap();
        assert_eq!(saved.snapshot.nodes[1].id, "continue-button");
        assert_eq!(saved.snapshot.title, "Sign in");
    }

    #[test]
    fn builds_windows_action_report_from_planned_selector() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        crate::wa::create_session_report(&root, "desktop-auth").unwrap();
        crate::wa::save_snapshot_report(
            &root,
            "desktop-auth",
            "login-form",
            "windows://uia/process/4242",
            "Sign in",
            Some("email-field"),
            vec![
                crate::wa::WaNode {
                    id: "email-field".to_string(),
                    role: "textbox".to_string(),
                    name: "Email".to_string(),
                    value: "".to_string(),
                    actions: vec!["focus".to_string(), "type".to_string()],
                    visible: true,
                    enabled: true,
                    provenance: "native".to_string(),
                    confidence: 1.0,
                },
                crate::wa::WaNode {
                    id: "continue-button".to_string(),
                    role: "button".to_string(),
                    name: "Continue".to_string(),
                    value: "".to_string(),
                    actions: vec!["click".to_string()],
                    visible: true,
                    enabled: true,
                    provenance: "native".to_string(),
                    confidence: 1.0,
                },
            ],
        )
        .unwrap();

        let report = build_action_report_from_payload(
            &root,
            "desktop-auth",
            Some("login-form"),
            "click",
            None,
            Some("button"),
            Some("Continue"),
            None,
            WindowsActionPayload {
                window_title: "Sign in".to_string(),
                process_id: Some(4242),
                executed_node_id: "continue-button".to_string(),
                status: "executed".to_string(),
                detail: "invoke pattern executed".to_string(),
            },
        )
        .unwrap();

        assert_eq!(report.session_id, "desktop-auth");
        assert_eq!(report.snapshot_name, "login-form");
        assert_eq!(report.action, "click");
        assert_eq!(report.matched.id, "continue-button");
        assert_eq!(report.executed_node_id, "continue-button");
        assert_eq!(report.execution_status, "executed");
        assert_eq!(report.target_process_id, Some(4242));
        assert!(report.preconditions.iter().any(|value| value == "supports:click"));
        assert!(report.snapshot_nda_path.contains(".velocity/wa-snapshots/desktop-auth--login-form.nda"));
    }

    #[test]
    fn builds_windows_wait_report_from_resolved_selector() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        crate::wa::create_session_report(&root, "desktop-auth").unwrap();
        crate::wa::save_snapshot_report(
            &root,
            "desktop-auth",
            "login-form",
            "windows://uia/process/4242",
            "Sign in",
            Some("email-field"),
            vec![crate::wa::WaNode {
                id: "email-field".to_string(),
                role: "textbox".to_string(),
                name: "Email".to_string(),
                value: "agent@example.com".to_string(),
                actions: vec!["focus".to_string(), "type".to_string()],
                visible: true,
                enabled: true,
                provenance: "native".to_string(),
                confidence: 1.0,
            }],
        )
        .unwrap();

        let report = build_wait_report_from_payload(
            &root,
            "desktop-auth",
            Some("login-form"),
            "value_equals",
            None,
            Some("textbox"),
            Some("Email"),
            Some("agent@example.com"),
            3000,
            100,
            WindowsWaitPayload {
                window_title: "Sign in".to_string(),
                process_id: Some(4242),
                observed_value: Some("agent@example.com".to_string()),
                satisfied: true,
                elapsed_ms: 120,
                detail: "condition satisfied".to_string(),
            },
        )
        .unwrap();

        assert_eq!(report.session_id, "desktop-auth");
        assert_eq!(report.snapshot_name, "login-form");
        assert_eq!(report.condition, "value_equals");
        assert_eq!(report.expected_value.as_deref(), Some("agent@example.com"));
        assert_eq!(report.observed_value.as_deref(), Some("agent@example.com"));
        assert!(report.satisfied);
        assert_eq!(report.elapsed_ms, 120);
        assert_eq!(report.target_process_id, Some(4242));
        assert_eq!(report.matched.id, "email-field");
        assert!(report.snapshot_nda_path.contains(".velocity/wa-snapshots/desktop-auth--login-form.nda"));
    }
}
