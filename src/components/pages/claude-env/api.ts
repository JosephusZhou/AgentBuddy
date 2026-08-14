/** ClaudeEnv 页的 Tauri 命令封装（从页面组件抽出）。 */

import type {
  ClaudeEnvironment,
  ClaudeEnvSniffResult,
  ClaudeEnvActionResult,
  ClaudeEnvShellStatus,
  ClaudeEnvMcpSyncResult,
} from "./types";

export async function invokeList(): Promise<ClaudeEnvironment[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("list_claude_environments") as Promise<ClaudeEnvironment[]>;
}

export async function invokeSniff(): Promise<ClaudeEnvSniffResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sniff_claude_environments") as Promise<ClaudeEnvSniffResult>;
}

export async function invokeImport(payload: {
  configDir: string;
  name?: string;
  slug?: string;
  aliasName?: string;
  notes?: string;
  installAlias?: boolean;
}): Promise<ClaudeEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("import_claude_environment", { payload }) as Promise<ClaudeEnvActionResult>;
}

export async function invokeClone(payload: {
  sourceId: string;
  name: string;
  slug: string;
  configDir: string;
  aliasName: string;
  notes?: string;
  baseUrl?: string;
  apiKey?: string;
  model?: string;
  // 四档模型覆盖：留空=跟随主模型，非空=写入该档覆盖。仅 model 写入时生效。
  modelHaiku?: string;
  modelSonnet?: string;
  modelOpus?: string;
  modelFable?: string;
  syncMcp?: boolean;
  syncSkills?: boolean;
  syncAgents?: boolean;
  installAlias?: boolean;
  providerId?: string;
}): Promise<ClaudeEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("clone_claude_environment", { payload }) as Promise<ClaudeEnvActionResult>;
}

export async function invokeUpsert(payload: {
  id?: string;
  name: string;
  slug: string;
  configDir: string;
  aliasName: string;
  notes?: string;
  // 三态：undefined=不改动，""=删除该键，"值"=写入。
  baseUrl?: string;
  apiKey?: string;
  model?: string;
  // 四档模型覆盖：留空=跟随主模型，非空=写入该档覆盖。写入时按档取值，留空的档回退到主模型。
  modelHaiku?: string;
  modelSonnet?: string;
  modelOpus?: string;
  modelFable?: string;
  providerId?: string;
}): Promise<ClaudeEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("upsert_claude_environment", { payload }) as Promise<ClaudeEnvActionResult>;
}

export async function invokeDelete(id: string, deleteFiles: boolean): Promise<ClaudeEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_claude_environment", { id, deleteFiles }) as Promise<ClaudeEnvActionResult>;
}

export async function invokeInstallEnvAlias(id: string): Promise<ClaudeEnvShellStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("install_claude_env_alias", { id }) as Promise<ClaudeEnvShellStatus>;
}

export async function invokeRemoveEnvAlias(id: string): Promise<ClaudeEnvShellStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("remove_claude_env_alias", { id }) as Promise<ClaudeEnvShellStatus>;
}

export async function invokeRemoveAllAliases(): Promise<ClaudeEnvShellStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("remove_all_claude_env_aliases") as Promise<ClaudeEnvShellStatus>;
}

export async function invokeShellStatus(): Promise<ClaudeEnvShellStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_claude_env_shell_status") as Promise<ClaudeEnvShellStatus>;
}

export async function invokeReveal(id: string): Promise<ClaudeEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reveal_claude_env_dir", { id }) as Promise<ClaudeEnvActionResult>;
}

export async function invokeOpenSettings(id: string): Promise<ClaudeEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("open_claude_env_settings", { id }) as Promise<ClaudeEnvActionResult>;
}

export async function invokeGetSecret(id: string): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_claude_env_secret", { id }) as Promise<string>;
}

/**
 * 从当前环境的 Base URL 拉取远端模型列表。
 *
 * **临时配置专用**：当 Claude 环境**不**关联 AI 供应商库、手填 baseUrl + apiKey 时调用。
 * 若 env 已关联供应商，则应优先使用该供应商的 `customModels`（由调用方在 UI 层处理）。
 */
export async function invokeFetchRemoteModels(baseUrl: string, apiKey?: string): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const result = await invoke("fetch_claude_env_remote_models", { baseUrl, apiKey }) as {
    modelIds: string[];
  };
  return result.modelIds;
}

export async function invokeSyncMcp(id: string): Promise<ClaudeEnvMcpSyncResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sync_claude_env_mcp", { id }) as Promise<ClaudeEnvMcpSyncResult>;
}

export async function invokeSyncSkills(id: string): Promise<{ ok: boolean; message: string; skillCount: number }> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sync_claude_env_skills", { id }) as Promise<{ ok: boolean; message: string; skillCount: number }>;
}

export async function invokeSyncAllMcp(): Promise<ClaudeEnvMcpSyncResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sync_all_claude_env_mcp") as Promise<ClaudeEnvMcpSyncResult>;
}
