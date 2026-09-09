; Only Sufe's own service is touched. Wait for it to release protected binaries
; before upgrading/uninstalling; never terminate unrelated proxy processes.
!macro SUFE_STOP_SERVICE
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -Command "try { Get-Service -Name xboard-svc -ErrorAction SilentlyContinue | ForEach-Object { Stop-Service -InputObject $$_ -ErrorAction Stop; $$_.WaitForStatus([System.ServiceProcess.ServiceControllerStatus]::Stopped, [TimeSpan]::FromSeconds(30)) }; exit 0 } catch { exit 1 }"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "无法停止 Sufe 服务。请关闭 Sufe 后重试安装。"
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; The broker deliberately accepts only the protected default installation.
  ; Reject an incompatible custom location before copying an unusable TUN app.
  !if "${ARCH}" == "x64"
    StrCpy $0 "$PROGRAMFILES64\Sufe"
  !else if "${ARCH}" == "arm64"
    StrCpy $0 "$PROGRAMFILES64\Sufe"
  !else
    StrCpy $0 "$PROGRAMFILES\Sufe"
  !endif
  ${If} $INSTDIR != $0
    MessageBox MB_ICONSTOP|MB_OK "Sufe 的 TUN 服务需要受保护的安装目录。请使用默认目录：$0"
    Abort
  ${EndIf}
  !insertmacro SUFE_STOP_SERVICE
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro SUFE_STOP_SERVICE
  nsExec::ExecToStack '"$SYSDIR\sc.exe" delete xboard-svc'
  Pop $0
  Pop $1
!macroend
