param([string]$Installer = 'artifacts/Sufe_0.1.0_x64-setup.exe')
$ErrorActionPreference = 'Stop'
$currentIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (([Security.Principal.WindowsPrincipal]::new($currentIdentity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this environment-based smoke launcher from a non-elevated PowerShell. WebView2 ignores remote-debugging environment flags in elevated apps. Elevated build agents must use a separate local diagnostic build; do not change machine policies.'
}
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$installerPath = (Resolve-Path (Join-Path $projectRoot $Installer)).Path
$smokeRoot = Join-Path $projectRoot ('.tools/native-smoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $smokeRoot | Out-Null
& 7z x $installerPath ('-o' + $smokeRoot) '-y' 'xboard-desktop.exe' 'mihomo.exe' 'xboard-svc.exe' 'binaries/wintun.dll' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Installer extraction failed' }
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$previousArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$previousProfile = $env:WEBVIEW2_USER_DATA_FOLDER
try {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port --remote-debugging-address=127.0.0.1"
    $env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $smokeRoot 'webview'
    $application = Start-Process -FilePath (Join-Path $smokeRoot 'xboard-desktop.exe') -WorkingDirectory $smokeRoot -WindowStyle Hidden -PassThru -Environment @{ WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS; WEBVIEW2_USER_DATA_FOLDER = $env:WEBVIEW2_USER_DATA_FOLDER } -RedirectStandardOutput (Join-Path $smokeRoot 'stdout.log') -RedirectStandardError (Join-Path $smokeRoot 'stderr.log')
    $info = @{ pid = $application.Id; port = $port; directory = $smokeRoot }
    $info | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $projectRoot '.tools/native-smoke-current.json') -Encoding utf8
    $info | ConvertTo-Json -Compress
} finally {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousArguments
    $env:WEBVIEW2_USER_DATA_FOLDER = $previousProfile
}
