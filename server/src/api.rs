use crate::config::Config;
use crate::{db, push};
use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};
use subtle::ConstantTimeEq;

const MAX_NOTIFICATION_BODY: usize = 8 * 1024 * 1024;
const MAX_DEVICE_BODY: usize = 4096;

pub struct App {
    pub cfg: Config,
    pub service_key: String,
    pub app_key: String,
    pub db: Mutex<Connection>,
}

pub enum Fail {
    Respond(Response),
    Internal(String),
}

impl From<rusqlite::Error> for Fail {
    fn from(e: rusqlite::Error) -> Self {
        Fail::Internal(format!("rusqlite::Error: {e}"))
    }
}

type Result<T> = std::result::Result<T, Fail>;

fn json_response(status: StatusCode, value: Value) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], value.to_string()).into_response()
}

fn error(status: StatusCode, message: &str) -> Fail {
    Fail::Respond(json_response(status, json!({ "error": message })))
}

fn method_not_allowed() -> Fail {
    error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed")
}

fn verify(headers: &HeaderMap, name: &str, expected: &str) -> Result<()> {
    let given = headers.get(name).map(|v| v.as_bytes()).unwrap_or(b"");
    if bool::from(expected.as_bytes().ct_eq(given)) {
        Ok(())
    } else {
        Err(error(StatusCode::UNAUTHORIZED, "Unauthorized"))
    }
}

/// PHP's empty(): null, false, 0, "", "0", [] and {} (decoded as []) are all empty.
fn php_empty(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => !b,
        Some(Value::Number(n)) => n.as_f64() == Some(0.0),
        Some(Value::String(s)) => s.is_empty() || s == "0",
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::Object(o)) => o.is_empty(),
    }
}

/// PHP's (int) cast of a query-string value: leading whitespace, optional sign, digits.
fn php_intval(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let n = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |acc, d| acc.saturating_mul(10).saturating_add((d - b'0') as i64));
    if neg { -n } else { n }
}

/// API consumers expect an object, including for legacy empty-array records.
fn decode_metadata(v: Option<&Value>) -> Value {
    match v.and_then(Value::as_str).and_then(|s| serde_json::from_str::<Value>(s).ok()) {
        Some(o @ Value::Object(_)) => o,
        _ => Value::Object(Map::new()),
    }
}

impl App {
    async fn with_db<T, F>(self: &Arc<Self>, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let app = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = app.db.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut conn)
        })
        .await
        .map_err(|e| Fail::Internal(format!("db task: {e}")))?
    }
}

pub async fn dispatch(State(app): State<Arc<App>>, method: Method, uri: Uri, headers: HeaderMap, body: Body) -> Response {
    let query: Vec<(String, String)> = uri
        .query()
        .map(|q| form_urlencoded::parse(q.as_bytes()).into_owned().collect())
        .unwrap_or_default();
    // Like $_GET: the last occurrence of a key wins.
    let param = |name: &str| query.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.clone());

    let path = uri.path();
    let result = match path {
        "/api/services" | "/api/services/" | "/api/services.php" => services(&app, &method, &headers).await,
        "/api/devices" | "/api/devices/" | "/api/devices.php" => devices(&app, &method, &headers, body).await,
        "/api/notifications" | "/api/notifications/" | "/api/notifications.php" => {
            notifications(&app, &method, &headers, body, param("id"), &param).await
        }
        p if p.starts_with("/api/notifications/") => {
            let raw = &p["/api/notifications/".len()..];
            let id = percent_encoding::percent_decode_str(raw).decode_utf8_lossy().into_owned();
            // The old rewrite appended the original query (QSA), so an explicit ?id= still won.
            notifications(&app, &method, &headers, body, param("id").or(Some(id)), &param).await
        }
        _ => Err(error(StatusCode::NOT_FOUND, "Not found")),
    };
    match result {
        Ok(r) => r,
        Err(Fail::Respond(r)) => r,
        Err(Fail::Internal(e)) => {
            // Never leak internals to clients.
            eprintln!("[gootif] {e}");
            json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": "Internal server error"}))
        }
    }
}

