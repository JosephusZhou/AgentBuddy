use crate::ai_provider::AiProviderRow;
use crate::claude_env::ClaudeEnvironmentRow;
use crate::codex_env::CodexEnvironmentRow;
use crate::mcp_config::McpServerRecord;
use crate::skills::{SkillMetaEntry, SkillSource};
use crate::sniff::SniffResult;
use crate::webdav::{WebDavConnection, WebDavConnectionRow};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn get_db_path() -> PathBuf {
    let dir = crate::config::app_dir().expect("Cannot determine AgentBuddy data directory");
    std::fs::create_dir_all(&dir).expect("Cannot create AgentBuddy data directory");
    dir.join("agents.db")
}

fn get_connection() -> Result<Connection, String> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Create schema if missing — never wipe existing rows on open
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            icon TEXT NOT NULL,
            install_paths TEXT NOT NULL DEFAULT '[]',
            config_dirs TEXT NOT NULL DEFAULT '[]',
            found INTEGER NOT NULL DEFAULT 0,
            scan_time INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS mcp_servers (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL UNIQUE,
            transport TEXT NOT NULL,
            command TEXT NOT NULL DEFAULT '',
            args TEXT NOT NULL DEFAULT '[]',
            env TEXT NOT NULL DEFAULT '{}',
            url TEXT NOT NULL DEFAULT '',
            headers TEXT NOT NULL DEFAULT '{}',
            applied_agents TEXT NOT NULL DEFAULT '[]',
            created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS webdav_connections (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            url TEXT NOT NULL,
            username TEXT NOT NULL,
            password_salt TEXT NOT NULL,
            password_nonce TEXT NOT NULL,
            password_cipher TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'disconnected',
            last_error TEXT NOT NULL DEFAULT '',
            last_checked_at INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS skills (
            id TEXT PRIMARY KEY,
            source TEXT NOT NULL DEFAULT 'local',
            repo_url TEXT NOT NULL DEFAULT '',
            github_owner TEXT NOT NULL DEFAULT '',
            github_repo TEXT NOT NULL DEFAULT '',
            github_path TEXT NOT NULL DEFAULT '',
            tag TEXT NOT NULL DEFAULT '',
            local_ref TEXT NOT NULL DEFAULT '',
            remote_ref TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS claude_environments (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            config_dir TEXT NOT NULL UNIQUE,
            alias_name TEXT NOT NULL UNIQUE,
            is_default INTEGER NOT NULL DEFAULT 0,
            source TEXT NOT NULL DEFAULT 'managed',
            notes TEXT NOT NULL DEFAULT '',
            alias_installed INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS codex_environments (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            config_dir TEXT NOT NULL UNIQUE,
            alias_name TEXT NOT NULL UNIQUE,
            is_default INTEGER NOT NULL DEFAULT 0,
            source TEXT NOT NULL DEFAULT 'managed',
            notes TEXT NOT NULL DEFAULT '',
            alias_installed INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ai_providers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            provider_type TEXT NOT NULL,
            base_url TEXT NOT NULL,
            api_key_salt TEXT NOT NULL DEFAULT '',
            api_key_nonce TEXT NOT NULL DEFAULT '',
            api_key_cipher TEXT NOT NULL DEFAULT '',
            default_model TEXT NOT NULL DEFAULT '',
            openai_default_model TEXT NOT NULL DEFAULT '',
            models_json TEXT NOT NULL DEFAULT '{}',
            notes TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS provider_route_toggle (
            provider_id TEXT NOT NULL,
            route_group TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            sort_order INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (provider_id, route_group)
         );
         CREATE TABLE IF NOT EXISTS agent_provider_bindings (
            ai_provider_id TEXT NOT NULL,
            agent_type TEXT NOT NULL,
            agent_provider_id TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (ai_provider_id, agent_type, agent_provider_id)
         );",
    )
    .map_err(|e| format!("Failed to create table: {}", e))?;

    // Additive migrations for DBs created before a column existed. `ALTER TABLE
    // ADD COLUMN` errors if the column is already present, so ignore that case.
    ensure_column(&conn, "skills", "tag", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "skills", "content_hash", "TEXT NOT NULL DEFAULT ''");
    ensure_column(
        &conn,
        "ai_providers",
        "openai_default_model",
        "TEXT NOT NULL DEFAULT ''",
    );
    ensure_column(
        &conn,
        "ai_providers",
        "sort_order",
        "INTEGER NOT NULL DEFAULT 0",
    );
    ensure_column(
        &conn,
        "ai_providers",
        "api_keys_json",
        "TEXT NOT NULL DEFAULT '[]'",
    );
    ensure_column(
        &conn,
        "ai_providers",
        "custom_models_json",
        "TEXT NOT NULL DEFAULT '[]'",
    );
    ensure_column(
        &conn,
        "ai_providers",
        "openai_base_url",
        "TEXT NOT NULL DEFAULT ''",
    );

    // Migration: 供应商类型从单选（anthropic/openai/universal）升级为多类型勾选
    // 集合（provider_type 列存 JSON 数组）。幂等：只精确匹配旧标量值，已迁移的行不受影响。
    //   anthropic → ["anthropic"]
    //   openai    → ["openai_chat","openai_responses"]，原 base_url（OpenAI URL）迁入
    //               openai_base_url、原 default_model（OpenAI 模型）迁入 openai_default_model
    //   universal → 三类型全勾，openai_base_url 取原派生值（base_url 去尾斜杠 + /v1）
    if let Err(e) = conn.execute_batch(
        "UPDATE ai_providers
            SET provider_type = '[\"anthropic\"]'
          WHERE provider_type = 'anthropic';
         UPDATE ai_providers
            SET provider_type = '[\"openai_chat\",\"openai_responses\"]',
                openai_base_url = base_url,
                base_url = '',
                openai_default_model = default_model,
                default_model = ''
          WHERE provider_type = 'openai';
         UPDATE ai_providers
            SET provider_type = '[\"anthropic\",\"openai_chat\",\"openai_responses\"]',
                openai_base_url = RTRIM(base_url, '/') || '/v1'
          WHERE provider_type = 'universal';",
    ) {
        eprintln!(
            "[agent-buddy] migrate ai_providers provider_types failed: {}",
            e
        );
    }
    ensure_column(
        &conn,
        "claude_environments",
        "provider_id",
        "TEXT NOT NULL DEFAULT ''",
    );
    ensure_column(
        &conn,
        "codex_environments",
        "provider_id",
        "TEXT NOT NULL DEFAULT ''",
    );

    // Migration: provider route toggles are now unified (no per-route-group
    // switch). Fold legacy 'claude_code'/'codex' rows into a single 'unified'
    // row per provider. Idempotent and cheap; runs on every open.
    if let Err(e) = conn.execute_batch(
        "DELETE FROM provider_route_toggle
            WHERE route_group <> 'unified'
              AND provider_id IN (SELECT provider_id FROM provider_route_toggle WHERE route_group = 'unified');
         UPDATE OR IGNORE provider_route_toggle SET route_group = 'unified' WHERE route_group <> 'unified';
         DELETE FROM provider_route_toggle WHERE route_group <> 'unified';",
    ) {
        eprintln!("[agent-buddy] unify provider_route_toggle failed: {}", e);
    }

    Ok(conn)
}

/// Add `column` to `table` if missing. Best-effort: a duplicate-column error
/// (column already there) is treated as success; other errors are logged.
fn ensure_column(conn: &Connection, table: &str, column: &str, decl: &str) {
    let sql = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, decl);
    if let Err(e) = conn.execute(&sql, []) {
        let msg = e.to_string();
        // rusqlite surfaces "duplicate column name" when the column already exists.
        if !msg.contains("duplicate column name") {
            eprintln!(
                "[agent-buddy] ensure_column {}.{} failed: {}",
                table, column, msg
            );
        }
    }
}

pub fn save_agents(agents: &[SniffResult]) -> Result<(), String> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    for agent in agents {
        let install_paths_json =
            serde_json::to_string(&agent.install_paths).unwrap_or_else(|_| "[]".to_string());
        let config_dirs_json =
            serde_json::to_string(&agent.config_dirs).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT INTO agents (name, display_name, icon, install_paths, config_dirs, found, scan_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(name) DO UPDATE SET
                display_name = excluded.display_name,
                icon = excluded.icon,
                install_paths = excluded.install_paths,
                config_dirs = excluded.config_dirs,
                found = excluded.found,
                scan_time = excluded.scan_time",
            params![
                agent.name,
                agent.display_name,
                agent.icon,
                install_paths_json,
                config_dirs_json,
                agent.found as i32,
                now,
            ],
        )
        .map_err(|e| format!("Failed to save agent {}: {}", agent.name, e))?;
    }

    Ok(())
}

/// 删除已从注册表移除的 agent 的历史缓存行（版本升级后的自愈）。
///
/// 背景：`save_agents` 只做 upsert、从不删除——旧版本扫描缓存（如 kiro /
/// deveco-code 等）在注册表移除对应 agent 后仍永远残留，导致 Agent 管理页
/// （get_cached_agents）继续展示已移除的 agent。启动时调用一次本函数清除。
/// 手动添加的同名 agent 会一并被删——与已移除的注册表项同名本就冲突，可接受。
pub fn purge_removed_agents(names: &[&str]) -> Result<usize, String> {
    if names.is_empty() {
        return Ok(0);
    }
    let conn = get_connection()?;
    let placeholders = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("DELETE FROM agents WHERE name IN ({placeholders})");
    conn.execute(&sql, rusqlite::params_from_iter(names.iter()))
        .map_err(|e| format!("Failed to purge removed agents: {e}"))
}

pub fn load_agents() -> Result<Vec<SniffResult>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT name, display_name, icon, install_paths, config_dirs, found FROM agents ORDER BY id",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let agents = stmt
        .query_map([], |row| {
            let install_paths_raw: String = row.get(3)?;
            let config_dirs_raw: String = row.get(4)?;

            let install_paths: Vec<String> =
                serde_json::from_str(&install_paths_raw).unwrap_or_default();
            let config_dirs: Vec<String> =
                serde_json::from_str(&config_dirs_raw).unwrap_or_default();

            Ok(SniffResult {
                name: row.get(0)?,
                display_name: row.get(1)?,
                icon: row.get(2)?,
                found: row.get::<_, i32>(5)? != 0,
                install_paths,
                config_dirs,
            })
        })
        .map_err(|e| format!("Failed to query agents: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read agents: {}", e))?;

    Ok(agents)
}

