# install.ps1 end to end: install the latest release, run it, uninstall it,
# and fail if anything is left behind. Piped to iex, as the README runs it.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute(
    'PSAvoidUsingInvokeExpression', '',
    Justification = 'the README pipes install.ps1 to iex')]
param()
$ErrorActionPreference = 'Stop'

function Fail([string]$message) {
    Write-Output "::error title=test:install-windows::$message"
    exit 1
}

$script = Join-Path (Split-Path $PSScriptRoot) 'install.ps1'
# each line install.ps1 and `synth install` printed
$out = @([IO.File]::ReadAllText($script) | Invoke-Expression)
$out
$line = @($out -match '^installed synth .* to (.+)$')
if ($line.Count -eq 0) {
    Fail 'install.ps1 printed no install line'
}
$exe = ($line[0] -replace '^installed synth .* to ', '').Trim()

$version = & $exe --version
$version
if ($LASTEXITCODE -ne 0 -or $version -notmatch '^synth \d') {
    Fail "$exe --version failed"
}

& $exe uninstall
if ($LASTEXITCODE -ne 0) {
    Fail "$exe uninstall exited with $LASTEXITCODE"
}
$left = @(Get-ChildItem -Force -LiteralPath (Split-Path $exe) -Filter 'synth*')
$left += @(Get-ChildItem -Force -LiteralPath ([IO.Path]::GetTempPath()) `
        -Filter 'synth-install-*')
if ($left.Count -gt 0) {
    $left | ForEach-Object { $_.FullName }
    Fail "uninstall left $($left.Count) files"
}
