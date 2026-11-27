# install.ps1 end to end: install the latest release, run it, update it,
# uninstall it, and fail if anything is left behind.
# Piped to iex, as the README runs it.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute(
    'PSAvoidUsingInvokeExpression', '',
    Justification = 'the README pipes install.ps1 to iex')]
param()
$ErrorActionPreference = 'Stop'

function Fail([string]$message) {
    Write-Output "::error title=test:winstall::$message"
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

# after back-to-back version bumps the newest dev build predates this
# release and runs older update code, so the round trip waits for a dev
# build of this version
$checked = @(& $exe update --dev --dry-run)
$checked
if ($LASTEXITCODE -ne 0) {
    Fail "$exe update --dev --dry-run failed"
}
$verified = @($checked -match '^synth \S+ verified')
if ($verified.Count -eq 0) {
    Fail "$exe update --dev --dry-run printed no verified line"
}
$dev = $verified[0] -replace '^synth (\S+) verified.*$', '$1'
$stable = $version -replace '^synth ', ''
if ([version]($dev -replace '[-+].*$', '') -lt
    [version]($stable -replace '[-+].*$', '')) {
    Write-Output ("test:winstall: update --dev round trip skipped, " +
        "the newest dev build $dev predates $stable")
} else {
    # the newest dev build differs from the stable one installed, so update
    # replaces the running synth.exe
    $updated = & $exe update --dev
    $updated
    if ($LASTEXITCODE -ne 0 -or $updated -notmatch '^synth .* -> ') {
        Fail "$exe update --dev failed"
    }
    $version = & $exe --version
    $version
    if ($LASTEXITCODE -ne 0 -or $version -notmatch '^synth \S+\+') {
        Fail "$exe --version after update is not a dev build"
    }

    # return to the release: uninstall runs the release's own code,
    # and the dev synth.exe is set aside in turn
    $updated = & $exe update
    $updated
    if ($LASTEXITCODE -ne 0 -or $updated -notmatch '^synth .* -> ') {
        Fail "$exe update back to stable failed"
    }
    $version = & $exe --version
    $version
    if ($LASTEXITCODE -ne 0 -or $version -notmatch '^synth [^+\s]+$') {
        Fail "$exe --version after update back is not a stable build"
    }
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
