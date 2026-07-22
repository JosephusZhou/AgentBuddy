/** SkillsManage 页的自包含可复用控件：来源筛选 chips、标签下拉，及来源归组辅助函数。 */

import { useCallback, useEffect, useRef, useState } from "react";
import { IconChevron, IconPlus } from "./icons";
import type { SkillRecord, SkillSource } from "./types";

/* ===== Source filter helpers ===== */

export interface SourceOption {
  key: string; // "all" | "local" | `${source}::${repoLabel}`
  kind: "all" | "local" | SkillSource;
  label: string;
  count: number;
}

// 参与来源归组所需的最小字段集：SkillRecord 与 CC Switch 预览项均满足
export type SourceInfo = Pick<SkillRecord, "source" | "repoUrl" | "githubOwner" | "githubRepo">;

// 远程技能的仓库网页地址：优先用 repoUrl，其次由平台 + owner/repo 拼接；本地或缺信息返回空串
export function skillRepoUrl(skill: SkillRecord): string {
  if (skill.source !== "github" && skill.source !== "gitcode") return "";
  const host = skill.source === "gitcode" ? "https://gitcode.com" : "https://github.com";
  return (
    skill.repoUrl ||
    (skill.githubOwner && skill.githubRepo
      ? `${host}/${skill.githubOwner}/${skill.githubRepo}`
      : "")
  );
}

// 一个技能归属的来源筛选键：本地统一为 "local"，远程按 平台::owner/repo 归组
export function sourceKeyOf(skill: SourceInfo): string {
  if (skill.source !== "github" && skill.source !== "gitcode") return "local";
  const repoLabel =
    skill.githubOwner && skill.githubRepo
      ? `${skill.githubOwner}/${skill.githubRepo}`
      : skill.repoUrl || (skill.source === "gitcode" ? "GitCode" : "GitHub");
  return `${skill.source}::${repoLabel}`;
}

// 由当前技能列表推导出的来源筛选项（保持首次出现顺序）
export function buildSourceOptions(skills: SourceInfo[]): SourceOption[] {
  const localCount = skills.filter(
    (s) => s.source !== "github" && s.source !== "gitcode"
  ).length;

  const remote = new Map<string, SourceOption>();
  for (const s of skills) {
    if (s.source !== "github" && s.source !== "gitcode") continue;
    const key = sourceKeyOf(s);
    const existing = remote.get(key);
    if (existing) {
      existing.count += 1;
    } else {
      const repoLabel =
        s.githubOwner && s.githubRepo
          ? `${s.githubOwner}/${s.githubRepo}`
          : s.repoUrl || (s.source === "gitcode" ? "GitCode" : "GitHub");
      remote.set(key, { key, kind: s.source, label: repoLabel, count: 1 });
    }
  }

  const options: SourceOption[] = [
    { key: "all", kind: "all", label: "全部", count: skills.length },
  ];
  if (localCount > 0) {
    options.push({ key: "local", kind: "local", label: "本地", count: localCount });
  }
  options.push(...remote.values());
  return options;
}

/* ===== Reusable source filter chips =====
   来源筛选条（单选 chips）：主列表与 CC Switch 迁移弹窗共用同一套样式与结构 */

interface SourceFilterChipsProps {
  options: SourceOption[];
  // 单选筛选传当前 key；多选勾选场景传已激活 key 集合
  active: string | ReadonlySet<string>;
  onSelect: (key: string) => void;
  disabled?: boolean;
}

export function SourceFilterChips({ options, active, onSelect, disabled }: SourceFilterChipsProps) {
  return (
    <div className="skill-source-filter" role="group" aria-label="按来源筛选">
      {options.map((opt) => {
        const isActive =
          typeof active === "string" ? opt.key === active : active.has(opt.key);
        const kindCls =
          opt.kind === "all"
            ? "skill-source-chip-all"
            : opt.kind === "local"
              ? "skill-source-chip-local"
              : opt.kind === "gitcode"
                ? "skill-source-chip-gitcode"
                : "skill-source-chip-github";
        return (
          <button
            key={opt.key}
            type="button"
            className={`skill-source-chip ${kindCls} ${isActive ? "active" : ""}`}
            aria-pressed={isActive}
            title={opt.label}
            disabled={disabled}
            onClick={() => onSelect(opt.key)}
          >
            <span className="skill-source-chip-label">{opt.label}</span>
            <span className="skill-source-chip-count">{opt.count}</span>
          </button>
        );
      })}
    </div>
  );
}

