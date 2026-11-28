//! Administrator-configured, read-only HTTP traffic sources.
//! Provider credentials and complete provider responses never leave this module.

use std::{
    error::Error,
    fs,
    net::{IpAddr, SocketAddr},
    os::unix::fs::PermissionsExt,
    path::Path,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path as RoutePath, State},
    routing::{get, post},
};
use reqwest::{Client, Url, redirect::Policy};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::lookup_host;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    control::{TrafficSource, configuration_audit, options_or_none},
    ip_info::public_ip,
    storage::{Storage, StorageError},
};

const MAX_SOURCES: i64 = 32;
const POLL_INTERVAL_MS: i64 = 30 * 60 * 1_000;
const MAX_RESPONSE_BYTES: usize = 128 * 1024;
const MAX_SECRET_BYTES: usize = 4_096;
const DNS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthMode {
    None,
    Bearer,
    LoginRefresh,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceConfig {
    pub name: String,
    pub enabled: bool,
    pub auth_mode: AuthMode,
    pub traffic_url: String,
    pub login_url: String,
    pub refresh_url: String,
    pub username_field: String,
    pub password_field: String,
    pub refresh_token_field: String,
    pub access_pointer: String,
    pub refresh_pointer: String,
    pub up_pointer: String,
    pub down_pointer: String,
    pub used_pointer: String,
    pub limit_pointer: String,
    pub limit_unit: String,
    pub reset_pointer: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceInput {
    pub config: SourceConfig,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub bearer_token: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    username: String,
    password: String,
    bearer_token: String,
    access_token: String,
    refresh_token: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TrafficSnapshot {
    pub up_bytes: u64,
    pub down_bytes: u64,
    pub used_bytes: u64,
    pub limit_bytes: Option<u64>,
    pub reset_at: Option<String>,
    pub fetched_at_unix_ms: u64,
}

#[derive(Serialize)]
struct AdminSource {
    node_id: String,
    config: SourceConfig,
    has_credential: bool,
    snapshot: Option<TrafficSnapshot>,
    last_attempt_unix_ms: u64,
    last_success_unix_ms: u64,
    status: String,
}

pub(crate) struct StoredTrafficSummary {
    pub name: String,
    pub enabled: bool,
    pub snapshot: Option<TrafficSnapshot>,
    pub status: String,
    pub last_success_ms: i64,
    pub config_corrupt: bool,
    pub snapshot_corrupt: bool,
}

fn fallback_source_config() -> SourceConfig {
    SourceConfig {
        name: String::new(),
        enabled: false,
        auth_mode: AuthMode::None,
        traffic_url: String::new(),
        login_url: String::new(),
        refresh_url: String::new(),
        username_field: "email".to_owned(),
        password_field: "password".to_owned(),
        refresh_token_field: "refresh_token".to_owned(),
        access_pointer: "/data/access_token".to_owned(),
        refresh_pointer: "/data/refresh_token".to_owned(),
        up_pointer: String::new(),
        down_pointer: String::new(),
        used_pointer: String::new(),
        limit_pointer: String::new(),
        limit_unit: "bytes".to_owned(),
        reset_pointer: String::new(),
    }
}

fn parse_stored_source(
    raw_config: &str,
    raw_snapshot: Option<&str>,
    status: String,
) -> (SourceConfig, Option<TrafficSnapshot>, String, bool, bool) {
    let (config, mut status, config_corrupt) =
        match serde_json::from_str::<SourceConfig>(raw_config) {
            Ok(parsed) => (parsed, status, false),
            Err(_) => (
                fallback_source_config(),
                "invalid_configuration".to_owned(),
                true,
            ),
        };
    let (snapshot, snapshot_corrupt) = match raw_snapshot {
        Some(raw) => match serde_json::from_str::<TrafficSnapshot>(raw) {
            Ok(parsed) => (Some(parsed), false),
            Err(_) => (None, true),
        },
        None => (None, false),
    };
    if !config_corrupt && snapshot_corrupt && matches!(status.as_str(), "ok" | "pending") {
        status = "invalid_data".to_owned();
    }
    (config, snapshot, status, config_corrupt, snapshot_corrupt)
}

pub(crate) fn read_node_traffic_summary(
    db: &Connection,
    node_id: &str,
) -> Result<Option<StoredTrafficSummary>, StorageError> {
    let row: Option<(String, Option<String>, String, i64)> = db
        .query_row(
            "SELECT config,snapshot,status,last_success_ms
             FROM external_traffic_sources WHERE node_id=?1",
            [node_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((raw_config, raw_snapshot, status, last_success_ms)) = row else {
        return Ok(None);
    };
    let (config, snapshot, status, config_corrupt, snapshot_corrupt) =
        parse_stored_source(&raw_config, raw_snapshot.as_deref(), status);
    Ok(Some(StoredTrafficSummary {
        name: config.name,
        enabled: config.enabled,
        snapshot,
        status,
        last_success_ms,
        config_corrupt,
        snapshot_corrupt,
    }))
}

struct DueSource {
    node_id: String,
    config: SourceConfig,
    encrypted_credential: Vec<u8>,
    generation: String,
}

pub(crate) struct CredentialKey(aead::LessSafeKey);

impl CredentialKey {
    pub(crate) fn from_file(path: &Path) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
            return Err("traffic credential key file must be a private regular file (0600)".into());
        }
        let key = fs::read(path)?;
        if key.len() != 32 {
            return Err("traffic credential key file must contain exactly 32 raw bytes".into());
        }
        let key = aead::UnboundKey::new(&aead::AES_256_GCM, &key)
            .map_err(|_| "invalid traffic credential key")?;
        Ok(Self(aead::LessSafeKey::new(key)))
    }

    fn encrypt(&self, credential: &Credential) -> Result<Vec<u8>, StorageError> {
        let mut value = serde_json::to_vec(credential)
            .map_err(|_| StorageError::InvalidInput("invalid traffic credential"))?;
        if value.len() > MAX_SECRET_BYTES {
            return Err(StorageError::InvalidInput("traffic credential too large"));
        }
        let mut nonce = [0_u8; aead::NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| StorageError::InvalidInput("credential encryption failed"))?;
        self.0
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut value,
            )
            .map_err(|_| StorageError::InvalidInput("credential encryption failed"))?;
        let mut sealed = nonce.to_vec();
        sealed.extend_from_slice(&value);
        Ok(sealed)
    }

    fn decrypt(&self, sealed: &[u8]) -> Result<Credential, StorageError> {
        if sealed.len() < aead::NONCE_LEN + aead::AES_256_GCM.tag_len()
            || sealed.len() > MAX_SECRET_BYTES + aead::NONCE_LEN + aead::AES_256_GCM.tag_len()
        {
            return Err(StorageError::InvalidInput(
                "stored traffic credential is invalid",
            ));
        }
        let mut nonce = [0_u8; aead::NONCE_LEN];
        nonce.copy_from_slice(&sealed[..aead::NONCE_LEN]);
        let mut value = sealed[aead::NONCE_LEN..].to_vec();
        let plain = self
            .0
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut value,
            )
            .map_err(|_| {
                StorageError::InvalidInput("stored traffic credential cannot be decrypted")
            })?;
        serde_json::from_slice(plain)
            .map_err(|_| StorageError::InvalidInput("stored traffic credential is invalid"))
    }
}

fn validate_config(config: &SourceConfig) -> Result<(), StorageError> {
    if config.name.trim().is_empty()
        || config.name.len() > 128
        || config.name.chars().any(char::is_control)
        || !["bytes", "gb", "gib"].contains(&config.limit_unit.as_str())
        || config.login_url.len() > 2_048
        || config.refresh_url.len() > 2_048
        || [
            &config.username_field,
            &config.password_field,
            &config.refresh_token_field,
        ]
        .iter()
        .any(|value| value.len() > 64)
        || [
            &config.access_pointer,
            &config.refresh_pointer,
            &config.up_pointer,
            &config.down_pointer,
            &config.used_pointer,
            &config.limit_pointer,
            &config.reset_pointer,
        ]
        .iter()
        .any(|value| value.len() > 256)
    {
        return Err(StorageError::InvalidInput(
            "invalid traffic source configuration",
        ));
    }
    let traffic = validate_url(&config.traffic_url)?;
    if config.auth_mode == AuthMode::LoginRefresh {
        let login = validate_url(&config.login_url)?;
        let refresh = validate_url(&config.refresh_url)?;
        if login.origin() != traffic.origin()
            || refresh.origin() != traffic.origin()
            || !valid_field_name(&config.username_field)
            || !valid_field_name(&config.password_field)
            || !valid_field_name(&config.refresh_token_field)
            || config.username_field == config.password_field
            || !valid_pointer(&config.access_pointer, true)
            || !valid_pointer(&config.refresh_pointer, true)
        {
            return Err(StorageError::InvalidInput(
                "invalid traffic login configuration",
            ));
        }
    }
    if !valid_pointer(&config.up_pointer, true)
        || !valid_pointer(&config.down_pointer, true)
        || !valid_pointer(&config.used_pointer, false)
        || !valid_pointer(&config.limit_pointer, false)
        || !valid_pointer(&config.reset_pointer, false)
    {
        return Err(StorageError::InvalidInput("invalid traffic JSON pointer"));
    }
    Ok(())
}

fn valid_field_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_pointer(value: &str, required: bool) -> bool {
    (!required && value.is_empty())
        || (value.starts_with('/') && value.len() <= 256 && !value.chars().any(char::is_control))
}

fn validate_url(raw: &str) -> Result<Url, StorageError> {
    if raw.len() > 2_048 {
        return Err(StorageError::InvalidInput("traffic URL too long"));
    }
    let url = Url::parse(raw).map_err(|_| StorageError::InvalidInput("invalid traffic URL"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(StorageError::InvalidInput(
            "traffic URL must use HTTPS without user information",
        ));
    }
    Ok(url)
}

fn retire_provider_evaluation(tx: &Transaction<'_>, node_id: &str) -> Result<(), StorageError> {
    if options_or_none(tx, node_id)?
        .is_some_and(|options| options.traffic_source == TrafficSource::Provider)
    {
        crate::notifications::retire_traffic_evaluation(
            tx,
            node_id,
            crate::storage::unix_time_ms()?,
        )?;
    }
    Ok(())
}

impl Storage {
    pub(crate) fn has_traffic_sources(&self) -> Result<bool, StorageError> {
        let db = self.connection()?;
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM external_traffic_sources)",
            [],
            |row| row.get(0),
        )?)
    }

    fn list_traffic_sources(&self) -> Result<Vec<AdminSource>, StorageError> {
        let db = self.connection()?;
        let mut statement = db.prepare("SELECT node_id,config,credential,snapshot,last_attempt_ms,last_success_ms,status FROM external_traffic_sources ORDER BY node_id LIMIT ?1")?;
        let rows = statement.query_map([MAX_SOURCES], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (node_id, config, credential, snapshot, attempt, success, status) = row?;
            let (config, snapshot, status, _config_corrupt, snapshot_corrupt) =
                parse_stored_source(&config, snapshot.as_deref(), status);
            Ok(AdminSource {
                node_id,
                has_credential: config.auth_mode != AuthMode::None && !credential.is_empty(),
                config,
                snapshot,
                last_attempt_unix_ms: u64::try_from(attempt).unwrap_or(0),
                last_success_unix_ms: if snapshot_corrupt {
                    0
                } else {
                    u64::try_from(success).unwrap_or(0)
                },
                status,
            })
        })
        .collect()
    }

    fn save_traffic_source(
        &self,
        node_id: &str,
        input: SourceInput,
        key: &CredentialKey,
    ) -> Result<(), StorageError> {
        validate_config(&input.config)?;
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=?1 AND disabled_at_ms IS NULL)",
            [node_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::NotFound);
        }
        let old: Option<(String, Vec<u8>)> = tx
            .query_row(
                "SELECT config,credential FROM external_traffic_sources WHERE node_id=?1",
                [node_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if old.is_none() {
            let count: i64 =
                tx.query_row("SELECT COUNT(*) FROM external_traffic_sources", [], |row| {
                    row.get(0)
                })?;
            if count >= MAX_SOURCES {
                return Err(StorageError::InvalidInput("traffic source limit reached"));
            }
        }
        let old_config: Option<SourceConfig> = old
            .as_ref()
            .and_then(|(value, _)| serde_json::from_str(value).ok());
        let changed_secret = !input.username.is_empty()
            || !input.password.is_empty()
            || !input.bearer_token.is_empty();
        let mut credential = old
            .as_ref()
            .and_then(|(_, value)| key.decrypt(value).ok())
            .unwrap_or_default();
        if !input.username.is_empty() || !input.password.is_empty() {
            if input.username.is_empty() || input.password.is_empty() {
                return Err(StorageError::InvalidInput(
                    "username and password must be updated together",
                ));
            }
            credential.username = input.username;
            credential.password = input.password;
            credential.access_token.clear();
            credential.refresh_token.clear();
        }
        if !input.bearer_token.is_empty() {
            credential.bearer_token = input.bearer_token;
        }
        match input.config.auth_mode {
            AuthMode::None => credential = Credential::default(),
            AuthMode::Bearer => {
                if credential.bearer_token.is_empty() {
                    return Err(StorageError::InvalidInput("bearer token is required"));
                }
                credential.username.clear();
                credential.password.clear();
                credential.access_token.clear();
                credential.refresh_token.clear();
            }
            AuthMode::LoginRefresh => {
                if credential.username.is_empty() || credential.password.is_empty() {
                    return Err(StorageError::InvalidInput("login credentials are required"));
                }
                credential.bearer_token.clear();
            }
        }
        let changed = changed_secret || old_config.as_ref().is_none_or(|old| old != &input.config);
        if changed && input.config.auth_mode == AuthMode::LoginRefresh {
            credential.access_token.clear();
            credential.refresh_token.clear();
        }
        let config = serde_json::to_string(&input.config)
            .map_err(|_| StorageError::InvalidInput("invalid traffic configuration"))?;
        let sealed = key.encrypt(&credential)?;
        if changed {
            retire_provider_evaluation(&tx, node_id)?;
        }
        let generation = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO external_traffic_sources(node_id,config,credential,last_attempt_ms,status,generation)
             VALUES(?1,?2,?3,0,'pending',?5) ON CONFLICT(node_id) DO UPDATE SET
             config=excluded.config,credential=excluded.credential,last_attempt_ms=0,
             snapshot=CASE WHEN ?4 THEN NULL ELSE external_traffic_sources.snapshot END,
             last_success_ms=CASE WHEN ?4 THEN 0 ELSE external_traffic_sources.last_success_ms END,
             generation=excluded.generation,
             status='pending'",
            params![node_id, config, sealed, changed, generation],
        )?;
        configuration_audit(&tx, "traffic_source.save", node_id)?;
        tx.commit()?;
        Ok(())
    }

    fn delete_traffic_source(&self, node_id: &str) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        if tx.execute(
            "DELETE FROM external_traffic_sources WHERE node_id=?1",
            [node_id],
        )? == 0
        {
            return Err(StorageError::NotFound);
        }
        retire_provider_evaluation(&tx, node_id)?;
        configuration_audit(&tx, "traffic_source.delete", node_id)?;
        tx.commit()?;
        Ok(())
    }

    fn claim_due_traffic_sources(&self, now: u64) -> Result<Vec<DueSource>, StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let due = i64::try_from(now)
            .unwrap_or(i64::MAX)
            .saturating_sub(POLL_INTERVAL_MS);
        let mut statement = tx.prepare("SELECT s.node_id,s.config,s.credential,s.generation FROM external_traffic_sources s JOIN nodes n ON n.id=s.node_id
            WHERE n.disabled_at_ms IS NULL AND CASE WHEN json_valid(s.config) THEN json_extract(s.config,'$.enabled')=1 ELSE 1 END AND s.status!='authentication_required' AND s.last_attempt_ms<=?1
            ORDER BY s.last_attempt_ms,s.node_id LIMIT 8")?;
        let rows = statement.query_map([due], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut sources = Vec::new();
        let mut invalid = Vec::new();
        for row in rows {
            let (node_id, config, encrypted_credential, generation) = row?;
            match serde_json::from_str(&config) {
                Ok(config) => sources.push(DueSource {
                    node_id,
                    config,
                    encrypted_credential,
                    generation,
                }),
                Err(_) => invalid.push((node_id, generation)),
            }
        }
        drop(statement);
        let attempt_at = i64::try_from(now).unwrap_or(i64::MAX);
        for (node_id, generation) in &invalid {
            tx.execute(
                "UPDATE external_traffic_sources SET last_attempt_ms=?2,status='invalid_configuration' WHERE node_id=?1 AND generation=?3",
                params![node_id, attempt_at, generation],
            )?;
        }
        for source in &sources {
            tx.execute(
                "UPDATE external_traffic_sources SET last_attempt_ms=?2 WHERE node_id=?1",
                params![source.node_id, attempt_at],
            )?;
        }
        tx.commit()?;
        Ok(sources)
    }

    fn complete_traffic_source(
        &self,
        node_id: &str,
        generation: &str,
        credential: Option<&[u8]>,
        snapshot: Option<&TrafficSnapshot>,
        status: &str,
        now: u64,
    ) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let encoded = snapshot
            .map(|value| {
                serde_json::to_string(value)
                    .map_err(|_| StorageError::InvalidInput("invalid traffic snapshot"))
            })
            .transpose()?;
        tx.execute("UPDATE external_traffic_sources SET credential=COALESCE(?2,credential),snapshot=COALESCE(?3,snapshot),
            last_success_ms=CASE WHEN ?3 IS NULL THEN last_success_ms ELSE ?5 END,status=?4 WHERE node_id=?1 AND generation=?6",
            params![node_id, credential, encoded, status, i64::try_from(now).unwrap_or(i64::MAX), generation])?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/traffic-sources", get(admin_sources))
        .route("/api/admin/traffic-sources/{node_id}", post(save_source))
        .route(
            "/api/admin/traffic-sources/{node_id}/delete",
            post(delete_source),
        )
}

