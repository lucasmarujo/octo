#ifndef Binary
  #define Binary "..\target\release\orkai.exe"
#endif
#define AppExe "orkai.exe"
#define AppVersion GetStringFileInfo(Binary, "ProductVersion")
#if AppVersion == ""
  #error The binary has no ProductVersion: build it with build.rs
#endif

[Setup]
AppId={{F90DC11F-64BF-418E-BF55-59C746EC206F}
AppName=orkai
AppVersion={#AppVersion}
AppPublisher=Lucas Marujo
AppPublisherURL=https://orkai.marujo.dev
AppSupportURL=https://github.com/lucasmarujo/orkai-cli/issues
AppUpdatesURL=https://github.com/lucasmarujo/orkai-cli/releases/latest
DefaultDirName={autopf}\orkai
PrivilegesRequired=lowest
DisableProgramGroupPage=yes
ShowLanguageDialog=auto
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
CloseApplications=no
SetupIconFile=..\assets\orkai.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName=orkai
OutputDir=..\target\installer
OutputBaseFilename=orkai-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
VersionInfoVersion={#GetVersionNumbersString(Binary)}
VersionInfoTextVersion={#AppVersion}
VersionInfoProductTextVersion={#AppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[CustomMessages]
english.AddToPath=Add orkai to the PATH, to run it from any terminal
brazilianportuguese.AddToPath=Adicionar o orkai ao PATH, para rodar em qualquer terminal

[Tasks]
Name: "addtopath"; Description: "{cm:AddToPath}"

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[UninstallDelete]
Type: files; Name: "{app}\{#AppExe}.old-*"
Type: files; Name: "{app}\.{#AppExe}.update-*"

[Code]
const
  EnvironmentKey = 'Environment';
  PathValue = 'Path';

var
  PreviousBinary: String;

function BinaryPath: String;
begin
  Result := ExpandConstant('{app}\{#AppExe}');
end;

function SamePath(Entry, Dir: String): Boolean;
begin
  Result := CompareText(RemoveBackslashUnlessRoot(Trim(Entry)), RemoveBackslashUnlessRoot(Dir)) = 0;
end;

function PathWithout(Path, Dir: String; var Found: Boolean): String;
var
  Rest, Entry: String;
  Split: Integer;
begin
  Result := '';
  Found := False;
  Rest := Path;
  while Rest <> '' do
  begin
    Split := Pos(';', Rest);
    if Split = 0 then
      Split := Length(Rest) + 1;
    Entry := Copy(Rest, 1, Split - 1);
    Delete(Rest, 1, Split);
    if SamePath(Entry, Dir) then
      Found := True
    else if Entry <> '' then
    begin
      if Result <> '' then
        Result := Result + ';';
      Result := Result + Entry;
    end;
  end;
end;

procedure SetPathEntry(Dir: String; Present: Boolean);
var
  Path, Updated: String;
  Found: Boolean;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, PathValue, Path) then
    Path := '';
  Updated := PathWithout(Path, Dir, Found);
  if Present and not Found then
  begin
    if (Path <> '') and (Path[Length(Path)] <> ';') then
      Path := Path + ';';
    RegWriteExpandStringValue(HKCU, EnvironmentKey, PathValue, Path + Dir);
  end
  else if not Present and Found then
  begin
    if Updated = '' then
      RegDeleteValue(HKCU, EnvironmentKey, PathValue)
    else
      RegWriteExpandStringValue(HKCU, EnvironmentKey, PathValue, Updated);
  end;
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
    SetPathEntry(ExpandConstant('{app}'), WizardIsTaskSelected('addtopath'));
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

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    SetPathEntry(ExpandConstant('{app}'), False);
end;
