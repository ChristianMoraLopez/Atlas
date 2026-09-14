param([string] $SolutionPath = (Join-Path $PSScriptRoot 'AtlasBridge_1_0_0_0.zip'))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem

$scheduledId = '{8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2}'
$eventId = '{7f7115e8-b821-4bb9-9b4e-5c7a5448610e}'
$expected = @{
    shared_office365 = 'atlas_office365'
    shared_teams = 'atlas_teams'
    shared_onedriveforbusiness = 'atlas_onedriveforbusiness'
}

function Get-OptionalProperty($Object, [string] $Name) {
    if ($null -eq $Object) { return $null }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Validate-Actions($Actions) {
    foreach ($property in $Actions.PSObject.Properties) {
        $action = $property.Value
        if ($action.type -eq 'OpenApiConnection') {
            $alias = $action.inputs.host.connectionName
            if (-not $expected.ContainsKey($alias)) { throw 'Nonstandard or unknown connector.' }
            if ($action.inputs.host.apiId -ne "/providers/Microsoft.PowerApps/apis/$alias") { throw 'Connector identity mismatch.' }
        } elseif ($action.type -notin @('Compose','InitializeVariable','SetVariable','Scope','Select','Query','Foreach','AppendToArrayVariable','Wait')) {
            throw "Unexpected action type: $($action.type)"
        }
        $nestedActions = Get-OptionalProperty $action 'actions'
        if ($null -ne $nestedActions) { Validate-Actions $nestedActions }
        $elseBranch = Get-OptionalProperty $action 'else'
        $elseActions = Get-OptionalProperty $elseBranch 'actions'
        if ($null -ne $elseActions) { Validate-Actions $elseActions }
    }
}

$archive = [IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $SolutionPath))
try {
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
    if ($solution.ImportExportXml.SolutionManifest.Version -ne '1.7.0.0') { throw 'Unexpected version.' }

    $rootIds = @($solution.ImportExportXml.SolutionManifest.RootComponents.RootComponent | ForEach-Object id | Sort-Object)
    if (($rootIds -join ',') -ne ((@($scheduledId, $eventId) | Sort-Object) -join ',')) { throw 'Unexpected solution root components.' }
    $workflows = @($custom.ImportExportXml.Workflows.Workflow)
    if ($workflows.Count -ne 2) { throw 'Expected the scheduled and Teams event cloud flows.' }
    foreach ($workflow in $workflows) {
        if ($workflow.Category -ne '5') { throw 'Expected cloud flow workflow metadata.' }
        if ($workflow.StateCode -ne '1' -or $workflow.StatusCode -ne '2') { throw 'Cloud flows must be exported as active.' }
    }
    $workflowIds = @($workflows | ForEach-Object WorkflowId | Sort-Object)
    if (($workflowIds -join ',') -ne ((@($scheduledId, $eventId) | Sort-Object) -join ',')) { throw 'Flow identities changed; unsafe for retries.' }

    $refs = @($custom.ImportExportXml.connectionreferences.connectionreference)
    if ($refs.Count -ne 3) { throw 'Expected three connection references.' }
    foreach ($api in $expected.Keys) {
        $logical = $expected[$api]
        $ref = @($refs | Where-Object connectionreferencelogicalname -EQ $logical)
        if ($ref.Count -ne 1 -or $ref[0].connectorid -ne "/providers/Microsoft.PowerApps/apis/$api") { throw "Invalid connection reference: $logical" }
    }

    $flows = @{}
    foreach ($workflow in $workflows) {
        $workflowName = $workflow.JsonFileName.TrimStart('/')
        $flowText = Read-Entry $workflowName
        $flow = $flowText | ConvertFrom-Json
        $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot "solution-source/$workflowName") | ConvertFrom-Json
        if (($flow | ConvertTo-Json -Depth 100 -Compress) -ne ($source | ConvertTo-Json -Depth 100 -Compress)) { throw "Packaged flow differs from reviewed source: $workflowName" }
        foreach ($api in $expected.Keys) {
            $logical = $expected[$api]
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
        Validate-Actions $flow.properties.definition.actions
        $flows[$workflow.WorkflowId] = $flow
    }

    $scheduledFlow = $flows[$scheduledId]
    $actions = $scheduledFlow.properties.definition.actions
    $triggers = @($scheduledFlow.properties.definition.triggers.PSObject.Properties)
    if ($triggers.Count -ne 1 -or $triggers[0].Name -ne 'Every_15_minutes' -or $triggers[0].Value.type -ne 'Recurrence' -or $triggers[0].Value.recurrence.frequency -ne 'Minute' -or $triggers[0].Value.recurrence.interval -ne 15) { throw 'Expected a portable 15-minute recurrence trigger.' }
    if ($triggers[0].Value.runtimeConfiguration.concurrency.runs -ne 1) { throw 'Recurring runs must not overlap.' }
    if ($actions.Compose_Atlas_bundle.inputs.schemaVersion -ne 2) { throw 'Expected evidence contract v2.' }
    foreach ($sourceName in @('calendar', 'mail', 'teams')) {
        if (-not $actions.Compose_Atlas_bundle.inputs.sources.$sourceName) { throw "Missing source health flag: $sourceName" }
    }
    foreach ($scope in @('Calendar_evidence', 'Mail_evidence', 'Teams_evidence')) {
        $states = @($actions.Compose_Atlas_bundle.runAfter.$scope | Sort-Object)
        if (($states -join ',') -ne 'Failed,Skipped,Succeeded,TimedOut') { throw 'Bundle must survive a partial connector failure.' }
    }
    if ($actions.Mail_evidence.actions.Get_emails_V3.inputs.parameters.top -gt 100) { throw 'Mail query is too broad for the connector timeout.' }
    $teamsLoop = $actions.Teams_evidence.actions.For_each_chat
    if ($teamsLoop.foreach -ne "@body('List_chats')?['value']") { throw 'Scheduled fallback must scan every recent chat returned by the connector.' }
    $teamsPace = $teamsLoop.actions.Pace_Teams_requests
    if ($teamsPace.type -ne 'Wait' -or $teamsPace.inputs.interval.count -lt 5 -or $teamsPace.inputs.interval.count -gt 30 -or $teamsPace.inputs.interval.unit -ne 'Second') { throw 'Scheduled Teams wait must respect the 5-30 second Power Automate interval.' }
    $teamsRequest = $teamsLoop.actions.Get_messages_in_chat.inputs.parameters
    if ($teamsRequest.'$top' -gt 20 -or $teamsRequest.'$filter' -notmatch 'lastModifiedDateTime.+StartUtc.+lastModifiedDateTime.+EndUtc' -or $teamsRequest.'$orderby' -ne 'lastModifiedDateTime desc') { throw 'Scheduled Teams query must use the bounded date filter.' }
    $teamsRetry = $teamsLoop.actions.Get_messages_in_chat.inputs.retryPolicy
    if ($teamsRetry.type -ne 'exponential' -or $teamsRetry.count -lt 4 -or $teamsRetry.minimumInterval -ne 'PT5S') { throw 'Scheduled Teams requests must retry transient failures.' }
    if ($actions.Create_Atlas_evidence_file.inputs.parameters.folderPath -ne '/AtlasBridge/inbox' -or $actions.Create_Atlas_evidence_file.inputs.parameters.name -notmatch 'AtlasInstallationId') { throw 'Invalid scheduled cloud inbox output.' }

    $eventFlow = $flows[$eventId]
    $eventTriggers = @($eventFlow.properties.definition.triggers.PSObject.Properties)
    if ($eventTriggers.Count -ne 1 -or $eventTriggers[0].Name -ne 'When_a_new_chat_message_is_added') { throw 'Missing Teams chat event trigger.' }
    $eventTrigger = $eventTriggers[0].Value
    if ($eventTrigger.type -ne 'OpenApiConnectionWebhook' -or $eventTrigger.inputs.host.operationId -ne 'WebhookChatMessageTrigger' -or $eventTrigger.inputs.host.connectionName -ne 'shared_teams' -or $eventTrigger.inputs.host.apiId -ne '/providers/Microsoft.PowerApps/apis/shared_teams') { throw 'Unexpected Teams event trigger.' }
    if ($eventTrigger.runtimeConfiguration.concurrency.runs -ne 1) { throw 'Teams event runs must not overlap.' }
    $eventActions = $eventFlow.properties.definition.actions
    $eventLoop = $eventActions.For_each_notification
    if ($eventLoop.foreach -ne "@triggerBody()?['value']" -or $eventLoop.runtimeConfiguration.concurrency.repetitions -ne 1) { throw 'Teams notifications must be processed sequentially.' }
    $eventRequest = $eventLoop.actions.Get_messages_for_triggered_chat
    if ($eventRequest.inputs.host.operationId -ne 'GetMessagesFromChat' -or $eventRequest.inputs.parameters.chatId -notmatch 'conversationId') { throw 'Event flow must query the triggered chat directly.' }
    if ($eventRequest.inputs.parameters.'$top' -gt 20 -or $eventRequest.inputs.parameters.'$filter' -notmatch 'lastModifiedDateTime.+StartUtc.+lastModifiedDateTime.+EndUtc') { throw 'Event Teams query must be bounded to the target day.' }
    if ($eventRequest.inputs.retryPolicy.type -ne 'exponential' -or $eventRequest.inputs.retryPolicy.count -lt 4) { throw 'Event Teams requests must retry transient failures.' }
    $eventBundle = $eventLoop.actions.Compose_Teams_event_bundle.inputs
    if ($eventBundle.schemaVersion -ne 2 -or -not $eventBundle.sources.teams -or $eventBundle.sources.calendar -or $eventBundle.sources.mail) { throw 'Event flow must report Teams-only evidence.' }
    $eventOutput = $eventLoop.actions.Create_Teams_event_evidence_file.inputs.parameters
    if ($eventOutput.folderPath -ne '/AtlasBridge/inbox' -or $eventOutput.name -notmatch 'AtlasInstallationId' -or $eventOutput.name -notmatch 'guid\(\)') { throw 'Invalid event cloud inbox output.' }

    if ($names.Count -ne 5) { throw 'Unexpected files in solution.' }
    Write-Host 'PASS: two active cloud flows, event-driven Teams capture, bounded fallback, three standard references, source parity and no user credentials.'
} finally {
    $archive.Dispose()
}
