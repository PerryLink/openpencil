//! Task entry containment, overflow reachability and translated text bounds.

use super::{HomeSurface, StudioPalette, HOME_TOPBAR_H};
use crate::widgets::{test_capture_backend::CaptureBackend, PaintCx};
use crate::{Point2D, Rect, RenderBackend};
use op_editor_core::{EditorState, HomeFamily, HomeHit, Locale};

fn right(rect: Rect) -> f32 {
    rect.origin.x + rect.size.x
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn narrow_task_entries_keep_every_task_reachable_and_the_menu_on_screen() {
    for width in [640.0, 700.0, 763.0, 1024.0, 1099.0, 1100.0, 1180.0, 1440.0] {
        for height in [400.0, 801.0] {
            for selected in HomeFamily::ALL {
                let mut state = EditorState::new();
                state.editor_ui.home.visible = true;
                state.editor_ui.home.set_task(selected, 0);
                state.editor_ui.home.more_open = true;
                let home = HomeSurface::for_editor(&state).unwrap();
                let layout = home.layout(width, height);
                let visible = layout.tabs.iter().filter(|r| r.size.x > 0.0).count();
                assert_eq!(visible, if width < 1100.0 { 4 } else { 7 });
                if visible == 4 {
                    let menu = layout.more_popover;
                    assert!(menu.origin.x >= 0.0 && right(menu) <= width);
                    assert!(menu.origin.y >= HOME_TOPBAR_H);
                    assert!(menu.origin.y + menu.size.y <= height - 8.0);
                }
                let order = home.focus_order(width, height);
                for family in HomeFamily::ALL {
                    let tab = HomeHit::Tab(family);
                    let hit = if order.contains(&tab) {
                        tab
                    } else {
                        HomeHit::MoreItem(family)
                    };
                    assert!(order.contains(&hit), "{width}x{height}: {family:?}");
                    let rect = home.focus_rect(&layout, hit).unwrap();
                    assert_eq!(home.hit_test(width, height, center(rect)), Some(hit));
                }
                assert!(order.contains(&HomeHit::Tab(selected)));
            }
        }
    }
}

#[test]
fn task_names_and_summaries_stay_inside_their_cards_in_every_locale() {
    let mut measure = CaptureBackend::default();
    for locale in Locale::ALL {
        for width in [640.0, 763.0, 1100.0, 1440.0] {
            let mut state = EditorState::new();
            state.editor_ui.locale = locale;
            state.editor_ui.home.visible = true;
            let home = HomeSurface::for_editor(&state).unwrap();
            let layout = home.layout(width, 801.0);
            let mut backend = CaptureBackend::default();
            super::paint_tabs(
                &home,
                &mut PaintCx {
                    backend: &mut backend,
                },
                &layout,
                width,
                (0.0, 1.0),
                StudioPalette::for_mode(state.editor_ui.effective_theme_mode()),
            );
            let tabs: Vec<_> = layout.tabs.iter().filter(|r| r.size.x > 0.0).collect();
            for (baseline, size) in [(24.0, 14.0), (44.0, 11.0)] {
                let texts: Vec<_> = backend
                    .texts
                    .iter()
                    .filter(|(_, at)| (at.y - layout.tabs_row.origin.y - baseline).abs() < 0.01)
                    .collect();
                assert_eq!(texts.len(), tabs.len());
                for ((text, at), rect) in texts.iter().zip(&tabs) {
                    let text_w = measure.measure_text_family(text, size, super::copy::SANS);
                    assert!(at.x >= rect.origin.x + 10.0, "{locale:?} {width}: {text}");
                    assert!(
                        at.x + text_w <= right(**rect) - 9.9,
                        "{locale:?} {width}: {text}"
                    );
                }
            }
            let mut icons: Vec<_> = backend
                .svg_strokes
                .iter()
                .filter(|(_, at, size, _, _)| {
                    *size == 22.0 && (at.y - layout.tabs_row.origin.y - 19.0).abs() < 0.01
                })
                .map(|(_, at, size, _, _)| (*at, *size))
                .collect();
            icons.dedup();
            assert_eq!(icons.len(), tabs.len());
            for ((at, size), rect) in icons.iter().zip(tabs) {
                assert!(at.x >= rect.origin.x + 9.9);
                assert!(at.x + size <= right(*rect) - 10.0);
            }
        }
    }
}

#[test]
fn more_menu_translations_leave_room_for_the_icons_and_arrows() {
    let mut measure = CaptureBackend::default();
    for locale in Locale::ALL {
        let mut state = EditorState::new();
        state.editor_ui.locale = locale;
        state.editor_ui.home.visible = true;
        state.editor_ui.home.more_open = true;
        let home = HomeSurface::for_editor(&state).unwrap();
        let layout = home.layout(763.0, 801.0);
        let mut backend = CaptureBackend::default();
        super::paint_more_popover(
            &home,
            &mut PaintCx {
                backend: &mut backend,
            },
            &layout,
            StudioPalette::for_mode(state.editor_ui.effective_theme_mode()),
        );
        for row in layout.more_rows.iter().filter(|r| r.size.x > 0.0) {
            for (baseline, size) in [(18.0, 13.0), (34.0, 11.0)] {
                let (text, at) = backend
                    .texts
                    .iter()
                    .rev()
                    .find(|(_, at)| {
                        (at.x - row.origin.x - 34.0).abs() < 0.01
                            && (at.y - row.origin.y - baseline).abs() < 0.01
                    })
                    .expect("menu line");
                assert!(
                    at.x + measure.measure_text_family(text, size, super::copy::SANS)
                        <= right(*row) - 26.0
                );
            }
        }
    }
}