async fn admin_sources(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let sources = state.database(Storage::list_traffic_sources).await?;
    Ok(Json(
        json!({"sources":sources,"credential_key_ready":state.traffic_key.is_some()}),
    ))
}

async fn save_source(
    State(state): State<AppState>,
    RoutePath(node_id): RoutePath<String>,
    Json(input): Json<SourceInput>,
) -> Result<Json<Value>, ApiError> {
    let key = state
        .traffic_key
        .clone()
        .ok_or_else(|| ApiError::bad_request("traffic credential key is not configured"))?;
    state
        .database(move |storage| storage.save_traffic_source(&node_id, input, &key))
        .await?;
    Ok(Json(json!({"ok":true})))
}

async fn delete_source(
    State(state): State<AppState>,
    RoutePath(node_id): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .database(move |storage| storage.delete_traffic_source(&node_id))
        .await?;
    Ok(Json(json!({"ok":true})))
}

impl AppState {
    /// Poll at most eight due sources serially; the Service never sends commands to Agents.
    ///
    /// # Errors
    ///
    /// Returns an error if the clock or initial due-source claim fails.
    pub async fn maintain_external_traffic(&self) -> Result<(), String> {
        let now = crate::current_time().map_err(|error| error.message)?;
        let sources = self
            .database(move |storage| storage.claim_due_traffic_sources(now))
            .await
            .map_err(|error| error.message)?;
        let poll = |config: SourceConfig, mut credential: Credential, now: u64| async move {
            let result = poll_source(&config, &mut credential, now).await;
            (credential, result)
        };
        self.sync_claimed_sources(sources, now, poll).await;
        Ok(())
    }

