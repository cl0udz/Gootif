use crate::config::Config;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn enabled(cfg: &Config) -> bool {
    matches!(cfg.option("APNS_ENABLED", "0").to_lowercase().as_str(), "1" | "true" | "yes")
}

struct Credentials {
    team: String,
    key_id: String,
    key_path: String,
}

fn credentials(cfg: &Config, environment: &str) -> Credentials {
    let prefix = if environment == "production" { "APNS_PRODUCTION_" } else { "APNS_SANDBOX_" };
    Credentials {
        team: cfg.option("APNS_TEAM_ID", ""),
        key_id: cfg.option(&format!("{prefix}KEY_ID"), ""),
        key_path: cfg.option(&format!("{prefix}KEY_PATH"), ""),
    }
}

pub fn ready(cfg: &Config, environment: &str) -> bool {
    let c = credentials(cfg, environment);
    enabled(cfg) && !c.team.is_empty() && !c.key_id.is_empty() && std::fs::File::open(&c.key_path).is_ok()
}

pub fn enqueue(conn: &Connection, push_enabled: bool, notification_id: &str) -> rusqlite::Result<()> {
    if !push_enabled {
        return Ok(());
    }
    let now = now();
    conn.execute(
        "INSERT INTO push_jobs
        (notification_id, installation_id, device_version, next_attempt_at, expires_at)
        SELECT ?, installation_id, version, ?, ? FROM push_devices WHERE active = 1",
        params![notification_id, now, now + 86400],
    )?;
    Ok(())
}

pub struct Registration {
    pub installation_id: String,
    pub token: String,
    pub topic: String,
    pub environment: String,
}

pub fn register(conn: &mut Connection, r: &Registration) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    // A restored/reinstalled app can have a new installation ID but the same token.
    tx.execute(
        "DELETE FROM push_devices WHERE token = ? AND topic = ? AND environment = ? AND installation_id != ?",
        params![r.token, r.topic, r.environment, r.installation_id],
    )?;
    tx.execute(
        "INSERT INTO push_devices
        (installation_id, token, topic, environment, updated_at) VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(installation_id) DO UPDATE SET
            version = CASE WHEN token != excluded.token OR topic != excluded.topic
                OR environment != excluded.environment OR active = 0 THEN version + 1 ELSE version END,
            token = excluded.token, topic = excluded.topic, environment = excluded.environment,
            active = 1, updated_at = excluded.updated_at",
        params![r.installation_id, r.token, r.topic, r.environment, now()],
    )?;
    tx.commit()
}

struct Job {
    job_id: i64,
    notification_id: String,
    installation_id: String,
    device_version: i64,
    attempts: i64,
    expires_at: i64,
    nid: Option<String>,
    service: Option<String>,
    title: Option<String>,
    priority: Option<String>,
    token: Option<String>,
    topic: Option<String>,
    environment: Option<String>,
}

struct SendResult {
    status: u16,
    reason: String,
    timestamp: Option<i64>,
}

impl SendResult {
    fn failed(reason: &str) -> Self {
        Self { status: 0, reason: reason.into(), timestamp: None }
    }
}

fn payload(job: &Job) -> String {
    let service = job.service.clone().unwrap_or_default();
    // Titles are already capped at 500 bytes. Omit body/metadata (reports can be 1 MB).
    let mut aps = Map::new();
    aps.insert("alert".into(), json!({"title": service, "body": job.title.clone().unwrap_or_default()}));
    aps.insert("thread-id".into(), json!(service));
    if job.priority.as_deref() == Some("high") {
        aps.insert("sound".into(), json!("default"));
    }
    json!({"aps": aps, "notification_id": job.nid}).to_string()
}

pub struct Sender<'a> {
    cfg: &'a Config,
    client: reqwest::Client,
    jwt_cache: HashMap<String, (i64, String)>,
}

impl<'a> Sender<'a> {
    pub fn new(cfg: &'a Config) -> Result<Self, BoxError> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { cfg, client, jwt_cache: HashMap::new() })
    }