/* ===== MCP servers persistence ===== */

fn row_to_mcp(row: &rusqlite::Row<'_>) -> rusqlite::Result<McpServerRecord> {
    let args_raw: String = row.get(4)?;
    let env_raw: String = row.get(5)?;
    let headers_raw: String = row.get(7)?;
    let agents_raw: String = row.get(8)?;

    let args: Vec<String> = serde_json::from_str(&args_raw).unwrap_or_default();
    let env: HashMap<String, String> = serde_json::from_str(&env_raw).unwrap_or_default();
    let headers: HashMap<String, String> = serde_json::from_str(&headers_raw).unwrap_or_default();
    let applied_agents: Vec<String> = serde_json::from_str(&agents_raw).unwrap_or_default();

    Ok(McpServerRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        transport: row.get(2)?,
        command: row.get(3)?,
        args,
        env,
        url: row.get(6)?,
        headers,
        applied_agents,
        created_at: row.get(9)?,
    })
}

pub fn load_mcp_servers() -> Result<Vec<McpServerRecord>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, transport, command, args, env, url, headers, applied_agents, created_at
             FROM mcp_servers
             ORDER BY created_at DESC",
        )
        .map_err(|e| format!("Failed to prepare mcp query: {}", e))?;

    let servers = stmt
        .query_map([], row_to_mcp)
        .map_err(|e| format!("Failed to query mcp: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read mcp: {}", e))?;

    Ok(servers)
}

