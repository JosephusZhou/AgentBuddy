#!/usr/bin/env python3
"""检查 AgentBuddy translator 与 CLIProxyAPI upstream 的 commit 漂移。

CLIProxyAPI aligned: 每个 AgentBuddy 翻译器文件头注释对应的 commit SHA
与 CLIProxyAPI upstream 实际最新 commit 对比：
- 距上次同步 <= 14 天：warning
- 距上次同步 >  14 天：error（CI 报警）
- 上游无新 commit：silent

数据源：
- docs/cli_proxy_api_sync_state.json — AgentBuddy 已知的每对 pair 上游 commit
- GitHub API: https://api.github.com/repos/router-for-me/CLIProxyAPI/commits

CI 集成：
- GitHub Actions: .github/workflows/upstream-sync-check.yml
- 每周一 9:00 UTC 跑一次 + 手动 `workflow_dispatch`

输出：
- 打印每个 pair 的同步状态
- 退出码：0 = 全部在 SLA 内；1 = 有超期未同步
"""

from __future__ import annotations

import json
import os
import sys
import urllib.request
import urllib.error
from dataclasses import dataclass, asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
SYNC_STATE_PATH = REPO_ROOT / "docs" / "cli_proxy_api_sync_state.json"
CLIPROXYAPI_REPO = "router-for-me/CLIProxyAPI"
GITHUB_API = f"https://api.github.com/repos/{CLIPROXYAPI_REPO}"
SLA_DAYS = 14
SOURCE_FORMAT_LABELS = {
    "Anthropic": "Anthropic Messages",
    "OpenAiChat": "OpenAI Chat Completions",
    "OpenAiResponses": "OpenAI Responses API",
    "Gemini": "Google Gemini generateContent",
    "CodexNative": "Codex CLI native",
    "Interactions": "OpenAI Interactions API",
    "Antigravity": "Antigravity CLI",
}


@dataclass
class PairStatus:
    pair: str
    source: str
    target: str
    last_verified_short_sha: str
    last_verified_full_sha: str
    last_verified_date: str
    upstream_short_sha: str
    upstream_full_sha: str
    upstream_date: str
    drift_days: int
    status: str  # "ok" | "warning" | "error" | "ahead"
    note: str = ""


def fetch_upstream_commits_for_dir(github_token: str | None, path: str, per_page: int = 20) -> list[dict[str, Any]]:
    """拉取 upstream 仓库某个目录最近 N 个 commit。"""
    url = f"{GITHUB_API}/commits?path={path}&per_page={per_page}"
    req = urllib.request.Request(url, headers={"Accept": "application/vnd.github+json"})
    if github_token:
        req.add_header("Authorization", f"Bearer {github_token}")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        if e.code == 403:
            print(f"[warn] GitHub API 403（可能触发限流）跳过 path={path}", file=sys.stderr)
            return []
        raise


def days_since(iso_date: str) -> int:
    dt = datetime.fromisoformat(iso_date.replace("Z", "+00:00"))
    return (datetime.now(timezone.utc) - dt).days


