//! Offline filename suggestions and full-text recognition of supplied pixels.
//!
//! macOS uses Apple's on-device Vision OCR through Rust Objective-C bindings.
//! No image is written to disk or sent to a service. The heading selection is
//! deterministic; this is not a generative vision-model captioner. Callers must
//! still sanitise `title` before using it as a filename and provide a timestamp
//! fallback when `suggest` returns `None`.

use image::RgbaImage;
#[cfg(any(target_os = "macos", test))]
use std::borrow::Cow;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameSuggestion {
    pub title: String,
    pub source: &'static str,
    /// Bounded OCR text, in approximate reading order. This can contain private
    /// material from the selected capture; callers should not log it.
    pub text: String,
}

/// Suggest a short title using only text recognised within this image.
///
/// Performs synchronous OCR on macOS, so run it on a background worker. The
/// macOS objects are constructed, used and dropped on that same worker. On
/// other platforms, returns `Ok(None)` without attempting external tools.
pub fn suggest(image: &RgbaImage) -> Result<Option<NameSuggestion>, String> {
    #[cfg(target_os = "macos")]
    {
        if image.width() == 0 || image.height() == 0 {
            return Ok(None);
        }
        Ok(rank_heading(&recognise_lines_macos(image, false)?))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = image;
        Ok(None)
    }
}

/// Recognise all text in a fully rendered capture, in approximate reading order.
///
/// Callers must supply the current flattened render, including crop/redactions,
/// and run this synchronous function on a background worker. Unlike filename
/// suggestions, this retains numbers, URLs, body text, interface labels and footers.
/// Language detection uses the languages supported by installed Apple Vision.
/// No network calls, disk files or retained recognition state are used.
/// Empty recognition succeeds with an empty string. Limits fail explicitly,
/// never silently return a truncated transcript.
pub fn recognise_text(image: &RgbaImage) -> Result<String, String> {
    if image.width() == 0
        || image.height() == 0
        || image.width() > crate::editor::MAX_DIMENSION
        || image.height() > crate::editor::MAX_DIMENSION
        || u64::from(image.width()) * u64::from(image.height()) > crate::editor::MAX_PIXELS
    {
        return Err(
            "Text recognition requires a non-empty image within the supported image limits.".into(),
        );
    }
    #[cfg(target_os = "macos")]
    {
        full_text(&recognise_lines_macos(image, true)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Local text recognition requires macOS and Apple Vision.".into())
    }
}

pub const MAX_FULL_TEXT_CHARS: usize = 32_768;
#[cfg(any(target_os = "macos", test))]
const MAX_TEXT_OBSERVATIONS: usize = 512;

#[cfg(any(target_os = "macos", test))]
const MAX_TITLE_CHARS: usize = 80;
#[cfg(any(target_os = "macos", test))]
const MAX_TEXT_CHARS: usize = 4_096;

/// Vision's bounding boxes are normalised and use a bottom-left origin.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug)]
struct TextLine {
    text: String,
    confidence: f32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[cfg(target_os = "macos")]
fn recognise_lines_macos(image: &RgbaImage, full: bool) -> Result<Vec<TextLine>, String> {
    use image::ImageEncoder;
    use objc2::{rc::autoreleasepool, runtime::AnyObject, AnyThread};
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{
        VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
        VNRequestTextRecognitionLevel,
    };

    // Bound Vision input independently of the source capture's dimensions.
    // Keep the original image borrowed when it is already small enough.
    let max_axis = if full { 4_096 } else { 2_048 };
    let thumbnail;
    let pixels = if image.width() > max_axis || image.height() > max_axis {
        thumbnail = image::imageops::thumbnail(image, max_axis, max_axis);
        &thumbnail
    } else {
        image
    };
    // Contrast processing uses only the bounded rendered input. Composite alpha
    // first so invisible RGB channels can never contribute to recognition.
    let normalised;
    let pixels = if full {
        normalised = full_text_contrast(pixels);
        normalised.as_ref()
    } else {
        pixels
    };
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| {
            format!("Could not prepare the capture for local text recognition: {error}")
        })?;

    autoreleasepool(|_| {
        let data = NSData::with_bytes(&png);
        let options = NSDictionary::<VNImageOption, AnyObject>::new();
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &options,
        );
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setMinimumTextHeight(if full { 0.0 } else { 0.012 });
        // Script/language detection uses the installed Vision implementation.
        // The app does not download a separate model or contact an endpoint.
        request.setAutomaticallyDetectsLanguage(true);
        let base_request: &VNRequest = &request;
        let requests = NSArray::<VNRequest>::from_slice(&[base_request]);
        handler.performRequests_error(&requests).map_err(|error| {
            format!(
                "Local text recognition could not finish: {}",
                error.localizedDescription()
            )
        })?;

        let Some(observations) = request.results() else {
            return Ok(Vec::new());
        };
        if full && observations.count() > MAX_TEXT_OBSERVATIONS {
            return Err(
                "Too many text regions to recognise at once; crop to a smaller area.".into(),
            );
        }
        let mut lines = Vec::new();
        for index in 0..observations.count().min(MAX_TEXT_OBSERVATIONS) {
            let observation = observations.objectAtIndex(index);
            let candidates = observation.topCandidates(1);
            let Some(candidate) = candidates.firstObject() else {
                continue;
            };
            let confidence = candidate.confidence();
            if !confidence.is_finite() || (!full && confidence <= 0.65) {
                continue;
            }
            // SAFETY: `observation` is a live Vision result from the completed
            // request. boundingBox takes no pointers and returns a value type.
            let bounds = unsafe { observation.boundingBox() };
            lines.push(TextLine {
                text: candidate.string().to_string(),
                confidence,
                x: bounds.origin.x as f32,
                y: bounds.origin.y as f32,
                width: bounds.size.width as f32,
                height: bounds.size.height as f32,
            });
        }
        Ok(lines)
    })
}