#[allow(dead_code)]
pub fn save_mcp_servers(servers: &[McpServerRecord]) -> Result<(), String> {
    let conn = get_connection()?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to begin transaction: {}", e))?;

    // Full replace of known rows by title upsert; keep simple: upsert each
    for s in servers {
        let args = serde_json::to_string(&s.args).unwrap_or_else(|_| "[]".into());
        let env = serde_json::to_string(&s.env).unwrap_or_else(|_| "{}".into());
        let headers = serde_json::to_string(&s.headers).unwrap_or_else(|_| "{}".into());
        let agents = serde_json::to_string(&s.applied_agents).unwrap_or_else(|_| "[]".into());

        tx.execute(
            "INSERT INTO mcp_servers
                (id, title, transport, command, args, env, url, headers, applied_agents, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(title) DO UPDATE SET
                id = excluded.id,
                transport = excluded.transport,
                command = excluded.command,
                args = excluded.args,
                env = excluded.env,
                url = excluded.url,
                headers = excluded.headers,
                applied_agents = excluded.applied_agents,
                created_at = mcp_servers.created_at",
            params![
                s.id,
                s.title,
                s.transport,
                s.command,
                args,
                env,
                s.url,
                headers,
                agents,
                s.created_at,
            ],
        )
        .map_err(|e| format!("Failed to save mcp {}: {}", s.title, e))?;
    }

    tx.commit()
        .map_err(|e| format!("Failed to commit mcp save: {}", e))?;
    Ok(())
}

/// Replace entire MCP table with the given list (used after UI edit/delete).
pub fn replace_mcp_servers(servers: &[McpServerRecord]) -> Result<(), String> {
    let conn = get_connection()?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to begin transaction: {}", e))?;

    tx.execute("DELETE FROM mcp_servers", [])
        .map_err(|e| format!("Failed to clear mcp table: {}", e))?;

    for s in servers {
        let args = serde_json::to_string(&s.args).unwrap_or_else(|_| "[]".into());
        let env = serde_json::to_string(&s.env).unwrap_or_else(|_| "{}".into());
        let headers = serde_json::to_string(&s.headers).unwrap_or_else(|_| "{}".into());
        let agents = serde_json::to_string(&s.applied_agents).unwrap_or_else(|_| "[]".into());

        tx.execute(
            "INSERT INTO mcp_servers
                (id, title, transport, command, args, env, url, headers, applied_agents, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                s.id,
                s.title,
                s.transport,
                s.command,
                args,
                env,
                s.url,
                headers,
                agents,
                s.created_at,
            ],
        )
        .map_err(|e| format!("Failed to insert mcp {}: {}", s.title, e))?;
    }

    tx.commit()
        .map_err(|e| format!("Failed to commit mcp replace: {}", e))?;
    Ok(())
}

pub fn delete_mcp_server(id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute("DELETE FROM mcp_servers WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete mcp: {}", e))?;
    Ok(())
}

/// 从所有 `mcp_servers.applied_agents`（JSON 数组）中剔除已下线的 agent 名，
/// 供版本升级清理使用。返回被改写的行数。
pub fn scrub_mcp_applied_agents(remove: &[&str]) -> Result<usize, String> {
    if remove.is_empty() {
        return Ok(0);
    }
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare("SELECT id, applied_agents FROM mcp_servers")
        .map_err(|e| format!("Failed to prepare mcp scrub: {}", e))?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| format!("Failed to query mcp servers: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect mcp servers: {}", e))?;

    let mut changed = 0usize;
    for (id, raw) in rows {
        let agents: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
        let kept: Vec<String> = agents
            .iter()
            .filter(|a| !remove.contains(&a.as_str()))
            .cloned()
            .collect();
        if kept.len() != agents.len() {
            let new_raw = serde_json::to_string(&kept).unwrap_or_else(|_| "[]".into());
            conn.execute(
                "UPDATE mcp_servers SET applied_agents = ?1 WHERE id = ?2",
                params![new_raw, id],
            )
            .map_err(|e| format!("Failed to update mcp applied_agents: {}", e))?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// 删除指定 `agent_type` 的全局供应商绑定行，供版本升级清理使用。
pub fn delete_agent_provider_bindings_by_agent_type(agent_types: &[&str]) -> Result<usize, String> {
    if agent_types.is_empty() {
        return Ok(0);
    }
    let conn = get_connection()?;
    let placeholders = agent_types
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("DELETE FROM agent_provider_bindings WHERE agent_type IN ({placeholders})");
    conn.execute(&sql, rusqlite::params_from_iter(agent_types.iter()))
        .map_err(|e| format!("Failed to delete agent provider bindings: {}", e))
}

/* ===== WebDAV connections persistence ===== */

fn row_to_webdav(row: &rusqlite::Row<'_>) -> rusqlite::Result<WebDavConnectionRow> {
    Ok(WebDavConnectionRow {
        id: row.get(0)?,
        name: row.get(1)?,
        url: row.get(2)?,
        username: row.get(3)?,
        password_salt: row.get(4)?,
        password_nonce: row.get(5)?,
        password_cipher: row.get(6)?,
        status: row.get(7)?,
        last_error: row.get(8)?,
        last_checked_at: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

/// Public list view — never includes password material.
pub fn load_webdav_connections() -> Result<Vec<WebDavConnection>, String> {
    let rows = load_webdav_connection_rows()?;
    Ok(rows.into_iter().map(WebDavConnection::from).collect())
}

pub fn load_webdav_connection_rows() -> Result<Vec<WebDavConnectionRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, url, username, password_salt, password_nonce, password_cipher,
                    status, last_error, last_checked_at, created_at, updated_at
             FROM webdav_connections
             ORDER BY created_at DESC",
        )
        .map_err(|e| format!("Failed to prepare webdav query: {}", e))?;

    let rows = stmt
        .query_map([], row_to_webdav)
        .map_err(|e| format!("Failed to query webdav: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read webdav: {}", e))?;

    Ok(rows)
}

pub fn get_webdav_connection_row(id: &str) -> Result<Option<WebDavConnectionRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, url, username, password_salt, password_nonce, password_cipher,
                    status, last_error, last_checked_at, created_at, updated_at
             FROM webdav_connections
             WHERE id = ?1",
        )
        .map_err(|e| format!("Failed to prepare webdav get: {}", e))?;

    stmt.query_row(params![id], row_to_webdav)
        .optional()
        .map_err(|e| format!("Failed to get webdav {}: {}", id, e))
}

pub fn upsert_webdav_connection_row(row: &WebDavConnectionRow) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT INTO webdav_connections
            (id, name, url, username, password_salt, password_nonce, password_cipher,
             status, last_error, last_checked_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            url = excluded.url,
            username = excluded.username,
            password_salt = excluded.password_salt,
            password_nonce = excluded.password_nonce,
            password_cipher = excluded.password_cipher,
            status = excluded.status,
            last_error = excluded.last_error,
            last_checked_at = excluded.last_checked_at,
            created_at = webdav_connections.created_at,
            updated_at = excluded.updated_at",
        params![
            row.id,
            row.name,
            row.url,
            row.username,
            row.password_salt,
            row.password_nonce,
            row.password_cipher,
            row.status,
            row.last_error,
            row.last_checked_at,
            row.created_at,
            row.updated_at,
        ],
    )
    .map_err(|e| format!("Failed to upsert webdav {}: {}", row.id, e))?;
    Ok(())
}

