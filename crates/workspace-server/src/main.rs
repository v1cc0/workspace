use std::{
    env,
    net::SocketAddr,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as RoutePath, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::map_response,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, sync::Mutex};
use turso::{Builder, Connection, IntoParams, Row, params, transaction::TransactionBehavior};

const GATEWAY: &str = "https://dev-oauth.pages.dev/";
const START: &str = "/v1/auth/github/start";
const CALLBACK: &str = "/v1/auth/github/callback";
const FLOW_TTL: i64 = 5 * 60;
const SESSION_TTL: i64 = 7 * 24 * 60 * 60;
const SCHEMA_VERSION: i64 = 1;

type StartupResult<T> = Result<T, Box<dyn std::error::Error>>;
type ApiResult<T> = Result<T, ApiError>;

struct ApiError(StatusCode, &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl From<turso::Error> for ApiError {
    fn from(_: turso::Error) -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable")
    }
}

const INVALID: ApiError = ApiError(StatusCode::BAD_REQUEST, "invalid_request");
const UNAUTHORIZED: ApiError = ApiError(StatusCode::UNAUTHORIZED, "unauthorized");
const GONE: ApiError = ApiError(StatusCode::GONE, "flow_expired");
const USED: ApiError = ApiError(StatusCode::CONFLICT, "flow_already_used");
const FAILED: ApiError = ApiError(StatusCode::FORBIDDEN, "authorization_failed");
const GATEWAY_ERROR: ApiError = ApiError(StatusCode::BAD_GATEWAY, "github_identity_unavailable");

struct App {
    db: Mutex<Connection>,
    origin: Url,
    gateway: Url,
    http: reqwest::Client,
}

impl App {
    fn new(db: Connection, origin: Url, gateway: Url) -> reqwest::Result<Self> {
        Ok(Self {
            db: Mutex::new(db),
            origin,
            gateway,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(16))
                .build()?,
        })
    }

    fn routes(self: Arc<Self>) -> Router {
        Router::new()
            .route("/v1/auth/flows", post(create_flow))
            .route(START, get(start))
            .route(CALLBACK, get(callback))
            .route("/v1/auth/flows/{id}/exchange", post(exchange))
            .route("/v1/me", get(me))
            .route("/v1/auth/logout", post(logout))
            .route("/health/live", get(|| async { StatusCode::NO_CONTENT }))
            .route("/health/ready", get(ready))
            .layer(DefaultBodyLimit::max(4096))
            .layer(map_response(|mut response: Response| async move {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response.headers_mut().insert(
                    header::REFERRER_POLICY,
                    HeaderValue::from_static("no-referrer"),
                );
                response
            }))
            .with_state(self)
    }
}

#[tokio::main]
async fn main() -> StartupResult<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [command] = args.as_slice() else {
        return Err("usage: workspace-server migrate|serve".into());
    };
    let path = env::var("WORKSPACE_DB_PATH")?;
    if path.is_empty() || path == ":memory:" {
        return Err("WORKSPACE_DB_PATH must name a database file".into());
    }
    match command.as_str() {
        "migrate" => migrate(&mut open(&path).await?).await?,
        "serve" => {
            if !Path::new(&path).is_file() {
                return Err("run workspace-server migrate before serve".into());
            }
            let db = open(&path).await?;
            if schema_version(&db).await? != SCHEMA_VERSION {
                return Err("unsupported database schema; run workspace-server migrate".into());
            }
            let listen: SocketAddr = setting("WORKSPACE_LISTEN", "127.0.0.1:8789")?.parse()?;
            if !listen.ip().is_loopback() {
                return Err("WORKSPACE_LISTEN must use a loopback address".into());
            }
            let origin = public_origin(&setting(
                "WORKSPACE_PUBLIC_ORIGIN",
                &format!("http://{listen}"),
            )?)?;
            let app = Arc::new(App::new(db, origin, Url::parse(GATEWAY)?)?);
            let listener = TcpListener::bind(listen).await?;
            eprintln!(
                "Workspace authentication API listening on {}",
                listener.local_addr()?
            );
            axum::serve(listener, app.routes()).await?;
        }
        _ => return Err("usage: workspace-server migrate|serve".into()),
    }
    Ok(())
}