/// Prepare only full-text OCR pixels, never the stored image or its export.
/// Composite RGBA onto opaque white. Sample at most 4,096 evenly spaced pixels;
/// invert RGB when at least 60% have integer luma (54R+183G+19B)/256 below 96.
/// This favours light-on-dark interfaces that Vision can miss in a full scene.
/// It is a bounded one-pass heuristic, not a general OCR accuracy guarantee.
#[cfg(any(target_os = "macos", test))]
fn full_text_contrast(image: &RgbaImage) -> Cow<'_, RgbaImage> {
    let opaque_rgb = |pixel: &image::Rgba<u8>| -> [u8; 3] {
        let alpha = u32::from(pixel[3]);
        [0, 1, 2].map(|i| ((u32::from(pixel[i]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8)
    };
    let count = image.as_raw().len() / 4;
    let stride = count.div_ceil(4_096).max(1);
    let mut dark = 0usize;
    let mut sampled = 0usize;
    for pixel in image.pixels().step_by(stride) {
        let [r, g, b] = opaque_rgb(pixel);
        let luma = (54 * u32::from(r) + 183 * u32::from(g) + 19 * u32::from(b)) / 256;
        dark += usize::from(luma < 96);
        sampled += 1;
    }
    let invert = sampled > 0 && dark * 5 >= sampled * 3;
    if !invert && image.pixels().all(|pixel| pixel[3] == 255) {
        return Cow::Borrowed(image);
    }
    Cow::Owned(RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        let mut rgb = opaque_rgb(image.get_pixel(x, y));
        if invert {
            rgb = rgb.map(|channel| 255 - channel);
        }
        image::Rgba([rgb[0], rgb[1], rgb[2], 255])
    }))
}

/// Group approximately aligned observations into rows, then read each row from
/// left to right. This is deterministic, not a document/table structure parser.
#[cfg(any(target_os = "macos", test))]
fn full_text(lines: &[TextLine]) -> Result<String, String> {
    if lines.len() > MAX_TEXT_OBSERVATIONS {
        return Err("Too many text regions to recognise at once; crop to a smaller area.".into());
    }
    let mut valid = Vec::new();
    for line in lines {
        if ![line.confidence, line.x, line.y, line.width, line.height]
            .iter()
            .all(|v| v.is_finite())
            || line.width <= 0.0
            || line.height <= 0.0
            || line.x < -0.001
            || line.y < -0.001
            || line.x + line.width > 1.01
            || line.y + line.height > 1.01
        {
            continue;
        }
        if line.text.chars().count() > MAX_FULL_TEXT_CHARS {
            return Err(
                "The recognised text exceeds 32768 characters; crop to a smaller area.".into(),
            );
        }
        let text = normalise_text(&line.text);
        if !text.is_empty() {
            valid.push((line, text));
        }
    }
    let centre = |line: &TextLine| line.y + line.height / 2.0;
    valid.sort_by(|(a, at), (b, bt)| {
        centre(b)
            .total_cmp(&centre(a))
            .then_with(|| a.x.total_cmp(&b.x))
            .then_with(|| at.cmp(bt))
    });
    let mut rows: Vec<Vec<(&TextLine, String)>> = Vec::new();
    for item in valid {
        if let Some(row) = rows.last_mut() {
            let first = row[0].0;
            if (centre(first) - centre(item.0)).abs() <= first.height.min(item.0.height) * 0.5 {
                row.push(item);
                continue;
            }
        }
        rows.push(vec![item]);
    }
    let mut output = String::new();
    let mut count = 0;
    for mut row in rows {
        row.sort_by(|(a, at), (b, bt)| {
            a.x.total_cmp(&b.x)
                .then_with(|| centre(b).total_cmp(&centre(a)))
                .then_with(|| at.cmp(bt))
        });
        for (_, text) in row {
            count += text.chars().count() + usize::from(!output.is_empty());
            if count > MAX_FULL_TEXT_CHARS {
                return Err(
                    "The recognised text exceeds 32768 characters; crop to a smaller area.".into(),
                );
            }
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&text);
        }
    }
    Ok(output)
}

