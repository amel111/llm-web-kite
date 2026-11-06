use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use futures_util::StreamExt;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};
use tera::{Context, Tera};
use tokio::sync::Mutex;
use tower_http::{services::ServeDir, trace::TraceLayer};

const OLLAMA_URL: &str = "http://127.0.0.1:11434";
const MODEL: &str = "temper-2-ion";
const DB_PATH: &str = "usage.db";

#[derive(Clone)]
struct AppState {
    templates: Arc<Tera>,
    settings: Arc<Mutex<Value>>,
    stats: Arc<StdMutex<SessionStats>>,
}

#[derive(Default)]
struct SessionStats {
    requests: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    recent: Vec<Value>,
}

impl SessionStats {
    fn snapshot(&self) -> Value {
        json!({
            "requests": self.requests,
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "total_tokens": self.total_tokens,
        })
    }
}

#[derive(Serialize)]
struct HealthResponse<'a> {
    model: &'a str,
    ollama: &'a str,
    status: &'a str,
    version: &'a str,
}

async fn health() -> Json<HealthResponse<'static>> {
    Json(HealthResponse {
        model: "temper-2-ion",
        ollama: "http://127.0.0.1:11434",
        status: "ok",
        version: "2.0-rust",
    })
}

fn json_error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error": message})))
}

// OpenAI-compatible upstream failure shape, matching the Python backend.
fn upstream_error(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_GATEWAY, Json(json!({"error": {"message": message, "type": "upstream_error"}})))
}

fn init_db() -> rusqlite::Result<()> {
    let con = Connection::open(DB_PATH)?;
    con.execute_batch(
        "CREATE TABLE IF NOT EXISTS conversations (
           id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, model TEXT,
           created_at REAL NOT NULL, updated_at REAL NOT NULL
         );
         CREATE TABLE IF NOT EXISTS messages (
           id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id INTEGER NOT NULL,
           role TEXT NOT NULL, content TEXT NOT NULL, prompt_tokens INTEGER DEFAULT 0,
           completion_tokens INTEGER DEFAULT 0, created_at REAL NOT NULL, model TEXT,
           elapsed_s REAL, branch_of INTEGER, branch_group INTEGER
         );
         CREATE TABLE IF NOT EXISTS usage (
           id INTEGER PRIMARY KEY AUTOINCREMENT, ts REAL NOT NULL, model TEXT,
           prompt_tokens INTEGER, completion_tokens INTEGER,
           total_tokens INTEGER, elapsed_s REAL, endpoint TEXT
         );",
    )
}

fn now_unix() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn record_usage(stats: &StdMutex<SessionStats>, model: &str, pt: i64, ct: i64, elapsed: f64, endpoint: &str) {
    let record = json!({
        "ts": now_unix(),
        "model": model,
        "prompt_tokens": pt,
        "completion_tokens": ct,
        "total_tokens": pt + ct,
        "elapsed_s": (elapsed * 1000.0).round() / 1000.0,
    });
    if let Ok(mut session) = stats.lock() {
        session.requests += 1;
        session.prompt_tokens += pt;
        session.completion_tokens += ct;
        session.total_tokens += pt + ct;
        session.recent.push(record.clone());
    }
    if let Ok(con) = Connection::open(DB_PATH) {
        con.execute(
            "INSERT INTO usage (ts,model,prompt_tokens,completion_tokens,total_tokens,elapsed_s,endpoint) VALUES (?,?,?,?,?,?,?)",
            params![
                record["ts"].as_f64().unwrap_or_else(now_unix),
                model,
                pt,
                ct,
                pt + ct,
                record["elapsed_s"].as_f64().unwrap_or(elapsed),
                endpoint
            ],
        ).ok();
    }
}

fn touch_conversation(cid: i64) {
    if let Ok(con) = Connection::open(DB_PATH) {
        con.execute("UPDATE conversations SET updated_at=? WHERE id=?", params![now_unix(), cid]).ok();
    }
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut chain = format!("{error}");
    let mut source = error.source();
    while let Some(cause) = source {
        chain.push_str(" <- ");
        chain.push_str(&format!("{cause}"));
        source = cause.source();
    }
    chain
}

// ---- auth: same rules as the Python before_request ----
fn api_key_config() -> String {
    std::env::var("PROXY_API_KEY").unwrap_or_else(|_| "change-me".to_string())
}

fn web_password() -> String {
    std::env::var("WEB_PASSWORD").unwrap_or_else(|_| api_key_config())
}

fn bearer_or_header_ok(headers: &axum::http::HeaderMap, api_key: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.trim() == api_key)
        .unwrap_or(false)
        || headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == api_key)
            .unwrap_or(false)
}

async fn auth_middleware(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let skipped = path == "/health"
        || path == "/login"
        || path == "/logout"
        || path == "/login?error=1"
        || path.starts_with("/static/");
    if skipped {
        return next.run(request).await;
    }
    let api_key = api_key_config();
    let cookie_ok = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|value| {
            value.split(';').any(|part| part.trim() == "rust_authed=1")
        })
        .unwrap_or(false);
    let authed = cookie_ok || bearer_or_header_ok(request.headers(), &api_key);
    if authed {
        return next.run(request).await;
    }
    if path == "/" || path == "/settings" || path.starts_with("/chat") {
        return Redirect::to("/login").into_response();
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": {"message": "invalid api key", "type": "auth_error"}})),
    ).into_response()
}

