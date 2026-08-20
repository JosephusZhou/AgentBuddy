// Suppress `unexpected cfg` warnings from the `objc` 0.2 crate macros
// (msg_send!, class! use `cfg!(feature = "cargo-clippy")` which is outdated).
#![allow(unexpected_cfgs)]

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
mod agent_model_config;
mod opencode_config;
mod pi_model_config;
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
    overwrite_ids: Option<Vec<String>>,
) -> Result<skills::SkillActionResult, String> {
    let tag = tag.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || skills::add_skill_local(path, tag, overwrite_ids))
        .await
        .map_err(|e| format!("导入本地 skill 任务失败: {}", e))?
}

#[tauri::command]
async fn check_skill_local_duplicate(
    path: String,
) -> Result<skills::SkillDuplicateCheckResult, String> {
    tauri::async_runtime::spawn_blocking(move || skills::check_skill_local_duplicate(path))
        .await
        .map_err(|e| format!("检查本地 skill 重复任务失败: {}", e))?
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
async fn pick_skill_folder_path(title: Option<String>) -> Result<Option<String>, String> {
    let title = title.unwrap_or_else(|| "选择目录".into());
    let result = tauri::async_runtime::spawn_blocking(move || crate::platform::pick_folder(&title))
        .await
        .map_err(|e| format!("选择目录任务失败: {}", e))??;
    Ok(result.map(|p| p.to_string_lossy().to_string()))
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
async fn update_skills_batch(ids: Vec<String>) -> Result<skills::BatchSkillResult, String> {
    tauri::async_runtime::spawn_blocking(move || skills::update_skills_batch(ids))
        .await
        .map_err(|e| format!("批量更新 skills 任务失败: {}", e))?
}

#[tauri::command]
async fn check_export_duplicates(
    skill_ids: Vec<String>,
) -> Result<skills::ExportDuplicateCheckResult, String> {
    tauri::async_runtime::spawn_blocking(move || skills::check_export_duplicates(skill_ids))
        .await
        .map_err(|e| format!("检查导出重复任务失败: {}", e))?
}

#[tauri::command]
async fn export_skills_to_dir(
    skill_ids: Vec<String>,
    install_mode: Option<String>,
    target_dir: String,
    overwrite_ids: Option<Vec<String>>,
) -> Result<skills::BatchSkillResult, String> {
    let install_mode = skills::SkillInstallMode::from_wire(install_mode.as_deref());
    let overwrite_ids = overwrite_ids.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        skills::export_skills_to_dir(skill_ids, install_mode, target_dir, overwrite_ids)
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

/// 远端拉取模型列表。仅服务于"临时配置"路径：Claude 环境 / Codex 环境 / 模型配置页
/// （OpenCode / Pi / Oh-My-Pi）在用户不关联 AI 供应商库、手填 baseUrl + apiKey 时调用。
/// **不**适用于"已配置 AI 供应商"——后者以 `custom_models_json` 为唯一来源。
#[tauri::command]
async fn fetch_claude_env_remote_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<claude_env::ClaudeEnvRemoteModelsResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        claude_env::fetch_remote_models(base_url, api_key)
    })
    .await
    .map_err(|e| format!("拉取远端模型任务失败: {e}"))?
}

/* ===== Agent 通用模型配置（OpenCode / Pi / Oh-My-Pi）===== */

#[tauri::command]
async fn get_agent_model_config(
    agent: String,
) -> Result<opencode_config::AgentModelConfigView, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::get_config(),
        a => pi_model_config::get_config(a),
    }
}

#[tauri::command]
async fn set_agent_model_defaults(
    agent: String,
    payload: opencode_config::SetDefaultsPayload,
) -> Result<opencode_config::AgentActionResult, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::set_defaults(payload),
        a => pi_model_config::set_defaults(a, payload),
    }
}

#[tauri::command]
async fn upsert_agent_provider(
    agent: String,
    payload: opencode_config::UpsertProviderPayload,
) -> Result<opencode_config::AgentActionResult, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::upsert_provider(payload),
        a => pi_model_config::upsert_provider(a, payload),
    }
}

