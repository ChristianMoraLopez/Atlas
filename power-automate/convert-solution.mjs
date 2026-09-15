// Development only. Builds reviewed solution sources; never authenticates to a tenant.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';

const root = dirname(fileURLToPath(import.meta.url));
const version = '1.10.0.0';
const scheduled = {
  id: '8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2',
  name: 'Atlas - Export evidence to OneDrive',
  file: 'AtlasExportEvidence-8e5c1f84-dcbb-4a2c-9d2f-62e9c38105d2.json',
};
const eventCapture = {
  id: '7f7115e8-b821-4bb9-9b4e-5c7a5448610e',
  name: 'Atlas - Capture Teams messages',
  file: 'AtlasCaptureTeamsMessages-7f7115e8-b821-4bb9-9b4e-5c7a5448610e.json',
};
const connectors = [
  ['shared_office365', 'atlas_office365', 'Office 365 Outlook'],
  ['shared_teams', 'atlas_teams', 'Microsoft Teams'],
  ['shared_onedriveforbusiness', 'atlas_onedriveforbusiness', 'OneDrive for Business'],
];
const refs = Object.fromEntries(connectors.map(([api, logical]) => [api, {
  runtimeSource: 'embedded', connection: { connectionReferenceLogicalName: logical }, api: { name: api },
}]));
const parameters = {
  '$connections': { defaultValue: {}, type: 'Object' },
  '$authentication': { defaultValue: {}, type: 'SecureObject' },
};
const flow = definition => ({
  properties: { connectionReferences: refs, definition, templateName: null },
  schemaVersion: '1.0.0.0',
});