fn is_authed(jar: &CookieJar, headers: &axum::http::HeaderMap) -> bool {
    jar.get("rust_authed").is_some() || bearer_or_header_ok(headers, &api_key_config())
}

// ---- template render ----
#[derive(Deserialize)]
struct ConvQuery {
    conv: Option<i64>,
}

async fn web_chat(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Query(query): Query<ConvQuery>,
) -> Response {
    render_chat(state, jar, headers, query.conv, false).await
}

async fn chat_by_id(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    render_chat(state, jar, headers, Some(id), false).await
}

async fn chat_new(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
) -> Response {
    render_chat(state, jar, headers, None, true).await
}

async fn render_chat(
    state: AppState,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    requested_id: Option<i64>,
    is_new: bool,
) -> Response {
    if !is_authed(&jar, &headers) {
        return Redirect::to("/login").into_response();
    }
    let active_id = if is_new {
        None
    } else {
        requested_id.or_else(|| {
            Connection::open(DB_PATH).ok().and_then(|con| {
                con.query_row("SELECT id FROM conversations ORDER BY updated_at DESC LIMIT 1", [], |row| row.get(0)).ok()
            })
        })
    };
    let conv_id_json = serde_json::to_string(&active_id).unwrap_or_else(|_| "null".to_string());
    let messages = active_id.map(list_messages).unwrap_or_default();
    let conversations = list_conversations();
    let mut context = Context::new();
    context.insert("active_id", &active_id);
    context.insert("conv_id_json", &conv_id_json);
    context.insert("messages", &messages);
    context.insert("messages_json", &serde_json::to_string(&messages).unwrap_or_else(|_| "[]".to_string()));
    let settings = state.settings.lock().await.clone();
    context.insert("model_json", &serde_json::to_string(&settings["model"]).unwrap_or_else(|_| format!("\"{MODEL}\"")));
    context.insert("ollama_json", &serde_json::to_string(&settings["ollama_url"]).unwrap_or_else(|_| format!("\"{OLLAMA_URL}\"")));
    context.insert("conv_groups", &group_conversations(conversations));
    context.insert("settings", &settings);
    context.insert("V", "rust");
    match state.templates.render("chat.html", &context) {
        Ok(body) => Html(body).into_response(),
        Err(error) => {
            tracing::error!("chat template render failed: {error:?}");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response()
        }
    }
}

fn list_conversations() -> Vec<Value> {
    let Ok(con) = Connection::open(DB_PATH) else { return Vec::new() };
    let Ok(mut statement) = con.prepare("SELECT id,title,model,created_at,updated_at FROM conversations ORDER BY updated_at DESC") else {
        return Vec::new();
    };
    statement.query_map([], |row| {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "title": row.get::<_, Option<String>>(1)?.unwrap_or_else(|| "Untitled".to_string()),
            "model": row.get::<_, Option<String>>(2)?,
            "created_at": row.get::<_, f64>(3)?,
            "updated_at": row.get::<_, f64>(4)?
        }))
    }).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
}

// Mirrors the Python _group_conversations: Today / Previous 7 days / Older,
// dropping empty groups. Rendered by chat.html as an ordered list of groups.
fn group_conversations(conversations: Vec<Value>) -> Value {
    let now = chrono::Local::now().date_naive();
    let week_ago = now - chrono::Duration::days(7);
    let mut today = Vec::new();
    let mut week = Vec::new();
    let mut older = Vec::new();
    for conversation in conversations {
        let stamp = conversation["updated_at"].as_f64().unwrap_or(0.0);
        let date = chrono::DateTime::from_timestamp(stamp as i64, 0)
            .map(|dt| dt.with_timezone(&chrono::Local).date_naive());
        match date {
            Some(date) if date >= now => today.push(conversation),
            Some(date) if date >= week_ago => week.push(conversation),
            _ => older.push(conversation),
        }
    }
    let mut groups = Vec::new();
    if !today.is_empty() {
        groups.push(json!({"name": "Today", "items": today}));
    }
    if !week.is_empty() {
        groups.push(json!({"name": "Previous 7 days", "items": week}));
    }
    if !older.is_empty() {
        groups.push(json!({"name": "Older", "items": older}));
    }
    Value::Array(groups)
}

