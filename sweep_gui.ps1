<#
.SYNOPSIS
    Live sweep of every reachable control in the running IDE, through the GUI
    control bridge on 127.0.0.1:19821.

.DESCRIPTION
    Drives the app the way an external driver would: it asks the bridge for the
    app map and the command palette, then walks every node and every entry those
    return. Nothing here is a hardcoded list of buttons -- a panel or sub-tab
    added to the app's tables gets swept automatically, which is the point.

    Each phase asserts on a follow-up GetState rather than on the command's own
    reply, so "the handler said yes" is never mistaken for "the screen changed".

    Phases:
      1  baseline state
      2  app map: nodes, edges, orphans
      3  command inventory and risk tiers
      4  every activity-bar rail and sub-tab
      5  every dock panel
      6  dock tab switching
      7  every navigation-tier command
      8  the gates: modify/execute tiers, routes and bad names
      9  the settings button, by every route that reaches it
     10  every workspace mode, and the controls only that mode exposes
     11  every modify-tier command, run for real
     11b the execute-tier commands that cost nothing to press
     12  the quick-open, go-to and find overlays, last

    A mode refusal is treated as information, not a dead end: see Press. The
    gate says which profiles carry a command, so the sweep switches to one,
    presses it there and comes back, rather than skipping the entry every mode
    deliberately hides.

.PARAMETER AllowUnsafe
    Run the modify tier for real and press the free execute commands. Off by
    default: they write files, close tabs and spawn things, so point this at a
    throwaway workspace.

.PARAMETER StartIn
    The mode the sweep begins and ends in. Defaults to whatever the instance
    restored.

.NOTES
    Needs a running instance to talk to. Launch the release binary against a
    throwaway workspace so the write tier has somewhere harmless to land:

        Start-Process target\release\velocity_ide_gui.exe `
          -ArgumentList '--workspace','target\sweep-workspace' -WindowStyle Minimized

    It is the answer to "have you actually pressed every button": the counts it
    prints -- rails, sub-tabs, dock panels, tabs cycled, palette entries by tier
    -- are what a coverage claim should be measured against.

.EXAMPLE
    # Read-only: navigation surface, gates and tab switching, nothing written.
    powershell -NoProfile -ExecutionPolicy Bypass -File .\sweep_gui.ps1

.EXAMPLE
    # Everything, including the modify tier and the free execute commands.
    powershell -NoProfile -ExecutionPolicy Bypass -File .\sweep_gui.ps1 -AllowUnsafe
#>
param(
    [switch]$AllowUnsafe,
    [string]$TokenFile = "$PSScriptRoot\target\gui-sweep-workspace\.velocity\gui_control.token",
    [string]$Report = "$PSScriptRoot\sweep_report.txt",
    [string]$StartIn = '',
    [int]$TimeoutMs = 20000
)

$ErrorActionPreference = 'Stop'
$script:pass = 0
$script:fail = 0
$script:skip = 0
$script:notes = New-Object System.Collections.Generic.List[string]
$script:lines = New-Object System.Collections.Generic.List[string]
$script:token = $null
$script:modeMoved = $false

function Log([string]$text) { $script:lines.Add($text); Write-Host $text }

function Check([string]$name, [bool]$ok, [string]$detail = '') {
    if ($ok) { $script:pass++; Log "  PASS  $name" }
    else { $script:fail++; Log "  FAIL  $name  $detail" }
}

function Note([string]$name, [string]$why) {
    $script:skip++; $script:notes.Add("$name :: $why"); Log "  SKIP  $name  $why"
}

function Send($cmd, $params) {
    if (-not $script:token) { $script:token = (Get-Content $TokenFile -Raw).Trim() }
    # The envelope is adjacently tagged, so even the no-argument commands have
    # to carry an (empty) params object.
    if ($null -eq $params) { $params = @{} }
    $payload = [ordered]@{ auth_token = $script:token; command = $cmd; params = $params }
    $json = $payload | ConvertTo-Json -Compress -Depth 12

    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $client.Connect('127.0.0.1', 19821)
        $client.ReceiveTimeout = $TimeoutMs
        $client.SendTimeout = $TimeoutMs
        $stream = $client.GetStream()
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($json + "`n")
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush()
        $line = (New-Object System.IO.StreamReader($stream)).ReadLine()
        if ($null -eq $line) { return [pscustomobject]@{ success = $false; error = 'bridge closed without a reply' } }
        return $line | ConvertFrom-Json
    } catch {
        return [pscustomobject]@{ success = $false; error = $_.Exception.Message }
    } finally { $client.Close() }
}

