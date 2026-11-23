; WorkBuddy Deck 安装向导（Inno Setup 6，WorkDaddy 同款口径）
;
; 对齐 babygoton/WorkDaddy scripts/win/workdaddy.iss 的机制：
; - PrivilegesRequired=lowest，per-user 装到 {localappdata}\Programs
; - 目录页之后插入自定义「选择官方客户端」页：自动扫注册表
;   （Uninstall 的 DisplayIcon/InstallLocation + App Paths）+ 固定目录，
;   命中 WorkBuddy.exe 预填到输入框，可手改，带「使用自动识别路径」按钮
; - PrepareToInstall：官方客户端未找到弹「请先安装官方原版软件」；
;   官方运行中弹「结束进程 / 重新检测 / 取消」三态对话框
; - 安装完成把选中的客户端路径写入 {%USERPROFILE}\.wb-switch\workbuddy_exe.json
;   （{"cn": "<路径>"}），运行时 load_workbuddy_exe_cache 优先采用
; - 卸载先 taskkill 本程序，再整目录删除
;
; 编译：ISCC.exe /DAppVersion=x.y.z src-tauri\installer\workbuddy-deck.iss

#define AppName "WorkBuddy Deck"
#define AppExe "workbuddy-task-assistant.exe"
#define ClientExe "WorkBuddy.exe"
#define AppPublisher "wbswitch"
#define AppGuid "{{7E3F9A62-2C41-4B8D-9A55-8F1D26C4E9A0}"

#ifndef SrcDir
  #define SrcDir "..\target\release"
#endif

#ifndef AppVersion
#error 请用 /DAppVersion=x.y.z 指定版本号
#endif