fn list_messages(cid: i64) -> Vec<Value> {
    let Ok(con) = Connection::open(DB_PATH) else { return Vec::new() };
    let Ok(mut statement) = con.prepare(
        "SELECT id,role,content,prompt_tokens,completion_tokens,created_at,model,elapsed_s,branch_of,branch_group
         FROM messages WHERE conversation_id=? ORDER BY id"
    ) else { return Vec::new() };
    statement.query_map(params![cid], |row| {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "role": row.get::<_, String>(1)?,
            "content": row.get::<_, String>(2)?,
            "prompt_tokens": row.get::<_, i64>(3)?,
            "completion_tokens": row.get::<_, i64>(4)?,
            "created_at": row.get::<_, f64>(5)?,
            "model": row.get::<_, Option<String>>(6)?,
            "elapsed_s": row.get::<_, Option<f64>>(7)?,
            "branch_of": row.get::<_, Option<i64>>(8)?,
            "branch_group": row.get::<_, Option<i64>>(9)?
        }))
    }).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
}

async fn login_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    if jar.get("rust_authed").is_some() {
        return Redirect::to("/").into_response();
    }
    let mut context = Context::new();
    context.insert("error", &Value::Null);
    match state.templates.render("login.html", &context) {
        Ok(body) => Html(body).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login_post(
    State(state): State<AppState>,
    jar: CookieJar,
    axum::Form(form): axum::Form<LoginForm>,
) -> Response {
    if form.password == web_password() {
        return (jar.add(Cookie::new("rust_authed", "1")), Redirect::to("/")).into_response();
    }
    // Same as the Python backend: re-render the login page with an error, 401.
    let mut context = Context::new();
    context.insert("error", &"Invalid password");
    match state.templates.render("login.html", &context) {
        Ok(body) => (StatusCode::UNAUTHORIZED, Html(body)).into_response(),
        Err(_) => Redirect::to("/login").into_response(),
    }
}

async fn logout(jar: CookieJar) -> (CookieJar, Redirect) {
    (jar.remove(Cookie::from("rust_authed")), Redirect::to("/login"))
}

// ---- models (Ollama-backed, settings-aware) ----
async fn models(State(state): State<AppState>) -> Json<Value> {
    let ollama_url = {
        let settings = state.settings.lock().await;
        settings_string(&settings, "ollama_url", OLLAMA_URL)
    };
    let tags = match reqwest::get(format!("{ollama_url}/api/tags")).await {
        Ok(response) => response.json::<Value>().await.unwrap_or_else(|_| json!({"models": []})),
        Err(_) => json!({"models": []}),
    };
    let empty_models = Vec::new();
    let data: Vec<Value> = tags["models"]
        .as_array()
        .unwrap_or(&empty_models)
        .iter()
        .filter_map(|m| m["name"].as_str().map(|name| json!({"id": name, "object": "model", "owned_by": "ollama"})))
        .collect();
    Json(json!({"object": "list", "data": if data.is_empty() { vec![json!({"id": MODEL, "object": "model", "owned_by": "ollama"})] } else { data }}))
}

fn settings_string(settings: &Value, key: &str, fallback: &str) -> String {
    settings[key].as_str().unwrap_or(fallback).to_string()
}

fn settings_number(settings: &Value, key: &str, fallback: f64) -> f64 {
    settings[key].as_f64().unwrap_or(fallback)
}

async fn running_model_names(ollama_url: &str) -> Vec<String> {
    match reqwest::get(format!("{ollama_url}/api/ps")).await {
        Ok(response) => response.json::<Value>().await.ok()
            .and_then(|value| value["models"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|model| model["name"].as_str().map(str::to_string))
            .collect(),
        Err(_) => Vec::new(),
    }
}

async fn model_status(State(state): State<AppState>) -> Json<Value> {
    let (configured, ollama_url) = {
        let settings = state.settings.lock().await;
        (settings_string(&settings, "model", MODEL), settings_string(&settings, "ollama_url", OLLAMA_URL))
    };
    let running = running_model_names(&ollama_url).await;
    let base = configured.split(':').next().unwrap_or(&configured);
    let loaded = running.iter().any(|name| name == &configured || name.split(':').next() == Some(base));
    Json(json!({"model": configured, "loaded": loaded, "running": running}))
}

async fn new_conversation(State(state): State<AppState>) -> Json<Value> {
    let model = {
        let settings = state.settings.lock().await;
        settings_string(&settings, "model", MODEL)
    };
    let now = now_unix();
    let con = Connection::open(DB_PATH).expect("open database");
    con.execute("INSERT INTO conversations (title, model, created_at, updated_at) VALUES (?, ?, ?, ?)",
        params![Option::<String>::None, model, now, now]).expect("create conversation");
    Json(json!({"id": con.last_insert_rowid()}))
}

async fn delete_conversation(Path(id): Path<i64>) -> Json<Value> {
    let con = Connection::open(DB_PATH).expect("open database");
    con.execute("DELETE FROM messages WHERE conversation_id=?", params![id]).ok();
    con.execute("DELETE FROM conversations WHERE id=?", params![id]).ok();
    Json(json!({"ok": true}))
}

#[derive(Deserialize)]
struct RenameRequest {
    title: String,
}

async fn rename_conversation(Path(id): Path<i64>, Json(request): Json<RenameRequest>) -> Response {
    let title = request.title.trim();
    if title.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "missing title").into_response();
    }
    match Connection::open(DB_PATH).and_then(|con| {
        con.execute("UPDATE conversations SET title=? WHERE id=?",
            params![truncate_chars(title, 80), id])
    }) {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response(),
    }
}

