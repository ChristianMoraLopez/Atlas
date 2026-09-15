# Atlas Tracker Writer

This is the second, optional Atlas solution. The first `AtlasBridge` solution gathers Microsoft 365 evidence. Atlas personalizes `AtlasTrackerWriter_1_0_0_0.zip` with the selected SharePoint workbook and prepares the Office Script in the signed-in user's synchronized OneDrive.

Import the generated ZIP in **Power Automate → Solutions → Import solution**, map the OneDrive for Business, SharePoint, and Excel Online (Business) connections, and finish the import. Atlas writes one serialized package to `AtlasBridge/tracker-outbox/current.json`; the active writer checks it every five minutes and uses the hidden `_source_id` column to make retries idempotent.

The corporate tenant must allow Office Scripts and the importing user must have edit access to the workbook. The flow never stores a password, MFA response, browser cookie, or access token.
