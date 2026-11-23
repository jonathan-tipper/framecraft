use framecraft::editor::{self, Annotation, Document, Point, Rect, Style, Theme};
use image::{Rgba, RgbaImage};

fn rect(x: u32, y: u32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}
fn doc() -> Document {
    Document {
        source: RgbaImage::from_fn(8, 6, |x, y| Rgba([(x * 20) as u8, (y * 30) as u8, 0, 255])),
        crop: None,
        annotations: vec![],
        style: Style {
            padding: 0,
            radius: 0,
            shadow: false,
            theme: Theme::Paper,
        },
    }
}
fn pixelate(r: Rect, block_size: u32) -> Annotation {
    Annotation::Pixelate {
        rect: r,
        block_size,
    }
}

#[test]
fn pixelate_grid_is_region_anchored_and_clipped() {
    let mut d = doc();
    d.annotations.push(pixelate(rect(1, 1, 5, 3), 2));
    let out = editor::render(&d).unwrap();
    assert_eq!(*out.get_pixel(1, 1), Rgba([30, 45, 0, 255]));
    assert_eq!(out.get_pixel(2, 2), out.get_pixel(1, 1));
    assert_eq!(*out.get_pixel(5, 3), Rgba([100, 90, 0, 255]));
    assert_eq!(out.get_pixel(0, 1), d.source.get_pixel(0, 1));
    assert_eq!(out.get_pixel(6, 3), d.source.get_pixel(6, 3));
}
#[test]
fn pixelate_uses_alpha_weighted_colour_and_canonical_transparency() {
    let mut d = doc();
    d.source = RgbaImage::from_pixel(2, 2, Rgba([0, 255, 255, 0]));
    d.source.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    d.annotations.push(pixelate(rect(0, 0, 2, 2), 2));
    assert_eq!(
        *editor::render(&d).unwrap().get_pixel(1, 1),
        Rgba([255, 0, 0, 64])
    );
    d.source.put_pixel(0, 0, Rgba([255, 0, 0, 0]));
    assert_eq!(
        *editor::render(&d).unwrap().get_pixel(1, 1),
        Rgba([0, 0, 0, 0])
    );
}
#[test]
fn hidden_redaction_and_crop_pixels_cannot_influence_visible_output() {
    let mut a = doc();
    a.crop = Some(rect(1, 1, 6, 4));
    a.annotations = vec![
        pixelate(rect(0, 0, 8, 6), 4),
        Annotation::Redact {
            rect: rect(3, 2, 2, 2),
        },
    ];
    let mut b = a.clone();
    for (x, y, p) in b.source.enumerate_pixels_mut() {
        if !(1..7).contains(&x)
            || !(1..5).contains(&y)
            || ((3..5).contains(&x) && (2..4).contains(&y))
        {
            *p = Rgba([255, 255, 255, 71]);
        }
    }
    assert_eq!(editor::render(&a).unwrap(), editor::render(&b).unwrap());
    a.style.padding = 8;
    a.style.radius = 3;
    a.style.shadow = true;
    b.style = a.style.clone();
    assert_eq!(editor::render(&a).unwrap(), editor::render(&b).unwrap());
}
#[test]
fn overlap_samples_original_and_explanatory_annotations_stay_above_effects() {
    let mut d = doc();
    let later = pixelate(rect(1, 1, 4, 4), 3);
    d.annotations = vec![pixelate(rect(0, 0, 8, 6), 4), later.clone()];
    let overlap = editor::render(&d).unwrap();
    d.annotations = vec![later.clone()];
    let original = editor::render(&d).unwrap();
    assert_eq!(overlap.get_pixel(2, 2), original.get_pixel(2, 2));
    d.annotations = vec![
        Annotation::Highlight {
            rect: rect(1, 1, 4, 4),
            colour: [255, 0, 0, 255],
        },
        later,
    ];
    assert_eq!(
        *editor::render(&d).unwrap().get_pixel(2, 2),
        Rgba([255, 0, 0, 255])
    );
}
#[test]
fn pixelate_rejects_invalid_strength_small_or_outside_regions() {
    for (r, b) in [
        (rect(0, 0, 2, 2), 0),
        (rect(0, 0, 2, 2), 1),
        (rect(0, 0, 2, 2), 129),
        (rect(0, 0, 1, 2), 12),
        (rect(7, 0, 2, 2), 12),
        (rect(u32::MAX, 0, 2, 2), 12),
    ] {
        let mut d = doc();
        d.annotations.push(pixelate(r, b));
        assert!(editor::render(&d).is_err(), "{r:?}, {b}");
    }
    for b in [2, 12, 128] {
        let mut d = doc();
        d.annotations
            .push(pixelate(rect(0, 0, u32::MAX, u32::MAX), b));
        assert!(editor::render(&d).is_ok());
    }
}
#[test]
fn pixelate_moves_in_source_pixels_and_hit_test_respects_layers() {
    let mut p = pixelate(rect(1, 1, 4, 3), 12);
    assert!(editor::translate_annotation(&mut p, Point { x: 2.2, y: 1.1 }, 8, 6).unwrap());
    assert_eq!(p, pixelate(rect(3, 2, 4, 3), 12));
    assert_eq!(
        editor::hit_test(&[p.clone()], Point { x: 4., y: 3. }, 0.),
        Some(0)
    );
    let high = Annotation::Highlight {
        rect: rect(3, 2, 4, 3),
        colour: [255, 0, 0, 80],
    };
    assert_eq!(
        editor::hit_test(&[high, p], Point { x: 4., y: 3. }, 0.),
        Some(0)
    );
}