    async fn sync_claimed_sources<F, Fut>(&self, sources: Vec<DueSource>, now: u64, mut poll: F)
    where
        F: FnMut(SourceConfig, Credential, u64) -> Fut,
        Fut: Future<Output = (Credential, Result<TrafficSnapshot, &'static str>)>,
    {
        for source in sources {
            let (snapshot, credential, status) = if let Some(key) = self.traffic_key.as_ref() {
                match key.decrypt(&source.encrypted_credential) {
                    Ok(credential) => {
                        let (updated, result) = poll(source.config, credential, now).await;
                        match key.encrypt(&updated) {
                            Ok(sealed) => match result {
                                Ok(snapshot) => (Some(snapshot), Some(sealed), "ok"),
                                Err(status) => (None, Some(sealed), status),
                            },
                            Err(_) => (None, None, "credential_unavailable"),
                        }
                    }
                    Err(_) => (None, None, "credential_unavailable"),
                }
            } else {
                (None, None, "credential_unavailable")
            };
            let node_id = source.node_id;
            let generation = source.generation;
            let logged_node_id = node_id.clone();
            if let Err(error) = self
                .database(move |storage| {
                    storage.complete_traffic_source(
                        &node_id,
                        &generation,
                        credential.as_deref(),
                        snapshot.as_ref(),
                        status,
                        now,
                    )
                })
                .await
            {
                tracing::warn!(
                    node_id = %logged_node_id,
                    error = %error.message,
                    "could not persist external traffic poll result"
                );
            }
        }
    }
}

fn credential_fits_envelope(credential: &Credential) -> bool {
    serde_json::to_vec(credential).is_ok_and(|bytes| bytes.len() <= MAX_SECRET_BYTES)
}

async fn poll_source(
    config: &SourceConfig,
    credential: &mut Credential,
    now: u64,
) -> Result<TrafficSnapshot, &'static str> {
    let request = |url, method, body: Option<Value>, token: Option<String>| async move {
        request_json(&url, method, body.as_ref(), token.as_deref()).await
    };
    poll_source_with(config, credential, now, request).await
}

