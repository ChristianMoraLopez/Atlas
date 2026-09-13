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
definition.triggers = {
  Every_15_minutes: { type: 'Recurrence', recurrence: { frequency: 'Minute', interval: 15 } }
};
const a = definition.actions;
a.AtlasInstallationId = { type: 'Compose', inputs: 'unconfigured', runAfter: {} };
a.TargetDate.runAfter = { AtlasInstallationId: ['Succeeded'] };
const calendar = a.Calendar_evidence.actions;
calendar.Get_calendar_view_of_events_V3.inputs.parameters.calendarId = "@first(body('Get_calendars_V2')?['value'])?['id']";
// Keep connector calls bounded; broad mailbox/chat scans regularly hit the two-minute Logic Apps HTTP limit.
a.Mail_evidence.actions.Get_emails_V3.inputs.parameters.top = 100;
a.Teams_evidence.actions.For_each_chat.foreach = "@take(body('List_chats')?['value'],30)";
a.Teams_evidence.actions.For_each_chat.actions.Get_messages_in_chat.inputs.parameters['$filter'] = "@concat('lastModifiedDateTime gt ',outputs('StartUtc'),' and lastModifiedDateTime lt ',outputs('EndUtc'))";
a.Teams_evidence.actions.For_each_chat.actions.Get_messages_in_chat.inputs.parameters['$orderby'] = 'lastModifiedDateTime desc';
a.Teams_evidence.actions.For_each_chat.actions.Get_messages_in_chat.inputs.parameters['$top'] = 20;
calendar.Set_CalendarSourceReady = { type: 'SetVariable', inputs: { name: 'CalendarSourceReady', value: true }, runAfter: { Set_CalendarEvidence: ['Succeeded'] } };
a.Mail_evidence.actions.Set_MailSourceReady = { type: 'SetVariable', inputs: { name: 'MailSourceReady', value: true }, runAfter: { Set_MailEvidence: ['Succeeded'] } };
a.Teams_evidence.actions.Set_TeamsSourceReady = { type: 'SetVariable', inputs: { name: 'TeamsSourceReady', value: true }, runAfter: { For_each_chat: ['Succeeded'] } };

const statusVariables = [
  ['Initialize_CalendarSourceReady', 'CalendarSourceReady', 'Initialize_TeamsEvidence'],
  ['Initialize_MailSourceReady', 'MailSourceReady', 'Initialize_CalendarSourceReady'],
  ['Initialize_TeamsSourceReady', 'TeamsSourceReady', 'Initialize_MailSourceReady'],
];
for (const [key, variable, after] of statusVariables) {
  a[key] = { type: 'InitializeVariable', inputs: { variables: [{ name: variable, type: 'boolean', value: false }] }, runAfter: { [after]: ['Succeeded'] } };
}
for (const scope of ['Calendar_evidence', 'Mail_evidence', 'Teams_evidence']) {
  a[scope].runAfter = { Initialize_TeamsSourceReady: ['Succeeded'] };
}
const terminalStates = ['Succeeded', 'Failed', 'Skipped', 'TimedOut'];
a.Compose_Atlas_bundle.inputs = {
  schemaVersion: 2,
  exportedAt: '@utcNow()',
  targetDate: "@outputs('TargetDate')",
  sources: {
    calendar: "@variables('CalendarSourceReady')",
    mail: "@variables('MailSourceReady')",
    teams: "@variables('TeamsSourceReady')"
  },
  calendar: "@variables('CalendarEvidence')",
  mail: "@variables('MailEvidence')",
  teams: "@variables('TeamsEvidence')"
};
a.Compose_Atlas_bundle.runAfter = Object.fromEntries(['Calendar_evidence', 'Mail_evidence', 'Teams_evidence'].map(scope => [scope, terminalStates]));
a.Create_Atlas_evidence_file.inputs.parameters.name = "@concat('atlas-evidence-',outputs('AtlasInstallationId'),'-',outputs('TargetDate'),'-',formatDateTime(utcNow(),'yyyyMMddTHHmmssZ'),'.json')";
const connectors = [ ['shared_office365', 'atlas_office365', 'Office 365 Outlook'], ['shared_teams', 'atlas_teams', 'Microsoft Teams'], ['shared_onedriveforbusiness', 'atlas_onedriveforbusiness', 'OneDrive for Business'] ];
const refs = Object.fromEntries(connectors.map(([api, logical]) => [api, {
  runtimeSource: 'embedded', connection: { connectionReferenceLogicalName: logical }, api: { name: api }
}]));
const flow = { properties: { connectionReferences: refs, definition, templateName: null }, schemaVersion: '1.0.0.0' };
mkdirSync(join(root, 'solution-source/Workflows'), { recursive: true });
writeFileSync(join(root, 'solution-source/Workflows', file), JSON.stringify(flow, null, 2) + '\n');
const solutionPath = join(root, 'solution-source/Other/Solution.xml');
let solution = readFileSync(solutionPath, 'utf8').replace(/<Version>.*?<\/Version>/, '<Version>1.5.0.0</Version>').replace(/<Managed>.*?<\/Managed>/, '<Managed>0</Managed>');
solution = solution.replace(/<RootComponents\s*\/>|<RootComponents>[\s\S]*?<\/RootComponents>/, `<RootComponents><RootComponent type="29" id="{${id}}" behavior="0" /></RootComponents>`);
writeFileSync(solutionPath, solution);
const fields = { JsonFileName: `/Workflows/${file}`, Type: 1, Subprocess: 0, Category: 5, Mode: 0, Scope: 4, OnDemand: 0, TriggerOnCreate: 0, TriggerOnDelete: 0, AsyncAutoDelete: 0, SyncWorkflowLogOnFailure: 0, StateCode: 1, StatusCode: 2, RunAs: 1, IsTransacted: 1, IntroducedVersion: '1.2.0.0', IsCustomizable: 1, BusinessProcessType: 0, IsCustomProcessingStepAllowedForOtherPublishers: 1, PrimaryEntity: 'none' };
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
