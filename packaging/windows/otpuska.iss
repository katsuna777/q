; Inno Setup script: per-user installer for Windows (no administrator rights needed).
; Build: ISCC /DAppVersion=1.0.0 packaging\windows\otpuska.iss   (output goes to dist\)
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F0B0C4E-6C0A-4B7B-9D0F-0A7E3C6B5D21}
AppName=Отпуска
AppVersion={#AppVersion}
AppPublisher=katsuna777
AppPublisherURL=https://github.com/katsuna777/q
DefaultDirName={autopf}\Otpuska
DefaultGroupName=Отпуска
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename=Otpuska-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\crates\otpuska-gui\assets\icon.ico
UninstallDisplayIcon={app}\Otpuska.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "Создать значок на рабочем столе"; GroupDescription: "Дополнительно:"; Flags: unchecked

[Files]
Source: "..\..\target\release\otpuska.exe"; DestDir: "{app}"; DestName: "Otpuska.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Отпуска"; Filename: "{app}\Otpuska.exe"
Name: "{autodesktop}\Отпуска"; Filename: "{app}\Otpuska.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Otpuska.exe"; Description: "Запустить «Отпуска»"; Flags: nowait postinstall skipifsilent

; The database lives in %LOCALAPPDATA%\Otpuska and is deliberately left in place on uninstall.