async fn poll_source_with<F, Fut>(
    config: &SourceConfig,
    credential: &mut Credential,
    now: u64,
    mut request: F,
) -> Result<TrafficSnapshot, &'static str>
where
    F: FnMut(Url, reqwest::Method, Option<Value>, Option<String>) -> Fut,
    Fut: Future<Output = Result<Value, &'static str>>,
{
    let traffic_url = validate_url(&config.traffic_url).map_err(|_| "invalid_configuration")?;
    let mut freshly_authenticated = false;
    let mut token = match config.auth_mode {
        AuthMode::None => None,
        AuthMode::Bearer => Some(credential.bearer_token.clone()),
        AuthMode::LoginRefresh => {
            if credential.access_token.is_empty() {
                authenticate_with(config, credential, &mut request).await?;
                freshly_authenticated = true;
            }
            Some(credential.access_token.clone())
        }
    };
    let mut response = request(
        traffic_url.clone(),
        reqwest::Method::GET,
        None,
        token.clone(),
    )
    .await;
    if matches!(response, Err("authentication_required"))
        && config.auth_mode == AuthMode::LoginRefresh
        && !freshly_authenticated
    {
        if credential.refresh_token.is_empty() {
            authenticate_with(config, credential, &mut request).await?;
        } else {
            match refresh_with(config, credential, &mut request).await {
                Ok(()) => {}
                // Only an explicit HTTP 401/403 permits one password-login attempt.
                // Invalid refresh responses and transient failures never trigger login.
                Err("refresh_http_authentication_rejected") => {
                    authenticate_with(config, credential, &mut request).await?;
                }
                Err(status) => return Err(status),
            }
        }
        token = Some(credential.access_token.clone());
        response = request(traffic_url, reqwest::Method::GET, None, token).await;
    }
    let value = response?;
    parse_snapshot(config, &value, now)
}

async fn authenticate_with<F, Fut>(
    config: &SourceConfig,
    credential: &mut Credential,
    request: &mut F,
) -> Result<(), &'static str>
where
    F: FnMut(Url, reqwest::Method, Option<Value>, Option<String>) -> Fut,
    Fut: Future<Output = Result<Value, &'static str>>,
{
    let url = validate_url(&config.login_url).map_err(|_| "invalid_configuration")?;
    let mut fields = serde_json::Map::new();
    fields.insert(
        config.username_field.clone(),
        Value::String(credential.username.clone()),
    );
    fields.insert(
        config.password_field.clone(),
        Value::String(credential.password.clone()),
    );
    let body = Value::Object(fields);
    let value = request(url, reqwest::Method::POST, Some(body), None).await?;
    let access = match token_field(&value, &config.access_pointer) {
        TokenField::Value(token) => token,
        TokenField::Oversized => return Err("invalid_data"),
        TokenField::Missing => return Err("authentication_required"),
    };
    let next_refresh = match token_field(&value, &config.refresh_pointer) {
        TokenField::Value(token) => token,
        TokenField::Missing => "",
        TokenField::Oversized => return Err("invalid_data"),
    };
    let mut updated = credential.clone();
    access.clone_into(&mut updated.access_token);
    next_refresh.clone_into(&mut updated.refresh_token);
    if !credential_fits_envelope(&updated) {
        return Err("invalid_data");
    }
    *credential = updated;
    Ok(())
}

