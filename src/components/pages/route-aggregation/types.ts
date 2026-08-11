// TypeScript types for route aggregation — mirrors Rust DTOs.

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
