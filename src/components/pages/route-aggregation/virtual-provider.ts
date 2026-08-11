/** 将本软件的路由聚合服务包装为"虚拟 AI 供应商"，供各环境/配置页的供应商下拉选择。
 *
 * 仅当聚合服务器正在运行时返回该虚拟供应商；选中后各页面按普通供应商流程
 * 回填 Base URL / API Key（主 Key）：
 * - Claude 环境（anthropic/universal 语义）：baseUrl = `http://host:port`（/v1/messages）
 * - Codex / OpenCode / Pi / OMP（openai 语义）：baseUrl = `http://host:port/v1`
 */

import type { AiProvider, ProviderType } from "../ai-providers/types";
import { invokeGetSecret } from "../ai-providers/api";
import { getConfig, getStatus } from "./api";

/** 虚拟供应商哨兵 ID：不存在于 AI 供应商库中，不可用 get_ai_provider_secret 读取。 */
export const ROUTE_AGGREGATION_PROVIDER_ID = "__route_aggregation__";

export function isRouteAggregationProvider(id: string | null | undefined): boolean {
  return id === ROUTE_AGGREGATION_PROVIDER_ID;
}

/**
 * 路由聚合服务运行中时返回虚拟供应商，否则返回 null。
 * providerType 由调用方按各自页面的过滤规则传入，决定回填时的 Base URL 解释方式。
 */
export async function fetchRouteAggregationProvider(
  providerType: ProviderType,
): Promise<AiProvider | null> {
  try {
    const status = await getStatus();
    if (!status.serverRunning) return null;
    const config = await getConfig();
    const base = `http://${status.listenAddress}:${status.listenPort}`;
    const primaryKey = config.apiKeys[0] ?? "";
    return {
      id: ROUTE_AGGREGATION_PROVIDER_ID,
      name: "路由聚合",
      providerType,
      baseUrl: base,
      openaiBaseUrl: `${base}/v1`,
      defaultModel: "",
      openaiDefaultModel: "",
      models: {},
      hasApiKey: primaryKey !== "",
      apiKeyCount: config.apiKeys.length,
      customModels: [],
      notes: "",
      createdAt: 0,
      updatedAt: 0,
      sortOrder: -1,
    };
  } catch {
    return null;
  }
}

/**
 * 统一的"选择供应商后取密钥"入口：虚拟供应商返回路由聚合主 Key，
 * 其余走 get_ai_provider_secret。
 */
export async function resolveProviderSecret(p: AiProvider): Promise<string> {
  if (isRouteAggregationProvider(p.id)) {
    const config = await getConfig();
    return config.apiKeys[0] ?? "";
  }
  return invokeGetSecret(p.id);
}