pub fn delete_webdav_connection(id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute("DELETE FROM webdav_connections WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete webdav: {}", e))?;
    Ok(())
}

pub fn update_webdav_status(
    id: &str,
    status: &str,
    last_error: &str,
    last_checked_at: i64,
) -> Result<(), String> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let changed = conn
        .execute(
            "UPDATE webdav_connections
             SET status = ?1, last_error = ?2, last_checked_at = ?3, updated_at = ?4
             WHERE id = ?5",
            params![status, last_error, last_checked_at, now, id],
        )
        .map_err(|e| format!("Failed to update webdav status: {}", e))?;
    if changed == 0 {
        return Err(format!("WebDAV connection not found: {}", id));
    }
    Ok(())
}

/* ===== Skills source metadata ===== */

fn skill_source_to_str(source: &SkillSource) -> &'static str {
    match source {
        SkillSource::Local => "local",
        SkillSource::Github => "github",
        SkillSource::Gitcode => "gitcode",
    }
}

fn skill_source_from_str(raw: &str) -> SkillSource {
    match raw {
        "github" => SkillSource::Github,
        "gitcode" => SkillSource::Gitcode,
        _ => SkillSource::Local,
    }
}

fn row_to_skill_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, SkillMetaEntry)> {
    let id: String = row.get(0)?;
    let source_raw: String = row.get(1)?;
    Ok((
        id,
        SkillMetaEntry {
            source: skill_source_from_str(&source_raw),
            repo_url: row.get(2)?,
            github_owner: row.get(3)?,
            github_repo: row.get(4)?,
            github_path: row.get(5)?,
            tag: row.get(6)?,
            local_ref: row.get(7)?,
            remote_ref: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
            content_hash: row.get(11)?,
        },
    ))
}

/// Load all skill metadata rows keyed by skill id (directory name).
pub fn load_skill_meta_map() -> Result<HashMap<String, SkillMetaEntry>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, source, repo_url, github_owner, github_repo, github_path,
                    tag, local_ref, remote_ref, created_at, updated_at, content_hash
             FROM skills",
        )
        .map_err(|e| format!("Failed to prepare skills query: {}", e))?;

    let rows = stmt
        .query_map([], row_to_skill_meta)
        .map_err(|e| format!("Failed to query skills: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read skills: {}", e))?;

    Ok(rows.into_iter().collect())
}

pub fn get_skill_meta(id: &str) -> Result<Option<SkillMetaEntry>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, source, repo_url, github_owner, github_repo, github_path,
                    tag, local_ref, remote_ref, created_at, updated_at, content_hash
             FROM skills
             WHERE id = ?1",
        )
        .map_err(|e| format!("Failed to prepare skill get: {}", e))?;

    stmt.query_row(params![id], |row| row_to_skill_meta(row).map(|(_, m)| m))
        .optional()
        .map_err(|e| format!("Failed to get skill {}: {}", id, e))
}

/// Insert or update a skill metadata row. Preserves created_at on conflict.
pub fn upsert_skill_meta(id: &str, entry: &SkillMetaEntry) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT INTO skills
            (id, source, repo_url, github_owner, github_repo, github_path,
             tag, local_ref, remote_ref, created_at, updated_at, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(id) DO UPDATE SET
            source = excluded.source,
            repo_url = excluded.repo_url,
            github_owner = excluded.github_owner,
            github_repo = excluded.github_repo,
            github_path = excluded.github_path,
            tag = excluded.tag,
            local_ref = excluded.local_ref,
            remote_ref = excluded.remote_ref,
            created_at = skills.created_at,
            updated_at = excluded.updated_at,
            content_hash = excluded.content_hash",
        params![
            id,
            skill_source_to_str(&entry.source),
            entry.repo_url,
            entry.github_owner,
            entry.github_repo,
            entry.github_path,
            entry.tag,
            entry.local_ref,
            entry.remote_ref,
            entry.created_at,
            entry.updated_at,
            entry.content_hash,
        ],
    )
    .map_err(|e| format!("Failed to upsert skill {}: {}", id, e))?;
    Ok(())
}

pub fn update_skill_refs(
    id: &str,
    local_ref: Option<&str>,
    remote_ref: Option<&str>,
    updated_at: i64,
) -> Result<(), String> {
    let mut entry = get_skill_meta(id)?.ok_or_else(|| format!("Skill not found: {}", id))?;
    if let Some(v) = local_ref {
        entry.local_ref = v.to_string();
    }
    if let Some(v) = remote_ref {
        entry.remote_ref = v.to_string();
    }
    entry.updated_at = updated_at;
    upsert_skill_meta(id, &entry)
}

#[allow(dead_code)]
pub fn delete_skill_meta(id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute("DELETE FROM skills WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete skill {}: {}", id, e))?;
    Ok(())
}

/* ===== Claude environments persistence ===== */

fn row_to_claude_env(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaudeEnvironmentRow> {
    Ok(ClaudeEnvironmentRow {
        id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        config_dir: row.get(3)?,
        alias_name: row.get(4)?,
        is_default: row.get::<_, i32>(5)? != 0,
        source: row.get(6)?,
        notes: row.get(7)?,
        alias_installed: row.get::<_, i32>(8)? != 0,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        provider_id: row.get(11)?,
    })
}

pub fn load_claude_environment_rows() -> Result<Vec<ClaudeEnvironmentRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            // 默认环境置顶；其余按创建时间升序（最早在上，最新创建在最下），排序稳定不随编辑浮动。
            "SELECT id, name, slug, config_dir, alias_name, is_default, source, notes,
                    alias_installed, created_at, updated_at, provider_id
             FROM claude_environments
             ORDER BY is_default DESC, created_at ASC",
        )
        .map_err(|e| format!("Failed to prepare claude_env query: {}", e))?;

    let rows = stmt
        .query_map([], row_to_claude_env)
        .map_err(|e| format!("Failed to query claude_env: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read claude_env: {}", e))?;

    Ok(rows)
}

pub fn get_claude_environment_row(id: &str) -> Result<Option<ClaudeEnvironmentRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, config_dir, alias_name, is_default, source, notes,
                    alias_installed, created_at, updated_at, provider_id
             FROM claude_environments
             WHERE id = ?1",
        )
        .map_err(|e| format!("Failed to prepare claude_env get: {}", e))?;

    stmt.query_row(params![id], row_to_claude_env)
        .optional()
        .map_err(|e| format!("Failed to get claude_env {}: {}", id, e))
}

