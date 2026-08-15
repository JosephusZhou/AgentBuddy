//! Claude Code system prompt constants.
//! Reference: CLIProxyAPI internal/runtime/executor/helps/claude_system_prompt.go
//! These correspond to the Claude Code v2.1.220 client identity baseline.

/// Agent identifier — the first line Claude Code sends.
pub const CLAUDE_CODE_AGENT_IDENTIFIER: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

/// Intro segment.
pub const CLAUDE_CODE_INTRO: &str = "You are an interactive CLI tool that helps users with software engineering tasks. Use the instructions below and the tools available to you to assist the user.\n\nIMPORTANT: you should not need to ask the user for additional information. If you cannot complete a task without more information, you should tell the user what you need.";

/// System segment.
pub const CLAUDE_CODE_SYSTEM: &str = "The user wants you to help them with software engineering tasks. The user is a software engineer working in a terminal-based development environment. Always follow the user's instructions carefully and precisely.";

/// Doing tasks segment.
pub const CLAUDE_CODE_DOING_TASKS: &str = "When working on tasks, you should:\n1. Understand the full context of the request\n2. Plan your approach before starting\n3. Execute methodically, testing as you go\n4. Verify your work before reporting completion";

/// Tone and style segment.
pub const CLAUDE_CODE_TONE_AND_STYLE: &str = "You should be concise, direct, and technical in your communication. Do not use excessive formatting, emojis, or pleasantries. Focus on the task at hand.";

/// Build the system array that mirrors real Claude Code's structure.
/// Returns a Vec of system prompt entries for the Anthropic API's `system` field.
pub fn build_system_array(original_system: Option<&str>) -> Vec<serde_json::Value> {
    let mut system = Vec::new();

    // system[0]: billing header placeholder (no cache_control)
    // The actual billing header goes in the HTTP header, not the body.
    // But the system[0] text should be present for structure matching.
    system.push(serde_json::json!({
        "type": "text",
        "text": ""
    }));

    // system[1]: agent identifier
    system.push(serde_json::json!({
        "type": "text",
        "text": CLAUDE_CODE_AGENT_IDENTIFIER
    }));

    // system[2]: core system prompt (concatenation of all segments)
    let core = format!(
        "{}\n\n{}\n\n{}\n\n{}",
        CLAUDE_CODE_INTRO,
        CLAUDE_CODE_SYSTEM,
        CLAUDE_CODE_DOING_TASKS,
        CLAUDE_CODE_TONE_AND_STYLE
    );
    let mut core_entry = serde_json::json!({
        "type": "text",
        "text": core
    });
    // Add cache_control to the last system block
    core_entry["cache_control"] = serde_json::json!({"type": "ephemeral"});
    system.push(core_entry);

    // If there's an original system prompt, inject it as the first user message
    // (handled by caller, not added to system array here)
    let _ = original_system;

    system
}
