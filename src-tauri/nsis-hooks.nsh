; WorkBuddy Deck 安装向导前置校验（Tauri NSIS hooks）
;
; 1) 查原软件：读注册表（App Paths + Uninstall，含 WOW6432Node）+ 默认安装目录，
;    判断用户是否安装了官方客户端。没装 → 弹窗「请先安装官方原版软件」并中止安装。
; 2) 检查运行状态：官方软件正在运行 → 弹窗提示先关闭（重试 / 取消），
;    否则无法替换或注入。
;
; 由 tauri.conf.json 的 bundle.windows.nsis.installerHooks 挂载，
; Tauri 构建时自动 include，不要手动编译本文件。
; 注意：NSIS 字符串里 PowerShell 的 $ 一律写成 $$（NSIS 转义），双引号在
; 单引号包裹的 NSIS 字符串里是字面量，不需要转义。

!macro NSIS_HOOK_PREINSTALL
  ; ---- 1) 官方客户端存在性：注册表 + 默认目录 ----
  ; PowerShell 探测到官方安装痕迹 exit 0，否则 exit 1。
  nsExec::ExecToStack 'powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$$found=$$false; $${exe}=Join-Path $${env:LOCALAPPDATA} 'Programs\WorkBuddy\WorkBuddy.exe'; if (Test-Path $${exe}) { $$found=$$true }; if (-not $$found) { $${keys}=@('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*','HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'); Get-ItemProperty $${keys} -ErrorAction SilentlyContinue | ForEach-Object { if ($$_.DisplayName -match 'WorkBuddy') { $$found=$$true } } }; if ($$found) { exit 0 } else { exit 1 }"'
  Pop $R0
  ${If} $R0 != 0
    MessageBox MB_ICONSTOP|MB_OK "未检测到官方原版软件（WorkBuddy）。$\n$\n请先安装官方原版软件，再运行本安装程序。" /SD IDOK
    Abort
  ${EndIf}

  ; ---- 2) 官方软件运行状态：在跑则要求先关闭 ----
  ; tasklist|find 匹配到 WorkBuddy.exe → find exit 0 = 正在运行。
WBD_RUNNING_LOOP:
  nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq WorkBuddy.exe" /NH 2>nul | find /I "WorkBuddy.exe" >nul 2>&1'
  Pop $R0
  ${If} $R0 == 0
    MessageBox MB_ICONEXCLAMATION|MB_RETRYCANCEL "检测到官方软件正在运行。$\n$\n请先关闭官方软件（WorkBuddy），然后点击「重试」继续安装。" IDRETRY WBD_RUNNING_LOOP
    Abort
  ${EndIf}
!macroend