async fn refresh_with<F, Fut>(
    config: &SourceConfig,
    credential: &mut Credential,
    request: &mut F,
) -> Result<(), &'static str>
where
    F: FnMut(Url, reqwest::Method, Option<Value>, Option<String>) -> Fut,
    Fut: Future<Output = Result<Value, &'static str>>,
{
    let url = validate_url(&config.refresh_url).map_err(|_| "invalid_configuration")?;
    let mut fields = serde_json::Map::new();
    fields.insert(
        config.refresh_token_field.clone(),
        Value::String(credential.refresh_token.clone()),
    );
    let body = Value::Object(fields);
    let value = request(url, reqwest::Method::POST, Some(body), None)
        .await
        .map_err(|status| match status {
            // request_json uses this status only for an HTTP 401/403 response.
            "authentication_required" => "refresh_http_authentication_rejected",
            // A non-JSON challenge or changed response requires administrator review.
            "invalid_data" => "authentication_required",
            status => status,
        })?;
    let access = match token_field(&value, &config.access_pointer) {
        TokenField::Value(token) => token,
        TokenField::Oversized => return Err("invalid_data"),
        TokenField::Missing => return Err("authentication_required"),
    };
    let next = match token_field(&value, &config.refresh_pointer) {
        TokenField::Value(token) => token.to_owned(),
        TokenField::Missing => credential.refresh_token.clone(),
        TokenField::Oversized => return Err("invalid_data"),
    };
    let mut updated = credential.clone();
    access.clone_into(&mut updated.access_token);
    updated.refresh_token = next;
    if !credential_fits_envelope(&updated) {
        return Err("invalid_data");
    }
    *credential = updated;
    Ok(())
}

enum TokenField<'a> {
    Missing,
    Oversized,
    Value(&'a str),
}

fn token_field<'a>(value: &'a Value, pointer: &str) -> TokenField<'a> {
    let Some(raw) = value.pointer(pointer).and_then(Value::as_str) else {
        return TokenField::Missing;
    };
    if raw.is_empty() {
        return TokenField::Missing;
    }
    if raw.len() > MAX_SECRET_BYTES {
        return TokenField::Oversized;
    }
    TokenField::Value(raw)
}

fn json_string<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= MAX_SECRET_BYTES)
}

fn required_bytes(value: &Value, pointer: &str) -> Result<u64, &'static str> {
    value
        .pointer(pointer)
        .and_then(|field| {
            field.as_u64().or_else(|| {
                field
                    .as_str()
                    .filter(|text| {
                        !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    .and_then(|text| text.parse().ok())
            })
        })
        .ok_or("invalid_data")
}

fn parse_snapshot(
    config: &SourceConfig,
    value: &Value,
    now: u64,
) -> Result<TrafficSnapshot, &'static str> {
    let up_bytes = required_bytes(value, &config.up_pointer)?;
    let down_bytes = required_bytes(value, &config.down_pointer)?;
    let used_bytes = if config.used_pointer.is_empty() {
        up_bytes.checked_add(down_bytes).ok_or("invalid_data")?
    } else {
        required_bytes(value, &config.used_pointer)?
    };
    let limit_bytes = if config.limit_pointer.is_empty() {
        None
    } else {
        let raw = required_bytes(value, &config.limit_pointer)?;
        let multiplier = match config.limit_unit.as_str() {
            "bytes" => 1_u64,
            "gb" => 1_000_000_000,
            "gib" => 1_073_741_824,
            _ => return Err("invalid_configuration"),
        };
        Some(raw.checked_mul(multiplier).ok_or("invalid_data")?)
    };
    let reset_at = if config.reset_pointer.is_empty() {
        None
    } else {
        Some(
            json_string(value, &config.reset_pointer)
                .ok_or("invalid_data")?
                .to_owned(),
        )
    };
    Ok(TrafficSnapshot {
        up_bytes,
        down_bytes,
        used_bytes,
        limit_bytes,
        reset_at,
        fetched_at_unix_ms: now,
    })
}

fn extract_literal_ip(url: &Url) -> Option<IpAddr> {
    let host = url.host_str()?;
    let unbracketed = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    unbracketed.parse::<IpAddr>().ok()
}

enum OutboundTarget<'a> {
    LiteralIp,
    ResolvedDomain(&'a str, Vec<SocketAddr>),
}

async fn resolve_public_target(url: &Url) -> Result<OutboundTarget<'_>, &'static str> {
    let host = url.host_str().ok_or("invalid_configuration")?;
    let port = url.port_or_known_default().ok_or("invalid_configuration")?;
    if let Some(ip) = extract_literal_ip(url) {
        if !public_ip(ip) {
            return Err("private_address_rejected");
        }
        return Ok(OutboundTarget::LiteralIp);
    }
    let addresses = tokio::time::timeout(DNS_TIMEOUT, lookup_host((host, port)))
        .await
        .map_err(|_| "provider_unavailable")?
        .map_err(|_| "provider_unavailable")?;
    let resolved: Vec<SocketAddr> = addresses
        .map(|address| SocketAddr::new(address.ip(), port))
        .collect();
    if resolved.is_empty() {
        return Err("provider_unavailable");
    }
    if resolved.iter().any(|address| !public_ip(address.ip())) {
        return Err("private_address_rejected");
    }
    Ok(OutboundTarget::ResolvedDomain(host, resolved))
}

