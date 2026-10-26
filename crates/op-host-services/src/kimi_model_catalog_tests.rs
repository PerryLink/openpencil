use super::*;

/// Shape of `kimi provider list --json` (2.3.1), with a placeholder key.
const LISTING: &str = r#"{
  "providers": {
    "managed:kimi-code": {"type": "kimi", "baseUrl": "https://api.example", "apiKey": "sk-placeholder"}
  },
  "models": {
    "kimi-code/k3.1-pro": {"provider": "managed:kimi-code", "model": "k3.1-pro", "displayName": "K3.1 Pro"},
    "kimi-code/k3": {"provider": "managed:kimi-code", "model": "k3", "displayName": " "},
    "bad alias": {"displayName": "Broken"}
  }
}"#;

#[test]
fn the_listing_yields_aliases_with_display_names_and_nothing_else() {
    let models = parse_kimi_models(LISTING);
    let rows: Vec<_> = models
        .iter()
        .map(|m| (m.value.as_str(), m.display_name.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("kimi-code/k3.1-pro", "K3.1 Pro"),
            ("kimi-code/k3", "kimi-code/k3")
        ]
    );
    assert!(models.iter().all(|m| m.provider == AgentProvider::Kimi));
    assert!(models
        .iter()
        .all(|m| !m.value.contains("sk-") && !m.display_name.contains("sk-")));
}

#[test]
fn a_signed_out_or_garbled_listing_has_no_models() {
    assert!(parse_kimi_models(r#"{"providers":{},"models":{}}"#).is_empty());
    assert!(parse_kimi_models("not json").is_empty());
    assert!(parse_kimi_models(r#"{"models":[]}"#).is_empty());
}

#[test]
fn the_cli_default_model_leads_the_picker() {
    let config = "default_model = \"kimi-code/k3\"\n[providers.x]\ndefault_model = \"other\"\n";
    assert_eq!(
        default_model_from_config(config).as_deref(),
        Some("kimi-code/k3")
    );
    assert_eq!(
        default_model_from_config("[s]\ndefault_model = \"x\"\n"),
        None
    );
    let ordered = default_first(parse_kimi_models(LISTING), Some("kimi-code/k3"));
    assert_eq!(ordered[0].value, "kimi-code/k3");
    let unchanged = default_first(parse_kimi_models(LISTING), Some("missing"));
    assert_eq!(unchanged[0].value, "kimi-code/k3.1-pro");
}

#[cfg(unix)]
#[test]
fn a_failed_listing_never_echoes_the_raw_config() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("openpencil-kimi-listing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("kimi");
    std::fs::write(
        &exe,
        "#!/bin/sh\necho '{\"providers\":{\"p\":{\"apiKey\":\"sk-secret\"}},\"models\":{}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = query_kimi_models(Locale::EnUs, &exe)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("sk-secret"), "{error}");
    assert!(error.contains("kimi"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}
