/** 将本软件的路由聚合服务包装为"虚拟 AI 供应商"，供各环境/配置页的供应商下拉选择。
 *
 * 始终按当前监听配置返回该虚拟供应商，以便已保存的环境在服务停止后仍能回显；
 * 选中后各页面按普通供应商流程回填 Base URL / API Key（主 Key）：
 * - Claude 环境（anthropic/universal 语义）：baseUrl = `http://host:port`（/v1/messages）
 * - Codex / OpenCode / Pi / OMP（openai 语义）：baseUrl = `http://host:port/v1`
 */

import type { AiProvider } from "../ai-providers/types";
import { invokeGetSecret } from "../ai-providers/api";
import { getConfig, getStatus } from "./api";

/** 虚拟供应商哨兵 ID：不存在于 AI 供应商库中，不可用 get_ai_provider_secret 读取。 */
export const ROUTE_AGGREGATION_PROVIDER_ID = "__route_aggregation__";

export function isRouteAggregationProvider(id: string | null | undefined): boolean {
  return id === ROUTE_AGGREGATION_PROVIDER_ID;
}

/** OpenAI 兼容入口地址（确保路由聚合入口包含 /v1）。 */
export function openaiProviderBaseUrl(p: AiProvider): string {
  if (isRouteAggregationProvider(p.id)) {
    return p.openaiBaseUrl || `${p.baseUrl.replace(/\/$/, "")}/v1`;
  }
  return p.providerType === "universal" ? p.openaiBaseUrl || p.baseUrl : p.baseUrl;
}

/**
 * 返回路由聚合虚拟供应商。即使服务当前未启动也返回：环境关联关系保存在
 * 数据库中，编辑环境时必须能够回显已有关系；此时仍使用配置中的监听地址，
 * 待服务启动后即可生效。
 * 路由聚合同时支持 Anthropic 与 OpenAI，因此始终标记为 universal；各页面
 * 根据自身协议使用 baseUrl 或 openaiBaseUrl。
 */
export async function fetchRouteAggregationProvider(): Promise<AiProvider | null> {
  try {
    const status = await getStatus();
    const config = await getConfig();
    const base = `http://${status.listenAddress}:${status.listenPort}`;
    const primaryKey = config.apiKeys[0] ?? "";
    return {
      id: ROUTE_AGGREGATION_PROVIDER_ID,
      name: "路由聚合",
      providerType: "universal",
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
