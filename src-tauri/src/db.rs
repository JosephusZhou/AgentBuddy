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
         );",
    )
    .map_err(|e| format!("Failed to create table: {}", e))?;

    // Additive migrations for DBs created before a column existed. `ALTER TABLE
    // ADD COLUMN` errors if the column is already present, so ignore that case.
    ensure_column(&conn, "skills", "tag", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "ai_providers", "openai_default_model", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "ai_providers", "sort_order", "INTEGER NOT NULL DEFAULT 0");
    ensure_column(&conn, "claude_environments", "provider_id", "TEXT NOT NULL DEFAULT ''");
    ensure_column(&conn, "codex_environments", "provider_id", "TEXT NOT NULL DEFAULT ''");

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
            eprintln!("[agent-buddy] ensure_column {}.{} failed: {}", table, column, msg);
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
        let install_paths_json = serde_json::to_string(&agent.install_paths)
            .unwrap_or_else(|_| "[]".to_string());
        let config_dirs_json = serde_json::to_string(&agent.config_dirs)
            .unwrap_or_else(|_| "[]".to_string());

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
/// codebuddy）在注册表移除对应 agent 后仍永远残留，导致 Agent 管理页
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

            let install_paths: Vec<String> = serde_json::from_str(&install_paths_raw)
                .unwrap_or_default();
            let config_dirs: Vec<String> = serde_json::from_str(&config_dirs_raw)
                .unwrap_or_default();

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
        },
    ))
}

/// Load all skill metadata rows keyed by skill id (directory name).
pub fn load_skill_meta_map() -> Result<HashMap<String, SkillMetaEntry>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, source, repo_url, github_owner, github_repo, github_path,
                    tag, local_ref, remote_ref, created_at, updated_at
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
                    tag, local_ref, remote_ref, created_at, updated_at
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
             tag, local_ref, remote_ref, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
            updated_at = excluded.updated_at",
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
    conn.execute(
        "DELETE FROM claude_environments WHERE id = ?1",
        params![id],
    )
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
    conn.execute(
        "DELETE FROM codex_environments WHERE id = ?1",
        params![id],
    )
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

/// Find all non-default Claude environments linked to the given provider_id.
pub fn load_claude_envs_by_provider(provider_id: &str) -> Result<Vec<ClaudeEnvironmentRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, config_dir, alias_name, is_default, source, notes,
                    alias_installed, created_at, updated_at, provider_id
             FROM claude_environments
             WHERE provider_id = ?1 AND is_default = 0",
        )
        .map_err(|e| format!("Failed to prepare claude_env by provider query: {}", e))?;
    let rows = stmt
        .query_map(params![provider_id], row_to_claude_env)
        .map_err(|e| format!("Failed to query claude_env by provider: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read claude_env by provider: {}", e))?;
    Ok(rows)
}

/// Find all non-default Codex environments linked to the given provider_id.
pub fn load_codex_envs_by_provider(provider_id: &str) -> Result<Vec<CodexEnvironmentRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, config_dir, alias_name, is_default, source, notes,
                    alias_installed, created_at, updated_at, provider_id
             FROM codex_environments
             WHERE provider_id = ?1 AND is_default = 0",
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
    })
}

pub fn load_ai_provider_rows() -> Result<Vec<AiProviderRow>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, provider_type, base_url, api_key_salt, api_key_nonce,
                    api_key_cipher, default_model, openai_default_model, models_json,
                    notes, created_at, updated_at, sort_order
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
                    notes, created_at, updated_at, sort_order
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
             notes, created_at, updated_at, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
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
            sort_order = excluded.sort_order",
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
        ],
    )
    .map_err(|e| format!("Failed to upsert ai_provider {}: {}", row.id, e))?;
    Ok(())
}

pub fn delete_ai_provider_row(id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute("DELETE FROM ai_providers WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete ai_provider {}: {}", id, e))?;
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

/// Load all toggle rows for a given route group.
pub fn load_provider_route_toggles(group: crate::route_aggregation::RouteGroup) -> Result<Vec<crate::route_aggregation::ProviderRouteToggle>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT provider_id, route_group, enabled, sort_order
             FROM provider_route_toggle
             WHERE route_group = ?1
             ORDER BY sort_order ASC",
        )
        .map_err(|e| format!("Failed to prepare route_toggle query: {}", e))?;

    let rows = stmt
        .query_map(params![group.as_str()], |row| {
            Ok(crate::route_aggregation::ProviderRouteToggle {
                provider_id: row.get(0)?,
                group: row.get(1)?,
                enabled: row.get::<_, i32>(2)? != 0,
                sort_order: row.get(3)?,
            })
        })
        .map_err(|e| format!("Failed to query route_toggle: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read route_toggle: {}", e))?;

    Ok(rows)
}

/// Upsert a provider's toggle for a route group.
pub fn upsert_provider_route_toggle(
    provider_id: &str,
    group: crate::route_aggregation::RouteGroup,
    enabled: bool,
    sort_order: i32,
) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT INTO provider_route_toggle (provider_id, route_group, enabled, sort_order)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(provider_id, route_group) DO UPDATE SET
            enabled = excluded.enabled,
            sort_order = excluded.sort_order",
        params![provider_id, group.as_str(), enabled as i32, sort_order],
    )
    .map_err(|e| format!("Failed to upsert route_toggle for {}: {}", provider_id, e))?;
    Ok(())
}

/// Delete toggle rows for a provider that no longer exists.
pub fn delete_provider_route_toggles(provider_id: &str) -> Result<(), String> {
    let conn = get_connection()?;
    conn.execute(
        "DELETE FROM provider_route_toggle WHERE provider_id = ?1",
        params![provider_id],
    )
    .map_err(|e| format!("Failed to delete route_toggle for {}: {}", provider_id, e))?;
    Ok(())
}

/// Batch-update sort_order for a route group.
pub fn reorder_provider_route_toggles(
    ids: &[String],
    group: crate::route_aggregation::RouteGroup,
) -> Result<(), String> {
    let conn = get_connection()?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to begin transaction: {}", e))?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE provider_route_toggle SET sort_order = ?1 WHERE provider_id = ?2 AND route_group = ?3",
            params![i as i32, id, group.as_str()],
        )
        .map_err(|e| format!("Failed to reorder route_toggle {}: {}", id, e))?;
    }
    tx.commit()
        .map_err(|e| format!("Failed to commit reorder: {}", e))?;
    Ok(())
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

    #[test]
    fn skill_meta_upsert_and_load() {
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
