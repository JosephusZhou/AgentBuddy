import { useState, useEffect, useCallback, useRef } from "react";
import { useStatusMessage } from "@/lib/useStatusMessage";
import { Toast } from "@/components/Toast";
import { useOverlayDismiss } from "../ui";

/* ===== Types ===== */
type ConnectionStatus = "connected" | "disconnected" | "checking";

interface WebDAVConnection {
  id: string;
  name: string;
  url: string;
  username: string;
  status: ConnectionStatus;
  lastError?: string;
  lastCheckedAt?: number | null;
  createdAt?: number;
  updatedAt?: number;
}

interface WebDavTestResult {
  ok: boolean;
  status: string;
  message: string;
  httpStatus?: number | null;
}

/* ===== Invoke helpers ===== */
async function invokeGetConnections(): Promise<WebDAVConnection[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const rows = await (invoke("get_webdav_connections") as Promise<
    Array<{
      id: string;
      name: string;
      url: string;
      username: string;
      status?: string;
      lastError?: string;
      lastCheckedAt?: number | null;
      createdAt?: number;
      updatedAt?: number;
    }>
  >);
  return rows.map((r) => ({
    id: r.id,
    name: r.name,
    url: r.url,
    username: r.username,
    status: r.status === "connected" ? "connected" : "disconnected",
    lastError: r.lastError || "",
    lastCheckedAt: r.lastCheckedAt ?? null,
    createdAt: r.createdAt,
    updatedAt: r.updatedAt,
  }));
}

async function invokeUpsert(payload: {
  id: string;
  name: string;
  url: string;
  username: string;
  password?: string;
}): Promise<WebDAVConnection> {
  const { invoke } = await import("@tauri-apps/api/core");
  const r = await (invoke("upsert_webdav_connection", { payload }) as Promise<{
    id: string;
    name: string;
    url: string;
    username: string;
    status?: string;
    lastError?: string;
    lastCheckedAt?: number | null;
    createdAt?: number;
    updatedAt?: number;
  }>);
  return {
    id: r.id,
    name: r.name,
    url: r.url,
    username: r.username,
    status: r.status === "connected" ? "connected" : "disconnected",
    lastError: r.lastError || "",
    lastCheckedAt: r.lastCheckedAt ?? null,
    createdAt: r.createdAt,
    updatedAt: r.updatedAt,
  };
}

async function invokeDelete(id: string): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("delete_webdav_connection", { id });
}

async function invokeTest(id: string): Promise<WebDavTestResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("test_webdav_connection", { id }) as Promise<WebDavTestResult>;
}

/* ===== SVG Icons ===== */
const IconPlus = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <line x1="12" y1="5" x2="12" y2="19" />
    <line x1="5" y1="12" x2="19" y2="12" />
  </svg>
);

const IconTrash = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="3 6 5 6 21 6" />
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
    <line x1="10" y1="11" x2="10" y2="17" />
    <line x1="14" y1="11" x2="14" y2="17" />
  </svg>
);

const IconClose = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <line x1="18" y1="6" x2="6" y2="18" />
    <line x1="6" y1="6" x2="18" y2="18" />
  </svg>
);

const IconTrashConfirm = () => (
  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="3 6 5 6 21 6" />
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
  </svg>
);

const IconEdit = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
  </svg>
);

const IconRefresh = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="23 4 23 10 17 10" />
    <polyline points="1 20 1 14 7 14" />
    <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
  </svg>
);

const IconEmpty = () => (
  <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" />
  </svg>
);

