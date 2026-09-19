; Inno Setup Script for Hawk Terminal
; Architecture: Windows x86_64
; Output: Single executable setup file

#define MyAppName "Hawk Terminal"
#define MyAppVersion "0.9.3"
#define MyAppPublisher "Breeze"
#define MyAppURL "https://breeze141all.github.io/hawk-site/"
#define MyAppExeName "hawk-terminal.exe"

[Setup]
AppId={{C78923F4-1188-4D5A-88D8-92FE5F5868E1}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={localappdata}\Programs\HawkTerminal
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
OutputDir=target\installer
OutputBaseFilename=hawk-terminal-x86_64-setup
SetupIconFile=assets\icon.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\{#MyAppExeName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "assets\icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "data\src\default_state.json"; DestDir: "{userappdata}\hawk-terminal"; DestName: "saved-state.json"; Flags: onlyifdoesntexist

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\icon.ico"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