pub fn upsert_claude_environment_row(row: &ClaudeEnvironmentRow) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT INTO claude_environments
            (id, name, slug, config_dir, alias_name, is_default, source, notes,
             alias_installed, created_at, updated_at, provider_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            slug = excluded.slug,
            config_dir = excluded.config_dir,
            alias_name = excluded.alias_name,
            is_default = excluded.is_default,
            source = excluded.source,
            notes = excluded.notes,
            alias_installed = excluded.alias_installed,
            created_at = claude_environments.created_at,
            updated_at = excluded.updated_at,
            provider_id = excluded.provider_id",
        params![
            row.id,
            row.name,
            row.slug,
            row.config_dir,
            row.alias_name,
            row.is_default as i32,
            row.source,
            row.notes,
            row.alias_installed as i32,
            row.created_at,
            row.updated_at,
            row.provider_id,
        ],
    )
    .map_err(|e| format!("Failed to upsert claude_env {}: {}", row.id, e))?;
    Ok(())
}

pub fn delete_claude_environment_row(id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute("DELETE FROM claude_environments WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete claude_env {}: {}", id, e))?;
    Ok(())
}

pub fn set_claude_env_alias_installed(id: &str, installed: bool) -> Result<(), String> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let changed = conn
        .execute(
            "UPDATE claude_environments
             SET alias_installed = ?1, updated_at = ?2
             WHERE id = ?3 AND is_default = 0",
            params![installed as i32, now, id],
        )
        .map_err(|e| format!("Failed to update claude_env alias flag: {}", e))?;
    if changed == 0 {
        return Err(format!("环境不存在或为默认环境: {}", id));
    }
    Ok(())
}

pub fn set_claude_env_alias_installed_all(installed: bool) -> Result<(), String> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute(
        "UPDATE claude_environments
         SET alias_installed = ?1, updated_at = ?2
         WHERE is_default = 0",
        params![installed as i32, now],
    )
    .map_err(|e| format!("Failed to update claude_env alias flags: {}", e))?;
    Ok(())
}

/* ===== Codex environments ===== */

fn row_to_codex_env(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodexEnvironmentRow> {
    Ok(CodexEnvironmentRow {
        id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        config_dir: row.get(3)?,
        alias_name: row.get(4)?,
        is_default: row.get::<_, i32>(5)? != 0,
        source: row.get(6)?,
        notes: row.get(7)?,
        alias_installed: row.get::<_, i32>(8)? != 0,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        provider_id: row.get(11)?,
    })
}

pub fn load_codex_environment_rows() -> Result<Vec<CodexEnvironmentRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            // 默认环境置顶；其余按创建时间升序（最早在上，最新创建在最下），排序稳定不随编辑浮动。
            "SELECT id, name, slug, config_dir, alias_name, is_default, source, notes,
                    alias_installed, created_at, updated_at, provider_id
             FROM codex_environments
             ORDER BY is_default DESC, created_at ASC",
        )
        .map_err(|e| format!("Failed to prepare codex_env query: {}", e))?;

    let rows = stmt
        .query_map([], row_to_codex_env)
        .map_err(|e| format!("Failed to query codex_env: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read codex_env: {}", e))?;

    Ok(rows)
}

pub fn get_codex_environment_row(id: &str) -> Result<Option<CodexEnvironmentRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, config_dir, alias_name, is_default, source, notes,
                    alias_installed, created_at, updated_at, provider_id
             FROM codex_environments
             WHERE id = ?1",
        )
        .map_err(|e| format!("Failed to prepare codex_env get: {}", e))?;

    stmt.query_row(params![id], row_to_codex_env)
        .optional()
        .map_err(|e| format!("Failed to get codex_env {}: {}", id, e))
}

pub fn upsert_codex_environment_row(row: &CodexEnvironmentRow) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT INTO codex_environments
            (id, name, slug, config_dir, alias_name, is_default, source, notes,
             alias_installed, created_at, updated_at, provider_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            slug = excluded.slug,
            config_dir = excluded.config_dir,
            alias_name = excluded.alias_name,
            is_default = excluded.is_default,
            source = excluded.source,
            notes = excluded.notes,
            alias_installed = excluded.alias_installed,
            created_at = codex_environments.created_at,
            updated_at = excluded.updated_at,
            provider_id = excluded.provider_id",
        params![
            row.id,
            row.name,
            row.slug,
            row.config_dir,
            row.alias_name,
            row.is_default as i32,
            row.source,
            row.notes,
            row.alias_installed as i32,
            row.created_at,
            row.updated_at,
            row.provider_id,
        ],
    )
    .map_err(|e| format!("Failed to upsert codex_env {}: {}", row.id, e))?;
    Ok(())
}

pub fn delete_codex_environment_row(id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute("DELETE FROM codex_environments WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete codex_env {}: {}", id, e))?;
    Ok(())
}

pub fn set_codex_env_alias_installed(id: &str, installed: bool) -> Result<(), String> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let changed = conn
        .execute(
            "UPDATE codex_environments
             SET alias_installed = ?1, updated_at = ?2
             WHERE id = ?3 AND is_default = 0",
            params![installed as i32, now, id],
        )
        .map_err(|e| format!("Failed to update codex_env alias flag: {}", e))?;
    if changed == 0 {
        return Err(format!("环境不存在或为默认环境: {}", id));
    }
    Ok(())
}

pub fn set_codex_env_alias_installed_all(installed: bool) -> Result<(), String> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute(
        "UPDATE codex_environments
         SET alias_installed = ?1, updated_at = ?2
         WHERE is_default = 0",
        params![installed as i32, now],
    )
    .map_err(|e| format!("Failed to update codex_env alias flags: {}", e))?;
    Ok(())
}

/// Find all Claude environments linked to the given provider_id, **including the
/// default environment**.
///
/// The default environment may legitimately be bound to a provider (the env
/// upsert path and the UI selector both allow it), so excluding it here would
/// make provider edits silently skip it.
pub fn load_claude_envs_by_provider(
    provider_id: &str,
) -> Result<Vec<ClaudeEnvironmentRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, config_dir, alias_name, is_default, source, notes,
                    alias_installed, created_at, updated_at, provider_id
             FROM claude_environments
             WHERE provider_id = ?1",
        )
        .map_err(|e| format!("Failed to prepare claude_env by provider query: {}", e))?;
    let rows = stmt
        .query_map(params![provider_id], row_to_claude_env)
        .map_err(|e| format!("Failed to query claude_env by provider: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read claude_env by provider: {}", e))?;
    Ok(rows)
}

