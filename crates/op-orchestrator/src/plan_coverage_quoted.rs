//! Sections the brief names by a quoted title: 「近 7 日票房」折线图.
//!
//! The quotes mark the block's name; what follows only says what kind of
//! block it is. A planner that wrote "近7日票房标题……奶油黄折线" covered the
//! panel, yet the whole-string match wanted 「近 7 日票房」折线图 verbatim,
//! called it missing and appended a second copy: both astra-new2 cinema
//! console drafts grew a duplicate chart or table that pushed the 900px
//! dashboard's lower panels off the screen.

use crate::plan::{OrchestratorPlan, Subtask};
use crate::plan_coverage::subtask_haystack;

/// Shorter titles (「热门」, 「新品」) are ordinary words a plan uses anywhere.
const MIN_TITLE_CHARS: usize = 3;

fn squash(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The title inside the section's 「」 or 『』, squashed, when long enough.
fn quoted_title(section: &str) -> Option<String> {
    let (open, close) = [('「', '」'), ('『', '』')]
        .into_iter()
        .find(|(open, _)| section.contains(*open))?;
    let start = section.find(open)? + open.len_utf8();
    let end = start + section[start..].find(close)?;
    let title = squash(&section[start..end]);
    (title.chars().count() >= MIN_TITLE_CHARS).then_some(title)
}

/// The first subtask whose label or elements carry the section's quoted
/// title, ignoring spacing and punctuation.
pub(crate) fn quoted_title_subtask(section: &str, plan: &OrchestratorPlan) -> Option<String> {
    let title = quoted_title(section)?;
    plan.subtasks
        .iter()
        .find(|st: &&Subtask| squash(&subtask_haystack(st)).contains(&title))
        .map(|st| st.id.clone())
}