async fn services(app: &Arc<App>, method: &Method, headers: &HeaderMap) -> Result<Response> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    verify(headers, "x-api-key", &app.app_key)?;
    let rows = app
        .with_db(|c| {
            Ok(db::query_json(
                c,
                "SELECT
                    service AS name,
                    COUNT(*) AS notification_count,
                    MAX(created_at) AS last_notification_at
                FROM notifications
                GROUP BY service
                ORDER BY last_notification_at DESC",
                [],
            )?)
        })
        .await?;
    Ok(json_response(StatusCode::OK, Value::Array(rows.into_iter().map(Value::Object).collect())))
}

async fn notifications(
    app: &Arc<App>,
    method: &Method,
    headers: &HeaderMap,
    body: Body,
    id: Option<String>,
    param: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<Response> {
    let id = id.filter(|i| !i.is_empty());
    match (method.clone(), id) {
        (Method::POST, None) => create_notification(app, headers, body).await,
        (Method::GET, Some(id)) => {
            verify(headers, "x-api-key", &app.app_key)?;
            let mut rows = app
                .with_db(move |c| Ok(db::query_json(c, "SELECT * FROM notifications WHERE id = ?", [id])?))
                .await?;
            let Some(mut row) = rows.pop() else {
                return Err(error(StatusCode::NOT_FOUND, "Not found"));
            };
            let metadata = decode_metadata(row.get("metadata"));
            row.insert("metadata".into(), metadata);
            Ok(json_response(StatusCode::OK, Value::Object(row)))
        }
        (Method::GET, None) => {
            verify(headers, "x-api-key", &app.app_key)?;
            let truthy = |v: Option<String>| v.filter(|s| !s.is_empty() && s != "0");
            let service = truthy(param("service"));
            let since = truthy(param("since"));
            let limit = param("limit").map_or(50, |l| php_intval(&l)).min(200);
            let offset = param("offset").map_or(0, |o| php_intval(&o)).max(0);

            let mut clauses = Vec::new();
            let mut values: Vec<SqlValue> = Vec::new();
            if let Some(s) = service {
                clauses.push("service = ?");
                values.push(SqlValue::Text(s));
            }
            if let Some(s) = since {
                clauses.push("created_at > ?");
                values.push(SqlValue::Text(s));
            }
            let where_clause = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };
            values.push(SqlValue::Integer(limit));
            values.push(SqlValue::Integer(offset));
            let sql = format!("SELECT * FROM notifications {where_clause} ORDER BY created_at DESC LIMIT ? OFFSET ?");
            let rows = app.with_db(move |c| Ok(db::query_json(c, &sql, params_from_iter(values))?)).await?;
            let rows = rows
                .into_iter()
                .map(|mut row| {
                    let metadata = decode_metadata(row.get("metadata"));
                    row.insert("metadata".into(), metadata);
                    Value::Object(row)
                })
                .collect();
            Ok(json_response(StatusCode::OK, Value::Array(rows)))
        }
        (Method::DELETE, Some(id)) => {
            verify(headers, "x-api-key", &app.app_key)?;
            let deleted = app.with_db(move |c| Ok(c.execute("DELETE FROM notifications WHERE id = ?", [id])?)).await?;
            if deleted == 0 {
                return Err(error(StatusCode::NOT_FOUND, "Not found"));
            }
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        _ => Err(method_not_allowed()),
    }
}

async fn create_notification(app: &Arc<App>, headers: &HeaderMap, body: Body) -> Result<Response> {
    verify(headers, "x-service-key", &app.service_key)?;
    let bytes = to_bytes(body, MAX_NOTIFICATION_BODY)
        .await
        .map_err(|_| error(StatusCode::PAYLOAD_TOO_LARGE, "Request too large"))?;
    let data = match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(o)) => o,
        Ok(Value::Array(_)) => Map::new(),
        _ => return Err(error(StatusCode::BAD_REQUEST, "Invalid JSON body")),
    };

    if php_empty(data.get("service")) || php_empty(data.get("title")) {
        return Err(error(StatusCode::BAD_REQUEST, "\"service\" and \"title\" are required"));
    }
    // Generous caps — agent reports live in metadata and can be large, but
    // nothing legitimate in this ecosystem exceeds these.
    let service = match data.get("service") {
        Some(Value::String(s)) if s.len() <= 100 => s.clone(),
        _ => return Err(error(StatusCode::BAD_REQUEST, "\"service\" must be a string of at most 100 bytes")),
    };
    let title = match data.get("title") {
        Some(Value::String(s)) if s.len() <= 500 => s.clone(),
        _ => return Err(error(StatusCode::BAD_REQUEST, "\"title\" must be a string of at most 500 bytes")),
    };
    let body_text = match data.get("body") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) if s.len() <= 65536 => s.clone(),
        _ => return Err(error(StatusCode::BAD_REQUEST, "\"body\" must be a string of at most 64 KB")),
    };
    let metadata = match data.get("metadata") {
        None | Some(Value::Null) => "{}".to_string(),
        Some(m @ (Value::Object(_) | Value::Array(_))) => m.to_string(),
        _ => return Err(error(StatusCode::BAD_REQUEST, "\"metadata\" must be a JSON object")),
    };
    if metadata.len() > 1_048_576 {
        return Err(error(StatusCode::BAD_REQUEST, "\"metadata\" must encode to at most 1 MB"));
    }
    let priority = match data.get("priority").and_then(Value::as_str) {
        Some(p @ ("low" | "medium" | "high")) => p.to_string(),
        _ => "medium".to_string(),
    };

    let id: String = rand::random::<[u8; 16]>().iter().map(|b| format!("{b:02x}")).collect();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let push_enabled = push::enabled(&app.cfg);
    {
        let (id, service, title, body_text, priority, metadata, now) =
            (id.clone(), service.clone(), title.clone(), body_text.clone(), priority.clone(), metadata.clone(), now.clone());
        app.with_db(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO notifications (id, service, title, body, priority, metadata, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![id, service, title, body_text, priority, metadata, now],
            )?;
            push::enqueue(&tx, push_enabled, &id)?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    }

    Ok(json_response(
        StatusCode::CREATED,
        json!({
            "id": id,
            "service": service,
            "title": title,
            "body": body_text,
            "priority": priority,
            "metadata": decode_metadata(Some(&Value::String(metadata))),
            "created_at": now,
        }),
    ))
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

async fn devices(app: &Arc<App>, method: &Method, headers: &HeaderMap, body: Body) -> Result<Response> {
    verify(headers, "x-api-key", &app.app_key)?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    // Bound input before parsing; tokens and installation IDs are small.
    let bytes = to_bytes(body, MAX_DEVICE_BODY)
        .await
        .map_err(|_| error(StatusCode::PAYLOAD_TOO_LARGE, "Request too large"))?;
    let data = match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(o)) => o,
        _ => Map::new(),
    };
    let installation_id = match data.get("installation_id") {
        Some(Value::String(s)) if is_uuid(s) => s.to_lowercase(),
        _ => return Err(error(StatusCode::BAD_REQUEST, "Valid installation_id required")),
    };

    if data.get("enabled") == Some(&Value::Bool(false)) {
        app.with_db(move |c| {
            c.execute("UPDATE push_devices SET active = 0 WHERE installation_id = ?", [installation_id])?;
            Ok(())
        })
        .await?;
        return Ok(json_response(StatusCode::OK, json!({"enabled": false})));
    }

    let expected_topic = match data.get("platform").and_then(Value::as_str) {
        Some("ios") => Some(app.cfg.option("APNS_IOS_TOPIC", "com.goosheys.gootif")),
        Some("macos") => Some(app.cfg.option("APNS_MACOS_TOPIC", "com.goosheys.gootif-mac")),
        _ => None,
    };
    let topic = data.get("topic").and_then(Value::as_str);
    let environment = data.get("environment").and_then(Value::as_str);
    let token = data.get("token").and_then(Value::as_str);
    let valid = expected_topic.is_some()
        && topic.is_some()
        && topic == expected_topic.as_deref()
        && matches!(environment, Some("development" | "production"))
        && token.is_some_and(|t| (32..=512).contains(&t.len()) && t.len() % 2 == 0 && t.bytes().all(|b| b.is_ascii_hexdigit()));
    if !valid {
        return Err(error(StatusCode::BAD_REQUEST, "Invalid platform, topic, environment or token"));
    }
    let environment = environment.unwrap_or_default().to_string();
    if !push::ready(&app.cfg, &environment) {
        return Err(error(StatusCode::SERVICE_UNAVAILABLE, "Push notifications are not configured for this environment"));
    }
    let registration = push::Registration {
        installation_id,
        token: token.unwrap_or_default().to_lowercase(),
        topic: topic.unwrap_or_default().to_string(),
        environment,
    };
    app.with_db(move |c| Ok(push::register(c, &registration)?)).await?;
    Ok(json_response(StatusCode::OK, json!({"enabled": true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    const SVC: &str = "test-service-key";
    const APP: &str = "test-app-key";
    const S: (&str, &str) = ("x-service-key", SVC);
    const A: (&str, &str) = ("x-api-key", APP);

    fn app(push: bool) -> axum::Router {
        let cfg = Config::from_pairs(&[("APNS_ENABLED", if push { "1" } else { "0" })]);
        let conn = db::open(":memory:").unwrap();
        let state = Arc::new(App { cfg, service_key: SVC.into(), app_key: APP.into(), db: Mutex::new(conn) });
        axum::Router::new().fallback(dispatch).with_state(state)
    }

    async fn call(router: &axum::Router, method: &str, uri: &str, headers: &[(&str, &str)], body: &str) -> (u16, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = router.clone().oneshot(req.body(Body::from(body.to_string())).unwrap()).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn notification_lifecycle() {
        let r = app(false);
        let (status, created) = call(
            &r,
            "POST",
            "/api/notifications",
            &[S],
            r#"{"service":"nas","title":"Backup done","priority":"high","metadata":{"files":3}}"#,
        )
        .await;
        assert_eq!(status, 201);
        assert_eq!(created["priority"], "high");
        assert_eq!(created["body"], "");
        assert_eq!(created["metadata"], json!({"files": 3}));
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(id.len(), 32);

        let (status, list) = call(&r, "GET", "/api/notifications?service=nas", &[A], "").await;
        assert_eq!(status, 200);
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["metadata"], json!({"files": 3}));

        let (status, services) = call(&r, "GET", "/api/services", &[A], "").await;
        assert_eq!(status, 200);
        assert_eq!(services[0]["name"], "nas");
        assert_eq!(services[0]["notification_count"], 1);

        assert_eq!(call(&r, "GET", &format!("/api/notifications/{id}"), &[A], "").await.0, 200);
        assert_eq!(call(&r, "GET", &format!("/api/notifications?id={id}"), &[A], "").await.0, 200);
        assert_eq!(call(&r, "DELETE", &format!("/api/notifications/{id}"), &[A], "").await.0, 204);
        assert_eq!(call(&r, "DELETE", &format!("/api/notifications/{id}"), &[A], "").await.0, 404);
        assert_eq!(call(&r, "GET", &format!("/api/notifications/{id}"), &[A], "").await.0, 404);
    }

    #[tokio::test]
    async fn auth_and_methods() {
        let r = app(false);
        let body = r#"{"service":"s","title":"t"}"#;
        assert_eq!(call(&r, "POST", "/api/notifications", &[], body).await.0, 401);
        assert_eq!(call(&r, "POST", "/api/notifications", &[A], body).await.0, 401);
        assert_eq!(call(&r, "POST", "/api/notifications", &[("x-service-key", "nope")], body).await.0, 401);
        assert_eq!(call(&r, "GET", "/api/notifications", &[S], "").await.0, 401);
        assert_eq!(call(&r, "GET", "/api/services", &[], "").await.0, 401);
        assert_eq!(call(&r, "POST", "/api/services", &[A], "").await.0, 405);
        assert_eq!(call(&r, "PUT", "/api/notifications", &[A], "").await.0, 405);
        assert_eq!(call(&r, "DELETE", "/api/notifications", &[A], "").await.0, 405);
        assert_eq!(call(&r, "GET", "/api/devices", &[A], "").await.0, 405);
        assert_eq!(call(&r, "GET", "/api/nothing", &[A], "").await.0, 404);
    }

    #[tokio::test]
    async fn create_validation() {
        let r = app(false);
        let cases = [
            ("{nope", 400),
            ("5", 400),
            ("[1,2]", 400),
            (r#"{"service":"s"}"#, 400),
            (r#"{"service":"0","title":"t"}"#, 400),
            (r#"{"service":123,"title":"t"}"#, 400),
            (r#"{"service":"s","title":"t","body":5}"#, 400),
            (r#"{"service":"s","title":"t","metadata":"m"}"#, 400),
            (r#"{"service":"s","title":"t","priority":"urgent","body":null}"#, 201),
        ];
        for (body, want) in cases {
            assert_eq!(call(&r, "POST", "/api/notifications", &[S], body).await.0, want, "{body}");
        }
        let long_title = format!(r#"{{"service":"s","title":"{}"}}"#, "t".repeat(501));
        assert_eq!(call(&r, "POST", "/api/notifications", &[S], &long_title).await.0, 400);
        let (_, created) = call(&r, "POST", "/api/notifications", &[S], r#"{"service":"s","title":"t","priority":"urgent"}"#).await;
        assert_eq!(created["priority"], "medium");
    }

    #[tokio::test]
    async fn list_paging_like_php() {
        let r = app(false);
        for i in 0..5 {
            call(&r, "POST", "/api/notifications", &[S], &format!(r#"{{"service":"s","title":"n{i}"}}"#)).await;
        }
        let count = |v: Value| v.as_array().unwrap().len();
        assert_eq!(count(call(&r, "GET", "/api/notifications?limit=2", &[A], "").await.1), 2);
        assert_eq!(count(call(&r, "GET", "/api/notifications?limit=2&offset=4", &[A], "").await.1), 1);
        assert_eq!(count(call(&r, "GET", "/api/notifications?limit=abc", &[A], "").await.1), 0);
        assert_eq!(count(call(&r, "GET", "/api/notifications?service=0", &[A], "").await.1), 5);
    }

    #[tokio::test]
    async fn devices() {
        let iid = "3F2B8C1E-0000-4000-8000-0000000000AA";
        let token = "AB".repeat(32);
        let reg = |env: &str| {
            format!(
                r#"{{"installation_id":"{iid}","platform":"ios","topic":"com.goosheys.gootif","environment":"{env}","token":"{token}"}}"#
            )
        };
        let r = app(false);
        assert_eq!(call(&r, "POST", "/api/devices", &[], &reg("production")).await.0, 401);
        assert_eq!(call(&r, "POST", "/api/devices", &[A], r#"{"installation_id":"abc"}"#).await.0, 400);
        assert_eq!(call(&r, "POST", "/api/devices", &[A], &reg("prod")).await.0, 400);
        // Push disabled: registration is refused, disabling still works.
        assert_eq!(call(&r, "POST", "/api/devices", &[A], &reg("production")).await.0, 503);
        let disable = format!(r#"{{"installation_id":"{iid}","enabled":false}}"#);
        assert_eq!(call(&r, "POST", "/api/devices", &[A], &disable).await, (200, json!({"enabled": false})));
        let big = format!("{{{}}}", " ".repeat(5000));
        assert_eq!(call(&r, "POST", "/api/devices", &[A], &big).await.0, 413);
    }

    #[test]
    fn php_helpers() {
        assert!(php_empty(None) && php_empty(Some(&json!("0"))) && php_empty(Some(&json!(""))) && php_empty(Some(&json!({}))));
        assert!(!php_empty(Some(&json!("a"))) && !php_empty(Some(&json!(1))));
        assert_eq!((php_intval("12abc"), php_intval("  -3"), php_intval("abc"), php_intval("+7")), (12, -3, 0, 7));
        assert!(is_uuid("3f2b8c1e-0000-4000-8000-0000000000aa") && !is_uuid("3f2b8c1e00004000800000000000000000aa"));
        assert_eq!(decode_metadata(Some(&json!("[]"))), json!({}));
        assert_eq!(decode_metadata(Some(&json!(r#"{"a":1}"#))), json!({"a": 1}));
    }
}
