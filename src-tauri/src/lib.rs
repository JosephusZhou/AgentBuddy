mod agent_open;
mod agents;
mod ai_provider;
mod backup;
mod claude_env;
mod codex_env;
mod config;
mod crypto;
mod db;
mod http_client;
mod mcp_config;
mod opencode_config;
mod platform;
mod project_config;
mod route_aggregation;
mod skills;
mod sniff;
mod webdav;

#[tauri::command]
async fn get_app_config() -> Result<config::AppConfig, String> {
    config::load_app_config()
}

#[tauri::command]
async fn set_theme(theme: String) -> Result<config::AppConfig, String> {
    config::set_theme(theme)
}

/// Set the NSWindow appearance to light or dark so that inactive traffic lights
/// render in the correct shade (gray for dark, light-gray for light).
/// Called by the frontend after loading or switching themes.
#[tauri::command]
async fn set_window_appearance(
    category: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let dark = category == "dark";
        set_ns_window_appearance(&app, dark);
    }
    let _ = category;
    Ok(())
}

#[tauri::command]
async fn get_network_settings() -> Result<config::NetworkSettings, String> {
    config::load_network_settings()
}

#[tauri::command]
async fn update_network_settings(
    settings: config::NetworkSettings,
) -> Result<config::NetworkSettings, String> {
    config::save_network_settings(settings)
}

#[tauri::command]
async fn sniff_agents() -> Result<Vec<sniff::SniffResult>, String> {
    let results = sniff::sniff_agents();
    db::save_agents(&results)?;
    Ok(results)
}

#[tauri::command]
async fn agent_open_targets(name: String) -> Result<agent_open::AgentOpenTargets, String> {
    Ok(agent_open::open_targets(&name))
}

#[tauri::command]
async fn reveal_agent_config_dir(name: String) -> Result<agent_open::AgentOpenResult, String> {
    tauri::async_runtime::spawn_blocking(move || agent_open::reveal_config_dir(name))
        .await
        .map_err(|e| format!("打开配置目录任务失败: {e}"))?
}

#[tauri::command]
async fn open_agent_config_file(
    name: String,
    kind: String,
) -> Result<agent_open::AgentOpenResult, String> {
    tauri::async_runtime::spawn_blocking(move || agent_open::open_config_file(name, kind))
        .await
        .map_err(|e| format!("打开配置文件任务失败: {e}"))?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentConfigStat {
    name: String,
    mcp_count: usize,
    skill_count: usize,
}

#[tauri::command]
async fn get_agent_config_stats(names: Vec<String>) -> Result<Vec<AgentConfigStat>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        names
            .iter()
            .map(|name| AgentConfigStat {
                name: name.clone(),
                mcp_count: mcp_config::count_agent_mcp_entries(name),
                skill_count: skills::count_agent_skills(name),
            })
            .collect()
    })
    .await
    .map_err(|e| format!("读取 Agent 配置统计任务失败: {e}"))
}

/// One MCP entry shown on the agent detail page (secrets like env/headers omitted).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentMcpInfo {
    title: String,
    transport: String,
    command: String,
    args: Vec<String>,
    url: String,
}

/// Everything the agent detail page shows for one agent.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentDetail {
    name: String,
    display_name: String,
    icon: String,
    found: bool,
    install_paths: Vec<String>,
    config_dirs: Vec<String>,
    config_dir: Option<String>,
    mcp_file: Option<String>,
    settings_file: Option<String>,
    mcps: Vec<AgentMcpInfo>,
    skills: Vec<skills::AgentSkillInfo>,
}

#[tauri::command]
async fn get_agent_detail(name: String) -> Result<AgentDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Base info comes from the cached scan (same source as the list view);
        // strip stale shim paths like `get_cached_agents` does.
        let cached = db::load_agents().unwrap_or_default();
        let row = cached.into_iter().find(|a| a.name == name);
        let (display_name, icon, found, mut install_paths, config_dirs) = match row {
            Some(a) => (
                a.display_name,
                a.icon,
                a.found,
                a.install_paths,
                a.config_dirs,
            ),
            None => (
                name.clone(),
                "?".to_string(),
                false,
                Vec::new(),
                Vec::new(),
            ),
        };
        install_paths.retain(|p| !sniff::is_shim_path(p));

        let targets = agent_open::open_targets(&name);

        let mcps = mcp_config::list_agent_mcp_entries(&name)
            .into_iter()
            .map(|(title, draft)| AgentMcpInfo {
                title,
                transport: draft.transport,
                command: draft.command,
                args: draft.args,
                url: draft.url,
            })
            .collect();

        let skills = skills::list_agent_skills(&name);

        AgentDetail {
            name,
            display_name,
            icon,
            found,
            install_paths,
            config_dirs,
            config_dir: targets.config_dir,
            mcp_file: targets.mcp_file,
            settings_file: targets.settings_file,
            mcps,
            skills,
        }
    })
    .await
    .map_err(|e| format!("读取 Agent 详情任务失败: {e}"))
}

#[tauri::command]
async fn get_cached_agents() -> Result<Vec<sniff::SniffResult>, String> {
    let mut agents = db::load_agents()?;
    // Cached scans taken while a CLI interceptor (e.g. cmux) was active may hold
    // stale shim paths. Strip them on read so the display self-heals without a
    // manual rescan; `found`/config dirs are left as stored, and the next real
    // sniff overwrites the row with clean paths.
    for agent in &mut agents {
        agent.install_paths.retain(|p| !sniff::is_shim_path(p));
    }
    Ok(agents)
}

#[tauri::command]
async fn add_agent_manual(
    name: String,
    cli_path: Option<String>,
    config_dir: Option<String>,
) -> Result<sniff::SniffResult, String> {
    let initials = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .collect::<String>();
    let icon = if initials.is_empty() {
        "?".to_string()
    } else {
        initials.to_uppercase()
    };

    let install_paths: Vec<String> = cli_path.into_iter().collect();
    let config_dirs: Vec<String> = config_dir.into_iter().collect();

    let agent = sniff::SniffResult {
        name: name.to_lowercase().replace(' ', "-"),
        display_name: name,
        icon,
        found: true,
        install_paths,
        config_dirs,
    };

    // Save to DB
    db::save_agents(&[agent.clone()])?;

    Ok(agent)
}

