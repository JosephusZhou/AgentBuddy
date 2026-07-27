import { useState } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { useOverlayDismiss } from "../ui";
import { getAgentIcon } from "../agent-icons";
import { AGENT_PROJECT_INFOS, type InitMode, type CheckResult, type InitResult } from "./project-config/types";
import { invokePickProjectFolder, invokeCheckProjectConfig, invokeInitProjectConfig } from "./project-config/api";
import { Folder } from "lucide-react";

const IconFolder = () => (
  <Folder size={16} strokeWidth={1.8} />
);

export default function ProjectConfig() {
  const [targetDir, setTargetDir] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [mode, setMode] = useState<InitMode>("symlink");
  const [busy, setBusy] = useState(false);
  const [checkResult, setCheckResult] = useState<CheckResult | null>(null);
  const [initResult, setInitResult] = useState<InitResult | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [statusMsg, setStatusMsg] = useStatusMessage();

  const confirmDismiss = useOverlayDismiss(() => !busy && setConfirmOpen(false), !busy);

  const toggleAgent = (name: string) => {
    setSelected(prev => {
      const next = new Set(prev);
      next.has(name) ? next.delete(name) : next.add(name);
      return next;
    });
  };

  const handlePickDir = async () => {
    try {
      const dir = await invokePickProjectFolder();
      if (dir) {
        setTargetDir(dir);
        setInitResult(null);
        setCheckResult(null);
      }
    } catch (e) {
      setStatusMsg(String(e));
    }
  };

  const doInit = async (overwrite: boolean) => {
    setConfirmOpen(false);
    setBusy(true);
    setStatusMsg("初始化中…");
    try {
      const agents = Array.from(selected).map(name => ({ name }));
      const result = await invokeInitProjectConfig(targetDir, agents, mode, overwrite);
      setInitResult(result);
      setCheckResult(null);
      setStatusMsg(result.errors.length > 0 ? `完成，但有 ${result.errors.length} 个错误` : "初始化完成");
    } catch (e) {
      setStatusMsg(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleInit = async () => {
    if (!targetDir) {
      setStatusMsg("请先选择目标目录");
      return;
    }
    if (selected.size === 0) {
      setStatusMsg("请至少勾选一个 Agent");
      return;
    }
    setBusy(true);
    setStatusMsg("检查中…");
    setInitResult(null);
    try {
      const agents = Array.from(selected).map(name => ({ name }));
      const result = await invokeCheckProjectConfig(targetDir, agents, mode);
      setCheckResult(result);
      if (result.existing.length > 0) {
        setConfirmOpen(true);
        setStatusMsg("");
        setBusy(false);
        return;
      }
      // No conflicts — run init while staying in busy state (single busy lifecycle).
      setStatusMsg("初始化中…");
      const init = await invokeInitProjectConfig(targetDir, agents, mode, false);
      setInitResult(init);
      setCheckResult(null);
      setStatusMsg(init.errors.length > 0 ? `完成，但有 ${init.errors.length} 个错误` : "初始化完成");
    } catch (e) {
      setStatusMsg(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="content-header">
        <h1 className="content-title">项目AI配置</h1>
        <p className="content-desc">为项目目录初始化多个 AI coding agent 的配置文件和目录</p>
      </div>

      <div className={`content-body project-config-body${initResult ? " has-result" : ""}`}>
        <Toast message={statusMsg} />

        {/* 目录选择 */}
        <div className="form-group">
          <label className="form-label">目标目录</label>
          <div className="form-input-with-action">
            <input
              className="form-input"
              value={targetDir}
              readOnly
              placeholder="点击右侧按钮选择项目目录…"
            />
            <button className="form-input-action" onClick={handlePickDir} disabled={busy} data-tooltip="选择目录">
              <IconFolder />
            </button>
          </div>
        </div>

        {/* Agent 勾选 */}
        <div className="form-group">
          <div className="agent-pick-header">
            <label className="form-label">
              选择 Agent
              {selected.size > 0 && (
                <span className="form-label-optional">已选 {selected.size} 个</span>
              )}
            </label>
            <div className="mcp-agent-actions">
              <button
                type="button"
                className="mcp-agent-action"
                onClick={() => setSelected(new Set(AGENT_PROJECT_INFOS.map(a => a.name)))}
                disabled={busy}
              >
                全选
              </button>
              <button
                type="button"
                className="mcp-agent-action"
                onClick={() => setSelected(new Set())}
                disabled={busy}
              >
                清空
              </button>
            </div>
          </div>
          <div className="agent-pick-grid">
            {AGENT_PROJECT_INFOS.map(agent => {
              const sel = selected.has(agent.name);
              const icon = getAgentIcon(agent.name);
              return (
                <button
                  key={agent.name}
                  type="button"
                  className={`agent-pick ${sel ? "selected" : ""}`}
                  onClick={() => toggleAgent(agent.name)}
                  disabled={busy}
                >
                  <span className={`agent-pick-icon ${sel ? "found" : ""}`}>
                    {icon ?? agent.displayName.slice(0, 2).toUpperCase()}
                  </span>
                  <span className="agent-pick-name">{agent.displayName}</span>
                  <span className={`agent-pick-check ${sel ? "checked" : ""}`}>{sel ? "✓" : ""}</span>
                </button>
              );
            })}
          </div>
        </div>

        {/* 模式选择 */}
        <div className="form-group">
          <label className="form-label">初始化模式</label>
          <div className="proj-mode-list">
            {(["symlink", "full"] as InitMode[]).map(m => (
              <button
                key={m}
                type="button"
                className={`agent-pick proj-mode-option ${mode === m ? "selected" : ""}`}
                onClick={() => setMode(m)}
                disabled={busy}
              >
                <div className="proj-mode-option-row">
                  <span className={`agent-pick-check ${mode === m ? "checked" : ""}`}>{mode === m ? "✓" : ""}</span>
                  <span className="agent-pick-name">{m === "symlink" ? "软链接模式（推荐）" : "全量模式"}</span>
                </div>
                <span className="proj-mode-option-desc">
                  {m === "symlink"
                    ? "在 .agents/ 存放共享内容（commands / rules / skills / agents），各 agent 配置目录对应子项软链接到此处。与全量模式的子目录集合不同：此处为统一共享集。Windows 需开启开发者模式。"
                    : "每个 agent 按常用骨架独立创建配置文件与子目录，互不依赖、无软链接。"}
                </span>
              </button>
            ))}
          </div>
        </div>

        <div className="proj-init-actions">
          <button
            className="btn btn-primary"
            onClick={handleInit}
            disabled={busy || !targetDir || selected.size === 0}
          >
            {busy ? "处理中…" : "初始化"}
          </button>
        </div>

        {/* 结果展示 */}
        {initResult && (
          <div className="proj-init-result">
            {initResult.created.length > 0 && (
              <div className="proj-init-result-group">
                <div className="proj-init-result-title is-success">已创建 ({initResult.created.length})</div>
                {initResult.created.map(p => (
                  <div key={`c:${p}`} className="proj-init-result-line is-success">{p}</div>
                ))}
              </div>
            )}
            {initResult.skipped.length > 0 && (
              <div className="proj-init-result-group">
                <div className="proj-init-result-title is-muted">已跳过 ({initResult.skipped.length})</div>
                {initResult.skipped.map(p => (
                  <div key={`s:${p}`} className="proj-init-result-line is-muted">{p}</div>
                ))}
              </div>
            )}
            {initResult.errors.length > 0 && (
              <div className="proj-init-result-group">
                <div className="proj-init-result-title is-danger">错误 ({initResult.errors.length})</div>
                {initResult.errors.map(p => (
                  <div key={`e:${p}`} className="proj-init-result-line is-danger">{p}</div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* 确认覆盖 modal */}
      <div className={`modal-overlay ${confirmOpen ? "visible" : ""}`} {...confirmDismiss}>
        <div className="modal" onClick={e => e.stopPropagation()}>
          <div className="modal-header">
            <span className="modal-title">检测到已存在的文件</span>
            <button
              type="button"
              className="modal-close"
              onClick={() => !busy && setConfirmOpen(false)}
              disabled={busy}
            >
              ✕
            </button>
          </div>
          <div className="modal-body">
            <p className="proj-confirm-hint">
              以下路径已存在。「跳过」保留原样；「覆盖」仅替换文件内容与可安全删除的软链接/空目录，
              <strong>不会删除非空真实目录</strong>（以免误伤用户数据）。
            </p>
            <div className="proj-confirm-list">
              {checkResult?.existing.map(item => (
                <div key={item.path} className="proj-confirm-item">
                  {item.isDir ? "📁" : "📄"} {item.path}
                </div>
              ))}
            </div>
          </div>
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setConfirmOpen(false)}
              disabled={busy}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => void doInit(false)}
              disabled={busy}
            >
              跳过已存在
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void doInit(true)}
              disabled={busy}
            >
              覆盖
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
