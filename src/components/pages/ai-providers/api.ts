/** AI 供应商管理页的 Tauri 命令封装。 */

import type {
  AiProvider,
  AiProviderActionResult,
  AiProviderUpsertPayload,
  ProviderType,
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

/** 从 Base URL 拉取远端模型列表（复用 Claude/Codex 环境页的同一个命令）。
 * `providerType` 可选：传入 `google-generative-ai` 时后端走 Google 专用路径
 * （`x-goog-api-key` header + 解析 `models[].name`）；其他类型或缺省走 OpenAI/
 * Anthropic 通用解析逻辑。 */
export async function invokeFetchRemoteModels(
  baseUrl: string,
  apiKey?: string,
  providerType?: ProviderType,
): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const result = (await invoke("fetch_claude_env_remote_models", {
    baseUrl,
    apiKey,
    providerType,
  })) as { modelIds: string[] };
  return result.modelIds;
}

/** 批量更新供应商排序（ids 按目标顺序排列）。 */
export async function invokeReorder(ids: string[]): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reorder_ai_providers", { ids }) as Promise<void>;
}
