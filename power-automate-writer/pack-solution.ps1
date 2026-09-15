param([string] $OutputPath = (Join-Path $PSScriptRoot 'AtlasTrackerWriter_1_0_0_0.zip'))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
$source = Join-Path $PSScriptRoot 'solution-source'
$files = @{
  'solution.xml' = Join-Path $source 'Other/Solution.xml'
  'customizations.xml' = Join-Path $source 'Other/Customizations.xml'
  'Workflows/AtlasWriteDailyTracker-4d6c39b7-cac8-4d19-a12e-95af49503b7f.json' = Join-Path $source 'Workflows/AtlasWriteDailyTracker-4d6c39b7-cac8-4d19-a12e-95af49503b7f.json'
}
$pending = "$OutputPath.pending"
if (Test-Path -LiteralPath $pending) { Remove-Item -LiteralPath $pending -Force }
$stream = [IO.File]::Open($pending,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
try {
  $zip = [IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Create,$true)
  try {
    foreach ($item in $files.GetEnumerator()) { $entry=$zip.CreateEntry($item.Key,[IO.Compression.CompressionLevel]::Optimal); $writer=[IO.StreamWriter]::new($entry.Open(),[Text.UTF8Encoding]::new($false)); try { $writer.Write((Get-Content -Raw -LiteralPath $item.Value)) } finally { $writer.Dispose() } }
    $entry=$zip.CreateEntry('[Content_Types].xml',[IO.Compression.CompressionLevel]::Optimal); $writer=[IO.StreamWriter]::new($entry.Open(),[Text.UTF8Encoding]::new($false)); try { $writer.Write('<?xml version="1.0" encoding="utf-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/octet-stream" /><Default Extension="json" ContentType="application/octet-stream" /></Types>') } finally { $writer.Dispose() }
  } finally { $zip.Dispose() }
} finally { $stream.Dispose() }
Move-Item -LiteralPath $pending -Destination $OutputPath -Force
Write-Host "Packed $OutputPath"
