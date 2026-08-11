// Tauri invoke wrappers for route aggregation commands.

import type {
  RouteAggregationConfig,
  RouteAggregationStatus,
} from "./types";

export async function getStatus(): Promise<RouteAggregationStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_aggregation_status");
}

export async function getConfig(): Promise<RouteAggregationConfig> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_aggregation_config");
}

export async function updateConfig(
  config: RouteAggregationConfig,
): Promise<RouteAggregationStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("update_route_aggregation_config", { config });
}

export async function startServer(): Promise<RouteAggregationStatus> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("start_route_aggregation");
}

export async function stopServer(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("stop_route_aggregation");
}

export async function toggleProviderRoute(
  providerId: string,
  enabled: boolean,
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("toggle_provider_route", { providerId, enabled });
}

export async function resetCircuitBreaker(providerId: string): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reset_circuit_breaker", { providerId });
}

/** 新增一个端点 API Key（后端随机生成），返回新 Key。 */
export async function addApiKey(): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("add_route_aggregation_api_key");
}

/** 删除指定索引的 API Key；索引 0（主 Key）不允许删除。 */
export async function deleteApiKey(index: number): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("delete_route_aggregation_api_key", { index });
}

/** 重新生成指定索引的 API Key，返回新 Key。 */
export async function regenerateApiKey(index: number): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("regenerate_route_aggregation_api_key", { index });
}

/** 获取供应商的有效模型列表（自定义模型优先，否则远程拉取）。 */
export async function getRouteProviderModels(
  providerId: string,
): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_provider_models", { providerId });
}