#[tauri::command]
async fn delete_agent_provider(
    agent: String,
    provider_id: String,
    delete_auth: bool,
) -> Result<opencode_config::AgentActionResult, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::delete_provider(provider_id, delete_auth),
        a => pi_model_config::delete_provider(a, provider_id, delete_auth),
    }
}

#[tauri::command]
async fn upsert_agent_model(
    agent: String,
    payload: opencode_config::UpsertModelPayload,
) -> Result<opencode_config::AgentActionResult, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::upsert_model(payload),
        a => pi_model_config::upsert_model(a, payload),
    }
}

#[tauri::command]
async fn delete_agent_model(
    agent: String,
    provider_id: String,
    model_id: String,
) -> Result<opencode_config::AgentActionResult, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::delete_model(provider_id, model_id),
        a => pi_model_config::delete_model(a, provider_id, model_id),
    }
}

#[tauri::command]
async fn get_agent_provider_secret(agent: String, provider_id: String) -> Result<String, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::get_provider_secret(provider_id),
        a => pi_model_config::get_provider_secret(a, provider_id),
    }
}

#[tauri::command]
async fn set_agent_provider_secret(
    agent: String,
    provider_id: String,
    api_key: String,
) -> Result<opencode_config::AgentActionResult, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::set_provider_secret(provider_id, api_key),
        a => pi_model_config::set_provider_secret(a, provider_id, api_key),
    }
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
async fn probe_models_endpoint(
    base_url: String,
) -> Result<opencode_config::ProbeModelsResult, String> {
    tauri::async_runtime::spawn_blocking(move || opencode_config::probe_models_endpoint(base_url))
        .await
        .map_err(|e| format!("探测任务失败: {e}"))?
}

#[tauri::command]
async fn reveal_agent_model_config(
    agent: String,
) -> Result<opencode_config::AgentActionResult, String> {
    use agent_model_config::ModelConfigAgent;
    match ModelConfigAgent::parse(&agent)? {
        ModelConfigAgent::Opencode => opencode_config::reveal_config(),
        a => pi_model_config::reveal_config(a),
    }
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

/// 远端拉取模型列表（Codex 环境专用变体；走 OpenAI 兼容路径）。
#[tauri::command]
async fn fetch_codex_env_remote_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<claude_env::ClaudeEnvRemoteModelsResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        claude_env::fetch_remote_models(base_url, api_key)
    })
    .await
    .map_err(|e| format!("拉取 Codex 远端模型任务失败: {e}"))?
}

/* ===== AI providers (上游模型供应商库) ===== */

/// 写操作后异步触发路由聚合内存池刷新，避免「刚加/改了供应商 → 下游
/// `/v1/models` 或模型配置页的虚拟供应商看到的还是旧 pool」这种隐性脏数据。
///
/// 用后台 spawn 而不是 inline await：
/// - 不阻塞 AI 供应商主命令的返回（用户操作的 UX 优先）；
/// - 即使刷新失败也不影响供应商写入的最终一致性，下次任一相关命令会兜底刷一次。
fn spawn_route_aggregation_pool_refresh(
    router: std::sync::Arc<route_aggregation::provider_router::ProviderRouter>,
) {
    // Refresh the provider pool from DB rows without network I/O. This keeps
    // provider changes visible immediately without blocking on remote calls.
    tauri::async_runtime::spawn(async move {
        for group in route_aggregation::RouteGroup::ALL {
            if let Err(e) = router.refresh_pool_fast(group).await {
                eprintln!(
                    "[ai-provider] 刷新路由聚合 {:?} pool 失败: {}",
                    group, e
                );
                continue;
            }
        }
    });
}

#[tauri::command]
async fn list_ai_providers() -> Result<Vec<ai_provider::AiProvider>, String> {
    tauri::async_runtime::spawn_blocking(ai_provider::list_providers)
        .await
        .map_err(|e| format!("列出 AI 供应商任务失败: {e}"))?
}

#[tauri::command]
async fn upsert_ai_provider(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    payload: ai_provider::AiProviderUpsertPayload,
) -> Result<ai_provider::AiProviderActionResult, String> {
    let result = tauri::async_runtime::spawn_blocking(move || ai_provider::upsert_provider(payload))
        .await
        .map_err(|e| format!("保存 AI 供应商任务失败: {e}"))??;
    spawn_route_aggregation_pool_refresh(state.provider_router.clone());
    Ok(result)
}