function Ok($resp) { [bool]$resp.success }
function Err($resp) { [string]$resp.error }
function Body($resp) { if ($resp.data) { $resp.data } else { [pscustomobject]@{} } }
function Settled() { (Body (Send 'GetState' $null)) }

function Switch-Mode($name) {
    Send 'RunCommand' @{ label = "Mode: $name"; allow_unsafe = $true }
}

# RunCommand a palette entry, satisfying a mode refusal instead of accepting it.
#
# The gate's message names the profiles that do carry the command, so a refusal
# is actionable: switch into one of them, press the entry there, and record that
# the mode moved so the caller can restore it. Without this, every mode-
# restricted write or execute entry hides behind whichever profile the sweep
# happens to start on -- which is how 'Build' and 'Run Selected Flow' came to
# fail a run that had never switched back out of Accessibility.
function Press($label, $unsafe) {
    $params = @{ label = $label }
    if ($unsafe) { $params.allow_unsafe = $true }
    $resp = Send 'RunCommand' $params
    $why = Err $resp
    if ($why -notmatch 'only available in') { return $resp }
    # Re-match positively: -notmatch wipes $matches, and the mode list is
    # slash-joined, so both halves of the pair have to be read from a match
    # that actually succeeded.
    if (-not ($why -match 'only available in (.+?) \(current mode:')) { return $resp }
    foreach ($want in @($matches[1] -split '/')) {
        $moved = Switch-Mode $want.Trim()
        if (-not (Ok $moved)) { continue }
        $script:modeMoved = $true
        $again = Send 'RunCommand' $params
        if (Ok $again) { return $again }
        $resp = $again
        $why = Err $resp
        if ($why -notmatch 'only available in') { return $resp }
    }
    return $resp
}

Log "V.E.L.O.C.I.T.Y. GUI sweep  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Log ''

# ── 1. Baseline ────────────────────────────────────────────────────────────
Log '[1] baseline state'
$base = Body (Send 'GetState' $null)
Check 'GetState answers' ($null -ne $base.workspace_root)
$startMode = $base.mode
if ($StartIn -and ($StartIn -ne $base.mode)) {
    $null = Switch-Mode $StartIn
    $startMode = (Settled).mode
}
Log "      workspace=$($base.workspace_root) mode=$startMode central=$($base.central_area) rail=$($base.active_panel)"

# ── 2. App map ─────────────────────────────────────────────────────────────
Log '[2] app map'
$mapResp = Send 'AppMap' @{}
if (-not (Ok $mapResp)) { Check 'AppMap answers' $false (Err $mapResp); Log 'cannot continue'; exit 1 }
Check 'AppMap answers' $true
$map = Body $mapResp
$nodes = @($map.nodes)
$edges = @($map.edges)
$orphans = @($map.summary.orphans)
Log "      $($nodes.Count) nodes, $($edges.Count) edges"
Check 'map has no orphan nodes' ($orphans.Count -eq 0) ($orphans -join ',')

$rails = @($nodes | Where-Object { $_.kind -eq 'rail' })
$sections = @($nodes | Where-Object { $_.kind -eq 'rail-section' })
$panels = @($nodes | Where-Object { $_.kind -eq 'panel' })
$modes = @($nodes | Where-Object { $_.kind -eq 'mode' })
$mapCmds = @($nodes | Where-Object { $_.kind -eq 'command' })
Log "      rails=$($rails.Count) sub-tabs=$($sections.Count) panels=$($panels.Count) modes=$($modes.Count) commands=$($mapCmds.Count)"
Check 'every rail is on the map' ($rails.Count -ge 8) "found $($rails.Count)"
Check 'every sub-tab is on the map' ($sections.Count -ge 34) "found $($sections.Count)"
Check 'every dock panel is on the map' ($panels.Count -ge 52) "found $($panels.Count)"
Check 'all four workspace modes are on the map' ($modes.Count -eq 4) "found $($modes.Count)"

