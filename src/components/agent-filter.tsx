/** Agent 筛选 chips（单选）：MCP 管理与 Skills 管理页共用。
 * 传入的 items 应已过滤为「本机扫描到已存在 + 用户手动添加」的 Agent。 */

import { getAgentIcon } from "./agent-icons";

export interface AgentFilterItem {
  name: string;
  display_name: string;
  /** 无品牌图标时的缩写回退（sniff 返回的 icon 字段） */
  icon: string;
  /** 已应用到该 Agent 的条目数 */
  count: number;
}

interface AgentFilterChipsProps {
  items: AgentFilterItem[];
  /** 「全部」chip 的计数（条目总数） */
  total: number;
  /** 当前选中："all" 表示全部 */
  active: string;
  onSelect: (key: string) => void;
}

export function AgentFilterChips({ items, total, active, onSelect }: AgentFilterChipsProps) {
  return (
    <div className="skill-tag-filter" role="group" aria-label="按 Agent 筛选">
      <button
        type="button"
        className={`skill-source-chip skill-source-chip-tag ${active === "all" ? "active" : ""}`}
        aria-pressed={active === "all"}
        onClick={() => onSelect("all")}
      >
        <span className="skill-source-chip-label">全部</span>
        <span className="skill-source-chip-count">{total}</span>
      </button>
      {items.map((agent) => {
        const isActive = agent.name === active;
        return (
          <button
            key={agent.name}
            type="button"
            className={`skill-source-chip skill-source-chip-tag skill-source-chip-agent ${
              isActive ? "active" : ""
            }`}
            aria-pressed={isActive}
            data-tooltip={agent.display_name}
            onClick={() => onSelect(agent.name)}
          >
            <span className="skill-source-chip-agent-icon" aria-hidden>
              {getAgentIcon(agent.name) ?? agent.icon}
            </span>
            <span className="skill-source-chip-label">{agent.display_name}</span>
            <span className="skill-source-chip-count">{agent.count}</span>
          </button>
        );
      })}
    </div>
  );
}
