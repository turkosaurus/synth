# Installs the latest stable synth for this machine, from PowerShell:
#
#   irm https://raw.githubusercontent.com/turkosaurus/synth/main/install.ps1 | iex
#
# Downloads the release archive, checks it against the sha256 digest GitHub
# recorded on upload, and runs `synth install`, which copies the binary to
# $env:XDG_BIN_HOME or %USERPROFILE%\.local\bin. Arguments are passed to
# `synth install`. Needs Windows PowerShell 5.1 or PowerShell 7, and the
# tar.exe that Windows 10 1803 and later ship.

# iex runs this in the caller's session. The script block keeps variables
# and preferences out of it, and nothing calls exit, which would close it.
& {
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell redraws its progress bar per chunk, slowing downloads
    $ProgressPreference = 'SilentlyContinue'

    $repo = 'turkosaurus/synth'
    $api = "https://api.github.com/repos/$repo/releases/latest"
    $downloads = "https://github.com/$repo/releases/download"
    $bin = 'synth.exe'
    $assetPrefix = 'synth-'
    $assetSuffix = '.tar.gz'
    $digestPrefix = 'sha256:'
    # the release target built for each PROCESSOR_ARCHITECTURE
    $windowsAmd64 = 'x86_64-pc-windows-msvc'

    function Fail([string]$message) {
        throw "install.ps1: $message"
    }

    # HTTPS only, redirects included. Windows PowerShell's response names
    # the final URI; PowerShell 7's names the request that reached it.
    function Get-Https([string]$uri, [string]$outFile) {
        $response = Invoke-WebRequest -Uri $uri -OutFile $outFile `
            -PassThru -UseBasicParsing
        $base = $response.BaseResponse
        $final = if ($base.PSObject.Properties['ResponseUri']) {
            $base.ResponseUri
        } else {
            $base.RequestMessage.RequestUri
        }
        if ($final.Scheme -ne 'https') {
            throw "redirected to $final, not HTTPS"
        }
    }

    $windows = $PSVersionTable.PSEdition -eq 'Desktop' -or $IsWindows
    if (-not $windows) {
        Fail 'releases for Linux and macOS install with install.sh'
    }
    # a 32-bit PowerShell on 64-bit Windows reports x86 here
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) {
        $arch = $env:PROCESSOR_ARCHITECTURE
    }
    switch ($arch) {
        'AMD64' { $target = $windowsAmd64 }
        default { Fail "no release is built for Windows $arch" }
    }
    $asset = "$assetPrefix$target$assetSuffix"

    # Windows' own bsdtar: Git's tar, often first on PATH, reads C: as a host
    $tar = Join-Path $env:SystemRoot 'System32\tar.exe'
    if (-not (Test-Path -LiteralPath $tar)) {
        Fail 'needs tar.exe, which Windows 10 1803 and later ship'
    }

    # Windows PowerShell may offer only TLS 1.0, which GitHub refuses.
    # The session's protocols are restored on the way out.
    $protocols = [Net.ServicePointManager]::SecurityProtocol
    $tls12 = [Net.SecurityProtocolType]::Tls12
    $tmp = Join-Path ([IO.Path]::GetTempPath()) "synth-install-$([guid]::NewGuid())"
    try {
        if ($protocols -ne 0 -and -not ($protocols -band $tls12)) {
            [Net.ServicePointManager]::SecurityProtocol = $protocols -bor $tls12
        }
        New-Item -ItemType Directory -Path $tmp | Out-Null

        $json = Join-Path $tmp 'release.json'
        try {
            Get-Https $api $json
        } catch {
            Fail "cannot read the latest release: $_"
        }
        # ReadAllText reads UTF-8; Windows PowerShell's Get-Content would not
        $release = [IO.File]::ReadAllText($json) | ConvertFrom-Json
        $tag = $release.tag_name
        if (-not $tag) {
            Fail 'the latest release has no tag'
        }
        $entry = @($release.assets | Where-Object { $_.name -eq $asset })
        $digest = if ($entry.Count -gt 0) { "$($entry[0].digest)" } else { '' }
        if (-not $digest.StartsWith($digestPrefix)) {
            Fail "$tag has no sha256 digest for $asset"
        }
        $want = $digest.Substring($digestPrefix.Length)

        $archive = Join-Path $tmp $asset
        try {
            Get-Https "$downloads/$tag/$asset" $archive
        } catch {
            Fail "cannot download $asset from ${tag}: $_"
        }
        $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
        if ($got -ne $want) {
            Fail "$asset sha256 is $got, expected $want"
        }

        & $tar -xzf $archive -C $tmp
        if ($LASTEXITCODE -ne 0) {
            Fail "cannot unpack $asset"
        }
        & (Join-Path $tmp $bin) install @args
        if ($LASTEXITCODE -ne 0) {
            Fail "synth install exited with $LASTEXITCODE"
        }
    } finally {
        [Net.ServicePointManager]::SecurityProtocol = $protocols
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
} @args