def check_one_pair(
    pair: dict[str, Any],
    github_token: str | None,
) -> PairStatus:
    source = pair["source"]
    target = pair["target"]
    source_dir = pair["source_dir"]
    target_dir = pair["target_dir"]
    last_verified_full = pair["last_verified_full_sha"]
    last_verified_short = pair["last_verified_short_sha"]
    last_verified_date = pair["last_verified_date"]

    # 上游目录的最近 commit
    commits = fetch_upstream_commits_for_dir(github_token, target_dir)
    if not commits:
        return PairStatus(
            pair=f"{SOURCE_FORMAT_LABELS.get(source, source)} → {SOURCE_FORMAT_LABELS.get(target, target)}",
            source=source,
            target=target,
            last_verified_short_sha=last_verified_short,
            last_verified_full_sha=last_verified_full,
            last_verified_date=last_verified_date,
            upstream_short_sha="?",
            upstream_full_sha="",
            upstream_date="",
            drift_days=-1,
            status="warning",
            note="无法拉取 upstream commits（限流 / 网络）",
        )
    top = commits[0]
    upstream_full = top["sha"]
    upstream_short = upstream_full[:7]
    upstream_date = top["commit"]["author"]["date"]
    drift = days_since(upstream_date)

    if upstream_full == last_verified_full:
        status = "ok"
        note = "已对齐 upstream HEAD"
    elif upstream_full.startswith(last_verified_short) or last_verified_full.startswith(upstream_short):
        status = "ok"
        note = "已对齐（短哈希匹配）"
    else:
        if drift <= SLA_DAYS:
            status = "warning"
            note = f"上游 {drift}d 内有新 commit"
        else:
            status = "error"
            note = f"上游 {drift}d 未同步（超 SLA {SLA_DAYS}d）"

    return PairStatus(
        pair=f"{SOURCE_FORMAT_LABELS.get(source, source)} → {SOURCE_FORMAT_LABELS.get(target, target)}",
        source=source,
        target=target,
        last_verified_short_sha=last_verified_short,
        last_verified_full_sha=last_verified_full,
        last_verified_date=last_verified_date,
        upstream_short_sha=upstream_short,
        upstream_full_sha=upstream_full,
        upstream_date=upstream_date,
        drift_days=drift,
        status=status,
        note=note,
    )


def main() -> int:
    if not SYNC_STATE_PATH.exists():
        print(f"[error] 同步状态文件不存在: {SYNC_STATE_PATH}", file=sys.stderr)
        print("  请先根据 docs/SYNC_PLAYBOOK.md 创建初始 sync state", file=sys.stderr)
        return 2

    state = json.loads(SYNC_STATE_PATH.read_text(encoding="utf-8"))
    github_token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")

    pairs = state.get("pairs", [])
    if not pairs:
        print("[warn] sync state 中无任何 pair", file=sys.stderr)
        return 0

    statuses: list[PairStatus] = []
    for pair in pairs:
        try:
            status = check_one_pair(pair, github_token)
        except Exception as e:
            print(f"[error] 检查 {pair.get('source', '?')} → {pair.get('target', '?')} 失败: {e}", file=sys.stderr)
            continue
        statuses.append(status)

    # 输出表格
    print(f"\n=== CLIProxyAPI 上游同步检查 ({len(statuses)} pairs) ===\n")
    for s in statuses:
        icon = {"ok": "✓", "warning": "⚠", "error": "✗", "ahead": "?"}.get(s.status, "?")
        print(
            f"{icon} {s.pair}\n"
            f"    AgentBuddy: {s.last_verified_short_sha} ({s.last_verified_date})\n"
            f"    Upstream  : {s.upstream_short_sha} ({s.upstream_date}, {s.drift_days}d 前)\n"
            f"    Status    : {s.status} — {s.note}\n"
        )

    # 输出新 sync state（供 review 后落地）
    new_state = {
        "last_checked_at": datetime.now(timezone.utc).isoformat(),
        "sla_days": SLA_DAYS,
        "pairs": [
            {
                **pair,
                "upstream_short_sha": s.upstream_short_sha,
                "upstream_full_sha": s.upstream_full_sha,
                "upstream_date": s.upstream_date,
                "drift_days": s.drift_days,
                "status": s.status,
            }
            for pair, s in zip(pairs, statuses)
        ],
    }
    new_path = SYNC_STATE_PATH.with_suffix(".json.new")
    new_path.write_text(json.dumps(new_state, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"\n新 sync state 已写到 {new_path}")
    print("人工 review 后 `mv` 覆盖原文件")

    error_count = sum(1 for s in statuses if s.status == "error")
    if error_count > 0:
        print(f"\n[FAIL] {error_count} 个 pair 超 SLA {SLA_DAYS}d 未同步", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
