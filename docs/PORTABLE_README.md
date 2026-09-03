# Atlas portable build

Atlas creates Circana interaction trackers from real Microsoft 365 activity.

## First launch

1. Extract the zip and run `Atlas.exe`. No installer is required.
2. Enter the **Application (client) ID** and **Directory (tenant) ID** from the team's Microsoft Entra app registration, then choose **Save and sign in**. These are public identifiers, not secrets.
3. Complete Microsoft 365 sign-in in the system browser. Atlas stores the OAuth token in Windows Credential Manager.
4. Enter your Corp ID, name, Area, Team Lead, and Circana Manager.
5. Configure the tracker once: select an existing `.xlsx` / `.xlsm`, create a new `.xlsx`, or paste a SharePoint/OneDrive workbook link.
6. Keep automatic sync enabled to update today's calendar at startup and hourly while Atlas is open, or use **Sync today's calendar** at any time. Adding a manual task is optional.

Atlas uses Microsoft WebView2 for its interface. WebView2 is normally already installed on supported Windows 10 and Windows 11 systems; if Windows reports that it is missing, install the Evergreen WebView2 Runtime from Microsoft.

For the supplied Circana workbook, choose **Use Circana template**. Your Atlas profile name must match the worksheet name (for example, `Sebastian Galindo`) so the app cannot write to another person's tab.

For optional Teams chat suggestions, install [Ollama](https://ollama.com/download/windows) and run `ollama pull qwen2.5:3b`. Teams content is sent only to `127.0.0.1:11434`; no cloud LLM client is included.

The Entra app must be a public desktop client with a `http://localhost` loopback redirect and delegated `User.Read`, `Calendars.Read`, `Mail.Read`, and `Files.ReadWrite` permissions. Atlas requests `Chat.Read` separately and only if Teams suggestions are enabled. Do not create or enter a client secret.

For SharePoint, paste the full link that opens the workbook in your browser. Atlas resolves it through Microsoft Graph, preserves `.xlsm` macros and workbook structure, and refuses to overwrite the remote file if it changed during the sync.

If anything fails, use the document button in the Atlas header to open the persistent diagnostic log. It is stored at `%APPDATA%\com.capgemini.atlas-tracker\logs\atlas.log`. Invalid settings are backed up and reset instead of causing a silent startup exit.

**Atlas never invents interactions.** Every exported row comes from a real calendar event, a real email, or a manual entry typed by the user. AI chat suggestions are reference-only and are never exported directly. Manual entries are optional. There is no filler generator or minimum row count.
