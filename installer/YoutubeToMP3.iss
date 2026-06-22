#define MyAppName "YouTube to MP3"
#define MyAppPublisher "jeremygold02"
#define MyAppURL "https://github.com/jeremygold02/youtube-to-mp3"
#define MyAppUserModelID "YoutubeToMP3.PyQt"

#ifndef MyAppVersion
#define MyAppVersion "0.1.0"
#endif

[Setup]
AppId={{74758192-9DAC-49D3-9B6D-056219884608}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases/latest
UninstallDisplayName={#MyAppName}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=output
OutputBaseFilename=YouTube to MP3 Setup
SetupIconFile=..\icon.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\icon.ico

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startmenuicon"; Description: "Create a Start Menu shortcut"; GroupDescription: "{cm:AdditionalIcons}"; Flags: checkedonce
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\dist\YouTube to MP3.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
Type: files; Name: "{autoprograms}\{#MyAppName}.lnk"
Type: files; Name: "{autodesktop}\{#MyAppName}.lnk"

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\YouTube to MP3.exe"; WorkingDir: "{app}"; IconFilename: "{app}\icon.ico"; AppUserModelID: "{#MyAppUserModelID}"; Tasks: startmenuicon
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\YouTube to MP3.exe"; WorkingDir: "{app}"; IconFilename: "{app}\icon.ico"; AppUserModelID: "{#MyAppUserModelID}"; Tasks: desktopicon

[Run]
Filename: "{autoprograms}\{#MyAppName}.lnk"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: shellexec nowait postinstall skipifsilent; Check: WizardIsTaskSelected('startmenuicon')
Filename: "{app}\YouTube to MP3.exe"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent; Check: not WizardIsTaskSelected('startmenuicon')
