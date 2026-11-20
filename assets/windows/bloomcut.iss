; SPDX-License-Identifier: AGPL-3.0-or-later
; SPDX-FileCopyrightText: 2026 Concat contributors
;
; BloomCut's installer: the app, plus what a
; user would otherwise have to fetch or be sent - the transcriber and
; voice models, and the AI assistant's rules - each put where the app looks
; for it, so the app works the moment it opens, offline included.
;
;   ISCC /DVersion=0.2.11-team.2 /DStage=<app folder> /DModels=<models>
;        /DRules=<rules folder> /DOut=<output folder> bloomcut.iss
;
; Stage:  BloomCut.exe and the DLLs beside it.
; Models: a folder with whisper-models\ and tts-models\, as in the app's
;         data folder.
; Rules:  the *.md files for Settings > AI assistant > Rules.
;
; Per user, no administrator: the models go to the user's own
; %LOCALAPPDATA%\{#DataId} and the rules to %APPDATA%, where the
; app reads them; a machine-wide install would have no single user's
; folders to put them in.

#ifndef Version
  #error Version is required
#endif
#ifndef Stage
  #error Stage is required: the app folder to install
#endif
#ifndef Models
  #error Models is required: the folder holding whisper-models and tts-models
#endif
#ifndef Rules
  #error Rules is required: the folder of the assistant's rule files
#endif
#ifndef Out
  #define Out "."
#endif
; The edition (concat_host::brand): BloomCut unless told otherwise. The
; school's edition passes /DAppName=AI自动剪辑工具 /DExeName=AI自动剪辑工具.exe
; /DDataId=app.aiclip.editor /DSetupName=AI-Clip-Setup /DAppGuid=... /DSite=...
; /DAdopt=0.
#ifndef AppName
  #define AppName "BloomCut"
#endif
#ifndef ExeName
  #define ExeName "BloomCut.exe"
#endif
#ifndef DataId
  #define DataId "app.bloomcut.editor"
#endif
#ifndef SetupName
  #define SetupName "BloomCut-Setup"
#endif
#ifndef AppGuid
  #define AppGuid "5C2E9A41-7D3B-4E8F-A1C6-93B0D4E7F2A8"
#endif
#ifndef Site
  #define Site "https://github.com/jasper-julian/bloomcut"
#endif
#ifndef Adopt
  #define Adopt "1"
#endif

[Setup]
; Its own id, so it neither upgrades nor is upgraded by the upstream app.
AppId={{{#AppGuid}}
AppName={#AppName}
AppVersion={#Version}
AppVerName={#AppName} {#Version}
AppPublisher={#AppName}
AppPublisherURL={#Site}
AppSupportURL={#Site}
AppUpdatesURL={#Site}
DefaultDirName={autopf}\{#AppName}
; Not the folder an earlier build under another name went to.
UsePreviousAppDir=no
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir={#Out}
OutputBaseFilename={#SetupName}
SetupIconFile=..\icons\concat.ico
UninstallDisplayIcon={app}\app.ico
UninstallDisplayName={#AppName}
; The models are already compressed; a solid archive of them costs minutes
; for little.
Compression=lzma2/fast
SolidCompression=no
; Past 2 GB the setup is split; it is not, at this size, but say it.
DiskSpanning=no
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
ShowLanguageDialog=auto

[Languages]
Name: "chinesesimplified"; MessagesFile: "languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\icons\concat.ico"; DestDir: "{app}"; DestName: "app.ico"; Flags: ignoreversion
; The models, where the app's Settings would have downloaded them.
Source: "{#Models}\whisper-models\*"; DestDir: "{localappdata}\{#DataId}\whisper-models"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Models}\tts-models\*"; DestDir: "{localappdata}\{#DataId}\tts-models"; Flags: ignoreversion recursesubdirs createallsubdirs
; The rules: a colleague's own edits to a file of the same name are kept.
Source: "{#Rules}\*"; DestDir: "{userappdata}\{#DataId}\assistant-rules"; Flags: onlyifdoesntexist uninsneveruninstall

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#ExeName}"; IconFilename: "{app}\app.ico"
Name: "{autoprograms}\{#AppName} (safe graphics)"; Filename: "{app}\{#ExeName}"; Parameters: "--software"; IconFilename: "{app}\app.ico"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#ExeName}"; IconFilename: "{app}\app.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#ExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
// A copy installed before the rename to BloomCut kept its models, settings
// and rules under app.concat.editor. Move that folder to the new name
// before the files above are put there, so the models are not installed a
// second time beside the old ones, and leave a junction at the old name for
// projects whose sounds and voices are recorded under it - what the app's
// own move (concat-host dirs.rs, `adopt`) does when it starts.
procedure Adopt(Base: String);
var
  Old, New: String;
  Code: Integer;
begin
  Old := AddBackslash(Base) + 'app.concat.editor';
  New := AddBackslash(Base) + '{#DataId}';
  if DirExists(Old) and not DirExists(New) then
  begin
    if RenameFile(Old, New) then
      Exec(ExpandConstant('{cmd}'), '/C mklink /J "' + Old + '" "' + New + '"', '', SW_HIDE, ewWaitUntilTerminated, Code)
    else
      Log('Could not move ' + Old + ' to ' + New);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  // Only BloomCut moves a folder from before its rename; another edition
  // never touches a folder that is not its own.
  if (CurStep = ssInstall) and ('{#Adopt}' = '1') then
  begin
    Adopt(ExpandConstant('{localappdata}'));
    Adopt(ExpandConstant('{userappdata}'));
  end;
end;