#[tauri::command]
async fn apply_mcp_to_agents(
    draft: mcp_config::McpDraft,
    agents: Vec<String>,
) -> Result<mcp_config::McpBatchResult, String> {
    Ok(mcp_config::apply_mcp_to_agents(&draft, &agents))
}

#[tauri::command]
async fn remove_mcp_from_agents(
    title: String,
    agents: Vec<String>,
) -> Result<mcp_config::McpBatchResult, String> {
    Ok(mcp_config::remove_mcp_from_agents(&title, &agents))
}

#[tauri::command]
async fn sniff_mcp_servers() -> Result<mcp_config::McpSniffResult, String> {
    let sniffed = mcp_config::sniff_mcp_servers();
    let existing = db::load_mcp_servers().unwrap_or_default();
    let merged = mcp_config::merge_sniffed_servers(&existing, &sniffed.servers);
    db::replace_mcp_servers(&merged)?;
    Ok(mcp_config::McpSniffResult {
        servers: merged,
        scanned_agents: sniffed.scanned_agents,
        found_entries: sniffed.found_entries,
        message: sniffed.message,
    })
}

#[tauri::command]
async fn get_mcp_servers() -> Result<Vec<mcp_config::McpServerRecord>, String> {
    db::load_mcp_servers()
}

#[tauri::command]
async fn save_mcp_servers(servers: Vec<mcp_config::McpServerRecord>) -> Result<(), String> {
    db::replace_mcp_servers(&servers)
}

#[tauri::command]
async fn delete_mcp_server(id: String) -> Result<(), String> {
    db::delete_mcp_server(&id)
}

#[tauri::command]
async fn test_mcp_connection(
    draft: mcp_config::McpDraft,
) -> Result<mcp_config::McpTestResult, String> {
    // Spawns a child process / blocking HTTP probe — offload from the async runtime.
    tauri::async_runtime::spawn_blocking(move || mcp_config::test_mcp_connection(draft))
        .await
        .map_err(|e| format!("MCP 连通性测试任务失败: {}", e))
}

#[tauri::command]
async fn get_webdav_connections() -> Result<Vec<webdav::WebDavConnection>, String> {
    webdav::list_connections()
}

#[tauri::command]
async fn upsert_webdav_connection(
    payload: webdav::WebDavUpsertPayload,
) -> Result<webdav::WebDavConnection, String> {
    webdav::upsert_connection(payload)
}

#[tauri::command]
async fn delete_webdav_connection(id: String) -> Result<(), String> {
    webdav::delete_connection(id)
}

#[tauri::command]
async fn test_webdav_connection(id: String) -> Result<webdav::WebDavTestResult, String> {
    // Blocking HTTP probe — offload from the async runtime.
    tauri::async_runtime::spawn_blocking(move || webdav::test_connection(id))
        .await
        .map_err(|e| format!("WebDAV 测试任务失败: {}", e))?
}

#[tauri::command]
async fn test_webdav_connection_draft(
    draft: webdav::WebDavDraftProbe,
) -> Result<webdav::WebDavTestResult, String> {
    tauri::async_runtime::spawn_blocking(move || webdav::test_connection_draft(draft))
        .await
        .map_err(|e| format!("WebDAV 测试任务失败: {}", e))?
}

#[tauri::command]
async fn list_backup_units() -> Result<Vec<backup::BackupUnitNode>, String> {
    tauri::async_runtime::spawn_blocking(backup::list_backup_units)
        .await
        .map_err(|e| format!("探测备份源任务失败: {}", e))?
}

#[tauri::command]
async fn get_backup_settings() -> Result<config::BackupSettings, String> {
    backup::get_backup_settings()
}

#[tauri::command]
async fn update_backup_settings(
    settings: config::BackupSettings,
) -> Result<config::BackupSettings, String> {
    backup::update_backup_settings(settings)
}

#[tauri::command]
async fn run_backup_upload(
    app: tauri::AppHandle,
    payload: backup::BackupRunPayload,
) -> Result<backup::BackupRunResult, String> {
    tauri::async_runtime::spawn_blocking(move || backup::run_backup_upload(app, payload))
        .await
        .map_err(|e| format!("备份任务失败: {}", e))?
}

#[tauri::command]
async fn list_remote_backups(
    payload: backup::ListRemoteBackupsPayload,
) -> Result<Vec<backup::RemoteBackupItem>, String> {
    tauri::async_runtime::spawn_blocking(move || backup::list_remote_backups(payload))
        .await
        .map_err(|e| format!("列举远程备份任务失败: {}", e))?
}

#[tauri::command]
async fn restore_remote_backup(
    app: tauri::AppHandle,
    payload: backup::RestoreBackupPayload,
) -> Result<backup::RestoreBackupResult, String> {
    tauri::async_runtime::spawn_blocking(move || backup::restore_remote_backup(app, payload))
        .await
        .map_err(|e| format!("恢复备份任务失败: {}", e))?
}

#[tauri::command]
async fn list_skills() -> Result<skills::SkillsListResult, String> {
    tauri::async_runtime::spawn_blocking(skills::list_skills)
        .await
        .map_err(|e| format!("列出 skills 任务失败: {}", e))?
}

#[tauri::command]
async fn sniff_skills() -> Result<skills::SkillSniffResult, String> {
    tauri::async_runtime::spawn_blocking(skills::sniff_skills)
        .await
        .map_err(|e| format!("嗅探 skills 任务失败: {}", e))?
}

#[tauri::command]
async fn preview_sniff_skills() -> Result<skills::SniffPreviewResult, String> {
    tauri::async_runtime::spawn_blocking(skills::preview_sniff_skills)
        .await
        .map_err(|e| format!("预览嗅探 skills 任务失败: {}", e))?
}

#[tauri::command]
async fn import_sniffed_skills(keys: Vec<String>) -> Result<skills::SniffImportResult, String> {
    tauri::async_runtime::spawn_blocking(move || skills::import_sniffed_skills(keys))
        .await
        .map_err(|e| format!("导入嗅探 skills 任务失败: {}", e))?
}

#[tauri::command]
async fn check_skill_updates() -> Result<skills::SkillUpdateCheckResult, String> {
    tauri::async_runtime::spawn_blocking(skills::check_skill_updates)
        .await
        .map_err(|e| format!("检查 skills 更新任务失败: {}", e))?
}

