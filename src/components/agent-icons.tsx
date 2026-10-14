/**
 * 内置 Agent 彩色图标（SVG，来自开源项目 Magpie https://github.com/yetone/magpie，MIT 协议）。
 *
 * 图标文件位于 public/agent-icons/*.svg，按 agent name 命名。
 * key 与 src-tauri/src/sniff.rs 中 AgentDef.name 保持一致；未匹配的 agent
 * （如手动添加）由调用方回退为名称缩写。
 */
import type { ReactNode } from "react";

/** 已知有彩色图标的 agent name 集合。 */
const KNOWN_AGENTS = new Set([
  "codex",
  "claude-code",
  "claude-desktop",
  "opencode",
  "antigravity",
  "workbuddy",
  "pi",
]);

/** 渲染指定 agent 的彩色图标；无匹配返回 null，由调用方回退到名称缩写。 */
export function getAgentIcon(name: string): ReactNode | null {
  if (!KNOWN_AGENTS.has(name)) return null;
  return <img src={`/agent-icons/${name}.svg`} alt={name} draggable={false} />;
}

/** 无品牌图标时的缩写回退：取前两个字母/汉字，大写。 */
function initials(label: string): string {
  const cleaned = label.replace(/[^a-zA-Z一-龥]/g, "");
  const base = cleaned.length >= 2 ? cleaned.slice(0, 2) : label.slice(0, 2);
  return base.toUpperCase();
}

interface AgentBadgeProps {
  /** sniff 的 name，用于匹配品牌图标 */
  name: string;
  /** 展示名（tooltip 与缩写回退用），缺省时用 name */
  label?: string;
}

/**
 * 页面中性的「已应用 Agent」徽章：优先显示品牌图标，无图标时回退名称缩写。
 * 悬停时在徽章下方展示完整名称（自绘 tooltip，样式随主题）。MCP、Skills 等列表共用，避免样式散落。
 */
export function AgentBadge({ name, label }: AgentBadgeProps) {
  const display = label ?? name;
  const icon = getAgentIcon(name);
  return (
    <span className="agent-badge" data-tooltip={display}>
      {icon ?? <span className="agent-badge-initials">{initials(display)}</span>}
    </span>
  );
}
