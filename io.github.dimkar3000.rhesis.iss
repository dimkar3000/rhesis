; Inno Setup script for the Windows installer, driven by scripts/build-windows.sh
; (it passes the /D defines below). Installs per user by default; the wizard
; offers an all-users install when run elevated.

#ifndef AppVersion
  #error AppVersion is required (scripts/build-windows.sh passes it)
#endif
#ifndef NumericVersion
  #define NumericVersion "0.0.0"
#endif
#ifndef SourceDir
  #error SourceDir is required (the staged Rhesis folder)
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif
#ifndef OutputBaseFilename
  #define OutputBaseFilename "rhesis-setup"
#endif
#ifndef VcRedistDir
  #error VcRedistDir is required (the fetched VC++ runtime dir; scripts/build-windows.sh passes it)
#endif

[Setup]
AppId={{6D0C3B1E-7A52-4C8B-9E1D-5B2F8A4C7E93}
AppName=Rhesis
AppVersion={#AppVersion}
AppPublisher=dimkar
AppPublisherURL=https://github.com/dimkar3000/rhesis
AppSupportURL=https://github.com/dimkar3000/rhesis/issues
VersionInfoVersion={#NumericVersion}
DefaultDirName={autopf}\Rhesis
DefaultGroupName=Rhesis
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
LicenseFile={#SourceDir}\License.md
SetupIconFile={#SourceDir}\rhesis.ico
UninstallDisplayIcon={app}\rhesis.ico
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion
; Extracted only when the chained runtime install will run (deleteafterinstall
; cleans it up afterwards). See the [Code] section below.
Source: "{#VcRedistDir}\vc_redist.x64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall; Check: VCRedistNeeded

[Icons]
Name: "{autoprograms}\Rhesis"; Filename: "{app}\rhesis.exe"; IconFilename: "{app}\rhesis.ico"
Name: "{autodesktop}\Rhesis"; Filename: "{app}\rhesis.exe"; IconFilename: "{app}\rhesis.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\rhesis.exe"; Description: "{cm:LaunchProgram,Rhesis}"; Flags: nowait postinstall skipifsilent

[Code]
// ------------------------------------------------------------------
// Chained installation of the VC++ 2015-2022 runtime (x64), fetched by
// scripts/build-windows.sh from https://aka.ms/vs/17/release/vc_redist.x64.exe.
// A fresh Windows system can ship without the runtime, in which case Rhesis
// refuses to start ("MSVCP140.dll is missing"); the installer fills that gap
// right before offering to launch the app.
// ------------------------------------------------------------------

const
  // Version info of the runtime's Runtimes key: HKLM\...\Runtimes\x64 (the
  // 64-bit registry view, which the registry functions use in 64-bit mode).
  VCRedistKeyPath = 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64';
  // Chained when the registered runtime is missing or older than this floor.
  // The floor is the oldest v143 runtime known to load the v143-built exe;
  // vc_redist upgrades older installs itself and reports a newer one politely
  // (exit code 1638), so a floor oddly higher than the fetched bundle stays
  // harmless.
  VCMinimumVersion = '14.40.33810';

{ Parsed off VersionText one dotted field at a time; 0 once exhausted. }
function ExtractVersionField(var VersionText: String): Integer;
var
  DotPos: Integer;
begin
  DotPos := Pos('.', VersionText);
  if DotPos = 0 then
  begin
    if VersionText = '' then
      Result := 0
    else begin
      Result := StrToIntDef(VersionText, 0);
      VersionText := '';
    end;
  end
  else
  begin
    Result := StrToIntDef(Copy(VersionText, 1, DotPos - 1), 0);
    Delete(VersionText, 1, DotPos);
  end;
end;

{ Dotted-decimal compare: True when Version >= Minimum ('v14.44.… >= 14.40.0'). }
function VCVersionAtLeast(const Version, Minimum: String): Boolean;
var
  V, M: String;
  VField, MField, I: Integer;
begin
  V := Version;
  if (Length(V) > 0) and (V[1] = 'v') then
    Delete(V, 1, 1);
  M := Minimum;
  for I := 1 to 4 do
  begin
    VField := ExtractVersionField(V);
    MField := ExtractVersionField(M);
    if VField <> MField then
    begin
      Result := VField > MField;
      Exit;
    end;
  end;
  Result := True;
end;

{ The registered runtime version ('v14.44.35211.00'), or '' when nothing
  2015-2022 is installed. }
function VCRedistVersion: String;
begin
  if not RegQueryStringValue(HKLM, VCRedistKeyPath, 'Version', Result) then
    Result := '';
end;

{ True when the runtime is absent entirely or below the floor above — both
  call for the chained install. }
function VCRedistNeeded: Boolean;
begin
  Result := (VCRedistVersion = '') or
    (not VCVersionAtLeast(VCRedistVersion, VCMinimumVersion));
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
begin
  if (CurStep <> ssPostInstall) or (not VCRedistNeeded) then
    Exit;

  // vc_redist requires elevation: 'runas' raises the UAC prompt on a
  // per-user install and is a no-op when the wizard already runs elevated.
  if not ShellExec('runas', ExpandConstant('{tmp}\vc_redist.x64.exe'),
     '/install /passive /norestart /log "' +
     ExpandConstant('{tmp}\vc_redist.log') + '"',
     '', SW_SHOWNORMAL, ewWaitUntilTerminated, ResultCode) then
  begin
    MsgBox('The Visual C++ runtime installer could not be run. Rhesis will ' +
      'not start until the runtime is installed manually:' + #13#10 +
      'https://aka.ms/vs/17/release/vc_redist.x64.exe', mbError, MB_OK);
    Exit;
  end;

  case ResultCode of
    0, 1638:
      // 0: installed; 1638: a newer version is already present (defensive,
      // see VCMinimumVersion above) — nothing to complain about.
      begin end;
    3010, 1641:
      // The runtime is installed and live; only the reboot is pending.
      MsgBox('The Visual C++ runtime was installed. A system restart is ' +
        'recommended before launching Rhesis.', mbInformation, MB_OK);
  else
    MsgBox(Format(
      'Installing the Visual C++ runtime failed with exit code %d. Rhesis may ' +
      'not start until the runtime is installed manually:' + #13#10 +
      'https://aka.ms/vs/17/release/vc_redist.x64.exe', [ResultCode]),
      mbError, MB_OK);
  end;
end;