# ── 3. Command inventory ───────────────────────────────────────────────────
Log '[3] command palette'
$invResp = Send 'ListCommands' @{}
Check 'ListCommands answers' (Ok $invResp) (Err $invResp)
$inv = Body $invResp
$commands = @($inv.commands)
Log "      $($commands.Count) commands across $($inv.categories.Count) categories"
Log ("      tiers: " + ((@($commands | Group-Object risk) | ForEach-Object { "$($_.Name)=$($_.Count)" }) -join '  '))
Check 'every command carries a risk tier' (@($commands | Where-Object { -not $_.risk }).Count -eq 0)
Check 'every command carries a category' (@($commands | Where-Object { -not $_.category }).Count -eq 0)
# The map is built from the palette, so a mismatch means one of them is lying.
Check 'the map carries every palette command' ($mapCmds.Count -eq $commands.Count) "map $($mapCmds.Count) vs palette $($commands.Count)"

# ── 4. Rails and sub-tabs ──────────────────────────────────────────────────
Log '[4] activity-bar rails and their sub-tabs'
foreach ($rail in ($rails | Sort-Object { $_.detail })) {
    $slug = $rail.id.Substring('rail:'.Length)
    $subs = @($sections | Where-Object { $_.id.StartsWith("$($rail.id)/") })
    Log "      $slug : $($subs.Count) sub-tabs"

    $null = (Send 'NavigatePanel' @{ panel = $slug })
    $st = (Settled)
    Check "rail '$slug' selected" ($st.active_panel -eq $slug) "rail is '$($st.active_panel)'"

    foreach ($sub in $subs) {
        $want = $sub.id.Substring($sub.id.LastIndexOf('/') + 1)
        $r = Send 'SelectSubTab' @{ rail = $slug; sub_tab = $want }
        if (-not (Ok $r)) { Check "$slug/$want" $false (Err $r); continue }
        $st = (Settled)
        Check "sub-tab '$slug/$want' shows '$($sub.label)'" ($st.active_section -eq $sub.label) "section is '$($st.active_section)'"
    }
}

# ── 5. Dock panels ─────────────────────────────────────────────────────────
Log '[5] dock panels, reached through the route finder'
$bad = 0
foreach ($panel in $panels) {
    $r = Send 'NavigateTo' @{ target = $panel.id }
    if (-not (Ok $r)) { Check "panel $($panel.id)" $false (Err $r); $bad++; continue }
    $st = (Settled)
    if ($st.focused_tab -eq $panel.label -and $st.central_area -eq 'dock') {
        $script:pass++
    } else {
        $script:fail++; $bad++
        Log "  FAIL  $($panel.id) '$($panel.label)' -> focused='$($st.focused_tab)' central='$($st.central_area)'"
    }
}
Check "all $($panels.Count) dock panels come on screen" ($bad -eq 0) "$bad failed"

# ── 6. Tab switching ───────────────────────────────────────────────────────
Log '[6] dock tab switching'
$lt = Send 'ListTabs' @{}
Check 'ListTabs answers' (Ok $lt) (Err $lt)
$tabs = @(Body $lt).tabs
$docked = @($tabs | Where-Object { $_.docked })
Log "      $($tabs.Count) tabs, $($docked.Count) actually in the dock"
Check 'the dock has tabs to switch between' ($docked.Count -ge 2) "$($docked.Count) docked"

$bad = 0
foreach ($tab in $docked) {
    $r = Send 'SelectTab' @{ tab = "$($tab.tab_id)" }
    if (-not (Ok $r)) { Check "tab $($tab.tab_id) '$($tab.title)'" $false (Err $r); $bad++; continue }
    $st = (Settled)
    if ($st.focused_tab -eq $tab.title) {
        $script:pass++
    } else {
        $script:fail++; $bad++
        Log "  FAIL  tab $($tab.tab_id) '$($tab.title)' -> focused='$($st.focused_tab)'"
    }
}
Check 'every docked tab takes focus when named by id' ($bad -eq 0) "$bad failed"

# By slug too, since that is how a caller without an id addresses a tab. Two
# panels share the title "Memory", so id is the only unambiguous handle; slug
# is the one a human would type.
$bad = 0
foreach ($tab in ($docked | Where-Object { $_.slug })) {
    $r = Send 'SelectTab' @{ tab = $tab.slug }
    if (-not (Ok $r)) { $bad++; Log "  FAIL  select '$($tab.slug)' -> $(Err $r)"; continue }
    if ((Settled).focused_tab -ne $tab.title) { $bad++; Log "  FAIL  select '$($tab.slug)' -> not focused" }
}
Check 'every docked tab takes focus when named by slug' ($bad -eq 0) "$bad failed"

