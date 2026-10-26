//! Quota evidence for options the plan names by price.
//!
//! The florist plan promised "三张等宽横排小卡，分别单周 ¥69、双周 ¥129、包月
//! ¥239" and both astra reruns delivered all three, but the current plan
//! carried a 「当前方案」 marker, so its structure differed from its siblings
//! and the family count said 2 of 3: two retries, then the section was marked
//! incomplete. Structure stays strict for every other count; here each
//! promised price found in its own sibling card is the delivery itself.

use crate::types::DocSink;
use jian_ops_schema::node::{PenNode, TextContent};
use op_design_lint::node_util::is_node_visible;
use op_editor_core::{NodeId, PenNodeExt};
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

static PRICE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[¥￥]\s*(?P<amount>[0-9][0-9,]*(?:\.[0-9]+)?)").expect("valid price pattern")
});
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9][0-9,]*(?:\.[0-9]+)?").expect("valid number pattern"));

fn amount(raw: &str) -> String {
    raw.replace(',', "")
}

/// The distinct prices the plan lists, when it lists at least two.
fn promised_prices(text: &str) -> BTreeSet<String> {
    let prices: BTreeSet<String> = PRICE
        .captures_iter(text)
        .map(|captures| amount(&captures["amount"]))
        .collect();
    if prices.len() >= 2 {
        prices
    } else {
        BTreeSet::new()
    }
}

fn collect_numbers(node: &PenNode, numbers: &mut BTreeSet<String>) {
    if !is_node_visible(node) {
        return;
    }
    if let PenNode::Text(text) = node {
        let content = match &text.content {
            TextContent::Plain(value) => value.clone(),
            TextContent::Styled(runs) => runs.iter().map(|run| run.text.as_str()).collect(),
        };
        numbers.extend(NUMBER.find_iter(&content).map(|m| amount(m.as_str())));
    }
    for child in node.children().into_iter().flatten() {
        collect_numbers(child, numbers);
    }
}

/// The most promised prices one container shows, each in its own child and
/// each child showing exactly one of them.
fn priced_siblings(node: &PenNode, prices: &BTreeSet<String>) -> usize {
    if !is_node_visible(node) {
        return 0;
    }
    let children = node.children().map(Vec::as_slice).unwrap_or(&[]);
    let mut found = BTreeSet::new();
    for child in children
        .iter()
        .filter(|child| matches!(child, PenNode::Frame(_)))
    {
        let mut numbers = BTreeSet::new();
        collect_numbers(child, &mut numbers);
        let owned: Vec<_> = numbers.intersection(prices).collect();
        if let [price] = owned.as_slice() {
            found.insert((*price).clone());
        }
    }
    children
        .iter()
        .map(|child| priced_siblings(child, prices))
        .fold(found.len(), usize::max)
}

pub(super) fn delivered_count(sink: &dyn DocSink, root_ids: &[String], text: &str) -> usize {
    let prices = promised_prices(text);
    if prices.is_empty() {
        return 0;
    }
    root_ids
        .iter()
        .filter_map(|id| {
            op_editor_core::walkers::find_node(sink.state().active_children(), &NodeId::new(id))
        })
        .map(|root| priced_siblings(root, &prices))
        .max()
        .unwrap_or(0)
}
