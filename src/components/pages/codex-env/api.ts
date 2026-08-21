/** CodexEnv 页的 Tauri 命令封装。 */

import type {
  CodexEnvironment,
  CodexEnvSniffResult,
  CodexEnvActionResult,
  CodexEnvShellStatus,
  CodexEnvMcpSyncResult,
} from "./types";

export async function invokeList(): Promise<CodexEnvironment[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("list_codex_environments") as Promise<CodexEnvironment[]>;
}

export async function invokeSniff(): Promise<CodexEnvSniffResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sniff_codex_environments") as Promise<CodexEnvSniffResult>;
}

export async function invokeImport(payload: {
  configDir: string;
  name?: string;
  slug?: string;
  aliasName?: string;
  notes?: string;
  installAlias?: boolean;
}): Promise<CodexEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("import_codex_environment", { payload }) as Promise<CodexEnvActionResult>;
}

export async function invokeClone(payload: {
  sourceId: string;
  name: string;
  slug: string;
  configDir: string;
  aliasName: string;
  notes?: string;
  model?: string;
  modelProvider?: string;
  baseUrl?: string;
  apiKey?: string;
  syncMcp?: boolean;
  syncSkills?: boolean;
  syncAgents?: boolean;
  syncOtherData?: boolean;
  syncMode?: "full" | "symlink";
  installAlias?: boolean;
  providerId?: string;
}): Promise<CodexEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("clone_codex_environment", { payload }) as Promise<CodexEnvActionResult>;
}

export async function invokeUpsert(payload: {
  id?: string;
  name: string;
  slug: string;
  configDir: string;
  aliasName: string;
  notes?: string;
  // 三态：undefined=不改动，""=删除该键，"值"=写入。
  model?: string;
  modelProvider?: string;
  baseUrl?: string;
  apiKey?: string;
  providerId?: string;
}): Promise<CodexEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("upsert_codex_environment", { payload }) as Promise<CodexEnvActionResult>;
}

export async function invokeDelete(id: string, deleteFiles: boolean): Promise<CodexEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_codex_environment", { id, deleteFiles }) as Promise<CodexEnvActionResult>;
}

export async function invokeInstallEnvAlias(id: string): Promise<CodexEnvShellStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("install_codex_env_alias", { id }) as Promise<CodexEnvShellStatus>;
}

export async function invokeRemoveEnvAlias(id: string): Promise<CodexEnvShellStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("remove_codex_env_alias", { id }) as Promise<CodexEnvShellStatus>;
}

export async function invokeRemoveAllAliases(): Promise<CodexEnvShellStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("remove_all_codex_env_aliases") as Promise<CodexEnvShellStatus>;
}

export async function invokeShellStatus(): Promise<CodexEnvShellStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_codex_env_shell_status") as Promise<CodexEnvShellStatus>;
}

export async function invokeReveal(id: string): Promise<CodexEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reveal_codex_env_dir", { id }) as Promise<CodexEnvActionResult>;
}

export async function invokeOpenConfig(id: string): Promise<CodexEnvActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("open_codex_env_config", { id }) as Promise<CodexEnvActionResult>;
}

export async function invokeGetSecret(id: string): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_codex_env_secret", { id }) as Promise<string>;
}

/**
 * 从当前 Codex 环境的 Base URL 拉取远端模型列表。
 *
 * **临时配置专用**：当 Codex 环境**不**关联 AI 供应商库、手填 baseUrl + apiKey 时调用。
 * 若 env 已关联供应商，则应优先使用该供应商的 `customModels`（由调用方在 UI 层处理）。
 */
export async function invokeFetchRemoteModels(baseUrl: string, apiKey?: string): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const result = await invoke("fetch_codex_env_remote_models", { baseUrl, apiKey }) as {
    modelIds: string[];
  };
  return result.modelIds;
}

export async function invokeSyncMcp(id: string): Promise<CodexEnvMcpSyncResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sync_codex_env_mcp", { id }) as Promise<CodexEnvMcpSyncResult>;
}

export async function invokeSyncSkills(id: string): Promise<{ ok: boolean; message: string; skillCount: number }> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sync_codex_env_skills", { id }) as Promise<{ ok: boolean; message: string; skillCount: number }>;
}

export async function invokeSyncAllMcp(): Promise<CodexEnvMcpSyncResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sync_all_codex_env_mcp") as Promise<CodexEnvMcpSyncResult>;
}
