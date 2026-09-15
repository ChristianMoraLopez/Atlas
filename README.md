# Atlas — Circana Interactions Tracker

For tenants that block the Atlas Entra application, Atlas includes a portal-assisted Power Automate solution installer. It prepares `AtlasBridge/inbox`, opens Microsoft's official portal, records user-confirmed setup stages, and completes only after a correlated evidence file passes the bundled schema. It does not copy browser cookies, automate credentials/MFA, call private portal endpoints, or distribute PAC. See [`power-automate/INSTALLER.md`](power-automate/INSTALLER.md).

Atlas is a Tauri v2 Windows desktop application that builds the Circana Interactions Tracker from a user's own Microsoft 365 calendar, mail, and Teams evidence. It remembers one local Excel or SharePoint destination and runs the daily tracker automatically at a configurable time, initially 17:30. It writes every real activity it found and opens the interface for attention when the day contains fewer than three. The portable package includes and manages its own local Ollama runtime and model for local interpretation.

> **Atlas never invents interactions on its own.** Every exported row is a finished meeting, a concrete work task inferred from real Microsoft 365 evidence, or a factual manual entry. A task may be assigned, in progress, or resolved. Atlas saves partial days and alerts the user when fewer than three activities were found.

## What it does

- Signs each teammate in through Microsoft Authorization Code + PKCE in the system browser.
- Stores the refresh token in size-safe chunks in Windows Credential Manager and keeps short-lived access tokens only in memory.
- Opens with a skippable Atlas SVG animation and honors Windows reduced-motion preferences.
- Extracts real meetings for any selected workday and excludes cancelled events, meal/tracker blocks, focus blocks, reminders, and to-do calendar entries.
- Normalizes only the first MMNI/SparkTriage meeting to 09:00–09:30 in the chosen local timezone.
- Preselects every finished meeting and each separate work task inferred from mail or Teams. Assigned and in-progress work is included; reminders, invitations, automatic notices, and inbox activity without a concrete task are excluded.
- Interprets mail and Teams evidence with bundled Local AI in both Graph and Power Automate modes. Every suggestion stays linked to its real source evidence, distinct tasks can come from the same email or conversation, and tracker summaries are written in English.
- Lets the user edit review fields and add a genuinely manual interaction through a blank form.
- Configures a new tracker, an existing `.xlsx` / `.xlsm`, or a SharePoint/OneDrive workbook link once and reuses it.
- Registers a per-user Windows scheduled task with limited privileges and runs at 17:30 by default. If Atlas is already open, the existing instance handles the run.
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
   - `Chat.Read`
5. Do not add application permissions or a client secret. Atlas is a public desktop client.

The application owner embeds the public client ID at build time through `CIRCANA_AZURE_CLIENT_ID`; end users never enter client or tenant identifiers. When no Atlas app registration is configured, first launch opens the Power Automate connector assistant instead. No client secret is used or requested.

## One-time tracker setup

After profile setup, choose one destination:

- **Existing Excel tracker** - select a local `.xlsx` or `.xlsm` template.
- **Create a new tracker** - choose the path for a new `.xlsx`; after its first write Atlas automatically treats it as an existing tracker.
- **SharePoint link** - paste a direct workbook sharing/browser link such as `https://tenant-my.sharepoint.com/:x:/r/.../Tracker.xlsm?web=1`.

The destination, daily time, and automatic-run preference are stored in local settings. In Graph mode, a SharePoint link is resolved and updated through Microsoft Graph. In Power Automate Inbox mode, Atlas uses the local OneDrive-synced copy of the linked workbook, preserving `.xlsm` macros while the OneDrive client publishes the update. The user must already have edit access.

For a workbook shared from another person’s OneDrive, the owner must share the **containing folder** with edit permission. Open **Shared → Shared with you**, select that folder, choose **Add shortcut to My files**, wait until it appears below `OneDrive - Circana` in File Explorer, then select the exact synced `.xlsm` or `.xlsx` in Atlas. OneDrive supports this shortcut operation for folders rather than individual shared files, so a file-only share must first be moved into a shared folder.

Power Automate Inbox mode creates `AtlasBridge/inbox/scheduled`, `AtlasBridge/inbox/teams`, `AtlasBridge/inbox/requested`, and `AtlasBridge/requests`. Choosing an older workday writes `requests/selected-date.txt`; solution 1.9 checks that request every five minutes, exports that date, and Atlas waits for the synchronized result. Existing flat inbox JSON remains readable.

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