const legacy = JSON.parse(readFileSync(join(root, `package-source/Microsoft.Flow/flows/${scheduled.id}/definition.json`), 'utf8'));
const scheduledDefinition = legacy.properties.definition;
delete scheduledDefinition.metadata;
scheduledDefinition.triggers = {
  Every_5_minutes: {
    type: 'Recurrence',
    recurrence: { frequency: 'Minute', interval: 5 },
    runtimeConfiguration: { concurrency: { runs: 1 } },
  },
};
const a = scheduledDefinition.actions;
a.AtlasInstallationId = { type: 'Compose', inputs: 'unconfigured', runAfter: {} };
a.Read_Atlas_requested_date = {
  type: 'OpenApiConnection',
  inputs: {
    host: {
      apiId: '/providers/Microsoft.PowerApps/apis/shared_onedriveforbusiness',
      connectionName: 'shared_onedriveforbusiness',
      operationId: 'GetFileContentByPath',
    },
    parameters: { path: '/AtlasBridge/requests/selected-date.txt', inferContentType: true },
    authentication: "@parameters('$authentication')",
  },
  runAfter: { AtlasInstallationId: ['Succeeded'] },
};
a.RequestedDate = {
  type: 'Compose',
  inputs: "@trim(base64ToString(coalesce(outputs('Read_Atlas_requested_date')?['body']?['$content'],'')))",
  runAfter: { Read_Atlas_requested_date: ['Succeeded', 'Failed', 'Skipped', 'TimedOut'] },
};
a.TargetDate.inputs = "@if(empty(outputs('RequestedDate')),formatDateTime(convertTimeZone(utcNow(),'UTC','SA Pacific Standard Time'),'yyyy-MM-dd'),outputs('RequestedDate'))";
a.TargetDate.runAfter = { RequestedDate: ['Succeeded'] };
const calendar = a.Calendar_evidence.actions;
calendar.Get_calendar_view_of_events_V3.inputs.parameters.calendarId = "@first(body('Get_calendars_V2')?['value'])?['id']";
const mail = a.Mail_evidence.actions;
mail.Get_emails_V3.inputs.parameters.top = 100;
mail.Select_mail_fields.inputs.select.bodyPreview = "@coalesce(item()?['bodyPreview'],'')";
mail.Select_mail_fields.inputs.select.direction = 'received';
mail.Get_sent_emails_V3 = {
  type: 'OpenApiConnection',
  inputs: {
    host: {
      apiId: '/providers/Microsoft.PowerApps/apis/shared_office365',
      connectionName: 'shared_office365',
      operationId: 'GetEmailsV3',
    },
    parameters: {
      folderPath: 'Sent Items',
      fetchOnlyUnread: false,
      includeAttachments: false,
      top: 100,
    },
    authentication: "@parameters('$authentication')",
  },
  runAfter: {},
};
mail.Filter_sent_email_to_target_day = {
  type: 'Query',
  inputs: {
    from: "@body('Get_sent_emails_V3')?['value']",
    where: "@and(not(empty(coalesce(item()?['sentDateTime'],item()?['receivedDateTime']))),greaterOrEquals(ticks(coalesce(item()?['sentDateTime'],item()?['receivedDateTime'])),ticks(outputs('StartUtc'))),less(ticks(coalesce(item()?['sentDateTime'],item()?['receivedDateTime'])),ticks(outputs('EndUtc'))))",
  },
  runAfter: { Get_sent_emails_V3: ['Succeeded'] },
};
mail.Select_sent_mail_fields = {
  type: 'Select',
  inputs: {
    from: "@body('Filter_sent_email_to_target_day')",
    select: {
      id: "@item()?['id']",
      subject: "@coalesce(item()?['subject'],'')",
      receivedDateTime: "@coalesce(item()?['sentDateTime'],item()?['receivedDateTime'])",
      senderAddress: "@string(item()?['toRecipients'])",
      bodyPreview: "@coalesce(item()?['bodyPreview'],'')",
      direction: 'sent',
      isDraft: false,
    },
  },
  runAfter: { Filter_sent_email_to_target_day: ['Succeeded'] },
};
mail.Set_MailEvidence.inputs.value = "@union(body('Select_mail_fields'),body('Select_sent_mail_fields'))";
mail.Set_MailEvidence.runAfter = {
  Select_mail_fields: ['Succeeded'],
  Select_sent_mail_fields: ['Succeeded'],
};
const teamsLoop = a.Teams_evidence.actions.For_each_chat;
a.Teams_evidence.actions.Filter_recent_chats = {
  type: 'Query',
  inputs: {
    from: "@body('List_chats')?['value']",
    where: "@and(not(empty(item()?['lastUpdatedDateTime'])),greaterOrEquals(ticks(item()?['lastUpdatedDateTime']),ticks(outputs('StartUtc'))))",
  },
  runAfter: { List_chats: ['Succeeded'] },
};
teamsLoop.foreach = "@take(body('Filter_recent_chats'),12)";
teamsLoop.runAfter = { Filter_recent_chats: ['Succeeded'] };
teamsLoop.actions.Pace_Teams_requests = { type: 'Wait', inputs: { interval: { count: 5, unit: 'Second' } }, runAfter: {} };
const teamsRequest = teamsLoop.actions.Get_messages_in_chat;
teamsRequest.inputs.parameters['$filter'] = "@concat('lastModifiedDateTime gt ',outputs('StartUtc'),' and lastModifiedDateTime lt ',outputs('EndUtc'))";
teamsRequest.inputs.parameters['$orderby'] = 'lastModifiedDateTime desc';
teamsRequest.inputs.parameters['$top'] = 20;
teamsRequest.inputs.retryPolicy = { type: 'exponential', count: 4, interval: 'PT10S', minimumInterval: 'PT5S', maximumInterval: 'PT1M' };
teamsRequest.runAfter = { Pace_Teams_requests: ['Succeeded'] };
calendar.Set_CalendarSourceReady = { type: 'SetVariable', inputs: { name: 'CalendarSourceReady', value: true }, runAfter: { Set_CalendarEvidence: ['Succeeded'] } };
a.Mail_evidence.actions.Set_MailSourceReady = { type: 'SetVariable', inputs: { name: 'MailSourceReady', value: true }, runAfter: { Set_MailEvidence: ['Succeeded'] } };
a.Teams_evidence.actions.Set_TeamsSourceReady = { type: 'SetVariable', inputs: { name: 'TeamsSourceReady', value: true }, runAfter: { For_each_chat: ['Succeeded'] } };
for (const [key, variable, after] of [
  ['Initialize_CalendarSourceReady', 'CalendarSourceReady', 'Initialize_TeamsEvidence'],
  ['Initialize_MailSourceReady', 'MailSourceReady', 'Initialize_CalendarSourceReady'],
  ['Initialize_TeamsSourceReady', 'TeamsSourceReady', 'Initialize_MailSourceReady'],
]) {
  a[key] = { type: 'InitializeVariable', inputs: { variables: [{ name: variable, type: 'boolean', value: false }] }, runAfter: { [after]: ['Succeeded'] } };
}
for (const scope of ['Calendar_evidence', 'Mail_evidence', 'Teams_evidence']) {
  a[scope].runAfter = { Initialize_TeamsSourceReady: ['Succeeded'] };
}
const terminalStates = ['Succeeded', 'Failed', 'Skipped', 'TimedOut'];
a.Compose_Atlas_bundle.inputs = {
  schemaVersion: 3,
  exportedAt: '@utcNow()',
  targetDate: "@outputs('TargetDate')",
  sources: {
    calendar: "@variables('CalendarSourceReady')",
    mail: "@variables('MailSourceReady')",
    teams: "@variables('TeamsSourceReady')",
  },
  calendar: "@variables('CalendarEvidence')",
  mail: "@variables('MailEvidence')",
  teams: "@variables('TeamsEvidence')",
};
a.Compose_Atlas_bundle.runAfter = Object.fromEntries(['Calendar_evidence', 'Mail_evidence', 'Teams_evidence'].map(scope => [scope, terminalStates]));
a.Create_Atlas_evidence_file.inputs.parameters.name = "@concat('atlas-evidence-',outputs('AtlasInstallationId'),'-',outputs('TargetDate'),'-',formatDateTime(utcNow(),'yyyyMMddTHHmmssZ'),'.json')";
a.Create_Atlas_evidence_file.inputs.parameters.folderPath = "@if(empty(outputs('RequestedDate')),'/AtlasBridge/inbox/scheduled','/AtlasBridge/inbox/requested')";

