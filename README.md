# Atlas — Circana Interactions Tracker

Atlas is a Tauri v2 Windows desktop application that builds the Circana Interactions Tracker from a user’s own Microsoft 365 calendar and mail. It supports optional Teams-chat interpretation through a local Ollama model, an editable review step, manual entries, and rerunnable `.xlsx` / `.xlsm` exports.

> **Atlas never invents interactions on its own.** Every exported row comes from exactly one of three sources: a real Microsoft Graph calendar event, a real Microsoft Graph email, or a manual entry the user typed. Manual entries are optional. There is no filler generator, minimum-row target, or blocking “add more” prompt.

## What it does

- Signs each teammate in through Microsoft Authorization Code + PKCE in the system browser.
- Stores only the OAuth token in the operating system credential manager.
- Extracts real meetings for a selected workday and excludes Lunch, Almuerzo, Tracker Time, Hora del Tracker, and cancelled meetings.
- Normalizes only the first MMNI/SparkTriage meeting to 09:00–09:30 in the chosen local timezone.
- Shows mail as unchecked candidates; the user must explicitly select each completed interaction.
- Optionally summarizes real Teams chat evidence using Ollama at `127.0.0.1:11434`. To preserve the three-source invariant, AI suggestions are reference-only and cannot be exported directly; “Log manually” opens a blank form and copies no AI content.
- Lets the user edit review fields and add a genuinely manual interaction through a blank form.
- Creates a new tracker or upserts the current user’s worksheet in an existing `.xlsx` / `.xlsm` workbook.
- Uses a hidden `_source_id` to update previously exported Graph rows without creating duplicates.
- Never writes CSA Name, Capgemini Team Lead, Circana Manager, MTTR, Resolution time, or IR Time. Existing formulas and VBA content are read and written by `umya-spreadsheet` without intentionally modifying other worksheets.

## Entra ID setup

Create one Microsoft Entra app registration for the team:

1. Choose **Accounts in this organizational directory only** unless your organization requires another account type.
2. Under **Authentication**, add the **Mobile and desktop applications** platform and the loopback redirect URI `http://localhost`.
3. Enable public client flows.
4. Add delegated Microsoft Graph permissions:
   - `User.Read`
   - `Calendars.Read`
   - `Mail.Read`
   - `Chat.Read` only when the optional Teams-chat feature will be used
5. Do not add application permissions or a client secret. Atlas is a public desktop client.

Add these public identifiers as GitHub repository **Actions variables** (not secrets):

- `CIRCANA_AZURE_CLIENT_ID` — Application (client) ID
- `CIRCANA_AZURE_TENANT_ID` — Directory (tenant) ID

They are compiled into the executable at build time. A build without them opens a configuration screen and cannot start authentication.

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

Pushing a tag that starts with `v` runs [`.github/workflows/release.yml`](.github/workflows/release.yml). GitHub Actions builds the NSIS installer with `tauri-apps/tauri-action`, creates a GitHub Release, and adds a portable zip containing the compiled executable and setup instructions.

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Unsigned internal builds can trigger Microsoft SmartScreen. Configure Windows code signing in the release workflow before broad distribution.

## Repository layout

- `src/` — React + Tailwind review interface
- `src-tauri/src/auth.rs` — Microsoft PKCE flow and secure token storage
- `src-tauri/src/graph.rs` — calendar, mail, and Teams evidence extraction
- `src-tauri/src/ollama.rs` — enforced loopback-only chat summarization
- `src-tauri/src/excel.rs` — workbook mapping, safe writes, and source-ID upserts
- `.github/workflows/` — Windows verification and tagged release builds