#[tauri::command]
async fn add_skill_local(
    path: String,
    tag: Option<String>,
) -> Result<skills::SkillActionResult, String> {
    let tag = tag.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || skills::add_skill_local(path, tag))
        .await
        .map_err(|e| format!("导入本地 skill 任务失败: {}", e))?
}

#[tauri::command]
async fn pick_and_add_skill_local(
    tag: Option<String>,
) -> Result<skills::SkillActionResult, String> {
    // Folder picker must run on a blocking thread; osascript is sync.
    let tag = tag.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || skills::pick_local_skill_folder(tag))
        .await
        .map_err(|e| format!("选择 skill 目录任务失败: {}", e))?
}

#[tauri::command]
async fn add_skill_github(
    url: String,
    tag: Option<String>,
) -> Result<skills::SkillActionResult, String> {
    let tag = tag.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || skills::add_skill_github(url, tag))
        .await
        .map_err(|e| format!("从 GitHub 导入 skill 任务失败: {}", e))?
}

#[tauri::command]
async fn add_skill_gitcode(
    url: String,
    tag: Option<String>,
) -> Result<skills::SkillActionResult, String> {
    let tag = tag.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || skills::add_skill_gitcode(url, tag))
        .await
        .map_err(|e| format!("从 GitCode 导入 skill 任务失败: {}", e))?
}

#[tauri::command]
async fn update_skill(skill_id: String) -> Result<skills::SkillActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || skills::update_skill(skill_id))
        .await
        .map_err(|e| format!("更新 skill 任务失败: {}", e))?
}

#[tauri::command]
async fn export_skill_to_dir(
    skill_id: String,
    install_mode: Option<String>,
) -> Result<skills::SkillActionResult, String> {
    // Folder picker must run on a blocking thread; osascript is sync.
    let install_mode = skills::SkillInstallMode::from_wire(install_mode.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        skills::export_skill_to_dir(skill_id, install_mode)
    })
    .await
    .map_err(|e| format!("应用 skill 到目录任务失败: {}", e))?
}

#[tauri::command]
async fn open_external_url(url: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || skills::open_external_url(url))
        .await
        .map_err(|e| format!("打开链接任务失败: {}", e))?
}

#[tauri::command]
async fn delete_skill(
    skill_id: String,
    delete_agent_copies: Option<bool>,
) -> Result<skills::SkillActionResult, String> {
    let delete_copies = delete_agent_copies.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || skills::delete_skill(skill_id, delete_copies))
        .await
        .map_err(|e| format!("删除 skill 任务失败: {}", e))?
}

#[tauri::command]
async fn apply_skill_to_agents(
    skill_id: String,
    agents: Vec<String>,
    tag: Option<String>,
    install_mode: Option<String>,
) -> Result<skills::SkillApplyResult, String> {
    let tag = tag.unwrap_or_default();
    let install_mode = skills::SkillInstallMode::from_wire(install_mode.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        skills::apply_skill_to_agents(skill_id, agents, tag, install_mode)
    })
    .await
    .map_err(|e| format!("应用 skill 到 Agent 任务失败: {}", e))?
}

#[tauri::command]
async fn batch_delete_skills(
    skill_ids: Vec<String>,
    delete_agent_copies: Option<bool>,
) -> Result<skills::BatchSkillResult, String> {
    let delete_copies = delete_agent_copies.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        skills::batch_delete_skills(skill_ids, delete_copies)
    })
    .await
    .map_err(|e| format!("批量删除 skill 任务失败: {}", e))?
}

#[tauri::command]
async fn batch_export_skills_to_dir(
    skill_ids: Vec<String>,
    install_mode: Option<String>,
) -> Result<skills::BatchSkillResult, String> {
    // Folder picker must run on a blocking thread; osascript is sync.
    let install_mode = skills::SkillInstallMode::from_wire(install_mode.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        skills::batch_export_skills_to_dir(skill_ids, install_mode)
    })
    .await
    .map_err(|e| format!("批量应用 skill 到目录任务失败: {}", e))?
}

#[tauri::command]
async fn batch_apply_skills_to_agents(
    skill_ids: Vec<String>,
    agents: Vec<String>,
    mode: String,
    install_mode: Option<String>,
) -> Result<skills::BatchSkillResult, String> {
    // 未知模式一律退回到「追加」，避免误触发覆盖导致解除现有应用。
    let apply_mode = if mode == "replace" {
        skills::BatchApplyMode::Replace
    } else {
        skills::BatchApplyMode::Add
    };
    let install_mode = skills::SkillInstallMode::from_wire(install_mode.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        skills::batch_apply_skills_to_agents(skill_ids, agents, apply_mode, install_mode)
    })
    .await
    .map_err(|e| format!("批量应用 skill 到 Agent 任务失败: {}", e))?
}

#[tauri::command]
async fn batch_set_skill_tag(
    skill_ids: Vec<String>,
    tag: Option<String>,
) -> Result<skills::BatchSkillResult, String> {
    let tag = tag.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || skills::batch_set_skill_tag(skill_ids, tag))
        .await
        .map_err(|e| format!("批量设置 skill 标签任务失败: {}", e))?
}

#[tauri::command]
async fn preview_cc_switch_skills() -> Result<skills::CcSwitchPreviewResult, String> {
    tauri::async_runtime::spawn_blocking(skills::preview_cc_switch_skills)
        .await
        .map_err(|e| format!("预览 CC Switch 迁移任务失败: {}", e))?
}

#[tauri::command]
async fn migrate_cc_switch_skills(
    cc_ids: Vec<String>,
) -> Result<skills::CcSwitchMigrateResult, String> {
    tauri::async_runtime::spawn_blocking(move || skills::migrate_cc_switch_skills(cc_ids))
        .await
        .map_err(|e| format!("CC Switch 迁移任务失败: {}", e))?
}

#[tauri::command]
async fn list_claude_environments() -> Result<Vec<claude_env::ClaudeEnvironment>, String> {
    tauri::async_runtime::spawn_blocking(claude_env::list_environments)
        .await
        .map_err(|e| format!("列出 Claude 环境任务失败: {}", e))?
}

