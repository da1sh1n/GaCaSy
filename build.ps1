<#
.SYNOPSIS
    Builds and signs all three Romzeta programs.

.DESCRIPTION
    One command for the whole thing. It does two jobs `cargo build` cannot:

      1. Makes sure a signing key exists. `listener/build.rs` refuses to
         compile a listener with no trust anchor, because such a listener
         would reject every cartridge in existence.

      2. Delegates to `xtask release`, which runs the four stages in the one
         order that works — build, sign, build the installer around the signed
         binaries, sign the installer. See SIGNING.md.

    Everything lands in target/release/.

.PARAMETER Clean
    Remove target/ before building.

.PARAMETER NoKeygen
    Fail rather than generating a dev key. For CI, where a key appearing out of
    nowhere should be an error.

.EXAMPLE
    .\build.ps1
.EXAMPLE
    .\build.ps1 -Clean
#>
[CmdletBinding()]
param(
    [switch]$Clean,
    [switch]$NoKeygen
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

function Invoke-Step {
    param([string]$Description, [scriptblock]$Command)
    Write-Host ">> $Description" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Description failed (exit $LASTEXITCODE)"
    }
}

# A listener is built to trust keys/romzeta.pub and keys/dev.pub, and needs at
# least one of them to exist. A fresh clone has neither: romzeta.pub arrives only
# with a published release, and dev.pub is gitignored because it is yours.
$anchors = @('keys/romzeta.pub', 'keys/dev.pub') | Where-Object { Test-Path $_ }
if (-not $anchors) {
    if ($NoKeygen) {
        throw "No trust anchor in keys/ and -NoKeygen was given. Run ``cargo run -p xtask -- keygen`` first."
    }
    Write-Host "No signing key yet — generating a dev key (once per machine)." -ForegroundColor Yellow
    Invoke-Step 'keygen' { cargo run -p xtask -- keygen }
} else {
    Write-Host "Trust anchors: $($anchors -join ', ')" -ForegroundColor DarkGray
}

if ($Clean) {
    Invoke-Step 'cargo clean' { cargo clean }
}

# Everything below here — including the build order — lives in xtask/src/release.rs.
Invoke-Step 'building and signing launcher, listener, installer' { cargo run -p xtask -- release }

Write-Host ''
Write-Host 'Done. Signed binaries are in target/release/:' -ForegroundColor Green
Get-ChildItem 'target/release' -Filter '*.exe' |
    Where-Object { $_.BaseName -in @('launcher', 'listener', 'installer') } |
    ForEach-Object { Write-Host ('  {0,-14} {1,10:N0} bytes' -f $_.Name, $_.Length) }
