// Tauri invoke wrappers for route aggregation commands.

import type {
  RouteAggregationConfig,
  RouteAggregationStatus,
  RouteLogEntry,
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

/** 获取供应商的对外模型列表。
 *
 * 来源唯一：AI 供应商编辑页配置的 `customModels`（已在后端从 `custom_models_json`
 * 读取并按 alias_id 优先展开）。即使该列表为空也**不**再向供应商远端 /v1/models
 * 拉取——配置侧的自定义列表即为对外暴露的全部模型。 */
export async function getRouteProviderModels(
  providerId: string,
): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_provider_models", { providerId });
}

/** 获取路由聚合服务近期的进出日志（内存中最新的在后）。 */
export async function getRouteLogs(): Promise<RouteLogEntry[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_aggregation_logs");
}

/** 清空路由聚合的进出日志。 */
export async function clearRouteLogs(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("clear_route_aggregation_logs");
}

/** 返回路由聚合日志文件的本地路径（JSONL），失败或未挂载时返回 null。 */
export async function getRouteLogFilePath(): Promise<string | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_aggregation_log_file_path");
}

/** 在 Finder/Explorer 中显示日志文件。 */
export async function revealRouteLogFile(): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reveal_route_aggregation_log_file");
}