/* ===== Component ===== */
export default function WebDAV() {
  const [connections, setConnections] = useState<WebDAVConnection[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useStatusMessage();
  const [formError, setFormError] = useState("");
  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);

  const formDismiss = useOverlayDismiss(() => setShowForm(false), !isSaving);
  const deleteDismiss = useOverlayDismiss(() => setDeleteTarget(null), !isDeleting);
  const [testingIds, setTestingIds] = useState<Set<string>>(new Set());

  const [formName, setFormName] = useState("");
  const [formUrl, setFormUrl] = useState("");
  const [formUsername, setFormUsername] = useState("");
  const [formPassword, setFormPassword] = useState("");
  const idCounter = useRef(0);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const hasLoaded = useRef(false);

  const nextId = useCallback(() => {
    idCounter.current += 1;
    return `webdav-${Date.now()}-${idCounter.current}`;
  }, []);

  const loadConnections = useCallback(async () => {
    try {
      const rows = await invokeGetConnections();
      setConnections(rows);
    } catch (err) {
      setStatusMsg(`加载 WebDAV 连接失败：${err instanceof Error ? err.message : String(err)}`);
      setConnections([]);
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    if (hasLoaded.current) return;
    hasLoaded.current = true;
    void loadConnections();
  }, [loadConnections]);

  useEffect(() => {
    if (showForm) {
      setTimeout(() => nameInputRef.current?.focus(), 100);
    }
  }, [showForm]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (!isSaving) setShowForm(false);
        if (!isDeleting) setDeleteTarget(null);
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [isSaving, isDeleting]);

  const openAdd = useCallback(() => {
    setEditingId(null);
    setFormName("");
    setFormUrl("");
    setFormUsername("");
    setFormPassword("");
    setFormError("");
    setShowForm(true);
  }, []);

  const openEdit = useCallback((conn: WebDAVConnection) => {
    setEditingId(conn.id);
    setFormName(conn.name);
    setFormUrl(conn.url);
    setFormUsername(conn.username);
    setFormPassword("");
    setFormError("");
    setShowForm(true);
  }, []);

  const runTest = useCallback(async (id: string, displayName?: string) => {
    setTestingIds((prev) => new Set(prev).add(id));
    setConnections((prev) =>
      prev.map((c) => (c.id === id ? { ...c, status: "checking" as const } : c)),
    );
    const label = displayName?.trim() || id;
    try {
      const result = await invokeTest(id);
      setConnections((prev) =>
        prev.map((c) =>
          c.id === id
            ? {
                ...c,
                status: result.ok ? "connected" : "disconnected",
                lastError: result.ok ? "" : result.message,
                lastCheckedAt: Math.floor(Date.now() / 1000),
              }
            : c,
        ),
      );
      setStatusMsg(
        result.ok ? `「${label}」${result.message}` : `「${label}」测试失败：${result.message}`,
      );
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setConnections((prev) =>
        prev.map((c) =>
          c.id === id
            ? {
                ...c,
                status: "disconnected",
                lastError: message,
                lastCheckedAt: Math.floor(Date.now() / 1000),
              }
            : c,
        ),
      );
      setStatusMsg(`「${label}」测试失败：${message}`);
    } finally {
      setTestingIds((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
    }
  }, []);

  const handleSave = useCallback(async () => {
    const name = formName.trim();
    const url = formUrl.trim();
    const username = formUsername.trim();
    const passwordTrimmed = formPassword.trim();

    if (!name || !url || !username) {
      setFormError("请填写名称、服务器地址和用户名");
      return;
    }
    if (!editingId && !passwordTrimmed) {
      setFormError("新建连接时密码不能为空");
      return;
    }
    if (!(url.startsWith("http://") || url.startsWith("https://"))) {
      setFormError("服务器地址必须以 http:// 或 https:// 开头");
      return;
    }

    setIsSaving(true);
    setFormError("");
    try {
      const id = editingId ?? nextId();
      const payload: {
        id: string;
        name: string;
        url: string;
        username: string;
        password?: string;
      } = { id, name, url, username };
      if (passwordTrimmed) {
        payload.password = passwordTrimmed;
      }

      const saved = await invokeUpsert(payload);
      setConnections((prev) => {
        const exists = prev.some((c) => c.id === saved.id);
        if (exists) {
          return prev.map((c) => (c.id === saved.id ? { ...c, ...saved } : c));
        }
        return [saved, ...prev];
      });
      setShowForm(false);
      setFormPassword("");
      setStatusMsg(editingId ? "连接已更新，正在测试连通性…" : "连接已保存，正在测试连通性…");
      void runTest(saved.id, saved.name);
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsSaving(false);
    }
  }, [formName, formUrl, formUsername, formPassword, editingId, nextId, runTest]);

  const handleDelete = useCallback(async () => {
    if (deleteTarget === null) return;
    setIsDeleting(true);
    try {
      await invokeDelete(deleteTarget);
      setConnections((prev) => prev.filter((c) => c.id !== deleteTarget));
      setDeleteTarget(null);
      setStatusMsg("连接已删除");
    } catch (err) {
      setStatusMsg(`删除失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setIsDeleting(false);
    }
  }, [deleteTarget]);

  const deleteName = connections.find((c) => c.id === deleteTarget)?.name;

  return (
    <>
      <div className="content-header">
        <h1 className="content-title">WebDAV</h1>
      </div>
      <div className="content-body">
        <div className="webdav-header">
          <span className="webdav-count">
            {loaded ? `${connections.length} 个连接` : "加载中…"}
          </span>
          <button className="btn-add" onClick={openAdd} title="添加 WebDAV 连接">
            <IconPlus />
          </button>
        </div>

        <Toast message={statusMsg} />

        {!loaded ? null : connections.length === 0 ? (
          <div className="empty-state">
            <IconEmpty />
            <div className="empty-state-text">暂无 WebDAV 连接，点击右上角添加</div>
          </div>
        ) : (
          <div className="webdav-list">
            {connections.map((conn) => {
              const isTesting = testingIds.has(conn.id) || conn.status === "checking";
              return (
                <div
                  key={conn.id}
                  className={`webdav-item ${isTesting ? "checking" : ""}`}
                >
                  <button
                    type="button"
                    className={`status-dot ${isTesting ? "checking" : conn.status}`}
                    title={isTesting ? "检测中…" : "重新检测连接"}
                    onClick={() => {
                      if (!isTesting) void runTest(conn.id, conn.name);
                    }}
                    disabled={isTesting}
                    style={{
                      border: "none",
                      padding: 0,
                      cursor: isTesting ? "default" : "pointer",
                      background: "transparent",
                    }}
                  />
                  <div className="webdav-info">
                    <div className="webdav-name">{conn.name}</div>
                    <div className="webdav-detail">{conn.url}</div>
                    <div className="webdav-detail">用户: {conn.username}</div>
                    {conn.status === "disconnected" && conn.lastError ? (
                      <div className="webdav-detail" style={{ color: "var(--seed-danger)" }}>
                        {conn.lastError}
                      </div>
                    ) : null}
                  </div>
                  <button
                    className="btn-delete"
                    onClick={() => void runTest(conn.id, conn.name)}
                    title="重新检测"
                    disabled={isTesting}
                    style={{ opacity: 1 }}
                  >
                    <IconRefresh />
                  </button>
                  <button
                    className="btn-delete"
                    onClick={() => openEdit(conn)}
                    title="编辑连接"
                    style={{ opacity: 1 }}
                  >
                    <IconEdit />
                  </button>
                  <button
                    className="btn-delete"
                    onClick={() => setDeleteTarget(conn.id)}
                    title="删除连接"
                    style={{ opacity: 1 }}
                  >
                    <IconTrash />
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* ===== Add / Edit Modal ===== */}
      <div
        className={`modal-overlay ${showForm ? "visible" : ""}`}
        {...formDismiss}
      >
        <div className="modal">
          <div className="modal-header">
            <h2 className="modal-title">
              {editingId ? "编辑 WebDAV 连接" : "添加 WebDAV 连接"}
            </h2>
            <button
              className="modal-close"
              onClick={() => !isSaving && setShowForm(false)}
              disabled={isSaving}
            >
              <IconClose />
            </button>
          </div>
          <div className="modal-body">
            <div className="form-group">
              <label className="form-label" htmlFor="webdav-name">名称</label>
              <input
                ref={nameInputRef}
                type="text"
                className="form-input"
                id="webdav-name"
                placeholder="输入连接名称"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                disabled={isSaving}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="webdav-url">服务器地址</label>
              <input
                type="url"
                className="form-input"
                id="webdav-url"
                placeholder="https://dav.example.com"
                value={formUrl}
                onChange={(e) => setFormUrl(e.target.value)}
                disabled={isSaving}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="webdav-username">用户名</label>
              <input
                type="text"
                className="form-input"
                id="webdav-username"
                placeholder="输入用户名"
                value={formUsername}
                onChange={(e) => setFormUsername(e.target.value)}
                disabled={isSaving}
              />
            </div>
            <div className="form-group">
              <label className="form-label" htmlFor="webdav-password">密码</label>
              <input
                type="password"
                className="form-input"
                id="webdav-password"
                placeholder={editingId ? "留空则保持原密码" : "输入密码"}
                value={formPassword}
                onChange={(e) => setFormPassword(e.target.value)}
                disabled={isSaving}
                autoComplete="new-password"
              />
            </div>
            {formError && <div className="mcp-form-error">{formError}</div>}
          </div>
          <div className="modal-footer">
            <button
              className="btn btn-secondary"
              onClick={() => setShowForm(false)}
              disabled={isSaving}
            >
              取消
            </button>
            <button
              className="btn btn-primary"
              onClick={() => void handleSave()}
              disabled={isSaving}
            >
              {isSaving ? "保存中…" : "保存"}
            </button>
          </div>
        </div>
      </div>

      {/* ===== Delete Confirm Modal ===== */}
      <div
        className={`modal-overlay ${deleteTarget !== null ? "visible" : ""}`}
        {...deleteDismiss}
      >
        <div className="modal" style={{ width: 380 }}>
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
            <div className="confirm-icon">
              <IconTrashConfirm />
            </div>
            <div className="confirm-text">
              确定要删除{deleteName ? `「${deleteName}」` : "此 WebDAV 连接"}吗？
            </div>
            <div className="confirm-subtext">
              删除后将无法恢复，需要重新配置连接信息。
            </div>
          </div>
          <div className="modal-footer">
            <button
              className="btn btn-secondary"
              onClick={() => setDeleteTarget(null)}
              disabled={isDeleting}
            >
              取消
            </button>
            <button
              className="btn btn-danger"
              onClick={() => void handleDelete()}
              disabled={isDeleting}
            >
              {isDeleting ? "删除中…" : "删除"}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
