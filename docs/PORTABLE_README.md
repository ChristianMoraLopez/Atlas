# Atlas portable build

Atlas creates Circana interaction trackers from real Microsoft 365 activity.

## First launch

1. Extract the zip and run `Atlas.exe`. No installer is required.
2. Atlas opens **Instalar conector de Microsoft 365**, creates the local OneDrive inbox and guides the one-time official Power Automate solution import. It never asks for an Application ID or tenant ID. See `power-automate/INSTALLER.md`. The final PC doesn't need PAC, .NET, Node.js, Rust, Visual Studio, or administrator rights.
3. Sign in with the Circana account in Microsoft's page and associate that account's Outlook, Teams and OneDrive connections during import.
4. Complete Microsoft 365 sign-in in the system browser. Atlas stores the OAuth token in Windows Credential Manager.
5. Enter your Corp ID, name, Area, Team Lead, and Circana Manager.
6. Configure the tracker once: select an existing `.xlsx` / `.xlsm`, create a new `.xlsx`, or paste a SharePoint/OneDrive workbook link.
7. Keep automatic sync enabled to update today's calendar, mail and Teams evidence at startup and hourly while Atlas is open. Atlas requires one Meeting, one E-Mail and one Task; it alerts you and enables manual entry when a real category is missing.

Atlas uses Microsoft WebView2 for its interface. WebView2 is normally already installed on supported Windows 10 and Windows 11 systems; if Windows reports that it is missing, install the Evergreen WebView2 Runtime from Microsoft.

For the supplied Circana workbook, choose **Use Circana template**. Your Atlas profile name must match the worksheet name (for example, `Sebastian Galindo`) so the app cannot write to another person's tab.

The portable folder already contains Atlas Local AI: a CPU-only Ollama runtime and the `qwen2.5:1.5b-instruct-q4_K_M` model. Atlas starts and stops it automatically on an available port from `127.0.0.1:11435` through `127.0.0.1:11445`; no separate Ollama installation or model download is required. Keep the `AtlasAI` folder beside `Atlas.exe`.

The Entra app must be a public desktop client with a `http://localhost` loopback redirect and delegated `User.Read`, `Calendars.Read`, `Mail.Read`, and `Chat.Read` permissions. Add `Files.ReadWrite` when using a SharePoint tracker. Do not create or enter a client secret.

For SharePoint, paste the full link that opens the workbook in your browser. Atlas resolves it through Microsoft Graph, preserves `.xlsm` macros and workbook structure, and refuses to overwrite the remote file if it changed during the sync.

If anything fails, use the document button in the Atlas header to open the persistent diagnostic log. It is stored at `%APPDATA%\com.capgemini.atlas-tracker\logs\atlas.log`; Local AI process output is stored beside it as `local-ai.log`. Invalid settings are backed up and reset instead of causing a silent startup exit.

**Atlas never invents interactions.** Every exported row comes from Microsoft 365 evidence or a factual manual entry. AI chat suggestions are reference-only. Atlas does enforce the three required daily categories and explains what is missing.