## Bundled Local AI and privacy boundary

The portable ZIP includes a CPU-only Ollama 0.30.8 runtime and the Apache-2.0-licensed `qwen2.5:1.5b-instruct-q4_K_M` model. Atlas discovers the `AtlasAI` folder beside `Atlas.exe`, verifies the expected model payload, starts Ollama in a hidden child process, waits for readiness, and stops the process when Atlas closes. Users do not install Ollama or download a model separately.

The Teams feature chooses the first available port from `127.0.0.1:11435` through `127.0.0.1:11445`, so an orphaned local process cannot block Atlas. Before every request, Rust verifies that the destination is plain HTTP and a loopback IP address. The model and port range are fixed by the build. There is no OpenAI, Anthropic, Azure OpenAI, or other cloud-LLM SDK in the dependency tree. Ollama output is appended to `%APPDATA%\com.capgemini.atlas-tracker\logs\local-ai.log`.

Release packaging downloads the official Ollama Windows x64 archive, verifies its pinned SHA-256, removes CUDA and Vulkan runners to create a broadly compatible CPU package, pulls the pinned model into a private model directory, verifies its manifest and model blob, and includes both third-party licenses. If any verification fails, the release build stops instead of publishing a partial portable package.

## Excel safety and reruns

Atlas supports the real `Circana Interactions Tracker V 1.0` layout: the `Data` table starts on row 2, date headers include their `(mm/dd/yyyy hh:mm)` suffixes, and `Incident Number` includes its `If applies` suffix. For an existing workbook, Atlas requires a worksheet whose name matches the profile’s full name or Login ID. It maps these labels safely even when they contain line breaks, writes only columns A–O on that sheet, and never writes the orange formula columns P–U.

Existing `.xlsm` / `.xlsx` files are updated by replacing only the selected worksheet XML inside the Office package. All other package entries are copied byte-for-byte, including `vbaProject.bin`, external links, other worksheets, hidden lookup data, and workbook metadata. Within the selected sheet, existing cell styles, the `Data` table, formulas, validations, and conditional formatting remain in place. New date cells inherit the template’s date style, so Excel displays `mm/dd/yyyy hh:mm` instead of the underlying numeric serial. Atlas fills the first unused row and stops with a clear error if the template cannot accept it.

Meetings keep their Microsoft event ID. Inferred tasks use a stable hash of their real mail or Teams evidence plus the task summary, which allows one message to support several separate tasks without producing duplicates on a rerun. App-created manual rows use `manual:<uuid>` and are skipped—not modified—if that exact ID already exists. Rows without Atlas source IDs are treated as user-owned and are never updated or deleted.

The writer first creates a complete temporary package and a safety copy before replacing an existing file. The hidden `_source_id` helper is placed immediately after the template columns and is not added to the visible `Data` table. Close a workbook in Excel before exporting so Windows does not lock it.

## Windows releases

Every push to `main` runs **Build Atlas Windows x64** from [`.github/workflows/ci.yml`](.github/workflows/ci.yml). After the tests pass, GitHub Actions compiles the executable and adds a versioned Windows x64 artifact to the workflow run. Its ready-to-extract portable ZIP contains `Atlas.exe`, `README.md`, and the complete `AtlasAI` runtime/model folder; the workflow artifact also includes `SHA256SUMS.txt` and is retained for seven days.

Pushing a tag that starts with `v` runs [`.github/workflows/release.yml`](.github/workflows/release.yml). It creates a permanent GitHub Release containing the self-contained portable ZIP and checksum. Repository variables may provide the optional Graph-mode public identifiers; the Power Automate Inbox setup never asks end users for tenant or application IDs.

```powershell
git tag v0.2.2
git push origin v0.2.2
```

Unsigned internal builds can trigger Microsoft SmartScreen. Configure Windows code signing in the release workflow before broad distribution.

## Repository layout

- `src/` — React + Tailwind review interface
- `src-tauri/src/auth.rs` — Microsoft PKCE flow and secure token storage
- `src-tauri/src/graph.rs` — calendar, mail, and Teams evidence extraction
- `src-tauri/src/ollama.rs` — managed bundled runtime and enforced loopback-only chat summarization
- `.github/scripts/package-portable.ps1` — verified Ollama/model packaging and license inclusion
- `src-tauri/src/excel.rs` — workbook mapping, safe writes, and source-ID upserts
- `.github/workflows/` — Windows verification and tagged release builds