fn add_message(
    cid: i64,
    role: &str,
    content: &str,
    pt: i64,
    ct: i64,
    model: Option<&str>,
    elapsed: Option<f64>,
    branch_of: Option<i64>,
    branch_group: Option<i64>,
) -> i64 {
    let con = Connection::open(DB_PATH).expect("open database");
    con.execute(
        "INSERT INTO messages (conversation_id,role,content,prompt_tokens,completion_tokens,created_at,model,elapsed_s,branch_of,branch_group)
         VALUES (?,?,?,?,?,?,?,?,?,?)",
        params![cid, role, content, pt, ct, now_unix(), model, elapsed, branch_of, branch_group],
    ).expect("insert message");
    con.last_insert_rowid()
}

// ---- selection of the same model settings as the web UI ----
fn chat_options(settings: &Value) -> Value {
    json!({
        "num_predict": settings_number(settings, "max_tokens", 1000.0),
        "num_ctx": settings_number(settings, "num_ctx", 32768.0),
        "temperature": settings_number(settings, "temperature", 0.2),
        "top_p": settings_number(settings, "top_p", 0.8),
        "top_k": settings_number(settings, "top_k", 20.0),
        "repeat_penalty": settings_number(settings, "repeat_penalty", 1.05)
    })
}

// Latest active user version per branch, preserving order (matches the Python
// _active_chat_messages logic used by the shared web UI).
fn active_chat_history(cid: i64) -> Value {
    let messages = list_messages(cid);
    let mut latest: HashMap<i64, i64> = HashMap::new();
    for m in &messages {
        if m["role"].as_str() != Some("user") {
            continue;
        }
        let mid = m["id"].as_i64().unwrap_or(0);
        let key = m["branch_group"].as_i64().or_else(|| m["branch_of"].as_i64()).unwrap_or(mid);
        latest.insert(key, mid);
    }
    let history: Vec<Value> = messages.into_iter()
        .filter(|m| {
            if m["role"].as_str() != Some("user") {
                return true;
            }
            let mid = m["id"].as_i64().unwrap_or(0);
            let key = m["branch_group"].as_i64().or_else(|| m["branch_of"].as_i64()).unwrap_or(mid);
            latest.get(&key) == Some(&mid)
        })
        .filter_map(|m| {
            let role = m["role"].as_str()?;
            let content = m["content"].as_str()?;
            Some(json!({"role": role, "content": content}))
        })
        .collect();
    if history.is_empty() {
        json!([{"role": "user", "content": ""}])
    } else {
        Value::Array(history)
    }
}

#[derive(Deserialize)]
struct SendRequest {
    conversation_id: i64,
    message: Option<String>,
    content: Option<String>,
    message_id: Option<i64>,
    #[serde(default)]
    skip_add_user: bool,
}

