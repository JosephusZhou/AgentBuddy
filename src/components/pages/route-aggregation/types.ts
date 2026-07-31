// TypeScript types for route aggregation — mirrors Rust DTOs.

export type RouteGroup = "claude_code" | "codex";

export type CloakingMode = "auto" | "always" | "never";

export interface RouteAggregationConfig {
  claudeCodeEnabled: boolean;
  codexEnabled: boolean;
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

export interface GroupStatus {
  enabled: boolean;
  activeProviders: ProviderRouteStatus[];
  totalProviders: number;
}

export interface RouteAggregationStatus {
  serverRunning: boolean;
  listenAddress: string;
  listenPort: number;
  claudeCode: GroupStatus;
  codex: GroupStatus;
}

export interface ProviderRouteToggle {
  providerId: string;
  group: string;
  enabled: boolean;
  sortOrder: number;
}

export const DEFAULT_CONFIG: RouteAggregationConfig = {
  claudeCodeEnabled: false,
  codexEnabled: false,
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
};
