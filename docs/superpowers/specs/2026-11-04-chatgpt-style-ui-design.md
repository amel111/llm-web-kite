# Design: ChatGPT-style UI Revamp for Temper Web UI

Date: 2026-11-04
Status: Approved

## Goal

Redesign the Temper web UI to match the chatgpt.com layout and visual
language: full-clone fidelity across chat, settings modal, and login.

## Decisions

- **Full clone** of the chatgpt.com layout (validated against browser
  mockups `layout-v1/v2`, `style-palette`).
- **Everything in scope**: chat page, settings converted to a modal, login
  restyled to a plain centered card.
- **Theme**: light default, manual toggle in sidebar footer, stored in
  localStorage, applied via `[data-theme]` on `<html>` (overrides
  `prefers-color-scheme`).
- **Message actions**: copy (client-side clipboard), regenerate, and
  edit-and-resend (user messages). Regenerate and edit require new backend
  endpoints.
- **Model picker**: dropdown in the top bar listing live models from
  `/api/models`; picking one persists via `/api/settings` (global default,
  same as today's settings behavior).
- **Icons**: inline stroke SVGs. Material Symbols + Google Fonts (Roboto)
  links removed — no icon font, no web fonts. `system-ui` stack.
- **CSS**: new `static/ui.css` with ChatGPT token set (light + dark);
  `static/m3.css` and the M3 token system are retired for the web UI.
  M3 purple `#6750A4` survives only as the default avatar fill.

## File layout

```
static/
  ui.css                 # new: ChatGPT tokens, light/dark, all components
  app.js                 # rewritten: sidebar, thread, composer, actions, modal
  vendor/                # unchanged (marked, purify, highlight.js)
templates/
  base.html              # no nav rail, no Google Fonts, theme bootstrap, modal shell
  chat.html              # full chat markup + settings modal markup
  login.html             # restyled centered card
  (settings.html         # deleted — settings is a modal now)
proxy.py                 # + /api/chat/regenerate, /api/chat/edit, /api/conversations/<id>/rename
```

## Components

### Sidebar (260px)
- New chat button (menu icon + pencil), search input filtering the rendered
  conversation list client-side.
- Conversations grouped by `updated_at`: Today / Previous 7 days / Older
  (computed server-side in Jinja).
- Per-item hover menu `…`: Rename (inline edit), Delete (existing
  `DELETE /api/conversations/<id>`).
- Footer: user chip (first letter avatar, `WEB_PASSWORD` string?), theme
  toggle, settings gear (opens modal).
- Collapse: hamburger in main area toggles sidebar (state in localStorage);
  mobile (<768px): sidebar becomes an overlay drawer.

### Top bar
- Model name + chevron dropdown (live `/api/models`, POST `/api/settings` on
  pick, updates label).
- New-chat pencil icon on the right.

### Thread (48rem centered)
- User messages: right-aligned bubble (`#f4f4f4` / `#2f2f33`).
- Assistant messages: full width, markdown + dark code card + per-code-block
  copy button (existing pipeline).
- Hover actions under the last assistant message: copy, regenerate; edit
  pencil on user bubbles.
- Token meta `in=… out=…` in the action row.
- Empty state (no messages): centered `Temper-2-Ion` wordmark + 4 suggestion
  chips (Rust-flavored); click fills composer and sends.
- Streaming: blinking caret at tail while streaming.

### Composer
- Pill `border-radius:26px`, autosizing textarea (up to 200px), attach `+`
  icon (decorative in v1), send button circle.
- Send button becomes a stop button (filled square) while streaming;
  stop aborts the reader (`reader.cancel()`), partial text is kept as the
  assistant message (server `finally` persists what streamed).
- Enter sends, Shift+Enter newline (unchanged).

### Settings modal
- Centered sheet, sections: Connection (Ollama URL, model, Test connection),
  Pricing (`input_price`, `output_price`, `usd_to_idr`), Usage totals
  (auto-refresh every 5s via `/api/stats`), Recent requests table, Export CSV.
- Same endpoints as the old page (`/api/settings`, `/api/settings/test`,
  `/api/stats`, `/stats/export`); sampling card stays removed.

### Login
- Centered card: wordmark, password field, sign-in button. Uses `ui.css`.

## Backend additions (proxy.py)

All cookie-auth (same as existing `/api/*` web routes):

```
POST /api/chat/regenerate    {conversation_id}
    delete last assistant message, re-stream reply over the remaining history
POST /api/chat/edit          {message_id, content}
    message must be that conversation's user message:
    update its content, delete every message with id > message_id in the conv,
    re-stream the reply
POST /api/conversations/<id>/rename  {title}
    update conversations.title
```

Streaming response bodies reuse the same SSE shape as `/api/chat/send`.
Refactor the send generator into a shared `_stream_chat(cid, history, model,
endpoint)` helper so send / regenerate / edit share one code path.

Data model: unchanged. `conversations.model` already exists; the top-bar
picker writes the global default only (per-conversation model is display-only
for now).

`GET /settings` redirects to `/`.

## Testing

1. Login, empty state, suggestion chip click → streams.
2. Markdown/hljs/copy still work on page reload (history) and live stream —
   regression of the markdown renderer.
3. Regenerate replaces last answer; edit truncates branch and re-streams.
4. Theme toggle persists over reload; dark palette verified.
5. Model picker lists live models and a pick changes the model used by the
   next send.
6. Settings modal: save settings, test connection, stats refresh, CSV export.
7. Sidebar: search filter, rename, delete, collapse, mobile drawer width.
8. Stop button mid-stream keeps partial text as assistant message.
9. XSS probe message still sanitized by DOMPurify.

## Non-goals

- File upload (attach icon decorative).
- Message branching beyond delete-and-restream (chat tree).
- Per-conversation model overrides beyond display.
- Voice input, prompt templates library.
- Backend auth overhaul / CSRF (separate workstream).