#[derive(Clone, Copy)]
enum SseMode {
    Web { cid: i64, endpoint: &'static str },
    OpenAI,
}

struct StreamState {
    buf: String,
    full: String,
    start: Instant,
    done: bool,
    pt: i64,
    ct: i64,
    eval_s: Option<f64>,
    model: String,
    mode: SseMode,
    persisted: bool,
}

fn sse_response(
    stats: Arc<StdMutex<SessionStats>>,
    response: reqwest::Response,
    model: String,
    mode: SseMode,
) -> Response {
    let start = Instant::now();
    let stream = response.bytes_stream().scan(
        StreamState {
            buf: String::new(),
            full: String::new(),
            start,
            done: false,
            pt: 0,
            ct: 0,
            eval_s: None,
            model,
            mode,
            persisted: false,
        },
        move |state, chunk| {
            let result = match chunk {
                Ok(bytes) => {
                    state.buf.push_str(&String::from_utf8_lossy(&bytes));
                    let mut output = String::new();
                    while let Some(pos) = state.buf.find('\n') {
                        let line = state.buf[..pos].trim().to_string();
                        state.buf.drain(..=pos);
                        if line.is_empty() {
                            continue;
                        }
                        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
                        let delta = value["message"]["content"].as_str().unwrap_or("");
                        if let Some(count) = value["prompt_eval_count"].as_i64() {
                            state.pt = count;
                        }
                        if let Some(count) = value["eval_count"].as_i64() {
                            state.ct = count;
                        }
                        let is_done = value["done"].as_bool() == Some(true);
                        match state.mode {
                            SseMode::Web { cid, endpoint } => {
                                if !delta.is_empty() {
                                    state.full.push_str(delta);
                                    output.push_str(&format!("data: {}\n\n", json!({
                                        "id": format!("c-{}", chrono::Utc::now().timestamp_millis()),
                                        "object": "chat.completion.chunk",
                                        "created": chrono::Utc::now().timestamp(),
                                        "model": state.model,
                                        "choices": [{"index": 0, "delta": {"content": delta}, "finish_reason": null}]
                                    })));
                                }
                                if is_done && !state.done {
                                    state.done = true;
                                    state.eval_s = value["eval_duration"].as_f64().map(|d| d / 1e9);
                                    let elapsed = state.eval_s.unwrap_or_else(|| state.start.elapsed().as_secs_f64());
                                    let model = state.model.clone();
                                    let full = state.full.clone();
                                    add_message(cid, "assistant", &full, state.pt, state.ct, Some(&model), Some(elapsed), None, None);
                                    record_usage(&stats, &model, state.pt, state.ct, elapsed, endpoint);
                                    touch_conversation(cid);
                                    state.persisted = true;
                                    output.push_str(&format!("data: {}\n\n", json!({
                                        "id": format!("c-{}", chrono::Utc::now().timestamp_millis()),
                                        "object": "chat.completion.chunk",
                                        "created": chrono::Utc::now().timestamp(),
                                        "model": model,
                                        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                                        "usage": {
                                            "prompt_tokens": state.pt,
                                            "completion_tokens": state.ct,
                                            "total_tokens": state.pt + state.ct,
                                            "model": model,
                                            "elapsed_s": (elapsed * 1000.0).round() / 1000.0
                                        }
                                    })));
                                    output.push_str("data: [DONE]\n\n");
                                }
                            }
                            SseMode::OpenAI => {
                                if !delta.is_empty() {
                                    state.full.push_str(delta);
                                }
                                let mut chunk = json!({
                                    "id": format!("chatcmpl-{}", chrono::Utc::now().timestamp_millis()),
                                    "object": "chat.completion.chunk",
                                    "created": chrono::Utc::now().timestamp(),
                                    "model": state.model,
                                    "choices": [{"index": 0, "delta": {"content": delta}, "finish_reason": null}]
                                });
                                if is_done {
                                    state.done = true;
                                    let elapsed = value["eval_duration"].as_f64().map(|d| d / 1e9)
                                        .unwrap_or_else(|| state.start.elapsed().as_secs_f64());
                                    chunk["choices"][0]["finish_reason"] = json!("stop");
                                    chunk["usage"] = json!({
                                        "prompt_tokens": state.pt,
                                        "completion_tokens": state.ct,
                                        "total_tokens": state.pt + state.ct
                                    });
                                    let model = state.model.clone();
                                    record_usage(&stats, &model, state.pt, state.ct, elapsed, "/v1/chat/completions");
                                    state.persisted = true;
                                    output.push_str(&format!("data: {}\n\n", chunk));
                                    output.push_str("data: [DONE]\n\n");
                                } else {
                                    output.push_str(&format!("data: {}\n\n", chunk));
                                }
                            }
                        }
                    }
                    Ok::<_, std::io::Error>(output)
                }
                Err(error) => {
                    // Mirror the Python finally-block: persist whatever streamed
                    // before the failure, once.
                    if !state.persisted {
                        let elapsed = state.start.elapsed().as_secs_f64();
                        let model = state.model.clone();
                        match state.mode {
                            SseMode::Web { cid, endpoint } => {
                                let full = state.full.clone();
                                add_message(cid, "assistant", &full, state.pt, state.ct, Some(&model), Some(elapsed), None, None);
                                record_usage(&stats, &model, state.pt, state.ct, elapsed, endpoint);
                                touch_conversation(cid);
                            }
                            SseMode::OpenAI => {
                                record_usage(&stats, &model, state.pt, state.ct, elapsed, "/v1/chat/completions");
                            }
                        }
                        state.persisted = true;
                    }
                    state.done = true;
                    let message = error_chain(&error);
                    Ok::<_, std::io::Error>(format!("data: {}\n\n", json!({"error": message})))
                }
            };
            futures_util::future::ready(Some(result))
        },
    );
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
        Body::from_stream(stream),
    ).into_response()
}

