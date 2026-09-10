// Development only. Converts the legacy definition; never authenticates to a tenant.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
const root = dirname(fileURLToPath(import.meta.url));
const id = '8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2';
const name = 'Atlas - Export evidence to OneDrive';
const file = `AtlasExportEvidence-${id}.json`;
const legacy = JSON.parse(readFileSync(join(root, `package-source/Microsoft.Flow/flows/${id}/definition.json`), 'utf8'));
const definition = legacy.properties.definition;
delete definition.metadata; // No creator, tenant, environment or connection instance from the template.
const a = definition.actions;
a.AtlasInstallationId = { type: 'Compose', inputs: 'unconfigured', runAfter: {} };
a.AtlasCalendarName = { type: 'Compose', inputs: 'Calendar', runAfter: { AtlasInstallationId: ['Succeeded'] } };
a.TargetDate.runAfter = { AtlasCalendarName: ['Succeeded'] };
const calendar = a.Calendar_evidence.actions;
calendar.Filter_calendar = { type: 'Query', inputs: {
  from: "@body('Get_calendars_V2')?['value']",
  where: "@or(empty(outputs('AtlasCalendarName')),equals(item()?['name'],outputs('AtlasCalendarName')))"
}, runAfter: { Get_calendars_V2: ['Succeeded'] } };
const validActions = {};
for (const key of ['Get_calendar_view_of_events_V3', 'Select_calendar_fields', 'Set_CalendarEvidence']) {
  validActions[key] = calendar[key]; delete calendar[key];
}
validActions.Get_calendar_view_of_events_V3.inputs.parameters.calendarId = "@first(body('Filter_calendar'))?['id']";
validActions.Get_calendar_view_of_events_V3.runAfter = {};
calendar.Require_unique_calendar = { type: 'If', expression: "@equals(length(body('Filter_calendar')),1)",
  actions: validActions, else: { actions: { Calendar_not_unique: { type: 'Terminate', inputs: {
    runStatus: 'Failed', runError: { code: 'AtlasCalendarNotUnique', message: 'Choose the exact calendar name in the Atlas installer and import its updated solution.' }
  }, runAfter: {} } } }, runAfter: { Filter_calendar: ['Succeeded'] } };
// A newly installed bridge must demonstrate all three connectors, not silently hide failures.
a.Compose_Atlas_bundle.runAfter.Mail_evidence = ['Succeeded'];
a.Compose_Atlas_bundle.runAfter.Teams_evidence = ['Succeeded'];
a.Create_Atlas_evidence_file.inputs.parameters.name = "@concat('atlas-evidence-',outputs('AtlasInstallationId'),'-',outputs('TargetDate'),'-',formatDateTime(utcNow(),'yyyyMMddTHHmmssZ'),'.json')";
const connectors = [ ['shared_office365', 'atlas_office365', 'Office 365 Outlook'], ['shared_teams', 'atlas_teams', 'Microsoft Teams'], ['shared_onedriveforbusiness', 'atlas_onedriveforbusiness', 'OneDrive for Business'] ];
const refs = Object.fromEntries(connectors.map(([api, logical]) => [api, {
  runtimeSource: 'embedded', connection: { connectionReferenceLogicalName: logical }, api: { name: api }
}]));
const flow = { properties: { connectionReferences: refs, definition, templateName: null }, schemaVersion: '1.0.0.0' };
mkdirSync(join(root, 'solution-source/Workflows'), { recursive: true });
writeFileSync(join(root, 'solution-source/Workflows', file), JSON.stringify(flow, null, 2) + '\n');
const solutionPath = join(root, 'solution-source/Other/Solution.xml');
let solution = readFileSync(solutionPath, 'utf8').replace(/<Version>.*?<\/Version>/, '<Version>1.0.0.0</Version>').replace(/<Managed>.*?<\/Managed>/, '<Managed>0</Managed>');
solution = solution.replace(/<RootComponents\s*\/>|<RootComponents>[\s\S]*?<\/RootComponents>/, `<RootComponents><RootComponent type="29" id="{${id}}" behavior="0" /></RootComponents>`);
writeFileSync(solutionPath, solution);
const fields = { JsonFileName: `/Workflows/${file}`, Type: 1, Subprocess: 0, Category: 5, Mode: 0, Scope: 4, OnDemand: 0, TriggerOnCreate: 0, TriggerOnDelete: 0, AsyncAutoDelete: 0, SyncWorkflowLogOnFailure: 0, StateCode: 0, StatusCode: 1, RunAs: 1, IsTransacted: 1, IntroducedVersion: '1.0.0.0', IsCustomizable: 1, BusinessProcessType: 0, IsCustomProcessingStepAllowedForOtherPublishers: 1, PrimaryEntity: 'none' };
writeFileSync(join(root, 'solution-source/Other/Customizations.xml'), `<?xml version="1.0" encoding="utf-8"?>
<ImportExportXml xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Entities/><Roles/><Workflows><Workflow WorkflowId="{${id}}" Name="${name}">
${Object.entries(fields).map(([key,value]) => `    <${key}>${value}</${key}>`).join('\n')}
    <LocalizedNames><LocalizedName languagecode="1033" description="${name}"/></LocalizedNames>
  </Workflow></Workflows>
  <FieldSecurityProfiles/><Templates/><EntityMaps/><EntityRelationships/><OrganizationSettings/><optionsets/><CustomControls/><SolutionPluginAssemblies/><EntityDataProviders/>
  <connectionreferences>${connectors.map(([api,logical,label]) => `
    <connectionreference connectionreferencelogicalname="${logical}"><connectionreferencedisplayname>${label}</connectionreferencedisplayname><connectorid>/providers/Microsoft.PowerApps/apis/${api}</connectorid><iscustomizable>1</iscustomizable><statecode>0</statecode><statuscode>1</statuscode></connectionreference>`).join('')}
  </connectionreferences><Languages><Language>1033</Language></Languages>
</ImportExportXml>\n`);
console.log('Converted solution sources without tenant credentials or connection instance IDs.');
const customPath = join(root, 'solution-source/Other/Customizations.xml');
const custom = readFileSync(customPath, 'utf8');
const workflowXml = custom.match(/<Workflow WorkflowId=[\s\S]*?<\/Workflow>/)[0];
writeFileSync(join(root, 'solution-source/Workflows', file + '.data.xml'), `<?xml version="1.0" encoding="utf-8"?>\n${workflowXml}\n`);
writeFileSync(customPath, custom.replace(/<Workflows>[\s\S]*?<\/Workflows>/, '<Workflows />'));
