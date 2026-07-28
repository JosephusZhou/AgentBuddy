import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { AgentBadge, getAgentIcon } from "../agent-icons";
import { CheckGlyph, useOverlayDismiss } from "../ui";
import { SKILL_UNSUPPORTED_AGENTS } from "./skills/types";
import type {
  SkillRecord,
  AgentResult,
  SniffPreviewResult,
  CcSwitchPreviewItem,
  CcSwitchPreviewResult,
  BatchApplyMode,
  SkillInstallMode,
} from "./skills/types";

import {
  invokeListSkills,
  invokePreviewSniff,
  invokeImportSniffed,
  invokeCheckUpdates,
  invokePickLocal,
  invokeAddGithub,
  invokeAddGitcode,
  invokeExportSkill,
  invokeUpdateSkill,
  invokeDeleteSkill,
  invokeApplySkill,
  invokeAddLocalPath,
  invokePreviewCcSwitch,
  invokeMigrateCcSwitch,
  invokeBatchDelete,
  invokeBatchExport,
  invokeBatchApply,
  invokeBatchSetTag,
} from "./skills/api";
import {
  IconPlus,
  IconChevron,
  IconSearch,
  IconRefresh,
  IconClose,
  IconExternal,
  IconFolder,
  IconGithub,
  IconGitcode,
  IconSkill,
  IconMigrate,
  IconTrash,
  IconCheckSquare,
  IconScan,
  IconPull,
  IconTags,
  IconApplyDir,
} from "./skills/icons";
import {
  TagSelect,
  SourceFilterChips,
  buildSourceOptions,
  sourceKeyOf,
  skillRepoUrl,
} from "./skills/controls";
import { AgentFilterChips } from "../agent-filter";

/* ===== Component ===== */

