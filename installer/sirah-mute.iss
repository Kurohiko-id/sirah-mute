; Sirah Mute per-user installer (Inno Setup 6).
; Build:  ISCC /DMyAppVersion=2.0.0 /DExePath=<path\to\sirahmute.exe> installer\sirah-mute.iss
; Output: installer\Output\SirahMute-Setup-<version>.exe
;
; The app manages its own autostart (HKCU Run value "SirahMute", and it removes the v1 value
; "MuteMic"), so the installer only cleans the Run value up on uninstall.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef ExePath
  #define ExePath "..\kuromute\target\release\sirahmute.exe"
#endif

#define MyAppName "Sirah Mute"
#define MyAppExeName "sirahmute.exe"
#define MyAppURL "https://github.com/Kurohiko-id/sirah-mute"
; AppId of the v1 "Kuro MuteMic" installer, only used to suggest uninstalling it.
#define LegacyAppId "{6A9F4C1E-3B7A-4E9E-9A2C-9F1C8B7D2A11}"

[Setup]
; New AppId on purpose: v1 could be installed per-machine, so no in-place upgrade from it.
AppId={{B131B4B2-6950-40F6-AD60-8683270943B1}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher=Kurohiko
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\Programs\SirahMute
DisableDirPage=auto
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
SetupIconFile=..\kuromute\assets\sirah.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
Compression=lzma2
SolidCompression=yes
OutputDir=Output
OutputBaseFilename=SirahMute-Setup-{#MyAppVersion}
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autoprograms}\{#MyAppName} Settings"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--settings"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Start {#MyAppName}"; Flags: nowait postinstall

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Code]
procedure Kill(const Args: String);
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), Args, '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure StopSirah;
begin
  { No /F first: the core closes cleanly on WM_CLOSE. Force only if it is still there. }
  Kill('/IM sirahmute.exe');
  Sleep(1500);
  Kill('/F /IM sirahmute.exe');
end;

{ Stop everything that would fight Sirah Mute: an older Sirah Mute, both v1 editions and the
  Super Lite PowerShell watcher (its AutoRevert would ping-pong with ours). }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  PidFile: String;
  Pid: AnsiString;
begin
  StopSirah;
  Kill('/F /IM "Kuro MuteMic.exe"');
  Kill('/F /IM "Kuro MuteMic Lite.exe"');
  PidFile := ExpandConstant('{%TEMP}\KuroForceAudio.pid');
  if LoadStringFromFile(PidFile, Pid) then
  begin
    Pid := Trim(Pid);
    { The image-name filter guards against a reused PID belonging to something else. }
    if Pid <> '' then
      Kill('/F /FI "PID eq ' + String(Pid) + '" /FI "IMAGENAME eq powershell.exe"');
    DeleteFile(PidFile);
  end;
  Result := '';
end;

function LegacyInstalled: Boolean;
var
  Key: String;
begin
  Key := 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#LegacyAppId}_is1';
  Result := RegKeyExists(HKCU, Key) or RegKeyExists(HKLM, Key);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssDone) and LegacyInstalled and not WizardSilent then
    MsgBox('Sirah Mute replaces Kuro MuteMic. It takes over the old settings and autostart the first time it runs.' + #13#10 + #13#10 +
      'You can uninstall the old "Kuro MuteMic" from Settings > Apps.', mbInformation, MB_OK);
end;

function InitializeUninstall: Boolean;
begin
  StopSirah;
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  { Settings in %APPDATA%\SirahMute are kept on purpose (reinstall keeps your setup). }
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'SirahMute');
end;
