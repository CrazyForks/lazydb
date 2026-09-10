$ErrorActionPreference = 'Stop'

$channel = if ($env:LAZYDB_CHANNEL) { $env:LAZYDB_CHANNEL } else { 'stable' }
$mcpSetup = if ($env:LAZYDB_MCP_SETUP) { $env:LAZYDB_MCP_SETUP } else { 'auto' }
if ($mcpSetup -notin @('auto', 'skip', 'ask')) { throw "invalid MCP setup mode: $mcpSetup" }
if ($channel -notin @('stable', 'beta')) { throw "invalid channel: $channel" }
$baseUrl = if ($env:LAZYDB_CHANNEL_BASE_URL) { $env:LAZYDB_CHANNEL_BASE_URL.TrimEnd('/') } else { 'https://lazydb.yelog.org/channels' }
$target = 'x86_64-pc-windows-msvc'
$installDir = if ($env:LAZYDB_INSTALL_DIR) { $env:LAZYDB_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'LazyDB\bin' }
$manifestPath = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
$archivePath = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName() + '.zip')
$extractDir = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())

try {
    Invoke-WebRequest -Uri "$baseUrl/$channel.json" -OutFile $manifestPath -UseBasicParsing -TimeoutSec 60
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.schema -ne 1 -or $manifest.product -ne 'lazydb' -or $manifest.channel -ne $channel) {
        throw 'manifest identity mismatch'
    }
    $asset = $manifest.assets.$target
    if ($null -eq $asset -or $asset.url -notlike 'https://github.com/yelog/lazydb/releases/download/*' -or $asset.sha256 -notmatch '^[0-9a-fA-F]{64}$') {
        throw 'invalid Windows release asset'
    }
    Invoke-WebRequest -Uri $asset.url -OutFile $archivePath -UseBasicParsing -TimeoutSec 60
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
    if ($actual -ne $asset.sha256.ToLowerInvariant()) { throw 'checksum mismatch' }

    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force
    $binary = Get-ChildItem -LiteralPath $extractDir -Filter lazydb.exe -File -Recurse | Select-Object -First 1
    if ($null -eq $binary) { throw 'archive does not contain lazydb.exe' }
    $versionJson = & $binary.FullName version --json
    if ($LASTEXITCODE -ne 0 -or ($versionJson | ConvertFrom-Json).version -ne $manifest.version) { throw 'staged binary failed version check' }

    [IO.Directory]::CreateDirectory($installDir) | Out-Null
    $configDir = Join-Path $env:APPDATA 'lazydb'
    $firstInstall = -not (Test-Path -LiteralPath (Join-Path $configDir 'install.json')) -and -not (Test-Path -LiteralPath (Join-Path $installDir 'lazydb.exe'))
    Copy-Item -LiteralPath $binary.FullName -Destination (Join-Path $installDir 'lazydb.exe') -Force
    [IO.Directory]::CreateDirectory($configDir) | Out-Null
    $state = @{ schema = 1; product = 'lazydb'; manager = 'native'; channel = $channel; version = $manifest.version; target = $target; path = (Join-Path $installDir 'lazydb.exe') } |
        ConvertTo-Json
    [IO.File]::WriteAllText(
        (Join-Path $configDir 'install.json'),
        $state,
        [Text.UTF8Encoding]::new($false)
    )

    $userPath = [string][Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($userPath -split ';') -contains $installDir)) {
        [Environment]::SetEnvironmentVariable('Path', (($userPath.TrimEnd(';') + ';' + $installDir).Trim(';')), 'User')
        Write-Host "Added $installDir to the user PATH. Open a new terminal to use lazydb."
    }
    Write-Host "lazydb $($manifest.version) installed ($channel)"
    Write-Host 'Configure LazyDB MCP for Claude Code, Codex, or OpenCode; you can choose a user-level or project-level configuration.'
    $interactive = [Environment]::UserInteractive -and $Host.Name -notmatch 'ServerRemoteHost' -and -not [Console]::IsInputRedirected -and -not [Console]::IsOutputRedirected
    if ($mcpSetup -ne 'skip' -and $interactive -and (($mcpSetup -eq 'ask') -or $firstInstall)) {
        $answer = Read-Host 'Configure LazyDB MCP now? [y/N]'
        if ($answer -match '^(?i:y|yes)$') {
            try {
                & (Join-Path $installDir 'lazydb.exe') mcp setup --help *> $null
                if ($LASTEXITCODE -ne 0) { throw 'MCP setup is unavailable in this LazyDB binary' }
                & (Join-Path $installDir 'lazydb.exe') mcp setup
                if ($LASTEXITCODE -ne 0) { throw "MCP setup exited with code $LASTEXITCODE" }
            } catch {
                Write-Warning 'LazyDB is installed, but MCP setup did not complete.'
                Write-Warning "Run '$(Join-Path $installDir 'lazydb.exe') mcp setup' to retry."
            }
        }
    } elseif ($mcpSetup -ne 'skip' -and -not $interactive) {
        Write-Warning 'MCP setup was skipped because no interactive terminal is available.'
        Write-Warning 'Run lazydb mcp setup in the target project directory to configure it.'
        }
} finally {
    Remove-Item -LiteralPath $manifestPath, $archivePath, $extractDir -Recurse -Force -ErrorAction SilentlyContinue
}