export default function SkillsManage() {
  const [skills, setSkills] = useState<SkillRecord[]>([]);
  const [agents, setAgents] = useState<AgentResult[]>([]);
  const [statusMsg, setStatusMsg] = useStatusMessage();
  const [loading, setLoading] = useState(true);
  const [isSniffing, setIsSniffing] = useState(false);
  const [isChecking, setIsChecking] = useState(false);
  const [isAdding, setIsAdding] = useState(false);
  const isPickingLocalFolder = useRef(false);
  const [exportingId, setExportingId] = useState<string | null>(null);
  const [directoryApplyTarget, setDirectoryApplyTarget] = useState<SkillRecord | null>(null);
  const [directoryApplyMode, setDirectoryApplyMode] = useState<SkillInstallMode>("link");
  const [batchDirectoryApplyOpen, setBatchDirectoryApplyOpen] = useState(false);
  const [batchDirectoryApplyMode, setBatchDirectoryApplyMode] = useState<SkillInstallMode>("link");
  const [updatingId, setUpdatingId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<SkillRecord | null>(null);
  const [deleteAgentCopies, setDeleteAgentCopies] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);

  // 编辑弹窗：修改标签 + 应用到 Agent
  const [editTarget, setEditTarget] = useState<SkillRecord | null>(null);
  const [editTag, setEditTag] = useState("");
  const [editAgents, setEditAgents] = useState<Set<string>>(new Set());
  const [editInstallMode, setEditInstallMode] = useState<SkillInstallMode>("link");
  const [isSavingEdit, setIsSavingEdit] = useState(false);

  const [showAddModal, setShowAddModal] = useState(false);
  const [addTab, setAddTab] = useState<"local" | "github" | "gitcode">("local");
  const [githubUrl, setGithubUrl] = useState("");
  const [gitcodeUrl, setGitcodeUrl] = useState("");
  const [localPath, setLocalPath] = useState("");
  const [formError, setFormError] = useState("");
  // 添加时选择的标签（空 = 无标签）
  const [addTag, setAddTag] = useState("");
  const githubInputRef = useRef<HTMLInputElement>(null);
  const gitcodeInputRef = useRef<HTMLInputElement>(null);
  const hasLoaded = useRef(false);

  const [showMigrateModal, setShowMigrateModal] = useState(false);
  const [migratePreview, setMigratePreview] = useState<CcSwitchPreviewResult | null>(null);
  const [selectedMigrateIds, setSelectedMigrateIds] = useState<Set<string>>(new Set());
  const [isPreviewingMigrate, setIsPreviewingMigrate] = useState(false);
  const [isMigrating, setIsMigrating] = useState(false);

  const [showSniffModal, setShowSniffModal] = useState(false);
  const [sniffPreview, setSniffPreview] = useState<SniffPreviewResult | null>(null);
  const [selectedSniffKeys, setSelectedSniffKeys] = useState<Set<string>>(new Set());
  const [isImportingSniff, setIsImportingSniff] = useState(false);

  // 来源筛选：单选，"all" 表示全部
  const [activeSource, setActiveSource] = useState<string>("all");
  // 标签筛选：单选，"all" 表示全部（与来源筛选为「与」关系）
  const [activeTag, setActiveTag] = useState<string>("all");
  // Agent 筛选：单选，"all" 表示全部（与来源/标签筛选为「与」关系）
  const [activeAgent, setActiveAgent] = useState<string>("all");
  // 搜索输入即时响应；查询值延迟更新，避免每次按键都重新筛选整表
  const [searchInput, setSearchInput] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  // 筛选区折叠态：默认展开；收起时对应筛选回退到「全部」，避免隐藏筛选造成困惑
  const [sourceExpanded, setSourceExpanded] = useState(true);
  const [tagExpanded, setTagExpanded] = useState(true);
  const [agentExpanded, setAgentExpanded] = useState(true);

  // ===== 批量管理 =====
  // 显式选择模式：进入后卡片改为可勾选，隐藏单卡操作，底部浮出操作条
  const [batchMode, setBatchMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [isBatchRunning, setIsBatchRunning] = useState(false);
  const [batchUpdatingScope, setBatchUpdatingScope] = useState<"available" | "selected" | null>(null);
  // 批量删除确认
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);
  const [batchDeleteAgentCopies, setBatchDeleteAgentCopies] = useState(false);
  // 批量应用到 Agent
  const [batchApplyOpen, setBatchApplyOpen] = useState(false);
  const [batchApplyAgents, setBatchApplyAgents] = useState<Set<string>>(new Set());
  const [batchApplyMode, setBatchApplyMode] = useState<BatchApplyMode>("add");
  const [batchInstallMode, setBatchInstallMode] = useState<SkillInstallMode>("link");
  // 批量设置标签
  const [batchTagOpen, setBatchTagOpen] = useState(false);
  const [batchTag, setBatchTag] = useState("");

  const addDismiss = useOverlayDismiss(() => setShowAddModal(false));
  const migrateDismiss = useOverlayDismiss(() => closeMigrate());
  const sniffDismiss = useOverlayDismiss(() => closeSniff());
  const editDismiss = useOverlayDismiss(() => setEditTarget(null), !isSavingEdit);
  const directoryApplyDismiss = useOverlayDismiss(
    () => setDirectoryApplyTarget(null),
    !exportingId
  );
  const batchDirectoryApplyDismiss = useOverlayDismiss(
    () => setBatchDirectoryApplyOpen(false),
    !isBatchRunning
  );
  const deleteDismiss = useOverlayDismiss(() => setDeleteTarget(null), !isDeleting);
  const batchDeleteDismiss = useOverlayDismiss(() => setBatchDeleteOpen(false), !isBatchRunning);
  const batchApplyDismiss = useOverlayDismiss(() => setBatchApplyOpen(false), !isBatchRunning);
  const batchTagDismiss = useOverlayDismiss(() => setBatchTagOpen(false), !isBatchRunning);

  const loadAgents = useCallback(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const cached = await (invoke("get_cached_agents") as Promise<AgentResult[]>);
      setAgents(cached.filter((a) => a.found));
    } catch {
      setAgents([]);
    }
  }, []);

  const reload = useCallback(async (opts?: { quiet?: boolean }) => {
    try {
      const res = await invokeListSkills();
      setSkills(res.skills);
      if (!opts?.quiet) {
        setStatusMsg(res.message);
      }
    } catch (e) {
      setStatusMsg(`加载技能失败: ${e}`);
      setSkills([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (hasLoaded.current) return;
    hasLoaded.current = true;
    void loadAgents();
    void reload({ quiet: true });
  }, [loadAgents, reload]);

  useEffect(() => {
    if (!showAddModal) return;
    setFormError("");
    if (addTab === "github") {
      setTimeout(() => githubInputRef.current?.focus(), 80);
    } else if (addTab === "gitcode") {
      setTimeout(() => gitcodeInputRef.current?.focus(), 80);
    }
  }, [showAddModal, addTab]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setShowAddModal(false);
        if (!isMigrating) setShowMigrateModal(false);
        if (!isImportingSniff) setShowSniffModal(false);
        if (!isSavingEdit) setEditTarget(null);
        if (!isBatchRunning) {
          setBatchDeleteOpen(false);
          setBatchApplyOpen(false);
          setBatchTagOpen(false);
        }
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [isMigrating, isImportingSniff, isSavingEdit, isBatchRunning]);

  const openAdd = useCallback(() => {
    setFormError("");
    setGithubUrl("");
    setGitcodeUrl("");
    setLocalPath("");
    setAddTag("");
    setAddTab("local");
    setShowAddModal(true);
  }, []);

  const openSniff = useCallback(async () => {
    if (isSniffing || isImportingSniff) return;
    setIsSniffing(true);
    setSniffPreview(null);
    setSelectedSniffKeys(new Set());
    setShowSniffModal(true);
    setStatusMsg("正在从各 Agent 的 skills 目录扫描…");
    try {
      const res = await invokePreviewSniff();
      setSniffPreview(res);
      // 默认勾选可导入项
      const next = new Set<string>();
      for (const item of res.items) {
        if (item.status === "import") next.add(item.key);
      }
      setSelectedSniffKeys(next);
      setStatusMsg(res.message);
    } catch (e) {
      setSniffPreview({
        ok: false,
        items: [],
        scannedAgents: 0,
        total: 0,
        importable: 0,
        skipExists: 0,
        message: `扫描失败: ${e}`,
      });
      setStatusMsg(`扫描失败: ${e}`);
    } finally {
      setIsSniffing(false);
    }
  }, [isSniffing, isImportingSniff]);

  const closeSniff = useCallback(() => {
    if (isImportingSniff) return;
    setShowSniffModal(false);
  }, [isImportingSniff]);

  const toggleSniffKey = useCallback((key: string, selectable: boolean) => {
    if (!selectable) return;
    setSelectedSniffKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const selectAllSniffImportable = useCallback(() => {
    if (!sniffPreview) return;
    setSelectedSniffKeys(
      new Set(
        sniffPreview.items.filter((i) => i.status === "import").map((i) => i.key)
      )
    );
  }, [sniffPreview]);

  const clearSniffSelection = useCallback(() => {
    setSelectedSniffKeys(new Set());
  }, []);

  const confirmSniffImport = useCallback(async () => {
    if (isImportingSniff) return;
    const keys = Array.from(selectedSniffKeys);
    if (keys.length === 0) {
      setStatusMsg("请先勾选要导入的技能");
      return;
    }
    setIsImportingSniff(true);
    setStatusMsg(`正在导入 ${keys.length} 个技能…`);
    try {
      const res = await invokeImportSniffed(keys);
      setSkills(res.skills);
      setStatusMsg(res.message);
      if (res.ok || res.imported > 0) {
        setShowSniffModal(false);
        setSniffPreview(null);
        setSelectedSniffKeys(new Set());
      }
    } catch (e) {
      setStatusMsg(`导入失败: ${e}`);
    } finally {
      setIsImportingSniff(false);
    }
  }, [isImportingSniff, selectedSniffKeys]);

  const doCheckUpdates = useCallback(async () => {
    if (isChecking) return;
    setIsChecking(true);
    setStatusMsg("正在检查 GitHub 技能更新…");
    try {
      const res = await invokeCheckUpdates();
      setSkills(res.skills);
      setStatusMsg(res.message);
    } catch (e) {
      setStatusMsg(`检查更新失败: ${e}`);
    } finally {
      setIsChecking(false);
    }
  }, [isChecking]);

  const doPickLocal = useCallback(async () => {
    if (isAdding || isPickingLocalFolder.current) return;
    isPickingLocalFolder.current = true;
    setFormError("");
    setStatusMsg("请选择本地 Skill 目录…");
    try {
      const res = await invokePickLocal(addTag.trim());
      if (res.ok) {
        setShowAddModal(false);
        await reload({ quiet: true });
        setStatusMsg(res.message);
      } else if (res.message && res.message !== "已取消选择") {
        setFormError(res.message);
        setStatusMsg(res.message);
      } else {
        setStatusMsg(res.message || "已取消");
      }
    } catch (e) {
      setFormError(String(e));
      setStatusMsg(`导入失败: ${e}`);
    } finally {
      isPickingLocalFolder.current = false;
    }
  }, [isAdding, reload, addTag]);

  const doAddLocalPath = useCallback(async () => {
    const path = localPath.trim();
    if (!path) {
      setFormError("请填写本地目录路径");
      return;
    }
    if (isAdding) return;
    setIsAdding(true);
    setFormError("");
    try {
      const res = await invokeAddLocalPath(path, addTag.trim());
      if (res.ok) {
        setShowAddModal(false);
        await reload({ quiet: true });
        setStatusMsg(res.message);
      } else {
        setFormError(res.message);
      }
    } catch (e) {
      setFormError(String(e));
    } finally {
      setIsAdding(false);
    }
  }, [isAdding, localPath, reload, addTag]);

  const doAddGithub = useCallback(async () => {
    const url = githubUrl.trim();
    if (!url) {
      setFormError("请填写 GitHub 仓库地址");
      return;
    }
    if (isAdding) return;
    setIsAdding(true);
    setFormError("");
    setStatusMsg("正在从 GitHub 克隆并导入…");
    try {
      const res = await invokeAddGithub(url, addTag.trim());
      if (res.ok) {
        setShowAddModal(false);
        await reload({ quiet: true });
        setStatusMsg(res.message);
      } else {
        setFormError(res.message);
        setStatusMsg(res.message);
      }
    } catch (e) {
      setFormError(String(e));
      setStatusMsg(`导入失败: ${e}`);
    } finally {
      setIsAdding(false);
    }
  }, [githubUrl, isAdding, reload, addTag]);

  const doAddGitcode = useCallback(async () => {
    const url = gitcodeUrl.trim();
    if (!url) {
      setFormError("请填写 GitCode 仓库地址");
      return;
    }
    if (isAdding) return;
    setIsAdding(true);
    setFormError("");
    setStatusMsg("正在从 GitCode 克隆并导入…");
    try {
      const res = await invokeAddGitcode(url, addTag.trim());
      if (res.ok) {
        setShowAddModal(false);
        await reload({ quiet: true });
        setStatusMsg(res.message);
      } else {
        setFormError(res.message);
        setStatusMsg(res.message);
      }
    } catch (e) {
      setFormError(String(e));
      setStatusMsg(`导入失败: ${e}`);
    } finally {
      setIsAdding(false);
    }
  }, [gitcodeUrl, isAdding, reload, addTag]);

  const openMigrate = useCallback(async () => {
    if (isPreviewingMigrate || isMigrating) return;
    setIsPreviewingMigrate(true);
    setStatusMsg("正在读取 ~/.cc-switch 中的 Skills…");
    setMigratePreview(null);
    setSelectedMigrateIds(new Set());
    setShowMigrateModal(true);
    try {
      const res = await invokePreviewCcSwitch();
      setMigratePreview(res);
      // 默认勾选可导入项
      const next = new Set<string>();
      for (const item of res.items) {
        if (item.status === "import") next.add(item.ccId);
      }
      setSelectedMigrateIds(next);
      setStatusMsg(res.message);
    } catch (e) {
      setMigratePreview({
        ok: false,
        items: [],
        total: 0,
        importable: 0,
        skipExists: 0,
        missing: 0,
        message: `预览失败: ${e}`,
        ccSwitchRoot: "",
      });
      setStatusMsg(`CC Switch 预览失败: ${e}`);
    } finally {
      setIsPreviewingMigrate(false);
    }
  }, [isMigrating, isPreviewingMigrate]);

  const closeMigrate = useCallback(() => {
    if (isMigrating) return;
    setShowMigrateModal(false);
  }, [isMigrating]);

  const toggleMigrateId = useCallback((ccId: string, selectable: boolean) => {
    if (!selectable) return;
    setSelectedMigrateIds((prev) => {
      const next = new Set(prev);
      if (next.has(ccId)) next.delete(ccId);
      else next.add(ccId);
      return next;
    });
  }, []);

  const selectAllImportable = useCallback(() => {
    if (!migratePreview) return;
    setSelectedMigrateIds(
      new Set(
        migratePreview.items
          .filter((i) => i.status === "import")
          .map((i) => i.ccId)
      )
    );
  }, [migratePreview]);

  const clearMigrateSelection = useCallback(() => {
    setSelectedMigrateIds(new Set());
  }, []);

  const confirmMigrate = useCallback(async () => {
    if (isMigrating) return;
    const ids = Array.from(selectedMigrateIds);
    if (ids.length === 0) {
      setStatusMsg("请先勾选要迁移的技能");
      return;
    }
    setIsMigrating(true);
    setStatusMsg(`正在迁移 ${ids.length} 个技能…`);
    try {
      const res = await invokeMigrateCcSwitch(ids);
      setSkills(res.skills);
      setStatusMsg(res.message);
      if (res.ok || res.imported > 0) {
        setShowMigrateModal(false);
        setMigratePreview(null);
        setSelectedMigrateIds(new Set());
      }
    } catch (e) {
      setStatusMsg(`迁移失败: ${e}`);
    } finally {
      setIsMigrating(false);
    }
  }, [isMigrating, selectedMigrateIds]);

  // 迁移弹窗内的可导入项（仅这些可被勾选，来源整组勾选也只作用于它们）
  const migrateImportableItems = useMemo(
    () => migratePreview?.items.filter((i) => i.status === "import") ?? [],
    [migratePreview]
  );

  // 迁移弹窗的来源选项：与主列表同一套归组规则，只统计可导入项
  const migrateSourceOptions = useMemo(
    () => buildSourceOptions(migrateImportableItems),
    [migrateImportableItems]
  );

  // 已整组勾选的来源集合（该来源全部可导入项均被选中时视为激活）
  const migrateActiveSources = useMemo(() => {
    const active = new Set<string>();
    for (const opt of migrateSourceOptions) {
      const group =
        opt.key === "all"
          ? migrateImportableItems
          : migrateImportableItems.filter((i) => sourceKeyOf(i) === opt.key);
      if (group.length > 0 && group.every((i) => selectedMigrateIds.has(i.ccId))) {
        active.add(opt.key);
      }
    }
    return active;
  }, [migrateSourceOptions, migrateImportableItems, selectedMigrateIds]);

  // 点击来源：整组勾选该来源的可导入项；若已全选则整组取消
  const toggleMigrateSource = useCallback(
    (key: string) => {
      if (isMigrating) return;
      const group =
        key === "all"
          ? migrateImportableItems
          : migrateImportableItems.filter((i) => sourceKeyOf(i) === key);
      if (group.length === 0) return;
      setSelectedMigrateIds((prev) => {
        const next = new Set(prev);
        const allChecked = group.every((i) => next.has(i.ccId));
        for (const item of group) {
          if (allChecked) next.delete(item.ccId);
          else next.add(item.ccId);
        }
        return next;
      });
    },
    [isMigrating, migrateImportableItems]
  );

  // 列表展示顺序：已激活来源的项优先置顶（组内及其余项均保持原有相对顺序）
  const sortedMigrateItems = useMemo(() => {
    const items = migratePreview?.items ?? [];
    // "all" 不是具体来源键，剔除后为空则无需重排
    const prioritized = new Set(
      Array.from(migrateActiveSources).filter((k) => k !== "all")
    );
    if (prioritized.size === 0) return items;
    const top: CcSwitchPreviewItem[] = [];
    const rest: CcSwitchPreviewItem[] = [];
    for (const item of items) {
      (prioritized.has(sourceKeyOf(item)) ? top : rest).push(item);
    }
    return [...top, ...rest];
  }, [migratePreview, migrateActiveSources]);

  const agentLabel = useCallback(
    (name: string) => agents.find((a) => a.name === name)?.display_name ?? name,
    [agents]
  );

  const openDirectoryApply = useCallback((skill: SkillRecord) => {
    setDirectoryApplyTarget(skill);
    setDirectoryApplyMode("link");
  }, []);

  const confirmDirectoryApply = useCallback(async () => {
    if (!directoryApplyTarget || exportingId) return;
    const skill = directoryApplyTarget;
    const modeLabel = directoryApplyMode === "link" ? "软链接" : "完整复制";
    setExportingId(skill.id);
    setStatusMsg(`请选择「${skill.title}」的应用目标目录…`);
    try {
      const res = await invokeExportSkill(skill.id, directoryApplyMode);
      setStatusMsg(res.ok ? res.message : res.message || "已取消");
      if (res.ok) setDirectoryApplyTarget(null);
    } catch (e) {
      setStatusMsg(`以${modeLabel}应用到目录失败: ${e}`);
    } finally {
      setExportingId(null);
    }
  }, [directoryApplyMode, directoryApplyTarget, exportingId]);

  const doUpdateSkill = useCallback(
    async (skill: SkillRecord) => {
      if (updatingId) return;
      setUpdatingId(skill.id);
      setStatusMsg(`正在从远端更新「${skill.title}」…`);
      try {
        const res = await invokeUpdateSkill(skill.id);
        setStatusMsg(res.message);
        if (res.ok) await reload({ quiet: true });
      } catch (e) {
        setStatusMsg(`更新失败: ${e}`);
      } finally {
        setUpdatingId(null);
      }
    },
    [updatingId, reload]
  );

  const confirmDelete = useCallback(async () => {
    if (!deleteTarget || isDeleting) return;
    setIsDeleting(true);
    try {
      const res = await invokeDeleteSkill(deleteTarget.id, deleteAgentCopies);
      if (res.ok) {
        setDeleteTarget(null);
        await reload({ quiet: true });
      }
      setStatusMsg(res.message);
    } catch (e) {
      setStatusMsg(`删除失败: ${e}`);
    } finally {
      setIsDeleting(false);
    }
  }, [deleteTarget, deleteAgentCopies, isDeleting, reload]);

  const openEdit = useCallback((skill: SkillRecord) => {
    setEditTarget(skill);
    setEditTag(skill.tag?.trim() ?? "");
    setEditAgents(new Set(skill.appliedAgents));
    setEditInstallMode("link");
    setIsSavingEdit(false);
  }, []);

  const toggleEditAgent = useCallback((name: string) => {
    if (SKILL_UNSUPPORTED_AGENTS.has(name)) return;
    setEditAgents((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const confirmEdit = useCallback(async () => {
    if (!editTarget || isSavingEdit) return;
    setIsSavingEdit(true);
    try {
      const res = await invokeApplySkill(
        editTarget.id,
        Array.from(editAgents),
        editTag.trim(),
        editInstallMode
      );
      setStatusMsg(res.message);
      if (res.ok) {
        setEditTarget(null);
      }
      await reload({ quiet: true });
    } catch (e) {
      setStatusMsg(`应用失败: ${e}`);
    } finally {
      setIsSavingEdit(false);
    }
  }, [editTarget, editAgents, editTag, editInstallMode, isSavingEdit, reload]);

  const openRepo = useCallback(
    async (skill: SkillRecord) => {
      const url = skillRepoUrl(skill);
      if (!url) return;
      try {
        // Tauri webview 拦截 window.open，改由后端用系统 `open` 打开默认浏览器
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("open_external_url", { url });
      } catch (e) {
        setStatusMsg(`打开仓库失败: ${e}`);
      }
    },
    [setStatusMsg]
  );

  // 来源筛选项由技能列表推导
  const sourceOptions = useMemo(() => buildSourceOptions(skills), [skills]);

  // 若当前选中的来源随技能变动而消失，回退到「全部」
  useEffect(() => {
    if (activeSource === "all") return;
    if (!sourceOptions.some((o) => o.key === activeSource)) {
      setActiveSource("all");
    }
  }, [activeSource, sourceOptions]);

  // 已有标签（去重、按名称排序），供筛选条与添加下拉复用
  const knownTags = useMemo(() => {
    const set = new Set<string>();
    for (const s of skills) {
      const t = s.tag?.trim();
      if (t) set.add(t);
    }
    return Array.from(set).sort((a, b) => a.localeCompare(b, "zh-Hans-CN"));
  }, [skills]);

  // 标签筛选项：全部 + 每个标签（带数量）
  const tagOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of skills) {
      const t = s.tag?.trim();
      if (t) counts.set(t, (counts.get(t) ?? 0) + 1);
    }
    const opts: { key: string; label: string; count: number }[] = [
      { key: "all", label: "全部", count: skills.length },
    ];
    for (const t of knownTags) {
      opts.push({ key: t, label: t, count: counts.get(t) ?? 0 });
    }
    return opts;
  }, [skills, knownTags]);

  // 若当前选中的标签随技能变动而消失，回退到「全部」
  useEffect(() => {
    if (activeTag === "all") return;
    if (!knownTags.includes(activeTag)) {
      setActiveTag("all");
    }
  }, [activeTag, knownTags]);

  // Agent 筛选项：仅本机已存在（扫描发现 + 手动添加，agents 已按 found 过滤）的 Agent，
  // 计数为该 Agent 已应用的技能数（可为 0，便于发现尚未应用任何技能的 Agent）
  const agentFilterOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of skills) {
      for (const name of s.appliedAgents) {
        counts.set(name, (counts.get(name) ?? 0) + 1);
      }
    }
    return agents.map((a) => ({
      name: a.name,
      display_name: a.display_name,
      icon: a.icon,
      count: counts.get(a.name) ?? 0,
    }));
  }, [agents, skills]);

  // 若当前选中的 Agent 被移除（卸载/删除），回退到「全部」
  useEffect(() => {
    if (activeAgent === "all") return;
    if (!agents.some((a) => a.name === activeAgent)) {
      setActiveAgent("all");
    }
  }, [activeAgent, agents]);

  useEffect(() => {
    const query = searchInput.trim().toLocaleLowerCase();
    if (!query) {
      setSearchQuery("");
      return;
    }
    const timer = window.setTimeout(() => setSearchQuery(query), 250);
    return () => window.clearTimeout(timer);
  }, [searchInput]);

  // 折叠/展开筛选区：仅切换显隐，保留当前选中项
  const toggleSourceExpanded = useCallback(() => {
    setSourceExpanded((prev) => !prev);
  }, []);
  const toggleTagExpanded = useCallback(() => {
    setTagExpanded((prev) => !prev);
  }, []);
  const toggleAgentExpanded = useCallback(() => {
    setAgentExpanded((prev) => !prev);
  }, []);

  // Agent、来源、标签与关键词为「与」筛选；关键词覆盖卡片当前展示的主要技能信息
  const filteredSkills = useMemo(
    () =>
      skills.filter((s) => {
        const matchesFilters =
          (activeAgent === "all" || s.appliedAgents.includes(activeAgent)) &&
          (activeSource === "all" || sourceKeyOf(s) === activeSource) &&
          (activeTag === "all" || (s.tag?.trim() ?? "") === activeTag);
        if (!matchesFilters || !searchQuery) return matchesFilters;
        return [
          s.title,
          s.description,
          s.tag,
          s.githubOwner,
          s.githubRepo,
          s.repoUrl,
          s.githubPath,
          s.localPath,
          ...s.appliedAgents.map((name) => agentLabel(name)),
        ].some((value) => value.toLocaleLowerCase().includes(searchQuery));
      }),
    [skills, activeAgent, activeSource, activeTag, searchQuery, agentLabel]
  );

  const updateAvailableSkills = useMemo(
    () => skills.filter((s) => s.updateAvailable),
    [skills]
  );

  const updateAvailableCount = updateAvailableSkills.length;

  // ===== 批量选择派生值与操作 =====
  // 当前筛选结果的 id 集，供「全选可见项」与选择态统计使用
  const filteredIdSet = useMemo(
    () => new Set(filteredSkills.map((s) => s.id)),
    [filteredSkills]
  );
  // 已选中且仍存在于技能库的 id（技能被删除后自动收敛）
  const validSelectedIds = useMemo(
    () => new Set(skills.filter((s) => selectedIds.has(s.id)).map((s) => s.id)),
    [skills, selectedIds]
  );
  // 已选中但不在当前筛选内的数量（提示用户选择未丢失，只是被筛选隐藏）
  const hiddenSelectedCount = useMemo(() => {
    let n = 0;
    for (const id of validSelectedIds) if (!filteredIdSet.has(id)) n += 1;
    return n;
  }, [validSelectedIds, filteredIdSet]);
  // 当前筛选内是否已全部选中（用于全选/取消全选切换）
  const allFilteredSelected =
    filteredSkills.length > 0 && filteredSkills.every((s) => selectedIds.has(s.id));

  const exitBatchMode = useCallback(() => {
    setBatchMode(false);
    setSelectedIds(new Set());
    setBatchDeleteOpen(false);
    setBatchApplyOpen(false);
    setBatchTagOpen(false);
  }, []);

  const toggleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  // 全选/取消全选：以当前筛选结果为准，不影响筛选外的已选项
  const toggleSelectAllFiltered = useCallback(() => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      const everySelected =
        filteredSkills.length > 0 && filteredSkills.every((s) => next.has(s.id));
      if (everySelected) {
        for (const s of filteredSkills) next.delete(s.id);
      } else {
        for (const s of filteredSkills) next.add(s.id);
      }
      return next;
    });
  }, [filteredSkills]);

  const clearSelection = useCallback(() => setSelectedIds(new Set()), []);

  const runBatchUpdate = useCallback(
    async (
      targetSkills: SkillRecord[],
      opts?: {
        exitWhenDone?: boolean;
        emptyMessage?: string;
        scope?: "available" | "selected";
      }
    ) => {
      if (isBatchRunning) return;
      const remoteTargets = targetSkills.filter(
        (s) => s.source === "github" || s.source === "gitcode"
      );
      if (remoteTargets.length === 0) {
        setStatusMsg(opts?.emptyMessage ?? "没有可更新的远程技能");
        return;
      }

      setIsBatchRunning(true);
      setBatchUpdatingScope(opts?.scope ?? null);
      setStatusMsg(`正在更新 ${remoteTargets.length} 个技能…`);
      let succeeded = 0;
      let failed = 0;
      const errors: string[] = [];
      try {
        for (const skill of remoteTargets) {
          try {
            const res = await invokeUpdateSkill(skill.id);
            if (res.ok) {
              succeeded += 1;
            } else {
              failed += 1;
              errors.push(`${skill.title}: ${res.message}`);
            }
          } catch (e) {
            failed += 1;
            errors.push(`${skill.title}: ${String(e)}`);
          }
        }
        await reload({ quiet: true });
        if (failed === 0) {
          setStatusMsg(`已更新 ${succeeded} 个技能到远端最新`);
          if (opts?.exitWhenDone) exitBatchMode();
        } else {
          setStatusMsg(
            `批量更新结束：成功 ${succeeded}，失败 ${failed}。${errors.slice(0, 3).join("；")}`
          );
        }
      } finally {
        setBatchUpdatingScope(null);
        setIsBatchRunning(false);
      }
    },
    [isBatchRunning, reload, exitBatchMode]
  );

  const doPullAvailableUpdates = useCallback(async () => {
    await runBatchUpdate(updateAvailableSkills, {
      emptyMessage: "当前没有检测到可更新的技能",
      scope: "available",
    });
  }, [runBatchUpdate, updateAvailableSkills]);

  const doBatchUpdateSelected = useCallback(async () => {
    const selected = skills.filter((s) => validSelectedIds.has(s.id));
    await runBatchUpdate(selected, {
      exitWhenDone: true,
      emptyMessage: "选中的技能里没有可更新的远程来源",
      scope: "selected",
    });
  }, [skills, validSelectedIds, runBatchUpdate]);

  // 批量删除：确认后调用后端，成功刷新整表并退出批量模式
  const confirmBatchDelete = useCallback(async () => {
    if (isBatchRunning) return;
    const ids = Array.from(validSelectedIds);
    if (ids.length === 0) {
      setStatusMsg("请先选择要删除的技能");
      return;
    }
    setIsBatchRunning(true);
    setStatusMsg(`正在删除 ${ids.length} 个技能…`);
    try {
      const res = await invokeBatchDelete(ids, batchDeleteAgentCopies);
      setSkills(res.skills);
      setStatusMsg(res.message);
      setBatchDeleteOpen(false);
      exitBatchMode();
    } catch (e) {
      setStatusMsg(`批量删除失败: ${e}`);
    } finally {
      setIsBatchRunning(false);
    }
  }, [isBatchRunning, validSelectedIds, batchDeleteAgentCopies, exitBatchMode]);

  const openBatchDirectoryApply = useCallback(() => {
    if (validSelectedIds.size === 0) {
      setStatusMsg("请先选择要应用的技能");
      return;
    }
    setBatchDirectoryApplyMode("link");
    setBatchDirectoryApplyOpen(true);
  }, [validSelectedIds]);

  // 批量应用到目录：安装方式确认后只弹一次目录选择器。
  const confirmBatchDirectoryApply = useCallback(async () => {
    if (isBatchRunning) return;
    const ids = Array.from(validSelectedIds);
    if (ids.length === 0) {
      setStatusMsg("请先选择要应用的技能");
      return;
    }
    const modeLabel = batchDirectoryApplyMode === "link" ? "软链接" : "完整复制";
    setIsBatchRunning(true);
    setStatusMsg("请选择应用目标目录…");
    try {
      const res = await invokeBatchExport(ids, batchDirectoryApplyMode);
      setStatusMsg(res.message);
      if (res.succeeded > 0) {
        setBatchDirectoryApplyOpen(false);
        exitBatchMode();
      }
    } catch (e) {
      setStatusMsg(`批量以${modeLabel}应用到目录失败: ${e}`);
    } finally {
      setIsBatchRunning(false);
    }
  }, [batchDirectoryApplyMode, isBatchRunning, validSelectedIds, exitBatchMode]);

  // 打开批量应用弹窗：默认追加模式，Agent 选择初始为空
  const openBatchApply = useCallback(() => {
    if (validSelectedIds.size === 0) {
      setStatusMsg("请先选择要应用的技能");
      return;
    }
    setBatchApplyAgents(new Set());
    setBatchApplyMode("add");
    setBatchInstallMode("link");
    setBatchApplyOpen(true);
  }, [validSelectedIds]);

  const toggleBatchApplyAgent = useCallback((name: string) => {
    if (SKILL_UNSUPPORTED_AGENTS.has(name)) return;
    setBatchApplyAgents((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const confirmBatchApply = useCallback(async () => {
    if (isBatchRunning) return;
    const ids = Array.from(validSelectedIds);
    if (ids.length === 0) {
      setStatusMsg("请先选择要应用的技能");
      return;
    }
    // 追加模式必须选至少一个 Agent；覆盖模式允许清空（= 全部解除）
    if (batchApplyMode === "add" && batchApplyAgents.size === 0) {
      setStatusMsg("请至少选择一个 Agent");
      return;
    }
    setIsBatchRunning(true);
    setStatusMsg(`正在为 ${ids.length} 个技能同步 Agent…`);
    try {
      const res = await invokeBatchApply(
        ids,
        Array.from(batchApplyAgents),
        batchApplyMode,
        batchInstallMode
      );
      setSkills(res.skills);
      setStatusMsg(res.message);
      setBatchApplyOpen(false);
      exitBatchMode();
    } catch (e) {
      setStatusMsg(`批量应用失败: ${e}`);
    } finally {
      setIsBatchRunning(false);
    }
  }, [isBatchRunning, validSelectedIds, batchApplyMode, batchApplyAgents, batchInstallMode, exitBatchMode]);

  // 打开批量标签弹窗：初值取选中项共同标签，否则留空
  const openBatchTag = useCallback(() => {
    if (validSelectedIds.size === 0) {
      setStatusMsg("请先选择要设置标签的技能");
      return;
    }
    const selectedTags = new Set(
      skills
        .filter((s) => validSelectedIds.has(s.id))
        .map((s) => s.tag?.trim() ?? "")
    );
    // 所选技能标签一致时预填该标签，便于微调；不一致则留空由用户决定
    setBatchTag(selectedTags.size === 1 ? [...selectedTags][0] : "");
    setBatchTagOpen(true);
  }, [validSelectedIds, skills]);

  const confirmBatchTag = useCallback(async () => {
    if (isBatchRunning) return;
    const ids = Array.from(validSelectedIds);
    if (ids.length === 0) {
      setStatusMsg("请先选择要设置标签的技能");
      return;
    }
    const tag = batchTag.trim();
    setIsBatchRunning(true);
    setStatusMsg(
      tag ? `正在为 ${ids.length} 个技能设置标签…` : `正在清除 ${ids.length} 个技能的标签…`
    );
    try {
      const res = await invokeBatchSetTag(ids, tag);
      setSkills(res.skills);
      setStatusMsg(res.message);
      setBatchTagOpen(false);
      exitBatchMode();
    } catch (e) {
      setStatusMsg(`批量设置标签失败: ${e}`);
    } finally {
      setIsBatchRunning(false);
    }
  }, [isBatchRunning, validSelectedIds, batchTag, exitBatchMode]);

  return (
    <>
      <div className="content-header">
        <div className="content-header-bar">
          <h1 className="content-title">Skills 管理</h1>
          <div className="header-actions">
            {/* 从右到左：添加、扫描、检查更新、从 CC Switch 迁移、批量管理 */}
            {updateAvailableCount > 0 && (
              <button
                className={`action-btn ${batchUpdatingScope === "available" ? "sniffing" : ""}`}
                data-tooltip={
                  batchUpdatingScope === "available"
                    ? "拉取中..."
                    : `拉取更新（${updateAvailableCount}）`
                }
                onClick={() => void doPullAvailableUpdates()}
                disabled={
                  isBatchRunning ||
                  isChecking ||
                  isSniffing ||
                  isMigrating ||
                  isAdding
                }
              >
                <IconPull />
              </button>
            )}
            <button
              className={`action-btn ${batchMode ? "active" : ""}`}
              data-tooltip={batchMode ? "退出批量管理" : "批量管理"}
              onClick={() => (batchMode ? exitBatchMode() : setBatchMode(true))}
              disabled={loading || skills.length === 0 || isMigrating}
              aria-pressed={batchMode}
            >
              <IconCheckSquare />
            </button>
            <button
              className={`action-btn ${isPreviewingMigrate || isMigrating ? "sniffing" : ""}`}
              data-tooltip={
                isMigrating
                  ? "迁移中..."
                  : isPreviewingMigrate
                    ? "读取中..."
                    : "从 CC Switch 迁移"
              }
              onClick={() => void openMigrate()}
              disabled={
                isPreviewingMigrate ||
                isMigrating ||
                isSniffing ||
                isChecking ||
                isAdding
              }
            >
              <IconMigrate />
            </button>
            <button
              className={`action-btn ${isChecking ? "sniffing" : ""}`}
              data-tooltip={isChecking ? "检查中..." : "检查更新"}
              onClick={() => void doCheckUpdates()}
              disabled={isChecking || isSniffing || isMigrating}
            >
              <IconRefresh />
            </button>
            <button
              className={`action-btn ${isSniffing ? "sniffing" : ""}`}
              data-tooltip={isSniffing ? "扫描中..." : "扫描"}
              onClick={() => void openSniff()}
              disabled={isSniffing || isChecking || isMigrating || isImportingSniff}
            >
              <IconScan />
            </button>
            <button
              className="action-btn"
              data-tooltip="添加"
              onClick={openAdd}
              disabled={isAdding || isMigrating}
            >
              <IconPlus />
            </button>
          </div>
        </div>
      </div>

      <div className="content-body">
        <Toast message={statusMsg} />

        {loading ? (
          <div className="empty-state">
            <div className="empty-state-text">正在加载技能库…</div>
          </div>
        ) : skills.length === 0 ? (
          <div className="empty-state">
            <IconSkill />
            <div className="empty-state-text">
              技能库为空。可点击右上角添加，或扫描各 Agent 已安装的 Skills
            </div>
          </div>
        ) : (
          <>
            <div className="mcp-summary">
              共 <strong>{skills.length}</strong> 个技能
              {updateAvailableCount > 0 && (
                <>
                  ，其中 <strong>{updateAvailableCount}</strong> 个有更新
                </>
              )}
            </div>
            <div className="skill-search">
              <IconSearch />
              <input
                type="search"
                value={searchInput}
                onChange={(event) => {
                  const value = event.target.value;
                  setSearchInput(value);
                  if (!value.trim()) setSearchQuery("");
                }}
                placeholder="搜索技能名称、简介、标签、来源或 Agent"
                aria-label="搜索技能"
              />
              {searchInput && (
                <button
                  type="button"
                  className="skill-search-clear"
                  data-tooltip="清空搜索"
                  aria-label="清空搜索"
                  onClick={() => {
                    setSearchInput("");
                    setSearchQuery("");
                  }}
                >
                  <IconClose />
                </button>
              )}
            </div>
            {agents.length > 0 && (
              <div className="skill-filter-section">
                <button
                  type="button"
                  className="skill-filter-heading"
                  aria-expanded={agentExpanded}
                  onClick={toggleAgentExpanded}
                >
                  <span className="skill-filter-heading-chevron" aria-hidden>
                    <IconChevron open={agentExpanded} />
                  </span>
                  <span className="skill-filter-heading-title">Agents</span>
                  {!agentExpanded && activeAgent !== "all" && (
                    <span className="skill-filter-heading-active">
                      {agentFilterOptions.find((o) => o.name === activeAgent)?.display_name ?? ""}
                    </span>
                  )}
                </button>
                {agentExpanded && (
                  <AgentFilterChips
                    items={agentFilterOptions}
                    total={skills.length}
                    active={activeAgent}
                    onSelect={setActiveAgent}
                  />
                )}
              </div>
            )}
            {sourceOptions.length > 1 && (
              <div className="skill-filter-section">
                <button
                  type="button"
                  className="skill-filter-heading"
                  aria-expanded={sourceExpanded}
                  onClick={toggleSourceExpanded}
                >
                  <span className="skill-filter-heading-chevron" aria-hidden>
                    <IconChevron open={sourceExpanded} />
                  </span>
                  <span className="skill-filter-heading-title">来源</span>
                  {!sourceExpanded && activeSource !== "all" && (
                    <span className="skill-filter-heading-active">
                      {sourceOptions.find((o) => o.key === activeSource)?.label ?? ""}
                    </span>
                  )}
                </button>
                {sourceExpanded && (
                  <SourceFilterChips
                    options={sourceOptions}
                    active={activeSource}
                    onSelect={setActiveSource}
                  />
                )}
              </div>
            )}
            {knownTags.length > 0 && (
              <div className="skill-filter-section">
                <button
                  type="button"
                  className="skill-filter-heading"
                  aria-expanded={tagExpanded}
                  onClick={toggleTagExpanded}
                >
                  <span className="skill-filter-heading-chevron" aria-hidden>
                    <IconChevron open={tagExpanded} />
                  </span>
                  <span className="skill-filter-heading-title">标签</span>
                  {!tagExpanded && activeTag !== "all" && (
                    <span className="skill-filter-heading-active">
                      {tagOptions.find((o) => o.key === activeTag)?.label ?? ""}
                    </span>
                  )}
                </button>
                {tagExpanded && (
                  <div className="skill-tag-filter" role="group" aria-label="按标签筛选">
                    {tagOptions.map((opt) => {
                      const active = opt.key === activeTag;
                      return (
                        <button
                          key={opt.key}
                          type="button"
                          className={`skill-source-chip skill-source-chip-tag ${active ? "active" : ""}`}
                          aria-pressed={active}
                          data-tooltip={opt.label}
                          onClick={() => setActiveTag(opt.key)}
                        >
                          <span className="skill-source-chip-label">{opt.label}</span>
                          <span className="skill-source-chip-count">{opt.count}</span>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            )}
            <div className="skill-list">
              {filteredSkills.length === 0 ? (
                <div className="empty-state skill-filter-empty">
                  <IconSearch />
                  <div className="empty-state-text">没有匹配的技能</div>
                </div>
              ) : filteredSkills.map((skill) => {
                const isRemote = skill.source === "github" || skill.source === "gitcode";
                const isGitcode = skill.source === "gitcode";
                const hostTag = isGitcode ? "skill-tag-gitcode" : "skill-tag-github";
                const hostLabel = isGitcode ? "GitCode" : "GitHub";
                const repoLabel =
                  skill.githubOwner && skill.githubRepo
                    ? `${skill.githubOwner}/${skill.githubRepo}`
                    : "";
                const checked = selectedIds.has(skill.id);
                return (
                  <div
                    key={skill.id}
                    className={`skill-card ${batchMode ? "selectable" : ""} ${
                      batchMode && checked ? "selected" : ""
                    }`}
                    onClick={batchMode ? () => toggleSelect(skill.id) : undefined}
                    role={batchMode ? "checkbox" : undefined}
                    aria-checked={batchMode ? checked : undefined}
                  >
                    <div className="skill-card-header">
                      {batchMode && (
                        <label
                          className="ui-check skill-card-check"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <input
                            type="checkbox"
                            className="ui-check-input"
                            checked={checked}
                            onChange={() => toggleSelect(skill.id)}
                          />
                          <CheckGlyph />
                        </label>
                      )}
                      <div className="skill-card-main">
                        <div className="skill-card-title-row">
                          <span className="skill-card-title">{skill.title}</span>
                          {isRemote ? (
                            (() => {
                              const repoUrl = skillRepoUrl(skill);
                              // 有可跳转地址时整个来源徽标可点击打开仓库网页；否则退化为静态徽标
                              return repoUrl ? (
                                <button
                                  type="button"
                                  className={`skill-source-link ${hostTag}`}
                                  data-tooltip={`打开仓库：${repoUrl}`}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    void openRepo(skill);
                                  }}
                                >
                                  <IconExternal />
                                  <span className="skill-source-link-label">
                                    {repoLabel || hostLabel}
                                  </span>
                                </button>
                              ) : (
                                <span className={`skill-tag ${hostTag}`}>{hostLabel}</span>
                              );
                            })()
                          ) : (
                            <span className="skill-tag skill-tag-local">本地</span>
                          )}
                          {skill.updateAvailable && (
                            <span className="skill-tag skill-tag-update">有更新</span>
                          )}
                          {skill.tag?.trim() && (
                            <span className="skill-tag skill-tag-custom">
                              {skill.tag.trim()}
                            </span>
                          )}
                        </div>
                        {skill.description ? (
                          <p className="skill-card-desc">{skill.description}</p>
                        ) : (
                          <p className="skill-card-desc skill-card-desc-muted">暂无简介</p>
                        )}
                      </div>
                      {!batchMode && (
                        <div className="skill-card-actions">
                          {isRemote && (
                            <button
                              type="button"
                              className="claude-env-action-btn"
                              data-tooltip={
                                skill.updateAvailable
                                  ? "有更新，点击拉取远端最新"
                                  : "更新到远端最新"
                              }
                              onClick={() => void doUpdateSkill(skill)}
                              disabled={updatingId === skill.id || isDeleting || isBatchRunning}
                            >
                              <IconPull />
                            </button>
                          )}
                          <button
                            type="button"
                            className="claude-env-action-btn"
                            data-tooltip="编辑标签与应用 Agent"
                            onClick={() => openEdit(skill)}
                            disabled={isDeleting || exportingId === skill.id || isBatchRunning}
                          >
                            <IconTags />
                          </button>
                          <button
                            type="button"
                            className="claude-env-action-btn"
                            data-tooltip="应用到目录…"
                            onClick={() => openDirectoryApply(skill)}
                            disabled={exportingId === skill.id || isBatchRunning}
                          >
                            <IconApplyDir />
                          </button>
                          <button
                            type="button"
                            className="claude-env-action-btn danger"
                            data-tooltip="删除技能"
                            onClick={() => {
                              setDeleteTarget(skill);
                              setDeleteAgentCopies(false);
                            }}
                            disabled={isDeleting || exportingId === skill.id || isBatchRunning}
                          >
                            <IconTrash />
                          </button>
                        </div>
                      )}
                    </div>

                    <div className="skill-card-agents">
                      {skill.appliedAgents.length > 0 ? (
                        <>
                          <span className="skill-card-agents-label">已应用</span>
                          <div className="agent-badge-list">
                            {skill.appliedAgents.map((name) => (
                              <AgentBadge key={name} name={name} label={agentLabel(name)} />
                            ))}
                          </div>
                        </>
                      ) : (
                        <span className="skill-card-agents-empty">未应用到任何 Agent</span>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </>
        )}
      </div>

      {/* ===== 批量操作条（批量模式下浮出） ===== */}
      {batchMode && !loading && skills.length > 0 && (
        <div className="skill-batch-bar">
          <div className="skill-batch-bar-left">
            <button
              type="button"
              className="mcp-agent-action"
              onClick={toggleSelectAllFiltered}
              disabled={isBatchRunning || filteredSkills.length === 0}
            >
              {allFilteredSelected ? "取消全选" : "全选"}
            </button>
            <button
              type="button"
              className="mcp-agent-action"
              onClick={clearSelection}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              清除
            </button>
            <span className="skill-batch-count">
              已选 <strong>{validSelectedIds.size}</strong> / {skills.length}
              {hiddenSelectedCount > 0 && (
                <span className="skill-batch-count-hint">
                  （{hiddenSelectedCount} 项不在当前筛选内）
                </span>
              )}
            </span>
          </div>
          <div className="skill-batch-bar-right">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => void doBatchUpdateSelected()}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              {batchUpdatingScope === "selected" ? "更新中…" : "更新"}
            </button>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={openBatchApply}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              应用到 Agent
            </button>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={openBatchTag}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              应用标签
            </button>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={openBatchDirectoryApply}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              应用到目录
            </button>
            <button
              type="button"
              className="btn btn-danger"
              onClick={() => {
                if (validSelectedIds.size === 0) {
                  setStatusMsg("请先选择要删除的技能");
                  return;
                }
                setBatchDeleteAgentCopies(false);
                setBatchDeleteOpen(true);
              }}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              删除
            </button>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={exitBatchMode}
              disabled={isBatchRunning}
            >
              退出
            </button>
          </div>
        </div>
      )}

      {/* ===== Add Skill Modal ===== */}
      <div
        className={`modal-overlay ${showAddModal ? "visible" : ""}`}
        {...addDismiss}
      >
        <div className="modal skill-add-modal">
          <div className="modal-header">
            <h2 className="modal-title">添加 Skill</h2>
            <button className="modal-close" onClick={() => setShowAddModal(false)}>
              <IconClose />
            </button>
          </div>

          <div className="modal-body">
            <div className="skill-add-tabs">
              <button
                type="button"
                className={`skill-add-tab ${addTab === "local" ? "active" : ""}`}
                onClick={() => setAddTab("local")}
              >
                <IconFolder />
                本地导入
              </button>
              <button
                type="button"
                className={`skill-add-tab ${addTab === "github" ? "active" : ""}`}
                onClick={() => setAddTab("github")}
              >
                <IconGithub />
                GitHub
              </button>
              <button
                type="button"
                className={`skill-add-tab ${addTab === "gitcode" ? "active" : ""}`}
                onClick={() => setAddTab("gitcode")}
              >
                <IconGitcode />
                GitCode
              </button>
            </div>

            {/* 标签选择：对三种导入方式统一生效 */}
            <div className="form-group">
              <label className="form-label">
                标签 <span className="form-label-optional">可选，用于分组筛选</span>
              </label>
              <TagSelect value={addTag} onChange={setAddTag} knownTags={knownTags} disabled={isAdding} />
            </div>

            {addTab === "local" ? (
              <>
                <p className="skill-add-hint">
                  选择包含 <code>SKILL.md</code> 的技能目录，将复制到{" "}
                  <code>~/.agentbuddy/skills</code>
                </p>
                <div className="form-group">
                  <label className="form-label" htmlFor="skill-local-path">
                    目录路径 <span className="form-label-optional">可选，也可点下方按钮选择</span>
                  </label>
                  <input
                    id="skill-local-path"
                    type="text"
                    className="form-input"
                    placeholder="例如: ~/Downloads/my-skill"
                    value={localPath}
                    onChange={(e) => setLocalPath(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void doAddLocalPath();
                    }}
                  />
                </div>
                {formError && <div className="form-error">{formError}</div>}
                <div className="skill-add-actions">
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={() => void doPickLocal()}
                    disabled={isAdding}
                  >
                    浏览文件夹…
                  </button>
                  <button
                    type="button"
                    className="btn btn-primary"
                    onClick={() => void doAddLocalPath()}
                    disabled={isAdding || !localPath.trim()}
                  >
                    {isAdding ? "导入中…" : "导入路径"}
                  </button>
                </div>
              </>
            ) : addTab === "github" ? (
              <>
                <p className="skill-add-hint">
                  支持 <code>owner/repo</code> 或完整 GitHub URL；仓库内需有{" "}
                  <code>SKILL.md</code>
                </p>
                <div className="form-group">
                  <label className="form-label" htmlFor="skill-github-url">
                    GitHub 地址
                  </label>
                  <input
                    ref={githubInputRef}
                    id="skill-github-url"
                    type="text"
                    className="form-input"
                    placeholder="例如: anthropics/skills 或 https://github.com/owner/repo"
                    value={githubUrl}
                    onChange={(e) => setGithubUrl(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void doAddGithub();
                    }}
                  />
                </div>
                {formError && <div className="form-error">{formError}</div>}
                <div className="skill-add-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    onClick={() => void doAddGithub()}
                    disabled={isAdding || !githubUrl.trim()}
                  >
                    {isAdding ? "克隆导入中…" : "从 GitHub 导入"}
                  </button>
                </div>
              </>
            ) : (
              <>
                <p className="skill-add-hint">
                  GitCode 为国内开源平台，支持 <code>owner/repo</code> 或完整
                  GitCode URL；仓库内需有 <code>SKILL.md</code>（支持多级子目录）
                </p>
                <div className="form-group">
                  <label className="form-label" htmlFor="skill-gitcode-url">
                    GitCode 地址
                  </label>
                  <input
                    ref={gitcodeInputRef}
                    id="skill-gitcode-url"
                    type="text"
                    className="form-input"
                    placeholder="例如: HarmonyOS_Skills/harmonyos-agent-skills 或 https://gitcode.com/owner/repo"
                    value={gitcodeUrl}
                    onChange={(e) => setGitcodeUrl(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void doAddGitcode();
                    }}
                  />
                </div>
                {formError && <div className="form-error">{formError}</div>}
                <div className="skill-add-actions">
                  <button
                    type="button"
                    className="btn btn-primary"
                    onClick={() => void doAddGitcode()}
                    disabled={isAdding || !gitcodeUrl.trim()}
                  >
                    {isAdding ? "克隆导入中…" : "从 GitCode 导入"}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      {/* ===== CC Switch migrate preview modal ===== */}
      <div
        className={`modal-overlay ${showMigrateModal ? "visible" : ""}`}
        {...migrateDismiss}
      >
        <div className="modal modal-lg skill-migrate-modal">
          <div className="modal-header">
            <h2 className="modal-title">从 CC Switch 迁移 Skills</h2>
            <button
              className="modal-close"
              onClick={closeMigrate}
              disabled={isMigrating}
            >
              <IconClose />
            </button>
          </div>

          <div className="modal-body skill-migrate-body">
            {isPreviewingMigrate && !migratePreview ? (
              <div className="skill-migrate-loading">正在扫描 ~/.cc-switch …</div>
            ) : migratePreview ? (
              <>
                <p className="skill-add-hint">
                  数据源：<code>{migratePreview.ccSwitchRoot || "~/.cc-switch"}</code>
                  。将复制到 <code>~/.agentbuddy/skills</code>，不会修改 CC Switch 原文件。
                </p>
                <div className="skill-migrate-summary">
                  共 <strong>{migratePreview.total}</strong> 项 · 可导入{" "}
                  <strong>{migratePreview.importable}</strong> · 已存在{" "}
                  <strong>{migratePreview.skipExists}</strong> · 源缺失{" "}
                  <strong>{migratePreview.missing}</strong>
                  {selectedMigrateIds.size > 0 && (
                    <>
                      {" "}
                      · 已选 <strong>{selectedMigrateIds.size}</strong>
                    </>
                  )}
                </div>

                {migratePreview.items.length === 0 ? (
                  <div className="skill-migrate-empty">{migratePreview.message}</div>
                ) : (
                  <>
                    {migrateSourceOptions.length > 1 && (
                      <div className="skill-filter-section">
                        <div className="skill-filter-heading static" role="presentation">
                          <span className="skill-filter-heading-title">来源</span>
                          <span className="skill-filter-heading-active">
                            点击来源可整组勾选 / 取消
                          </span>
                        </div>
                        <SourceFilterChips
                          options={migrateSourceOptions}
                          active={migrateActiveSources}
                          onSelect={toggleMigrateSource}
                          disabled={isMigrating}
                        />
                      </div>
                    )}
                    <div className="skill-migrate-toolbar">
                      <button
                        type="button"
                        className="mcp-agent-action"
                        onClick={selectAllImportable}
                        disabled={migratePreview.importable === 0 || isMigrating}
                      >
                        全选可导入
                      </button>
                      <button
                        type="button"
                        className="mcp-agent-action"
                        onClick={clearMigrateSelection}
                        disabled={selectedMigrateIds.size === 0 || isMigrating}
                      >
                        清除选择
                      </button>
                    </div>
                    <div className="skill-migrate-list">
                      {sortedMigrateItems.map((item) => {
                        const selectable = item.status === "import";
                        const checked = selectedMigrateIds.has(item.ccId);
                        const repoLabel =
                          item.githubOwner && item.githubRepo
                            ? `${item.githubOwner}/${item.githubRepo}`
                            : "";
                        return (
                          <label
                            key={item.ccId}
                            className={`skill-migrate-item skill-migrate-status-${item.status} ${
                              checked ? "selected" : ""
                            } ${selectable ? "" : "disabled"}`}
                          >
                            <input
                              type="checkbox"
                              className="skill-migrate-check"
                              checked={checked}
                              disabled={!selectable || isMigrating}
                              onChange={() => toggleMigrateId(item.ccId, selectable)}
                            />
                            <div className="skill-migrate-item-main">
                              <div className="skill-migrate-item-title-row">
                                <span className="skill-migrate-item-title">{item.title}</span>
                                {item.source === "github" ? (
                                  <span className="skill-tag skill-tag-github">
                                    {repoLabel || "GitHub"}
                                  </span>
                                ) : (
                                  <span className="skill-tag skill-tag-local">本地</span>
                                )}
                                <span
                                  className={`skill-migrate-badge skill-migrate-badge-${item.status}`}
                                >
                                  {item.statusLabel}
                                </span>
                              </div>
                              {item.description && (
                                <p className="skill-migrate-item-desc">{item.description}</p>
                              )}
                              {item.enabledAgents.length > 0 && (
                                <div className="skill-migrate-item-agents">
                                  <span className="skill-migrate-item-agents-label">
                                    CC Switch 已启用
                                  </span>
                                  <div className="agent-badge-list">
                                    {item.enabledAgents.map((n) => (
                                      <AgentBadge key={n} name={n} label={agentLabel(n)} />
                                    ))}
                                  </div>
                                </div>
                              )}
                            </div>
                          </label>
                        );
                      })}
                    </div>
                  </>
                )}
              </>
            ) : null}
          </div>

          <div className="modal-footer skill-migrate-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={closeMigrate}
              disabled={isMigrating}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmMigrate()}
              disabled={
                isMigrating ||
                isPreviewingMigrate ||
                selectedMigrateIds.size === 0 ||
                !migratePreview?.ok
              }
            >
              {isMigrating
                ? "迁移中…"
                : `确认迁移${selectedMigrateIds.size > 0 ? `（${selectedMigrateIds.size}）` : ""}`}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Sniff preview modal ===== */}
      <div
        className={`modal-overlay ${showSniffModal ? "visible" : ""}`}
        {...sniffDismiss}
      >
        <div className="modal modal-lg skill-migrate-modal">
          <div className="modal-header">
            <h2 className="modal-title">扫描 Agent 中的 Skills</h2>
            <button
              className="modal-close"
              onClick={closeSniff}
              disabled={isImportingSniff}
            >
              <IconClose />
            </button>
          </div>

          <div className="modal-body skill-migrate-body">
            {isSniffing && !sniffPreview ? (
              <div className="skill-migrate-loading">正在扫描各 Agent 的 skills 目录…</div>
            ) : sniffPreview ? (
              <>
                <p className="skill-add-hint">
                  从各 Agent 的 skills 目录发现的技能，将复制到{" "}
                  <code>~/.agentbuddy/skills</code>，不会修改 Agent 原文件。
                </p>
                <div className="skill-migrate-summary">
                  已扫描 <strong>{sniffPreview.scannedAgents}</strong> 个 Agent · 共{" "}
                  <strong>{sniffPreview.total}</strong> 项 · 可导入{" "}
                  <strong>{sniffPreview.importable}</strong> · 已存在{" "}
                  <strong>{sniffPreview.skipExists}</strong>
                  {selectedSniffKeys.size > 0 && (
                    <>
                      {" "}
                      · 已选 <strong>{selectedSniffKeys.size}</strong>
                    </>
                  )}
                </div>

                {sniffPreview.items.length === 0 ? (
                  <div className="skill-migrate-empty">{sniffPreview.message}</div>
                ) : (
                  <>
                    <div className="skill-migrate-toolbar">
                      <button
                        type="button"
                        className="mcp-agent-action"
                        onClick={selectAllSniffImportable}
                        disabled={sniffPreview.importable === 0 || isImportingSniff}
                      >
                        全选可导入
                      </button>
                      <button
                        type="button"
                        className="mcp-agent-action"
                        onClick={clearSniffSelection}
                        disabled={selectedSniffKeys.size === 0 || isImportingSniff}
                      >
                        清除选择
                      </button>
                    </div>
                    <div className="skill-migrate-list">
                      {sniffPreview.items.map((item) => {
                        const selectable = item.status === "import";
                        const checked = selectedSniffKeys.has(item.key);
                        return (
                          <label
                            key={item.key}
                            className={`skill-migrate-item skill-migrate-status-${item.status} ${
                              checked ? "selected" : ""
                            } ${selectable ? "" : "disabled"}`}
                          >
                            <input
                              type="checkbox"
                              className="skill-migrate-check"
                              checked={checked}
                              disabled={!selectable || isImportingSniff}
                              onChange={() => toggleSniffKey(item.key, selectable)}
                            />
                            <div className="skill-migrate-item-main">
                              <div className="skill-migrate-item-title-row">
                                <span className="skill-migrate-item-title">{item.title}</span>
                                <span
                                  className={`skill-migrate-badge skill-migrate-badge-${item.status}`}
                                >
                                  {item.statusLabel}
                                </span>
                              </div>
                              {item.description && (
                                <p className="skill-migrate-item-desc">{item.description}</p>
                              )}
                              {item.foundAgents.length > 0 && (
                                <div className="skill-migrate-item-agents">
                                  <span className="skill-migrate-item-agents-label">
                                    发现于
                                  </span>
                                  <div className="agent-badge-list">
                                    {item.foundAgents.map((n) => (
                                      <AgentBadge key={n} name={n} label={agentLabel(n)} />
                                    ))}
                                  </div>
                                </div>
                              )}
                            </div>
                          </label>
                        );
                      })}
                    </div>
                  </>
                )}
              </>
            ) : null}
          </div>

          <div className="modal-footer skill-migrate-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={closeSniff}
              disabled={isImportingSniff}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmSniffImport()}
              disabled={
                isImportingSniff ||
                isSniffing ||
                selectedSniffKeys.size === 0 ||
                !sniffPreview?.ok
              }
            >
              {isImportingSniff
                ? "导入中…"
                : `确认导入${selectedSniffKeys.size > 0 ? `（${selectedSniffKeys.size}）` : ""}`}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Edit Skill Modal ===== */}
      <div
        className={`modal-overlay ${editTarget ? "visible" : ""}`}
        {...editDismiss}
      >
        <div className="modal skill-edit-modal">
          <div className="modal-header">
            <h2 className="modal-title">
              编辑技能{editTarget ? `：${editTarget.title}` : ""}
            </h2>
            <button
              className="modal-close"
              onClick={() => !isSavingEdit && setEditTarget(null)}
              disabled={isSavingEdit}
            >
              <IconClose />
            </button>
          </div>

          {editTarget && (
            <div className="modal-body">
              {/* 标签 */}
              <div className="form-group">
                <label className="form-label">
                  标签 <span className="form-label-optional">可选，用于分组筛选</span>
                </label>
                <TagSelect
                  value={editTag}
                  onChange={setEditTag}
                  knownTags={knownTags}
                  disabled={isSavingEdit}
                />
              </div>

              <div className="form-group">
                <label className="form-label">安装方式</label>
                <div className="skill-install-mode" role="radiogroup" aria-label="安装方式">
                  <button
                    type="button"
                    role="radio"
                    aria-checked={editInstallMode === "link"}
                    className={`skill-install-mode-opt ${editInstallMode === "link" ? "active" : ""}`}
                    onClick={() => setEditInstallMode("link")}
                    disabled={isSavingEdit}
                  >
                    <span className="skill-install-mode-title">软链接</span>
                    <span className="skill-install-mode-desc">技能库更新会立即反映到 Agent</span>
                  </button>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={editInstallMode === "copy"}
                    className={`skill-install-mode-opt ${editInstallMode === "copy" ? "active" : ""}`}
                    onClick={() => setEditInstallMode("copy")}
                    disabled={isSavingEdit}
                  >
                    <span className="skill-install-mode-title">完整复制</span>
                    <span className="skill-install-mode-desc">复制完整目录，Agent 不依赖技能库</span>
                  </button>
                </div>
              </div>

              {/* 应用到 Agent */}
              <div className="form-group">
                <div className="agent-pick-header">
                  <label className="form-label">
                    应用到 Agent
                    {editAgents.size > 0 && (
                      <span className="form-label-optional">已选 {editAgents.size} 个</span>
                    )}
                  </label>
                </div>
                <p className="skill-add-hint">
                  勾选后，将以{editInstallMode === "link" ? "软链接" : "完整复制"}形式把该技能应用到对应
                  Agent 的 <code>skills</code> 目录；取消勾选会移除其对应项。
                </p>
                {agents.length === 0 ? (
                  <div className="agent-pick-empty">
                    暂无已安装的 Agent，请先到「Agent 管理」完成扫描
                  </div>
                ) : (
                  <div className="agent-pick-grid">
                    {agents.map((agent) => {
                      const unsupported = SKILL_UNSUPPORTED_AGENTS.has(agent.name);
                      const selected = editAgents.has(agent.name);
                      return (
                        <button
                          key={agent.name}
                          type="button"
                          className={`agent-pick ${selected ? "selected" : ""} ${
                            unsupported ? "disabled" : ""
                          }`}
                          onClick={() => toggleEditAgent(agent.name)}
                          disabled={unsupported || isSavingEdit}
                          data-tooltip={unsupported ? "该 Agent 暂无标准 Skills 目录" : undefined}
                        >
                          <span className={`agent-pick-icon ${selected ? "found" : ""}`}>
                            {getAgentIcon(agent.name) ?? agent.icon}
                          </span>
                          <span className="agent-pick-name">{agent.display_name}</span>
                          <span className={`agent-pick-check ${selected ? "checked" : ""}`}>
                            {selected ? "✓" : ""}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                )}
                {(() => {
                  const removed = editTarget.appliedAgents.filter(
                    (name) => !editAgents.has(name)
                  );
                  if (removed.length === 0) return null;
                  const labels = removed.map((n) => agentLabel(n)).join("、");
                  return (
                    <div className="skill-edit-warning">
                      将从这些 Agent 移除该技能（含真实副本）：{labels}
                    </div>
                  );
                })()}
              </div>
            </div>
          )}

          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setEditTarget(null)}
              disabled={isSavingEdit}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmEdit()}
              disabled={isSavingEdit}
            >
              {isSavingEdit ? "保存中…" : "保存并同步"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== 应用到目录弹窗 ===== */}
      <div
        className={`modal-overlay ${directoryApplyTarget ? "visible" : ""}`}
        {...directoryApplyDismiss}
      >
        <div className="modal skill-edit-modal">
          <div className="modal-header">
            <h2 className="modal-title">
              应用到目录{directoryApplyTarget ? `：${directoryApplyTarget.title}` : ""}
            </h2>
            <button
              className="modal-close"
              onClick={() => !exportingId && setDirectoryApplyTarget(null)}
              disabled={Boolean(exportingId)}
            >
              <IconClose />
            </button>
          </div>
          <div className="modal-body">
            <div className="form-group">
              <label className="form-label">安装方式</label>
              <div className="skill-install-mode" role="radiogroup" aria-label="安装方式">
                <button
                  type="button"
                  role="radio"
                  aria-checked={directoryApplyMode === "link"}
                  className={`skill-install-mode-opt ${directoryApplyMode === "link" ? "active" : ""}`}
                  onClick={() => setDirectoryApplyMode("link")}
                  disabled={Boolean(exportingId)}
                >
                  <span className="skill-install-mode-title">软链接</span>
                  <span className="skill-install-mode-desc">技能库更新会立即反映到目标目录</span>
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={directoryApplyMode === "copy"}
                  className={`skill-install-mode-opt ${directoryApplyMode === "copy" ? "active" : ""}`}
                  onClick={() => setDirectoryApplyMode("copy")}
                  disabled={Boolean(exportingId)}
                >
                  <span className="skill-install-mode-title">完整复制</span>
                  <span className="skill-install-mode-desc">复制完整目录，目标不依赖技能库</span>
                </button>
              </div>
            </div>
            <p className="skill-add-hint">
              确认后选择目标目录，将在其中创建同名技能目录；若同名项已存在，将不会覆盖。
            </p>
          </div>
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setDirectoryApplyTarget(null)}
              disabled={Boolean(exportingId)}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmDirectoryApply()}
              disabled={Boolean(exportingId)}
            >
              {exportingId ? "应用中…" : "选择目录并应用"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== 批量应用到目录弹窗 ===== */}
      <div
        className={`modal-overlay ${batchDirectoryApplyOpen ? "visible" : ""}`}
        {...batchDirectoryApplyDismiss}
      >
        <div className="modal skill-edit-modal">
          <div className="modal-header">
            <h2 className="modal-title">批量应用到目录（{validSelectedIds.size} 个技能）</h2>
            <button
              className="modal-close"
              onClick={() => !isBatchRunning && setBatchDirectoryApplyOpen(false)}
              disabled={isBatchRunning}
            >
              <IconClose />
            </button>
          </div>
          <div className="modal-body">
            <div className="form-group">
              <label className="form-label">安装方式</label>
              <div className="skill-install-mode" role="radiogroup" aria-label="安装方式">
                <button
                  type="button"
                  role="radio"
                  aria-checked={batchDirectoryApplyMode === "link"}
                  className={`skill-install-mode-opt ${batchDirectoryApplyMode === "link" ? "active" : ""}`}
                  onClick={() => setBatchDirectoryApplyMode("link")}
                  disabled={isBatchRunning}
                >
                  <span className="skill-install-mode-title">软链接</span>
                  <span className="skill-install-mode-desc">技能库更新会立即反映到目标目录</span>
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={batchDirectoryApplyMode === "copy"}
                  className={`skill-install-mode-opt ${batchDirectoryApplyMode === "copy" ? "active" : ""}`}
                  onClick={() => setBatchDirectoryApplyMode("copy")}
                  disabled={isBatchRunning}
                >
                  <span className="skill-install-mode-title">完整复制</span>
                  <span className="skill-install-mode-desc">复制完整目录，目标不依赖技能库</span>
                </button>
              </div>
            </div>
            <p className="skill-add-hint">
              确认后选择一个目标目录，所有选中技能将以相同方式应用到该目录；同名项不会被覆盖。
            </p>
          </div>
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setBatchDirectoryApplyOpen(false)}
              disabled={isBatchRunning}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmBatchDirectoryApply()}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              {isBatchRunning ? "应用中…" : "选择目录并应用"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Delete Skill Modal ===== */}
      <div
        className={`modal-overlay ${deleteTarget ? "visible" : ""}`}
        {...deleteDismiss}
      >
        <div className="modal" style={{ width: 400 }}>
          <div className="modal-header">
            <h2 className="modal-title">确认删除</h2>
            <button
              className="modal-close"
              onClick={() => !isDeleting && setDeleteTarget(null)}
              disabled={isDeleting}
            >
              <IconClose />
            </button>
          </div>
          <div className="confirm-body">
            <div className="confirm-text">
              确定删除技能{deleteTarget ? `「${deleteTarget.title}」` : ""}吗？
            </div>
            <div className="confirm-subtext">
              将从技能库删除 <code>~/.agentbuddy/skills</code> 下的对应目录，此操作不可撤销。
            </div>
            <label className="ui-check">
              <input
                type="checkbox"
                className="ui-check-input"
                checked={deleteAgentCopies}
                onChange={(e) => setDeleteAgentCopies(e.target.checked)}
                disabled={isDeleting}
              />
              <CheckGlyph />
              <span className="ui-check-label">同时删除已应用到各 Agent 的 Skill 副本</span>
            </label>
          </div>
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setDeleteTarget(null)}
              disabled={isDeleting}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-danger"
              onClick={() => void confirmDelete()}
              disabled={isDeleting}
            >
              {isDeleting ? "删除中…" : "删除"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== 批量删除确认弹窗 ===== */}
      <div
        className={`modal-overlay ${batchDeleteOpen ? "visible" : ""}`}
        {...batchDeleteDismiss}
      >
        <div className="modal" style={{ width: 440 }}>
          <div className="modal-header">
            <h2 className="modal-title">批量删除</h2>
            <button
              className="modal-close"
              onClick={() => !isBatchRunning && setBatchDeleteOpen(false)}
              disabled={isBatchRunning}
            >
              <IconClose />
            </button>
          </div>
          <div className="confirm-body">
            <div className="confirm-text">
              确定删除选中的 <strong>{validSelectedIds.size}</strong> 个技能吗？
            </div>
            <div className="confirm-subtext">
              将从技能库删除 <code>~/.agentbuddy/skills</code> 下的对应目录，此操作不可撤销。
            </div>
            {(() => {
              const titles = skills
                .filter((s) => validSelectedIds.has(s.id))
                .map((s) => s.title);
              const shown = titles.slice(0, 6);
              return (
                <div className="skill-batch-namelist">
                  {shown.map((t, i) => (
                    <span key={i} className="skill-batch-nametag">
                      {t}
                    </span>
                  ))}
                  {titles.length > shown.length && (
                    <span className="skill-batch-nametag skill-batch-nametag-more">
                      等 {titles.length} 个
                    </span>
                  )}
                </div>
              );
            })()}
            <label className="ui-check">
              <input
                type="checkbox"
                className="ui-check-input"
                checked={batchDeleteAgentCopies}
                onChange={(e) => setBatchDeleteAgentCopies(e.target.checked)}
                disabled={isBatchRunning}
              />
              <CheckGlyph />
              <span className="ui-check-label">同时删除已应用到各 Agent 的 Skill 副本</span>
            </label>
          </div>
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setBatchDeleteOpen(false)}
              disabled={isBatchRunning}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-danger"
              onClick={() => void confirmBatchDelete()}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              {isBatchRunning ? "删除中…" : `删除（${validSelectedIds.size}）`}
            </button>
          </div>
        </div>
      </div>

      {/* ===== 批量应用到 Agent 弹窗 ===== */}
      <div
        className={`modal-overlay ${batchApplyOpen ? "visible" : ""}`}
        {...batchApplyDismiss}
      >
        <div className="modal skill-edit-modal">
          <div className="modal-header">
            <h2 className="modal-title">
              批量应用到 Agent（{validSelectedIds.size} 个技能）
            </h2>
            <button
              className="modal-close"
              onClick={() => !isBatchRunning && setBatchApplyOpen(false)}
              disabled={isBatchRunning}
            >
              <IconClose />
            </button>
          </div>

          <div className="modal-body">
            {/* 模式：追加 / 覆盖 */}
            <div className="form-group">
              <label className="form-label">应用方式</label>
              <div className="skill-batch-mode" role="radiogroup" aria-label="应用方式">
                <button
                  type="button"
                  role="radio"
                  aria-checked={batchApplyMode === "add"}
                  className={`skill-batch-mode-opt ${batchApplyMode === "add" ? "active" : ""}`}
                  onClick={() => setBatchApplyMode("add")}
                  disabled={isBatchRunning}
                >
                  <span className="skill-batch-mode-title">追加</span>
                  <span className="skill-batch-mode-desc">
                    并入各技能现有应用，不解除任何已应用的 Agent
                  </span>
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={batchApplyMode === "replace"}
                  className={`skill-batch-mode-opt ${batchApplyMode === "replace" ? "active" : ""}`}
                  onClick={() => setBatchApplyMode("replace")}
                  disabled={isBatchRunning}
                >
                  <span className="skill-batch-mode-title">覆盖</span>
                  <span className="skill-batch-mode-desc">
                    以下方选择为最终态，未勾选的 Agent 将被解除
                  </span>
                </button>
              </div>
            </div>

            <div className="form-group">
              <label className="form-label">安装方式</label>
              <div className="skill-install-mode" role="radiogroup" aria-label="安装方式">
                <button
                  type="button"
                  role="radio"
                  aria-checked={batchInstallMode === "link"}
                  className={`skill-install-mode-opt ${batchInstallMode === "link" ? "active" : ""}`}
                  onClick={() => setBatchInstallMode("link")}
                  disabled={isBatchRunning}
                >
                  <span className="skill-install-mode-title">软链接</span>
                  <span className="skill-install-mode-desc">技能库更新会立即反映到 Agent</span>
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={batchInstallMode === "copy"}
                  className={`skill-install-mode-opt ${batchInstallMode === "copy" ? "active" : ""}`}
                  onClick={() => setBatchInstallMode("copy")}
                  disabled={isBatchRunning}
                >
                  <span className="skill-install-mode-title">完整复制</span>
                  <span className="skill-install-mode-desc">复制完整目录，Agent 不依赖技能库</span>
                </button>
              </div>
            </div>

            {/* 选择 Agent */}
            <div className="form-group">
              <div className="agent-pick-header">
                <label className="form-label">
                  目标 Agent
                  {batchApplyAgents.size > 0 && (
                    <span className="form-label-optional">已选 {batchApplyAgents.size} 个</span>
                  )}
                </label>
              </div>
              <p className="skill-add-hint">
                {batchApplyMode === "replace"
                  ? `将把每个选中技能的应用最终态设为下列勾选的 Agent（${batchInstallMode === "link" ? "软链接" : "完整复制"}）。`
                  : `将把下列勾选的 Agent 追加到每个选中技能的应用列表（${batchInstallMode === "link" ? "软链接" : "完整复制"}）。`}
              </p>
              {agents.length === 0 ? (
                <div className="agent-pick-empty">
                  暂无已安装的 Agent，请先到「Agent 管理」完成扫描
                </div>
              ) : (
                <div className="agent-pick-grid">
                  {agents.map((agent) => {
                    const unsupported = SKILL_UNSUPPORTED_AGENTS.has(agent.name);
                    const selected = batchApplyAgents.has(agent.name);
                    return (
                      <button
                        key={agent.name}
                        type="button"
                        className={`agent-pick ${selected ? "selected" : ""} ${
                          unsupported ? "disabled" : ""
                        }`}
                        onClick={() => toggleBatchApplyAgent(agent.name)}
                        disabled={unsupported || isBatchRunning}
                        data-tooltip={unsupported ? "该 Agent 暂无标准 Skills 目录" : undefined}
                      >
                        <span className={`agent-pick-icon ${selected ? "found" : ""}`}>
                          {getAgentIcon(agent.name) ?? agent.icon}
                        </span>
                        <span className="agent-pick-name">{agent.display_name}</span>
                        <span className={`agent-pick-check ${selected ? "checked" : ""}`}>
                          {selected ? "✓" : ""}
                        </span>
                      </button>
                    );
                  })}
                </div>
              )}
              {batchApplyMode === "replace" && batchApplyAgents.size === 0 && (
                <div className="skill-edit-warning">
                  未勾选任何 Agent：将从所有选中技能解除全部已应用的 Agent。
                </div>
              )}
            </div>
          </div>

          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setBatchApplyOpen(false)}
              disabled={isBatchRunning}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmBatchApply()}
              disabled={
                isBatchRunning ||
                validSelectedIds.size === 0 ||
                (batchApplyMode === "add" && batchApplyAgents.size === 0)
              }
            >
              {isBatchRunning
                ? "同步中…"
                : batchApplyMode === "replace"
                  ? "覆盖应用"
                  : "追加应用"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== 批量设置标签弹窗 ===== */}
      <div
        className={`modal-overlay ${batchTagOpen ? "visible" : ""}`}
        {...batchTagDismiss}
      >
        <div className="modal skill-edit-modal">
          <div className="modal-header">
            <h2 className="modal-title">
              批量设置标签（{validSelectedIds.size} 个技能）
            </h2>
            <button
              className="modal-close"
              onClick={() => !isBatchRunning && setBatchTagOpen(false)}
              disabled={isBatchRunning}
            >
              <IconClose />
            </button>
          </div>

          <div className="modal-body">
            <div className="form-group">
              <label className="form-label">
                标签 <span className="form-label-optional">留空表示清除标签</span>
              </label>
              <p className="skill-add-hint">
                仅修改所选技能的分组标签，不影响其应用到 Agent 的状态。
              </p>
              <TagSelect
                value={batchTag}
                onChange={setBatchTag}
                knownTags={knownTags}
                disabled={isBatchRunning}
              />
              {batchTag.trim() === "" && (
                <div className="skill-edit-warning">
                  未填写标签：将清除所选技能的现有标签。
                </div>
              )}
            </div>
          </div>

          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setBatchTagOpen(false)}
              disabled={isBatchRunning}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmBatchTag()}
              disabled={isBatchRunning || validSelectedIds.size === 0}
            >
              {isBatchRunning
                ? "处理中…"
                : batchTag.trim() === ""
                  ? "清除标签"
                  : "应用标签"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
