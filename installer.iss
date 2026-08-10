; ─────────────────────────────────────────────────────────────────────────
; V.E.L.O.C.I.T.Y. — Inno Setup Installer
; ─────────────────────────────────────────────────────────────────────────
; Build with:  ISCC.exe /DMyAppVersion=0.1.0 installer.iss
; Or simply:   .\build_release.ps1
; ─────────────────────────────────────────────────────────────────────────

#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif

#define MyAppName       "V.E.L.O.C.I.T.Y."
#define MyAppFullName   "V.E.L.O.C.I.T.Y. Cognitive IDE"
#define MyAppPublisher  "UnitBuilds-CC"
#define MyAppURL        "https://github.com/UnitBuilds/Kimi-Code"
#define MyAppExeName    "velocity_ide.exe"

[Setup]
AppId={{A7B3C9D1-4E5F-6A7B-8C9D-0E1F2A3B4C5D}
AppName={#MyAppFullName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppFullName}
LicenseFile=LICENSE
OutputDir=output
OutputBaseFilename=VELOCITY-{#MyAppVersion}-Setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\velocity_ide.exe
SetupIconFile=compiler:SetupClassicIcon.ico

; Minimum Windows 10
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addtopath"; Description: "Add to system PATH"; GroupDescription: "Environment:"

[Files]
; Main binaries
Source: "dist\bin\velocity_mcp.exe";      DestDir: "{app}\bin"; Flags: ignoreversion
Source: "dist\bin\velocity_ide.exe";      DestDir: "{app}\bin"; Flags: ignoreversion
Source: "dist\bin\velocity-drone.exe";    DestDir: "{app}\bin"; Flags: ignoreversion
Source: "dist\bin\run_nda.exe";           DestDir: "{app}\bin"; Flags: ignoreversion

; Documentation
Source: "dist\LICENSE.txt";  DestDir: "{app}"; Flags: ignoreversion
Source: "dist\README.md";    DestDir: "{app}"; Flags: ignoreversion

[Icons]
; IDE Chat — opens a terminal with the interactive chat session
Name: "{group}\{#MyAppFullName}"; Filename: "cmd.exe"; Parameters: "/k ""{app}\bin\{#MyAppExeName}"" chat"; WorkingDir: "{app}\bin"
; MCP Server — stdio JSON-RPC server (used by IDEs/editors, not launched directly)
Name: "{group}\VELOCITY. MCP Server"; Filename: "cmd.exe"; Parameters: "/k ""{app}\bin\velocity_mcp.exe"""; WorkingDir: "{app}\bin"
Name: "{group}\{cm:UninstallProgram,{#MyAppFullName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppFullName}"; Filename: "cmd.exe"; Parameters: "/k ""{app}\bin\{#MyAppExeName}"" chat"; WorkingDir: "{app}\bin"; Tasks: desktopicon

[Run]
Filename: "cmd.exe"; Parameters: "/k ""{app}\bin\{#MyAppExeName}"" chat"; WorkingDir: "{app}\bin"; Description: "Launch {#MyAppFullName} Chat"; Flags: postinstall skipifsilent

[Registry]
; Store install path for other tools to discover
Root: HKLM; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}"; ValueType: string; ValueName: "InstallDir"; ValueData: "{app}"
Root: HKLM; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}"; ValueType: string; ValueName: "Version"; ValueData: "{#MyAppVersion}"

[Code]
// ── Set system environment variables on install ───────────────────────
// These are written to the actual system Environment registry key so
// std::env::var() in the Rust binaries picks them up.
const
  EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

procedure SetEnvVars();
var
  BinDir: string;
  SandboxDll: string;
begin
  BinDir := ExpandConstant('{app}\bin');
  SandboxDll := ExpandConstant('{commonappdata}\WUIAS\wuias_shield.dll');

  // VELOCITY_MCP_SERVER — point to the installed MCP server
  RegWriteStringValue(HKLM, EnvironmentKey, 'VELOCITY_MCP_SERVER', BinDir + '\velocity_mcp.exe');

  // WUIAS_SHIELD_DLL — default sandbox DLL location
  RegWriteStringValue(HKLM, EnvironmentKey, 'WUIAS_SHIELD_DLL', SandboxDll);

  // Notify running processes (Explorer, taskbar, etc.) of the change
  // Broadcast is done in CurStepChanged after all env/PATH writes complete.
end;

// ── Remove environment variables on uninstall ─────────────────────────
procedure RemoveEnvVars();
begin
  RegDeleteValue(HKLM, EnvironmentKey, 'VELOCITY_MCP_SERVER');
  RegDeleteValue(HKLM, EnvironmentKey, 'WUIAS_SHIELD_DLL');
  // Broadcast is done in CurUninstallStepChanged after all env/PATH writes.
end;

// ── Notify system of environment changes ──────────────────────────────
// Broadcasts WM_SETTINGCHANGE so running apps (Explorer, terminals) pick
// up the new environment variables immediately.
function SendMessageTimeout(hWnd: LongInt; Msg: Cardinal; wParam: LongInt;
  lParam: String; fuFlags: Cardinal; uTimeout: Cardinal; var lpdwResult: LongInt): LongInt;
  external 'SendMessageTimeoutW@user32.dll stdcall';

procedure SendBroadcastMessage();
var
  dwResult: LongInt;
begin
  SendMessageTimeout($FFFF, $001A, 0, 'Environment', 2, 5000, dwResult);
end;

// ── Add bin directory to system PATH ──────────────────────────────────
procedure AddToPath();
var
  CurrentPath: string;
  BinDir: string;
begin
  BinDir := ExpandConstant('{app}\bin');
  if RegQueryStringValue(HKLM, EnvironmentKey,
    'Path', CurrentPath) then
  begin
    if Pos(Uppercase(BinDir), Uppercase(CurrentPath)) = 0 then
    begin
      if CurrentPath <> '' then
        CurrentPath := CurrentPath + ';';
      CurrentPath := CurrentPath + BinDir;
      RegWriteStringValue(HKLM, EnvironmentKey,
        'Path', CurrentPath);
    end;
  end;
end;

// ── Remove bin directory from PATH on uninstall ───────────────────────
procedure RemoveFromPath();
var
  CurrentPath: string;
  BinDir: string;
  NewPath: string;
  Pos1, Pos2: Integer;
begin
  BinDir := ExpandConstant('{app}\bin');
  if RegQueryStringValue(HKLM, EnvironmentKey,
    'Path', CurrentPath) then
  begin
    Pos1 := Pos(Uppercase(BinDir), Uppercase(CurrentPath));
    if Pos1 > 0 then
    begin
      // Find the start of this entry (after previous semicolon or start)
      Pos2 := Pos1;
      while (Pos2 > 1) and (Copy(CurrentPath, Pos2 - 1, 1) <> ';') do
        Dec(Pos2);

      // Find the end (next semicolon or end)
      Pos1 := Pos1 + Length(BinDir);
      while (Pos1 <= Length(CurrentPath)) and (Copy(CurrentPath, Pos1, 1) <> ';') do
        Inc(Pos1);
      if Pos1 <= Length(CurrentPath) then
        Inc(Pos1); // skip the semicolon

      NewPath := Copy(CurrentPath, 1, Pos2 - 1) + Copy(CurrentPath, Pos1, MaxInt);
      RegWriteStringValue(HKLM, EnvironmentKey,
        'Path', NewPath);
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    SetEnvVars();
    if WizardIsTaskSelected('addtopath') then
      AddToPath();
    // Broadcast once after all env + PATH changes
    SendBroadcastMessage();
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RemoveEnvVars();
    RemoveFromPath();
    // Broadcast once after all env + PATH cleanup
    SendBroadcastMessage();
    // Clean up app registry keys
    RegDeleteKeyIncludingSubkeys(HKLM, 'Software\{#MyAppPublisher}\{#MyAppName}');
  end;
end;
