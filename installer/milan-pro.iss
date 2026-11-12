; Build: ISCC.exe /DAppVersion=0.1.0 /DSourceExe=..\target\release\milan-pro.exe installer\milan-pro.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\milan-pro.exe"
#endif

[Setup]
; AppId must never change, or upgrades install side by side instead of replacing.
AppId={{308B10A3-8CA9-42C9-9089-E4BF9AE88DD3}
AppName=Milan Pro
AppVersion={#AppVersion}
AppVerName=Milan Pro {#AppVersion}
AppPublisher=Milan Pro Contributors
AppPublisherURL=https://github.com/knivesbatz/milan-pro
AppSupportURL=https://github.com/knivesbatz/milan-pro/issues
AppUpdatesURL=https://github.com/knivesbatz/milan-pro/releases
DefaultDirName={autopf}\Milan Pro
DefaultGroupName=Milan Pro
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
OutputDir=..\release
OutputBaseFilename=MilanPro-{#AppVersion}-Setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UninstallDisplayIcon={app}\MilanPro.exe
UninstallDisplayName=Milan Pro
CloseApplications=yes
; The updater relaunches Milan Pro itself (/RELAUNCH below); Restart Manager restarting it too would open two.
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "MilanPro.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
; FFmpeg sits beside MilanPro.exe because Rust's Command searches the app's own folder before PATH.
Source: "ffmpeg\*"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Milan Pro"; Filename: "{app}\MilanPro.exe"
Name: "{autodesktop}\Milan Pro"; Filename: "{app}\MilanPro.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\MilanPro.exe"; Description: "{cm:LaunchProgram,Milan Pro}"; Flags: nowait postinstall skipifsilent
; In-app updater: "Setup.exe /SILENT ... /RELAUNCH=<running MilanPro.exe>" starts the updated app when done.
Filename: "{app}\MilanPro.exe"; Flags: nowait runasoriginaluser; Check: RelaunchRequested; AfterInstall: Relaunched

[Code]
var
  DidRelaunch: Boolean;

function RelaunchRequested: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:RELAUNCH|}') <> '');
end;

procedure Relaunched;
begin
  DidRelaunch := True;
end;

// A failed or aborted silent update brings back the Milan Pro that started it.
procedure DeinitializeSetup;
var
  Code: Integer;
begin
  if RelaunchRequested and not DidRelaunch then
    ExecAsOriginalUser(ExpandConstant('{param:RELAUNCH|}'), '', '', SW_SHOWNORMAL, ewNoWait, Code);
end;