/// Find all Codex environments linked to the given provider_id, **including the
/// default environment** (see [`load_claude_envs_by_provider`] for the rationale).
pub fn load_codex_envs_by_provider(provider_id: &str) -> Result<Vec<CodexEnvironmentRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, config_dir, alias_name, is_default, source, notes,
                    alias_installed, created_at, updated_at, provider_id
             FROM codex_environments
             WHERE provider_id = ?1",
        )
        .map_err(|e| format!("Failed to prepare codex_env by provider query: {}", e))?;
    let rows = stmt
        .query_map(params![provider_id], row_to_codex_env)
        .map_err(|e| format!("Failed to query codex_env by provider: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read codex_env by provider: {}", e))?;
    Ok(rows)
}

/// Clear provider_id on all environments linked to the given provider (used on provider delete).
///
/// Deliberately has **no** `is_default` filter: deleting a provider must not leave
/// a dangling `provider_id` on the default environment either. Locked by
/// `envs_by_provider_include_default_and_clear_covers_default`.
pub fn clear_provider_id_on_envs(provider_id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute(
        "UPDATE claude_environments SET provider_id = '', updated_at = ?1 WHERE provider_id = ?2",
        params![now, provider_id],
    )
    .map_err(|e| format!("Failed to clear claude_env provider_id: {}", e))?;
    conn.execute(
        "UPDATE codex_environments SET provider_id = '', updated_at = ?1 WHERE provider_id = ?2",
        params![now, provider_id],
    )
    .map_err(|e| format!("Failed to clear codex_env provider_id: {}", e))?;
    Ok(())
}

/* ===== AI providers persistence ===== */

fn row_to_ai_provider(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiProviderRow> {
    Ok(AiProviderRow {
        id: row.get(0)?,
        name: row.get(1)?,
        provider_type: row.get(2)?,
        base_url: row.get(3)?,
        api_key_salt: row.get(4)?,
        api_key_nonce: row.get(5)?,
        api_key_cipher: row.get(6)?,
        default_model: row.get(7)?,
        openai_default_model: row.get(8)?,
        models_json: row.get(9)?,
        notes: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        sort_order: row.get(13)?,
        api_keys_json: row.get(14)?,
        custom_models_json: row.get(15)?,
        openai_base_url: row.get(16)?,
    })
}

pub fn load_ai_provider_rows() -> Result<Vec<AiProviderRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, provider_type, base_url, api_key_salt, api_key_nonce,
                    api_key_cipher, default_model, openai_default_model, models_json,
                    notes, created_at, updated_at, sort_order, api_keys_json, custom_models_json,
                    openai_base_url
             FROM ai_providers
             ORDER BY sort_order ASC, created_at ASC",
        )
        .map_err(|e| format!("Failed to prepare ai_providers query: {}", e))?;

    let rows = stmt
        .query_map([], row_to_ai_provider)
        .map_err(|e| format!("Failed to query ai_providers: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read ai_providers: {}", e))?;

    Ok(rows)
}

pub fn get_ai_provider_row(id: &str) -> Result<Option<AiProviderRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, provider_type, base_url, api_key_salt, api_key_nonce,
                    api_key_cipher, default_model, openai_default_model, models_json,
                    notes, created_at, updated_at, sort_order, api_keys_json, custom_models_json,
                    openai_base_url
             FROM ai_providers
             WHERE id = ?1",
        )
        .map_err(|e| format!("Failed to prepare ai_provider get: {}", e))?;

    stmt.query_row(params![id], row_to_ai_provider)
        .optional()
        .map_err(|e| format!("Failed to get ai_provider {}: {}", id, e))
}

pub fn upsert_ai_provider_row(row: &AiProviderRow) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT INTO ai_providers
            (id, name, provider_type, base_url, api_key_salt, api_key_nonce,
             api_key_cipher, default_model, openai_default_model, models_json,
             notes, created_at, updated_at, sort_order, api_keys_json, custom_models_json,
             openai_base_url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            provider_type = excluded.provider_type,
            base_url = excluded.base_url,
            api_key_salt = excluded.api_key_salt,
            api_key_nonce = excluded.api_key_nonce,
            api_key_cipher = excluded.api_key_cipher,
            default_model = excluded.default_model,
            openai_default_model = excluded.openai_default_model,
            models_json = excluded.models_json,
            notes = excluded.notes,
            created_at = ai_providers.created_at,
            updated_at = excluded.updated_at,
            sort_order = excluded.sort_order,
            api_keys_json = excluded.api_keys_json,
            custom_models_json = excluded.custom_models_json,
            openai_base_url = excluded.openai_base_url",
        params![
            row.id,
            row.name,
            row.provider_type,
            row.base_url,
            row.api_key_salt,
            row.api_key_nonce,
            row.api_key_cipher,
            row.default_model,
            row.openai_default_model,
            row.models_json,
            row.notes,
            row.created_at,
            row.updated_at,
            row.sort_order,
            row.api_keys_json,
            row.custom_models_json,
            row.openai_base_url,
        ],
    )
    .map_err(|e| format!("Failed to upsert ai_provider {}: {}", row.id, e))?;
    Ok(())
}

pub fn delete_ai_provider_row(id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute("DELETE FROM ai_providers WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete ai_provider {}: {}", id, e))?;
    // Cascade: remove all agent bindings for this provider.
    conn.execute(
        "DELETE FROM agent_provider_bindings WHERE ai_provider_id = ?1",
        params![id],
    )
    .map_err(|e| format!("Failed to delete agent bindings for {}: {}", id, e))?;
    Ok(())
}

/// Batch-update sort_order for ai_providers. `orders` is a list of (id, sort_order).
pub fn reorder_ai_provider_rows(orders: &[(String, i64)]) -> Result<(), String> {
    let conn = get_connection()?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to begin transaction: {}", e))?;
    for (id, order) in orders {
        tx.execute(
            "UPDATE ai_providers SET sort_order = ?1 WHERE id = ?2",
            params![order, id],
        )
        .map_err(|e| format!("Failed to reorder ai_provider {}: {}", id, e))?;
    }
    tx.commit()
        .map_err(|e| format!("Failed to commit reorder: {}", e))?;
    Ok(())
}

/* ===== Provider route toggles (route aggregation) ===== */

