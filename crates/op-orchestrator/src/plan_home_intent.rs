//! Classify mobile app roots whose bottom tab bar is structural anatomy.

use crate::plan::OrchestratorPlan;

/// A multi-section mobile plan whose root frame is named like an app
/// home/main/feed screen — the shape whose bottom tab bar is mandatory.
pub(super) fn plan_is_app_home_screen(plan: &OrchestratorPlan) -> bool {
    if plan.subtasks.len() < 3 {
        return false;
    }

    screen_is_app_home(&plan.root_frame.name)
}

pub(super) fn screen_is_app_home(name: &str) -> bool {
    let name = normalize_screen_name(name);
    let secondary_flow = [
        "detail",
        "details",
        "form",
        "login",
        "log in",
        "sign in",
        "sign up",
        "signup",
        "register",
        "checkout",
        "onboarding",
        "confirmation",
        "success",
        "wizard",
        "buy now",
    ]
    .iter()
    .any(|phrase| screen_name_has_phrase(&name, phrase));
    if secondary_flow {
        return false;
    }
    if ["详情", "确认", "登录", "注册", "结算", "支付", "表单"]
        .iter()
        .any(|marker| name.contains(marker))
    {
        return false;
    }

    // Preserve the legacy substring behavior for established primary-screen
    // markers. Planner names commonly compact these into HomeScreen,
    // Newsfeed, Discovery, or Browser.
    [
        "home",
        "feed",
        "discover",
        "browse",
        "dashboard",
        "首页",
        "主页",
        "发现",
    ]
    .iter()
    .any(|marker| name.contains(marker))
        || ["main screen", "now screen"]
            .iter()
            .any(|phrase| screen_name_has_phrase(&name, phrase))
}

fn normalize_screen_name(name: &str) -> String {
    name.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn screen_name_has_phrase(name: &str, phrase: &str) -> bool {
    let padded_name = format!(" {name} ");
    let padded_phrase = format!(" {phrase} ");
    padded_name.contains(&padded_phrase)
}
