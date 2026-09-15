//! Entrance-choreography unit tests for the 制图台 Home surface: the
//! pure `home_enter` phase function (timings copied from the prototype's
//! keyframes) and the state-level deadline contract.

use super::{home_enter, HomeEnterBlock};
use op_editor_core::{HomeState, HOME_ENTER_WINDOW_MS};

const SHOWN: u64 = 10_000;

/// start / mid / end for one block: below the start the block sits at
/// `(rise, 0)`, inside the window it is partial, past the end settled.
fn assert_block_curve(block: HomeEnterBlock, index: usize, start: u64, duration: u64, rise: f32) {
    assert_eq!(
        home_enter(block, index, SHOWN, SHOWN + start),
        (rise, 0.0),
        "{block:?} not at rest before its window"
    );
    let (mid_dy, mid_alpha) = home_enter(block, index, SHOWN, SHOWN + start + duration / 2);
    assert!(
        mid_dy > 0.0 && mid_dy < rise,
        "{block:?} mid rise {mid_dy} not inside 0..{rise}"
    );
    assert!(
        (0.0..1.0).contains(&mid_alpha),
        "{block:?} mid alpha {mid_alpha} not inside 0..1"
    );
    assert_eq!(
        home_enter(block, index, SHOWN, SHOWN + start + duration),
        (0.0, 1.0),
        "{block:?} must settle exactly at its window end"
    );
}

#[test]
fn headline_subtitle_sheet_and_footer_keep_their_windows() {
    // (block, start, duration, rise) straight from the prototype's
    // keyframes: headline 0/420/16, subtitle 80/360/12, sheet
    // 140/360/12, footer 600/300 fade-only.
    assert_block_curve(HomeEnterBlock::Headline, 0, 0, 420, 16.0);
    assert_block_curve(HomeEnterBlock::Subtitle, 0, 80, 360, 12.0);
    assert_block_curve(HomeEnterBlock::Sheet, 0, 140, 360, 12.0);
    // Footer fades only: its rise is 0 at every phase.
    let (mid_dy, mid_alpha) = home_enter(HomeEnterBlock::Footer, 0, SHOWN, SHOWN + 750);
    assert_eq!(mid_dy, 0.0);
    assert!((0.0..1.0).contains(&mid_alpha));
    assert_eq!(
        home_enter(HomeEnterBlock::Footer, 0, SHOWN, SHOWN + 600),
        (0.0, 0.0)
    );
    assert_eq!(
        home_enter(HomeEnterBlock::Footer, 0, SHOWN, SHOWN + 900),
        (0.0, 1.0)
    );
}

#[test]
fn the_underline_draws_left_to_right_after_the_headline_settles() {
    // 520 ms in, nothing has drawn yet (the headline settled at 420).
    assert_eq!(
        home_enter(HomeEnterBlock::Underline, 0, SHOWN, SHOWN + 520),
        (0.0, 0.0)
    );
    let (_, mid) = home_enter(HomeEnterBlock::Underline, 0, SHOWN, SHOWN + 520 + 450);
    assert!((0.0..1.0).contains(&mid), "halfway fraction {mid}");
    // The 900 ms draw is the choreography's longest block.
    assert_eq!(
        home_enter(HomeEnterBlock::Underline, 0, SHOWN, SHOWN + 520 + 900),
        (0.0, 1.0)
    );
}

#[test]
fn chips_stagger_40ms_apart_over_300ms() {
    assert_block_curve(HomeEnterBlock::Chip, 0, 260, 300, 8.0);
    // Chip 3 waits out 3 × 40 ms of stagger.
    assert_eq!(
        home_enter(HomeEnterBlock::Chip, 3, SHOWN, SHOWN + 260 + 120 - 1),
        (8.0, 0.0)
    );
    assert!(home_enter(HomeEnterBlock::Chip, 3, SHOWN, SHOWN + 260 + 120 + 1).1 > 0.0);
    assert_eq!(
        home_enter(HomeEnterBlock::Chip, 3, SHOWN, SHOWN + 260 + 120 + 300),
        (0.0, 1.0)
    );
}

#[test]
fn cards_stagger_70ms_apart_so_card_two_waits_for_card_one() {
    assert_block_curve(HomeEnterBlock::Card, 0, 380, 360, 18.0);
    // Card 1 starts exactly 70 ms after card 0, so when card 0 settles
    // card 1 is still mid-flight, and it settles 70 ms later still.
    let card_one_done = SHOWN + 380 + 360;
    assert_eq!(
        home_enter(HomeEnterBlock::Card, 0, SHOWN, card_one_done),
        (0.0, 1.0)
    );
    let (lagging_dy, lagging_alpha) = home_enter(HomeEnterBlock::Card, 1, SHOWN, card_one_done);
    assert!(lagging_dy > 0.0 && lagging_alpha < 1.0);
    assert_eq!(
        home_enter(HomeEnterBlock::Card, 1, SHOWN, SHOWN + 380 + 70 - 1),
        (18.0, 0.0),
        "card 1 must not move before its own staggered start"
    );
    assert!(home_enter(HomeEnterBlock::Card, 1, SHOWN, SHOWN + 380 + 70 + 1).0 < 18.0);
    assert_eq!(
        home_enter(HomeEnterBlock::Card, 1, SHOWN, card_one_done + 70),
        (0.0, 1.0)
    );
    assert_eq!(
        home_enter(HomeEnterBlock::Card, 3, SHOWN, SHOWN + 380 + 210 + 360),
        (0.0, 1.0)
    );
}

#[test]
fn an_unstamped_surface_paints_settled() {
    // shown_at_ms == 0 means "not started" (no host paint has stamped
    // the clock yet) — every block paints at t = 1, never frozen.
    for block in [
        HomeEnterBlock::Headline,
        HomeEnterBlock::Subtitle,
        HomeEnterBlock::Underline,
        HomeEnterBlock::Sheet,
        HomeEnterBlock::Chip,
        HomeEnterBlock::Card,
        HomeEnterBlock::Footer,
    ] {
        for index in 0..4 {
            assert_eq!(
                home_enter(block, index, 0, 1_234),
                (0.0, 1.0),
                "{block:?}[{index}] must be static without a stamp"
            );
        }
    }
}

#[test]
fn the_deadline_frames_while_the_entrance_runs_and_stops_after() {
    let home = HomeState {
        visible: true,
        shown_at_ms: SHOWN,
        ..HomeState::default()
    };
    assert_eq!(home.entrance_deadline_ms(SHOWN), Some(SHOWN + 16));
    // The window covers the underline's 1 420 ms draw — the longest
    // block in the choreography.
    assert!(home.entrance_deadline_ms(SHOWN + 1_420).is_some());
    assert_eq!(
        home.entrance_deadline_ms(SHOWN + HOME_ENTER_WINDOW_MS - 1),
        Some(SHOWN + HOME_ENTER_WINDOW_MS - 1 + 16)
    );
    assert_eq!(
        home.entrance_deadline_ms(SHOWN + HOME_ENTER_WINDOW_MS),
        None,
        "past the whole window nothing animates"
    );
}
