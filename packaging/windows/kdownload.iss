; Inno Setup script to package kdownload for Windows.
; Expect the Windows release archive to be extracted to dist\kdownload-<version>-windows-x86_64.

#define MyAppName "kdownload"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "kdownload contributors"
#define MyAppURL "https://github.com/kdownload/kdownload"
#define ReleaseDir "..\\..\\dist\\kdownload-{#MyAppVersion}-windows-x86_64"
#define OutputDir "..\\..\\dist"

[Setup]
AppId={{8A960A11-A159-4A95-851D-3D108713B95F}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
UninstallDisplayIcon={app}\kdownload.exe
LicenseFile={#ReleaseDir}\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename={#MyAppName}-{#MyAppVersion}-windows-installer
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
ChangesEnvironment=yes
DisableProgramGroupPage=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#ReleaseDir}\\kdownload.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\\{#MyAppName}\\README"; Filename: "{app}\\README.md"
Name: "{autoprograms}\\{#MyAppName}\\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\\kdownload"; Filename: "{cmd}"; Parameters: "/K \"\"""{app}\\kdownload.exe"" --help\"""; WorkingDir: "{app}"; Tasks: desktopicon; Comment: "Open a Command Prompt with kdownload help"

[Registry]
Root: HKLM; Subkey: "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Flags: preservestringtype; Check: NeedsAddPath('{app}')

[Run]
Filename: "{cmd}"; Parameters: "/K \"\"""{app}\\kdownload.exe"" --version\"""; Description: "Verify kdownload installation"; Flags: nowait postinstall skipifsilent unchecked

[Code]
function NeedsAddPath(AppDir: string): Boolean;
var
  ExistingPath: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE,
    'SYSTEM\CurrentControlSet\Control\Session Manager\Environment',
    'Path', ExistingPath) then
  begin
    Result := True;
    exit;
  end;

  AppDir := UpperCase(AppDir);
  ExistingPath := ';' + UpperCase(ExistingPath) + ';';
  Result := Pos(';' + AppDir + ';', ExistingPath) = 0;
end;
