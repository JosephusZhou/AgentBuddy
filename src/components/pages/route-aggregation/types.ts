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

/** 进站协议，分别对应路由聚合支持的两种透传协议。
 *
 * Phase 5+：路由聚合只接受 Claude Messages 与 Codex Responses 两种业务
 * 协议；`openaiModelsList` 仅用于 `GET /v1/models` 元数据查询（不入路由池）。
 */
export type InboundProtocol =
  | "claudeMessages"
  | "codexResponses"
  | "openaiModelsList";

export interface RouteLogEntry {
  /** 单调递增 ID；越大越新。 */
  id: number;
  /** Unix 毫秒。 */
  timestampMs: number;
  protocol: InboundProtocol;
  inboundMethod: string;
  inboundPath: string;
  /** 进站请求头（已脱敏：Authorization / Cookie / x-api-key 等被替换为 "***"）。 */
  inboundHeaders: Array<[string, string]>;
  /** 进站请求体（JSON 解析后；超过 8KB 会被截断并设置标志）。 */
  inboundBody: unknown | null;
  inboundBodyTruncated: boolean;
  inboundModel: string | null;
  providerId: string | null;
  providerName: string | null;
  upstreamUrl: string | null;
  upstreamStatus: number | null;
  upstreamHeaders: Array<[string, string]>;
  /** 上游响应体（流式响应时为 null；非流式截断到 8KB）。 */
  upstreamBody: string | null;
  upstreamBodyTruncated: boolean;
  /** 是否为流式请求（SSE）。流式响应体不会读入日志。 */
  stream: boolean;
  durationMs: number;
  success: boolean;
  error: string | null;
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
 * 与 Rust 端 `provider_router::build_pool_from_db` 的协议过滤保持一致：
 * 路由聚合只接受 Anthropic / OpenAI / Universal 三类 backend，其它类型
 * 一律不进入 pool。 */
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
