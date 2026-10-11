use super::*;
use op_editor_core::{BrandKitPayload, EditorState, HomeBrandStatus, HomeFamily};

fn state(compact: bool, count: usize, brand: bool) -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.home.visible = true;
    state.editor_ui.home.set_task(HomeFamily::AppUi, 0);
    if compact {
        state.editor_ui.touch = true;
        state.editor_ui.size_class = op_editor_core::size_class::EditorSizeClass::Compact;
    }
    for index in 0..count {
        state
            .chat
            .add_attachment(op_editor_core::chat::ChatAttachment {
                name: format!("参考设计页面截图版本{index}.png"),
                media_type: "image/png".into(),
                data: vec![1],
            });
    }
    if brand {
        state.editor_ui.home.brand.status = HomeBrandStatus::Ready(Box::new(BrandKitPayload {
            label: "Leafline".into(),
            swatches: vec![
                "#0D775A".into(),
                "#FFFFFF".into(),
                "#111111".into(),
                "#F97316".into(),
            ],
            variables: Default::default(),
            themes: Default::default(),
            design_md: None,
        }));
    }
    state
}

fn measure(label: &str) -> f32 {
    label
        .chars()
        .map(|c| if c.is_ascii() { 6.0 } else { 11.0 })
        .sum()
}

fn right(rect: Rect) -> f32 {
    rect.origin.x + rect.size.x
}
fn bottom(rect: Rect) -> f32 {
    rect.origin.y + rect.size.y
}

#[test]
fn references_stay_in_one_bounded_row_and_clear_text_and_brand() {
    for (compact, width) in [
        (false, 1180.0),
        (false, 700.0),
        (true, 390.0),
        (true, 320.0),
    ] {
        for count in [1, 2, 3, 5] {
            for brand in [false, true] {
                let state = state(compact, count, brand);
                let home = HomeSurface::for_editor(&state).unwrap();
                let input = home.layout(width, 844.0).input_box;
                let text = input_text_rect(&home, input);
                if compact {
                    assert!(home.input_text_area("", input).max_visible_lines >= 2);
                }
                let bounds = attachment_bounds(&home, input);
                let chips = attachment_chips(&home.attachment_names, bounds, measure);
                assert!(!chips.is_empty());
                assert!(bottom(text) + 5.0 <= bounds.origin.y);
                let mut previous_right = bounds.origin.x - CHIP_GAP;
                for chip in &chips {
                    assert!(chip.rect.origin.x >= previous_right + CHIP_GAP - 0.01);
                    assert!(right(chip.rect) <= right(bounds) + 0.01);
                    assert_eq!(chip.rect.origin.y, bounds.origin.y);
                    assert!(bottom(chip.rect) <= bottom(input));
                    previous_right = right(chip.rect);
                }
                if let Some((brand, _)) = brand_chip::chip_rects(&home, input) {
                    assert!(right(bounds) + CHIP_GAP <= brand.origin.x + 0.01);
                    assert!(bottom(text) + 5.0 <= brand.origin.y);
                }
            }
        }
    }
}

#[test]
fn overflow_keeps_the_total_reference_count_visible() {
    let names = (0..5)
        .map(|n| format!("参考设计截图版本{n}.png"))
        .collect::<Vec<_>>();
    let chips = attachment_chips(&names, Rect::xywh(0.0, 0.0, 240.0, ROW_H), measure);
    let displayed = chips.iter().filter(|chip| chip.image).count();
    let remaining = chips
        .iter()
        .find(|chip| !chip.image)
        .expect("overflow count");
    assert_eq!(remaining.label, format!("+{}", names.len() - displayed));
    assert!(chips
        .iter()
        .filter(|chip| chip.image)
        .all(|chip| !chip.label.ends_with("png")));
}

#[test]
fn a_narrow_shared_row_shows_the_total_instead_of_an_unreadable_filename() {
    let names = (0..3)
        .map(|n| format!("参考设计截图版本{n}.png"))
        .collect::<Vec<_>>();
    let bounds = Rect::xywh(10.0, 20.0, 88.0, ROW_H);
    let chips = attachment_chips(&names, bounds, measure);
    assert_eq!(chips.len(), 1);
    assert!(chips[0].image);
    assert_eq!(chips[0].label, "3");
    assert_eq!(chips[0].rect, bounds);
}

