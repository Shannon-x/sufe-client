param([switch]$SkipTests)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $projectRoot
try {
    if (-not $SkipTests) {
        cargo test -p xboard-core --lib
        if ($LASTEXITCODE -ne 0) { throw 'Core tests failed' }
    }
    python scripts/install-kernel.py --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Kernel verification failed' }
    cargo build -p xboard-svc
    if ($LASTEXITCODE -ne 0) { throw 'Service sidecar build failed' }
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target/debug/xboard-svc.exe') -Destination (Join-Path $projectRoot 'desktop/src-tauri/binaries/xboard-svc-x86_64-pc-windows-msvc.exe') -Force
    $localTools = Join-Path $projectRoot '.tools'
    New-Item -ItemType Directory -Force -Path $localTools | Out-Null
    # Local review builds do not publish updates or require a production key.
    $bundleConfig = Join-Path $localTools 'local-bundle.json'
    [IO.File]::WriteAllText($bundleConfig, '{"bundle":{"createUpdaterArtifacts":false}}', [Text.UTF8Encoding]::new($false))
    Push-Location (Join-Path $projectRoot 'desktop')
    try {
        npm run tauri -- build --debug --bundles nsis --config $bundleConfig
        if ($LASTEXITCODE -ne 0) { throw 'Desktop bundle build failed' }
    } finally { Pop-Location }
    $artifactRoot = Join-Path $projectRoot 'artifacts'
    New-Item -ItemType Directory -Force -Path $artifactRoot | Out-Null
    $installers = Get-ChildItem -LiteralPath (Join-Path $projectRoot 'target/debug/bundle/nsis') -Filter '*.exe'
    foreach ($installerFile in $installers) {
        Copy-Item -LiteralPath $installerFile.FullName -Destination $artifactRoot -Force
        Get-FileHash -LiteralPath (Join-Path $artifactRoot $installerFile.Name) -Algorithm SHA256
    }
} finally { Pop-Location }
