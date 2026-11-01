; Inno Setup script — compiled by .github/workflows/windows-release.yml
; Usage: iscc /DAppVersion=0.1.0 /DExePath=..\target\release\blackhole.exe windows-installer.iss
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef ExePath
  #define ExePath "..\target\release\blackhole.exe"
#endif

[Setup]
AppId={{B1ACC401-E000-4B1A-9C0D-0B1AC4401E00}
AppName=BlackHole
AppVersion={#AppVersion}
AppPublisher=BlackHole
DefaultDirName={autopf}\BlackHole
DefaultGroupName=BlackHole
UninstallDisplayIcon={app}\BlackHole.exe
SetupIconFile=..\crates\app\assets\blackhole.ico
OutputDir=..\dist
OutputBaseFilename=BlackHole-Setup-{#AppVersion}-x64
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
CloseApplications=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "BlackHole.exe"; Flags: ignoreversion

[Icons]
Name: "{group}\BlackHole"; Filename: "{app}\BlackHole.exe"
Name: "{autodesktop}\BlackHole"; Filename: "{app}\BlackHole.exe"; Tasks: desktopicon

[Run]
; Interactive installs offer to launch the app; silent runs (self-update) relaunch it directly.
Filename: "{app}\BlackHole.exe"; Description: "{cm:LaunchProgram,BlackHole}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\BlackHole.exe"; Flags: nowait runasoriginaluser; Check: WizardSilent
