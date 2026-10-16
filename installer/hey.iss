; Hey for Windows installer (Inno Setup 6).
; Build: iscc /DAppVersion=0.4.0 installer\hey.iss
; Installs for the current user only, so it never needs admin rights.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Keep this ID the same forever: Windows uses it to recognise updates.
AppId={{8118C34E-1B65-4E1B-B622-5AA2CD0B6EB7}
AppName=Hey
AppVersion={#AppVersion}
AppVerName=Hey {#AppVersion}
AppPublisher=joelcave42
AppPublisherURL=https://github.com/joelcave42/hey
AppSupportURL=https://github.com/joelcave42/hey/issues
DefaultDirName={localappdata}\Programs\Hey
DisableProgramGroupPage=yes
DisableDirPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=Hey-Setup-{#AppVersion}
SetupIconFile=..\windows\hey.ico
UninstallDisplayIcon={app}\hey.exe
UninstallDisplayName=Hey
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Close a running Hey before replacing it, and start it again afterwards.
CloseApplications=force
RestartApplications=no

[Tasks]
Name: "startup"; Description: "Start Hey when I sign in to Windows"

[Files]
Source: "..\windows\target\release\hey.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Hey"; Filename: "{app}\hey.exe"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Hey"; ValueData: """{app}\hey.exe"""; Tasks: startup; Flags: uninsdeletevalue
; Hey adds these itself while it runs. They're listed here so uninstalling
; removes them: notifications, notification buttons, "Send to phone" in
; Explorer, and Start with Windows.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\joelcave42.Hey"; Flags: uninsdeletekey dontcreatekey
Root: HKCU; Subkey: "Software\Classes\hey-action"; Flags: uninsdeletekey dontcreatekey
Root: HKCU; Subkey: "Software\Classes\*\shell\HeySendToPhone"; Flags: uninsdeletekey dontcreatekey
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Hey"; Flags: uninsdeletevalue dontcreatekey

[Run]
Filename: "{app}\hey.exe"; Description: "Open Hey"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/IM hey.exe /F"; Flags: runhidden; RunOnceId: "StopHey"
