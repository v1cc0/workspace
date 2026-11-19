use super::*;
use serde_json::Value;
use tokio::task::JoinHandle;

const VERIFIER: &str = "desktop_verifier_abcdefghijklmnopqrstuvwxyz";

async fn serve(listener: TcpListener, router: Router) -> JoinHandle<()> {
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() })
}

struct TestServer {
    app: Arc<App>,
    http: reqwest::Client,
    task: JoinHandle<()>,
}

impl TestServer {
    async fn start(db: Connection, gateway: Url) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let app = Arc::new(App::new(db, origin, gateway).unwrap());
        let task = serve(listener, app.clone().routes()).await;
        Self {
            http: app.http.clone(),
            app,
            task,
        }
    }

    fn url(&self, path: &str) -> Url {
        self.app.origin.join(path).unwrap()
    }

    async fn flow(&self) -> (String, Url) {
        let response = self
            .http
            .post(self.url("/v1/auth/flows"))
            .json(&json!({"code_challenge": digest(VERIFIER)}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body: Value = response.json().await.unwrap();
        (
            body["flow_id"].as_str().unwrap().into(),
            Url::parse(body["login_url"].as_str().unwrap()).unwrap(),
        )
    }

    async fn start_flow(&self, start: Url, code: &str) -> (Url, String) {
        let response = self.http.get(start).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
        let gateway_login =
            Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        assert_eq!(gateway_login.path(), "/api/login");
        let query: std::collections::HashMap<_, _> = gateway_login.query_pairs().collect();
        assert_eq!(query["provider"], "github");
        assert_eq!(query["scope"], "read:user");
        assert_ne!(query["code_challenge"], digest(VERIFIER));
        let mut callback = Url::parse(&query["redirect_uri"]).unwrap();
        assert_eq!(callback, self.url(CALLBACK));
        let ticket = format!("{code}:{}", query["code_challenge"]);
        callback
            .query_pairs_mut()
            .extend_pairs([("state", query["state"].as_ref()), ("code", &ticket)]);
        (callback, cookie.split(';').next().unwrap().into())
    }

    async fn exchange(&self, id: &str, verifier: &str) -> reqwest::Response {
        self.http
            .post(self.url(&format!("/v1/auth/flows/{id}/exchange")))
            .json(&json!({"verifier": verifier}))
            .send()
            .await
            .unwrap()
    }

    async fn me(&self, token: &str) -> reqwest::Response {
        self.http
            .get(self.url("/v1/me"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock_gateway() -> (Url, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let task = serve(listener, Router::new().route("/api/exchange", post(|Json(request): Json<Value>| async move {
        let verifier = request["code_verifier"].as_str().unwrap();
        assert!(valid_token(verifier));
        assert_ne!(verifier, VERIFIER);
        assert_eq!(Url::parse(request["redirect_uri"].as_str().unwrap()).unwrap().path(), CALLBACK);
        let (code, challenge) = request["code"].as_str().unwrap().split_once(':').unwrap();
        assert_eq!(digest(verifier), challenge);
        Json(match code {
            "missing-user" => json!({"access_token": "gateway-only", "user": null}),
            "google-user" => json!({"provider": "google", "user": {"id": "42", "login": "google"}}),
            "valid-ticket" => json!({"access_token": "gateway-only", "user": {"id": 42, "login": "octocat"}}),
            code => panic!("unexpected mock ticket {code}"),
        })
    }))).await;
    (origin, task)
}

#[tokio::test]
async fn github_session_survives_reopen_and_is_revocable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.db");
    let mut db = open(path.to_str().unwrap()).await.unwrap();
    assert_eq!(schema_version(&db).await.unwrap(), 0);
    migrate(&mut db).await.unwrap();
    migrate(&mut db).await.unwrap();
    let (gateway, gateway_task) = mock_gateway().await;
    let server = TestServer::start(db, gateway.clone()).await;
    assert_eq!(
        server
            .http
            .get(server.url("/health/ready"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );

    let (id, start) = server.flow().await;
    assert_eq!(
        server.exchange(&id, VERIFIER).await.status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        server.exchange(&id, &"a".repeat(43)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let (callback, cookie) = server.start_flow(start, "valid-ticket").await;
    assert_eq!(
        server
            .http
            .get(callback.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = server
        .http
        .get(callback.clone())
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        server
            .http
            .get(callback)
            .header(header::COOKIE, cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let (first, second) = tokio::join!(
        server.exchange(&id, VERIFIER),
        server.exchange(&id, VERIFIER)
    );
    let (issued, denied) = if first.status() == StatusCode::OK {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(denied.status(), StatusCode::CONFLICT);
    let issued: Value = issued.error_for_status().unwrap().json().await.unwrap();
    let token = issued["token"].as_str().unwrap();
    assert!(valid_token(token));
    assert_eq!(issued["expires_in"], SESSION_TTL);
    {
        let db = server.app.db.lock().await;
        let row = super::first(&db, "SELECT token_hash FROM sessions", ())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), digest(token));
    }
    server.task.abort();
    drop(server);
    let db = open(path.to_str().unwrap()).await.unwrap();
    let server = TestServer::start(db, gateway).await;
    assert_eq!(
        server.me(token).await.json::<Value>().await.unwrap(),
        json!({"id": 42, "login": "octocat"})
    );
    for _ in 0..2 {
        let response = server
            .http
            .post(server.url("/v1/auth/logout"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    assert_eq!(server.me(token).await.status(), StatusCode::UNAUTHORIZED);
    gateway_task.abort();
}

#[tokio::test]
async fn rejects_missing_identity_other_provider_expiration_and_denial() {
    let mut db = open(":memory:").await.unwrap();
    migrate(&mut db).await.unwrap();
    let (gateway, gateway_task) = mock_gateway().await;
    let server = TestServer::start(db, gateway).await;
    for code in ["missing-user", "google-user"] {
        let (id, start) = server.flow().await;
        let (callback, cookie) = server.start_flow(start, code).await;
        let response = server
            .http
            .get(callback)
            .header(header::COOKIE, cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            server.exchange(&id, VERIFIER).await.status(),
            StatusCode::FORBIDDEN
        );
    }

    let (id, start) = server.flow().await;
    let (mut callback, cookie) = server.start_flow(start, "valid-ticket").await;
    let state = callback
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    callback
        .query_pairs_mut()
        .clear()
        .append_pair("state", &state)
        .append_pair("error", "access_denied");
    assert_eq!(
        server
            .http
            .get(callback)
            .header(header::COOKIE, cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        server.exchange(&id, VERIFIER).await.status(),
        StatusCode::FORBIDDEN
    );

    let (id, _) = server.flow().await;
    server
        .app
        .db
        .lock()
        .await
        .execute(
            "UPDATE login_flows SET expires_at = 0 WHERE id = ?1",
            [id.clone()],
        )
        .await
        .unwrap();
    assert_eq!(
        server.exchange(&id, VERIFIER).await.status(),
        StatusCode::GONE
    );
    gateway_task.abort();
}

#[test]
fn public_origin_requires_https_except_on_loopback() {
    assert!(public_origin("http://127.0.0.1:8080").is_ok());
    assert!(public_origin("https://workspace.example.org").is_ok());
    for invalid in [
        "http://workspace.example.org",
        "https://workspace.example.org/path",
        "https://user@workspace.example.org",
    ] {
        assert!(public_origin(invalid).is_err());
    }
}