#[tauri::command]
async fn delete_ai_provider(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    id: String,
) -> Result<ai_provider::AiProviderActionResult, String> {
    let result = tauri::async_runtime::spawn_blocking(move || ai_provider::delete_provider(id))
        .await
        .map_err(|e| format!("删除 AI 供应商任务失败: {e}"))??;
    spawn_route_aggregation_pool_refresh(state.provider_router.clone());
    Ok(result)
}

#[tauri::command]
async fn get_ai_provider_secret(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || ai_provider::get_provider_secret(id))
        .await
        .map_err(|e| format!("读取供应商密钥任务失败: {e}"))?
}

#[tauri::command]
async fn get_ai_provider_secrets(id: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || ai_provider::get_provider_secrets(id))
        .await
        .map_err(|e| format!("读取供应商密钥任务失败: {e}"))?
}

#[tauri::command]
async fn reorder_ai_providers(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    ids: Vec<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || ai_provider::reorder_providers(ids))
        .await
        .map_err(|e| format!("供应商排序任务失败: {e}"))??;
    spawn_route_aggregation_pool_refresh(state.provider_router.clone());
    Ok(())
}

/* ===== Route aggregation (local proxy server) ===== */

#[tauri::command]
async fn get_route_aggregation_status(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<route_aggregation::RouteAggregationStatus, String> {
    // Refresh pools so newly added/toggled providers show up without restart.
    for group in route_aggregation::RouteGroup::ALL {
        state.provider_router.refresh_pool_fast(group).await?;
    }
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

    // Check if listen address/port changed — only that requires a server restart.
    // Group toggles and other non-structural changes are handled at runtime:
    // handlers read the shared config on each request.
    let needs_restart = {
        let old = state.config.read().await;
        let addr_changed = old.listen_address != config.listen_address
            || old.listen_port != config.listen_port;
        drop(old);
        addr_changed && state.server.read().await.is_some()
    };

    // Update the shared config — handlers will see the new values immediately.
    *state.config.write().await = config.clone();

    if needs_restart {
        // Stop existing server (await full shutdown so port is released)
        if let Some(server) = state.server.write().await.take() {
            server.stop().await;
        }
        // Restart on the new address/port
        let server = route_aggregation::server::RouteAggregationServer::start(
            state.config.clone(),
            state.provider_router.clone(),
            state.log_store.clone(),
        )
        .await?;
        *state.server.write().await = Some(std::sync::Arc::new(server));
    }

    // Refresh provider pools for both API formats
    for group in route_aggregation::RouteGroup::ALL {
        state.provider_router.refresh_pool_fast(group).await?;
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

    // Refresh provider pools for both API formats
    for group in route_aggregation::RouteGroup::ALL {
        state.provider_router.refresh_pool_fast(group).await?;
    }

    let server = route_aggregation::server::RouteAggregationServer::start(
        state.config.clone(),
        state.provider_router.clone(),
        state.log_store.clone(),
    )
    .await?;
    *state.server.write().await = Some(std::sync::Arc::new(server));

    // Remember that the server was running, so it auto-starts next launch.
    {
        let mut config = state.config.write().await;
        config.auto_start = true;
        route_aggregation::config::save_config(&config)?;
    }

    Ok(state.get_status().await)
}

#[tauri::command]
async fn stop_route_aggregation(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<(), String> {
    if let Some(server) = state.server.write().await.take() {
        server.stop().await;
    }
    let mut config = state.config.write().await;
    config.auto_start = false;
    route_aggregation::config::save_config(&config)?;
    Ok(())
}

#[tauri::command]
async fn toggle_provider_route(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    provider_id: String,
    enabled: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        db::upsert_provider_route_toggle(&provider_id, enabled, 0)
    })
    .await
    .map_err(|e| format!("切换供应商路由开关任务失败: {e}"))??;
    // Refresh in-memory provider pools so status reflects the new toggle state
    for group in route_aggregation::RouteGroup::ALL {
        state.provider_router.refresh_pool_fast(group).await?;
    }
    Ok(())
}

#[tauri::command]
async fn reset_circuit_breaker(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    provider_id: String,
) -> Result<(), String> {
    for group in route_aggregation::RouteGroup::ALL {
        state.provider_router.reset_breaker(&provider_id, group).await;
    }
    Ok(())
}

/// Add a new endpoint API key (generated, appended and persisted).
/// Returns the newly generated key.
#[tauri::command]
async fn add_route_aggregation_api_key(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<String, String> {
    let api_key = route_aggregation::config::generate_api_key();
    let mut config = state.config.write().await;
    config.api_keys.push(api_key.clone());
    route_aggregation::config::save_config(&config)?;
    Ok(api_key)
}

/// Delete an endpoint API key by index.
/// The primary key (index 0) cannot be deleted — only regenerated.
#[tauri::command]
async fn delete_route_aggregation_api_key(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    index: usize,
) -> Result<(), String> {
    let mut config = state.config.write().await;
    if index == 0 {
        return Err("主 API Key 不能删除，只能重新生成".to_string());
    }
    if index >= config.api_keys.len() {
        return Err(format!("无效的 API Key 索引: {}", index));
    }
    config.api_keys.remove(index);
    route_aggregation::config::save_config(&config)?;
    Ok(())
}

/// Regenerate the endpoint API key at the given index.
/// Returns the newly generated key.
#[tauri::command]
async fn regenerate_route_aggregation_api_key(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
    index: usize,
) -> Result<String, String> {
    let api_key = route_aggregation::config::generate_api_key();
    let mut config = state.config.write().await;
    if index >= config.api_keys.len() {
        return Err(format!("无效的 API Key 索引: {}", index));
    }
    config.api_keys[index] = api_key.clone();
    route_aggregation::config::save_config(&config)?;
    Ok(api_key)
}

/// Snapshot of all in-memory route aggregation log entries, newest last.
/// Returned to the UI's "进出日志" list. Empty when the server isn't running.
#[tauri::command]
async fn get_route_aggregation_logs(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<Vec<route_aggregation::LogEntry>, String> {
    Ok(state.log_store.snapshot().await)
}

/// Clear all in-memory route aggregation log entries.
#[tauri::command]
async fn clear_route_aggregation_logs(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<(), String> {
    state.log_store.clear().await;
    Ok(())
}

/// Path of the on-disk route aggregation log file, if a sink was attached
/// during setup. The file is JSONL (one entry per line) and is the durable
/// source of truth — the in-memory ring is just a fast UI window.
#[tauri::command]
fn get_route_aggregation_log_file_path(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Option<String> {
    state
        .log_store
        .file_path()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Reveal the route aggregation log file in the OS file manager (Finder on
/// macOS, Explorer on Windows). Soft-fails if the file hasn't been created
/// yet (e.g. nobody has hit the proxy).
#[tauri::command]
fn reveal_route_aggregation_log_file(
    state: tauri::State<'_, route_aggregation::RouteAggregationState>,
) -> Result<(), String> {
    let path = state
        .log_store
        .file_path()
        .ok_or_else(|| "本地日志文件未挂载".to_string())?;
    crate::platform::reveal_path(&path)
}

/// Get the effective model list of a provider for the route aggregation UI.
///
/// 来源唯一：`ai_providers.custom_models_json`（用户在 AI 供应商编辑页手动配置的
/// 模型列表）。即使该列表为空也**不**再向供应商远端 /v1/models 拉取——配置侧的
/// 自定义列表即为对外暴露的全部模型。
#[tauri::command]
async fn get_route_provider_models(
    provider_id: String,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let rows = db::load_ai_provider_rows()?;
        let row = rows
            .iter()
            .find(|r| r.id == provider_id)
            .ok_or_else(|| format!("未找到供应商: {}", provider_id))?;

        // 唯一来源：custom_models_json。alias_id 优先于 model；空列表直接返回空。
        let custom: Vec<ai_provider::CustomModel> =
            serde_json::from_str(&row.custom_models_json).unwrap_or_default();
        Ok(custom
            .into_iter()
            .map(|m| {
                if m.alias_id.trim().is_empty() {
                    m.model
                } else {
                    m.alias_id
                }
            })
            .filter(|id| !id.trim().is_empty())
            .collect())
    })
    .await
    .map_err(|e| format!("获取供应商模型列表任务失败: {e}"))?
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
            pick_skill_folder_path,
            check_skill_local_duplicate,
            add_skill_github,
            add_skill_gitcode,
            update_skill,
            update_skills_batch,
            check_export_duplicates,
            export_skills_to_dir,
            open_external_url,
            delete_skill,
            apply_skill_to_agents,
            batch_delete_skills,
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
            get_agent_model_config,
            set_agent_model_defaults,
            upsert_agent_provider,
            delete_agent_provider,
            upsert_agent_model,
            delete_agent_model,
            get_agent_provider_secret,
            set_agent_provider_secret,
            fetch_models_dev_catalog,
            probe_models_endpoint,
            reveal_agent_model_config,
            pick_project_folder,
            check_project_config_exists,
            init_project_config,
            list_ai_providers,
            upsert_ai_provider,
            delete_ai_provider,
            get_ai_provider_secret,
            get_ai_provider_secrets,
            reorder_ai_providers,
            get_route_aggregation_status,
            get_route_aggregation_config,
            update_route_aggregation_config,
            start_route_aggregation,
            stop_route_aggregation,
            toggle_provider_route,
            reset_circuit_breaker,
            add_route_aggregation_api_key,
            delete_route_aggregation_api_key,
            regenerate_route_aggregation_api_key,
            get_route_provider_models,
            get_route_aggregation_logs,
            clear_route_aggregation_logs,
            get_route_aggregation_log_file_path,
            reveal_route_aggregation_log_file,
        ])
        .setup(|_app| {
            use tauri::Manager;
            // Ensure ~/.agentbuddy, skills/, and config.json exist before the UI loads.
            if let Err(err) = config::ensure_app_config() {
                eprintln!("[agent-buddy] failed to ensure app config: {}", err);
            }

            // Models.dev 目录由后端统一维护：启动时优先使用 7 天内的磁盘缓存，
            // 过期或缺失时后台刷新一次，避免各页面首次打开时重复请求。
            tauri::async_runtime::spawn_blocking(|| {
                if let Err(err) = opencode_config::fetch_models_dev_catalog(false) {
                    eprintln!("[agent-buddy] Models.dev catalog refresh failed: {}", err);
                }
            });

            // 清除已从注册表移除的 agent 的历史扫描缓存（save_agents 只 upsert 不删除，
            // 否则 Agent 管理页会一直展示 kiro / codebuddy / deveco-code 等已下线的 agent）。
            if let Err(err) = db::purge_removed_agents(&["kiro", "codebuddy", "deveco-code"]) {
                eprintln!("[agent-buddy] failed to purge removed agents: {}", err);
            }

            // Route aggregation: load config and register global state.
            let mut ra_config = route_aggregation::config::load_config()
                .unwrap_or_default();
            // Auto-generate the primary endpoint API key on first use.
            if ra_config.api_keys.is_empty() {
                ra_config
                    .api_keys
                    .push(route_aggregation::config::generate_api_key());
                let _ = route_aggregation::config::save_config(&ra_config);
            }
            let ra_state = route_aggregation::RouteAggregationState::new(ra_config.clone());
            // Attach an on-disk log file so the user can `tail -f` the route
            // aggregation traffic without depending on the UI. Best-effort:
            // if the app data dir isn't writable we just skip the sink.
            if let Ok(dir) = crate::platform::app_data_dir() {
                let log_path = dir.join("logs").join("route_aggregation.log");
                if let Some(file) = route_aggregation::LogFile::open(&log_path) {
                    eprintln!(
                        "[route-aggregation] logging to {}",
                        file.path().display()
                    );
                    ra_state.log_store.attach_file(file);
                }
            }
            _app.manage(ra_state);

            // Auto-start server if it was running when the app last exited.
            if ra_config.auto_start {
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
                    // Refresh pools for both API formats
                    for group in route_aggregation::RouteGroup::ALL {
                        let _ = state.provider_router.refresh_pool_fast(group).await;
                    }
                    match route_aggregation::server::RouteAggregationServer::start(
                        state.config.clone(),
                        state.provider_router.clone(),
                        state.log_store.clone(),
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
