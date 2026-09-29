# Fetches a released Obelus and puts it somewhere on your PATH.
#
#   irm https://raw.githubusercontent.com/obelus-editor/obelus/master/contrib/install.ps1 | iex
#
# With options, which a pipe into `iex` has nowhere to put, download it
# first:
#
#   irm .../install.ps1 -OutFile install.ps1
#   .\install.ps1 -Binary ob
#   .\install.ps1 -NoShortcut
#   .\install.ps1 -Uninstall
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
    [string] $Directory = "$env:LOCALAPPDATA\Obelus\bin",

    # Take it away again.
    #
    # The other half of a script that writes somebody's PATH and their
    # Start menu: those two are the things a reader cannot simply delete,
    # so the script that made them is what unmakes them. Nothing is
    # fetched -- what is on the machine is the whole of what this is about.
    [switch] $Uninstall,

    # Leave the Start menu alone.
    #
    # For a machine nobody sits in front of -- a build agent, an image
    # being prepared -- where the binary is wanted and a menu entry for it
    # is a file somebody else has to notice and remove.
    [switch] $NoShortcut
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # or every download draws a bar over the output

$repository = 'obelus-editor/obelus'

# Where the Start menu entry goes, which both halves of this need to know.
$link = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Obelus.lnk'

if ($Uninstall) {
    $removed = 0

    foreach ($each in @('obg', 'ob')) {
        $exe = Join-Path $Directory "$each.exe"
        if (Test-Path -LiteralPath $exe) {
            Remove-Item -LiteralPath $exe -Force
            Write-Host "  removed $exe"
            $removed++
        }
    }

    # The shortcut, and only where it is this Obelus's. A `.lnk` of that
    # name pointing somewhere else is somebody else's -- a second install
    # in a directory of their own, most likely -- and a script that removed
    # it would be taking away what it never put there.
    if (Test-Path -LiteralPath $link) {
        $shell = New-Object -ComObject WScript.Shell
        $points = $shell.CreateShortcut($link).TargetPath
        if ($points -eq (Join-Path $Directory 'obg.exe')) {
            Remove-Item -LiteralPath $link -Force
            Write-Host "  removed $link"
            $removed++
        } else {
            Write-Host "  left $link alone: it points at $points"
        }
    }

    # And the directory with the PATH entry that named it, where it is
    # empty and so Obelus's alone. A reader who installed into a directory
    # of their own keeps both, because what a script may take back is what
    # it put there -- and `~/tools` with one thing gone out of it is not
    # something to remove from anybody's PATH.
    if ((Test-Path -LiteralPath $Directory) -and -not (Get-ChildItem -LiteralPath $Directory -Force)) {
        Remove-Item -LiteralPath $Directory -Force
        Write-Host "  removed $Directory"

        $theirs = [Environment]::GetEnvironmentVariable('Path', 'User')
        $was = @($theirs -split ';' | Where-Object { $_ })
        $kept = @($was | Where-Object { $_ -ne $Directory })
        if ($kept.Count -ne $was.Count) {
            [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
            Write-Host "  took $Directory out of your PATH. A new terminal will have it gone."
        }
    }

    if ($removed -eq 0) {
        Write-Host "There is no Obelus in $Directory to remove."
    }
    return
}

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

    # A Start menu entry for the window, which is the whole of what a
    # launcher is. The `.deb`, the `.rpm` and the AppImage carry a desktop
    # entry and an icon for exactly this; Windows has no package of
    # Obelus's to carry one, and this script is the way in -- so the
    # shortcut is the script's.
    #
    # The window alone. `ob` is a program for a terminal that is already
    # open, and a Start menu entry for it is a console window on a
    # directory nobody chose.
    if (-not $NoShortcut -and $binaries -contains 'obg') {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $link) | Out-Null

        # WScript.Shell, which is on a stock Windows and is the only way to
        # write a `.lnk` without a module nobody has. The icon comes with
        # the binary -- `obelus-gui` sets it in its own `build.rs` -- so
        # there is no second file to install and none to leave behind.
        $shell = New-Object -ComObject WScript.Shell
        $shortcut = $shell.CreateShortcut($link)
        $shortcut.TargetPath = Join-Path $Directory 'obg.exe'
        # Started from a menu there is no directory to have been in, and
        # Obelus opens on the one it was started in. Home is the answer a
        # reader can make sense of; the temp directory a shell hands out is
        # not.
        $shortcut.WorkingDirectory = $env:USERPROFILE
        $shortcut.Description = 'A code reader. It does not want you to type.'
        $shortcut.Save()
        Write-Host "  a Start menu entry: $link"
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