async fn stream_chat(State(state): State<AppState>, Json(request): Json<SendRequest>) -> Response {
    let content = request.content.clone()
        .or_else(|| request.message.clone())
        .map(|c| c.trim().to_string())
        .unwrap_or_default();
    if content.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "missing message").into_response();
    }
    let cid = request.conversation_id;
    let settings = state.settings.lock().await.clone();
    let model = settings_string(&settings, "model", MODEL);
    let ollama_url = settings_string(&settings, "ollama_url", OLLAMA_URL);

    // Edit path: keep the original as an older version (branch) and remove the
    // old response branch, mirroring the Python backend.
    let mut message_id: Option<i64> = None;
    if let Some(mid) = request.message_id {
        let con = match Connection::open(DB_PATH) {
            Ok(con) => con,
            Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response(),
        };
        let orig = con.query_row(
            "SELECT role, branch_of, branch_group FROM messages WHERE id=? AND conversation_id=?",
            params![mid, cid],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, Option<i64>>(2)?)),
        );
        match orig {
            Ok((role, orig_branch_of, orig_branch_group)) => {
                if role != "user" {
                    return json_error(StatusCode::BAD_REQUEST, "can only edit user messages").into_response();
                }
                message_id = Some(mid);
                if request.message_id.is_some() {
                    con.execute("DELETE FROM messages WHERE conversation_id=? AND id>?", params![cid, mid]).ok();
                }
                if !request.skip_add_user {
                    let branch_of = orig_branch_of.unwrap_or(mid);
                    let branch_group = orig_branch_group.unwrap_or(mid);
                    add_message(cid, "user", &content, 0, 0, None, None, Some(branch_of), Some(branch_group));
                    con.execute("UPDATE conversations SET title=? WHERE id=?", params![truncate_chars(&content, 40), cid]).ok();
                }
            }
            Err(_) => return json_error(StatusCode::NOT_FOUND, "not found").into_response(),
        }
    }
    if message_id.is_none() && !request.skip_add_user {
        add_message(cid, "user", &content, 0, 0, None, None, None, None);
        let con = Connection::open(DB_PATH).expect("open database");
        con.execute(
            "UPDATE conversations SET title=? WHERE id=? AND (title IS NULL OR title='')",
            params![truncate_chars(&content, 40), cid],
        ).ok();
    }

    let endpoint: &'static str = if request.message_id.is_some() {
        if request.skip_add_user { "/web/regenerate" } else { "/web/edit" }
    } else if request.skip_add_user {
        "/web/regenerate"
    } else {
        "/web/chat"
    };

    let payload = json!({
        "model": model,
        "messages": active_chat_history(cid),
        "stream": true,
        "options": chat_options(&settings)
    });
    let response = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(300))
        .build()
        .expect("build ollama client")
        .post(format!("{ollama_url}/api/chat"))
        .json(&payload)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let chain = error_chain(&error);
            tracing::error!("ollama chat failed: {chain}");
            return json_error(StatusCode::BAD_GATEWAY, &chain).into_response();
        },
    };
    if !response.status().is_success() {
        return json_error(StatusCode::BAD_GATEWAY, &format!("Ollama returned {}", response.status())).into_response();
    }
    sse_response(state.stats.clone(), response, model, SseMode::Web { cid, endpoint })
}

