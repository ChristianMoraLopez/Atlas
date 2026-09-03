# Atlas — Circana Interactions Tracker

Atlas is a Tauri v2 Windows desktop application that builds the Circana Interactions Tracker from a user's own Microsoft 365 calendar and mail. It remembers one local Excel or SharePoint destination, can sync today's calendar automatically while it is running, and keeps manual tasks optional. It supports optional Teams-chat interpretation through a local Ollama model and rerunnable `.xlsx` / `.xlsm` exports.

> **Atlas never invents interactions on its own.** Every exported row comes from exactly one of three sources: a real Microsoft Graph calendar event, a real Microsoft Graph email, or a manual entry the user typed. Manual entries are optional. There is no filler generator, minimum-row target, or blocking “add more” prompt.

## What it does

- Signs each teammate in through Microsoft Authorization Code + PKCE in the system browser.
- Stores the refresh token in size-safe chunks in Windows Credential Manager and keeps short-lived access tokens only in memory.
- Extracts real meetings for a selected workday and excludes Lunch, Almuerzo, Tracker Time, Hora del Tracker, and cancelled meetings.
- Normalizes only the first MMNI/SparkTriage meeting to 09:00–09:30 in the chosen local timezone.
- Shows mail as unchecked candidates; the user must explicitly select each completed interaction.
- Optionally summarizes real Teams chat evidence using Ollama at `127.0.0.1:11434`. To preserve the three-source invariant, AI suggestions are reference-only and cannot be exported directly; “Log manually” opens a blank form and copies no AI content.
- Lets the user edit review fields and add a genuinely manual interaction through a blank form.
- Configures a new tracker, an existing `.xlsx` / `.xlsm`, or a SharePoint/OneDrive workbook link once and reuses it.
- Can sync today's calendar at startup and hourly while Atlas remains open; a one-click sync is always available.
- Downloads SharePoint workbooks through Microsoft Graph, patches them locally, and uploads with an `If-Match` conflict guard so a newer remote edit is never overwritten.
- Uses a hidden `_source_id` to update previously exported Graph rows without creating duplicates.
- Never writes CSA Name, Capgemini Team Lead, Circana Manager, MTTR, Resolution time, or IR Time. Existing files are patched at the Office-package XML level so formulas, VBA, and unrelated worksheets remain intact.
- Writes startup, command, browser, authentication, Graph, and export failures to `%APPDATA%\com.capgemini.atlas-tracker\logs\atlas.log`, available from the log button in the header.

## Entra ID setup

Create one Microsoft Entra app registration for the team:

1. Choose **Accounts in this organizational directory only** unless your organization requires another account type.
2. Under **Authentication**, add the **Mobile and desktop applications** platform and the loopback redirect URI `http://localhost`.
3. Enable public client flows.
4. Add delegated Microsoft Graph permissions:
   - `User.Read`
   - `Calendars.Read`
   - `Mail.Read`
   - `Files.ReadWrite` for a configured SharePoint/OneDrive tracker
   - `Chat.Read` only when the optional Teams-chat feature will be used
5. Do not add application permissions or a client secret. Atlas is a public desktop client.

On first launch, Atlas asks for these public identifiers:

- Application (client) ID
- Directory (tenant) ID

They are saved in the user's local Atlas settings and can be changed from the Microsoft connection button in the app. Changing either identifier clears the previous Microsoft token and starts a new PKCE login. Normal local-Excel login requests only profile, calendar, and mail access. Atlas asks for `Files.ReadWrite` when a SharePoint destination is configured and asks for `Chat.Read` only when Teams suggestions are enabled. No client secret is used or requested. Build-time `CIRCANA_AZURE_CLIENT_ID` and `CIRCANA_AZURE_TENANT_ID` values remain optional defaults for managed team builds.

## One-time tracker setup

After profile setup, choose one destination:

- **Existing Excel tracker** - select a local `.xlsx` or `.xlsm` template.
- **Create a new tracker** - choose the path for a new `.xlsx`; after its first write Atlas automatically treats it as an existing tracker.
- **SharePoint link** - paste a direct workbook sharing/browser link such as `https://tenant-my.sharepoint.com/:x:/r/.../Tracker.xlsm?web=1`.

