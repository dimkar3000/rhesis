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

[Icons]
Name: "{autoprograms}\Rhesis"; Filename: "{app}\rhesis.exe"; IconFilename: "{app}\rhesis.ico"
Name: "{autodesktop}\Rhesis"; Filename: "{app}\rhesis.exe"; IconFilename: "{app}\rhesis.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\rhesis.exe"; Description: "{cm:LaunchProgram,Rhesis}"; Flags: nowait postinstall skipifsilent
