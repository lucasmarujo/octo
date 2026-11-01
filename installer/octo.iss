#ifndef Binary
  #define Binary "..\target\release\octo.exe"
#endif
#define AppExe "Octo.exe"
#define AppVersion GetStringFileInfo(Binary, "ProductVersion")
#if AppVersion == ""
  #error The binary has no ProductVersion: build it with tauri-build
#endif

[Setup]
AppId={{4790510D-69A3-4053-BB86-2D7AEA02DAAC}
AppName=Octo
AppVersion={#AppVersion}
AppPublisher=OctoAI
AppPublisherURL=https://octo.marujo.dev
AppSupportURL=https://github.com/lucasmarujo/octo/issues
AppUpdatesURL=https://github.com/lucasmarujo/octo/releases/latest
DefaultDirName={autopf}\Octo
PrivilegesRequired=lowest
DisableProgramGroupPage=yes
ShowLanguageDialog=auto
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=no
SetupIconFile=..\desktop\icons\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName=Octo
OutputDir=..\target\installer
OutputBaseFilename=Octo-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
VersionInfoVersion={#GetVersionNumbersString(Binary)}
VersionInfoTextVersion={#AppVersion}
VersionInfoProductTextVersion={#AppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Octo"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\Octo"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,Octo}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\{#AppExe}.old-*"
Type: files; Name: "{app}\.{#AppExe}.update-*"

[Code]
var
  PreviousBinary: String;

function BinaryPath: String;
begin
  Result := ExpandConstant('{app}\{#AppExe}');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssInstall) and FileExists(BinaryPath) then
  begin
    PreviousBinary := BinaryPath + '.old-setup-' + GetDateTimeString('yyyymmddhhnnsszzz', #0, #0);
    if not RenameFile(BinaryPath, PreviousBinary) then
      PreviousBinary := '';
  end
  else if CurStep = ssPostInstall then
  begin
    if PreviousBinary <> '' then
      DeleteFile(PreviousBinary);
    PreviousBinary := '';
  end;
end;

procedure DeinitializeSetup;
begin
  if (PreviousBinary <> '') and not FileExists(BinaryPath) then
    RenameFile(PreviousBinary, BinaryPath);
end;
