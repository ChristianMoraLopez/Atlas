$ErrorActionPreference = "Stop"

$sourceRoot = Join-Path $PSScriptRoot "package-source"
$archivePath = Join-Path $PSScriptRoot "Atlas-Export-Evidence.zip"
$flowPath = "Microsoft.Flow/flows/8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2"

if (-not (Test-Path -LiteralPath $sourceRoot -PathType Container)) {
    throw "Package source directory is missing: $sourceRoot"
}

$jsonFiles = Get-ChildItem -LiteralPath $sourceRoot -Filter "*.json" -File -Recurse
foreach ($jsonFile in $jsonFiles) {
    Get-Content -Raw -LiteralPath $jsonFile.FullName | ConvertFrom-Json | Out-Null
}

if (Test-Path -LiteralPath $archivePath -PathType Leaf) {
    Remove-Item -LiteralPath $archivePath -Force
}
Compress-Archive -Path (Join-Path $sourceRoot "*") -DestinationPath $archivePath -CompressionLevel Optimal

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
try {
    $entries = @($archive.Entries | ForEach-Object { $_.FullName.Replace("\", "/") })
    foreach ($required in @(
        "manifest.json",
        "Microsoft.Flow/flows/manifest.json",
        "$flowPath/apisMap.json",
        "$flowPath/connectionsMap.json",
        "$flowPath/definition.json"
    )) {
        if ($entries -notcontains $required) {
            throw "Generated package is missing $required"
        }
    }
} finally {
    $archive.Dispose()
}

Write-Host "Created $archivePath"