#[tauri::command]
async fn sniff_claude_environments() -> Result<claude_env::ClaudeEnvSniffResult, String> {
    tauri::async_runtime::spawn_blocking(claude_env::sniff_environments)
        .await
        .map_err(|e| format!("扫描 Claude 环境任务失败: {}", e))?
}

#[tauri::command]
async fn import_claude_environment(
    payload: claude_env::ClaudeEnvImportPayload,
) -> Result<claude_env::ClaudeEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::import_environment(payload))
        .await
        .map_err(|e| format!("导入 Claude 环境任务失败: {}", e))?
}

#[tauri::command]
async fn clone_claude_environment(
    payload: claude_env::ClaudeEnvClonePayload,
) -> Result<claude_env::ClaudeEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::clone_environment(payload))
        .await
        .map_err(|e| format!("复制 Claude 环境任务失败: {}", e))?
}

#[tauri::command]
async fn upsert_claude_environment(
    payload: claude_env::ClaudeEnvUpsertPayload,
) -> Result<claude_env::ClaudeEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::upsert_environment(payload))
        .await
        .map_err(|e| format!("更新 Claude 环境任务失败: {}", e))?
}

#[tauri::command]
async fn delete_claude_environment(
    id: String,
    delete_files: bool,
) -> Result<claude_env::ClaudeEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::delete_environment(id, delete_files))
        .await
        .map_err(|e| format!("删除 Claude 环境任务失败: {}", e))?
}

#[tauri::command]
async fn install_claude_env_alias(id: String) -> Result<claude_env::ClaudeEnvShellStatus, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::install_env_alias(id))
        .await
        .map_err(|e| format!("写入 shell 别名任务失败: {}", e))?
}

#[tauri::command]
async fn remove_claude_env_alias(id: String) -> Result<claude_env::ClaudeEnvShellStatus, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::remove_env_alias(id))
        .await
        .map_err(|e| format!("移除 shell 别名任务失败: {}", e))?
}

#[tauri::command]
async fn remove_all_claude_env_aliases() -> Result<claude_env::ClaudeEnvShellStatus, String> {
    tauri::async_runtime::spawn_blocking(claude_env::remove_all_aliases)
        .await
        .map_err(|e| format!("清除 shell 别名任务失败: {}", e))?
}

#[tauri::command]
async fn get_claude_env_shell_status() -> Result<claude_env::ClaudeEnvShellStatus, String> {
    tauri::async_runtime::spawn_blocking(claude_env::get_shell_status)
        .await
        .map_err(|e| format!("读取 shell 状态任务失败: {}", e))?
}

#[tauri::command]
async fn reveal_claude_env_dir(id: String) -> Result<claude_env::ClaudeEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::reveal_dir(id))
        .await
        .map_err(|e| format!("打开目录任务失败: {}", e))?
}

#[tauri::command]
async fn open_claude_env_settings(id: String) -> Result<claude_env::ClaudeEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::open_settings(id))
        .await
        .map_err(|e| format!("打开配置文件任务失败: {}", e))?
}

#[tauri::command]
async fn get_claude_env_secret(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::get_env_secret(id))
        .await
        .map_err(|e| format!("读取环境密钥任务失败: {}", e))?
}

#[tauri::command]
async fn sync_claude_env_mcp(id: String) -> Result<claude_env::ClaudeEnvMcpSyncResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::sync_mcp_to_environment(id))
        .await
        .map_err(|e| format!("同步 MCP 任务失败: {}", e))?
}

#[tauri::command]
async fn sync_claude_env_skills(
    id: String,
) -> Result<claude_env::ClaudeEnvSkillsSyncResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::sync_skills_to_environment(id))
        .await
        .map_err(|e| format!("同步 Claude skills 任务失败: {e}"))?
}

#[tauri::command]
async fn sync_all_claude_env_mcp() -> Result<claude_env::ClaudeEnvMcpSyncResult, String> {
    tauri::async_runtime::spawn_blocking(claude_env::sync_mcp_to_all_environments)
        .await
        .map_err(|e| format!("批量同步 MCP 任务失败: {}", e))?
}

#[tauri::command]
async fn get_claude_env_mcp_status() -> Result<claude_env::ClaudeEnvMcpStatusResult, String> {
    tauri::async_runtime::spawn_blocking(claude_env::get_mcp_sync_status)
        .await
        .map_err(|e| format!("读取 MCP 同步状态任务失败: {}", e))?
}

#[tauri::command]
async fn fetch_claude_env_remote_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<claude_env::ClaudeEnvRemoteModelsResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::fetch_remote_models(base_url, api_key))
        .await
        .map_err(|e| format!("拉取远端模型任务失败: {e}"))?
}

/* ===== OpenCode provider / model config ===== */

#[tauri::command]
async fn get_opencode_config() -> Result<opencode_config::OpencodeConfigView, String> {
    opencode_config::get_config()
}

#[tauri::command]
async fn set_opencode_defaults(
    payload: opencode_config::SetDefaultsPayload,
) -> Result<opencode_config::OpencodeActionResult, String> {
    opencode_config::set_defaults(payload)
}

#[tauri::command]
async fn upsert_opencode_provider(
    payload: opencode_config::UpsertProviderPayload,
) -> Result<opencode_config::OpencodeActionResult, String> {
    opencode_config::upsert_provider(payload)
}

#[tauri::command]
async fn delete_opencode_provider(
    provider_id: String,
    delete_auth: bool,
) -> Result<opencode_config::OpencodeActionResult, String> {
    opencode_config::delete_provider(provider_id, delete_auth)
}

#[tauri::command]
async fn upsert_opencode_model(
    payload: opencode_config::UpsertModelPayload,
) -> Result<opencode_config::OpencodeActionResult, String> {
    opencode_config::upsert_model(payload)
}

#[tauri::command]
async fn delete_opencode_model(
    provider_id: String,
    model_id: String,
) -> Result<opencode_config::OpencodeActionResult, String> {
    opencode_config::delete_model(provider_id, model_id)
}

#[tauri::command]
async fn get_opencode_provider_secret(provider_id: String) -> Result<String, String> {
    opencode_config::get_provider_secret(provider_id)
}

#[tauri::command]
async fn set_opencode_provider_secret(
    provider_id: String,
    api_key: String,
) -> Result<opencode_config::OpencodeActionResult, String> {
    opencode_config::set_provider_secret(provider_id, api_key)
}

