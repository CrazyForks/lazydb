param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('stable', 'beta')]
    [string] $Channel,
    [Parameter(Mandatory = $true)]
    [string] $Version,
    [switch] $AllowUserPathMutation
)

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'This smoke test requires Windows.' }
if (-not $AllowUserPathMutation) { throw 'Use -AllowUserPathMutation on an isolated Windows account; this test temporarily changes user PATH.' }
$hostExe = (Get-Process -Id $PID).Path
$expectedHash = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot '../../pages/install.ps1') -Algorithm SHA256).Hash
$temp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
$testHome = Join-Path $temp 'home'
$appData = Join-Path $temp 'appdata'
$localAppData = Join-Path $temp 'localappdata'
$installDir = Join-Path $localAppData 'LazyDB [smoke] bin'
$installer = Join-Path $temp 'installer.ps1'
# Do not cast to string: an absent user PATH must remain absent.
$oldPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$savedEnvironment = @{}
$overrides = @{
    HOME = $testHome; USERPROFILE = $testHome; APPDATA = $appData; LOCALAPPDATA = $localAppData
    LAZYDB_INSTALL_DIR = $installDir; LAZYDB_CHANNEL = $Channel; LAZYDB_MCP_SETUP = 'skip'
    LAZYDB_CHANNEL_BASE_URL = 'https://lazydb.yelog.org/channels'
}
foreach ($name in $overrides.Keys) { $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }

try {
    foreach ($dir in @($testHome, $appData, $localAppData)) { [IO.Directory]::CreateDirectory($dir) | Out-Null }
    foreach ($name in $overrides.Keys) { [Environment]::SetEnvironmentVariable($name, $overrides[$name], 'Process') }
    for ($attempt = 1; $attempt -le 6; $attempt++) {
        try {
            # Every retry starts clean, so an earlier binary cannot mask a failed installation.
            Remove-Item -LiteralPath $installDir, (Join-Path $appData 'lazydb') -Recurse -Force -ErrorAction SilentlyContinue
            [Environment]::SetEnvironmentVariable('Path', $oldPath, 'User')
            Invoke-WebRequest -Uri 'https://lazydb.yelog.org/install.ps1' -UseBasicParsing -OutFile $installer -TimeoutSec 60
            if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash -ne $expectedHash) {
                throw 'Deployed install.ps1 SHA256 differs from checked-out pages/install.ps1 (stale or unexpected deployment).'
            }
            $process = Start-Process -FilePath $hostExe -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$installer`"" -PassThru -NoNewWindow
            try {
                $null = $process.Handle
                if (-not $process.WaitForExit(120000)) { throw 'installer timed out after 120 seconds' }
                if ($process.ExitCode -ne 0) { throw "installer failed with exit code $($process.ExitCode)" }
            } finally {
                if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
                $process.Dispose()
            }
            $versionOutput = Join-Path $temp 'version.json'
            $process = Start-Process -FilePath (Join-Path $installDir 'lazydb.exe') -ArgumentList 'version --json' -RedirectStandardOutput $versionOutput -PassThru -NoNewWindow
            try {
                $null = $process.Handle
                if (-not $process.WaitForExit(30000)) { throw 'installed binary version command timed out' }
                $process.WaitForExit()
                if ($process.ExitCode -ne 0) { throw 'installed binary version command failed' }
            } finally {
                if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
                $process.Dispose()
            }
            $versionJson = Get-Content -LiteralPath $versionOutput -Raw
            $installedVersion = ($versionJson | ConvertFrom-Json).version
            if ($installedVersion -ne $Version) { throw "installed version mismatch: $installedVersion (expected $Version)" }
            $statePath = Join-Path $appData 'lazydb/install.json'
            $stateBytes = [IO.File]::ReadAllBytes($statePath)
            if ($stateBytes.Length -ge 3 -and $stateBytes[0] -eq 0xef -and $stateBytes[1] -eq 0xbb -and $stateBytes[2] -eq 0xbf) { throw 'installation state contains a UTF-8 BOM' }
            $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
            if ($state.schema -ne 1 -or $state.product -ne 'lazydb' -or $state.manager -ne 'native' -or $state.channel -ne $Channel -or $state.version -ne $Version -or $state.target -ne 'x86_64-pc-windows-msvc' -or $state.path -ne (Join-Path $installDir 'lazydb.exe')) { throw 'installation state identity mismatch' }
            Write-Host "Online Windows installation verified: $Channel $Version ($hostExe)"
            break
        } catch {
            if ($attempt -eq 6) { throw }
            Write-Warning "Attempt $attempt failed: $($_.Exception.Message)"
            Start-Sleep -Seconds 15
        }
    }
} finally {
    try { [Environment]::SetEnvironmentVariable('Path', $oldPath, 'User') } finally {
        foreach ($name in $savedEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process') }
        Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
    }
}