fn setting(name: &str, default: &str) -> Result<String, env::VarError> {
    match env::var(name) {
        Err(env::VarError::NotPresent) => Ok(default.into()),
        result => result,
    }
}

fn public_origin(value: &str) -> StartupResult<Url> {
    let url = Url::parse(value)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
    {
        return Err(
            "WORKSPACE_PUBLIC_ORIGIN must be an HTTPS origin or a loopback HTTP origin".into(),
        );
    }
    Ok(url)
}

async fn open(path: &str) -> turso::Result<Connection> {
    let db = Builder::new_local(path).build().await?.connect()?;
    db.execute("PRAGMA foreign_keys = ON", ()).await?;
    Ok(db)
}

async fn first(db: &Connection, sql: &str, params: impl IntoParams) -> turso::Result<Option<Row>> {
    db.query(sql, params).await?.next().await
}

async fn schema_version(db: &Connection) -> turso::Result<i64> {
    first(db, "PRAGMA user_version", ())
        .await?
        .ok_or(turso::Error::QueryReturnedNoRows)?
        .get(0)
}

async fn migrate(db: &mut Connection) -> StartupResult<()> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await?;
    match schema_version(&tx).await? {
        0 => tx.execute_batch(include_str!("schema.sql")).await?,
        SCHEMA_VERSION => {}
        _ => return Err("unsupported database schema version".into()),
    }
    tx.commit().await?;
    Ok(())
}

async fn ready(State(app): State<Arc<App>>) -> ApiResult<StatusCode> {
    let db = app.db.lock().await;
    if schema_version(&db).await? != SCHEMA_VERSION {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "schema_unavailable",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn random_token() -> ApiResult<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "random_unavailable"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn digest(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

fn valid_token(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

#[derive(Deserialize)]
struct NewFlow {
    code_challenge: String,
}

async fn create_flow(
    State(app): State<Arc<App>>,
    Json(request): Json<NewFlow>,
) -> ApiResult<impl IntoResponse> {
    if !valid_token(&request.code_challenge) {
        return Err(INVALID);
    }
    let id = random_token()?;
    let mut url = app.origin.clone();
    url.set_path(START);
    url.query_pairs_mut().append_pair("flow_id", &id);

    let db = app.db.lock().await;
    let now = now();
    db.execute("DELETE FROM login_flows WHERE expires_at <= ?1", [now])
        .await?;
    db.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now])
        .await?;
    let created = db
        .execute(
            "INSERT INTO login_flows (id, challenge, expires_at) SELECT ?1, ?2, ?3
         WHERE (SELECT count(*) FROM login_flows) < 1024",
            params![id.clone(), request.code_challenge, now + FLOW_TTL],
        )
        .await?;
    if created == 0 {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_login_flows",
        ));
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "flow_id": id, "login_url": url.as_str(), "expires_in": FLOW_TTL, "interval": 2
        })),
    ))
}

#[derive(Deserialize)]
struct StartQuery {
    flow_id: String,
}

async fn start(
    State(app): State<Arc<App>>,
    Query(query): Query<StartQuery>,
) -> ApiResult<Response> {
    if !valid_token(&query.flow_id) {
        return Err(INVALID);
    }
    let state = random_token()?;
    let browser = random_token()?;
    let verifier = random_token()?;
    let redirect_uri = app.origin.join(CALLBACK).expect("absolute callback path");
    let mut location = app
        .gateway
        .join("/api/login")
        .expect("absolute gateway path");
    location.query_pairs_mut().extend_pairs([
        ("provider", "github"),
        ("scope", "read:user"),
        ("response_mode", "query"),
        ("redirect_uri", redirect_uri.as_str()),
        ("state", &state),
        ("code_challenge", &digest(&verifier)),
    ]);
    let db = app.db.lock().await;
    let claimed = db
        .execute(
            "UPDATE login_flows SET state_hash = ?1, browser_hash = ?2, gateway_verifier = ?3
         WHERE id = ?4 AND expires_at > ?5 AND state_hash IS NULL",
            params![
                digest(&state),
                digest(&browser),
                verifier,
                query.flow_id,
                now()
            ],
        )
        .await?;
    if claimed == 0 {
        return Err(GONE);
    }
    let mut response = Redirect::to(location.as_str()).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        browser_cookie(&app, &state, &browser, FLOW_TTL),
    );
    Ok(response)
}

