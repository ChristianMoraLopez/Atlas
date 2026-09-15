param([string] $SolutionPath = (Join-Path $PSScriptRoot 'AtlasTrackerWriter_1_0_0_0.zip'))
$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.IO.Compression.FileSystem
$script=(Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'office-script/Atlas Write Tracker.osts') | ConvertFrom-Json)
if($script.body -notmatch 'timezoneOffsetMinutes' -or $script.body -notmatch 'offsetMinutes / 1440'){throw 'Office Script must preserve the Atlas local timezone in Excel serial dates.'}
$zip=[IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $SolutionPath))
try {
  $names=@($zip.Entries | ForEach-Object FullName); foreach($name in @('solution.xml','customizations.xml','[Content_Types].xml','Workflows/AtlasWriteDailyTracker-4d6c39b7-cac8-4d19-a12e-95af49503b7f.json')) { if($names -notcontains $name){throw "Missing $name"} }
  $entry=$zip.GetEntry('Workflows/AtlasWriteDailyTracker-4d6c39b7-cac8-4d19-a12e-95af49503b7f.json'); $reader=[IO.StreamReader]::new($entry.Open()); try{$flow=$reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()}
  if($flow.properties.state -ne 'Started'){throw 'Writer flow must be active.'}; if($flow.properties.definition.triggers.Check_Atlas_outbox.recurrence.interval -ne 5){throw 'Writer must poll the outbox every five minutes.'}
  $a=$flow.properties.definition.actions; if($a.Get_current_tracker_package.inputs.parameters.path -ne '/AtlasBridge/tracker-outbox/current.json'){throw 'Outbox path changed.'}; if($a.Get_writer_script_metadata.inputs.parameters.path -ne '/Documents/Office Scripts/Atlas Write Tracker.osts'){throw 'Office Script path changed.'}; if($a.Write_rows_with_Office_Script.inputs.host.operationId -ne 'RunScriptProdV2'){throw 'Writer must use the supported Office Script action.'}
  Write-Host 'PASS: active SharePoint tracker writer, serialized outbox, Office Script and three standard connection references.'
} finally {$zip.Dispose()}