    fn jwt(&mut self, environment: &str) -> Result<String, BoxError> {
        let now = now();
        if let Some((at, jwt)) = self.jwt_cache.get(environment) {
            if *at > now - 3000 {
                return Ok(jwt.clone());
            }
        }
        let c = credentials(self.cfg, environment);
        let key = EncodingKey::from_ec_pem(&std::fs::read(&c.key_path)?)?;
        let mut header = Header::new(Algorithm::ES256);
        header.typ = None;
        header.kid = Some(c.key_id);
        let jwt = jsonwebtoken::encode(&header, &json!({"iss": c.team, "iat": now}), &key)?;
        self.jwt_cache.insert(environment.to_string(), (now, jwt.clone()));
        Ok(jwt)
    }

    async fn send(&mut self, job: &Job) -> SendResult {
        let environment = job.environment.as_deref().unwrap_or("");
        if !ready(self.cfg, environment) {
            return SendResult::failed("APNsNotConfigured");
        }
        let jwt = match self.jwt(environment) {
            Ok(jwt) => jwt,
            Err(e) => {
                eprintln!("[gootif-push] unable to sign APNs token: {e}");
                return SendResult::failed("SenderException");
            }
        };
        let host = if environment == "production" { "api.push.apple.com" } else { "api.sandbox.push.apple.com" };
        let request = self
            .client
            .post(format!("https://{host}/3/device/{}", job.token.as_deref().unwrap_or("")))
            .header("authorization", format!("bearer {jwt}"))
            .header("apns-topic", job.topic.as_deref().unwrap_or(""))
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .header("apns-expiration", job.expires_at.to_string())
            // Retry the same logical alert without intentionally merging distinct messages.
            .header("apns-collapse-id", job.notification_id.as_str())
            .header("content-type", "application/json")
            .body(payload(job));
        let response = match request.send().await {
            Ok(r) => r,
            Err(_) => return SendResult::failed("TransportError"),
        };
        let status = response.status().as_u16();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let reason = match body.get("reason").and_then(Value::as_str) {
            Some(r) => r.to_string(),
            None if status == 200 => String::new(),
            None => "TransportError".into(),
        };
        let timestamp = body.get("timestamp").and_then(|t| t.as_i64().or_else(|| t.as_f64().map(|f| f as i64)));
        SendResult { status, reason, timestamp }
    }

    // Worker holds a process lock, so each job has at most one concurrent sender.
    // A crash after APNs accepts but before SQLite commits can retry (at-least-once).
    pub async fn process_one(&mut self, conn: &mut Connection, now: i64) -> Result<bool, BoxError> {
        let job = conn
            .query_row(
                "SELECT j.id, j.notification_id, j.installation_id, j.device_version, j.attempts, j.expires_at,
                    n.id, n.service, n.title, n.priority, d.token, d.topic, d.environment
                FROM push_jobs j
                LEFT JOIN push_devices d ON d.installation_id = j.installation_id
                    AND d.version = j.device_version AND d.active = 1
                LEFT JOIN notifications n ON n.id = j.notification_id
                WHERE j.state = 'pending' AND j.next_attempt_at <= ? ORDER BY j.id LIMIT 1",
                [now],
                |r| {
                    Ok(Job {
                        job_id: r.get(0)?,
                        notification_id: r.get(1)?,
                        installation_id: r.get(2)?,
                        device_version: r.get(3)?,
                        attempts: r.get(4)?,
                        expires_at: r.get(5)?,
                        nid: r.get(6)?,
                        service: r.get(7)?,
                        title: r.get(8)?,
                        priority: r.get(9)?,
                        token: r.get(10)?,
                        topic: r.get(11)?,
                        environment: r.get(12)?,
                    })
                },
            )
            .optional()?;
        let Some(job) = job else { return Ok(false) };

        let mut state = "pending";
        let error: String;
        let mut attempts = job.attempts;
        let mut next = now;
        let mut result = None;
        let is_empty = |v: &Option<String>| v.as_deref().map_or(true, str::is_empty);
        if is_empty(&job.token) || is_empty(&job.nid) || job.expires_at <= now {
            state = "cancelled";
            error = "DeviceChangedMessageDeletedOrExpired".into();
        } else {
            attempts += 1;
            let r = self.send(&job).await;
            error = r.reason.chars().take(100).collect();
            let status = r.status;
            if status == 200 {
                state = "sent";
            } else if status == 410 || error == "BadDeviceToken" || error == "DeviceTokenNotForTopic" {
                state = "failed";
            } else if status == 0 || status == 429 || status >= 500 || status == 403 {
                next = now + (5 * 2i64.pow(attempts.min(9) as u32)).min(3600) + rand::thread_rng().gen_range(0..=5);
                if attempts >= 30 {
                    state = "failed";
                }
            } else {
                state = "failed";
            }
            result = Some(r);
        }

        let tx = conn.transaction()?;
        // Only Unregistered invalidates registration. Never disable a newer token or
        // a registration newer than the invalidation timestamp returned by Apple.
        if let Some(SendResult { status: 410, timestamp: Some(ts), .. }) = result {
            tx.execute(
                "UPDATE push_devices SET active = 0 WHERE installation_id = ? AND version = ? AND updated_at * 1000 <= CAST(? AS INTEGER)",
                params![job.installation_id, job.device_version, ts],
            )?;
        }
        tx.execute(
            "UPDATE push_jobs SET state = ?, attempts = ?, next_attempt_at = ?, last_error = ? WHERE id = ?",
            params![state, attempts, next, (!error.is_empty()).then_some(&error), job.job_id],
        )?;
        tx.commit()?;
        if !error.is_empty() && state != "cancelled" {
            eprintln!("[gootif-push] job={} state={state} reason={error}", job.job_id);
        }
        Ok(true)
    }
}