async fn regenerate(State(state): State<AppState>, Json(request): Json<SendRequest>) -> Response {
    let con = match Connection::open(DB_PATH) {
        Ok(con) => con,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response(),
    };
    // Python checks only the last message: drop it when it is an assistant
    // reply, otherwise leave the conversation untouched.
    let last: Option<(i64, String)> = con.query_row(
        "SELECT id, role FROM messages WHERE conversation_id=? ORDER BY id DESC LIMIT 1",
        params![request.conversation_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    ).ok();
    match last {
        None => return json_error(StatusCode::BAD_REQUEST, "empty").into_response(),
        Some((id, role)) => {
            if role == "assistant" {
                con.execute("DELETE FROM messages WHERE id=?", params![id]).ok();
            }
        }
    }
    drop(con);
    let history = list_messages(request.conversation_id);
    let last_user = history.iter().rev().find(|message| message["role"] == "user");
    let content = last_user.and_then(|message| message["content"].as_str()).unwrap_or("").to_string();
    stream_chat(
        State(state),
        Json(SendRequest {
            conversation_id: request.conversation_id,
            message: None,
            content: Some(content),
            message_id: None,
            skip_add_user: true,
        }),
    ).await
}

// ---- model load/unload (checks Ollama state first, like the Python version) ----
#[derive(Deserialize)]
struct ModelRequest {
    model: Option<String>,
}

async fn model_load(State(state): State<AppState>, Json(request): Json<ModelRequest>) -> Response {
    let (want, ollama_url) = {
        let settings = state.settings.lock().await;
        (request.model.clone().unwrap_or_else(|| settings_string(&settings, "model", MODEL)),
         settings_string(&settings, "ollama_url", OLLAMA_URL))
    };
    let loaded = running_model_names(&ollama_url).await.iter().any(|name| {
        let base = want.split(':').next().unwrap_or(&want);
        name == &want || name.split(':').next() == Some(base)
    });
    if loaded {
        return Json(json!({"ok": true, "model": want})).into_response();
    }
    match reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("build ollama client")
        .post(format!("{ollama_url}/api/generate"))
        .json(&json!({"model": want, "prompt": "hi"}))
        .send().await {
        Ok(response) if response.status().is_success() => Json(json!({"ok": true, "model": want})).into_response(),
        Ok(response) => json_error(StatusCode::BAD_GATEWAY, &format!("Ollama returned {}", response.status())).into_response(),
        Err(error) => json_error(StatusCode::BAD_GATEWAY, &error_chain(&error)).into_response(),
    }
}

async fn model_unload(State(state): State<AppState>, Json(request): Json<ModelRequest>) -> Response {
    let (model, ollama_url) = {
        let settings = state.settings.lock().await;
        (request.model.clone().unwrap_or_else(|| settings_string(&settings, "model", MODEL)),
         settings_string(&settings, "ollama_url", OLLAMA_URL))
    };
    match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build ollama client")
        .post(format!("{ollama_url}/api/generate"))
        .json(&json!({"model": model, "prompt": "", "keep_alive": 0})).send().await {
        Ok(response) if response.status().is_success() => Json(json!({"ok": true})).into_response(),
        Ok(response) => json_error(StatusCode::BAD_GATEWAY, &format!("Ollama returned {}", response.status())).into_response(),
        Err(error) => json_error(StatusCode::BAD_GATEWAY, &error_chain(&error)).into_response(),
    }
}

async fn save_settings(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let mut settings = state.settings.lock().await;
    if let Some(object) = input.as_object() {
        for key in ["ollama_url", "model", "temperature", "top_p", "top_k", "repeat_penalty", "max_tokens", "num_ctx", "input_price", "output_price", "usd_to_idr"] {
            if let Some(value) = object.get(key) {
                settings[key] = value.clone();
            }
        }
    }
    match serde_json::to_string_pretty(&*settings)
        .ok()
        .and_then(|text| std::fs::write("settings.json", text).ok()) {
        Some(()) => Json(json!({"ok": true})).into_response(),
        None => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to save settings").into_response(),
    }
}

async fn test_settings(State(state): State<AppState>, Json(input): Json<Value>) -> Json<Value> {
    let fallback = {
        let settings = state.settings.lock().await;
        settings_string(&settings, "ollama_url", OLLAMA_URL)
    };
    let url = input["ollama_url"].as_str().unwrap_or(&fallback).trim_end_matches('/');
    match reqwest::get(format!("{url}/api/tags")).await {
        Ok(response) if response.status().is_success() => {
            let count = response.json::<Value>().await.ok()
                .and_then(|value| value["models"].as_array().map(Vec::len)).unwrap_or(0);
            Json(json!({"ok": true, "count": count}))
        }
        Ok(response) => Json(json!({"ok": false, "error": format!("Ollama returned {}", response.status())})),
        Err(error) => Json(json!({"ok": false, "error": error.to_string()})),
    }
}

// ---- stats: totals + recent requests, and CSV export ----
async fn stats(State(state): State<AppState>) -> Response {
    let zero = json!({"requests": 0, "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0});
    let totals = Connection::open(DB_PATH).ok().and_then(|con| {
        con.query_row(
            "SELECT COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0) FROM usage",
            [], |row| Ok(json!({
                "requests": row.get::<_, i64>(0)?,
                "prompt_tokens": row.get::<_, i64>(1)?,
                "completion_tokens": row.get::<_, i64>(2)?,
                "total_tokens": row.get::<_, i64>(3)?
            }))
        ).ok()
    }).unwrap_or_else(|| zero.clone());
    let (session, recent) = match state.stats.lock() {
        Ok(session) => (session.snapshot(), session.recent.clone()),
        Err(_) => (zero.clone(), Vec::new()),
    };
    Json(json!({"totals": totals, "recent": recent, "session": session})).into_response()
}

async fn stats_export() -> Response {
    let mut body = String::from("ts,model,prompt_tokens,completion_tokens,total_tokens,elapsed_s,endpoint\n");
    if let Ok(con) = Connection::open(DB_PATH) {
        if let Ok(mut statement) = con.prepare(
            "SELECT ts,model,prompt_tokens,completion_tokens,total_tokens,elapsed_s,endpoint FROM usage ORDER BY id"
        ) {
            let rows: Vec<String> = statement.query_map([], |row| {
                let ts: f64 = row.get(0)?;
                let model: Option<String> = row.get(1)?;
                let pt: i64 = row.get(2)?;
                let ct: i64 = row.get(3)?;
                let tt: i64 = row.get(4)?;
                let elapsed: Option<f64> = row.get(5)?;
                let endpoint: Option<String> = row.get(6)?;
                Ok(vec![
                    format!("{ts}"),
                    model.unwrap_or_default(),
                    format!("{pt}"),
                    format!("{ct}"),
                    format!("{tt}"),
                    elapsed.map(|e| format!("{e}")).unwrap_or_default(),
                    endpoint.unwrap_or_default(),
                ].join(","))
            }).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default();
            for row in rows {
                body.push_str(&row);
                body.push('\n');
            }
        }
    }
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("text/csv")),
            (header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=usage.csv")),
        ],
        body,
    ).into_response()
}

// ---- OpenAI-compatible API ----
fn build_chat_options(input: &Value) -> Value {
    json!({
        "temperature": input["temperature"].as_f64().unwrap_or(0.2),
        "top_p": input["top_p"].as_f64().unwrap_or(0.8),
        "top_k": input["top_k"].as_i64().unwrap_or(20),
        "repeat_penalty": input["repeat_penalty"].as_f64().unwrap_or(1.05),
        "num_predict": input["max_tokens"].as_i64().unwrap_or(1000)
    })
}

async fn openai_chat(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let model = input["model"].as_str().unwrap_or(MODEL).to_string();
    let messages = input["messages"].as_array().cloned().unwrap_or_default();
    let ollama_url = {
        let settings = state.settings.lock().await;
        settings_string(&settings, "ollama_url", OLLAMA_URL)
    };
    let wants_stream = input["stream"].as_bool().unwrap_or(false);
    let payload = if wants_stream {
        json!({"model": model, "messages": messages, "stream": true, "options": build_chat_options(&input)})
    } else {
        json!({"model": model, "messages": messages, "stream": false, "options": build_chat_options(&input)})
    };
    if wants_stream {
        let response = match reqwest::Client::new().post(format!("{ollama_url}/api/chat")).json(&payload).send().await {
            Ok(response) => response,
            Err(error) => return json_error(StatusCode::BAD_GATEWAY, &error_chain(&error)).into_response(),
        };
        if !response.status().is_success() {
            return json_error(StatusCode::BAD_GATEWAY, &format!("Ollama returned {}", response.status())).into_response();
        }
        return sse_response(state.stats.clone(), response, model, SseMode::OpenAI);
    }
    let started = Instant::now();
    let response = match reqwest::Client::new().post(format!("{ollama_url}/api/chat")).json(&payload).send().await {
        Ok(response) => response,
        Err(error) => return upstream_error(&error_chain(&error)).into_response(),
    };
    if !response.status().is_success() {
        return upstream_error(&format!("Ollama returned {}", response.status())).into_response();
    }
    let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    let content = body["message"]["content"].as_str().unwrap_or("");
    let prompt_tokens = body["prompt_eval_count"].as_i64().unwrap_or(0);
    let completion_tokens = body["eval_count"].as_i64().unwrap_or(0);
    let total = prompt_tokens + completion_tokens;
    record_usage(&state.stats, &model, prompt_tokens, completion_tokens, started.elapsed().as_secs_f64(), "/v1/chat/completions");
    Json(json!({
        "id": format!("chatcmpl-{}", chrono::Utc::now().timestamp_millis()),
        "object": "chat.completion", "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": prompt_tokens, "completion_tokens": completion_tokens, "total_tokens": total}
    })).into_response()
}