The destination and automatic-sync preference are stored in local settings. A SharePoint link is encoded as a Microsoft Graph sharing token, resolved to its drive item, downloaded to a unique temporary file, patched with the same macro-preserving writer used for local files, and uploaded to that exact drive item. Atlas requires delegated `Files.ReadWrite` access and the signed-in user must already have edit access to the workbook.

## Local development

Prerequisites:

- Node.js 24+
- Rust stable with the MSVC target
- Visual Studio Build Tools with **Desktop development with C++** and the Windows SDK
- Microsoft Edge WebView2 runtime

PowerShell:

```powershell
$env:CIRCANA_AZURE_CLIENT_ID = "your-client-id"
$env:CIRCANA_AZURE_TENANT_ID = "your-tenant-id"
npm install
npm run tauri dev
```

Frontend-only checks can be run with `npm run build`; Rust tests use `cargo test --manifest-path src-tauri/Cargo.toml`.

## Local AI privacy boundary

The Teams feature has one fixed generation endpoint: `http://127.0.0.1:11434/api/generate`. Before every Ollama request, Rust verifies that the destination is plain HTTP and a loopback IP address. The model name is configurable, but the endpoint is not. There is no OpenAI, Anthropic, Azure OpenAI, or other cloud-LLM SDK in the dependency tree.

Install [Ollama for Windows](https://ollama.com/download/windows), then pull the default model:

```powershell
ollama pull qwen2.5:3b
```

If Ollama or the selected model is unavailable, Atlas displays setup instructions and does not silently send chat content elsewhere.

## Excel safety and reruns

Atlas supports the real `Circana Interactions Tracker V 1.0` layout: the `Data` table starts on row 2, date headers include their `(mm/dd/yyyy hh:mm)` suffixes, and `Incident Number` includes its `If applies` suffix. For an existing workbook, Atlas requires a worksheet whose name matches the profile’s full name or Login ID. It maps these labels safely even when they contain line breaks, writes only columns A–O on that sheet, and never writes the orange formula columns P–U.

Existing `.xlsm` / `.xlsx` files are updated by replacing only the selected worksheet XML inside the Office package. All other package entries are copied byte-for-byte, including `vbaProject.bin`, external links, other worksheets, hidden lookup data, and workbook metadata. Within the selected sheet, existing cell styles, the `Data` table, formulas, validations, and conditional formatting remain in place. Atlas fills the first unused preformatted row inside the table and stops with a clear error if the table has no capacity; it never appends a malformed row beyond the template.

Calendar and mail rows are keyed as `graph:calendar:<id>` and `graph:mail:<id>`. A second export updates the matching row. App-created manual rows use `manual:<uuid>` and are skipped—not modified—if that exact ID already exists. Rows without Atlas source IDs are treated as user-owned and are never updated or deleted.

The writer first creates a complete temporary package and a safety copy before replacing an existing file. The hidden `_source_id` helper is placed immediately after the template columns and is not added to the visible `Data` table. Close a workbook in Excel before exporting so Windows does not lock it.

## Windows releases

Every push to `main` runs **Build Atlas Windows x64** from [`.github/workflows/ci.yml`](.github/workflows/ci.yml). After the tests pass, GitHub Actions compiles the portable executable and adds a versioned Windows x64 artifact to the workflow run. The artifact contains `Atlas.exe`, `README.md`, `SHA256SUMS.txt`, and the ready-to-extract portable zip; it is retained for seven days.

Pushing a tag that starts with `v` runs [`.github/workflows/release.yml`](.github/workflows/release.yml). It creates a permanent GitHub Release containing one portable zip with `Atlas.exe` and setup instructions. Repository variables can provide managed defaults, but are not required because users can enter the public identifiers inside Atlas.

```powershell
git tag v0.2.0
git push origin v0.2.0
```

Unsigned internal builds can trigger Microsoft SmartScreen. Configure Windows code signing in the release workflow before broad distribution.

## Repository layout

- `src/` — React + Tailwind review interface
- `src-tauri/src/auth.rs` — Microsoft PKCE flow and secure token storage
- `src-tauri/src/graph.rs` — calendar, mail, and Teams evidence extraction
- `src-tauri/src/ollama.rs` — enforced loopback-only chat summarization
- `src-tauri/src/excel.rs` — workbook mapping, safe writes, and source-ID upserts
- `.github/workflows/` — Windows verification and tagged release builds
