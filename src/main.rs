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
        Some("--render") if args.len() == 3 => {
            export(
                editor::load_image(Path::new(&args[1]))?,
                Path::new(&args[2]),
            )?;
        }
        Some("--help" | "-h") => {
            println!("Framecraft\n\nframecraft                       Open capture sessions\nframecraft --demo                Open a labelled editor sample\nframecraft --demo-export OUT.png  Render a sample without a display\nframecraft --render IN OUT.png    Apply the default style\n\nAll capture and editing stays on this computer.");
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
