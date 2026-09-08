param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('stable', 'beta')]
    [string] $Channel,
    [Parameter(Mandatory = $true)]
    [string] $Version
)

$ErrorActionPreference = 'Stop'
$entry = if ($Channel -eq 'stable') { 'install.sh' } else { 'install-beta.sh' }
$temp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
$home = Join-Path $temp 'home'
$appData = Join-Path $temp 'appdata'
$localAppData = Join-Path $temp 'localappdata'
$installDir = Join-Path $localAppData 'LazyDB [smoke] bin'
$installer = Join-Path $temp 'installer.ps1'
$oldPath = [string][Environment]::GetEnvironmentVariable('Path', 'User')
$restorePath = $true

try {
    New-Item -ItemType Directory -Path $home, $appData, $localAppData -Force | Out-Null
    $env:HOME = $home
    $env:APPDATA = $appData
    $env:LOCALAPPDATA = $localAppData
    $env:LAZYDB_INSTALL_DIR = $installDir
    $env:LAZYDB_CHANNEL = $Channel
    $env:LAZYDB_MCP_SETUP = 'skip'
    $env:LAZYDB_CHANNEL_BASE_URL = 'https://lazydb.yelog.org/channels'

    $attempt = 1
    while ($attempt -le 6) {
        try {
            Invoke-WebRequest -Uri "https://lazydb.yelog.org/$entry" -UseBasicParsing -OutFile $installer
            if ((Get-Item -LiteralPath $installer).Length -eq 0) {
                throw 'online installer is empty'
            }
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $installer
            if ($LASTEXITCODE -ne 0) {
                throw "installer failed with exit code $LASTEXITCODE"
            }
            $versionJson = & (Join-Path $installDir 'lazydb.exe') version --json
            if ($LASTEXITCODE -ne 0) { throw 'installed binary version command failed' }
            $installedVersion = ($versionJson | ConvertFrom-Json).version
            if ($installedVersion -ne $Version) {
                throw "installed version mismatch: $installedVersion (expected $Version)"
            }
            $statePath = Join-Path $appData 'lazydb/install.json'
            $stateBytes = [IO.File]::ReadAllBytes($statePath)
            if ($stateBytes.Length -ge 3 -and $stateBytes[0] -eq 0xef -and $stateBytes[1] -eq 0xbb -and $stateBytes[2] -eq 0xbf) {
                throw 'installation state contains a UTF-8 BOM'
            }
            $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
            if ($state.channel -ne $Channel -or $state.version -ne $Version -or $state.target -ne 'x86_64-pc-windows-msvc') {
                throw 'installation state identity mismatch'
            }
            Write-Host "Online Windows installation verified: $Channel $Version"
            exit 0
        } catch {
            if ($attempt -eq 6) { throw }
            Write-Warning "Attempt $attempt failed: $($_.Exception.Message)"
            Start-Sleep -Seconds 15
            $attempt++
        }
    }
} finally {
    if ($restorePath) {
        [Environment]::SetEnvironmentVariable('Path', $oldPath, 'User')
    }
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
