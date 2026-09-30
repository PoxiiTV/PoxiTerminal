#ifndef AppVersion
  #define AppVersion "2.1.0"
#endif

#ifndef NumericVersion
  #define NumericVersion "2.1.0.0"
#endif

#ifndef Configuration
  #define Configuration "release"
#endif

#ifndef PackageBrand
  #define PackageBrand "PoxiTerminal"
#endif

#ifndef Architecture
  #define Architecture "x64"
#endif

#define RepoRoot ".."
#ifndef BuildRoot
  #define BuildRoot RepoRoot + "\target\" + Configuration
#endif

[Setup]
#ifdef AcceptanceFixture
AppId={{55C51219-76DF-4981-A318-A0261C1B3199}
AppName=PoxiTerminal Update Acceptance
AppVerName=PoxiTerminal Update Acceptance {#AppVersion}
#else
AppId={{2586025B-5FC6-4D84-B832-B2DDB5D3130E}
AppName=PoxiTerminal
AppVerName=PoxiTerminal {#AppVersion}
#endif
AppVersion={#AppVersion}
AppPublisher=Poxi
AppPublisherURL=https://github.com/PoxiiTV/PoxiTerminal
AppSupportURL=https://github.com/PoxiiTV/PoxiTerminal/issues
AppUpdatesURL=https://github.com/PoxiiTV/PoxiTerminal/releases
VersionInfoVersion={#NumericVersion}
VersionInfoTextVersion={#AppVersion}
VersionInfoCompany=Poxi
VersionInfoDescription=PoxiTerminal Installer
VersionInfoProductName=PoxiTerminal
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductTextVersion={#AppVersion}
DefaultDirName={code:DefaultInstallDir}
UsePreviousAppDir=yes
DefaultGroupName=PoxiTerminal
UsePreviousGroup=no
DisableProgramGroupPage=yes
DisableWelcomePage=no
DisableDirPage=no
DisableReadyPage=no
PrivilegesRequired=lowest
#if Architecture == "arm64"
ArchitecturesAllowed=arm64
#else
ArchitecturesAllowed=x64compatible
#endif
MinVersion=10.0.17763
LicenseFile={#RepoRoot}\LICENSE
SetupIconFile={#RepoRoot}\nebula_app\windows\nebula.ico
UninstallDisplayIcon={app}\poxiterminal.exe
OutputDir={#RepoRoot}\dist
OutputBaseFilename={#PackageBrand}-v{#AppVersion}-windows-{#Architecture}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
RestartIfNeededByRun=no
SetupLogging=yes
ChangesEnvironment=yes
ShowLanguageDialog=no

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[CustomMessages]
spanish.DesktopIcon=Crear un acceso directo en el escritorio
spanish.AutoStart=Iniciar PoxiTerminal al iniciar sesión en Windows
spanish.InstallFont=Instalar la fuente Maple Mono para el usuario actual
spanish.AddToPath=Añadir PoxiTerminal al PATH del usuario
spanish.OpenInPoxiTerminal=Abrir en PoxiTerminal
spanish.OpenInPoxiTerminalWsl=Abrir en PoxiTerminal (WSL)
spanish.WslMenuConflict=Ya existe un submenú de WSL que pertenece a otra instalación o se ha modificado, así que se ha conservado: %1.
spanish.WslMenuRegistrationFailed=No se pudo registrar el submenú contextual de WSL.
spanish.LaunchProgram=Abrir PoxiTerminal
spanish.UninstallProgram=Desinstalar PoxiTerminal
spanish.CloseLegacyProgram=Cierra la aplicación que se está ejecutando en %1 y vuelve a intentar la instalación.
spanish.MigrationFailed=PoxiTerminal se ha instalado, pero la instalación anterior no se pudo migrar por completo: %1. Cierra la aplicación anterior y vuelve a ejecutar este instalador. El resto de archivos y la configuración se han conservado.
spanish.MigrationPreflightFailed=No se pudo comprobar la instalación anterior: %1. La instalación no ha empezado.
spanish.RemovePathFailed=No se pudo quitar el directorio de instalación de PoxiTerminal del PATH.

[Tasks]
#ifndef AcceptanceFixture
Name: "installfont"; Description: "{cm:InstallFont}"
Name: "addtopath"; Description: "{cm:AddToPath}"
Name: "desktopicon"; Description: "{cm:DesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "{cm:AutoStart}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
#endif

[Files]
Source: "{#BuildRoot}\poxiterminal.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BuildRoot}\pebrel-hook.exe"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "{#BuildRoot}\conpty.dll"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "{#BuildRoot}\OpenConsole.exe"; DestDir: "{app}\runtime"; Flags: ignoreversion
Source: "{#RepoRoot}\assets\fonts\MapleMonoNormal-NF-CN-Regular.ttf"; DestDir: "{app}\fonts"; Flags: ignoreversion
#ifndef AcceptanceFixture
Source: "{#RepoRoot}\assets\fonts\MapleMonoNormal-NF-CN-Regular.ttf"; DestDir: "{autofonts}"; FontInstall: "Maple Mono Normal NF CN"; Tasks: installfont; Flags: onlyifdoesntexist uninsneveruninstall
#endif
Source: "{#RepoRoot}\INSTALL.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#RepoRoot}\docs\lua-configuration.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#RepoRoot}\docs\runtime-control-api.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#RepoRoot}\docs\runtime-api-v1.schema.json"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "{#RepoRoot}\docs\skills\pebrel-runtime\SKILL.md"; DestDir: "{app}\skills\pebrel-runtime"; Flags: ignoreversion
Source: "{#RepoRoot}\docs\skills\pebrel-runtime\agents\openai.yaml"; DestDir: "{app}\skills\pebrel-runtime\agents"; Flags: ignoreversion
Source: "{#RepoRoot}\LICENSE"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#RepoRoot}\licenses\LICENSE-LUA"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#RepoRoot}\licenses\LICENSE-MLUA"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#RepoRoot}\THIRD-PARTY-NOTICES"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
#ifndef AcceptanceFixture
Name: "{group}\PoxiTerminal"; Filename: "{app}\poxiterminal.exe"; Parameters: "--gpui"; WorkingDir: "{%USERPROFILE}"; AppUserModelID: "com.poxiitv.poxiterminal"
Name: "{group}\{cm:UninstallProgram}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\PoxiTerminal"; Filename: "{app}\poxiterminal.exe"; Parameters: "--gpui"; WorkingDir: "{%USERPROFILE}"; AppUserModelID: "com.poxiitv.poxiterminal"; Tasks: desktopicon
Name: "{userstartup}\PoxiTerminal"; Filename: "{app}\poxiterminal.exe"; Parameters: "--gpui"; WorkingDir: "{%USERPROFILE}"; AppUserModelID: "com.poxiitv.poxiterminal"; Tasks: autostart
#endif

[Registry]
#ifndef AcceptanceFixture
Root: HKCU; Subkey: "Software\PoxiTerminal"; ValueType: dword; ValueName: "InstallerAddedToPath"; ValueData: "1"; Tasks: addtopath; Check: NeedsAddToPath; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddToPath; Flags: preservestringtype
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\poxiterminal.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\poxiterminal.exe"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\poxiterminal.exe"; ValueType: string; ValueName: "Path"; ValueData: "{app}"
; 目录背景使用 %V，选中的目录对象使用 %1；两者必须由 Explorer 展开后再交给 CLI。
; 每个动词使用独立的应用子键，卸载时只删除 PoxiTerminal 自己注册的菜单。
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\PoxiTerminal"; ValueType: string; ValueName: "MUIVerb"; ValueData: "{cm:OpenInPoxiTerminal}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\PoxiTerminal"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\poxiterminal.exe,0"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\PoxiTerminal\command"; ValueType: string; ValueName: ""; ValueData: """{app}\poxiterminal.exe"" --gpui --working-directory ""%V"""
Root: HKCU; Subkey: "Software\Classes\Directory\shell\PoxiTerminal"; ValueType: string; ValueName: "MUIVerb"; ValueData: "{cm:OpenInPoxiTerminal}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\PoxiTerminal"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\poxiterminal.exe,0"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\PoxiTerminal\command"; ValueType: string; ValueName: ""; ValueData: """{app}\poxiterminal.exe"" --gpui --working-directory ""%1"""
#endif

[Run]
Filename: "{app}\poxiterminal.exe"; Parameters: "--gpui"; Description: "{cm:LaunchProgram}"; WorkingDir: "{%USERPROFILE}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
#ifndef AcceptanceFixture
; 必须在 Inno 删除 poxiterminal.exe 前调用应用自己的结构化清理逻辑，避免直接改写用户配置。
Filename: "{app}\poxiterminal.exe"; Parameters: "setup-ai --remove"; WorkingDir: "{app}"; RunOnceId: "RemovePoxiTerminalAiHooks"; Flags: runhidden skipifdoesntexist
#endif

[Code]
#include "installer-migration.iss"

function NeedsAddToPath: Boolean;
var
  ExistingPath: string;
begin
  ExistingPath := '';
  Result := True;
  if RegQueryStringValue(HKCU, 'Environment', 'Path', ExistingPath) then
    Result := not PathContainsDirectory(ExistingPath, ExpandConstant('{app}'));
end;

#ifndef AcceptanceFixture
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ExistingPath: string;
begin
  if CurUninstallStep <> usUninstall then
    Exit;

  { WSL 右键项是 [Code] 动态写的、没有 uninsdeletekey，必须自己认领删除；
    这一步排在 PATH 那段的早退之前——两条互不依赖。 }
  RemoveOwnedWslContextMenus;

  if not RegValueExists(HKCU, 'Software\PoxiTerminal', 'InstallerAddedToPath') then
    Exit;

  ExistingPath := '';
  if RegQueryStringValue(HKCU, 'Environment', 'Path', ExistingPath) and
    not RegWriteExpandStringValue(HKCU, 'Environment', 'Path',
      RemovePathDirectory(ExistingPath, ExpandConstant('{app}'))) then
    RaiseException(CustomMessage('RemovePathFailed'));
  if not RegDeleteValue(HKCU, 'Software\PoxiTerminal', 'InstallerAddedToPath') then
    RaiseException(CustomMessage('RemovePathFailed'));
  RegDeleteKeyIfEmpty(HKCU, 'Software\PoxiTerminal');
end;
#endif