const eventDefinition = {
  '$schema': 'https://schema.management.azure.com/providers/Microsoft.Logic/schemas/2016-06-01/workflowdefinition.json#',
  contentVersion: '1.0.0.0',
  parameters,
  triggers: {
    When_a_new_chat_message_is_added: {
      type: 'OpenApiConnectionWebhook',
      inputs: {
        host: {
          apiId: '/providers/Microsoft.PowerApps/apis/shared_teams',
          connectionName: 'shared_teams',
          operationId: 'WebhookChatMessageTrigger',
        },
        parameters: {},
        authentication: "@parameters('$authentication')",
      },
      runtimeConfiguration: { concurrency: { runs: 1 } },
    },
  },
  actions: {
    AtlasInstallationId: { type: 'Compose', inputs: 'unconfigured', runAfter: {} },
    TargetDate: {
      type: 'Compose',
      inputs: "@formatDateTime(convertTimeZone(utcNow(),'UTC','SA Pacific Standard Time'),'yyyy-MM-dd')",
      runAfter: { AtlasInstallationId: ['Succeeded'] },
    },
    StartUtc: {
      type: 'Compose',
      inputs: "@convertToUtc(concat(outputs('TargetDate'),'T00:00:00'),'SA Pacific Standard Time','yyyy-MM-ddTHH:mm:ssZ')",
      runAfter: { TargetDate: ['Succeeded'] },
    },
    EndUtc: {
      type: 'Compose',
      inputs: "@convertToUtc(concat(formatDateTime(addDays(outputs('TargetDate'),1),'yyyy-MM-dd'),'T00:00:00'),'SA Pacific Standard Time','yyyy-MM-ddTHH:mm:ssZ')",
      runAfter: { StartUtc: ['Succeeded'] },
    },
    For_each_notification: {
      type: 'Foreach',
      foreach: "@triggerBody()?['value']",
      actions: {
        Get_messages_for_triggered_chat: {
          type: 'OpenApiConnection',
          inputs: {
            host: {
              apiId: '/providers/Microsoft.PowerApps/apis/shared_teams',
              connectionName: 'shared_teams',
              operationId: 'GetMessagesFromChat',
            },
            parameters: {
              chatId: "@items('For_each_notification')?['conversationId']",
              '$filter': "@concat('lastModifiedDateTime gt ',outputs('StartUtc'),' and lastModifiedDateTime lt ',outputs('EndUtc'))",
              '$orderby': 'lastModifiedDateTime desc',
              '$top': 20,
            },
            authentication: "@parameters('$authentication')",
            retryPolicy: { type: 'exponential', count: 4, interval: 'PT10S', minimumInterval: 'PT5S', maximumInterval: 'PT1M' },
          },
          runAfter: {},
        },
        Filter_messages_to_target_day: {
          type: 'Query',
          inputs: {
            from: "@body('Get_messages_for_triggered_chat')?['value']",
            where: "@and(not(empty(item()?['createdDateTime'])),greaterOrEquals(ticks(item()?['createdDateTime']),ticks(outputs('StartUtc'))),less(ticks(item()?['createdDateTime']),ticks(outputs('EndUtc'))))",
          },
          runAfter: { Get_messages_for_triggered_chat: ['Succeeded'] },
        },
        Select_Teams_fields: {
          type: 'Select',
          inputs: {
            from: "@body('Filter_messages_to_target_day')",
            select: {
              id: "@item()?['id']",
              chatId: "@items('For_each_notification')?['conversationId']",
              topic: '',
              createdDateTime: "@item()?['createdDateTime']",
              author: "@coalesce(item()?['from']?['user']?['displayName'],'')",
              content: "@coalesce(item()?['body']?['content'],'')",
              messageType: "@coalesce(item()?['messageType'],'message')",
            },
          },
          runAfter: { Filter_messages_to_target_day: ['Succeeded'] },
        },
        Compose_Teams_event_bundle: {
          type: 'Compose',
          inputs: {
          schemaVersion: 3,
            exportedAt: '@utcNow()',
            targetDate: "@outputs('TargetDate')",
            sources: { calendar: false, mail: false, teams: true },
            calendar: [],
            mail: [],
            teams: "@body('Select_Teams_fields')",
          },
          runAfter: { Select_Teams_fields: ['Succeeded'] },
        },
        Create_Teams_event_evidence_file: {
          type: 'OpenApiConnection',
          inputs: {
            host: {
              apiId: '/providers/Microsoft.PowerApps/apis/shared_onedriveforbusiness',
              connectionName: 'shared_onedriveforbusiness',
              operationId: 'CreateFile',
            },
            parameters: {
              folderPath: '/AtlasBridge/inbox/teams',
              name: "@concat('atlas-evidence-',outputs('AtlasInstallationId'),'-',outputs('TargetDate'),'-teams-',guid(),'.json')",
              body: "@string(outputs('Compose_Teams_event_bundle'))",
            },
            authentication: "@parameters('$authentication')",
          },
          runAfter: { Compose_Teams_event_bundle: ['Succeeded'] },
        },
      },
      runAfter: { EndUtc: ['Succeeded'] },
      runtimeConfiguration: { concurrency: { repetitions: 1 } },
    },
  },
  outputs: {},
};

