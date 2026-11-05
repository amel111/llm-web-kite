# Design: Temper Web UI (Chat + Settings/Monitoring)

Date: 2026-11-04
Status: Approved (pending)

## Goal
Add a browser UI to the existing Temper proxy: a chat page and a settings/monitoring page. Persist chat history in SQLite. Style with Material Design 3 tokens (CSS custom properties, since @material/web is maintenance-only).

## Scope (v1)
- Web chat: conversation list, message thread, send, streaming response, new conversation.
- Settings/monitoring: totals + recent usage, cost simulation (USD/IDR), token export CSV, model selector, sampling params, Ollama URL, pricing, connection test, proxy info.
- Auth: single password (PROXY_API_KEY by default, overridable via WEB_PASSWORD) stored in session cookie.
- No user accounts, no multi-tenant.

## Architecture
- Single Flask app (proxy.py) serves both API (`/v1/*`) and web (`/`, `/chat`, `/settings`).
- Templates in `templates/`, static CSS in `static/`.
- SQLite DB `usage.db` extended with `conversations`, `messages` tables.
- Settings persisted to `settings.json` (model, sampling, ollama_url, pricing). Loaded at startup; editable via settings page.

## Routes
- `GET/POST /login`, `POST /logout`
- `GET /` -> chat (redirect to /login if no session)
- `GET /chat/<conv_id>` -> chat thread
- `POST /api/chat/send` -> send message, stream response, persist
- `POST /api/conversations/new`, `DELETE /api/conversations/<id>`
- `GET /settings` -> settings + monitoring
- `POST /api/settings` -> save settings
- `POST /api/settings/test` -> test Ollama connection
- `GET /api/stats`, `GET /api/stats/export` (already exist)
- `GET /api/models`

## Data model
conversations(id, title, model, created_at, updated_at)
messages(id, conversation_id, role, content, prompt_tokens, completion_tokens, created_at)

## M3 Styling
- CSS custom properties for color/typography/shape tokens (light + dark via prefers-color-scheme).
- Roboto Flex font, Material Symbols icons.
- Components: top app bar, navigation rail (desktop) / bottom nav (mobile), cards, text fields, buttons, tables, snackbars.

## Non-goals
- No SSR of web components; plain HTML + CSS + small JS.
- No multi-user.
- No HTTPS (use reverse proxy).

## Files touched
- proxy.py: add web routes, DB tables, settings load/save, session auth.
- templates/: base.html, login.html, chat.html, settings.html
- static/: m3.css, app.js
- temper.py: add `web` open command maybe.