#[cfg(any(target_os = "macos", test))]
fn normalise_text(text: &str) -> String {
    text.split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(target_os = "macos", test))]
fn meaningful(text: &str) -> bool {
    let length = text.chars().count();
    if !(3..=180).contains(&length) {
        return false;
    }
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let alphanumeric = text.chars().filter(|c| c.is_alphanumeric()).count();
    let visible = text.chars().filter(|c| !c.is_whitespace()).count();
    if letters < 3 || alphanumeric * 2 < visible {
        return false;
    }
    let lower = text.to_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("www.")
        || (text.contains('@') && !text.contains(' '))
    {
        return false;
    }
    !matches!(
        lower.trim_matches(|c: char| !c.is_alphanumeric()),
        "microsoft teams"
            | "teams"
            | "presenting"
            | "you are presenting"
            | "stop presenting"
            | "stop sharing"
            | "share content"
            | "chat"
            | "people"
            | "participants"
            | "reactions"
            | "raise hand"
            | "more"
            | "camera"
            | "mic"
            | "mute"
            | "unmute"
            | "leave"
            | "share"
            | "search"
            | "settings"
            | "view"
            | "recording"
    )
}

#[cfg(any(target_os = "macos", test))]
fn short_title(text: &str) -> String {
    let mut title = String::new();
    for word in text.split_whitespace().take(12) {
        let extra = word.chars().count() + usize::from(!title.is_empty());
        if title.chars().count() + extra > MAX_TITLE_CHARS {
            break;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    if title.is_empty() {
        // A single long word still has a bounded, valid UTF-8 title.
        title = text.chars().take(MAX_TITLE_CHARS).collect();
    }
    title
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, '•' | '|' | ':' | ';'))
        .to_owned()
}