async fn openai_completion(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let model = input["model"].as_str().unwrap_or(MODEL).to_string();
    let prompt = input["prompt"].as_str().unwrap_or("");
    let ollama_url = {
        let settings = state.settings.lock().await;
        settings_string(&settings, "ollama_url", OLLAMA_URL)
    };
    let payload = json!({"model": model, "prompt": prompt, "stream": false, "options": build_chat_options(&input)});
    let started = Instant::now();
    let response = match reqwest::Client::new().post(format!("{ollama_url}/api/generate")).json(&payload).send().await {
        Ok(response) => response,
        Err(error) => return upstream_error(&error_chain(&error)).into_response(),
    };
    if !response.status().is_success() {
        return upstream_error(&format!("Ollama returned {}", response.status())).into_response();
    }
    let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    let prompt_tokens = body["prompt_eval_count"].as_i64().unwrap_or(0);
    let completion_tokens = body["eval_count"].as_i64().unwrap_or(0);
    record_usage(&state.stats, &model, prompt_tokens, completion_tokens, started.elapsed().as_secs_f64(), "/v1/completions");
    Json(json!({
        "id": format!("cmpl-{}", chrono::Utc::now().timestamp_millis()),
        "object": "text_completion", "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{"text": body["response"].as_str().unwrap_or(""), "index": 0, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": prompt_tokens, "completion_tokens": completion_tokens, "total_tokens": prompt_tokens + completion_tokens}
    })).into_response()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    init_db().expect("failed to initialize SQLite database");
    let mut templates = Tera::new("templates/**/*").expect("failed to load templates");
    templates.autoescape_on(vec!["html"]);
    let settings = std::fs::read_to_string("settings.json")
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({"model": MODEL, "ollama_url": OLLAMA_URL}));
    let state = AppState {
        templates: Arc::new(templates),
        settings: Arc::new(Mutex::new(settings)),
        stats: Arc::new(StdMutex::new(SessionStats::default())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/", get(web_chat))
        .route("/chat/new", get(chat_new))
        .route("/chat/{id}", get(chat_by_id))
        .route("/login", get(login_get).post(login_post))
        .route("/logout", get(logout))
        .route("/api/models", get(models))
        .route("/api/model_status", get(model_status))
        .route("/api/model_load", post(model_load))
        .route("/api/model_unload", post(model_unload))
        .route("/models", get(models))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(openai_chat))
        .route("/v1/completions", post(openai_completion))
        .route("/api/conversations/new", post(new_conversation))
        .route("/api/conversations/{id}", delete(delete_conversation))
        .route("/api/conversations/{id}/rename", post(rename_conversation))
        .route("/api/chat/send", post(stream_chat))
        .route("/api/chat/edit", post(stream_chat))
        .route("/api/chat/regenerate", post(regenerate))
        .route("/api/settings", post(save_settings))
        .route("/api/settings/test", post(test_settings))
        .route("/api/stats", get(stats))
        .route("/stats", get(stats))
        .route("/stats/export", get(stats_export))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn(auth_middleware));
    let address = SocketAddr::from(([127, 0, 0, 1], 8090));

    tracing::info!("Rust proxy listening on {}", address);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("failed to bind Rust proxy listener");
    axum::serve(listener, app)
        .await
        .expect("Rust proxy server failed");
}
