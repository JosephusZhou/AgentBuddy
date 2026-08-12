// TypeScript types for route aggregation — mirrors Rust DTOs.

import type { ProviderType } from "../ai-providers/types";

export type CloakingMode = "auto" | "always" | "never";

export interface RouteAggregationConfig {
  listenAddress: string;
  listenPort: number;
  autoFailover: boolean;
  maxRetries: number;
  streamFirstByteTimeout: number;
  streamIdleTimeout: number;
  nonStreamTotalTimeout: number;
  cloakingMode: CloakingMode;
  claudeCodeVersion: string;
  codexVersion: string;
  /** 端点 API Key 列表；第一个为主 Key（只能重新生成，不能删除）。 */
  apiKeys: string[];
  /** 应用启动时是否自动拉起代理服务器（由启动/停止动作维护）。 */
  autoStart: boolean;
}

export interface ProviderRouteStatus {
  id: string;
  name: string;
  providerType: string;
  enabled: boolean;
  circuitState: string; // "closed" | "open" | "half_open"
  consecutiveFailures: number;
  lastError: string | null;
  lastErrorAt: number | null;
  requestCount: number;
  successCount: number;
}

export interface RouteAggregationStatus {
  serverRunning: boolean;
  listenAddress: string;
  listenPort: number;
  /** 两种接口格式合并后的供应商状态列表。 */
  providers: ProviderRouteStatus[];
}

export const DEFAULT_CONFIG: RouteAggregationConfig = {
  listenAddress: "127.0.0.1",
  listenPort: 16888,
  autoFailover: true,
  maxRetries: 3,
  streamFirstByteTimeout: 60,
  streamIdleTimeout: 120,
  nonStreamTotalTimeout: 600,
  cloakingMode: "auto",
  claudeCodeVersion: "2.1.63",
  codexVersion: "0.146.0",
  apiKeys: [],
  autoStart: false,
};

/** 是否可以参与路由聚合转发。
 *
 * 与 Rust 端 `provider_router::refresh_pool` 的协议过滤保持一致：
 * 当前聚合代理只实现 Anthropic Messages 与 OpenAI Chat Completions/Responses
 * 两种协议转发，因此只有这三种类型能被勾选；其他类型（如 Google Generative
 * AI 的 generateContent 协议）协议不兼容，在前端禁用勾选并展示说明，避免
 * toggle 写入 DB 后下游 pool 永远不收录、UI 状态永远显示"已勾选"的错乱。 */
export function isRouteableProviderType(
  providerType: ProviderType | string,
): boolean {
  return (
    providerType === "anthropic" ||
    providerType === "openai" ||
    providerType === "universal"
  );
}

/** 不支持路由聚合的供应商在 UI 上的提示文案。 */
export const UNSUPPORTED_ROUTE_PROVIDER_HINT =
  "该供应商的 API 协议与路由聚合不兼容，无法参与聚合转发（可在 AI 供应商管理页直接使用）";