/* ===== Reusable tag select =====
   标签下拉：无标签 / 已有标签 / 新增标签（去重、区分大小写、回车复用）。
   自管理开合与新增态，供「添加」「编辑」弹窗复用。 */

interface TagSelectProps {
  value: string;
  onChange: (v: string) => void;
  knownTags: string[];
  disabled?: boolean;
}

export function TagSelect({ value, onChange, knownTags, disabled }: TagSelectProps) {
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newText, setNewText] = useState("");
  const [notice, setNotice] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const resetCreate = useCallback(() => {
    setCreating(false);
    setNewText("");
    setNotice("");
  }, []);

  // 点击外部关闭
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
        resetCreate();
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open, resetCreate]);

  // 进入新增输入态时自动聚焦
  useEffect(() => {
    if (creating) setTimeout(() => inputRef.current?.focus(), 40);
  }, [creating]);

  // 确认新增：精确去重（区分大小写，完全相同则复用已有）
  const commit = useCallback(() => {
    const t = newText.trim();
    if (!t) {
      setNotice("请输入标签内容");
      return;
    }
    const existing = knownTags.find((k) => k === t);
    onChange(existing ?? t);
    setOpen(false);
    resetCreate();
  }, [newText, knownTags, onChange, resetCreate]);

  return (
    <div className={`app-select ${open ? "open" : ""} ${disabled ? "disabled" : ""}`} ref={rootRef}>
      <button
        type="button"
        className="app-select-trigger form-input"
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          setOpen((v) => !v);
          resetCreate();
        }}
      >
        <span className={`app-select-value ${value ? "" : "placeholder"}`}>
          {value || "无标签"}
        </span>
        <span className="app-select-chevron" aria-hidden>
          <IconChevron open={open} />
        </span>
      </button>
      {open && (
        <div className="app-select-menu" role="listbox">
          <button
            type="button"
            role="option"
            aria-selected={!value}
            className={`app-select-option ${value ? "" : "selected"}`}
            onClick={() => {
              onChange("");
              setOpen(false);
            }}
          >
            <span className="app-select-option-title">无标签</span>
          </button>
          {knownTags.map((t) => (
            <button
              key={t}
              type="button"
              role="option"
              aria-selected={value === t}
              className={`app-select-option ${value === t ? "selected" : ""}`}
              onClick={() => {
                onChange(t);
                setOpen(false);
              }}
            >
              <span className="app-select-option-title">{t}</span>
            </button>
          ))}
          {creating ? (
            <div className="app-select-option app-select-newtag">
              <input
                ref={inputRef}
                type="text"
                className="form-input app-select-newtag-input"
                placeholder="输入新标签后回车"
                value={newText}
                maxLength={20}
                onChange={(e) => {
                  setNewText(e.target.value);
                  if (notice) setNotice("");
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    commit();
                  } else if (e.key === "Escape") {
                    e.stopPropagation();
                    resetCreate();
                  }
                }}
              />
              {(() => {
                const t = newText.trim();
                const dupe = t !== "" && knownTags.includes(t);
                const msg = notice || (dupe ? "该标签已存在，回车即复用" : "");
                return msg ? <span className="app-select-newtag-notice">{msg}</span> : null;
              })()}
            </div>
          ) : (
            <button
              type="button"
              className="app-select-option app-select-newtag-trigger"
              onClick={() => {
                resetCreate();
                setCreating(true);
              }}
            >
              <IconPlus />
              <span className="app-select-option-title">新增标签</span>
            </button>
          )}
        </div>
      )}
    </div>
  );
}