# ── 7. Navigation-tier commands ────────────────────────────────────────────
Log '[7] every navigation-tier command'
# Two hold-backs. The overlay openers (quick-open, go-to-line, go-to-symbol,
# the find bars) are deferred to phase 12, because nothing over the bridge
# presses Escape and running them early would leave the app wearing an overlay
# for the rest of the sweep. Mode-hidden entries are skipped here and picked up
# in phase 10, which visits the mode that shows them.
$nav = @($commands | Where-Object { $_.risk -eq 'navigate' -and -not $_.interactive })
function Test-Overlay($c) {
    $c.label -like 'Go to File*' -or $c.label -like 'Go to Line*' -or
    $c.label -like 'Go to Symbol*' -or $c.label -eq 'Find' -or
    $c.label -eq 'Find / Replace'
}
$navNow = @($nav | Where-Object { -not (Test-Overlay $_) })
$navOverlay = @($nav | Where-Object { Test-Overlay $_ })
$ran = New-Object System.Collections.Generic.HashSet[string]

foreach ($c in $navNow) {
    $r = Send 'RunCommand' @{ label = $c.label }
    if ((Err $r) -match 'only available in') { Note "'$($c.label)'" 'hidden in the starting mode; phase 10 visits the mode that shows it'; continue }
    if (-not (Ok $r)) { Check "run '$($c.label)'" $false (Err $r); continue }
    [void]$ran.Add($c.label)
    if ($c.label -in @('Next Tab', 'Previous Tab', 'Go Back', 'Go Forward')) {
        $st = (Settled)
        Check "'$($c.label)' ran and left something focused" ([bool]$st.focused_tab) "focused_tab is empty"
    } else {
        Check "'$($c.label)' ran" $true
    }
}

# ── 8. The gates ───────────────────────────────────────────────────────────
Log '[8] risk gates'
$gated = @($commands | Where-Object { $_.risk -ne 'navigate' })
$leaks = @()
foreach ($c in $gated) {
    $r = Send 'RunCommand' @{ label = $c.label }
    if (Ok $r) { $leaks += $c.label }
}
Check "none of the $($gated.Count) gated commands ran without allow_unsafe" ($leaks.Count -eq 0) "leaked: $($leaks -join ', ')"

# A native modal blocks the frame loop until a person answers it, so the
# opt-in flag cannot make running one over a socket safe.
foreach ($c in @($commands | Where-Object { $_.interactive })) {
    $r = Send 'RunCommand' @{ label = $c.label; allow_unsafe = $true }
    Check "'$($c.label)' is refused even with allow_unsafe" (-not (Ok $r)) 'it ran, and would have hung the UI thread on a dialog'
    if (-not (Ok $r)) {
        Check "  ...and the refusal says why" ((Err $r) -match 'native modal') (Err $r)
    }
}

# A route must not smuggle a state change past the tier gate.
$r = Send 'NavigateTo' @{ target = 'cmd:Build' }
Check 'NavigateTo refuses an execute-tier command' (-not (Ok $r)) 'gui_navigate_to accepted a build'
$r = Send 'NavigateTo' @{ target = 'mode:coder' }
Check 'NavigateTo refuses a persisting mode switch' (-not (Ok $r)) 'gui_navigate_to accepted a mode change'
# Bad names are errors, not a silent fall-through to whatever index 0 happens to be.
foreach ($junk in @('panel:definitely-not-real', 'rail:not_a_rail', 'nonsense')) {
    $r = Send 'NavigateTo' @{ target = $junk }
    Check "NavigateTo rejects '$junk'" (-not (Ok $r)) 'it resolved to something'
}
$r = Send 'RunCommand' @{ label = 'nonsense-not-in-the-palette' }
Check 'RunCommand rejects an unknown label' (-not (Ok $r)) (Err $r)
$r = Send 'SelectSubTab' @{ rail = 'files'; sub_tab = 'nope' }
Check 'SelectSubTab rejects an unknown section' (-not (Ok $r)) (Err $r)
$r = Send 'SelectTab' @{ tab = 'tab-999999' }
Check 'SelectTab rejects an unknown tab' (-not (Ok $r)) (Err $r)

