; Inno Setup script for flag-on-caret-rs.
; The Rust build is a single self-contained exe (flags/cursors are baked in),
; so this installer just drops FlagOnCaret.exe, makes shortcuts and (optionally)
; sets autostart. Derived from the LangBarXX installer; LGPL-3.0.
;
; Expects FlagOnCaret.exe and App.ico next to this .iss at compile time
; (build.ps1 / CI stage them here).

#define MyAppName "FlagOnCaret"
#ifndef MyAppVersion
  #define MyAppVersion "0.2.0"
#endif
#define MyAppExeName "FlagOnCaret.exe"
#define MyAppURL "https://github.com/n0isy/flag-on-caret-rs"

[Setup]
AppId={{2F9E7B14-9C0A-4E3D-B1A6-7D5C2E8F0A31}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisherURL={#MyAppURL}
UsePreviousAppDir=yes
DefaultDirName={code:GetDefRoot}\FlagOnCaret
DefaultGroupName={#MyAppName}
LicenseFile=LGPL-3.0.txt
Uninstallable=not IsTaskSelected('portablemode')
OutputDir=.
OutputBaseFilename=FlagOnCaret_setup
SolidCompression=yes
Compression=lzma2
PrivilegesRequired=none
ArchitecturesInstallIn64BitMode=x64compatible
WizardImageFile=WizModernImage-IS.bmp
WizardSmallImageFile=WizModernSmallImage-IS.bmp
SetupIconFile=Install.ico
ShowLanguageDialog=no
DisableDirPage=auto

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: portablemode; Description: "Portable version"; Flags: unchecked
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: checkablealone
Name: "autorun"; Description: "Run at Windows startup"; Check: not IsTaskSelected('portablemode')

[Files]
Source: "FlagOnCaret.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "App.ico";         DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}";         Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\App.ico"; Check: not IsTaskSelected('portablemode')
Name: "{group}\Uninstall";            Filename: "{uninstallexe}";        Check: not IsTaskSelected('portablemode')
Name: "{commondesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\App.ico"; Tasks: desktopicon; Check: not IsTaskSelected('portablemode')

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Check: not IsTaskSelected('portablemode'); Flags: nowait postinstall

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "FlagOnCaret"; ValueData: """{app}\{#MyAppExeName}"""; Tasks: autorun; Flags: uninsdeletevalue

[UninstallRun]
Filename: "taskkill"; Parameters: "/im ""FlagOnCaret.exe"" /f"; Flags: runhidden; RunOnceId: "killflag"

[Code]
function GetDefRoot(Param: String): String;
begin
  if not IsAdminLoggedOn then
    Result := ExpandConstant('{localappdata}')
  else
    Result := ExpandConstant('{pf}')
end;

// Kill a running instance before (re)installing, so the exe can be overwritten.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/im FlagOnCaret.exe /f', '',
       SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(500);
  Result := '';
end;
