//! Bottom-navigation normalization for mobile plans.

use crate::plan::{OrchestratorPlan, Region, Subtask};
use crate::types::DesignRequest;

use super::plan_home_intent::{plan_is_app_home_screen, screen_is_app_home};

fn prompt_requests_bottom_nav(prompt: &str) -> bool {
    let hay = prompt.to_lowercase();
    if prompt_forbids_bottom_nav(prompt) {
        return false;
    }
    hay.contains("bottom nav")
        || hay.contains("bottom navigation")
        || hay.contains("bottom tab")
        || hay.contains("bottom-tab")
        || hay.contains("tab bar")
        || hay.contains("tabbar")
        || hay.contains("底部导航")
        || hay.contains("底栏")
}

fn prompt_forbids_bottom_nav(prompt: &str) -> bool {
    let hay = prompt.to_lowercase();
    hay.contains("no bottom nav")
        || hay.contains("without bottom nav")
        || hay.contains("without bottom navigation")
        || hay.contains("不要底部导航")
        || hay.contains("不需要底部导航")
}

pub(super) fn is_bottom_nav_subtask(st: &Subtask) -> bool {
    let hay = format!(
        "{} {} {}",
        st.id.to_lowercase(),
        st.label.to_lowercase(),
        st.elements.as_deref().unwrap_or_default().to_lowercase()
    );
    // Chinese labels and the `bottom-nav` id are what a Chinese brief gets
    // planned as; missing them appended a second, English "Bottom
    // Navigation" subtask under a plan that already had `bottom-nav
    // (底部导航栏)`, and the screen drew two tab bars.
    hay.contains("bottom nav")
        || hay.contains("bottom-nav")
        || hay.contains("底部导航")
        || hay.contains("底部标签")
        || hay.contains("底栏")
        || hay.contains("bottom-navigation")
        || hay.contains("bottom navigation")
        || hay.contains("bottom tab")
        || hay.contains("bottom-tab")
        || hay.contains("tab bar")
        || hay.contains("tabbar")
        || hay.contains("bottom-tab-bar")
}

pub(super) fn ensure_requested_bottom_nav_subtask(
    plan: &mut OrchestratorPlan,
    req: &DesignRequest,
) {
    if prompt_forbids_bottom_nav(&req.prompt) {
        plan.subtasks.retain(|st| !is_bottom_nav_subtask(st));
        return;
    }
    // A multi-screen app is often named simply "App". Decide per screen,
    // even if a planner collapsed each complete screen into one subtask.
    let groups = crate::screen_groups::group_subtasks_by_screen(&plan.subtasks);
    if groups.len() > 1 {
        if req.append_context.is_some() {
            return;
        }
        for (index, group) in groups.iter().enumerate() {
            // A partial hero/section continuation is not a whole new home.
            let screen_height: f64 = group
                .indices
                .iter()
                .map(|&i| plan.subtasks[i].region.height)
                .sum();
            let complete_screen =
                group.indices.len() >= 3 || screen_height >= plan.root_frame.height * 0.75;
            if screen_is_app_home(&group.screen)
                && complete_screen
                && !group
                    .indices
                    .iter()
                    .any(|&i| is_bottom_nav_subtask(&plan.subtasks[i]))
            {
                let mut nav = bottom_nav_subtask(plan.root_frame.width);
                nav.id = format!("bottom-navigation-{index}");
                nav.screen = Some(group.screen.clone());
                plan.subtasks.push(nav);
            }
        }
        return;
    }
    if plan.subtasks.iter().any(is_bottom_nav_subtask) {
        return;
    }
    // Two ways in: the prompt asked for it, OR this is an app HOME/main
    // screen — a multi-section mobile plan whose root/screen reads as a
    // home/feed — where a bottom tab bar is anatomy, not an option. Single-
    // task flows (<3 sections) never qualify.
    if !prompt_requests_bottom_nav(&req.prompt) && !plan_is_app_home_screen(plan) {
        return;
    }

    plan.subtasks
        .push(bottom_nav_subtask(plan.root_frame.width));
}

fn bottom_nav_subtask(width: f64) -> Subtask {
    Subtask {
        id: "bottom-navigation".into(),
        label: "Bottom Navigation".into(),
        region: Region {
            width,
            height: 78.0,
        },
        bleed_hero: false,
        id_prefix: String::new(),
        parent_frame_id: None,
        insert_after_sibling_id: None,
        elements: Some(
            "bottom tab bar with this app's own 3-5 top-level destinations as icon + label tabs (choose tabs that fit the product, not a fixed Home/Search/Orders set); role bottom-tab-bar; full-width surface matching the page; transparent tab item frames; active state via accent icon/label color, not filled pills"
                .into(),
        ),
        screen: None,
        generated_root_id: None,
        existing_section_labels: None, covers: None,
        retry_feedback: None,
    }
}
