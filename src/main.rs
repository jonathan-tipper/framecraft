#[cfg(feature = "ui")]
mod app;
#[cfg(feature = "ui")]
mod session_ui;

use framecraft::editor::{self, Document, Style};
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--demo-export") if args.len() == 2 => {
            export(editor::demo_image(), Path::new(&args[1]))?;
        }
        Some("--demo-session") if args.len() == 2 => {
            demo_session(Path::new(&args[1]))?;
        }
        Some("--demo-pack") if args.len() == 2 => {
            demo_pack(Path::new(&args[1]))?;
        }
        Some("--render") if args.len() == 3 => {
            export(
                editor::load_image(Path::new(&args[1]))?,
                Path::new(&args[2]),
            )?;
        }
        Some("--help" | "-h") => {
            println!("Framecraft\n\nframecraft                       Open capture sessions\nframecraft --demo                Open a labelled editor sample\nframecraft --demo-export OUT.png  Render a sample without a display\nframecraft --demo-session DIR     Create ten synthetic review frames\nframecraft --demo-pack DIR        Create an offline ten-frame pack\nframecraft --render IN OUT.png    Apply the default style\n\nAll capture and editing stays on this computer.");
        }
        None | Some("--demo") if args.len() <= 1 => {
            #[cfg(feature = "ui")]
            {
                let demo = args.first().is_some_and(|a| a == "--demo");
                let options = eframe::NativeOptions {
                    viewport: eframe::egui::ViewportBuilder::default()
                        .with_inner_size([1180.0, 790.0])
                        .with_min_inner_size([900.0, 620.0])
                        .with_title("Framecraft"),
                    ..Default::default()
                };
                eframe::run_native(
                    "Framecraft",
                    options,
                    Box::new(move |cc| Ok(Box::new(app::Framecraft::new(cc, demo)))),
                )?;
            }
            #[cfg(not(feature = "ui"))]
            return Err(
                "UI is disabled. Build with default features, or use --render / --demo-export."
                    .into(),
            );
        }
        _ => return Err("Unknown arguments. Run framecraft --help.".into()),
    }
    Ok(())
}

fn export(source: image::RgbaImage, output: &Path) -> Result<(), Box<dyn Error>> {
    let doc = Document {
        source,
        crop: None,
        annotations: vec![],
        style: Style::default(),
    };
    let image = editor::render(&doc)?;
    // Always encode a new PNG. Source metadata is not copied into the export.
    image.save_with_format(output, image::ImageFormat::Png)?;
    println!(
        "Saved {} x {} PNG to {}",
        image.width(),
        image.height(),
        output.display()
    );
    Ok(())
}

fn sample_document(number: u32) -> Document {
    use framecraft::editor::{Annotation, Point, Rect};
    Document {
        source: editor::demo_image(),
        crop: None,
        annotations: vec![
            Annotation::Text {
                position: Point { x: 50.0, y: 60.0 },
                text: format!("Synthetic review frame {number}"),
                font_size: 32.0,
                colour: [255, 255, 255, 255],
                background: Some([20, 24, 30, 255]),
            },
            Annotation::Step {
                center: Point { x: 100.0, y: 150.0 },
                number,
                radius: 30.0,
                colour: [91, 221, 203, 255],
                text_colour: [17, 22, 29, 255],
            },
            Annotation::Redact {
                rect: Rect {
                    x: 240,
                    y: 300,
                    width: 220,
                    height: 50,
                },
            },
        ],
        style: Style::default(),
    }
}
fn demo_session(parent: &Path) -> Result<(), Box<dyn Error>> {
    use framecraft::{
        output::Snapshot,
        session::{SessionStore, Trigger},
    };
    std::fs::create_dir_all(parent)?;
    let mut store = SessionStore::create(parent, "Synthetic review")?;
    for number in 1..=10 {
        let doc = sample_document(number);
        let saved = store.save(
            &doc.source,
            Some(&format!("Synthetic frame {number}")),
            Trigger::Manual,
        )?;
        let path = saved.path.with_file_name(format!(
            "{}-edited-01.png",
            saved.path.file_stem().unwrap().to_string_lossy()
        ));
        Snapshot::current(&doc)?.save(&path)?;
        store.handle(saved).register_edit(&path)?;
    }
    println!(
        "Created ten synthetic originals and ten edited variants in {}",
        store.folder().display()
    );
    Ok(())
}
fn demo_pack(parent: &Path) -> Result<(), Box<dyn Error>> {
    use framecraft::output::{self, PackFrame, Snapshot};
    std::fs::create_dir_all(parent)?;
    let frames = (1..=10)
        .map(|number| {
            Ok(PackFrame {
                snapshot: Snapshot::current(&sample_document(number))?,
                caption: format!("Synthetic example {number}"),
                variant_label: "Current edits".into(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let path = output::export_pack(
        parent,
        "Framecraft-sample-pack",
        "Synthetic evidence pack",
        &frames,
        &[],
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    )?;
    println!("Created {}", path.display());
    Ok(())
}
