param(
    [Parameter(Mandatory = $true)]
    [string]$AtlasExecutable,

    [Parameter(Mandatory = $true)]
    [string]$ArchiveName,

    [string]$OutputDirectory = "release-output"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$OllamaVersion = "0.30.8"
$OllamaArchiveUrl = "https://github.com/ollama/ollama/releases/download/v$OllamaVersion/ollama-windows-amd64.zip"
$OllamaArchiveSha256 = "c2d26d97e698027329c252629d7113bbc05d874b49960cbb03e93a39ae9fd95c"
$Model = "qwen2.5:1.5b-instruct-q4_K_M"
$ModelManifestSha256 = "65ec06548149b04c096a120e4a6da9d4017ea809c91734ea5631e89f96ddc57b"
$ModelBlobSha256 = "183715c435899236895da3869489cc30ac241476b4971a20285b1a462818a5b4"
$ModelBlobSize = 986048512L
$ModelLicenseSha256 = "832dd9e00a68dd83b3c3fb9f5588dad7dcf337a0db50f7d9483f310cd292e92e"

$RepositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..\..")).Path
$ExecutablePath = (Resolve-Path -LiteralPath $AtlasExecutable).Path
$OutputRoot = [IO.Path]::GetFullPath((Join-Path $RepositoryRoot $OutputDirectory))
$CacheRoot = if ($env:ATLAS_AI_CACHE) {
    [IO.Path]::GetFullPath($env:ATLAS_AI_CACHE)
} else {
    Join-Path $RepositoryRoot ".atlas-ai-cache"
}
$RuntimeCache = Join-Path $CacheRoot "ollama-$OllamaVersion-cpu-windows-x64"
$ModelsCache = Join-Path $CacheRoot "models"
$PortableRoot = Join-Path $env:RUNNER_TEMP "atlas-portable-$([guid]::NewGuid().ToString('N'))"
$DownloadPath = Join-Path $env:RUNNER_TEMP "ollama-$OllamaVersion-windows-x64.zip"
$ExtractRoot = Join-Path $env:RUNNER_TEMP "ollama-extract-$([guid]::NewGuid().ToString('N'))"
$Server = $null

function Assert-ChildPath {
    param([string]$Root, [string]$Target)
    $rootPath = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
    $targetPath = [IO.Path]::GetFullPath($Target)
    if (-not $targetPath.StartsWith($rootPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to modify a path outside $rootPath : $targetPath"
    }
}

function Remove-ValidatedTree {
    param([string]$Root, [string]$Target)
    if (Test-Path -LiteralPath $Target) {
        Assert-ChildPath -Root $Root -Target $Target
        Remove-Item -LiteralPath $Target -Recurse -Force
    }
}

function Test-ModelCache {
    $manifest = Join-Path $ModelsCache "manifests\registry.ollama.ai\library\qwen2.5\1.5b-instruct-q4_K_M"
    $modelBlob = Join-Path $ModelsCache "blobs\sha256-$ModelBlobSha256"
    $licenseBlob = Join-Path $ModelsCache "blobs\sha256-$ModelLicenseSha256"
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf) -or
        -not (Test-Path -LiteralPath $modelBlob -PathType Leaf) -or
        -not (Test-Path -LiteralPath $licenseBlob -PathType Leaf)) {
        return $false
    }
    if ((Get-Item -LiteralPath $modelBlob).Length -ne $ModelBlobSize) {
        return $false
    }
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $manifest).Hash.ToLowerInvariant() -eq $ModelManifestSha256
}

