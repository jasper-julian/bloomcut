; SPDX-License-Identifier: AGPL-3.0-or-later
; SPDX-FileCopyrightText: 2026 Concat contributors
;
; The team's installer: the app as concat.iss installs it, plus what a
; colleague would otherwise have to fetch or be sent - the transcriber and
; voice models, and the AI assistant's rules - each put where Concat looks
; for it, so the app works the moment it opens, offline included.
;
;   ISCC /DVersion=0.2.11-team.1 /DStage=<app folder> /DModels=<models>
;        /DRules=<rules folder> /DOut=<output folder> concat-team.iss
;
; Stage:  concat.exe and the DLLs beside it.
; Models: a folder with whisper-models\ and tts-models\, as in the app's
;         data folder.
; Rules:  the *.md files for Settings > AI assistant > Rules.
;
; Per user, no administrator: the models go to the user's own
; %LOCALAPPDATA%\app.concat.editor and the rules to %APPDATA%, where the
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

[Setup]
; Its own id, so it neither upgrades nor is upgraded by the upstream app.
AppId={{5C2E9A41-7D3B-4E8F-A1C6-93B0D4E7F2A8}
AppName=Concat
AppVersion={#Version}
AppVerName=Concat {#Version}
AppPublisher=Concat contributors
AppPublisherURL=https://github.com/jasper-julian/concat-man
AppSupportURL=https://github.com/jasper-julian/concat-man/issues
AppUpdatesURL=https://github.com/jasper-julian/concat-man/releases
DefaultDirName={autopf}\Concat
DefaultGroupName=Concat
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir={#Out}
OutputBaseFilename=Concat-{#Version}-setup
SetupIconFile=..\icons\concat.ico
UninstallDisplayIcon={app}\concat.ico
UninstallDisplayName=Concat
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
Source: "..\icons\concat.ico"; DestDir: "{app}"; Flags: ignoreversion
; The models, where the app's Settings would have downloaded them.
Source: "{#Models}\whisper-models\*"; DestDir: "{localappdata}\app.concat.editor\whisper-models"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Models}\tts-models\*"; DestDir: "{localappdata}\app.concat.editor\tts-models"; Flags: ignoreversion recursesubdirs createallsubdirs
; The rules: a colleague's own edits to a file of the same name are kept.
Source: "{#Rules}\*"; DestDir: "{userappdata}\app.concat.editor\assistant-rules"; Flags: onlyifdoesntexist uninsneveruninstall

[Icons]
Name: "{autoprograms}\Concat"; Filename: "{app}\concat.exe"; IconFilename: "{app}\concat.ico"
Name: "{autoprograms}\Concat (safe graphics)"; Filename: "{app}\concat.exe"; Parameters: "--software"; IconFilename: "{app}\concat.ico"
Name: "{autodesktop}\Concat"; Filename: "{app}\concat.exe"; IconFilename: "{app}\concat.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\concat.exe"; Description: "{cm:LaunchProgram,Concat}"; Flags: nowait postinstall skipifsilent
