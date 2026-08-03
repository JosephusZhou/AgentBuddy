// Tauri invoke wrappers for route aggregation commands.

import type {
  RouteAggregationConfig,
  RouteAggregationStatus,
  ProviderRouteToggle,
  ProviderRouteStatus,
  RouteGroup,
  ModelEntry,
  ModelSource,
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

export async function getProviderToggles(
  group: RouteGroup,
): Promise<ProviderRouteToggle[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_provider_route_toggles", { group });
}

export async function toggleProviderRoute(
  providerId: string,
  group: RouteGroup,
  enabled: boolean,
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("toggle_provider_route", { providerId, group, enabled });
}

export async function reorderProviderRoutes(
  ids: string[],
  group: RouteGroup,
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reorder_provider_routes", { ids, group });
}

export async function resetCircuitBreaker(
  providerId: string,
  group: RouteGroup,
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reset_circuit_breaker", { providerId, group });
}

export async function getCircuitBreakerStatus(
  group: RouteGroup,
): Promise<ProviderRouteStatus[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_circuit_breaker_status", { group });
}

export async function regenerateApiKey(group: RouteGroup): Promise<string> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("regenerate_route_aggregation_api_key", { group });
}

export async function updateModels(
  group: RouteGroup,
  models: ModelEntry[],
): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("update_route_aggregation_models", { group, models });
}

export async function getRouteModels(group: RouteGroup): Promise<ModelSource[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_route_aggregation_models", { group });
}

export async function resetModels(group: RouteGroup): Promise<ModelEntry[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("reset_route_aggregation_models", { group });
}
