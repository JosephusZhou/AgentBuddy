#!/usr/bin/env python3
"""检查 AgentBuddy translator 与 CLIProxyAPI upstream 的 commit 漂移。

CLIProxyAPI aligned: 每个 AgentBuddy 翻译器文件头注释对应的 commit SHA
与 CLIProxyAPI upstream 实际最新 commit 对比：
- 距上次同步 <= 14 天：warning
- 距上次同步 >  14 天：error（CI 报警）
- 上游无新 commit：silent

每个 pair 内部追踪两个方向（请求方向 = source_dir，响应方向 = target_dir），
任一方向漂移都会报警。整体 status = max(source, target) 严重度。

数据源：
- docs/cli_proxy_api_sync_state.json — AgentBuddy 已知的每对 pair 两方向上游 commit
- GitHub API: https://api.github.com/repos/router-for-me/CLIProxyAPI/commits

CI 集成：
- GitHub Actions: .github/workflows/upstream-sync-check.yml
- 每周一 9:00 UTC 跑一次 + 手动 `workflow_dispatch`

输出：
- 打印每个 pair 两方向同步状态
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
class DirectionStatus:
    """一对 translator 的单个方向（请求 source_dir 或响应 target_dir）状态。"""
    direction: str  # "source" | "target"
    label: str  # "请求方向" | "响应方向"
    cli_proxy_api_dir: str  # CLIProxyAPI 仓库子目录路径
    last_verified_short_sha: str
    last_verified_full_sha: str
    last_verified_date: str
    upstream_short_sha: str
    upstream_full_sha: str
    upstream_date: str
    drift_days: int
    status: str  # "ok" | "warning" | "error"
    note: str = ""


@dataclass
class PairStatus:
    pair: str
    source: str
    target: str
    source_status: DirectionStatus
    target_status: DirectionStatus
    overall_status: str  # "ok" | "warning" | "error" = max(severity)


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


def check_one_direction(
    direction: str,
    label: str,
    cli_proxy_api_dir: str,
    last_verified_full: str,
    last_verified_short: str,
    last_verified_date: str,
    github_token: str | None,
) -> DirectionStatus:
    """检查单个方向（请求或响应）漂移。"""
    commits = fetch_upstream_commits_for_dir(github_token, cli_proxy_api_dir)
    if not commits:
        return DirectionStatus(
            direction=direction,
            label=label,
            cli_proxy_api_dir=cli_proxy_api_dir,
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

    return DirectionStatus(
        direction=direction,
        label=label,
        cli_proxy_api_dir=cli_proxy_api_dir,
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


def check_one_pair(
    pair: dict[str, Any],
    github_token: str | None,
) -> PairStatus:
    source = pair["source"]
    target = pair["target"]
    source_dir = pair["source_dir"]
    target_dir = pair["target_dir"]
    source_block = pair.get("source_state", {})
    target_block = pair.get("target_state", {})

    # 请求方向（source_dir = 客户端协议 → 下游协议 的代码）
    source_status = check_one_direction(
        direction="source",
        label="请求方向",
        cli_proxy_api_dir=source_dir,
        last_verified_full=source_block.get("last_verified_full_sha", pair["last_verified_full_sha"]),
        last_verified_short=source_block.get("last_verified_short_sha", pair["last_verified_short_sha"]),
        last_verified_date=source_block.get("last_verified_date", pair["last_verified_date"]),
        github_token=github_token,
    )
    # 响应方向（target_dir = 下游协议 → 客户端协议 的代码）
    target_status = check_one_direction(
        direction="target",
        label="响应方向",
        cli_proxy_api_dir=target_dir,
        last_verified_full=target_block.get("last_verified_full_sha", pair["last_verified_full_sha"]),
        last_verified_short=target_block.get("last_verified_short_sha", pair["last_verified_short_sha"]),
        last_verified_date=target_block.get("last_verified_date", pair["last_verified_date"]),
        github_token=github_token,
    )

    # 整体严重度 = 两个方向中较严的那个
    severity = {"ok": 0, "warning": 1, "error": 2}
    overall = max(source_status.status, target_status.status, key=lambda s: severity.get(s, 0))

    return PairStatus(
        pair=f"{SOURCE_FORMAT_LABELS.get(source, source)} → {SOURCE_FORMAT_LABELS.get(target, target)}",
        source=source,
        target=target,
        source_status=source_status,
        target_status=target_status,
        overall_status=overall,
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

    # 输出表格（每个 pair 两方向各一行）
    print(f"\n=== CLIProxyAPI 上游同步检查 ({len(statuses)} pairs) ===\n")
    for s in statuses:
        overall_icon = {"ok": "✓", "warning": "⚠", "error": "✗"}.get(s.overall_status, "?")
        print(f"{overall_icon} {s.pair} (整体: {s.overall_status})")
        for d in (s.source_status, s.target_status):
            icon = {"ok": "✓", "warning": "⚠", "error": "✗"}.get(d.status, "?")
            print(
                f"    {icon} [{d.label}] {d.cli_proxy_api_dir}\n"
                f"        AgentBuddy: {d.last_verified_short_sha} ({d.last_verified_date})\n"
                f"        Upstream  : {d.upstream_short_sha} ({d.upstream_date}, {d.drift_days}d 前)\n"
                f"        Status    : {d.status} — {d.note}"
            )
        print()

    # 输出新 sync state（供 review 后落地）
    new_state = {
        "last_checked_at": datetime.now(timezone.utc).isoformat(),
        "sla_days": SLA_DAYS,
        "upstream_repo": CLIPROXYAPI_REPO,
        "pairs": [
            {
                **pair,
                "source_state": {
                    "last_verified_short_sha": s.source_status.upstream_short_sha,
                    "last_verified_full_sha": s.source_status.upstream_full_sha,
                    "last_verified_date": s.source_status.upstream_date,
                    "drift_days": s.source_status.drift_days,
                    "status": s.source_status.status,
                },
                "target_state": {
                    "last_verified_short_sha": s.target_status.upstream_short_sha,
                    "last_verified_full_sha": s.target_status.upstream_full_sha,
                    "last_verified_date": s.target_status.upstream_date,
                    "drift_days": s.target_status.drift_days,
                    "status": s.target_status.status,
                },
                "overall_status": s.overall_status,
                # 顶层 last_verified_* 保留 = max(source, target).last_verified
                # （兼容旧代码 / 单字段快速判断）
                "last_verified_short_sha": max(
                    s.source_status.upstream_short_sha,
                    s.target_status.upstream_short_sha,
                    key=lambda x: 0 if x == "?" else 1,  # "?" 排后
                ) if s.source_status.upstream_short_sha != "?" or s.target_status.upstream_short_sha != "?"
                else "?",
            }
            for pair, s in zip(pairs, statuses)
        ],
    }
    # 简化版顶层 last_verified：取 source/target 中更新日期更晚的那个
    for pair_new, s in zip(new_state["pairs"], statuses):
        src_date = s.source_status.upstream_date
        tgt_date = s.target_status.upstream_date
        if src_date and tgt_date:
            if src_date >= tgt_date:
                pair_new["last_verified_short_sha"] = s.source_status.upstream_short_sha
                pair_new["last_verified_full_sha"] = s.source_status.upstream_full_sha
                pair_new["last_verified_date"] = s.source_status.upstream_date
            else:
                pair_new["last_verified_short_sha"] = s.target_status.upstream_short_sha
                pair_new["last_verified_full_sha"] = s.target_status.upstream_full_sha
                pair_new["last_verified_date"] = s.target_status.upstream_date
        elif src_date:
            pair_new["last_verified_short_sha"] = s.source_status.upstream_short_sha
            pair_new["last_verified_full_sha"] = s.source_status.upstream_full_sha
            pair_new["last_verified_date"] = src_date
        else:
            pair_new["last_verified_short_sha"] = s.target_status.upstream_short_sha
            pair_new["last_verified_full_sha"] = s.target_status.upstream_full_sha
            pair_new["last_verified_date"] = tgt_date

    new_path = SYNC_STATE_PATH.with_suffix(".json.new")
    new_path.write_text(json.dumps(new_state, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"\n新 sync state 已写到 {new_path}")
    print("人工 review 后 `mv` 覆盖原文件")

    error_count = sum(1 for s in statuses if s.overall_status == "error")
    if error_count > 0:
        print(f"\n[FAIL] {error_count} 个 pair 超 SLA {SLA_DAYS}d 未同步", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