/// Load all unified provider toggle rows.
pub fn load_provider_route_toggles(
) -> Result<Vec<crate::route_aggregation::ProviderRouteToggle>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT provider_id, enabled, sort_order
             FROM provider_route_toggle
             ORDER BY sort_order ASC",
        )
        .map_err(|e| format!("Failed to prepare route_toggle query: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(crate::route_aggregation::ProviderRouteToggle {
                provider_id: row.get(0)?,
                enabled: row.get::<_, i32>(1)? != 0,
                sort_order: row.get(2)?,
            })
        })
        .map_err(|e| format!("Failed to query route_toggle: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read route_toggle: {}", e))?;

    Ok(rows)
}

/// Upsert a provider's unified toggle.
pub fn upsert_provider_route_toggle(
    provider_id: &str,
    enabled: bool,
    sort_order: i32,
) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT INTO provider_route_toggle (provider_id, route_group, enabled, sort_order)
         VALUES (?1, 'unified', ?2, ?3)
         ON CONFLICT(provider_id, route_group) DO UPDATE SET
            enabled = excluded.enabled,
            sort_order = excluded.sort_order",
        params![provider_id, enabled as i32, sort_order],
    )
    .map_err(|e| format!("Failed to upsert route_toggle for {}: {}", provider_id, e))?;
    Ok(())
}

/// Delete toggle rows for a provider that no longer exists.
#[allow(dead_code)]
pub fn delete_provider_route_toggles(provider_id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "DELETE FROM provider_route_toggle WHERE provider_id = ?1",
        params![provider_id],
    )
    .map_err(|e| format!("Failed to delete route_toggle for {}: {}", provider_id, e))?;
    Ok(())
}

/* ===== Agent provider bindings (全局供应商 → opencode/pi provider) ===== */

/// A binding: one global ai_provider is linked to one provider entry inside
/// another agent's config file (opencode / pi).
#[derive(Debug, Clone)]
pub struct AgentProviderBinding {
    /// Kept for completeness / future "list all bindings" views; current
    /// loaders always filter by ai_provider_id so callers already know it.
    #[allow(dead_code)]
    pub ai_provider_id: String,
    pub agent_type: String,
    pub agent_provider_id: String,
}

/// Upsert a binding. Idempotent: re-inserting the same triple is a no-op.
pub fn upsert_agent_provider_binding(
    ai_provider_id: &str,
    agent_type: &str,
    agent_provider_id: &str,
) -> Result<(), String> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Clock error: {}", e))?
        .as_secs() as i64;
    conn.execute(
        "INSERT INTO agent_provider_bindings
            (ai_provider_id, agent_type, agent_provider_id, created_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(ai_provider_id, agent_type, agent_provider_id) DO NOTHING",
        params![ai_provider_id, agent_type, agent_provider_id, now],
    )
    .map_err(|e| format!("Failed to upsert agent binding: {}", e))?;
    Ok(())
}

/// Remove a single binding.
pub fn delete_agent_provider_binding(
    ai_provider_id: &str,
    agent_type: &str,
    agent_provider_id: &str,
) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "DELETE FROM agent_provider_bindings
         WHERE ai_provider_id = ?1 AND agent_type = ?2 AND agent_provider_id = ?3",
        params![ai_provider_id, agent_type, agent_provider_id],
    )
    .map_err(|e| format!("Failed to delete agent binding: {}", e))?;
    Ok(())
}

/// List all bindings for a given global ai_provider.
pub fn load_agent_provider_bindings(
    ai_provider_id: &str,
) -> Result<Vec<AgentProviderBinding>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT ai_provider_id, agent_type, agent_provider_id
             FROM agent_provider_bindings
             WHERE ai_provider_id = ?1",
        )
        .map_err(|e| format!("Failed to prepare binding query: {}", e))?;
    let rows = stmt
        .query_map([ai_provider_id], |row| {
            Ok(AgentProviderBinding {
                ai_provider_id: row.get(0)?,
                agent_type: row.get(1)?,
                agent_provider_id: row.get(2)?,
            })
        })
        .map_err(|e| format!("Failed to query bindings: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read bindings: {}", e))?;
    Ok(rows)
}

