; Build: ISCC.exe /DAppVersion=0.1.0 /DSourceExe=..\target\release\milan-pro.exe installer\milan-pro.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\milan-pro.exe"
#endif
; The OFX plug-in host, built beside SourceExe by the same `cargo build --release`.
#ifndef HostExe
  #define HostExe AddBackslash(ExtractFileDir(SourceExe)) + "milan-plugin-host.exe"
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
; Brand: tools/make-icons.py makes these; image areas at 100/125/150/175/200% DPI, Setup picks the closest.
SetupIconFile=..\assets\brand\milan-pro.ico
WizardImageFile=..\assets\brand\installer\wizard-202x386.png,..\assets\brand\installer\wizard-269x515.png,..\assets\brand\installer\wizard-336x643.png,..\assets\brand\installer\wizard-403x772.png,..\assets\brand\installer\wizard-430x824.png
WizardSmallImageFile=..\assets\brand\installer\wizard-small-58.png,..\assets\brand\installer\wizard-small-77.png,..\assets\brand\installer\wizard-small-97.png,..\assets\brand\installer\wizard-small-116.png,..\assets\brand\installer\wizard-small-124.png
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
; MilanPro.exe carries assets/brand/milan-pro.ico as its icon (build.rs), as do the shortcuts below.
UninstallDisplayIcon={app}\MilanPro.exe
UninstallDisplayName=Milan Pro
CloseApplications=yes
; The updater relaunches Milan Pro itself (/RELAUNCH below); Restart Manager restarting it too would open two.
RestartApplications=no

[Messages]
WelcomeLabel1=Welcome to Milan Pro
WelcomeLabel2=Setup will install [name/ver], the open-source video editor, on your computer.%n%nMultitrack editing, effects, titles, audio and rendering, with FFmpeg included so your media opens right away.%n%nClick Next to continue, or Cancel to exit Setup.
FinishedHeadingLabel=Milan Pro is ready
FinishedLabel=Setup has installed [name] on your computer. Start it any time from the Start Menu or its shortcut.
FinishedLabelNoIcons=Setup has installed [name] on your computer.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "MilanPro.exe"; Flags: ignoreversion
; R32: runs OFX plug-ins out of process (MilanPro.exe looks for it in its own folder).
Source: "{#HostExe}"; DestDir: "{app}"; DestName: "milan-plugin-host.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
; FFmpeg sits beside MilanPro.exe because Rust's Command searches the app's own folder before PATH.
Source: "ffmpeg\*"; DestDir: "{app}"; Flags: ignoreversion
; VEGAS scripts for File > Import > VEGAS Project (run from VEGAS's Tools > Scripting > Run Script...).
Source: "..\tools\vegas\*.cs"; DestDir: "{app}\vegas"; Flags: ignoreversion

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

// The updater starts Setup just before Milan Pro closes: give it up to 20 s to exit (main.rs holds the
// "MilanProRunning" mutex until then). Not the AppMutex directive, which aborts a silent Setup instead of
// waiting. Still running after that, CloseApplications deals with it. An interactive Setup shows its own
// "close applications" page, so only a silent one waits.
function InitializeSetup: Boolean;
var
  I: Integer;
begin
  if WizardSilent then
    for I := 1 to 40 do
    begin
      if not CheckForMutexes('MilanProRunning') then
        Break;
      Sleep(500);
    end;
  Result := True;
end;

// A failed or aborted silent update brings back the Milan Pro that started it.
procedure DeinitializeSetup;
var
  Code: Integer;
begin
  if RelaunchRequested and not DidRelaunch then
    ExecAsOriginalUser(ExpandConstant('{param:RELAUNCH|}'), '', '', SW_SHOWNORMAL, ewNoWait, Code);
end;