#[test]
fn empty_reference_row_preserves_the_input_geometry() {
    for compact in [false, true] {
        let state = state(compact, 0, false);
        let home = HomeSurface::for_editor(&state).unwrap();
        let input = home
            .layout(if compact { 390.0 } else { 1180.0 }, 844.0)
            .input_box;
        let text = input_text_rect(&home, input);
        let pad = if compact { 7.0 } else { 9.0 };
        assert_eq!(text.origin.y, input.origin.y + pad);
        assert_eq!(text.size.y, input.size.y - pad * 2.0);
    }
}

#[test]
fn the_phone_paints_the_same_brand_chip_that_its_close_target_hits() {
    use crate::widgets::{test_capture_backend::CaptureBackend, PaintCx, Widget};
    let state = state(true, 1, true);
    let home = HomeSurface::for_editor(&state).unwrap();
    let (brand, close) = home.brand_chip_rects(390.0, 844.0).expect("brand");
    let mut backend = CaptureBackend::default();
    home.paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        Rect::xywh(0.0, 0.0, 390.0, 844.0),
    );
    assert!(
        backend
            .round_fills
            .iter()
            .any(|(rect, radius, _)| *rect == brand && *radius == 7.0),
        "brand must be visible on phones"
    );
    assert!(
        backend
            .texts
            .iter()
            .any(|(label, at)| !label.is_empty() && brand.contains(*at)),
        "brand label must be visible"
    );
    assert_eq!(
        home.hit_test(
            390.0,
            844.0,
            Point2D::new(close.origin.x + 8.0, close.origin.y + 8.0)
        ),
        Some(op_editor_core::HomeHit::BrandClear)
    );
}

#[test]
fn reference_manager_reaches_every_image_and_traps_hits_and_focus() {
    use crate::widgets::{test_capture_backend::CaptureBackend, PaintCx, Widget};
    use op_editor_core::HomeHit;
    for (compact, width, height) in [
        (false, 1180.0, 900.0),
        (false, 700.0, 600.0),
        (true, 390.0, 844.0),
        (true, 320.0, 320.0),
    ] {
        let mut state = state(compact, 4, true);
        let center =
            |r: Rect| Point2D::new(r.origin.x + r.size.x / 2.0, r.origin.y + r.size.y / 2.0);
        let home = HomeSurface::for_editor(&state).unwrap();
        let page = home.layout(width, height);
        let manage = manage_rect(&home, page.input_box).unwrap();
        let (brand, _) = brand_chip::chip_rects(&home, page.input_box).unwrap();
        assert!(right(manage) + CHIP_GAP <= brand.origin.x + 0.01);
        assert_eq!(
            home.hit_test(width, height, center(manage)),
            Some(HomeHit::References)
        );
        assert!(home
            .focus_order(width, height)
            .contains(&HomeHit::References));
        state.apply_home_reference_action(HomeHit::References);
        state.editor_ui.home.scroll_y = 100.0;
        let home = HomeSurface::for_editor(&state).unwrap();
        let page = home.layout(width, height);
        let order = home.focus_order(width, height);
        assert_eq!(
            order,
            vec![
                HomeHit::ReferencesClose,
                HomeHit::ReferenceRemove(0),
                HomeHit::ReferenceRemove(1),
                HomeHit::ReferenceRemove(2),
                HomeHit::ReferenceRemove(3)
            ]
        );
        for hit in &order {
            let rect = home.focus_rect(&page, *hit).unwrap();
            assert!(rect.origin.x >= 0.0 && rect.origin.y >= 0.0);
            assert!(right(rect) <= width && bottom(rect) <= height);
            assert!(rect.size.x >= 44.0 && rect.size.y >= 44.0);
            assert_eq!(home.hit_test(width, height, center(rect)), Some(*hit));
            assert_eq!(home.focus_scroll_for(width, height, *hit, 56.0), None);
        }
        assert_eq!(
            home.hit_test(width, height, Point2D::new(2.0, 2.0)),
            Some(HomeHit::ReferencesClose)
        );
        let mut backend = CaptureBackend::default();
        home.paint(
            &mut PaintCx {
                backend: &mut backend,
            },
            Rect::xywh(0.0, 0.0, width, height),
        );
        let last = home.focus_rect(&page, HomeHit::ReferenceRemove(3)).unwrap();
        assert!(backend.texts.iter().any(|(text, at)| text
            == op_i18n::translate(state.editor_ui.locale, "home.references.remove")
            && last.contains(*at)));
    }
}