try {
    New-Item -ItemType Directory -Path $CacheRoot -Force | Out-Null

    if (-not (Test-Path -LiteralPath (Join-Path $RuntimeCache "ollama.exe") -PathType Leaf)) {
        Write-Host "Downloading verified Ollama $OllamaVersion Windows runtime"
        Invoke-WebRequest -Uri $OllamaArchiveUrl -OutFile $DownloadPath
        $runtimeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $DownloadPath).Hash.ToLowerInvariant()
        if ($runtimeHash -ne $OllamaArchiveSha256) {
            throw "Ollama archive checksum mismatch. Expected $OllamaArchiveSha256, received $runtimeHash."
        }
        New-Item -ItemType Directory -Path $ExtractRoot -Force | Out-Null
        Expand-Archive -LiteralPath $DownloadPath -DestinationPath $ExtractRoot

        foreach ($gpuDirectory in @("cuda_v12", "cuda_v13", "vulkan")) {
            Remove-ValidatedTree -Root $ExtractRoot -Target (Join-Path $ExtractRoot "lib\ollama\$gpuDirectory")
        }
        Remove-ValidatedTree -Root $CacheRoot -Target $RuntimeCache
        Move-Item -LiteralPath $ExtractRoot -Destination $RuntimeCache
        Remove-Item -LiteralPath $DownloadPath -Force
    }

    if (-not (Test-ModelCache)) {
        Write-Host "Downloading verified bundled model $Model"
        Remove-ValidatedTree -Root $CacheRoot -Target $ModelsCache
        New-Item -ItemType Directory -Path $ModelsCache -Force | Out-Null
        $serverEnvironment = @{
            OLLAMA_HOST = "127.0.0.1:11435"
            OLLAMA_MODELS = $ModelsCache
            OLLAMA_NOHISTORY = "1"
        }
        $Server = Start-Process -FilePath (Join-Path $RuntimeCache "ollama.exe") -ArgumentList "serve" -WorkingDirectory $RuntimeCache -WindowStyle Hidden -PassThru -Environment $serverEnvironment
        $ready = $false
        for ($attempt = 0; $attempt -lt 60; $attempt++) {
            try {
                Invoke-RestMethod -Uri "http://127.0.0.1:11435/api/tags" -TimeoutSec 2 | Out-Null
                $ready = $true
                break
            } catch {
                Start-Sleep -Seconds 1
            }
        }
        if (-not $ready) {
            throw "The temporary Ollama process did not become ready while packaging Atlas."
        }
        $pullBody = @{ model = $Model; stream = $false } | ConvertTo-Json -Compress
        $pull = Invoke-RestMethod -Uri "http://127.0.0.1:11435/api/pull" -Method Post -ContentType "application/json" -Body $pullBody -TimeoutSec 2700
        if ($pull.status -ne "success") {
            throw "Ollama did not report a successful model pull."
        }
        Stop-Process -Id $Server.Id -Force -ErrorAction SilentlyContinue
        $Server.WaitForExit()
        $Server = $null
    }

    if (-not (Test-ModelCache)) {
        throw "The bundled model failed its manifest and size verification."
    }
    $modelBlob = Join-Path $ModelsCache "blobs\sha256-$ModelBlobSha256"
    $actualModelHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $modelBlob).Hash.ToLowerInvariant()
    if ($actualModelHash -ne $ModelBlobSha256) {
        throw "The bundled model blob checksum is invalid."
    }

    New-Item -ItemType Directory -Path $PortableRoot -Force | Out-Null
    $aiRoot = Join-Path $PortableRoot "AtlasAI"
    New-Item -ItemType Directory -Path $aiRoot -Force | Out-Null
    Copy-Item -Path (Join-Path $RuntimeCache "*") -Destination $aiRoot -Recurse -Force
    Copy-Item -LiteralPath $ModelsCache -Destination $aiRoot -Recurse -Force
    New-Item -ItemType Directory -Path (Join-Path $aiRoot "licenses") -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $RepositoryRoot "docs\OLLAMA_LICENSE.txt") -Destination (Join-Path $aiRoot "licenses\Ollama-LICENSE.txt")
    Copy-Item -LiteralPath (Join-Path $ModelsCache "blobs\sha256-$ModelLicenseSha256") -Destination (Join-Path $aiRoot "licenses\Qwen2.5-LICENSE.txt")
    Copy-Item -LiteralPath (Join-Path $RepositoryRoot "docs\LOCAL_AI_NOTICES.md") -Destination (Join-Path $aiRoot "THIRD_PARTY_NOTICES.md")
    @(
        "Ollama=$OllamaVersion"
        "Model=$Model"
        "ModelManifestSha256=$ModelManifestSha256"
        "InferenceEndpoint=http://127.0.0.1:11435"
    ) | Set-Content -Encoding ascii -LiteralPath (Join-Path $aiRoot "BUILD_INFO.txt")

    Copy-Item -LiteralPath $ExecutablePath -Destination (Join-Path $PortableRoot "Atlas.exe")
    Copy-Item -LiteralPath (Join-Path $RepositoryRoot "docs\PORTABLE_README.md") -Destination (Join-Path $PortableRoot "README.md")

    New-Item -ItemType Directory -Path $OutputRoot -Force | Out-Null
    $archive = Join-Path $OutputRoot $ArchiveName
    if (Test-Path -LiteralPath $archive) {
        Remove-Item -LiteralPath $archive -Force
    }
    Compress-Archive -Path (Join-Path $PortableRoot "*") -DestinationPath $archive -CompressionLevel Fastest

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $entryNames = @($zip.Entries | ForEach-Object FullName)
        foreach ($required in @(
            "Atlas.exe",
            "README.md",
            "AtlasAI/ollama.exe",
            "AtlasAI/BUILD_INFO.txt",
            "AtlasAI/models/blobs/sha256-$ModelBlobSha256"
        )) {
            if ($entryNames -notcontains $required) {
                throw "Portable archive is missing $required"
            }
        }
    } finally {
        $zip.Dispose()
    }

    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash
    "$hash *$ArchiveName" | Set-Content -Encoding ascii -LiteralPath (Join-Path $OutputRoot "SHA256SUMS.txt")
    Write-Host "Created $archive with bundled Local AI"
} finally {
    if ($null -ne $Server -and -not $Server.HasExited) {
        Stop-Process -Id $Server.Id -Force -ErrorAction SilentlyContinue
    }
    Remove-ValidatedTree -Root $env:RUNNER_TEMP -Target $PortableRoot
    Remove-ValidatedTree -Root $env:RUNNER_TEMP -Target $ExtractRoot
    if (Test-Path -LiteralPath $DownloadPath) {
        Assert-ChildPath -Root $env:RUNNER_TEMP -Target $DownloadPath
        Remove-Item -LiteralPath $DownloadPath -Force
    }
}
