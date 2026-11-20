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

[Registry]
; BlackHole as a browser of this user (HKA = HKCU for a per-user install): Windows lists it in
; Settings > Apps > Default apps, where the person makes it the default (the welcome tour and
; Settings > General open that page: ms-settings:defaultapps?registeredAppUser=BlackHole).
; Links come in as `BlackHole.exe --open <url>`.
Root: HKA; Subkey: "Software\Classes\BlackHoleURL"; ValueType: string; ValueName: ""; ValueData: "BlackHole URL"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\BlackHoleURL"; ValueType: string; ValueName: "FriendlyTypeName"; ValueData: "BlackHole URL"
Root: HKA; Subkey: "Software\Classes\BlackHoleURL\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\BlackHole.exe,0"
Root: HKA; Subkey: "Software\Classes\BlackHoleURL\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\BlackHole.exe"" --open ""%1"""
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole"; ValueType: string; ValueName: ""; ValueData: "BlackHole"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\BlackHole.exe,0"
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\BlackHole.exe"""
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "BlackHole"
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole\Capabilities"; ValueType: string; ValueName: "ApplicationIcon"; ValueData: "{app}\BlackHole.exe,0"
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "A fast, elegant browser that respects your privacy."
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole\Capabilities\StartMenu"; ValueType: string; ValueName: "StartMenuInternet"; ValueData: "BlackHole"
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole\Capabilities\URLAssociations"; ValueType: string; ValueName: "http"; ValueData: "BlackHoleURL"
Root: HKA; Subkey: "Software\Clients\StartMenuInternet\BlackHole\Capabilities\URLAssociations"; ValueType: string; ValueName: "https"; ValueData: "BlackHoleURL"
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "BlackHole"; ValueData: "Software\Clients\StartMenuInternet\BlackHole\Capabilities"; Flags: uninsdeletevalue

[Run]
; Interactive installs offer to launch the app; silent runs (self-update) relaunch it directly.
Filename: "{app}\BlackHole.exe"; Description: "{cm:LaunchProgram,BlackHole}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\BlackHole.exe"; Flags: nowait runasoriginaluser; Check: WizardSilent
