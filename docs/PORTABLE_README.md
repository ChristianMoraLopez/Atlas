# Atlas portable build

Atlas creates Circana interaction trackers from real Microsoft 365 activity.

## First launch

1. Extract the zip and run `Atlas.exe`. No installer is required.
2. Enter the **Application (client) ID** and **Directory (tenant) ID** from the team's Microsoft Entra app registration, then choose **Save and sign in**. These are public identifiers, not secrets.
3. Complete Microsoft 365 sign-in in the system browser. Atlas stores the OAuth token in Windows Credential Manager.
4. Enter your Corp ID, name, Area, Team Lead, and Circana Manager.
5. Choose a workday, extract, review the rows, and export.

Atlas uses Microsoft WebView2 for its interface. WebView2 is normally already installed on supported Windows 10 and Windows 11 systems; if Windows reports that it is missing, install the Evergreen WebView2 Runtime from Microsoft.

For the supplied Circana workbook, choose **Use Circana template**. Your Atlas profile name must match the worksheet name (for example, `Sebastian Galindo`) so the app cannot write to another person's tab.

For optional Teams chat suggestions, install [Ollama](https://ollama.com/download/windows) and run `ollama pull qwen2.5:3b`. Teams content is sent only to `127.0.0.1:11434`; no cloud LLM client is included.

The Entra app must be a public desktop client with a `http://localhost` loopback redirect and delegated `User.Read`, `Calendars.Read`, `Mail.Read`, and—only if Teams suggestions are enabled—`Chat.Read` permissions. Do not create or enter a client secret.

**Atlas never invents interactions.** Every exported row comes from a real calendar event, a real email, or a manual entry typed by the user. AI chat suggestions are reference-only and are never exported directly. Manual entries are optional. There is no filler generator or minimum row count.
