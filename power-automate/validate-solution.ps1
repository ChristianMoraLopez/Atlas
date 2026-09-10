param([string] $SolutionPath = (Join-Path $PSScriptRoot 'AtlasBridge_1_0_0_0.zip'))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $SolutionPath))
try {
    function Get-OptionalProperty($Object, [string] $Name) {
        if ($null -eq $Object) { return $null }
        $property = $Object.PSObject.Properties[$Name]
        if ($null -eq $property) { return $null }
        return $property.Value
    }
    $names = @($archive.Entries | ForEach-Object FullName)
    if (($names | Sort-Object -Unique).Count -ne $names.Count) { throw 'Duplicate ZIP entries.' }
    function Read-Entry([string] $Name) {
        $entry = $archive.GetEntry($Name)
        if (-not $entry -or $entry.Length -gt 1048576) { throw "Missing or oversized entry: $Name" }
        $reader = [IO.StreamReader]::new($entry.Open())
        try { $reader.ReadToEnd() } finally { $reader.Dispose() }
    }
    [xml] $solution = Read-Entry 'solution.xml'
    [xml] $custom = Read-Entry 'customizations.xml'
    [xml] $types = Read-Entry '[Content_Types].xml'
    if ($solution.ImportExportXml.SolutionManifest.UniqueName -ne 'AtlasBridge') { throw 'Unexpected solution identity.' }
    if ($solution.ImportExportXml.SolutionManifest.Managed -ne '0') { throw 'Expected unmanaged solution.' }
    if ($solution.ImportExportXml.SolutionManifest.Version -ne '1.0.0.0') { throw 'Unexpected version.' }
    $workflows = @($custom.ImportExportXml.Workflows.Workflow)
    if ($workflows.Count -ne 1 -or $workflows[0].Category -ne '5') { throw 'Expected one cloud flow.' }
    if ($workflows[0].WorkflowId -ne '{8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2}') { throw 'Flow identity changed; unsafe for retries.' }
    $workflowName = $workflows[0].JsonFileName.TrimStart('/')
    $flowText = Read-Entry $workflowName
    $flow = $flowText | ConvertFrom-Json
    $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot "solution-source/$workflowName") | ConvertFrom-Json
    if (($flow | ConvertTo-Json -Depth 100 -Compress) -ne ($source | ConvertTo-Json -Depth 100 -Compress)) { throw 'Packaged flow differs from reviewed sources.' }
    $expected = @{
        shared_office365 = 'atlas_office365'
        shared_teams = 'atlas_teams'
        shared_onedriveforbusiness = 'atlas_onedriveforbusiness'
    }
    $refs = @($custom.ImportExportXml.connectionreferences.connectionreference)
    if ($refs.Count -ne 3) { throw 'Expected three connection references.' }
    foreach ($api in $expected.Keys) {
        $logical = $expected[$api]
        $ref = @($refs | Where-Object connectionreferencelogicalname -EQ $logical)
        if ($ref.Count -ne 1 -or $ref[0].connectorid -ne "/providers/Microsoft.PowerApps/apis/$api") { throw "Invalid connection reference: $logical" }
        $connection = $flow.properties.connectionReferences.$api
        if ($connection.connection.connectionReferenceLogicalName -ne $logical -or $connection.api.name -ne $api) { throw 'Flow reference does not resolve.' }
        $connectionName = Get-OptionalProperty $connection 'connectionName'
        $connectionId = Get-OptionalProperty $connection.connection 'id'
        $nestedConnectionId = Get-OptionalProperty $connection.connection 'connectionId'
        if ($connectionName -or $connectionId -or $nestedConnectionId) { throw 'Connection instance found in distributable template.' }
    }
    $metadata = Get-OptionalProperty $flow.properties.definition 'metadata'
    $creator = Get-OptionalProperty $metadata 'creator'
    if ($creator -or $flowText -match 'shared-office365-atlas-template|tenantId|connectionId|clientSecret|access_token|refresh_token') { throw 'User-specific metadata or credentials found.' }
    function Validate-Actions($Actions) {
        foreach ($property in $Actions.PSObject.Properties) {
            $action = $property.Value
            if ($action.type -eq 'OpenApiConnection') {
                $alias = $action.inputs.host.connectionName
                if (-not $expected.ContainsKey($alias)) { throw 'Nonstandard or unknown connector.' }
                if ($action.inputs.host.apiId -ne "/providers/Microsoft.PowerApps/apis/$alias") { throw 'Connector identity mismatch.' }
            } elseif ($action.type -notin @('Compose','InitializeVariable','SetVariable','Scope','Select','Query','Foreach','AppendToArrayVariable','If','Terminate')) {
                throw "Unexpected action type: $($action.type)"
            }
            $nestedActions = Get-OptionalProperty $action 'actions'
            if ($null -ne $nestedActions) { Validate-Actions $nestedActions }
            $elseBranch = Get-OptionalProperty $action 'else'
            $elseActions = Get-OptionalProperty $elseBranch 'actions'
            if ($null -ne $elseActions) { Validate-Actions $elseActions }
        }
    }
    Validate-Actions $flow.properties.definition.actions
    $actions = $flow.properties.definition.actions
    foreach ($scope in @('Calendar_evidence', 'Mail_evidence', 'Teams_evidence')) {
        if (@($actions.Compose_Atlas_bundle.runAfter.$scope).Count -ne 1 -or $actions.Compose_Atlas_bundle.runAfter.$scope[0] -ne 'Succeeded') { throw 'New solution must verify all three sources.' }
    }
    if ($actions.Create_Atlas_evidence_file.inputs.parameters.folderPath -ne '/AtlasBridge/inbox') { throw 'Unexpected cloud inbox.' }
    if ($actions.Create_Atlas_evidence_file.inputs.parameters.name -notmatch 'AtlasInstallationId') { throw 'Missing installation correlation.' }
    if ($names.Count -ne 4) { throw 'Unexpected files in solution.' }
    Write-Host 'PASS: solution XML, cloud flow, three standard references, source parity, no user connection IDs, correlation and strict source completion.'
} finally { $archive.Dispose() }
