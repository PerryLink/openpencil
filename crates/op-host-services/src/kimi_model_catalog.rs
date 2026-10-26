//! Kimi Code's model catalog for the Settings card and the chat picker.
//!
//! `kimi provider list --json` prints the CLI's raw provider and model
//! config — provider API keys and OAuth storage included. Only the `models`
//! table is read, and the raw stdout never reaches a log line or an error
//! message: failures are worded from the `providerProbe.*` catalog and the
//! CLI's stderr alone.

use std::path::Path;
use std::time::Duration;

use op_ai::agent_settings_state::AgentProvider;
use op_ai::chat_models::ModelEntry;
use op_ai::chat_provider::CliName;
use op_i18n::Locale;

use crate::chat_subprocess_safety;
use crate::cli_probe_error::CliProbeError;
use crate::cli_probe_support::{bounded_cli_output, diagnose_timeout, BoundedProbe};
use crate::model_discovery::resolve_cli;

/// The listing only reads the local config; it never goes to the network.
const MODELS_PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const PROVIDER: &str = "Kimi Code";
const LOGIN_COMMAND: &str = "`kimi`";

/// Model aliases from `kimi provider list --json`, as the CLI's `-m` takes
/// them (`kimi-code/k3.1-pro`), labelled with each model's display name.
pub fn parse_kimi_models(raw: &str) -> Vec<ModelEntry> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(models) = value.get("models").and_then(|models| models.as_object()) else {
        return Vec::new();
    };
    models
        .iter()
        .filter(|(alias, _)| !alias.is_empty() && !alias.contains(char::is_whitespace))
        .map(|(alias, model)| {
            let display = model
                .get("displayName")
                .and_then(|name| name.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(alias);
            ModelEntry::new(AgentProvider::Kimi, alias.clone(), display)
        })
        .collect()
}

/// List the signed-in account's models. An empty table means nobody has
/// logged in yet, so it reads as the "run `kimi` once" error.
pub(crate) fn query_kimi_models(
    locale: Locale,
    exe: &Path,
) -> Result<Vec<ModelEntry>, CliProbeError> {
    let args = ["provider", "list", "--json"];
    let output = match bounded_cli_output(CliName::Kimi, exe, &args, MODELS_PROBE_TIMEOUT) {
        BoundedProbe::Completed(output) => output,
        // Stdout is withheld from the diagnosis: it is the raw config.
        BoundedProbe::TimedOut { stderr, .. } => {
            return Err(CliProbeError::Timeout(diagnose_timeout(
                CliName::Kimi,
                PROVIDER,
                LOGIN_COMMAND,
                MODELS_PROBE_TIMEOUT,
                &[],
                &stderr,
            )))
        }
        BoundedProbe::Failed => {
            return Err(CliProbeError::Localized(op_i18n::translate_with(
                locale,
                "providerProbe.modelQueryFailed",
                &[("name", PROVIDER)],
            )))
        }
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        let models = parse_kimi_models(&String::from_utf8_lossy(&output.stdout));
        if !models.is_empty() {
            return Ok(default_first(models, configured_default().as_deref()));
        }
    } else if let Some(message) =
        chat_subprocess_safety::friendly_stderr_error(Some(CliName::Kimi), &stderr)
    {
        return Err(CliProbeError::CliReported(message));
    } else if !stderr.trim().is_empty() {
        return Err(CliProbeError::CliReported(stderr.trim().to_string()));
    }
    Err(CliProbeError::Localized(op_i18n::translate_with(
        locale,
        "providerProbe.modelQueryFailedRunLogin",
        &[("name", PROVIDER), ("command", LOGIN_COMMAND)],
    )))
}

/// The CLI's own `default_model`, read from the top-level table of
/// `~/.kimi-code/config.toml` by that one key; nothing else in the file is
/// looked at.
fn configured_default() -> Option<String> {
    let config = std::fs::read_to_string(dirs::home_dir()?.join(".kimi-code/config.toml")).ok()?;
    default_model_from_config(&config)
}

fn default_model_from_config(config: &str) -> Option<String> {
    config
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "default_model").then(|| value.trim().trim_matches('"').to_string())
        })
        .filter(|alias| !alias.is_empty())
}

/// The picker preselects its first row, so the CLI's default leads.
fn default_first(mut models: Vec<ModelEntry>, default: Option<&str>) -> Vec<ModelEntry> {
    if let Some(index) = default.and_then(|alias| models.iter().position(|m| m.value == alias)) {
        let entry = models.remove(index);
        models.insert(0, entry);
    }
    models
}

/// Startup discovery: the account's models, or the CLI's own default when
/// the listing fails (the transport then passes no `-m` at all).
pub fn discover_kimi() -> Vec<ModelEntry> {
    let Some(exe) = resolve_cli("kimi") else {
        return Vec::new();
    };
    query_kimi_models(Locale::EnUs, &exe).unwrap_or_else(|_| kimi_default_model())
}

fn kimi_default_model() -> Vec<ModelEntry> {
    vec![ModelEntry::new(
        AgentProvider::Kimi,
        "default",
        "Kimi Code default",
    )]
}

#[cfg(test)]
#[path = "kimi_model_catalog_tests.rs"]
mod tests;
