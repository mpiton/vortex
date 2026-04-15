# Download Error Popover Design

## Summary

Add a small circular error-info icon next to the `Error` state in the downloads table. Clicking the icon opens a small anchored bubble that shows the raw backend error message with no filtering or truncation beyond normal wrapping.

This feature applies only to rows whose state is `Error`. It must survive query invalidation, page refresh, and app restart.

## Goals

- Let users inspect the real backend failure directly from the table.
- Avoid forcing users to open the detail panel or scan logs for common failures.
- Preserve the exact backend error string so TLS, certificate, proxy, and transport failures remain diagnosable.

## Non-Goals

- Show error details for `Retry`, `Paused`, `Completed`, or other non-error states.
- Reconstruct the message by parsing download logs.
- Introduce hover-only behavior.
- Add insecure TLS bypasses or alter retry policy as part of this feature.

## Decision

Use a persisted nullable `error_message` field on the `downloads` read/write model, expose it through `DownloadView`, and render a click-triggered inline bubble from that field in the downloads table.

This is preferred over a frontend-only event cache because the message must survive refresh and restart, and over log parsing because logs are derived presentation output rather than the source of truth.

## Data Model

Add a nullable `error_message` column to the `downloads` table.

Populate `error_message` when a download transitions to `Error`.

Clear `error_message` when a download leaves the failed state through any flow that represents a fresh attempt or terminal success, including:

- manual retry
- automatic retry
- resume/start flows that restart work after a previous failure
- successful completion
- removal

The exact stored value is the raw backend failure string already carried by `DomainEvent::DownloadFailed.error`.

## Backend Changes

### Persistence

- Add a new SQLite migration that appends `error_message` to `downloads`.
- Extend the SeaORM download entity with `error_message: Option<String>`.
- Extend any reconstruction and persistence mapping needed so the field can be read and written without affecting domain invariants.

### Domain and application flow

The domain aggregate does not currently retain the failure message. That is acceptable for this feature. The backend should persist the message at the application/repository layer when handling failure transitions rather than promoting it into core domain state.

Expected behavior:

- on failure: persist state `Error` and the raw failure string
- on retry/restart/resume from failed attempt: clear `error_message`
- on success: clear `error_message`

### Read models

- Add `error_message: Option<String>` to the Rust `DownloadView`.
- Add `errorMessage?: string | null` to the frontend `DownloadView` type.
- Expose the field through `DownloadViewDto` in camelCase.
- Update the SQLite read repository query mapping so list rows receive the stored value directly.

`DownloadDetailView` does not need to be extended for the first version because the requested interaction is list-only.

## Frontend Changes

### State cell

Extend `StateIndicator` so it can optionally receive `errorMessage`.

When:

- `state !== 'Error'`: render the current colored dot and label only
- `state === 'Error'` and `errorMessage` is absent: render the current error label only
- `state === 'Error'` and `errorMessage` exists: render the error label plus a small circular info button

### Click interaction

Use a click-triggered anchored bubble attached to the icon. The bubble should:

- stay open until outside click, second click, or `Escape`
- support wrapped multi-line text
- handle long TLS and transport errors without horizontal overflow
- avoid affecting row selection or row-click behavior

The visual control should stay compact and secondary to the main status label.

### Component strategy

If the existing tooltip primitive cannot reliably support click-persisted behavior, add a small local popover-style component or use an available Radix primitive already present in the dependency set. Do not overload hover tooltip behavior for this requirement.

## Error Handling and Edge Cases

- If a failed row has no stored `error_message`, the icon is omitted rather than showing placeholder text.
- If a retry starts and the row state changes away from `Error`, the icon disappears immediately with the state change.
- Existing rows created before the migration default to `NULL` and show no icon until a future failure writes a message.

## Testing

### Backend

- migration and entity mapping cover the new nullable column
- failure handling persists the raw error message
- retry/restart flows clear the persisted message
- successful completion clears the persisted message
- `DownloadViewDto` serializes `errorMessage` in camelCase

### Frontend

- `StateIndicator` shows no icon for non-error states
- `StateIndicator` shows no icon for `Error` without `errorMessage`
- `StateIndicator` shows the icon for `Error` with `errorMessage`
- clicking the icon reveals the full backend error string
- clicking the icon does not trigger row actions or selection side effects

## Implementation Notes

- Keep the raw string untouched; do not sanitize or shorten it for this surface.
- Reuse existing table styling and spacing so the new affordance looks native to the current downloads view rather than like a separate widget.