[Setup]
AppId={#AppGuid}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\Programs\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
WizardStyle=modern
ArchitecturesAllowed=x64compatible
MinVersion=10.0
CloseApplications=no
RestartApplications=no
SetupLogging=yes
OutputDir=..\target\release\bundle\innosetup
OutputBaseFilename=WorkBuddyDeck-Setup-{#AppVersion}
SetupIconFile=..\icons\icon.ico
UninstallDisplayName={#AppName}
Compression=lzma2/ultra64
SolidCompression=yes

[Languages]
Name: "chs"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Files]
Source: "{#SrcDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\icons\icon.ico"; DestDir: "{app}\icons"; Flags: ignoreversion

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加任务："

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\icons\icon.ico"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\icons\icon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "立即打开 {#AppName}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{cmd}"; Parameters: "/C taskkill /IM {#AppExe} /F >nul 2>&1"; Flags: runhidden; RunOnceId: "KillDeck"

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Code]
const
  ClientExeName = '{#ClientExe}';
  UninstallKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall';
  AppPathsKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\';

var
  ClientPage: TInputFileWizardPage;
  AutoDetectPath: String;

{ ---------- 工具函数 ---------- }

{ 从 DisplayIcon 值里解出 exe 路径：去引号、去 ",图标索引" 后缀 }
function ExecutableFromDisplayIcon(const Icon: String): String;
var
  QuotePos: Integer;
begin
  Result := Trim(Icon);
  if Length(Result) = 0 then
    Exit;
  if Result[1] = '"' then begin
    Delete(Result, 1, 1);
    QuotePos := Pos('"', Result);
    if QuotePos > 0 then
      Result := Copy(Result, 1, QuotePos - 1);
  end else begin
    QuotePos := Pos(',', Result);
    if QuotePos > 0 then
      Result := Copy(Result, 1, QuotePos - 1);
  end;
  Result := Trim(Result);
end;

function AddBackslashIfMissing(const Path: String): String;
begin
  if (Length(Path) > 0) and (Path[Length(Path)] <> '\') then
    Result := Path + '\'
  else
    Result := Path;
end;

{ 只接受文件名等于 WorkBuddy.exe 且真实存在的候选；取第一个命中 }
function ConsiderClientCandidate(const Path: String; var Best: String): Boolean;
begin
  Result := False;
  if (Length(Best) > 0) then
    Exit;
  if (Length(Path) > 0) and (CompareText(ExtractFileName(Path), ClientExeName) = 0)
     and FileExists(Path) then begin
    Best := Path;
    Result := True;
  end;
end;

procedure ScanUninstallRoot(const RootKey: Integer; var Best: String);
var
  Names: TArrayOfString;
  I: Integer;
  Icon, Location: String;
  SubKey: String;
begin
  if not RegGetSubkeyNames(RootKey, UninstallKey, Names) then
    Exit;
  for I := 0 to GetArrayLength(Names) - 1 do begin
    SubKey := UninstallKey + '\' + Names[I];
    if RegQueryStringValue(RootKey, SubKey, 'DisplayIcon', Icon) then
      ConsiderClientCandidate(ExecutableFromDisplayIcon(Icon), Best);
    if RegQueryStringValue(RootKey, SubKey, 'InstallLocation', Location) then
      ConsiderClientCandidate(AddBackslashIfMissing(Trim(Location)) + ClientExeName, Best);
  end;
end;

procedure ScanAppPaths(const RootKey: Integer; var Best: String);
var
  Value: String;
begin
  if RegQueryStringValue(RootKey, AppPathsKey + ClientExeName, '', Value) then
    ConsiderClientCandidate(ExecutableFromDisplayIcon(Value), Best);
end;

{ 官方客户端检测：Uninstall 注册表（HKCU/HKLM64/HKLM32）→ App Paths → 固定目录 }
function DetectOfficialClient(var Binary: String): Boolean;
begin
  Binary := '';
  ScanUninstallRoot(HKCU, Binary);
  ScanUninstallRoot(HKLM64, Binary);
  ScanUninstallRoot(HKLM32, Binary);
  ScanAppPaths(HKCU, Binary);
  ScanAppPaths(HKLM64, Binary);
  ScanAppPaths(HKLM32, Binary);
  ConsiderClientCandidate(ExpandConstant('{localappdata}') + '\Programs\WorkBuddy\' + ClientExeName, Binary);
  ConsiderClientCandidate(ExpandConstant('{pf}') + '\WorkBuddy\' + ClientExeName, Binary);
  ConsiderClientCandidate(ExpandConstant('{pf32}') + '\WorkBuddy\' + ClientExeName, Binary);
  ConsiderClientCandidate(ExpandConstant('{sd}') + '\Program Files\WorkBuddy\' + ClientExeName, Binary);
  Result := Length(Binary) > 0;
end;

{ 官方客户端是否在运行（tasklist | find，find 命中即 exit 0） }
function ClientRunning(): Boolean;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{cmd}'),
    Format('/C tasklist /FI "IMAGENAME eq %s" /NH 2>nul | find /I "%s" >nul 2>&1', [ClientExeName, ClientExeName]),
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := ResultCode = 0;
end;

{ 三态关闭对话框：「结束进程」/「重新检测」/「取消」，对齐 WorkDaddy EnsureWorkBuddyClosed }
function EnsureClientClosed(): Boolean;
var
  ResultCode: Integer;
begin
  Result := False;
  while True do begin
    if not ClientRunning() then begin
      Result := True;
      Exit;
    end;
    case MsgBox('检测到官方软件正在运行。需要先关闭官方软件（WorkBuddy），才能替换或注入。'#13#10#13#10 +
        '「是」= 自动结束官方软件进程并继续；' +
        '「否」= 我已手动关闭，重新检测；' +
        '「取消」= 退出安装。',
        mbConfirmation, MB_YESNOCANCEL) of
      IDYES: begin
        Exec(ExpandConstant('{cmd}'),
          Format('/C taskkill /IM %s /F >nul 2>&1', [ClientExeName]),
          '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
        Sleep(800);
      end;
      IDNO: Sleep(500);
      IDCANCEL: Exit;
    end;
  end;
end;

{ ---------- 向导页面 ---------- }

procedure AutoDetectButtonOnClick(Sender: TObject);
begin
  if DetectOfficialClient(AutoDetectPath) then
    ClientPage.Values[0] := AutoDetectPath
  else
    MsgBox('未自动识别到官方客户端（WorkBuddy）。请确认已安装官方原版软件，或手动选择 WorkBuddy.exe。',
      mbError, MB_OK);
end;

procedure InitializeWizard();
begin
  ClientPage := CreateInputFilePage(wpSelectDir,
    '选择官方客户端',
    '选择要连接的官方客户端主程序。本程序不改官方客户端本体，只通过调试端口连接并注入面板。',
    '点击下一步继续');
  ClientPage.Add('官方客户端路径（WorkBuddy.exe）:',
    '可执行文件|*.exe|所有文件|*.*', '.exe');

  { 检测到官方客户端就预填；检测不到留空，由「下一步」拦截 }
  if DetectOfficialClient(AutoDetectPath) then
    ClientPage.Values[0] := AutoDetectPath;

  { 「使用自动识别路径」按钮（对齐 WorkDaddy 的自定义按钮） }
  with TButton.Create(WizardForm) do begin
    Parent := ClientPage.Surface;
    Left := ClientPage.Edits[0].Left;
    Top := ClientPage.Edits[0].Top + ClientPage.Edits[0].Height + ScaleY(8);
    Width := ScaleX(140);
    Height := ScaleY(26);
    Caption := '使用自动识别路径';
    OnClick := @AutoDetectButtonOnClick;
  end;
end;

{ ---------- 校验 ---------- }

function ValidateClientSelection(): Boolean;
var
  Path: String;
begin
  Result := False;
  Path := Trim(ClientPage.Values[0]);
  if Length(Path) = 0 then begin
    MsgBox('请先安装官方原版软件（WorkBuddy），再运行本安装程序。', mbError, MB_OK);
    Exit;
  end;
  if not FileExists(Path) then begin
    MsgBox(Format('所选文件不存在：%s'#13#10'请重新选择官方客户端路径。', [Path]), mbError, MB_OK);
    Exit;
  end;
  if CompareText(ExtractFileName(Path), ClientExeName) <> 0 then begin
    MsgBox(Format('所选文件不是官方客户端（应为 %s）：%s', [ClientExeName, ExtractFileName(Path)]), mbError, MB_OK);
    Exit;
  end;
  Result := True;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = ClientPage.ID then
    Result := ValidateClientSelection();
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not ValidateClientSelection() then begin
    Result := '安装已停止：官方客户端校验未通过。';
    Exit;
  end;
  if not EnsureClientClosed() then
    Result := '安装已取消：官方软件正在运行。请先关闭官方软件（WorkBuddy）后重试。';
end;

{ ---------- 安装完成：写入客户端路径缓存 ---------- }

{ 写 USERPROFILE 下 .wb-switch/workbuddy_exe.json，内容为 cn 键 + 客户端路径。
  运行时 wb-switch-core 的 load_workbuddy_exe_cache 优先采用该路径，
  用户手动改过路径的机器不再依赖注册表猜测。 }
procedure CurStepChanged(CurStep: TSetupStep);
var
  StoreDir: String;
  JsonPath: String;
  Path: String;
  Lines: TArrayOfString;
begin
  if CurStep = ssPostInstall then begin
    Path := Trim(ClientPage.Values[0]);
    if Length(Path) = 0 then
      Exit;
    { JSON 转义：反斜杠翻倍 }
    StringChangeEx(Path, '\', '\\', True);
    StoreDir := ExpandConstant('{%USERPROFILE}\.wb-switch');
    if not DirExists(StoreDir) then begin
      if not ForceDirectories(StoreDir) then
        Exit;
    end;
    JsonPath := StoreDir + '\workbuddy_exe.json';
    SetArrayLength(Lines, 1);
    Lines[0] := '{"cn": "' + Path + '"}';
    SaveStringsToUTF8File(JsonPath, Lines, False);
  end;
end;
