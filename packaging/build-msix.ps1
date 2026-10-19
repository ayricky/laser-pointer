[CmdletBinding()]
param(
    [ValidateNotNullOrEmpty()][string]$IdentityName = 'RicardoMariano.LaserPointer',
    [ValidateNotNullOrEmpty()][string]$Publisher = 'CN=LaserPointerDev',
    [ValidateNotNullOrEmpty()][string]$PublisherDisplayName = 'Ricardo Mariano',
    [string]$Version,
    [ValidateNotNullOrEmpty()][string]$TargetDir = 'target',
    [switch]$NoBuild,
    [string]$CertificatePath,
    [Security.SecureString]$CertificatePassword
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Native {
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$(Split-Path $Program -Leaf) failed with exit code $LASTEXITCODE."
    }
}

function Find-Sdk {
    $roots = @("${env:ProgramFiles(x86)}\Windows Kits\10")
    foreach ($registryPath in @('HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots')) {
        $installed = Get-ItemProperty -LiteralPath $registryPath -ErrorAction SilentlyContinue
        if ($installed -and $installed.PSObject.Properties['KitsRoot10']) { $roots += $installed.KitsRoot10 }
    }
    $candidates = @(foreach ($root in ($roots | Select-Object -Unique)) {
        Get-ChildItem -LiteralPath (Join-Path $root 'bin') -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^10\.\d+\.\d+\.\d+$' } |
            ForEach-Object {
                $directory = Join-Path $_.FullName 'x64'
                if (Test-Path -LiteralPath (Join-Path $directory 'makeappx.exe')) {
                    [PSCustomObject]@{ Version = [version]$_.Name; Directory = $directory }
                }
            }
    })
    $newest = $candidates | Sort-Object Version -Descending | Select-Object -First 1
    if (-not $newest) { throw 'Windows SDK not found. Install the Windows 10/11 SDK with makeappx, makepri and signtool.' }
    foreach ($tool in @('makeappx.exe', 'makepri.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $newest.Directory $tool))) { throw "Selected SDK is missing $tool`: $($newest.Directory)" }
    }
    return $newest.Directory
}

$repository = Split-Path $PSScriptRoot -Parent
Push-Location $repository
try {
    if (-not $Version) {
        $cargoManifest = Get-Content -LiteralPath 'Cargo.toml' -Raw
        if ($cargoManifest -notmatch '(?m)^version\s*=\s*"(\d+\.\d+\.\d+)"') { throw 'Cannot read the package version from Cargo.toml.' }
        $Version = "$($Matches[1]).0"
    }
    if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$') { throw 'Version must have four numeric parts, for example 1.1.0.0.' }
    $parts = @($Version.Split('.') | ForEach-Object { [uint64]$_ })
    if (@($parts | Where-Object { $_ -gt 65535 }).Count -or $parts[0] -eq 0) { throw 'MSIX version parts must be 0..65535, with a nonzero first part.' }
    if ($parts[3] -ne 0) { throw 'The Store reserves the fourth version part; use zero.' }
    if ($CertificatePassword -and -not $CertificatePath) { throw 'CertificatePassword requires CertificatePath.' }
    $sdk = Find-Sdk
    Write-Host "Using Windows SDK: $sdk"
    $target = [IO.Path]::GetFullPath((Join-Path $repository $TargetDir))
    if ([IO.Path]::IsPathRooted($TargetDir)) { $target = [IO.Path]::GetFullPath($TargetDir) }
    $lockedTarget = [IO.Path]::GetFullPath((Join-Path $repository 'target'))
    if (-not $NoBuild -and $target -eq $lockedTarget) {
        $running = @(Get-Process -Name 'laser-pointer' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $target 'release\laser-pointer.exe') })
        if ($running.Count) { throw 'target\release\laser-pointer.exe is running. Use -TargetDir target-codex to avoid the locked executable.' }
    }
    if (-not $NoBuild) {
        Invoke-Native 'cargo' @('build', '--release', '--target-dir', $target)
    }
    $executable = Join-Path $target 'release\laser-pointer.exe'
    if (-not (Test-Path -LiteralPath $executable)) { throw "Release executable not found: $executable. Build it first or omit -NoBuild." }
    $dist = [IO.Path]::GetFullPath((Join-Path $repository 'dist'))
    $stage = [IO.Path]::GetFullPath((Join-Path $dist 'msix\stage'))
    if (-not $stage.StartsWith($dist + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Staging path escaped dist.' }
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Copy-Item -LiteralPath $executable -Destination $stage
    foreach ($file in @('LICENSE', 'THIRD-PARTY-NOTICES.txt')) { Copy-Item -LiteralPath $file -Destination $stage }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Assets') -Destination $stage -Recurse
    $manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'AppxManifest.xml') -Raw
    foreach ($entry in @{ IdentityName = $IdentityName; Publisher = $Publisher; PublisherDisplayName = $PublisherDisplayName; Version = $Version }.GetEnumerator()) {
        $manifest = $manifest.Replace("{{$($entry.Key)}}", [Security.SecurityElement]::Escape($entry.Value))
    }
    $encoding = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, $encoding)
    $configuration = Join-Path $dist 'msix\priconfig.xml'
    Invoke-Native (Join-Path $sdk 'makepri.exe') @('createconfig', '/cf', $configuration, '/dq', 'en-US', '/o')
    $resourceConfiguration = [xml](Get-Content -LiteralPath $configuration -Raw)
    $resourceConfiguration.resources.RemoveChild($resourceConfiguration.resources.packaging) | Out-Null
    $resourceConfiguration.Save($configuration)
    Invoke-Native (Join-Path $sdk 'makepri.exe') @('new', '/pr', $stage, '/cf', $configuration, '/mn', (Join-Path $stage 'AppxManifest.xml'), '/of', (Join-Path $stage 'resources.pri'), '/o')
    $package = Join-Path $dist "LaserPointer_${Version}_x64.msix"
    Invoke-Native (Join-Path $sdk 'makeappx.exe') @('pack', '/d', $stage, '/p', $package, '/o')
    if ($CertificatePath) {
        $signTool = Join-Path $sdk 'signtool.exe'
        if (-not (Test-Path -LiteralPath $signTool)) { throw "Selected SDK is missing signtool.exe: $sdk" }
        $certificate = (Resolve-Path -LiteralPath $CertificatePath).Path
        $arguments = @('sign', '/fd', 'SHA256', '/f', $certificate)
        $passwordPointer = [IntPtr]::Zero
        try {
            if ($CertificatePassword) {
                $passwordPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($CertificatePassword)
                $arguments += @('/p', [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordPointer))
            }
            Invoke-Native $signTool ($arguments + @($package))
        } finally {
            $arguments = $null
            if ($passwordPointer -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordPointer) }
        }
    }
    Write-Host "Created $package"
} catch {
    throw "MSIX packaging failed: $($_.Exception.Message)"
} finally {
    Pop-Location
}
