# Fetches a released Obelus and puts it somewhere on your PATH.
#
#   irm https://raw.githubusercontent.com/sunli829/obelus/master/contrib/install.ps1 | iex
#
# With options, which a pipe into `iex` has nowhere to put, download it
# first:
#
#   irm .../install.ps1 -OutFile install.ps1
#   .\install.ps1 -Binary ob
#
# It verifies what it downloaded against the release's own SHA256SUMS. A
# script that pipes into a shell and then installs an unchecked binary has
# asked for trust twice and earned it once.

[CmdletBinding()]
param(
    # Which to install. `obg` is the window and `ob` is the terminal.
    #
    # The window by default, because that is the Obelus to meet first: the
    # presses a terminal cannot report arrive as themselves, and the marks
    # are in the binary rather than guessed at from somebody else's font.
    [ValidateSet('obg', 'ob', 'both')]
    [string] $Binary = 'obg',

    # A release to install. The default is the latest.
    [string] $Version,

    # Where to put it.
    [string] $Directory = "$env:LOCALAPPDATA\Obelus\bin"
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # or every download draws a bar over the output

$repository = 'sunli829/obelus'

# Windows PowerShell 5.1 is what a stock Windows opens, and `irm | iex` is
# how this is meant to be run, so nothing here may be PowerShell 7's alone.
# That rules out -SkipHttpErrorCheck, and RuntimeInformation, which needs a
# .NET older machines do not have.
if ([Net.ServicePointManager]::SecurityProtocol -notmatch 'Tls12') {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
}

# Which build. The archives are named by the target triple, so this is the
# whole of the platform detection. The environment's own answer, because it
# is the one every version of Windows has.
$target = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x86_64-pc-windows-msvc' }
    'ARM64' { 'aarch64-pc-windows-msvc' }
    default { throw "there is no Windows build for $env:PROCESSOR_ARCHITECTURE" }
}

# The latest release. Asked of the API rather than of the redirect, which is
# how install.sh asks: reading a Location header without following it is
# spelt differently in the two PowerShells, and this is spelt once.
if (-not $Version) {
    # Caught, because $ErrorActionPreference is Stop and a repository with
    # no releases answers 404: without this the reader gets PowerShell's own
    # page of red instead of the one sentence that says what happened.
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repository/releases/latest" -Headers @{ 'User-Agent' = 'obelus-install' }
        $Version = $release.tag_name
    } catch {
        $Version = $null
    }
    if (-not $Version) { throw 'there is no published release yet' }
}

$binaries = if ($Binary -eq 'both') { @('ob', 'obg') } else { @($Binary) }

Write-Host "Obelus $Version for $target"

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("obelus-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null

try {
    $sums = Join-Path $work 'SHA256SUMS'
    Invoke-WebRequest -Uri "https://github.com/$repository/releases/download/$Version/SHA256SUMS" -OutFile $sums

    # Name to sum, compared as a name: a version has dots in it, and a dot
    # in a pattern is any character.
    $said = @{}
    foreach ($line in Get-Content $sums) {
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2) { $said[$parts[1].TrimStart('*').Trim()] = $parts[0] }
    }

    New-Item -ItemType Directory -Force -Path $Directory | Out-Null

    foreach ($each in $binaries) {
        $name = "$each-$($Version.TrimStart('v'))-$target"
        $archive = Join-Path $work "$name.zip"

        Write-Host "  fetching $name.zip"
        Invoke-WebRequest -Uri "https://github.com/$repository/releases/download/$Version/$name.zip" -OutFile $archive

        $wanted = $said["$name.zip"]
        if (-not $wanted) { throw "SHA256SUMS says nothing about $name.zip" }
        $got = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLower()
        if ($got -ne $wanted.ToLower()) {
            throw "$name.zip is not what the release says it is: $got, where SHA256SUMS says $wanted"
        }

        Expand-Archive -Path $archive -DestinationPath $work -Force
        Copy-Item -Path (Join-Path $work "$name\$each.exe") -Destination (Join-Path $Directory "$each.exe") -Force
        Write-Host "  installed $Directory\$each.exe"
    }

    # The user's own PATH, not the machine's: this installs into their
    # profile and asking for an administrator to do it would be asking for
    # one thing too many.
    $theirs = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($theirs -split ';') -notcontains $Directory) {
        [Environment]::SetEnvironmentVariable('Path', "$theirs;$Directory", 'User')
        Write-Host ""
        Write-Host "$Directory was added to your PATH. A new terminal will have it."
    }
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