fn browser_cookie(app: &App, state: &str, value: &str, max_age: i64) -> HeaderValue {
    let secure = if app.origin.scheme() == "https" {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "ws_oauth_{state}={value}; Path={CALLBACK}; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}"
    )).expect("validated token and static cookie attributes")
}

#[derive(Deserialize)]
struct CallbackQuery {
    state: String,
    code: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct GatewayResponse {
    provider: Option<String>,
    user: Option<GitHubUser>,
}

#[derive(Deserialize, Serialize)]
struct GitHubUser {
    id: i64,
    login: String,
}

async fn github_user(app: &App, code: &str, verifier: &str) -> ApiResult<GitHubUser> {
    let mut response = app
        .http
        .post(
            app.gateway
                .join("/api/exchange")
                .expect("absolute gateway path"),
        )
        .json(&json!({"code": code, "code_verifier": verifier,
            "redirect_uri": app.origin.join(CALLBACK).expect("absolute callback path").as_str()}))
        .send()
        .await
        .map_err(|_| GATEWAY_ERROR)?;
    if !response.status().is_success() {
        return Err(GATEWAY_ERROR);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| GATEWAY_ERROR)? {
        if body.len() + chunk.len() > 16_384 {
            return Err(GATEWAY_ERROR);
        }
        body.extend_from_slice(&chunk);
    }
    let result: GatewayResponse = serde_json::from_slice(&body).map_err(|_| GATEWAY_ERROR)?;
    if result.provider.is_some_and(|provider| provider != "github") {
        return Err(GATEWAY_ERROR);
    }
    result
        .user
        .filter(|user| user.id > 0 && !user.login.is_empty() && user.login.len() <= 256)
        .ok_or(GATEWAY_ERROR)
}

async fn callback(
    State(app): State<Arc<App>>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if !valid_token(&query.state)
        || query
            .code
            .as_ref()
            .is_some_and(|code| code.is_empty() || code.len() > 8192)
        || query.code.is_some() == query.error.is_some()
    {
        return Err(INVALID);
    }
    let name = format!("ws_oauth_{}=", query.state);
    let browser = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|cookie| cookie.trim().strip_prefix(&name))
        .filter(|value| valid_token(value))
        .ok_or(UNAUTHORIZED)?;
    let (id, verifier) = {
        let db = app.db.lock().await;
        let flow = first(&db,
            "SELECT id, gateway_verifier FROM login_flows WHERE state_hash = ?1 AND browser_hash = ?2
             AND expires_at > ?3 AND gateway_verifier IS NOT NULL",
            params![digest(&query.state), digest(browser), now()],
        ).await?.ok_or(UNAUTHORIZED)?;
        let id: String = flow.get(0)?;
        let verifier: String = flow.get(1)?;
        let count = db
            .execute(
                "UPDATE login_flows SET gateway_verifier = NULL
            WHERE id = ?1 AND gateway_verifier IS NOT NULL",
                [id.clone()],
            )
            .await?;
        if count == 0 {
            return Err(USED);
        }
        (id, verifier)
    };