pub async fn run_worker(cfg: &Config, once: bool) -> Result<(), BoxError> {
    if !enabled(cfg) {
        eprintln!("Set APNS_ENABLED=1 to start the push worker.");
        std::process::exit(1);
    }
    let db_path = cfg.require("DB_PATH")?;
    let mut conn = crate::db::open(&db_path)?;
    let lock = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(format!("{db_path}.push.lock"))?;
    if lock.try_lock().is_err() {
        eprintln!("Another push worker is running or lock is unavailable.");
        std::process::exit(1);
    }
    let mut sender = Sender::new(cfg)?;
    let mut last_cleanup = 0;
    loop {
        let worked = sender.process_one(&mut conn, now()).await?;
        if now() - last_cleanup > 3600 {
            conn.execute("DELETE FROM push_jobs WHERE state != 'pending' AND expires_at < ?", [now() - 7 * 86400])?;
            last_cleanup = now();
        }
        if once && !worked {
            return Ok(());
        }
        if !worked {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration(token: &str) -> Registration {
        Registration {
            installation_id: "3f2b8c1e-0000-4000-8000-0000000000aa".into(),
            token: token.into(),
            topic: "com.goosheys.gootif".into(),
            environment: "production".into(),
        }
    }

    #[test]
    fn register_and_enqueue() {
        let mut conn = crate::db::open(":memory:").unwrap();
        let version = |c: &Connection| -> i64 { c.query_row("SELECT version FROM push_devices", [], |r| r.get(0)).unwrap() };
        register(&mut conn, &registration("aa")).unwrap();
        register(&mut conn, &registration("aa")).unwrap();
        assert_eq!(version(&conn), 1, "same token keeps the version");
        register(&mut conn, &registration("bb")).unwrap();
        assert_eq!(version(&conn), 2, "new token bumps the version");

        enqueue(&conn, false, "n1").unwrap();
        enqueue(&conn, true, "n2").unwrap();
        let jobs: Vec<(String, i64)> = conn
            .prepare("SELECT notification_id, device_version FROM push_jobs")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(jobs, vec![("n2".to_string(), 2)]);
    }

    #[test]
    fn payload_shape() {
        let job = Job {
            job_id: 1,
            notification_id: "n1".into(),
            installation_id: "i".into(),
            device_version: 1,
            attempts: 0,
            expires_at: 0,
            nid: Some("n1".into()),
            service: Some("nas".into()),
            title: Some("Backup done".into()),
            priority: Some("high".into()),
            token: None,
            topic: None,
            environment: None,
        };
        let v: Value = serde_json::from_str(&payload(&job)).unwrap();
        let want = json!({
            "aps": {"alert": {"title": "nas", "body": "Backup done"}, "thread-id": "nas", "sound": "default"},
            "notification_id": "n1"
        });
        assert_eq!(v, want);
    }
}
