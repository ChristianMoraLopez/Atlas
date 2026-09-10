param(
    [string] $OutputPath = (Join-Path $PSScriptRoot 'AtlasBridge_1_0_0_0.zip')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$source = Join-Path $PSScriptRoot 'solution-source'
$workflow = Get-ChildItem -LiteralPath (Join-Path $source 'Workflows') -Filter '*.json' -File
if ($workflow.Count -ne 1) { throw 'Expected one reviewed workflow JSON file.' }
$dataPath = "$($workflow.FullName).data.xml"
if (-not (Test-Path -LiteralPath $dataPath -PathType Leaf)) { throw 'Missing workflow XML metadata.' }

$customizations = Get-Content -Raw -LiteralPath (Join-Path $source 'Other/Customizations.xml')
$workflowData = Get-Content -Raw -LiteralPath $dataPath
$workflowData = $workflowData -replace '^\s*<\?xml[^>]*\?>\s*', ''
$customizations = $customizations -replace '<Workflows\s*/>', "<Workflows>$workflowData</Workflows>"

$absoluteOutput = [IO.Path]::GetFullPath($OutputPath)
$expectedRoot = [IO.Path]::GetFullPath($PSScriptRoot) + [IO.Path]::DirectorySeparatorChar
if (-not $absoluteOutput.StartsWith($expectedRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Output must stay inside the power-automate directory.'
}
$pending = "$absoluteOutput.pending"
if (Test-Path -LiteralPath $pending) { Remove-Item -LiteralPath $pending -Force }

$stream = [IO.File]::Open($pending, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
try {
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $true)
    try {
        function Add-TextEntry([string] $Name, [string] $Text) {
            $entry = $archive.CreateEntry($Name, [IO.Compression.CompressionLevel]::Optimal)
            $writer = [IO.StreamWriter]::new($entry.Open(), [Text.UTF8Encoding]::new($false))
            try { $writer.Write($Text) } finally { $writer.Dispose() }
        }
        Add-TextEntry 'solution.xml' (Get-Content -Raw -LiteralPath (Join-Path $source 'Other/Solution.xml'))
        Add-TextEntry 'customizations.xml' $customizations
        Add-TextEntry '[Content_Types].xml' '<?xml version="1.0" encoding="utf-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/octet-stream" /><Default Extension="json" ContentType="application/octet-stream" /></Types>'
        Add-TextEntry "Workflows/$($workflow.Name)" (Get-Content -Raw -LiteralPath $workflow.FullName)
    } finally { $archive.Dispose() }
} finally { $stream.Dispose() }

Move-Item -LiteralPath $pending -Destination $absoluteOutput -Force
Write-Host "Packed reviewed solution: $absoluteOutput"