#[test]
fn moving_pixelation_resamples_new_content_and_redact_remains_final() {
    let mut d = doc();
    d.annotations = vec![pixelate(rect(0, 0, 2, 2), 2)];
    let first = editor::render(&d).unwrap();
    editor::translate_annotation(&mut d.annotations[0], Point { x: 4., y: 2. }, 8, 6).unwrap();
    let moved = editor::render(&d).unwrap();
    assert_ne!(first.get_pixel(0, 0), moved.get_pixel(4, 2));
    assert_eq!(moved.get_pixel(0, 0), d.source.get_pixel(0, 0));
    let redaction = Annotation::Redact {
        rect: rect(4, 2, 2, 2),
    };
    d.annotations.insert(0, redaction);
    let a = editor::render(&d).unwrap();
    d.annotations.reverse();
    let b = editor::render(&d).unwrap();
    assert_eq!(a, b);
    assert_eq!(*a.get_pixel(4, 2), Rgba([24, 26, 32, 255]));
    assert_eq!(
        editor::hit_test(&d.annotations, Point { x: 4.5, y: 2.5 }, 0.),
        Some(1)
    );
}
#[test]
fn crop_edge_samples_only_eligible_pixels_without_reanchoring_grid() {
    let mut d = doc();
    d.annotations = vec![pixelate(rect(0, 0, 8, 6), 4)];
    d.crop = Some(rect(3, 1, 4, 4));
    let out = editor::render(&d).unwrap();
    assert_eq!(*out.get_pixel(0, 0), Rgba([60, 60, 0, 255]));
    assert_eq!(*out.get_pixel(1, 0), Rgba([100, 60, 0, 255]));
    assert_eq!(*out.get_pixel(0, 3), Rgba([60, 120, 0, 255]));
}
#[test]
fn cumulative_pixelation_work_is_rejected_before_rendering() {
    let mut d = doc();
    d.source = RgbaImage::new(512, 512);
    d.annotations = vec![pixelate(rect(0, 0, 512, 512), 2); 1000];
    assert!(editor::render(&d).unwrap_err().contains("too many pixels"));
}
#[test]
fn flattened_save_drag_pack_and_reopen_preserve_the_same_pixels() {
    use framecraft::output::{self, PackFrame, Snapshot};
    let mut d = doc();
    d.annotations = vec![pixelate(rect(1, 1, 6, 4), 3)];
    let original = d.source.clone();
    let snapshot = Snapshot::current(&d).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let saved = temp.path().join("saved.png");
    snapshot.save(&saved).unwrap();
    let drag = snapshot.drag_file().unwrap();
    assert_eq!(editor::load_image(&saved).unwrap(), snapshot.pixels);
    assert_eq!(editor::load_image(&drag.path).unwrap(), snapshot.pixels);
    let reopened = Document {
        source: editor::load_image(&saved).unwrap(),
        annotations: vec![],
        ..d.clone()
    };
    assert_eq!(editor::render(&reopened).unwrap(), snapshot.pixels);
    let frames = vec![PackFrame {
        snapshot: snapshot.clone(),
        caption: String::new(),
        variant_label: "Edited".into(),
    }];
    let pack = output::export_pack(
        temp.path(),
        "pack",
        "Pixelated",
        &frames,
        &[],
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(
        editor::load_image(&pack.join("frame-0001.png")).unwrap(),
        snapshot.pixels
    );
    assert_eq!(d.source, original);
}
#[test]
#[ignore = "Explicit release-mode 4K compositor timing; not a UI latency assertion"]
fn pixelate_4k_timing() {
    let mut d = doc();
    d.source = RgbaImage::from_fn(3840, 2160, |x, y| {
        Rgba([(x % 256) as u8, (y % 256) as u8, 42, 255])
    });
    d.annotations = vec![
        pixelate(rect(0, 0, 3840, 2160), 12),
        Annotation::Redact {
            rect: rect(1900, 1000, 300, 200),
        },
    ];
    for _ in 0..3 {
        let started = std::time::Instant::now();
        let image = editor::render(&d).unwrap();
        assert_eq!(image.dimensions(), (3840, 2160));
        println!("4K full-image Pixelate12 + Redact: {:?}", started.elapsed());
    }
}

#[test]
fn edit_source_preview_preserves_crop_sampling_and_source_coordinates() {
    let mut a = doc();
    a.crop = Some(rect(3, 1, 4, 4));
    a.annotations = vec![pixelate(rect(0, 0, 8, 6), 4)];
    let preview = editor::render_edit_source(&a).unwrap();
    let exported = editor::render(&a).unwrap();
    assert_eq!(preview.dimensions(), a.source.dimensions());
    assert_eq!(
        image::imageops::crop_imm(&preview, 3, 1, 4, 4).to_image(),
        exported
    );
    let mut b = a.clone();
    for (x, y, pixel) in b.source.enumerate_pixels_mut() {
        if !(3..7).contains(&x) || !(1..5).contains(&y) {
            *pixel = Rgba([251, 43, 71, 90]);
        }
    }
    let second = editor::render_edit_source(&b).unwrap();
    assert_eq!(
        image::imageops::crop_imm(&second, 3, 1, 4, 4).to_image(),
        exported
    );
    a.style.padding = 20;
    a.style.radius = 10;
    a.style.shadow = true;
    assert_eq!(editor::render_edit_source(&a).unwrap(), preview);
}
