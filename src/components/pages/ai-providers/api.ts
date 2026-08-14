/** AI 供应商管理页的 Tauri 命令封装。 */

import type {
  AiProvider,
  AiProviderActionResult,
  AiProviderUpsertPayload,
} from "./types";

export async function invokeList(): Promise<AiProvider[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("list_ai_providers") as Promise<AiProvider[]>;
}

export async function invokeUpsert(
  payload: AiProviderUpsertPayload,
): Promise<AiProviderActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("upsert_ai_provider", { payload }) as Promise<AiProviderActionResult>;
}

export async function invokeDelete(id: string): Promise<AiProviderActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_ai_provider", { id }) as Promise<AiProviderActionResult>;
}

/** 按需拉取明文 API Key（编辑表单用）；列表永不调用。 */
export async function invokeGetSecret(id: string): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_ai_provider_secret", { id }) as Promise<string>;
}

/** 按需拉取全部明文 API Key（编辑表单用，支持多 Key）。 */
export async function invokeGetSecrets(id: string): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_ai_provider_secrets", { id }) as Promise<string[]>;
}

/**
 * 从 Base URL 拉取远端模型列表。
 *
 * **临时配置专用**：当 Claude 环境 / Codex 环境 / 模型配置页（OpenCode / Pi / Oh-My-Pi）
 * 在用户不关联 AI 供应商库、手填 baseUrl + apiKey 时调用。
 *
 * **不**适用于 AI 供应商库（`ai_providers`）本身——后者以 `custom_models_json` 为
 * 唯一来源，**不**发起远程请求。
 *
 */
export async function invokeFetchRemoteModels(
  baseUrl: string,
  apiKey?: string,
): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const result = (await invoke("fetch_claude_env_remote_models", {
    baseUrl,
    apiKey,
  })) as { modelIds: string[] };
  return result.modelIds;
}

/** 批量更新供应商排序（ids 按目标顺序排列）。 */
export async function invokeReorder(ids: string[]): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reorder_ai_providers", { ids }) as Promise<void>;
}
