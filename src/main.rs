use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use futures_util::StreamExt;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};
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
           prompt_tokens INTEGER DEFAULT 0, completion_tokens INTEGER DEFAULT 0,
           total_tokens INTEGER DEFAULT 0, elapsed_s REAL, endpoint TEXT
         );",
    )
}

fn is_authed(jar: &CookieJar, headers: &axum::http::HeaderMap) -> bool {
    let api_key = std::env::var("PROXY_API_KEY").unwrap_or_else(|_| "change-me".to_string());
    jar.get("rust_authed").is_some()
        || headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|v| v == api_key)
            .unwrap_or(false)
        || headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == api_key)
            .unwrap_or(false)
}

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
    render_chat(state, jar, headers, query.conv).await
}

async fn chat_by_id(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    render_chat(state, jar, headers, Some(id)).await
}

async fn render_chat(
    state: AppState,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    requested_id: Option<i64>,
) -> Response {
    if !is_authed(&jar, &headers) {
        return Redirect::to("/login").into_response();
    }
    let active_id = requested_id.or_else(|| {
        Connection::open(DB_PATH).ok().and_then(|con| {
            con.query_row("SELECT id FROM conversations ORDER BY updated_at DESC LIMIT 1", [], |row| row.get(0)).ok()
        })
    });
    let messages = active_id.map(list_messages).unwrap_or_default();
    let conversations = list_conversations();
    let mut context = Context::new();
    context.insert("active_id", &active_id);
    context.insert("messages", &messages);
    context.insert("messages_json", &serde_json::to_string(&messages).unwrap_or_else(|_| "[]".to_string()));
    let settings = state.settings.lock().await.clone();
    context.insert("model_json", &serde_json::to_string(&settings["model"]).unwrap_or_else(|_| "\"temper-2-ion\"".to_string()));
    context.insert("ollama_json", &serde_json::to_string(&settings["ollama_url"]).unwrap_or_else(|_| "\"http://127.0.0.1:11434\"".to_string()));
    context.insert("conv_groups", &json!({"Today": conversations}));
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
            "title": row.get::<_, Option<String>>(1)?.unwrap_or_else(|| "New chat".to_string()),
            "model": row.get::<_, Option<String>>(2)?,
            "created_at": row.get::<_, f64>(3)?,
            "updated_at": row.get::<_, f64>(4)?
        }))
    }).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
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