# ── 9. Settings, every way in ──────────────────────────────────────────────
Log '[9] the settings button'
# Two different calls reach it and they are not interchangeable: the palette
# entry and the gear are `toggle_panel`, which closes an already-open panel,
# while gui_navigate_to focuses and must never close. Starting from a known
# state is the only way to tell those apart on a live app.
if ((Settled).focused_tab -eq 'Settings') { $null = Send 'TogglePanel' @{ panel = 'settings' } }
$before = (Settled).focused_tab

$null = Send 'RunCommand' @{ label = 'Settings' }
$after = (Settled).focused_tab
Check "the Settings palette command opens it from '$before'" ($after -eq 'Settings') "focused='$after'"
$null = Send 'RunCommand' @{ label = 'Settings' }
$after = (Settled).focused_tab
Check 'pressing it again closes it (it is a toggle)' ($after -ne 'Settings') "still focused on '$after'"

$null = Send 'NavigateTo' @{ target = 'settings' }
Check 'gui_navigate_to settings opens it' ((Settled).focused_tab -eq 'Settings')
$null = Send 'NavigateTo' @{ target = 'settings' }
Check 'navigating to an open panel leaves it open' ((Settled).focused_tab -eq 'Settings') 'gui_navigate_to toggled it closed'

$tabs2 = @(Body (Send 'ListTabs' @{})).tabs
Check 'an open settings panel is a dock tab' (@($tabs2 | Where-Object { $_.slug -eq 'settings' }).Count -ge 1) 'no settings tab'
$null = Send 'SelectTab' @{ tab = 'settings' }
Check 'SelectTab settings focuses it' ((Settled).focused_tab -eq 'Settings')
$null = Send 'TogglePanel' @{ panel = 'settings' }
Check 'TogglePanel closes it again' ((Settled).focused_tab -ne 'Settings')
$tabs3 = @(Body (Send 'ListTabs' @{})).tabs
Check 'a closed panel is not reported as a tab' (@($tabs3 | Where-Object { $_.slug -eq 'settings' }).Count -eq 0) 'ListTabs still lists it'

# ── 10. Every workspace mode ───────────────────────────────────────────────
Log '[10] every workspace mode, and the controls only it exposes'
# Mode switching is modify-tier: it rewrites workspace-preferences.json. That
# is fine here -- this is a throwaway workspace -- and it is the only way to
# reach the entries a profile deliberately hides.
foreach ($m in @($commands | Where-Object { $_.label -like 'Mode:*' -and $_.label -notlike 'Mode: Reset*' })) {
    $want = $m.label.Substring('Mode: '.Length)
    $r = Send 'RunCommand' @{ label = $m.label; allow_unsafe = $true }
    if (-not (Ok $r)) { Check "switch to $($m.label)" $false (Err $r); continue }
    Check "switch to '$($m.label)'" ((Settled).mode -eq $want) "mode reported as '$((Settled).mode)'"
    $script:modeMoved = $true
    # The rails and sub-tabs have to survive a rebuild, and the mode-hidden
    # navigation entries have to be pressable somewhere.
    foreach ($rail in $rails) {
        $slug = $rail.id.Substring('rail:'.Length)
        $null = Send 'NavigatePanel' @{ panel = $slug }
        if ((Settled).active_panel -ne $slug) { Check "$($m.label): rail '$slug'" $false 'rail did not select' }
    }
    foreach ($c in $navNow) {
        $rr = Send 'RunCommand' @{ label = $c.label }
        if ((Err $rr) -match 'only available in') { continue }
        if (Ok $rr) { [void]$ran.Add($c.label) } else { Check "$($m.label) : '$($c.label)'" $false (Err $rr) }
    }
    Log "      $($m.label): rails re-selected, navigation swept"
}
$uncovered = @($navNow | Where-Object { -not $ran.Contains($_.label) })
Check 'every navigation command ran in at least one mode' ($uncovered.Count -eq 0) ($uncovered.label -join ', ')

$r = Send 'RunCommand' @{ label = 'Mode: Reset Layout to Default'; allow_unsafe = $true }
Check 'the layout reset command runs' (Ok $r) (Err $r)

