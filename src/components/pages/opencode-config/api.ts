/** 通用模型配置页（OpenCode / Pi / Oh-My-Pi）的 Tauri 命令封装。 */

import type {
  AgentActionResult,
  AgentModelConfigView,
  ModelsDevCatalog,
  ModelConfigAgentId,
  ProbeModelsResult,
  SetDefaultsPayload,
  UpsertModelPayload,
  UpsertProviderPayload,
} from "./types";

export async function invokeGetConfig(agent: ModelConfigAgentId): Promise<AgentModelConfigView> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_agent_model_config", { agent }) as Promise<AgentModelConfigView>;
}

export async function invokeSetDefaults(
  agent: ModelConfigAgentId,
  payload: SetDefaultsPayload,
): Promise<AgentActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("set_agent_model_defaults", { agent, payload }) as Promise<AgentActionResult>;
}

export async function invokeUpsertProvider(
  agent: ModelConfigAgentId,
  payload: UpsertProviderPayload,
): Promise<AgentActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("upsert_agent_provider", { agent, payload }) as Promise<AgentActionResult>;
}

export async function invokeDeleteProvider(
  agent: ModelConfigAgentId,
  providerId: string,
  deleteAuth: boolean,
): Promise<AgentActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_agent_provider", {
    agent,
    providerId,
    deleteAuth,
  }) as Promise<AgentActionResult>;
}

export async function invokeUpsertModel(
  agent: ModelConfigAgentId,
  payload: UpsertModelPayload,
): Promise<AgentActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("upsert_agent_model", { agent, payload }) as Promise<AgentActionResult>;
}

export async function invokeDeleteModel(
  agent: ModelConfigAgentId,
  providerId: string,
  modelId: string,
): Promise<AgentActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_agent_model", {
    agent,
    providerId,
    modelId,
  }) as Promise<AgentActionResult>;
}

export async function invokeGetSecret(
  agent: ModelConfigAgentId,
  providerId: string,
): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_agent_provider_secret", { agent, providerId }) as Promise<string>;
}

export async function invokeSetSecret(
  agent: ModelConfigAgentId,
  providerId: string,
  apiKey: string,
): Promise<AgentActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("set_agent_provider_secret", {
    agent,
    providerId,
    apiKey,
  }) as Promise<AgentActionResult>;
}

export async function invokeFetchCatalog(force: boolean): Promise<ModelsDevCatalog> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("fetch_models_dev_catalog", { force }) as Promise<ModelsDevCatalog>;
}

export async function invokeProbeModelsEndpoint(baseUrl: string): Promise<ProbeModelsResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("probe_models_endpoint", { baseUrl }) as Promise<ProbeModelsResult>;
}

export async function invokeRevealConfig(agent: ModelConfigAgentId): Promise<AgentActionResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reveal_agent_model_config", { agent }) as Promise<AgentActionResult>;
}