async fn login_get(State(state): State<AppState>) -> Response {
    match state.templates.render("login.html", &Context::new()) {
        Ok(body) => Html(body).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login_post(jar: CookieJar, axum::Form(form): axum::Form<LoginForm>) -> (CookieJar, Redirect) {
    let api_key = std::env::var("PROXY_API_KEY").unwrap_or_else(|_| "change-me".to_string());
    if form.password == api_key {
        return (jar.add(Cookie::new("rust_authed", "1")), Redirect::to("/"));
    }
    (jar, Redirect::to("/login"))
}

async fn logout(jar: CookieJar) -> (CookieJar, Redirect) {
    (jar.remove(Cookie::from("rust_authed")), Redirect::to("/login"))
}

async fn models() -> Json<Value> {
    let result = reqwest::get(format!("{OLLAMA_URL}/api/tags")).await;
    let tags = match result {
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

async fn model_status() -> Json<Value> {
    let response = reqwest::get(format!("{OLLAMA_URL}/api/ps")).await;
    let running: Vec<String> = match response {
        Ok(response) => response.json::<Value>().await.ok()
            .and_then(|value| value["models"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|model| model["name"].as_str().map(str::to_string))
            .collect(),
        Err(_) => Vec::new(),
    };
    let configured = MODEL.to_string();
    let base = configured.split(':').next().unwrap_or(&configured);
    let loaded = running.iter().any(|name| name == &configured || name.split(':').next() == Some(base));
    Json(json!({"model": configured, "loaded": loaded, "running": running}))
}

async fn new_conversation() -> Json<Value> {
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    let con = Connection::open(DB_PATH).expect("open database");
    con.execute("INSERT INTO conversations (title, model, created_at, updated_at) VALUES (?, ?, ?, ?)",
        params!["", MODEL, now, now]).expect("create conversation");
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
        con.execute("UPDATE conversations SET title=?, updated_at=? WHERE id=?",
            params![title.chars().take(80).collect::<String>(), chrono::Utc::now().timestamp_millis() as f64 / 1000.0, id])
    }) {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct SendRequest {
    conversation_id: i64,
    message: Option<String>,
    content: Option<String>,
    message_id: Option<i64>,
}

fn add_message(cid: i64, role: &str, content: &str, model: Option<&str>, elapsed: Option<f64>) -> i64 {
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    let con = Connection::open(DB_PATH).expect("open database");
    con.execute(
        "INSERT INTO messages (conversation_id,role,content,created_at,model,elapsed_s) VALUES (?,?,?,?,?,?)",
        params![cid, role, content, now, model, elapsed],
    ).expect("insert message");
    con.last_insert_rowid()
}

fn settings_number(settings: &Value, key: &str, fallback: f64) -> f64 {
    settings[key].as_f64().unwrap_or(fallback)
}

fn settings_string(settings: &Value, key: &str, fallback: &str) -> String {
    settings[key].as_str().unwrap_or(fallback).to_string()
}

fn chat_history(cid: i64) -> Vec<Value> {
    list_messages(cid).into_iter()
        .filter_map(|m| Some(json!({"role": m["role"].as_str()?, "content": m["content"].as_str()?})))
        .collect()
}

async fn stream_chat(State(state): State<AppState>, Json(request): Json<SendRequest>) -> Response {
    let content = request.content.or(request.message).unwrap_or_default().trim().to_string();
    if content.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "missing message").into_response();
    }
    let cid = request.conversation_id;
    if request.message_id.is_some() {
        let con = Connection::open(DB_PATH).expect("open database");
        con.execute("DELETE FROM messages WHERE conversation_id=? AND id>?", params![cid, request.message_id]).ok();
    }
    add_message(cid, "user", &content, None, None);
    let settings = state.settings.lock().await.clone();
    let model = settings_string(&settings, "model", MODEL);
    let ollama_url = settings_string(&settings, "ollama_url", OLLAMA_URL);
    let payload = json!({
        "model": model,
        "messages": chat_history(cid),
        "stream": true,
        "options": {
            "num_predict": settings_number(&settings, "max_tokens", 1000.0),
            "num_ctx": settings_number(&settings, "num_ctx", 32768.0),
            "temperature": settings_number(&settings, "temperature", 0.2),
            "top_p": settings_number(&settings, "top_p", 0.8),
            "top_k": settings_number(&settings, "top_k", 20.0),
            "repeat_penalty": settings_number(&settings, "repeat_penalty", 1.05)
        }
    });
    let response = match reqwest::Client::new().post(format!("{ollama_url}/api/chat")).json(&payload).send().await {
        Ok(response) => response,
        Err(error) => return json_error(StatusCode::BAD_GATEWAY, &error.to_string()).into_response(),
    };
    let stream = response.bytes_stream().scan(
        (String::new(), String::new(), std::time::Instant::now(), false),
        move |state, chunk| {
        let result = match chunk {
            Ok(bytes) => {
                state.0.push_str(&String::from_utf8_lossy(&bytes));
                let mut output = String::new();
                while let Some(pos) = state.0.find('\n') {
                    let line = state.0[..pos].trim().to_string();
                    state.0.drain(..=pos);
                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                        if let Some(delta) = value["message"]["content"].as_str() {
                            state.1.push_str(delta);
                            output.push_str(&format!("data: {}\n\n", json!({
                                "choices": [{"delta": {"content": delta}, "finish_reason": null}]
                            })));
                        }
                        if value["done"].as_bool() == Some(true) && !state.3 {
                            state.3 = true;
                            add_message(cid, "assistant", &state.1, Some(&model), Some(state.2.elapsed().as_secs_f64()));
                            output.push_str("data: [DONE]\n\n");
                        }

                    }
                }
                Ok::<_, std::io::Error>(output)
            }
            Err(error) => Err(std::io::Error::other(error)),
        };
        futures_util::future::ready(Some(result))
    });
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
        Body::from_stream(stream),
    ).into_response()
}

async fn regenerate(State(state): State<AppState>, Json(request): Json<SendRequest>) -> Response {
    let con = match Connection::open(DB_PATH) {
        Ok(con) => con,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response(),
    };
    con.execute(
        "DELETE FROM messages WHERE id=(SELECT id FROM messages WHERE conversation_id=? AND role='assistant' ORDER BY id DESC LIMIT 1)",
        params![request.conversation_id],
    ).ok();
    drop(con);
    let history = list_messages(request.conversation_id);
    let Some(last_user) = history.iter().rev().find(|message| message["role"] == "user") else {
        return json_error(StatusCode::BAD_REQUEST, "conversation has no user message").into_response();
    };
    stream_chat(
        State(state),
        Json(SendRequest {
            conversation_id: request.conversation_id,
            message: None,
            content: last_user["content"].as_str().map(str::to_string),
            message_id: None,
        }),
    ).await
}

#[derive(Deserialize)]
struct ModelRequest {
    model: Option<String>,
}

async fn model_load(Json(request): Json<ModelRequest>) -> Response {
    let model = request.model.unwrap_or_else(|| MODEL.to_string());
    match reqwest::Client::new().post(format!("{OLLAMA_URL}/api/generate"))
        .json(&json!({"model": model, "prompt": "hi"})).send().await {
        Ok(response) if response.status().is_success() => Json(json!({"ok": true, "model": model})).into_response(),
        Ok(response) => json_error(StatusCode::BAD_GATEWAY, &format!("Ollama returned {}", response.status())).into_response(),
        Err(error) => json_error(StatusCode::BAD_GATEWAY, &error.to_string()).into_response(),
    }
}

async fn model_unload(Json(request): Json<ModelRequest>) -> Response {
    let model = request.model.unwrap_or_else(|| MODEL.to_string());
    match reqwest::Client::new().post(format!("{OLLAMA_URL}/api/generate"))
        .json(&json!({"model": model, "prompt": "", "keep_alive": 0})).send().await {
        Ok(response) if response.status().is_success() => Json(json!({"ok": true})).into_response(),
        Ok(response) => json_error(StatusCode::BAD_GATEWAY, &format!("Ollama returned {}", response.status())).into_response(),
        Err(error) => json_error(StatusCode::BAD_GATEWAY, &error.to_string()).into_response(),
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

async fn test_settings(Json(input): Json<Value>) -> Json<Value> {
    let url = input["ollama_url"].as_str().unwrap_or(OLLAMA_URL).trim_end_matches('/');
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

async fn stats() -> Json<Value> {
    let con = Connection::open(DB_PATH).ok();
    let totals = con.as_ref().and_then(|connection| connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(prompt_tokens+completion_tokens),0) FROM usage",
        [], |row| Ok(json!({
            "requests": row.get::<_, i64>(0)?,
            "prompt_tokens": row.get::<_, i64>(1)?,
            "completion_tokens": row.get::<_, i64>(2)?,
            "total_tokens": row.get::<_, i64>(3)?
        }))
    ).ok()).unwrap_or_else(|| json!({"requests": 0, "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}));
    Json(json!({"totals": totals, "recent": [], "session": totals}))
}

async fn openai_chat(Json(input): Json<Value>) -> Response {
    let model = input["model"].as_str().unwrap_or(MODEL);
    let messages = input["messages"].as_array().cloned().unwrap_or_default();
    let payload = json!({"model": model, "messages": messages, "stream": false, "options": {
        "temperature": input["temperature"].as_f64().unwrap_or(0.2),
        "top_p": input["top_p"].as_f64().unwrap_or(0.8),
        "top_k": input["top_k"].as_i64().unwrap_or(20),
        "repeat_penalty": input["repeat_penalty"].as_f64().unwrap_or(1.05),
        "num_predict": input["max_tokens"].as_i64().unwrap_or(1000)
    }});
    let response = match reqwest::Client::new().post(format!("{OLLAMA_URL}/api/chat")).json(&payload).send().await {
        Ok(response) => response,
        Err(error) => return json_error(StatusCode::BAD_GATEWAY, &error.to_string()).into_response(),
    };
    if !response.status().is_success() {
        return json_error(StatusCode::BAD_GATEWAY, &format!("Ollama returned {}", response.status())).into_response();
    }
    let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    let content = body["message"]["content"].as_str().unwrap_or("");
    let prompt_tokens = body["prompt_eval_count"].as_i64().unwrap_or(0);
    let completion_tokens = body["eval_count"].as_i64().unwrap_or(0);
    let total = prompt_tokens + completion_tokens;
    if let Ok(con) = Connection::open(DB_PATH) {
        con.execute("INSERT INTO usage (ts,model,prompt_tokens,completion_tokens,total_tokens,elapsed_s,endpoint) VALUES (?,?,?,?,?,?,?)",
            params![chrono::Utc::now().timestamp_millis() as f64 / 1000.0, model, prompt_tokens, completion_tokens, total, 0.0, "/v1/chat/completions"]).ok();
    }
    Json(json!({
        "id": format!("chatcmpl-{}", chrono::Utc::now().timestamp_millis()),
        "object": "chat.completion", "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": prompt_tokens, "completion_tokens": completion_tokens, "total_tokens": total}
    })).into_response()
}

async fn openai_completion(Json(input): Json<Value>) -> Response {
    let model = input["model"].as_str().unwrap_or(MODEL);
    let prompt = input["prompt"].as_str().unwrap_or("");
    let payload = json!({"model": model, "prompt": prompt, "stream": false, "options": {
        "temperature": input["temperature"].as_f64().unwrap_or(0.2),
        "top_p": input["top_p"].as_f64().unwrap_or(0.8),
        "top_k": input["top_k"].as_i64().unwrap_or(20),
        "repeat_penalty": input["repeat_penalty"].as_f64().unwrap_or(1.05),
        "num_predict": input["max_tokens"].as_i64().unwrap_or(1000)
    }});
    let response = match reqwest::Client::new().post(format!("{OLLAMA_URL}/api/generate")).json(&payload).send().await {
        Ok(response) => response,
        Err(error) => return json_error(StatusCode::BAD_GATEWAY, &error.to_string()).into_response(),
    };
    if !response.status().is_success() {
        return json_error(StatusCode::BAD_GATEWAY, &format!("Ollama returned {}", response.status())).into_response();
    }
    let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    Json(json!({
        "id": format!("cmpl-{}", chrono::Utc::now().timestamp_millis()),
        "object": "text_completion", "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{"text": body["response"].as_str().unwrap_or(""), "index": 0, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": body["prompt_eval_count"].as_i64().unwrap_or(0), "completion_tokens": body["eval_count"].as_i64().unwrap_or(0)}
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
    let state = AppState { templates: Arc::new(templates), settings: Arc::new(Mutex::new(settings)) };

    let app = Router::new()
        .route("/health", get(health))
        .route("/", get(web_chat))
        .route("/chat/new", get(web_chat))
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
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
        .layer(TraceLayer::new_for_http());
    let address = SocketAddr::from(([127, 0, 0, 1], 8090));

    tracing::info!("Rust proxy listening on {}", address);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("failed to bind Rust proxy listener");
    axum::serve(listener, app)
        .await
        .expect("Rust proxy server failed");
}