# Land back on the profile the sweep started from. The loop above finishes on
# whichever mode it visited last, and pressing the write and execute tiers from
# there makes the mode gate refuse calls that have nothing to do with the mode.
$null = Switch-Mode $startMode
Check "back in '$startMode' before the write tier" ((Settled).mode -eq $startMode) "mode reported as '$((Settled).mode)'"

# ── 11. The write tier, for real ───────────────────────────────────────────
Log '[11] modify-tier commands, run against the throwaway workspace'
if (-not $AllowUnsafe) {
    Note 'modify-tier commands' 'pass -AllowUnsafe to run them; they write files and close tabs'
} else {
    # Everything the sweep has already verified by refusing these; now press
    # them. Execute tier is still not run: see the skips below.
    foreach ($c in @($commands | Where-Object { $_.risk -eq 'modify' -and $_.label -notlike 'Mode:*' -and -not $_.interactive })) {
        $r = Press $c.label $true
        if ((Err $r) -match 'only available in') { Note "'$($c.label)'" "hidden by every mode, even after switching: $(Err $r)"; continue }
        Check "modify '$($c.label)'" (Ok $r) (Err $r)
    }
}

# Execute-tier: only the ones that cannot cost anything get pressed. The rest
# spend model tokens, spawn a compiler, or hand the agent every pending tool
# approval, and none of that is what a UI sweep is for.
Log '[11b] execute tier'
$execSafe = @('Build', 'Run', 'Run Selected Flow', 'Approve All Tools', 'Decline All Tools')
$execCostly = @{
    'Request Inline Suggestion' = 'spends a model call';
    'Plan Sub-Agents'           = 'spends a model call';
    'Refresh Models'            = 'hits the provider over the network';
    'Rollback Deploy'           = 'mutates deployment state';
    'NDA: Open Browser Viewer'  = 'writes a file and raises a browser window'
}
foreach ($c in @($commands | Where-Object { $_.risk -eq 'execute' })) {
    if ($c.label -in $execSafe) {
        $r = Press $c.label $true
        Check "execute '$($c.label)'" (Ok $r) (Err $r)
    } elseif ($execCostly.ContainsKey($c.label)) {
        Note "'$($c.label)'" $execCostly[$c.label]
    } else {
        Note "'$($c.label)'" 'not classified by this sweep'
    }
}

if ($script:modeMoved) {
    $null = Switch-Mode $startMode
    Check "the instance is left in '$startMode'" ((Settled).mode -eq $startMode) "mode reported as '$((Settled).mode)'"
    $script:modeMoved = $false
}

# ── 12. Overlay openers, last ──────────────────────────────────────────────
Log '[12] the quick-open, go-to and find overlays'
# Held to the end on purpose: these raise an in-app overlay and there is no
# bridge call that presses Escape, so the instance is left wearing them and is
# shut down straight afterwards.
foreach ($c in $navOverlay) {
    $r = Press $c.label $false
    if ((Err $r) -match 'only available in') { Note "'$($c.label)'" 'hidden by the current mode'; continue }
    Check "overlay '$($c.label)'" (Ok $r) (Err $r)
}

# ── summary ────────────────────────────────────────────────────────────────
$final = Settled
Log ''
Log "passed $script:pass   failed $script:fail   skipped $script:skip"
Log "ended in mode '$($final.mode)' on rail '$($final.active_panel)' with '$($final.focused_tab)' focused"
Log "commands: $($commands.Count) total, $($nav.Count) navigation, $(@($commands | Where-Object { $_.risk -eq 'modify' }).Count) modify, $(@($commands | Where-Object { $_.risk -eq 'execute' }).Count) execute"
Log "surface: $($rails.Count) rails, $($sections.Count) sub-tabs, $($panels.Count) dock panels, $($docked.Count) tabs cycled, $($commands.Count) palette entries"
if ($script:notes.Count -gt 0) {
    Log ''
    Log 'not pressed, and why:'
    foreach ($n in $script:notes) { Log "  $n" }
}
# Tee-Object in a calling shell holds the report open, and losing the file is
# not a reason to make a clean run look like a failed one.
try {
    Set-Content -Path $Report -Value $script:lines -Encoding UTF8
} catch {
    Log "(could not write $($Report): $($_.Exception.Message))"
}
Write-Host "`nreport: $Report"
if ($script:fail -gt 0) { exit 1 } else { exit 0 }
