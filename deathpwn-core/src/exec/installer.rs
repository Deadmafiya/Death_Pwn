//! Resolve BlackArch install commands for missing tools via the AI provider.

use crate::error::{DeathpwnError, Result};
use crate::providers::{AiProvider, ChatRequest};

const INSTALL_SYSTEM: &str = "You are a package resolver for BlackArch Linux. \
Given a missing command-line tool, reply with ONLY the single shell command that \
installs it (e.g. `pacman -S --noconfirm nmap`, an AUR helper invocation, or \
`go install ...`). No prose, no explanation, no code fences.";

/// Ask the AI for the BlackArch install command for `tool` and return the
/// sanitized shell script to run. Errors if the model returns nothing usable.
pub async fn resolve_install_script(ai: &dyn AiProvider, tool: &str) -> Result<String> {
    let req = ChatRequest {
        system: INSTALL_SYSTEM.to_string(),
        user: format!(
            "Missing tool: {tool}\nReturn only the shell command to install it on BlackArch Linux."
        ),
        temperature: 0.0,
    };
    let raw = ai
        .complete(&req)
        .await
        .map_err(|e| DeathpwnError::Provider(format!("{e:?}")))?;
    let script = sanitize(&raw);
    if script.is_empty() {
        return Err(DeathpwnError::Exec(format!(
            "no install command produced for `{tool}`"
        )));
    }
    Ok(script)
}

/// Take the first meaningful line, dropping code fences and stray backticks.
fn sanitize(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("```"))
        .map(|l| l.trim_matches('`').trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DeathpwnError;
    use crate::providers::{FakeAiProvider, ProviderError};

    #[tokio::test]
    async fn resolve_strips_code_fence_and_returns_command() {
        let ai = FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(
            "```sh\npacman -S --noconfirm nmap\n```".to_string(),
        )]);
        let script = resolve_install_script(&ai, "nmap").await.unwrap();
        assert_eq!(script, "pacman -S --noconfirm nmap");
    }

    #[tokio::test]
    async fn resolve_errors_on_empty_response() {
        let ai = FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(String::new())]);
        let err = resolve_install_script(&ai, "nmap").await.unwrap_err();
        assert!(matches!(err, DeathpwnError::Exec(_)));
    }
}
