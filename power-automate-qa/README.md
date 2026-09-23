# AtlasQA — Power Automate solution for the QA (manager) role

`AtlasQA_1_0_0_0.zip` is a **template**. Nobody imports it as-is. When a manager picks the QA role in Atlas, the connector assistant personalizes it for that person before handing over the ZIP:

| Component | Template | Personalized (example) |
| --- | --- | --- |
| Solution unique name | `AtlasQA` | `AtlasQA_ChristianMora_<installation key>` |
| Solution display name | `AtlasQA` | `Atlas QA - Christian Mora - a84c12f9` |
| Flow names | `Atlas QA - Export mailbox evidence` | `Atlas QA - Export mailbox evidence (Christian Mora)` |
| Connection references | `atlas_qa_office365`, `atlas_qa_onedriveforbusiness` | `atlas_qa_office365_<key>`, `atlas_qa_onedriveforbusiness_<key>` |
| Workflow GUIDs | fixed template GUIDs | derived from the installation ID (SHA-256, stable across versions) |
| `AtlasInstallationId` compose | `unconfigured` | the persistent installation ID |

Colleagues in the same environment therefore get independent solutions. Importing a newer ZIP from the same installation updates that person's solution instead of creating a second one. The flows only act on files stamped with their own installation ID.

## Flows

1. **Atlas QA - Export mailbox evidence** (every 5 minutes). Reads `/AtlasBridge/qa/requests/qa-request.txt`. When it holds a request for this installation that has not been answered yet, it runs each query with *Get emails (V3)* (`searchQuery`, `top`), then writes one page per query and a `-done.json` marker to `/AtlasBridge/qa/inbox`.
2. **Atlas QA - Watch analyst mail** (*When a new email arrives (V3)*). If the sender is on `/AtlasBridge/qa/requests/qa-watch.txt`, it writes a signal file with that message to `/AtlasBridge/qa/inbox/live`. Atlas then syncs and evaluates straight away.

Connectors: Office 365 Outlook and OneDrive for Business (standard). Atlas never receives credentials.

## How Atlas drives it

- **History:** Atlas asks for monthly windows going back from today, eight queries per request, subject keywords plus audited participants. A window that returns the page limit is split in half and asked again. The import stops at the configured limit, or after six empty months older than the last QA activity.
- **Twice a day** (09:00 and 15:00 by default) and **every time an audited analyst writes to the manager**, Atlas requests the days since the last sync.
- Answers are stored locally, grouped by conversation, and evaluated with the bundled local AI. Results are written to one workbook per analyst, with one sheet per ISO week of the request.

## Rebuilding the template

Edit `solution-source/` and run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File power-automate-qa/pack-solution.ps1
```
