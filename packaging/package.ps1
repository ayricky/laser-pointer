<#
.SYNOPSIS
Builds the Microsoft Store package: an MSIX per architecture, and a bundle when there are several.

.DESCRIPTION
The identity defaults to the one Partner Center assigned the Store app (Product management >
Product identity), so the bundle can be uploaded as it is.

Everything is written to dist/. Nothing is signed: the Store signs what it publishes. To try a
build on this PC, turn on Developer Mode and register its folder:
    Add-AppxPackage -Register dist\x64\AppxManifest.xml

.EXAMPLE
./packaging/package.ps1
./packaging/package.ps1 -Architectures x64,arm64
#>
param(
    [string[]] $Architectures = @('x64'),
    [string] $Name = 'ayricky.ScreenLaserPointer',
    [string] $Publisher = 'CN=16A8F02B-6B7E-43C2-A153-37AA3E4897B6',
    [string] $PublisherDisplayName = 'ayricky',
    # Package what's already in target/<triple>/release instead of building it.
    [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $root 'dist'
$triples = @{ x64 = 'x86_64-pc-windows-msvc'; arm64 = 'aarch64-pc-windows-msvc' }

# The Store wants four version fields, the last one 0.
$cargo = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^version = "(\d+\.\d+\.\d+)"') { throw 'No version in Cargo.toml' }
$version = "$($Matches[1]).0"

$sdk = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\10.*\x64\makeappx.exe' -ErrorAction SilentlyContinue |
    Sort-Object { [version]$_.Directory.Parent.Name } | Select-Object -Last 1
if (-not $sdk) { throw 'makeappx.exe not found: install the Windows SDK' }
$makeappx = $sdk.FullName
$makepri = Join-Path $sdk.DirectoryName 'makepri.exe'

function Invoke-Tool([string] $tool, [string[]] $arguments) {
    $output = & $tool @arguments 2>&1
    if ($LASTEXITCODE) { $output | Write-Host; throw "$(Split-Path $tool -Leaf) failed" }
}

if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
$packages = Join-Path $dist 'packages'
New-Item -ItemType Directory $packages | Out-Null

foreach ($architecture in $Architectures) {
    $triple = $triples[$architecture]
    if (-not $triple) { throw "Unknown architecture '$architecture': use x64 or arm64" }
    if (-not $SkipBuild) {
        cargo build --release --locked --target $triple --manifest-path (Join-Path $root 'Cargo.toml')
        if ($LASTEXITCODE) { throw "cargo build failed for $architecture" }
    }

    $layout = Join-Path $dist $architecture
    New-Item -ItemType Directory (Join-Path $layout 'Assets') | Out-Null
    Copy-Item (Join-Path $root "target\$triple\release\laser-pointer.exe") $layout
    Copy-Item (Join-Path $root 'LICENSE') $layout
    Copy-Item (Join-Path $PSScriptRoot 'Assets\*.png') (Join-Path $layout 'Assets')

    $manifest = Get-Content (Join-Path $PSScriptRoot 'AppxManifest.xml') -Raw
    $fields = @{
        Name = $Name; Publisher = $Publisher; PublisherDisplayName = $PublisherDisplayName
        Version = $version; Architecture = $architecture
    }
    foreach ($field in $fields.Keys) {
        $manifest = $manifest.Replace("{{$field}}", [Security.SecurityElement]::Escape($fields[$field]))
    }
    if ($manifest -match '\{\{\w+\}\}') { throw "Unfilled field $($Matches[0]) in the manifest" }
    $manifestPath = Join-Path $layout 'AppxManifest.xml'
    [IO.File]::WriteAllText($manifestPath, $manifest, [Text.UTF8Encoding]::new($false))

    # resources.pri lets Windows pick the right scale and size of each image, including the
    # unplated taskbar icons.
    $config = Join-Path $dist "priconfig-$architecture.xml"
    Invoke-Tool $makepri @('createconfig', '/cf', $config, '/dq', 'en-US', '/pv', '10.0.0', '/o')
    # One resources.pri with every scale in it: the default config would split scales into
    # resource packages, which only a bundle built from them could deliver.
    $xml = [xml](Get-Content $config -Raw)
    $packaging = $xml.SelectSingleNode('/resources/packaging')
    if ($packaging) { [void]$packaging.ParentNode.RemoveChild($packaging) }
    $xml.Save($config)
    Invoke-Tool $makepri @('new', '/pr', $layout, '/cf', $config, '/mn', $manifestPath, '/of', (Join-Path $layout 'resources.pri'), '/o')
    Remove-Item $config

    $package = Join-Path $packages "LaserPointer_${version}_$architecture.msix"
    Invoke-Tool $makeappx @('pack', '/d', $layout, '/p', $package, '/o')
    Write-Host "Packaged $package"
}

if ($Architectures.Count -gt 1) {
    $bundle = Join-Path $dist "LaserPointer_$version.msixbundle"
    Invoke-Tool $makeappx @('bundle', '/d', $packages, '/p', $bundle, '/bv', $version, '/o')
    Write-Host "Bundled $bundle"
}
