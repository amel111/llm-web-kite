# llm-web-kite

Rust Axum web proxy for chatting with local Ollama models.

## Requirements

- Rust and Cargo
- Ollama installed and running
- At least one Ollama model

Check that Ollama is running:

```powershell
ollama list
```

Pull a model if needed:

```powershell
ollama pull llama3.2:1b
```

## Run

Open PowerShell in this project folder and set an API key:

```powershell
$env:PROXY_API_KEY = "choose-a-local-key"
cargo run --release
```

Open the web interface at:

```text
http://127.0.0.1:8090
```

Sign in with the value of `PROXY_API_KEY`.

The application uses Ollama at `http://127.0.0.1:11434` by default. The
selected model and Ollama URL are saved in the local `settings.json` file after
you change them in the web interface.

## Configuration

Set these environment variables before starting the server when needed:

```powershell
$env:PROXY_API_KEY = "choose-a-local-key"
$env:OLLAMA_URL = "http://127.0.0.1:11434"
$env:OLLAMA_MODEL = "llama3.2:1b"
cargo run --release
```

If `PROXY_API_KEY` is not set, the development fallback key is `change-me`.
Always set your own key before sharing the server or exposing it beyond your
local machine.

## Stop

Press `Ctrl+C` in the PowerShell window running the server.

## Python version

The Python implementation is maintained separately in
[`llm-web-vela`](../Temper-2-Ion).
