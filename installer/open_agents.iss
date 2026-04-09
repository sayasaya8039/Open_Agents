; Open Agents — Inno Setup Script
; Usage: iscc installer/open_agents.iss

#define MyAppName "Open Agents"
#define MyAppVersion "0.4.7"
#define MyAppPublisher "Open Agents"
#define MyAppURL "https://github.com/sayasaya8039/Open_Agents"
#define MyAppExeName "open_agents.exe"

[Setup]
AppId={{E8A3F1D2-4B5C-6D7E-8F9A-0B1C2D3E4F5A}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=OpenAgents-{#MyAppVersion}-setup-win-x64
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#MyAppExeName}
PrivilegesRequired=lowest
SetupIconFile=..\installer\open_agents.ico

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Main binary
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

; llama.cpp — root (CUDA + shared)
Source: "..\third_party\llama.cpp\windows-x64\*.dll"; DestDir: "{app}\third_party\llama.cpp\windows-x64"; Flags: ignoreversion
Source: "..\third_party\llama.cpp\windows-x64\*.exe"; DestDir: "{app}\third_party\llama.cpp\windows-x64"; Flags: ignoreversion
Source: "..\third_party\llama.cpp\windows-x64\manifest.json"; DestDir: "{app}\third_party\llama.cpp\windows-x64"; Flags: ignoreversion

; llama.cpp — CPU
Source: "..\third_party\llama.cpp\windows-x64\cpu\*"; DestDir: "{app}\third_party\llama.cpp\windows-x64\cpu"; Flags: ignoreversion recursesubdirs

; llama.cpp — Vulkan
Source: "..\third_party\llama.cpp\windows-x64\vulkan\*"; DestDir: "{app}\third_party\llama.cpp\windows-x64\vulkan"; Flags: ignoreversion recursesubdirs

; llama.cpp — Upstream
Source: "..\third_party\llama.cpp\windows-x64\upstream\*"; DestDir: "{app}\third_party\llama.cpp\windows-x64\upstream"; Flags: ignoreversion recursesubdirs

; README
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