#[tauri::command]
async fn fetch_models_dev_catalog(
    force: bool,
) -> Result<opencode_config::ModelsDevCatalog, String> {
    tauri::async_runtime::spawn_blocking(move || opencode_config::fetch_models_dev_catalog(force))
        .await
        .map_err(|e| format!("拉取目录任务失败: {e}"))?
}

#[tauri::command]
async fn probe_opencode_models_endpoint(
    base_url: String,
) -> Result<opencode_config::ProbeModelsResult, String> {
    tauri::async_runtime::spawn_blocking(move || opencode_config::probe_models_endpoint(base_url))
        .await
        .map_err(|e| format!("探测任务失败: {e}"))?
}

#[tauri::command]
async fn reveal_opencode_config() -> Result<opencode_config::OpencodeActionResult, String> {
    opencode_config::reveal_config()
}

#[tauri::command]
async fn get_opencode_fork_sync_status() -> Result<opencode_config::OpencodeForkSyncStatus, String>
{
    tauri::async_runtime::spawn_blocking(opencode_config::get_fork_sync_status)
        .await
        .map_err(|e| format!("查询 OpenCode fork 同步状态失败: {e}"))?
}

#[tauri::command]
async fn sync_opencode_to_fork(
    agent: String,
    sync_mcp: Option<bool>,
    sync_skills: Option<bool>,
) -> Result<opencode_config::OpencodeForkSyncResult, String> {
    let sync_mcp = sync_mcp.unwrap_or(false);
    let sync_skills = sync_skills.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        opencode_config::sync_to_fork_agent(agent, sync_mcp, sync_skills)
    })
    .await
    .map_err(|e| format!("同步 OpenCode 配置任务失败: {e}"))?
}

#[tauri::command]
async fn sync_opencode_to_all_forks(
    sync_mcp: Option<bool>,
    sync_skills: Option<bool>,
) -> Result<opencode_config::OpencodeForkSyncResult, String> {
    let sync_mcp = sync_mcp.unwrap_or(false);
    let sync_skills = sync_skills.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        opencode_config::sync_to_all_forks(sync_mcp, sync_skills)
    })
        .await
        .map_err(|e| format!("批量同步 OpenCode 配置任务失败: {e}"))?
}

/* ===== Codex multi-env (CODEX_HOME) ===== */

#[tauri::command]
async fn list_codex_environments() -> Result<Vec<codex_env::CodexEnvironment>, String> {
    tauri::async_runtime::spawn_blocking(codex_env::list_environments)
        .await
        .map_err(|e| format!("列出 Codex 环境任务失败: {}", e))?
}

#[tauri::command]
async fn sniff_codex_environments() -> Result<codex_env::CodexEnvSniffResult, String> {
    tauri::async_runtime::spawn_blocking(codex_env::sniff_environments)
        .await
        .map_err(|e| format!("扫描 Codex 环境任务失败: {}", e))?
}

#[tauri::command]
async fn import_codex_environment(
    payload: codex_env::CodexEnvImportPayload,
) -> Result<codex_env::CodexEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::import_environment(payload))
        .await
        .map_err(|e| format!("导入 Codex 环境任务失败: {}", e))?
}

#[tauri::command]
async fn clone_codex_environment(
    payload: codex_env::CodexEnvClonePayload,
) -> Result<codex_env::CodexEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::clone_environment(payload))
        .await
        .map_err(|e| format!("复制 Codex 环境任务失败: {}", e))?
}

#[tauri::command]
async fn upsert_codex_environment(
    payload: codex_env::CodexEnvUpsertPayload,
) -> Result<codex_env::CodexEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::upsert_environment(payload))
        .await
        .map_err(|e| format!("更新 Codex 环境任务失败: {}", e))?
}

#[tauri::command]
async fn delete_codex_environment(
    id: String,
    delete_files: bool,
) -> Result<codex_env::CodexEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::delete_environment(id, delete_files))
        .await
        .map_err(|e| format!("删除 Codex 环境任务失败: {}", e))?
}

#[tauri::command]
async fn install_codex_env_alias(id: String) -> Result<codex_env::CodexEnvShellStatus, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::install_env_alias(id))
        .await
        .map_err(|e| format!("写入 Codex shell 别名任务失败: {}", e))?
}

#[tauri::command]
async fn remove_codex_env_alias(id: String) -> Result<codex_env::CodexEnvShellStatus, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::remove_env_alias(id))
        .await
        .map_err(|e| format!("移除 Codex shell 别名任务失败: {}", e))?
}

#[tauri::command]
async fn remove_all_codex_env_aliases() -> Result<codex_env::CodexEnvShellStatus, String> {
    tauri::async_runtime::spawn_blocking(codex_env::remove_all_aliases)
        .await
        .map_err(|e| format!("清除 Codex shell 别名任务失败: {}", e))?
}

#[tauri::command]
async fn get_codex_env_shell_status() -> Result<codex_env::CodexEnvShellStatus, String> {
    tauri::async_runtime::spawn_blocking(codex_env::get_shell_status)
        .await
        .map_err(|e| format!("读取 Codex shell 状态任务失败: {}", e))?
}

#[tauri::command]
async fn reveal_codex_env_dir(id: String) -> Result<codex_env::CodexEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::reveal_dir(id))
        .await
        .map_err(|e| format!("打开 Codex 目录任务失败: {}", e))?
}

#[tauri::command]
async fn open_codex_env_config(id: String) -> Result<codex_env::CodexEnvActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::open_config(id))
        .await
        .map_err(|e| format!("打开 Codex 配置文件任务失败: {}", e))?
}

#[tauri::command]
async fn get_codex_env_secret(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::get_env_secret(id))
        .await
        .map_err(|e| format!("读取 Codex 环境密钥任务失败: {}", e))?
}

#[tauri::command]
async fn sync_codex_env_mcp(id: String) -> Result<codex_env::CodexEnvMcpSyncResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::sync_mcp_to_environment(id))
        .await
        .map_err(|e| format!("同步 Codex MCP 任务失败: {}", e))?
}

#[tauri::command]
async fn sync_codex_env_skills(id: String) -> Result<codex_env::CodexEnvSkillsSyncResult, String> {
    tauri::async_runtime::spawn_blocking(move || codex_env::sync_skills_to_environment(id))
        .await
        .map_err(|e| format!("同步 Codex skills 任务失败: {e}"))?
}