async fn request_json(
    url: &Url,
    method: reqwest::Method,
    body: Option<&Value>,
    token: Option<&str>,
) -> Result<Value, &'static str> {
    let target = resolve_public_target(url).await?;
    let mut builder = Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(10))
        .no_proxy();
    if let OutboundTarget::ResolvedDomain(host, addresses) = target {
        builder = builder.resolve_to_addrs(host, &addresses);
    }
    let client = builder.build().map_err(|_| "provider_unavailable")?;
    let mut request = client.request(method, url.clone());
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let mut response = request.send().await.map_err(|_| "provider_unavailable")?;
    if matches!(
        response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err("authentication_required");
    }
    if !response.status().is_success() {
        return Err("provider_http_error");
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("response_too_large");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "provider_unavailable")? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err("response_too_large");
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid_data")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SourceConfig {
        SourceConfig {
            name: "example".to_owned(),
            enabled: true,
            auth_mode: AuthMode::None,
            traffic_url: "https://example.com/usage".to_owned(),
            login_url: String::new(),
            refresh_url: String::new(),
            username_field: String::new(),
            password_field: String::new(),
            refresh_token_field: String::new(),
            access_pointer: String::new(),
            refresh_pointer: String::new(),
            up_pointer: "/data/up".to_owned(),
            down_pointer: "/data/down".to_owned(),
            used_pointer: "/data/billed".to_owned(),
            limit_pointer: "/data/quota".to_owned(),
            limit_unit: "gib".to_owned(),
            reset_pointer: "/data/reset".to_owned(),
        }
    }

    #[test]
    fn parses_billable_usage_separately_from_directional_counters() {
        let source = config();
        let response =
            json!({"data":{"up":100,"down":200,"billed":250,"quota":2,"reset":"2026-11-01"}});
        let snapshot = parse_snapshot(&source, &response, 123).expect("valid snapshot");
        assert_eq!(snapshot.up_bytes, 100);
        assert_eq!(snapshot.down_bytes, 200);
        assert_eq!(snapshot.used_bytes, 250);
        assert_eq!(snapshot.limit_bytes, Some(2 * 1_073_741_824));
        assert_eq!(snapshot.reset_at.as_deref(), Some("2026-11-01"));
        assert_eq!(snapshot.fetched_at_unix_ms, 123);
    }

    #[test]
    fn rejects_non_https_and_bad_pointers() {
        let mut source = config();
        source.traffic_url = "http://example.com/usage".to_owned();
        assert!(validate_config(&source).is_err());
        source.traffic_url = "https://example.com/usage".to_owned();
        source.up_pointer = "data/up".to_owned();
        assert!(validate_config(&source).is_err());
    }

    #[test]
    fn rejects_overflow_and_missing_counter() {
        let mut source = config();
        source.used_pointer.clear();
        assert!(
            parse_snapshot(
                &source,
                &json!({"data":{"up":u64::MAX,"down":1,"quota":2,"reset":"next"}}),
                1
            )
            .is_err()
        );
        assert!(
            parse_snapshot(
                &source,
                &json!({"data":{"up":1,"quota":2,"reset":"next"}}),
                1
            )
            .is_err()
        );
    }

    #[test]
    fn accepts_decimal_integer_strings_but_not_fractional_units() {
        let source = config();
        let response =
            json!({"data":{"up":"100","down":"200","billed":"250","quota":"2","reset":"next"}});
        assert_eq!(
            parse_snapshot(&source, &response, 1)
                .expect("decimal integer")
                .used_bytes,
            250
        );
        let response =
            json!({"data":{"up":"1.5","down":200,"billed":250,"quota":2,"reset":"next"}});
        assert!(parse_snapshot(&source, &response, 1).is_err());
    }

    fn state_with_key() -> (tempfile::TempDir, AppState, CredentialKey) {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("traffic.key");
        fs::write(&key_path, [7_u8; 32]).unwrap();
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = crate::ServiceConfig::with_database(dir.path().join("senlun.db"));
        config.traffic_key_file = Some(key_path.clone());
        let state = AppState::open(&config).unwrap();
        let key = CredentialKey::from_file(&key_path).unwrap();
        (dir, state, key)
    }

    fn enroll_synthetic_node(state: &AppState, name: &str) -> String {
        let now = crate::storage::unix_time_ms().unwrap();
        let secret = state.storage.create_enrollment(600, now).unwrap();
        let request = senlun_protocol::EnrollmentRequest {
            protocol_version: senlun_protocol::PROTOCOL_VERSION,
            node_name: name.to_owned(),
            agent_version: "test".to_owned(),
            region: "SG".to_owned(),
            group: String::new(),
        };
        let token_hash = crate::storage::hash_token(&secret.token);
        state
            .storage
            .enroll(&request, &token_hash, 100, now)
            .unwrap()
            .node_id
    }

    fn save_enabled_source(state: &AppState, key: &CredentialKey, node_id: &str, name: &str) {
        let mut source = config();
        source.name = name.to_owned();
        source.traffic_url = "https://example.invalid/usage".to_owned();
        let input = SourceInput {
            config: source,
            username: String::new(),
            password: String::new(),
            bearer_token: String::new(),
        };
        state
            .storage
            .save_traffic_source(node_id, input, key)
            .unwrap();
    }

    #[tokio::test]
    async fn per_source_encrypt_and_persistence_failures_do_not_stall_remaining_sources() {
        let (_dir, state, key) = state_with_key();
        let node_a = enroll_synthetic_node(&state, "synthetic-a");
        let node_b = enroll_synthetic_node(&state, "synthetic-b");
        let node_c = enroll_synthetic_node(&state, "synthetic-c");
        save_enabled_source(&state, &key, &node_a, "source-a");
        save_enabled_source(&state, &key, &node_b, "source-b");
        save_enabled_source(&state, &key, &node_c, "source-c");

        let prior_snapshot = TrafficSnapshot {
            up_bytes: 10,
            down_bytes: 20,
            used_bytes: 30,
            limit_bytes: None,
            reset_at: None,
            fetched_at_unix_ms: 1_000,
        };
        let encoded_prior = serde_json::to_string(&prior_snapshot).unwrap();
        {
            let db = state.storage.connection().unwrap();
            db.execute(
                "UPDATE external_traffic_sources
                 SET snapshot=?2,last_success_ms=1000,last_attempt_ms=1,status='ok'
                 WHERE node_id=?1",
                params![node_a, encoded_prior],
            )
            .unwrap();
            db.execute(
                "UPDATE external_traffic_sources SET last_attempt_ms=2 WHERE node_id=?1",
                [&node_b],
            )
            .unwrap();
            db.execute(
                "UPDATE external_traffic_sources SET last_attempt_ms=3 WHERE node_id=?1",
                [&node_c],
            )
            .unwrap();
        }

        let now = u64::try_from(POLL_INTERVAL_MS).unwrap() + 10_000;
        let claimed = state.storage.claim_due_traffic_sources(now).unwrap();
        let claimed_ids: Vec<&str> = claimed
            .iter()
            .map(|source| source.node_id.as_str())
            .collect();
        assert_eq!(
            claimed_ids,
            vec![node_a.as_str(), node_b.as_str(), node_c.as_str()]
        );

        // Fail persistence only for node_b; node_a fails encryption; node_c must still finish.
        let trigger = format!(
            "CREATE TRIGGER fail_source_b BEFORE UPDATE ON external_traffic_sources
             WHEN NEW.node_id = '{node_b}' BEGIN SELECT RAISE(ABORT, 'synthetic db failure'); END;"
        );
        state
            .storage
            .connection()
            .unwrap()
            .execute_batch(&trigger)
            .unwrap();

        let poll = |cfg: SourceConfig, mut credential: Credential, ts: u64| async move {
            if cfg.name == "source-a" {
                credential.access_token = "a".repeat(MAX_SECRET_BYTES);
            }
            let snapshot = TrafficSnapshot {
                up_bytes: 100,
                down_bytes: 200,
                used_bytes: 300,
                limit_bytes: None,
                reset_at: None,
                fetched_at_unix_ms: ts,
            };
            (credential, Ok(snapshot))
        };
        state.sync_claimed_sources(claimed, now, poll).await;

        let listed = state.storage.list_traffic_sources().unwrap();
        let result_a = listed.iter().find(|item| item.node_id == node_a).unwrap();
        assert_eq!(result_a.status, "credential_unavailable");
        assert_eq!(result_a.last_success_unix_ms, 1_000);
        assert_eq!(result_a.snapshot.as_ref().unwrap().used_bytes, 30);

        let result_b = listed.iter().find(|item| item.node_id == node_b).unwrap();
        assert_eq!(result_b.status, "pending");
        assert_eq!(result_b.last_success_unix_ms, 0);
        assert!(result_b.snapshot.is_none());

        let result_c = listed.iter().find(|item| item.node_id == node_c).unwrap();
        assert_eq!(result_c.status, "ok");
        assert_eq!(result_c.last_success_unix_ms, now);
        assert_eq!(result_c.snapshot.as_ref().unwrap().used_bytes, 300);
    }

    fn save_provider_node(state: &AppState, key: &CredentialKey, name: &str) -> String {
        let node_id = enroll_synthetic_node(state, name);
        let mut options = crate::control::NodeOptions {
            id: node_id.clone(),
            name: name.to_owned(),
            currency: "USD".to_owned(),
            traffic_source: TrafficSource::Provider,
            traffic_limit_type: "sum".to_owned(),
            traffic_reset_day: 1,
            ..crate::control::NodeOptions::default()
        };
        state
            .storage
            .save_node_options(&node_id, &mut options)
            .unwrap();
        save_enabled_source(state, key, &node_id, name);
        node_id
    }

    #[test]
    fn corrupt_source_or_snapshot_degrades_single_node_in_admin_state_and_dashboard() {
        let (_dir, state, key) = state_with_key();
        let bad_config = save_provider_node(&state, &key, "synthetic-bad-config");
        let bad_snapshot = save_provider_node(&state, &key, "synthetic-bad-snapshot");
        let healthy = save_provider_node(&state, &key, "synthetic-healthy");

        let now = crate::storage::unix_time_ms().unwrap();
        let healthy_snapshot = TrafficSnapshot {
            up_bytes: 200,
            down_bytes: 300,
            used_bytes: 500,
            limit_bytes: Some(1_000),
            reset_at: Some("2026-12-01".to_owned()),
            fetched_at_unix_ms: now,
        };
        let encoded_healthy = serde_json::to_string(&healthy_snapshot).unwrap();
        {
            let db = state.storage.connection().unwrap();
            db.execute(
                "UPDATE external_traffic_sources
                 SET snapshot=?2,last_success_ms=?3,status='ok' WHERE node_id=?1",
                params![healthy, encoded_healthy, i64::try_from(now).unwrap()],
            )
            .unwrap();
            db.execute(
                "UPDATE external_traffic_sources SET config='{broken json' WHERE node_id=?1",
                [&bad_config],
            )
            .unwrap();
            db.execute(
                "UPDATE external_traffic_sources
                 SET snapshot='{broken snapshot',last_success_ms=?2,status='ok' WHERE node_id=?1",
                params![bad_snapshot, i64::try_from(now).unwrap()],
            )
            .unwrap();
        }

        let admin = state.storage.admin_state().unwrap();
        let admin_nodes = admin["nodes"].as_array().unwrap();
        assert_eq!(admin_nodes.len(), 3);
        let admin_for = |id: &str| {
            admin_nodes
                .iter()
                .find(|item| item["id"].as_str() == Some(id))
                .unwrap()
        };
        assert_eq!(admin_for(&healthy)["selected_traffic"]["status"], "ready");
        assert_eq!(admin_for(&healthy)["selected_traffic"]["used_bytes"], 500);
        assert_eq!(
            admin_for(&bad_config)["selected_traffic"]["status"],
            "sync_failed"
        );
        assert!(admin_for(&bad_config)["selected_traffic"]["percentage"].is_null());
        assert_eq!(
            admin_for(&bad_snapshot)["selected_traffic"]["status"],
            "sync_failed"
        );
        assert!(admin_for(&bad_snapshot)["selected_traffic"]["used_bytes"].is_null());
        assert!(admin_for(&bad_snapshot)["selected_traffic"]["last_success_unix_ms"].is_null());

        let dashboard = state
            .storage
            .emerald_dashboard(90, 100, &crate::control::DashboardProbeConfig::default())
            .unwrap();
        let clients = dashboard["clients"].as_object().unwrap();
        assert_eq!(clients.len(), 3);
        assert_eq!(clients[&healthy]["selected_traffic"]["status"], "ready");
        assert_eq!(
            clients[&healthy]["external_traffic"]["snapshot"]["used_bytes"],
            500
        );
        assert_eq!(
            clients[&bad_config]["selected_traffic"]["status"],
            "sync_failed"
        );
        assert!(clients[&bad_config].get("external_traffic").is_none());
        assert_eq!(
            clients[&bad_snapshot]["selected_traffic"]["status"],
            "sync_failed"
        );
        assert!(clients[&bad_snapshot]["external_traffic"]["snapshot"].is_null());
        assert_eq!(
            clients[&bad_snapshot]["external_traffic"]["status"],
            "invalid_data"
        );

        let (total, native) = state.storage.native_nodes(0, 10, 90).unwrap();
        assert_eq!(total, 3);
        assert_eq!(native.len(), 3);
    }

    #[test]
    fn corrupt_stored_config_does_not_block_claiming_healthy_sources() {
        let (_dir, state, key) = state_with_key();
        let syntax_bad = enroll_synthetic_node(&state, "synthetic-syntax-bad");
        let schema_bad = enroll_synthetic_node(&state, "synthetic-schema-bad");
        let good_node = enroll_synthetic_node(&state, "synthetic-good");
        save_enabled_source(&state, &key, &syntax_bad, "syntax-bad-source");
        save_enabled_source(&state, &key, &schema_bad, "schema-bad-source");
        save_enabled_source(&state, &key, &good_node, "good-source");
        {
            let db = state.storage.connection().unwrap();
            db.execute(
                "UPDATE external_traffic_sources SET config='{broken json' WHERE node_id=?1",
                [&syntax_bad],
            )
            .unwrap();
            db.execute(
                "UPDATE external_traffic_sources SET config='{\"enabled\":true}' WHERE node_id=?1",
                [&schema_bad],
            )
            .unwrap();
        }

        let now = u64::try_from(POLL_INTERVAL_MS).unwrap() + 10_000;
        let claimed = state.storage.claim_due_traffic_sources(now).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].node_id, good_node);
        for node_id in [&syntax_bad, &schema_bad] {
            let status: String = state
                .storage
                .connection()
                .unwrap()
                .query_row(
                    "SELECT status FROM external_traffic_sources WHERE node_id=?1",
                    [node_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(status, "invalid_configuration");
        }
        let listed = state.storage.list_traffic_sources().unwrap();
        assert_eq!(listed.len(), 3);
        let broken = listed
            .iter()
            .find(|item| item.node_id == syntax_bad)
            .unwrap();
        assert_eq!(broken.status, "invalid_configuration");
        save_enabled_source(&state, &key, &syntax_bad, "repaired-source");
        let repaired = state.storage.list_traffic_sources().unwrap();
        let fixed = repaired
            .iter()
            .find(|item| item.node_id == syntax_bad)
            .unwrap();
        assert_eq!(fixed.config.name, "repaired-source");
    }

    fn login_refresh_config() -> SourceConfig {
        let mut source = config();
        source.auth_mode = AuthMode::LoginRefresh;
        source.traffic_url = "https://example.invalid/usage".to_owned();
        source.login_url = "https://example.invalid/login".to_owned();
        source.refresh_url = "https://example.invalid/refresh".to_owned();
        source.username_field = "email".to_owned();
        source.password_field = "password".to_owned();
        source.refresh_token_field = "refresh_token".to_owned();
        source.access_pointer = "/access".to_owned();
        source.refresh_pointer = "/refresh".to_owned();
        source
    }

    #[tokio::test]
    async fn fresh_login_does_not_relogin_on_immediate_auth_rejection() {
        let source = login_refresh_config();
        let mut credential = Credential {
            username: "synthetic-user".to_owned(),
            password: "synthetic-password".to_owned(),
            ..Credential::default()
        };
        let login_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&login_calls);
        let request = move |url: Url, _method, _body, _token| {
            let counter = std::sync::Arc::clone(&counter);
            async move {
                if url.path() == "/login" {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    return Ok(json!({"access": "fresh-access", "refresh": "fresh-refresh"}));
                }
                Err("authentication_required")
            }
        };
        let Err(err) = poll_source_with(&source, &mut credential, 100, request).await else {
            panic!("expected immediate 401/403 after fresh login to fail");
        };
        assert_eq!(err, "authentication_required");
        assert_eq!(login_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn oversized_single_or_combined_tokens_return_invalid_data_and_preserve_credential() {
        let source = login_refresh_config();
        let base = Credential {
            username: "synthetic-user".to_owned(),
            password: "synthetic-password".to_owned(),
            access_token: "kept-access".to_owned(),
            refresh_token: "kept-refresh".to_owned(),
            ..Credential::default()
        };

        let cases = [
            ("a".repeat(MAX_SECRET_BYTES + 1), "small-refresh".to_owned()),
            ("small-access".to_owned(), "r".repeat(MAX_SECRET_BYTES + 1)),
            ("x".repeat(2_500), "y".repeat(2_500)),
        ];
        for (access, refresh) in cases {
            let mut existing = base.clone();
            let mut req = move |_url, _method, _body, _token| {
                let access = access.clone();
                let refresh = refresh.clone();
                async move { Ok(json!({"access": access, "refresh": refresh})) }
            };
            let refresh_err = refresh_with(&source, &mut existing, &mut req)
                .await
                .unwrap_err();
            assert_eq!(refresh_err, "invalid_data");
            assert_eq!(existing.access_token, "kept-access");
            assert_eq!(existing.refresh_token, "kept-refresh");

            let auth_err = authenticate_with(&source, &mut existing, &mut req)
                .await
                .unwrap_err();
            assert_eq!(auth_err, "invalid_data");
            assert_eq!(existing.access_token, "kept-access");
            assert_eq!(existing.refresh_token, "kept-refresh");
        }
    }

    #[tokio::test]
    async fn ipv6_literal_urls_parse_without_brackets_and_reject_non_public_addresses() {
        let doc_v6 = Url::parse("https://[2001:db8::1]/usage").unwrap();
        let loopback_v6 = Url::parse("https://[::1]/usage").unwrap();
        let doc_v4 = Url::parse("https://198.51.100.10/usage").unwrap();
        assert_eq!(
            extract_literal_ip(&doc_v6),
            Some("2001:db8::1".parse().unwrap())
        );
        assert_eq!(
            extract_literal_ip(&loopback_v6),
            Some("::1".parse().unwrap())
        );
        for url in [doc_v6, loopback_v6, doc_v4] {
            let Err(status) = resolve_public_target(&url).await else {
                panic!("non-public literal address must be rejected");
            };
            assert_eq!(status, "private_address_rejected");
        }
    }
}
