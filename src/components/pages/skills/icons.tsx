/** SkillsManage 页用到的图标（从页面组件抽出，供页面与控件复用）。
 *  标准图标统一使用 lucide-react；IconGithub / IconGitcode 为品牌图标，保留手写 SVG。 */
import {
  Briefcase,
  ChevronDown,
  CloudDownload,
  Copy,
  ExternalLink,
  Folder,
  FolderInput,
  FolderOutput,
  ListChecks,
  Plus,
  Radar,
  RefreshCw,
  Search,
  SquarePen,
  Tags,
  Trash,
  X,
} from "lucide-react";

export const IconPlus = () => <Plus strokeWidth={2} />;

export const IconChevron = ({ open }: { open?: boolean }) => (
  <ChevronDown
    strokeWidth={2}
    style={{ transform: open ? "rotate(180deg)" : undefined, transition: "transform 0.15s ease" }}
  />
);

export const IconSearch = () => <Search strokeWidth={1.8} />;

export const IconRefresh = () => <RefreshCw strokeWidth={1.8} />;

export const IconClose = () => <X size={16} strokeWidth={2} />;

export const IconExternal = () => <ExternalLink size={14} strokeWidth={1.8} />;

export const IconFolder = () => <Folder strokeWidth={1.8} />;

/** 品牌图标：GitHub（lucide 无等价物，保留手写 SVG） */
export const IconGithub = () => (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden>
    <path d="M12 .5C5.73.5.75 5.48.75 11.76c0 4.97 3.22 9.18 7.69 10.66.56.1.77-.24.77-.54 0-.27-.01-1.16-.02-2.1-3.13.68-3.79-1.33-3.79-1.33-.51-1.3-1.25-1.65-1.25-1.65-1.02-.7.08-.68.08-.68 1.13.08 1.72 1.16 1.72 1.16 1 .1.72 2.72 2.72 2.72.8.62 1.84.44 2.29.34.07-.66.39-1.1.71-1.35-2.5-.28-5.12-1.25-5.12-5.56 0-1.23.44-2.23 1.16-3.02-.12-.28-.5-1.42.11-2.96 0 0 .95-.3 3.11 1.15a10.8 10.8 0 0 1 5.66 0c2.16-1.45 3.11-1.15 3.11-1.15.61 1.54.23 2.68.11 2.96.72.79 1.16 1.79 1.16 3.02 0 4.32-2.63 5.27-5.14 5.55.4.35.76 1.03.76 2.08 0 1.5-.01 2.71-.01 3.08 0 .3.2.65.78.54A11.02 11.02 0 0 0 23.25 11.76C23.25 5.48 18.27.5 12 .5z" />
  </svg>
);

/** 品牌图标：GitCode（lucide 无等价物，保留手写 SVG） */
export const IconGitcode = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <circle cx="12" cy="12" r="9" />
    <path d="M8 9.5 5 12l3 2.5" />
    <path d="M16 9.5 19 12l-3 2.5" />
    <line x1="13.5" y1="7.5" x2="10.5" y2="16.5" />
  </svg>
);

export const IconSkill = () => <Briefcase strokeWidth={1.8} />;

export const IconMigrate = () => <FolderInput strokeWidth={1.8} />;

export const IconCopy = () => <Copy size={16} strokeWidth={1.8} />;

export const IconTrash = () => <Trash size={16} strokeWidth={1.8} />;

export const IconEdit = () => <SquarePen size={16} strokeWidth={1.8} />;

export const IconCheckSquare = () => <ListChecks strokeWidth={1.8} />;

/** 头部「扫描」按钮：Radar（与搜索框的放大镜区分语义） */
export const IconScan = () => <Radar strokeWidth={1.8} />;

/** 头部「拉取更新」按钮：CloudDownload（与「检查更新」的 RefreshCw 区分） */
export const IconPull = () => <CloudDownload strokeWidth={1.8} />;

/** item「编辑标签与应用 Agent」：Tags */
export const IconTags = () => <Tags size={16} strokeWidth={1.8} />;

/** item「应用到目录」：FolderOutput（与「迁移」的 FolderInput 方向相对） */
export const IconApplyDir = () => <FolderOutput size={16} strokeWidth={1.8} />;