mkdirSync(join(root, 'solution-source/Workflows'), { recursive: true });
for (const [item, content] of [
  [scheduled, flow(scheduledDefinition)],
  [eventCapture, flow(eventDefinition)],
]) {
  writeFileSync(join(root, 'solution-source/Workflows', item.file), JSON.stringify(content, null, 2) + '\n');
}

const solutionPath = join(root, 'solution-source/Other/Solution.xml');
let solution = readFileSync(solutionPath, 'utf8')
  .replace(/<Version>.*?<\/Version>/, `<Version>${version}</Version>`)
  .replace(/<Managed>.*?<\/Managed>/, '<Managed>0</Managed>');
solution = solution.replace(
  /<RootComponents\s*\/>|<RootComponents>[\s\S]*?<\/RootComponents>/,
  `<RootComponents>${[scheduled, eventCapture].map(item => `<RootComponent type="29" id="{${item.id}}" behavior="0" />`).join('')}</RootComponents>`,
);
writeFileSync(solutionPath, solution);

const fields = item => ({
  JsonFileName: `/Workflows/${item.file}`,
  Type: 1,
  Subprocess: 0,
  Category: 5,
  Mode: 0,
  Scope: 4,
  OnDemand: 0,
  TriggerOnCreate: 0,
  TriggerOnDelete: 0,
  AsyncAutoDelete: 0,
  SyncWorkflowLogOnFailure: 0,
  StateCode: 1,
  StatusCode: 2,
  RunAs: 1,
  IsTransacted: 1,
  IntroducedVersion: version,
  IsCustomizable: 1,
  BusinessProcessType: 0,
  IsCustomProcessingStepAllowedForOtherPublishers: 1,
  PrimaryEntity: 'none',
});
const workflowXml = item => `<Workflow WorkflowId="{${item.id}}" Name="${item.name}">
${Object.entries(fields(item)).map(([key, value]) => `    <${key}>${value}</${key}>`).join('\n')}
    <LocalizedNames><LocalizedName languagecode="1033" description="${item.name}"/></LocalizedNames>
  </Workflow>`;
for (const item of [scheduled, eventCapture]) {
  writeFileSync(
    join(root, 'solution-source/Workflows', item.file + '.data.xml'),
    `<?xml version="1.0" encoding="utf-8"?>\n${workflowXml(item)}\n`,
  );
}
writeFileSync(join(root, 'solution-source/Other/Customizations.xml'), `<?xml version="1.0" encoding="utf-8"?>
<ImportExportXml xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Entities/><Roles/><Workflows />
  <FieldSecurityProfiles/><Templates/><EntityMaps/><EntityRelationships/><OrganizationSettings/><optionsets/><CustomControls/><SolutionPluginAssemblies/><EntityDataProviders/>
  <connectionreferences>${connectors.map(([api, logical, label]) => `
    <connectionreference connectionreferencelogicalname="${logical}"><connectionreferencedisplayname>${label}</connectionreferencedisplayname><connectorid>/providers/Microsoft.PowerApps/apis/${api}</connectorid><iscustomizable>1</iscustomizable><statecode>0</statecode><statuscode>1</statuscode></connectionreference>`).join('')}
  </connectionreferences><Languages><Language>1033</Language></Languages>
</ImportExportXml>\n`);
console.log('Built two active flow sources without tenant credentials or connection instance IDs.');
