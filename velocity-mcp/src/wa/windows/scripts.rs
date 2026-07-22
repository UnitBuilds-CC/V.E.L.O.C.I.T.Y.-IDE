pub fn build_capture_script() -> &'static str {
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

pub fn build_action_script() -> &'static str {
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

pub fn build_wait_script() -> &'static str {
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
