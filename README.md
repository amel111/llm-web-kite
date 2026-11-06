<div align="center">
  <img src="assets/kite-banner.svg" alt="Kite — local AI, on your terms" width="100%">

  <h1>Kite</h1>
  <p><strong>A small, self-hosted web UI and API bridge for Ollama.</strong></p>
  <p>Chat with local models, keep conversations on your machine, and connect OpenAI-compatible clients to your Ollama server.</p>

  <p><code>RUST 2024</code> &nbsp;·&nbsp; <code>AXUM</code> &nbsp;·&nbsp; <code>OLLAMA</code></p>
</div>

---

## What is Kite?

Kite is a Rust and Axum application that gives local Ollama models a browser-based chat interface and an OpenAI-compatible API. It runs on your computer, stores chat history in a local SQLite database, and streams model responses to the UI.

### Highlights

- **Local-first chat** — conversations and usage data are stored in `usage.db` on the machine running Kite.
- **Streaming responses** — chat output appears as the model generates it, including thinking data when the model supports it.
- **Conversation controls** — create, rename, delete, edit, and regenerate conversations and replies.
- **Ollama model controls** — browse available models and load or unload models from the UI.
- **OpenAI-compatible endpoints** — expose chat and completion routes for compatible clients.
- **No external image dependency** — the Rust and Ollama illustrations in this README are stored in this repository.

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) with Cargo (Rust 2024 edition support; Rust 1.85 or newer)
- [Ollama](https://ollama.com/download) installed and running
- At least one model downloaded in Ollama

For example, download a small model:

```sh
ollama pull llama3.2:1b
```

Kite's built-in default model is `temper-2-ion`. Set `OLLAMA_MODEL` to a model you have installed, or select a model in Kite's settings after launch.

## Run from source

Run these commands from the repository root. Keep the `templates/` and `static/` directories alongside the executable; Kite loads them at startup.

### Windows (PowerShell)

```powershell
$env:PROXY_API_KEY = "choose-a-strong-local-key"
$env:OLLAMA_MODEL = "llama3.2:1b"
cargo run --release
```

### Linux and macOS

```sh
export PROXY_API_KEY='choose-a-strong-local-key'
export OLLAMA_MODEL='llama3.2:1b'
cargo run --release
```

Open **http://127.0.0.1:8090** in your browser and sign in with the `PROXY_API_KEY` value. Stop the server with **Ctrl+C** in its terminal.

> **Security note:** Kite binds to `127.0.0.1` by default, so it listens on the local machine only. Use a strong API key even for local use. Do not expose the service publicly without putting an appropriately secured reverse proxy and network controls in front of it.

## Configuration

Set these environment variables before launching Kite:

| Variable | Default | Purpose |
| --- | --- | --- |
| `PROXY_API_KEY` | `change-me` | Login/API key. Set your own value. |
| `OLLAMA_URL` | `http://127.0.0.1:11434` | Ollama server URL. |
| `OLLAMA_MODEL` | `temper-2-ion` | Default model when no model is stored in settings. |
| `PROXY_PORT` | `8090` | Local TCP port Kite listens on. |
| `KITE_DB_PATH` | `<project directory>/usage.db` | SQLite database file path. |
| `KITE_SETTINGS_PATH` | `settings.json` | Settings file path. |

The selected model and Ollama URL can be changed in Kite's settings page and are saved in `settings.json`. Run Kite from the repository root so its relative `settings.json`, `templates/`, and `static/` paths resolve correctly. The database defaults to the Cargo project directory.

## API

Kite provides these OpenAI-compatible routes:

- `GET /v1/models`
- `POST /v1/chat/completions` (streaming and non-streaming)
- `POST /v1/completions`

The `/v1/` routes accept a bearer API key. The web UI uses the login session. `GET /health` is available without authentication.

Example request:

```sh
curl http://127.0.0.1:8090/v1/chat/completions \
  -H 'Authorization: Bearer choose-a-strong-local-key' \
  -H 'Content-Type: application/json' \
  -d '{"model":"llama3.2:1b","messages":[{"role":"user","content":"Hello!"}]}'
```

## Build and test

```sh
cargo check
cargo test
cargo build --release
```

The release binary is written under `target/release/` (`llm-web-kite.exe` on Windows, `llm-web-kite` on Linux and macOS). The UI templates and `static/` assets are runtime files and should remain available next to the project when starting the server.

## Project artwork

The README banner is an original local SVG featuring Rust and Ollama-inspired marks: [`assets/kite-banner.svg`](assets/kite-banner.svg). It requires no remote image host to render.

---

<div align="center">
  <sub>Built with Rust · Axum · Ollama</sub>
</div>