#[cfg(any(target_os = "macos", test))]
fn rank_heading(lines: &[TextLine]) -> Option<NameSuggestion> {
    let mut candidates = lines
        .iter()
        .filter(|line| {
            [line.confidence, line.x, line.y, line.width, line.height]
                .iter()
                .all(|value| value.is_finite())
                && line.confidence > 0.65
                && (0.012..=0.4).contains(&line.height)
                && line.width > 0.015
                && line.x >= -0.001
                && line.y >= -0.001
                && line.x + line.width <= 1.01
                && line.y + line.height <= 1.01
                // Tiny footer text should not become a slide title.
                && !(line.y < 0.08 && line.height < 0.04)
        })
        .map(|line| (line, normalise_text(&line.text)))
        .filter(|(_, text)| meaningful(text))
        .collect::<Vec<_>>();
    let score = |line: &TextLine, text: &str| {
        let words = text.split_whitespace().count();
        let upper_position = (line.y + line.height / 2.0).clamp(0.0, 1.0);
        let length_penalty = if words > 14 {
            (words - 14) as f32 * 0.3
        } else {
            0.0
        };
        line.height.min(0.16) * 100.0
            + upper_position * 2.5
            + line.confidence
            + words.min(8) as f32 * 0.08
            - length_penalty
    };
    let (_, heading) = candidates
        .iter()
        .max_by(|(a, a_text), (b, b_text)| score(a, a_text).total_cmp(&score(b, b_text)))?;
    let title = short_title(heading);
    if !meaningful(&title) {
        return None;
    }

    // Vision reports a bottom-left origin: decreasing y reads top to bottom.
    candidates.sort_by(|(a, _), (b, _)| b.y.total_cmp(&a.y).then_with(|| a.x.total_cmp(&b.x)));
    let text = candidates
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(MAX_TEXT_CHARS)
        .collect();
    Some(NameSuggestion {
        title,
        source: "apple-vision-ocr",
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::{rank_heading, TextLine, MAX_TEXT_CHARS, MAX_TITLE_CHARS};

    fn line(text: &str, y: f32, height: f32, confidence: f32) -> TextLine {
        TextLine {
            text: text.into(),
            confidence,
            x: 0.1,
            y,
            width: 0.8,
            height,
        }
    }

    #[test]
    fn prominent_heading_beats_body_and_meeting_chrome() {
        let result = rank_heading(&[
            line("Microsoft Teams", 0.94, 0.05, 0.99),
            line("Capital projects outlook", 0.78, 0.065, 0.96),
            line("The next programme starts in September", 0.50, 0.025, 0.98),
            line("Stop sharing", 0.04, 0.04, 0.99),
        ])
        .unwrap();
        assert_eq!(result.title, "Capital projects outlook");
        assert_eq!(result.source, "apple-vision-ocr");
        assert!(!result.text.contains("Microsoft Teams"));
    }

    #[test]
    fn upper_heading_wins_when_text_sizes_match() {
        let result = rank_heading(&[
            line("Upper heading", 0.80, 0.06, 0.90),
            line("Lower heading", 0.30, 0.06, 0.90),
        ])
        .unwrap();
        assert_eq!(result.title, "Upper heading");
        assert!(result.text.starts_with("Upper heading\n"));
    }

    #[test]
    fn no_meaningful_text_returns_no_suggestion() {
        assert!(rank_heading(&[
            line("123456", 0.80, 0.06, 0.99),
            line("Uncertain heading", 0.80, 0.06, 0.65),
            line("Unreadable fine print", 0.80, 0.005, 0.99),
            line("www.example.com", 0.80, 0.06, 0.99),
            line("--- !!! ???", 0.80, 0.06, 0.99),
        ])
        .is_none());
    }

    #[test]
    fn non_finite_metrics_and_small_footer_are_rejected() {
        assert!(rank_heading(&[
            line("Bad dimensions", f32::NAN, 0.06, 0.99),
            line("Confidential footer", 0.02, 0.02, 0.99),
        ])
        .is_none());
    }

    #[test]
    fn unicode_is_preserved_without_breaking_utf8() {
        let result =
            rank_heading(&[line("  Réseau\n énergétique   2027  ", 0.80, 0.06, 0.99)]).unwrap();
        assert_eq!(result.title, "Réseau énergétique 2027");
    }

    #[test]
    fn title_and_ocr_text_are_bounded() {
        let lines = (0..100)
            .map(|_| line("Energy infrastructure investment programme operating assumptions for the next financial planning cycle and beyond", 0.80, 0.06, 0.99))
            .collect::<Vec<_>>();
        let result = rank_heading(&lines).unwrap();
        assert!(result.title.chars().count() <= MAX_TITLE_CHARS);
        assert!(result.title.split_whitespace().count() <= 12);
        assert!(result.text.chars().count() <= MAX_TEXT_CHARS);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn unsupported_platform_has_no_external_fallback() {
        assert_eq!(super::suggest(&image::RgbaImage::new(1, 1)).unwrap(), None);
    }
    #[test]
    fn full_text_keeps_numbers_urls_footer_and_low_confidence_in_reading_order() {
        let mut right = line("Right", 0.80, 0.04, 0.8);
        right.x = 0.55;
        right.width = 0.3;
        let mut left = line("12345", 0.79, 0.04, 0.3);
        left.width = 0.3;
        let lines = vec![
            line("www.example.com", 0.50, 0.02, 0.9),
            right,
            line("Footer", 0.01, 0.005, 0.8),
            left,
        ];
        assert_eq!(
            super::full_text(&lines).unwrap(),
            "12345\nRight\nwww.example.com\nFooter"
        );
    }

    #[test]
    fn full_text_is_unicode_safe_bounded_and_rejects_invalid_geometry() {
        assert_eq!(
            super::full_text(&[
                line(" Réseau  2027 ", 0.5, 0.04, 0.9),
                line("invalid", f32::NAN, 0.04, 0.9)
            ])
            .unwrap(),
            "Réseau 2027"
        );
        assert!(super::full_text(&[line(
            &"é".repeat(super::MAX_FULL_TEXT_CHARS + 1),
            0.5,
            0.04,
            0.9
        )])
        .is_err());
        assert_eq!(super::full_text(&[]).unwrap(), "");
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit native Apple Vision smoke test, using synthetic pixels only"]
    fn native_ocr_uses_only_flattened_synthetic_pixels() {
        use crate::editor::{render, Annotation, Document, Point, Rect, Style, Theme};
        let label = |y, text: &str| Annotation::Text {
            position: Point { x: 20.0, y },
            text: text.into(),
            font_size: 48.0,
            colour: [0, 0, 0, 255],
            background: None,
        };
        let doc = Document {
            source: image::RgbaImage::from_pixel(1_100, 500, image::Rgba([255; 4])),
            crop: Some(Rect {
                x: 0,
                y: 0,
                width: 1_100,
                height: 300,
            }),
            annotations: vec![
                label(20.0, "Framecraft local text 2026"),
                label(80.0, "Second line 12345"),
                label(160.0, "REDACTEDSECRET"),
                label(380.0, "CROPPEDSECRET"),
                Annotation::Redact {
                    rect: Rect {
                        x: 0,
                        y: 150,
                        width: 1_100,
                        height: 100,
                    },
                },
            ],
            style: Style {
                padding: 0,
                radius: 0,
                shadow: false,
                theme: Theme::Paper,
            },
        };
        let text = super::recognise_text(&render(&doc).unwrap()).unwrap();
        assert!(
            text.contains("Framecraft local text 2026"),
            "missing first line: {text}"
        );
        assert!(
            text.contains("Second line 12345"),
            "missing second line: {text}"
        );
        assert!(
            !text.contains("SECRET"),
            "recognised cropped/redacted text: {text}"
        );
        assert!(text.find("Framecraft").unwrap() < text.find("Second").unwrap());
    }

    #[test]
    fn full_text_contrast_preserves_light_pixels_and_removes_hidden_alpha_rgb() {
        let light = image::RgbaImage::from_pixel(10, 10, image::Rgba([240, 240, 240, 255]));
        assert!(matches!(
            super::full_text_contrast(&light),
            std::borrow::Cow::Borrowed(_)
        ));
        let mut alpha = light.clone();
        alpha.put_pixel(0, 0, image::Rgba([255, 0, 0, 0]));
        alpha.put_pixel(1, 0, image::Rgba([0, 0, 0, 128]));
        let result = super::full_text_contrast(&alpha);
        assert_eq!(result.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(result.get_pixel(1, 0).0, [127, 127, 127, 255]);
        assert_eq!(alpha.get_pixel(0, 0).0, [255, 0, 0, 0]);
    }

    #[test]
    fn full_text_contrast_inverts_only_at_documented_dark_fraction() {
        let mut pixels = image::RgbaImage::from_pixel(10, 1, image::Rgba([255; 4]));
        for x in 0..5 {
            pixels.put_pixel(x, 0, image::Rgba([20, 24, 30, 255]));
        }
        assert!(matches!(
            super::full_text_contrast(&pixels),
            std::borrow::Cow::Borrowed(_)
        ));
        pixels.put_pixel(5, 0, image::Rgba([20, 24, 30, 255]));
        let result = super::full_text_contrast(&pixels);
        assert_eq!(result.get_pixel(0, 0).0, [235, 231, 225, 255]);
        assert_eq!(result.get_pixel(9, 0).0, [0, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(0, 0).0, [20, 24, 30, 255]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit native Apple Vision dark-interface regression, synthetic pixels only"]
    fn native_dark_interface_ocr_recognises_label_without_revealing_redaction() {
        use crate::editor::{render, Annotation, Document, Point, Rect, Style};
        let mut doc = Document {
            source: crate::editor::demo_image(),
            crop: None,
            style: Style::default(),
            annotations: vec![
                Annotation::Text {
                    position: Point { x: 50.0, y: 60.0 },
                    text: "Synthetic review frame 1".into(),
                    font_size: 32.0,
                    colour: [255; 4],
                    background: Some([20, 24, 30, 255]),
                },
                Annotation::Step {
                    center: Point { x: 100.0, y: 150.0 },
                    number: 1,
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
        };
        let text = super::recognise_text(&render(&doc).unwrap()).unwrap();
        assert!(
            text.contains("Synthetic review frame 1"),
            "missing dark-scene label: {text}"
        );
        doc.annotations.push(Annotation::Redact {
            rect: Rect {
                x: 50,
                y: 60,
                width: 470,
                height: 60,
            },
        });
        let text = super::recognise_text(&render(&doc).unwrap()).unwrap();
        assert!(
            !text.contains("Synthetic"),
            "recognised redacted label: {text}"
        );
    }
}
