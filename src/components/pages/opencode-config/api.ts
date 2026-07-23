/** OpenCode 配置页的 Tauri 命令封装。 */

import type {
  ModelsDevCatalog,
  OpencodeActionResult,
  OpencodeConfigView,
  OpencodeForkSyncResult,
  OpencodeForkSyncStatus,
  ProbeModelsResult,
  SetDefaultsPayload,
  UpsertModelPayload,
  UpsertProviderPayload,
} from "./types";

export async function invokeGetConfig(): Promise<OpencodeConfigView> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_opencode_config") as Promise<OpencodeConfigView>;
}

export async function invokeSetDefaults(
  payload: SetDefaultsPayload,
): Promise<OpencodeActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("set_opencode_defaults", { payload }) as Promise<OpencodeActionResult>;
}

export async function invokeUpsertProvider(
  payload: UpsertProviderPayload,
): Promise<OpencodeActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("upsert_opencode_provider", { payload }) as Promise<OpencodeActionResult>;
}

export async function invokeDeleteProvider(
  providerId: string,
  deleteAuth: boolean,
): Promise<OpencodeActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_opencode_provider", {
    providerId,
    deleteAuth,
  }) as Promise<OpencodeActionResult>;
}

export async function invokeUpsertModel(
  payload: UpsertModelPayload,
): Promise<OpencodeActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("upsert_opencode_model", { payload }) as Promise<OpencodeActionResult>;
}

export async function invokeDeleteModel(
  providerId: string,
  modelId: string,
): Promise<OpencodeActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_opencode_model", {
    providerId,
    modelId,
  }) as Promise<OpencodeActionResult>;
}

export async function invokeGetSecret(providerId: string): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_opencode_provider_secret", { providerId }) as Promise<string>;
}

export async function invokeSetSecret(
  providerId: string,
  apiKey: string,
): Promise<OpencodeActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("set_opencode_provider_secret", {
    providerId,
    apiKey,
  }) as Promise<OpencodeActionResult>;
}

export async function invokeFetchCatalog(force = false): Promise<ModelsDevCatalog> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("fetch_models_dev_catalog", { force }) as Promise<ModelsDevCatalog>;
}

export async function invokeProbeModels(baseUrl: string): Promise<ProbeModelsResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("probe_opencode_models_endpoint", { baseUrl }) as Promise<ProbeModelsResult>;
}

export async function invokeRevealConfig(): Promise<OpencodeActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reveal_opencode_config") as Promise<OpencodeActionResult>;
}

export async function invokeGetForkSyncStatus(): Promise<OpencodeForkSyncStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_opencode_fork_sync_status") as Promise<OpencodeForkSyncStatus>;
}

export async function invokeSyncToFork(
  agent: string,
  syncMcp = false,
  syncSkills = false,
): Promise<OpencodeForkSyncResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sync_opencode_to_fork", {
    agent,
    syncMcp,
    syncSkills,
  }) as Promise<OpencodeForkSyncResult>;
}

export async function invokeSyncToAllForks(
  syncMcp = false,
  syncSkills = false,
): Promise<OpencodeForkSyncResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("sync_opencode_to_all_forks", {
    syncMcp,
    syncSkills,
  }) as Promise<OpencodeForkSyncResult>;
}