    let result = match query.code {
        Some(code) => github_user(&app, &code, &verifier).await,
        None => Err(FAILED),
    };
    let mut response = match result {
        Ok(user) => {
            let mut db = app.db.lock().await;
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?;
            tx.execute(
                "INSERT INTO users (github_user_id, login) VALUES (?1, ?2)
                ON CONFLICT (github_user_id) DO UPDATE SET login = excluded.login",
                params![user.id, user.login],
            )
            .await?;
            let updated = tx
                .execute(
                    "UPDATE login_flows SET github_user_id = ?1 WHERE id = ?2 AND expires_at > ?3",
                    params![user.id, id, now()],
                )
                .await?;
            if updated == 0 {
                tx.rollback().await?;
                GONE.into_response()
            } else {
                tx.commit().await?;
                "GitHub sign-in complete. Return to Workspace.".into_response()
            }
        }
        Err(error) => {
            app.db
                .lock()
                .await
                .execute("UPDATE login_flows SET failed = 1 WHERE id = ?1", [id])
                .await?;
            error.into_response()
        }
    };
    response.headers_mut().insert(
        header::SET_COOKIE,
        browser_cookie(&app, &query.state, "", 0),
    );
    Ok(response)
}

#[derive(Deserialize)]
struct ExchangeRequest {
    verifier: String,
}

async fn exchange(
    State(app): State<Arc<App>>,
    RoutePath(id): RoutePath<String>,
    Json(request): Json<ExchangeRequest>,
) -> ApiResult<Response> {
    let verifier = request.verifier;
    if !valid_token(&id)
        || !(43..=128).contains(&verifier.len())
        || !verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
    {
        return Err(INVALID);
    }
    let mut db = app.db.lock().await;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await?;
    let now = now();
    let Some(flow) = first(
        &tx,
        "SELECT challenge, expires_at, github_user_id, consumed_at, failed
        FROM login_flows WHERE id = ?1",
        [id.clone()],
    )
    .await?
    else {
        tx.rollback().await?;
        return Err(GONE);
    };
    let result = if flow.get::<String>(0)? != digest(&verifier) {
        Err(UNAUTHORIZED)
    } else if flow.get::<i64>(1)? <= now {
        Err(GONE)
    } else if flow.get::<Option<i64>>(3)?.is_some() {
        Err(USED)
    } else if flow.get::<i64>(4)? != 0 {
        Err(FAILED)
    } else if let Some(user) = flow.get::<Option<i64>>(2)? {
        let token = random_token()?;
        let changed = tx
            .execute(
                "UPDATE login_flows SET consumed_at = ?1
            WHERE id = ?2 AND consumed_at IS NULL AND expires_at > ?1",
                params![now, id],
            )
            .await?;
        if changed != 1 {
            tx.rollback().await?;
            return Err(USED);
        }
        tx.execute(
            "INSERT INTO sessions VALUES (?1, ?2, ?3)",
            params![digest(&token), user, now + SESSION_TTL],
        )
        .await?;
        tx.commit().await?;
        return Ok(Json(json!({"token": token, "expires_in": SESSION_TTL})).into_response());
    } else {
        Ok((StatusCode::ACCEPTED, Json(json!({"status": "pending"}))).into_response())
    };
    tx.rollback().await?;
    result
}

fn bearer_hash(headers: &HeaderMap) -> ApiResult<String> {
    let (scheme, token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .ok_or(UNAUTHORIZED)?;
    if !scheme.eq_ignore_ascii_case("Bearer") || !valid_token(token) {
        return Err(UNAUTHORIZED);
    }
    Ok(digest(token))
}

async fn me(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Json<GitHubUser>> {
    let hash = bearer_hash(&headers)?;
    let db = app.db.lock().await;
    let row = first(
        &db,
        "SELECT users.github_user_id, users.login FROM sessions JOIN users USING (github_user_id)
        WHERE token_hash = ?1 AND expires_at > ?2",
        params![hash, now()],
    )
    .await?
    .ok_or(UNAUTHORIZED)?;
    Ok(Json(GitHubUser {
        id: row.get(0)?,
        login: row.get(1)?,
    }))
}

async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<StatusCode> {
    let hash = bearer_hash(&headers)?;
    app.db
        .lock()
        .await
        .execute("DELETE FROM sessions WHERE token_hash = ?1", [hash])
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
