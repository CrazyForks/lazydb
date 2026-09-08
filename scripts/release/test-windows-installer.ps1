$ErrorActionPreference = 'Stop'

function Assert-True {
    param(
        [bool] $Condition,
        [string] $Message
    )
    if (-not $Condition) {
        throw "Assertion failed: $Message"
    }
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$installerPath = Join-Path $root 'pages/install.ps1'
$installer = Get-Content -LiteralPath $installerPath -Raw

Assert-True ($installer -match 'Invoke-WebRequest -Uri "\$baseUrl/\$channel\.json" -OutFile \$manifestPath') `
    'manifest is downloaded to a file'
Assert-True ($installer -match 'Invoke-WebRequest -Uri \$asset\.url -OutFile \$archivePath') `
    'release archive is downloaded to a file'
Assert-True ($installer -match 'Get-FileHash -Algorithm SHA256') `
    'release archive is checksum-verified'
Assert-True ($installer -match 'Expand-Archive') 'release archive is extracted'
Assert-True ($installer -match 'ConvertFrom-Json') 'manifest is parsed as JSON'
Assert-True ($installer -match 'finally') 'temporary files are cleaned up'

# These tests intentionally fail before the compatibility fix. They protect the
# Windows PowerShell 5.1 contracts that are not exercised by POSIX installer tests.
Assert-True ($installer -match '\$archivePath[^\r\n]*\.zip') `
    'temporary archive has a .zip extension'
Assert-True ($installer -match 'Expand-Archive\s+-LiteralPath') `
    'archive extraction uses a literal path'
Assert-True ($installer -match 'UTF8Encoding\]\:\:new\(\$false\)') `
    'installation state is written as UTF-8 without a BOM'
Assert-True ($installer -match '\[string\]\[Environment\]::GetEnvironmentVariable\(''Path'', ''User''\)') `
    'missing user PATH is handled as an empty string'

Write-Host 'Windows installer contract tests passed.'
