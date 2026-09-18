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
# Records the filters the caller actually asked for, so a filter that matches
# nothing can be reported instead of quietly dropped (bug #64).
$filterText = @()
if (-not [string]::IsNullOrWhiteSpace($processIdFilter)) {
    $filterText += "processId $processIdFilter"
    for ($i = 0; $i -lt $topLevel.Count; $i++) {
        $candidate = $topLevel.Item($i)
        # `[long]`, not `[int]`: a process id is unsigned, so casting it down
        # made anything above Int32.MaxValue throw a conversion error instead of
        # simply reporting no match.
        if ($candidate.Current.ProcessId -eq [long]$processIdFilter) {
            $target = $candidate
            break
        }
    }
}
if ($null -eq $target -and -not [string]::IsNullOrWhiteSpace($windowNameFilter)) {
    $filterText += "windowNameContains '$windowNameFilter'"
    for ($i = 0; $i -lt $topLevel.Count; $i++) {
        $candidate = $topLevel.Item($i)
        $name = $candidate.Current.Name
        if (-not [string]::IsNullOrWhiteSpace($name) -and $name.ToLowerInvariant().Contains($windowNameFilter.ToLowerInvariant())) {
            $target = $candidate
            break
        }
    }
}
# The fallbacks belong to the unfiltered path only. A caller that named a
# process asked for that process; capturing some other window and reporting it
# as a success is how a stale process id read back as a fresh snapshot.
if ($null -eq $target -and $filterText.Count -eq 0) {
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
if ($null -eq $target -and $filterText.Count -eq 0) {
    for ($i = 0; $i -lt $topLevel.Count; $i++) {
        $candidate = $topLevel.Item($i)
        $name = $candidate.Current.Name
        if (-not [string]::IsNullOrWhiteSpace($name)) {
            $target = $candidate
            break
        }
    }
}
if ($null -eq $target -and $filterText.Count -eq 0 -and $topLevel.Count -gt 0) {
    $target = $topLevel.Item(0)
}
if ($null -eq $target) {
    if ($filterText.Count -gt 0) {
        $seen = @()
        for ($i = 0; $i -lt $topLevel.Count; $i++) {
            $candidate = $topLevel.Item($i)
            $candidateName = $candidate.Current.Name
            if ([string]::IsNullOrWhiteSpace($candidateName)) { $candidateName = '<untitled>' }
            $seen += ('{0} (pid {1})' -f $candidateName, $candidate.Current.ProcessId)
        }
        throw ('no top-level window matched ' + ($filterText -join ' and ') + '; ' + $topLevel.Count + ' window(s) on the desktop: ' + (($seen | Select-Object -First 12) -join ', '))
    }
    throw 'no Windows UIAutomation target window found'
}

Add-WaNode $target 0 $maxDepth $maxChildren

[PSCustomObject]@{
    window_title = ($target.Current.Name)
    process_id = ($target.Current.ProcessId)
    focus_node_id = $focusNodeId
    # `.ToArray()` rather than array-wrapping the list in `@()`: the latter
    # throws "Argument types do not match" on Windows PowerShell 5.1 (bug #17).
    nodes = $nodeList.ToArray()
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
            if ($candidate.Current.ProcessId -eq [long]$processIdFilter) {
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
            if ($candidate.Current.ProcessId -eq [long]$processIdFilter) {
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
    # No fallback to the foreground window here: the action path throws on a
    # null target, so returning null is the honest answer.
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

$startedSw = [System.Diagnostics.Stopwatch]::StartNew()
$windowTitle = ''
$processId = $null
$observedValue = $null
$satisfied = $false
$detail = ''

while ($startedSw.Elapsed.TotalMilliseconds -le $timeoutMs) {
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

$elapsed = [Math]::Max(0, [int]$startedSw.Elapsed.TotalMilliseconds)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The defect these lines pin (bug #64): `wa_capture_windows_snapshot`
    /// asked for one process, found no window belonging to it, and captured the
    /// foreground window instead - then reported success. A supplied filter has
    /// to be either honoured or reported as unmatched.
    #[test]
    fn a_named_process_that_matches_nothing_is_reported_not_substituted() {
        let script = build_capture_script();
        assert!(
            script.contains("no top-level window matched"),
            "an unmatched filter has to say so: {script}"
        );
        assert!(
            script.contains("$filterText += \"processId $processIdFilter\""),
            "the process id the caller named has to reach the error text"
        );
        assert!(
            script.contains("$filterText += \"windowNameContains '$windowNameFilter'\""),
            "so does the title filter"
        );
    }

    /// Every fallback in the capture script is for the unfiltered path. If one
    /// of them loses its guard the substitution comes straight back.
    #[test]
    fn the_unfiltered_fallbacks_are_all_gated_on_having_no_filter() {
        let script = build_capture_script();
        let fallbacks = [
            "if ($null -eq $target -and $filterText.Count -eq 0) {\n    $foreground",
            "if ($null -eq $target -and $filterText.Count -eq 0) {\n    for ($i = 0",
            "if ($null -eq $target -and $filterText.Count -eq 0 -and $topLevel.Count -gt 0)",
        ];
        for fallback in fallbacks {
            assert!(
                script.contains(fallback),
                "fallback is reachable with a filter supplied: missing `{fallback}`"
            );
        }
        // Once at the P/Invoke declaration and once at the call site - two is
        // correct; three would mean a second, unguarded fallback.
        assert_eq!(
            script.matches("GetForegroundWindow").count(),
            2,
            "the foreground window may only be consulted once, inside the guard"
        );
        assert_eq!(
            script
                .matches("$foreground = [WaNative]::GetForegroundWindow()")
                .count(),
            1,
            "the guarded call site has to be unique"
        );
    }

    /// Process ids are unsigned. Reading one as `[int]` made anything above
    /// Int32.MaxValue fail with a .NET conversion complaint - a developer-facing
    /// message, and a different one per script - instead of "no window matched".
    #[test]
    fn process_ids_are_compared_as_unsigned_everywhere() {
        for script in [
            build_capture_script(),
            build_action_script(),
            build_wait_script(),
        ] {
            assert!(
                !script.contains("[int]$processIdFilter"),
                "a large process id would throw a conversion error here"
            );
            assert!(
                script.contains("[long]$processIdFilter"),
                "the filter has to be read as a wide integer"
            );
        }
    }

    /// The action path never had the bug - it returns null and the caller
    /// throws. Guarded so a "helpful" foreground fallback can't be added there.
    #[test]
    fn the_action_and_wait_paths_still_refuse_a_null_target() {
        for script in [build_action_script(), build_wait_script()] {
            assert!(
                !script.contains("GetForegroundWindow"),
                "neither path may act on a window the caller did not name"
            );
        }
        assert!(build_action_script()
            .contains("throw 'no Windows UIAutomation target window found for action execution'"));
    }

    /// End to end against the real shell: the generated script, a process id
    /// that owns no window, and the answer the caller gets. The id is above
    /// Int32::MAX, so this also proves the `[long]` comparison rather than a
    /// conversion error.
    #[cfg(target_os = "windows")]
    #[test]
    fn an_unmatched_process_id_refuses_the_capture_in_the_shell() {
        let temp = tempfile::tempdir().unwrap();
        let err = crate::wa::capture_windows_snapshot_report(
            temp.path(),
            "b64",
            "unmatched",
            None,
            Some(4_000_000_000),
            None,
            1,
            1,
        )
        .expect_err("no window can belong to that process id");
        let text = err.to_string();
        assert!(
            text.contains("no top-level window matched processId 4000000000"),
            "got: {text}"
        );
        assert!(
            !text.contains("Int32"),
            "must not surface a conversion error: {text}"
        );
        assert!(
            !temp.path().join("wa-snapshots").exists()
                || std::fs::read_dir(temp.path().join("wa-snapshots"))
                    .map(|mut it| it.next().is_none())
                    .unwrap_or(true),
            "a refused capture must not leave a snapshot behind"
        );
    }
}
