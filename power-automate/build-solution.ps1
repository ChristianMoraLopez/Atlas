param([Parameter(Mandatory = $true)][string] $PacPath)
$ErrorActionPreference = 'Stop'
# Development/CI only. No auth, import, install or automatic tool download.
if (-not (Test-Path -LiteralPath $PacPath -PathType Leaf)) { throw 'Supply a developer-owned PAC executable.' }
& node (Join-Path $PSScriptRoot 'convert-solution.mjs')
if ($LASTEXITCODE -ne 0) { throw 'Solution conversion failed.' }
& $PacPath solution pack --folder (Join-Path $PSScriptRoot 'solution-source') --zipfile (Join-Path $PSScriptRoot 'AtlasBridge_1_0_0_0.zip') --packagetype Unmanaged
if ($LASTEXITCODE -ne 0) { throw 'Official solution packing failed.' }
& (Join-Path $PSScriptRoot 'validate-solution.ps1')
