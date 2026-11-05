# Design: Chat Markdown Rendering for Temper Web UI

Date: 2026-11-04
Status: Approved

## Goal

Render assistant replies in the Temper web chat as Markdown with Rust syntax
highlighting and per-block copy buttons. Replace the current `textContent`
dump with a proper chat reading experience.

## Scope

Chat page only (`/` and `/chat/<cid>`). Settings/monitoring page, auth, API
surface, and data model are untouched.

## Decisions

- **Chat experience focus** — this is the first of possibly several UI
  improvements; later work (visual polish, security hardening) gets its own
  spec.
- **Chat features in scope**: Markdown rendering, Rust syntax highlighting,
  copy buttons on code blocks. Stream controls, conversation management, and
  composer upgrades are out of scope for this spec.
- **Vendor dependencies locally** in `static/vendor/` — the proxy is a
  self-hosted tool that may run offline; no new CDN dependencies.
- **DOMPurify** sanitizes every rendered HTML, streaming and final.
- **Hybrid streaming render** — prose renders live as chunks arrive; open
  code blocks accumulate raw until their fence closes.

## Approach

Extract the inline chat script from `templates/chat.html` into
`static/app.js` (the file the original 2026-11-04 web-ui spec planned but
never created — JS stayed inline), and vendor three small client libraries.
The backend does not change: raw Markdown stays in `messages.content`,
rendering happens client-side.

Rejected alternatives:
- Keep JS inline in `chat.html`: smallest diff, but the template grows by
  ~150 lines of renderer logic and stays untestable.
- Server-side Markdown render (python-markdown + Pygments): conflicts with
  the local-vendoring decision and makes the hybrid stream awkward over SSE.

## File layout

```
static/
  app.js                 # extracted chat client + markdown renderer
  m3.css                 # unchanged
  vendor/
    marked.min.js        # ~40KB, MIT
    purify.min.js        # ~20KB, MPL-2.0 or Apache-2.0
    highlight.min.js     # ~30KB core, BSD
    rust.min.js          # ~2KB Rust grammar for highlight.js, BSD
    LICENSES.txt         # license strings + exact pinned versions
templates/
  base.html              # add vendor <script> tags before {% block scripts %}
  chat.html              # drop inline <script>, load app.js
```

Pinned versions recorded in `LICENSES.txt` (e.g. `marked 12.x`,
`dompurify 3.x`, `highlight.js 11.x`) so future updates are deliberate.

## Component: app.js

Responsibilities:

1. **Conversation list / navigation** — moved verbatim from the inline script
   (`newConv`, conv list behavior).
2. **SSE reader** — current fetch + `ReadableStream` loop, unchanged wire
   format (`data: {...}` chunks, `data: [DONE]`, `usage` on final chunk).
3. **Hybrid markdown renderer**:
   - Accumulate the full raw answer string `full` as today.
   - Find the last unclosed ` ``` ` fence in `full`:
     - Prose before it: `marked.parse()` -> `DOMPurify.sanitize()` ->
       `innerHTML`, scheduled at most once per animation frame (~16ms) so
      fast chunks do not thrash the DOM.
     - Open code-block tail after the fence: append as escaped plain
       `<pre><code>` text, no highlighting, until the fence closes.
   - On `[DONE]`: one final `marked.parse` + `DOMPurify.sanitize`, then
     `hljs.highlightElement` on every `pre code` block. Blocks whose
     language is `rust` (or detected as Rust) use the vendored Rust grammar.
4. **Copy buttons** — after final render, inject one M3-styled
   `content_copy` icon button into each `pre` block. Click uses
   `navigator.clipboard.writeText` and shows the existing snackbar from
   `base.html` ("Copied"). Button is keyboard-focusable, label via `aria-label`.
5. **Scroll discipline** — auto-scroll to bottom only while the user is
   already at the bottom. Fixes current forced scroll (any paste or scroll-up
   fight against the stream).

Roles:
- User messages: inserted with `textContent`, safe by construction, unchanged
  from today.
- Assistant messages: raw markdown streamed into `full`, rendered through the
  pipeline above. Never `innerHTML`-ed without sanitization.

## Backend / data model

None. No new routes, no SSE format change, no DB change, no auth change.
`messages.content` continues to store raw markdown; the API key/session
posture is identical.

## Testing

1. **Manual stream test** against a local Ollama: ask for a Rust function,
   verify prose formats live, fenced code stays plain until the fence closes,
   then highlights; copy button copies; scroll stays put when scrolled up.
2. **XSS probe**: seed a message containing
   `<img src=x onerror=alert(1)>` and a `<script>` payload, render, confirm
   DOMPurify strips both (inspect DOM, no alert).
3. **Offline check**: serve with network disabled except Ollama; fonts
   failing is pre-existing, but vendor scripts must load from `/static/vendor/`.
4. **Regression**: non-streaming page load of an existing conversation
   (messages rendered server-side today via Jinja `{{ m.content }}`) must
   render through the same client pipeline without losing stored replies.

Note on (4): the page-load path currently renders server-side. As part of
this work the page-load render moves client-side too — `chat.html` emits
raw content into a data island (e.g. a `<script type="application/json">`
block or `data-*` attributes), and `app.js` runs every stored assistant
message through the same sanitize + highlight pipeline. One renderer, one
code path, streaming and history identical.

## Non-goals

- No stream controls (stop/regenerate/edit).
- No conversation rename/search/auto-title.
- No composer upgrades.
- No settings page changes.
- No new auth, CSRF, or secret-management work (tracked separately).
- No build step, no npm, no bundler — hand-vendored minified files only.