#[tauri::command]
async fn sync_all_codex_env_mcp() -> Result<codex_env::CodexEnvMcpSyncResult, String> {
    tauri::async_runtime::spawn_blocking(codex_env::sync_mcp_to_all_environments)
        .await
        .map_err(|e| format!("批量同步 Codex MCP 任务失败: {}", e))?
}

#[tauri::command]
async fn fetch_codex_env_remote_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<claude_env::ClaudeEnvRemoteModelsResult, String> {
    tauri::async_runtime::spawn_blocking(move || claude_env::fetch_remote_models(base_url, api_key))
        .await
        .map_err(|e| format!("拉取 Codex 远端模型任务失败: {e}"))?
}

/* ===== AI providers (上游模型供应商库) ===== */

#[tauri::command]
async fn list_ai_providers() -> Result<Vec<ai_provider::AiProvider>, String> {
    tauri::async_runtime::spawn_blocking(ai_provider::list_providers)
        .await
        .map_err(|e| format!("列出 AI 供应商任务失败: {e}"))?
}

#[tauri::command]
async fn upsert_ai_provider(
    payload: ai_provider::AiProviderUpsertPayload,
) -> Result<ai_provider::AiProviderActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || ai_provider::upsert_provider(payload))
        .await
        .map_err(|e| format!("保存 AI 供应商任务失败: {e}"))?
}

#[tauri::command]
async fn delete_ai_provider(id: String) -> Result<ai_provider::AiProviderActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || ai_provider::delete_provider(id))
        .await
        .map_err(|e| format!("删除 AI 供应商任务失败: {e}"))?
}

#[tauri::command]
async fn get_ai_provider_secret(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || ai_provider::get_provider_secret(id))
        .await
        .map_err(|e| format!("读取供应商密钥任务失败: {e}"))?
}

#[tauri::command]
async fn reorder_ai_providers(ids: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || ai_provider::reorder_providers(ids))
        .await
        .map_err(|e| format!("供应商排序任务失败: {e}"))?
}

/* ===== Route aggregation (local proxy server) ===== */

#[tauri::command]
async fn get_route_aggregation_status(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<route_aggregation::RouteAggregationStatus, String> {
    Ok(state.get_status().await)
}

#[tauri::command]
async fn get_route_aggregation_config(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<route_aggregation::RouteAggregationConfig, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
async fn update_route_aggregation_config(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    config: route_aggregation::RouteAggregationConfig,
) -> Result<route_aggregation::RouteAggregationStatus, String> {
    let config = route_aggregation::config::normalize_config(config)?;
    route_aggregation::config::save_config(&config)?;
    *state.config.write().await = config.clone();

    // If server is running and structural changes were made (group toggles),
    // we need to restart. For non-structural changes, just update the config snapshot.
    let needs_restart = {
        let server_guard = state.server.read().await;
        server_guard.is_some()
    };

    if needs_restart {
        // Stop existing server
        if let Some(mut server) = state.server.write().await.take() {
            server.stop();
        }
        // Start with new config if any group is enabled
        if config.claude_code_enabled || config.codex_enabled {
            let server = route_aggregation::server::RouteAggregationServer::start(
                config.clone(),
                state.provider_router.clone(),
            )
            .await?;
            *state.server.write().await = Some(std::sync::Arc::new(server));
        }
    }

    // Refresh provider pools for enabled groups
    if config.claude_code_enabled {
        state.provider_router.refresh_pool(route_aggregation::RouteGroup::ClaudeCode).await?;
    }
    if config.codex_enabled {
        state.provider_router.refresh_pool(route_aggregation::RouteGroup::Codex).await?;
    }

    Ok(state.get_status().await)
}

#[tauri::command]
async fn start_route_aggregation(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<route_aggregation::RouteAggregationStatus, String> {
    let config = state.config.read().await.clone();

    // Port pre-check
    let test_addr = format!("{}:{}", config.listen_address, config.listen_port);
    if std::net::TcpListener::bind(&test_addr).is_err() {
        return Err(format!(
            "端口 {} 被占用，请在路由聚合设置中更改监听端口",
            config.listen_port
        ));
    }

    // Refresh provider pools
    if config.claude_code_enabled {
        state.provider_router.refresh_pool(route_aggregation::RouteGroup::ClaudeCode).await?;
    }
    if config.codex_enabled {
        state.provider_router.refresh_pool(route_aggregation::RouteGroup::Codex).await?;
    }

    let server = route_aggregation::server::RouteAggregationServer::start(
        config,
        state.provider_router.clone(),
    )
    .await?;
    *state.server.write().await = Some(std::sync::Arc::new(server));
    Ok(state.get_status().await)
}

#[tauri::command]
async fn stop_route_aggregation(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<(), String> {
    if let Some(mut server) = state.server.write().await.take() {
        server.stop();
    }
    Ok(())
}

#[tauri::command]
async fn get_provider_route_toggles(
    group: String,
) -> Result<Vec<route_aggregation::ProviderRouteToggle>, String> {
    let group = route_aggregation::RouteGroup::from_str(&group)
        .ok_or_else(|| format!("无效的路由组: {}", group))?;
    tauri::async_runtime::spawn_blocking(move || db::load_provider_route_toggles(group))
        .await
        .map_err(|e| format!("读取供应商路由开关任务失败: {e}"))?
}

#[tauri::command]
async fn toggle_provider_route(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    provider_id: String,
    group: String,
    enabled: bool,
) -> Result<(), String> {
    let group = route_aggregation::RouteGroup::from_str(&group)
        .ok_or_else(|| format!("无效的路由组: {}", group))?;
    tauri::async_runtime::spawn_blocking(move || {
        db::upsert_provider_route_toggle(&provider_id, group, enabled, 0)
    })
    .await
    .map_err(|e| format!("切换供应商路由开关任务失败: {e}"))?;
    // Refresh in-memory provider pool so status reflects the new toggle state
    state.provider_router.refresh_pool(group).await?;
    Ok(())
}

#[tauri::command]
async fn reorder_provider_routes(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    ids: Vec<String>,
    group: String,
) -> Result<(), String> {
    let group = route_aggregation::RouteGroup::from_str(&group)
        .ok_or_else(|| format!("无效的路由组: {}", group))?;
    tauri::async_runtime::spawn_blocking(move || db::reorder_provider_route_toggles(&ids, group))
        .await
        .map_err(|e| format!("供应商路由排序任务失败: {e}"))?;
    state.provider_router.refresh_pool(group).await?;
    Ok(())
}

#[tauri::command]
async fn reset_circuit_breaker(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    provider_id: String,
    group: String,
) -> Result<(), String> {
    let group = route_aggregation::RouteGroup::from_str(&group)
        .ok_or_else(|| format!("无效的路由组: {}", group))?;
    state.provider_router.reset_breaker(&provider_id, group).await;
    Ok(())
}

#[tauri::command]
async fn get_circuit_breaker_status(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    group: String,
) -> Result<Vec<route_aggregation::ProviderRouteStatus>, String> {
    let group = route_aggregation::RouteGroup::from_str(&group)
        .ok_or_else(|| format!("无效的路由组: {}", group))?;
    Ok(state.provider_router.get_provider_statuses(group).await)
}

#[tauri::command]
async fn pick_project_folder() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        platform::pick_folder("选择项目目录")
            .map(|opt| opt.map(|pb| pb.to_string_lossy().to_string()))
    })
    .await
    .map_err(|e| format!("选择目录任务失败: {e}"))?
}

#[tauri::command]
async fn check_project_config_exists(
    target_dir: String,
    selected_agents: Vec<project_config::AgentConfigRequest>,
    mode: project_config::InitMode,
    skill_ids: Vec<String>,
) -> Result<project_config::CheckResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        project_config::check_project_config_exists(&target_dir, &selected_agents, &mode, &skill_ids)
    })
    .await
    .map_err(|e| format!("检查项目配置任务失败: {e}"))?
}

