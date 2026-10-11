//! Optional Studio Home prompt construction for task-faithful benchmarks.

use op_editor_core::{AppPages, EditorState, HomeDevice, HomeFamily};
use std::fmt;

#[derive(Debug)]
pub(crate) enum HomePromptError {
    UnknownFamily(String),
    InvalidOption(String),
    EmptyBrief,
}

impl fmt::Display for HomePromptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFamily(value) => write!(f, "unknown smoke Home family {value:?}"),
            Self::InvalidOption(message) => f.write_str(message),
            Self::EmptyBrief => f.write_str("Home brief is empty"),
        }
    }
}

pub(crate) fn from_env(prompt: String) -> Result<String, HomePromptError> {
    prepare(
        prompt,
        std::env::var("OPENPENCIL_SMOKE_HOME_FAMILY")
            .ok()
            .as_deref(),
        std::env::var("OPENPENCIL_SMOKE_HOME_DEVICE")
            .ok()
            .as_deref(),
        std::env::var("OPENPENCIL_SMOKE_HOME_APP_PAGES")
            .ok()
            .as_deref(),
    )
}

fn prepare(
    prompt: String,
    family: Option<&str>,
    device: Option<&str>,
    pages: Option<&str>,
) -> Result<String, HomePromptError> {
    if (device.is_some() || pages.is_some()) && family != Some("app") {
        return Err(HomePromptError::InvalidOption(
            "smoke Home device/page options require HOME_FAMILY=app".into(),
        ));
    }
    let Some(family) = family else {
        return Ok(prompt);
    };
    let family =
        parse_family(family).ok_or_else(|| HomePromptError::UnknownFamily(family.into()))?;
    let mut state = EditorState::new();
    state.editor_ui.home.task = family;
    state.editor_ui.home.set_draft(prompt);
    if let Some(value) = device {
        let device = match value {
            "mobile" => HomeDevice::Mobile,
            "desktop" => HomeDevice::Desktop,
            _ => {
                return Err(HomePromptError::InvalidOption(format!(
                    "unknown smoke Home device {value:?}"
                )))
            }
        };
        state.editor_ui.home.set_device(device);
    }
    if let Some(value) = pages {
        let pages = AppPages::from_id(value).ok_or_else(|| {
            HomePromptError::InvalidOption(format!("unknown smoke Home app pages {value:?}"))
        })?;
        state.editor_ui.home.set_app_pages(pages);
    }
    state
        .editor_ui
        .home
        .generation_prompt()
        .ok_or(HomePromptError::EmptyBrief)
}

fn parse_family(id: &str) -> Option<HomeFamily> {
    HomeFamily::from_id(id).or_else(|| (id == "cards").then_some(HomeFamily::KnowledgeCards))
}

pub(crate) fn record_purpose(state: &mut EditorState) {
    state.editor_ui.home.work_family = std::env::var("OPENPENCIL_SMOKE_HOME_FAMILY")
        .ok()
        .as_deref()
        .and_then(parse_family);
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_orchestrator::{detect_design_type, DesignType};

    #[test]
    fn home_card_contract_wins_over_app_words_in_the_article() {
        let prompt = prepare(
            "把文章转成4张卡：App界面从一页开始，需要再加多页。".into(),
            Some("cards"),
            None,
            None,
        )
        .unwrap();
        assert_eq!(detect_design_type(&prompt).type_, DesignType::Card);
        assert!(prompt.contains("App界面从一页开始"));
    }

    #[test]
    fn unknown_family_is_not_silently_treated_as_a_raw_brief() {
        assert!(prepare("brief".into(), Some("not-a-family"), None, None).is_err());
        assert_eq!(
            prepare("raw brief".into(), None, None, None).unwrap(),
            "raw brief"
        );
    }

    #[test]
    fn desktop_console_uses_the_home_app_contract() {
        let prompt = prepare(
            "Admin console (1440×900)".into(),
            Some("app"),
            Some("desktop"),
            Some("single"),
        )
        .unwrap();
        assert!(prompt.contains("desktop app"));
        assert!(prompt.contains("交付 1 页界面"));
        assert!(!prompt.contains("landing page"));
        assert!(!prompt.contains("mobile app"));
        assert_eq!(detect_design_type(&prompt).type_, DesignType::DesktopScreen);
    }

    #[test]
    fn explicit_multiple_pages_and_default_single_page_match_home() {
        let single = prepare("coffee".into(), Some("app"), None, None).unwrap();
        assert!(single.contains("mobile app"));
        assert!(single.contains("交付 1 页界面"));
        let multiple = prepare(
            "4 pages".into(),
            Some("app"),
            Some("mobile"),
            Some("multiple"),
        )
        .unwrap();
        assert!(multiple.contains("交付 4 页完整界面"));
    }

    #[test]
    fn invalid_or_misplaced_options_fail_before_generation() {
        for (family, device, pages) in [
            (None, Some("desktop"), None),
            (Some("web"), None, Some("single")),
            (Some("app"), Some("deskop"), None),
            (Some("app"), None, Some("many")),
        ] {
            assert!(prepare("brief".into(), family, device, pages).is_err());
        }
    }
}
