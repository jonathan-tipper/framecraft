//! Offline filename suggestions from the text already visible in a capture.
//!
//! macOS uses Apple's on-device Vision OCR through Rust Objective-C bindings.
//! No image is written to disk or sent to a service. The heading selection is
//! deterministic; this is not a generative vision-model captioner. Callers must
//! still sanitise `title` before using it as a filename and provide a timestamp
//! fallback when `suggest` returns `None`.

use image::RgbaImage;

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
        suggest_macos(image)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = image;
        Ok(None)
    }
}

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
fn suggest_macos(image: &RgbaImage) -> Result<Option<NameSuggestion>, String> {
    use image::ImageEncoder;
    use objc2::{rc::autoreleasepool, runtime::AnyObject, AnyThread};
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{
        VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
        VNRequestTextRecognitionLevel,
    };

    if image.width() == 0 || image.height() == 0 {
        return Ok(None);
    }

    // Bound Vision input independently of the source capture's dimensions.
    // Keep the original image borrowed when it is already small enough.
    let thumbnail;
    let pixels = if image.width() > 2_048 || image.height() > 2_048 {
        thumbnail = image::imageops::thumbnail(image, 2_048, 2_048);
        &thumbnail
    } else {
        image
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
        request.setMinimumTextHeight(0.012);
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
            return Ok(None);
        };
        let mut lines = Vec::new();
        for index in 0..observations.count().min(512) {
            let observation = observations.objectAtIndex(index);
            let candidates = observation.topCandidates(1);
            let Some(candidate) = candidates.firstObject() else {
                continue;
            };
            let confidence = candidate.confidence();
            if !confidence.is_finite() || confidence <= 0.65 {
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
        Ok(rank_heading(&lines))
    })
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
}