#[tauri::command]
async fn init_project_config(
    target_dir: String,
    selected_agents: Vec<project_config::AgentConfigRequest>,
    mode: project_config::InitMode,
    overwrite: bool,
    mcp_servers: Vec<mcp_config::McpDraft>,
    skill_ids: Vec<String>,
    skill_mode: Option<String>,
) -> Result<project_config::InitResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        project_config::init_project_config(
            &target_dir,
            &selected_agents,
            &mode,
            overwrite,
            &mcp_servers,
            &skill_ids,
            skills::SkillInstallMode::from_wire(skill_mode.as_deref()),
        )
    })
    .await
    .map_err(|e| format!("初始化项目配置任务失败: {e}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
// ── macOS traffic-light helpers ──────────────────────────────────────────

/// Heuristic: classify a theme id as dark or light without duplicating the
/// frontend theme registry. The frontend calls `set_window_appearance` on
/// load/change, so any misclassification here is corrected within milliseconds.
#[cfg(target_os = "macos")]
fn theme_is_dark(theme_id: &str) -> bool {
    const DARK_HINTS: &[&str] = &[
        "dark", "night", "dracula", "monokai", "nord",
        "palenight", "cobalt", "synthwave", "mocha",
    ];
    DARK_HINTS.iter().any(|h| theme_id.contains(h))
}

/// Set the NSWindow's appearance to Aqua (light) or DarkAqua (dark).
/// This controls how macOS renders inactive traffic lights:
/// – DarkAqua → medium gray circles when window loses focus
/// – Aqua     → light gray circles when window loses focus
///
/// All objc calls are dispatched to the main thread via run_on_main_thread,
/// because macOS UI APIs are not safe to call from a tokio worker thread.
#[cfg(target_os = "macos")]
fn set_ns_window_appearance(app: &tauri::AppHandle, dark: bool) {
    use tauri::Manager;
    let app = app.clone();
    // run_on_main_thread borrows `app` (&self), so the closure must capture
    // a separate clone to avoid "cannot move out of borrowed" error.
    let handler = app.clone();
    let _ = app.run_on_main_thread(move || {
        use objc::{class, msg_send, sel, sel_impl};
        if let Some(window) = handler.get_webview_window("main") {
            if let Ok(ns_window_ptr) = window.ns_window() {
                unsafe {
                    let ns_window = ns_window_ptr as *mut objc::runtime::Object;
                    let name: &[u8] = if dark {
                        b"NSAppearanceNameDarkAqua\0"
                    } else {
                        b"NSAppearanceNameAqua\0"
                    };
                    let ns_name: *mut objc::runtime::Object = msg_send![
                        class!(NSString),
                        stringWithUTF8String: name.as_ptr() as *const i8
                    ];
                    let appearance: *mut objc::runtime::Object = msg_send![
                        class!(NSAppearance),
                        appearanceNamed: ns_name
                    ];
                    let _: () = msg_send![ns_window, setAppearance: appearance];
                }
            }
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            sniff_agents,
            get_cached_agents,
            get_agent_config_stats,
            get_agent_detail,
            add_agent_manual,
            agent_open_targets,
            reveal_agent_config_dir,
            open_agent_config_file,
            apply_mcp_to_agents,
            remove_mcp_from_agents,
            sniff_mcp_servers,
            get_mcp_servers,
            save_mcp_servers,
            delete_mcp_server,
            test_mcp_connection,
            get_app_config,
            set_theme,
            set_window_appearance,
            get_network_settings,
            update_network_settings,
            get_webdav_connections,
            upsert_webdav_connection,
            delete_webdav_connection,
            test_webdav_connection,
            test_webdav_connection_draft,
            list_backup_units,
            get_backup_settings,
            update_backup_settings,
            run_backup_upload,
            list_remote_backups,
            restore_remote_backup,
            list_skills,
            sniff_skills,
            preview_sniff_skills,
            import_sniffed_skills,
            check_skill_updates,
            add_skill_local,
            pick_and_add_skill_local,
            add_skill_github,
            add_skill_gitcode,
            update_skill,
            export_skill_to_dir,
            open_external_url,
            delete_skill,
            apply_skill_to_agents,
            batch_delete_skills,
            batch_export_skills_to_dir,
            batch_apply_skills_to_agents,
            batch_set_skill_tag,
            preview_cc_switch_skills,
            migrate_cc_switch_skills,
            list_claude_environments,
            sniff_claude_environments,
            import_claude_environment,
            clone_claude_environment,
            upsert_claude_environment,
            delete_claude_environment,
            install_claude_env_alias,
            remove_claude_env_alias,
            remove_all_claude_env_aliases,
            get_claude_env_shell_status,
            reveal_claude_env_dir,
            open_claude_env_settings,
            get_claude_env_secret,
            sync_claude_env_mcp,
            sync_claude_env_skills,
            sync_all_claude_env_mcp,
            get_claude_env_mcp_status,
            fetch_claude_env_remote_models,
            list_codex_environments,
            sniff_codex_environments,
            import_codex_environment,
            clone_codex_environment,
            upsert_codex_environment,
            delete_codex_environment,
            install_codex_env_alias,
            remove_codex_env_alias,
            remove_all_codex_env_aliases,
            get_codex_env_shell_status,
            reveal_codex_env_dir,
            open_codex_env_config,
            get_codex_env_secret,
            sync_codex_env_mcp,
            sync_codex_env_skills,
            sync_all_codex_env_mcp,
            fetch_codex_env_remote_models,
            get_opencode_config,
            set_opencode_defaults,
            upsert_opencode_provider,
            delete_opencode_provider,
            upsert_opencode_model,
            delete_opencode_model,
            get_opencode_provider_secret,
            set_opencode_provider_secret,
            fetch_models_dev_catalog,
            probe_opencode_models_endpoint,
            reveal_opencode_config,
            get_opencode_fork_sync_status,
            sync_opencode_to_fork,
            sync_opencode_to_all_forks,
            pick_project_folder,
            check_project_config_exists,
            init_project_config,
            list_ai_providers,
            upsert_ai_provider,
            delete_ai_provider,
            get_ai_provider_secret,
            reorder_ai_providers,
            get_route_aggregation_status,
            get_route_aggregation_config,
            update_route_aggregation_config,
            start_route_aggregation,
            stop_route_aggregation,
            get_provider_route_toggles,
            toggle_provider_route,
            reorder_provider_routes,
            reset_circuit_breaker,
            get_circuit_breaker_status,
        ])
        .setup(|_app| {
            use tauri::Manager;
            // Ensure ~/.agentbuddy, skills/, and config.json exist before the UI loads.
            if let Err(err) = config::ensure_app_config() {
                eprintln!("[agent-buddy] failed to ensure app config: {}", err);
            }

            // 清除已从注册表移除的 agent 的历史扫描缓存（save_agents 只 upsert 不删除，
            // 否则 Agent 管理页会一直展示 kiro / codebuddy 等已下线的 agent）。
            if let Err(err) = db::purge_removed_agents(&["kiro", "codebuddy"]) {
                eprintln!("[agent-buddy] failed to purge removed agents: {}", err);
            }

            // Route aggregation: load config and register global state.
            let ra_config = route_aggregation::config::load_config()
                .unwrap_or_default();
            let ra_state = route_aggregation::RouteAggregationState::new(ra_config.clone());
            _app.manage(ra_state);

            // Auto-start server if it was running when the app last exited.
            if ra_config.claude_code_enabled || ra_config.codex_enabled {
                let app_handle = _app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<route_aggregation::RouteAggregationState>();
                    let config = state.config.read().await.clone();
                    // Port pre-check
                    let test_addr = format!("{}:{}", config.listen_address, config.listen_port);
                    if std::net::TcpListener::bind(&test_addr).is_err() {
                        eprintln!(
                            "[route-aggregation] port {} in use, skipping auto-start",
                            config.listen_port
                        );
                        return;
                    }
                    // Refresh pools
                    if config.claude_code_enabled {
                        let _ = state.provider_router
                            .refresh_pool(route_aggregation::RouteGroup::ClaudeCode)
                            .await;
                    }
                    if config.codex_enabled {
                        let _ = state.provider_router
                            .refresh_pool(route_aggregation::RouteGroup::Codex)
                            .await;
                    }
                    match route_aggregation::server::RouteAggregationServer::start(
                        config,
                        state.provider_router.clone(),
                    )
                    .await
                    {
                        Ok(server) => {
                            *state.server.write().await = Some(std::sync::Arc::new(server));
                            eprintln!("[route-aggregation] auto-started successfully");
                        }
                        Err(e) => {
                            eprintln!("[route-aggregation] auto-start failed: {}", e);
                        }
                    }
                });
            }

            // macOS: fix inactive traffic lights rendering as black instead of gray
            // when using Overlay titleBarStyle. Three things are needed:
            // 1. setOpaque:NO — lets the system composite the title bar correctly
            //    when titlebarAppearsTransparent is true
            // 2. setBackgroundColor: — gives the system a baseline color
            // 3. setAppearance: — set initial appearance from saved theme so
            //    inactive traffic lights use the correct shade (gray for dark
            //    themes, light-gray for light themes). Frontend will call
            //    set_window_appearance to keep it in sync on theme change.
            #[cfg(target_os = "macos")]
            {
                use objc::{class, msg_send, sel, sel_impl};
                use tauri::Manager;
                if let Some(window) = _app.get_webview_window("main") {
                    if let Ok(ns_window_ptr) = window.ns_window() {
                        unsafe {
                            let ns_window = ns_window_ptr as *mut objc::runtime::Object;

                            // 1. setOpaque:NO
                            let _: () = msg_send![ns_window, setOpaque: false];

                            // 2. setBackgroundColor: dark gray (#2C2C2C)
                            let bg_color: *mut objc::runtime::Object = msg_send![
                                class!(NSColor),
                                colorWithCalibratedRed: 0.17f64
                                green: 0.17f64
                                blue: 0.17f64
                                alpha: 1.0f64
                            ];
                            let _: () = msg_send![
                                ns_window,
                                setBackgroundColor: bg_color
                            ];
                        }
                    }

                    // 3. setAppearance based on saved theme (dispatched to main thread)
                    let theme = config::load_app_config()
                        .map(|c| c.theme)
                        .unwrap_or_else(|_| "qoder-light".to_string());
                    let app_handle = _app.handle().clone();
                    set_ns_window_appearance(&app_handle, theme_is_dark(&theme));
                }
            }

            // DevTools only in debug builds; Manager is only needed here.
            #[cfg(debug_assertions)]
            {
                if let Some(window) = _app.get_webview_window("main") {
                    window.open_devtools();
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
