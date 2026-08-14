#!/usr/bin/env python3
"""检查路由聚合的客户端指纹代码与 CLIProxyAPI 上游的提交漂移。

路由聚合当前只支持同协议透传，代码同步范围仅包括 Claude Code / Codex CLI
客户端指纹（cloaking）。不会检查或维护协议转换代码。
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
SYNC_STATE_PATH = REPO_ROOT / "docs" / "cli_proxy_api_sync_state.json"
CLIPROXYAPI_REPO = "router-for-me/CLIProxyAPI"
GITHUB_API = f"https://api.github.com/repos/{CLIPROXYAPI_REPO}"
SLA_DAYS = 14


@dataclass
class PathStatus:
    """单个本地文件对应的上游路径状态。"""

    local_path: str
    upstream_path: str
    last_verified_short_sha: str
    last_verified_full_sha: str
    last_verified_date: str
    upstream_short_sha: str
    upstream_full_sha: str
    upstream_date: str
    drift_days: int
    status: str
    note: str = ""


def fetch_upstream_commits(
    github_token: str | None, path: str, per_page: int = 20
) -> list[dict[str, Any]]:
    """拉取上游某个目录最近的提交。"""

    url = f"{GITHUB_API}/commits?path={path}&per_page={per_page}"
    request = urllib.request.Request(
        url, headers={"Accept": "application/vnd.github+json"}
    )
    if github_token:
        request.add_header("Authorization", f"Bearer {github_token}")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        if error.code == 403:
            print(
                f"[warn] GitHub API 403（可能触发限流）跳过 path={path}",
                file=sys.stderr,
            )
            return []
        raise


def days_since(iso_date: str) -> int:
    date = datetime.fromisoformat(iso_date.replace("Z", "+00:00"))
    return (datetime.now(timezone.utc) - date).days


def check_path(
    local_path: str,
    upstream_path: str,
    last_verified_full: str,
    last_verified_short: str,
    last_verified_date: str,
    github_token: str | None,
) -> PathStatus:
    commits = fetch_upstream_commits(github_token, upstream_path)
    if not commits:
        return PathStatus(
            local_path,
            upstream_path,
            last_verified_short,
            last_verified_full,
            last_verified_date,
            "?",
            "",
            "",
            -1,
            "warning",
            "无法拉取 upstream commits（限流 / 网络）",
        )

    latest = commits[0]
    upstream_full = latest["sha"]
    upstream_short = upstream_full[:7]
    upstream_date = latest["commit"]["author"]["date"]
    drift = days_since(upstream_date)

    if upstream_full == last_verified_full:
        status, note = "ok", "已对齐 upstream HEAD"
    elif upstream_full.startswith(last_verified_short) or last_verified_full.startswith(
        upstream_short
    ):
        status, note = "ok", "已对齐（短哈希匹配）"
    elif drift <= SLA_DAYS:
        status, note = "warning", f"上游 {drift}d 内有新 commit"
    else:
        status, note = "error", f"上游 {drift}d 未同步（超 SLA {SLA_DAYS}d）"

    return PathStatus(
        local_path,
        upstream_path,
        last_verified_short,
        last_verified_full,
        last_verified_date,
        upstream_short,
        upstream_full,
        upstream_date,
        drift,
        status,
        note,
    )


def main() -> int:
    if not SYNC_STATE_PATH.exists():
        print(f"[error] 同步状态文件不存在: {SYNC_STATE_PATH}", file=sys.stderr)
        return 2

    state = json.loads(SYNC_STATE_PATH.read_text(encoding="utf-8"))
    github_token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")

    passthrough_paths = state.get("passthrough_paths", [])
    if passthrough_paths:
        print(f"\n=== Passthrough 链路覆盖 ({len(passthrough_paths)} paths) ===\n")
        for path in passthrough_paths:
            print(f"  • {path.get('label', path.get('id', '?'))}")
            print(f"      入站     : {path.get('inbound', '?')}")
            print(f"      客户端   : {path.get('client', '?')}")
            print(f"      upstream : {path.get('upstream_format', '?')}")
            print(f"      传输     : {path.get('transport', '?')}")
            print(f"      header   : {path.get('header_rewrite', '?')}\n")

    checked_anchors: list[tuple[dict[str, Any], list[PathStatus]]] = []
    anchors = state.get("client_fingerprint_anchors", [])
    for anchor in anchors:
        label = anchor.get("label", anchor.get("id", "?"))
        version = anchor.get("current_version", "?")
        files = anchor.get("files", [])
        print(f"=== Cloaking 客户端指纹：{label}（{version}，{len(files)} files）===\n")
        statuses: list[PathStatus] = []
        for file in files:
            status = check_path(
                file.get("local_path", "?"),
                file.get("upstream_path", ""),
                file.get("last_verified_full_sha", "?"),
                file.get("last_verified_short_sha", "?"),
                file.get("last_verified_date", "?"),
                github_token,
            )
            statuses.append(status)
            icon = {"ok": "✓", "warning": "⚠", "error": "✗"}.get(
                status.status, "?"
            )
            print(
                f"  {icon} {status.local_path}\n"
                f"      AgentBuddy: {status.last_verified_short_sha} ({status.last_verified_date})\n"
                f"      Upstream  : {status.upstream_short_sha} ({status.upstream_date}, {status.drift_days}d 前)\n"
                f"      → {status.upstream_path}\n"
                f"      Status    : {status.status} — {status.note}\n"
            )
        checked_anchors.append((anchor, statuses))

    new_anchors = []
    for anchor, statuses in checked_anchors:
        new_files = []
        for original, status in zip(anchor.get("files", []), statuses):
            new_files.append(
                {
                    **original,
                    "last_verified_short_sha": status.upstream_short_sha,
                    "last_verified_full_sha": status.upstream_full_sha,
                    "last_verified_date": status.upstream_date,
                    "drift_days": status.drift_days,
                    "status": status.status,
                }
            )
        new_anchors.append({**anchor, "files": new_files})

    new_state = {
        **state,
        "schema_version": 6,
        "last_checked_at": datetime.now(timezone.utc).isoformat(),
        "sla_days": SLA_DAYS,
        "upstream_repo": CLIPROXYAPI_REPO,
        "client_fingerprint_anchors": new_anchors,
    }
    for legacy_key in ("pairs", "translator_pairs", "removed_pairs"):
        new_state.pop(legacy_key, None)
    new_path = SYNC_STATE_PATH.with_suffix(".json.new")
    new_path.write_text(
        json.dumps(new_state, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"新 sync state 已写到 {new_path}")

    any_error = any(
        status.status == "error" for _, statuses in checked_anchors for status in statuses
    )
    return 1 if any_error else 0


if __name__ == "__main__":
    raise SystemExit(main())