/// One-time migration from legacy `skills-meta.json` into SQLite.
/// Safe to call repeatedly; no-ops when file missing or already empty.
pub fn migrate_skills_meta_json_if_present() -> Result<(), String> {
    // Prefer current app data dir; also check legacy ~/.agentbuddy for older installs.
    let mut candidates = Vec::new();
    if let Ok(dir) = crate::config::app_dir() {
        candidates.push(dir.join("skills-meta.json"));
    }
    if let Ok(legacy) = crate::platform::legacy_app_data_dir() {
        let p = legacy.join("skills-meta.json");
        if !candidates.iter().any(|c| c == &p) {
            candidates.push(p);
        }
    }
    let path = match candidates.into_iter().find(|p| p.exists()) {
        Some(p) => p,
        None => return Ok(()),
    };

    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    if raw.trim().is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }

    #[derive(serde::Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct LegacyFile {
        #[serde(default)]
        skills: HashMap<String, SkillMetaEntry>,
    }

    let legacy: LegacyFile = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "[agent-buddy] skills-meta.json parse failed, leaving file in place: {}",
                e
            );
            return Ok(());
        }
    };

    for (id, entry) in legacy.skills {
        // Prefer existing DB row if already present (do not overwrite).
        if get_skill_meta(&id)?.is_some() {
            continue;
        }
        upsert_skill_meta(&id, &entry)?;
    }

    // Rename rather than hard-delete so accidental re-run can be recovered.
    let bak = path.with_extension("json.bak");
    if let Err(e) = std::fs::rename(&path, &bak) {
        // If rename fails, still try delete so we don't loop forever.
        let _ = std::fs::remove_file(&path);
        eprintln!(
            "[agent-buddy] failed to archive skills-meta.json ({}): {}",
            bak.display(),
            e
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::SkillSource;

    /// 与既有 db / ai_provider 测试同款：持 HOME 锁 + 唯一 id + 测试结束（含 panic）清理，
    /// **不交换 HOME**。交换 HOME 会与未持锁的 HOME 读取者（如 agent_open）产生竞态，
    /// 使整套测试间歇性失败（该竞态在本次修改前已存在）。
    fn unique_tag(tag: &str) -> String {
        format!(
            "__agentbuddy_test_{}_{}",
            tag,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    /// Drop 时删除本测试插入的环境行（panic 展开也会执行），
    /// 避免在真实 agents.db 里留下垃圾行。
    #[derive(Default)]
    struct EnvRowsGuard {
        claude: Vec<String>,
        codex: Vec<String>,
    }

    impl Drop for EnvRowsGuard {
        fn drop(&mut self) {
            for id in &self.claude {
                let _ = delete_claude_environment_row(id);
            }
            for id in &self.codex {
                let _ = delete_codex_environment_row(id);
            }
        }
    }

    fn claude_row(
        id: &str,
        config_dir: &std::path::Path,
        is_default: bool,
        provider_id: &str,
    ) -> ClaudeEnvironmentRow {
        ClaudeEnvironmentRow {
            id: id.into(),
            name: format!("claude-{}", id),
            slug: id.into(),
            config_dir: config_dir.to_string_lossy().to_string(),
            alias_name: format!("claude-{}", id),
            is_default,
            source: "managed".into(),
            notes: String::new(),
            alias_installed: false,
            provider_id: provider_id.into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn codex_row(
        id: &str,
        config_dir: &std::path::Path,
        is_default: bool,
        provider_id: &str,
    ) -> CodexEnvironmentRow {
        CodexEnvironmentRow {
            id: id.into(),
            name: format!("codex-{}", id),
            slug: id.into(),
            config_dir: config_dir.to_string_lossy().to_string(),
            alias_name: format!("codex-{}", id),
            is_default,
            source: "managed".into(),
            notes: String::new(),
            alias_installed: false,
            provider_id: provider_id.into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    /// 回归：默认环境同样可以绑定供应商，按供应商查找环境时必须包含它；
    /// 同时锁定 `clear_provider_id_on_envs` 不带 `is_default` 过滤（删除供应商后
    /// 默认环境不得留下悬挂 provider_id）。
    #[test]
    fn envs_by_provider_include_default_and_clear_covers_default() {
        let _home_guard = crate::config::lock_home_for_test();
        let tag = unique_tag("envs_by_provider");
        let tmp = std::env::temp_dir();
        let mut guard = EnvRowsGuard::default();

        let prov = format!("{}-prov", tag);
        let claude_default_id = format!("{}-claude-default", tag);
        let claude_work_id = format!("{}-claude-work", tag);
        let claude_other_id = format!("{}-claude-other", tag);
        let codex_default_id = format!("{}-codex-default", tag);
        let codex_work_id = format!("{}-codex-work", tag);

        upsert_claude_environment_row(&claude_row(
            &claude_default_id,
            &tmp.join(format!("{}-claude-default", tag)),
            true,
            &prov,
        ))
        .unwrap();
        guard.claude.push(claude_default_id.clone());
        upsert_claude_environment_row(&claude_row(
            &claude_work_id,
            &tmp.join(format!("{}-claude-work", tag)),
            false,
            &prov,
        ))
        .unwrap();
        guard.claude.push(claude_work_id.clone());
        upsert_claude_environment_row(&claude_row(
            &claude_other_id,
            &tmp.join(format!("{}-claude-other", tag)),
            false,
            &format!("{}-other", tag),
        ))
        .unwrap();
        guard.claude.push(claude_other_id.clone());

        // 按供应商查找必须同时命中默认环境与非默认环境，且不牵连其它供应商。
        let claude = load_claude_envs_by_provider(&prov).unwrap();
        let ids: Vec<_> = claude.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids.len(), 2, "默认环境与非默认环境都应命中: {:?}", ids);
        assert!(
            claude.iter().any(|r| r.is_default),
            "默认环境被排除: {:?}",
            ids
        );
        assert!(
            claude.iter().any(|r| !r.is_default),
            "非默认环境缺失: {:?}",
            ids
        );
        assert!(
            !claude.iter().any(|r| r.id == claude_other_id),
            "其它供应商的环境不应被牵连"
        );

        upsert_codex_environment_row(&codex_row(
            &codex_default_id,
            &tmp.join(format!("{}-codex-default", tag)),
            true,
            &prov,
        ))
        .unwrap();
        guard.codex.push(codex_default_id.clone());
        upsert_codex_environment_row(&codex_row(
            &codex_work_id,
            &tmp.join(format!("{}-codex-work", tag)),
            false,
            &prov,
        ))
        .unwrap();
        guard.codex.push(codex_work_id.clone());

        let codex = load_codex_envs_by_provider(&prov).unwrap();
        let ids: Vec<_> = codex.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids.len(), 2, "默认环境与非默认环境都应命中: {:?}", ids);
        assert!(
            codex.iter().any(|r| r.is_default),
            "默认环境被排除: {:?}",
            ids
        );

        // 删除供应商路径：默认环境的 provider_id 也必须被清空。
        clear_provider_id_on_envs(&prov).unwrap();
        for id in [
            &claude_default_id,
            &claude_work_id,
            &codex_default_id,
            &codex_work_id,
        ] {
            if let Some(row) = get_claude_environment_row(id).unwrap() {
                assert_eq!(row.provider_id, "", "claude 环境 {} 仍挂着 provider", id);
            }
            if let Some(row) = get_codex_environment_row(id).unwrap() {
                assert_eq!(row.provider_id, "", "codex 环境 {} 仍挂着 provider", id);
            }
        }
    }

    #[test]
    fn skill_meta_upsert_and_load() {
        let _home_guard = crate::config::lock_home_for_test();
        let id = format!(
            "__agentbuddy_test_skill_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );
        let entry = SkillMetaEntry {
            source: SkillSource::Github,
            repo_url: "https://github.com/example/repo".into(),
            github_owner: "example".into(),
            github_repo: "repo".into(),
            github_path: "skills/foo".into(),
            tag: "前端".into(),
            local_ref: "abc".into(),
            remote_ref: "def".into(),
            created_at: 1,
            updated_at: 2,
            content_hash: "hash0".into(),
        };
        upsert_skill_meta(&id, &entry).expect("upsert");
        let loaded = get_skill_meta(&id).expect("get").expect("exists");
        assert_eq!(loaded.github_owner, "example");
        assert_eq!(loaded.github_repo, "repo");
        assert_eq!(loaded.local_ref, "abc");
        assert_eq!(loaded.tag, "前端");
        assert!(matches!(loaded.source, SkillSource::Github));

        update_skill_refs(&id, Some("newlocal"), Some("newremote"), 99).expect("refs");
        let loaded2 = get_skill_meta(&id).expect("get2").expect("exists2");
        assert_eq!(loaded2.local_ref, "newlocal");
        assert_eq!(loaded2.remote_ref, "newremote");
        assert_eq!(loaded2.updated_at, 99);
        // created_at preserved
        assert_eq!(loaded2.created_at, 1);

        delete_skill_meta(&id).expect("delete");
        assert!(get_skill_meta(&id).expect("get3").is_none());
    }
}
