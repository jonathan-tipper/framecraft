//! Deterministic pixel compositor. Coordinates always refer to the source image.
//!
//! Redactions are deliberately applied last, before cropping and presentation.
//! The returned image contains pixels only: no original layers or metadata.

use std::{borrow::Cow, path::Path};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, Rgba, RgbaImage};

pub const MAX_PIXELS: u64 = 64_000_000;
pub const MAX_DIMENSION: u32 = 16_384;
const REDACTION_COLOUR: Rgba<u8> = Rgba([24, 26, 32, 255]);
const MAX_ANNOTATIONS: usize = 1_000;
const MAX_FREEHAND_POINTS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub enum Annotation {
    Arrow {
        from: Point,
        to: Point,
        colour: [u8; 4],
        width: f32,
    },
    Rectangle {
        rect: Rect,
        colour: [u8; 4],
        width: u32,
    },
    Highlight {
        rect: Rect,
        colour: [u8; 4],
    },
    Freehand {
        points: Vec<Point>,
        colour: [u8; 4],
        width: f32,
    },
    Redact {
        rect: Rect,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Midnight,
    Lagoon,
    Paper,
    Sunset,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Lagoon => "Lagoon",
            Self::Paper => "Paper",
            Self::Sunset => "Sunset",
        }
    }

    fn colours(self) -> ([u8; 3], [u8; 3], [u8; 3]) {
        match self {
            Self::Midnight => ([23, 26, 56], [58, 49, 94], [83, 71, 133]),
            Self::Lagoon => ([11, 49, 59], [31, 110, 116], [105, 209, 190]),
            Self::Paper => ([237, 233, 222], [223, 224, 217], [251, 249, 242]),
            Self::Sunset => ([90, 42, 72], [186, 92, 97], [247, 176, 135]),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Style {
    pub padding: u32,
    pub radius: u32,
    pub shadow: bool,
    pub theme: Theme,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            padding: 64,
            radius: 18,
            shadow: true,
            theme: Theme::Lagoon,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub source: RgbaImage,
    pub crop: Option<Rect>,
    pub annotations: Vec<Annotation>,
    pub style: Style,
}

/// Validate dimensions before allocating a pixel buffer.
fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("Images and crops must have non-zero dimensions.".into());
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(format!(
            "Each image dimension must be at most {MAX_DIMENSION} pixels."
        ));
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("Images must contain at most 64 million pixels.".into());
    }
    Ok(())
}

fn crop_rect(doc: &Document) -> Result<Rect, String> {
    let (width, height) = doc.source.dimensions();
    check_dimensions(width, height)?;
    let crop = doc.crop.unwrap_or(Rect {
        x: 0,
        y: 0,
        width,
        height,
    });
    if crop.width == 0 || crop.height == 0 {
        return Err("The crop must have a non-zero width and height.".into());
    }
    if u64::from(crop.x) + u64::from(crop.width) > u64::from(width)
        || u64::from(crop.y) + u64::from(crop.height) > u64::from(height)
    {
        return Err("The crop extends beyond the source image.".into());
    }
    Ok(crop)
}

/// Output dimensions include exactly `padding` pixels on each edge.
pub fn dimensions(doc: &Document) -> Result<(u32, u32), String> {
    let crop = crop_rect(doc)?;
    let border = doc
        .style
        .padding
        .checked_mul(2)
        .ok_or("Padding is too large.")?;
    let width = crop
        .width
        .checked_add(border)
        .ok_or("Output width is too large.")?;
    let height = crop
        .height
        .checked_add(border)
        .ok_or("Output height is too large.")?;
    check_dimensions(width, height)?;
    Ok((width, height))
}

/// Decode PNG or JPEG by its content, with a pixel limit checked before decoding.
/// Orientation is applied, but source EXIF, ICC and other metadata are not retained.
pub fn load_image(path: &Path) -> Result<RgbaImage, String> {
    let mut reader = ImageReader::open(path)
        .map_err(|e| format!("Could not open image: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("Could not identify image: {e}"))?;
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return Err("Choose a PNG or JPEG image.".into());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("Could not read image: {e}"))?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    // Check the decoder on the same open file, avoiding a dimensions/decode race.
    if decoder.total_bytes() > MAX_PIXELS * 8 {
        return Err("The decoded image exceeds the memory limit.".into());
    }
    let orientation = decoder
        .orientation()
        .map_err(|e| format!("Invalid image orientation: {e}"))?;
    let mut decoded =
        DynamicImage::from_decoder(decoder).map_err(|e| format!("Could not decode image: {e}"))?;
    decoded.apply_orientation(orientation);
    Ok(decoded.into_rgba8())
}

fn clipped_rect(rect: Rect, width: u32, height: u32) -> Option<Rect> {
    let right = (u64::from(rect.x) + u64::from(rect.width)).min(u64::from(width)) as u32;
    let bottom = (u64::from(rect.y) + u64::from(rect.height)).min(u64::from(height)) as u32;
    if rect.width == 0 || rect.height == 0 || rect.x >= right || rect.y >= bottom {
        None
    } else {
        Some(Rect {
            x: rect.x,
            y: rect.y,
            width: right - rect.x,
            height: bottom - rect.y,
        })
    }
}

fn validate_annotations(doc: &Document) -> Result<(), String> {
    if doc.annotations.len() > MAX_ANNOTATIONS {
        return Err(format!(
            "Use no more than {MAX_ANNOTATIONS} annotations per image."
        ));
    }
    for annotation in &doc.annotations {
        match annotation {
            Annotation::Arrow {
                from, to, width, ..
            } => {
                if ![from.x, from.y, to.x, to.y]
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
                    || !width.is_finite()
                    || *width <= 0.0
                    || *width > 512.0
                {
                    return Err("Arrow coordinates must be finite and line width must be between 0 and 512 pixels.".into());
                }
            }
            Annotation::Rectangle { rect, width, .. } => {
                if rect.width == 0 || rect.height == 0 || *width == 0 || *width > 512 {
                    return Err("Rectangles need non-zero dimensions and a line width from 1 to 512 pixels.".into());
                }
            }
            Annotation::Highlight { rect, .. } => {
                if clipped_rect(*rect, doc.source.width(), doc.source.height()).is_none() {
                    return Err("A highlight must cover at least one source pixel.".into());
                }
            }
            Annotation::Freehand { points, width, .. } => {
                if points.is_empty() || points.len() > MAX_FREEHAND_POINTS {
                    return Err(format!(
                        "Freehand strokes need 1 to {MAX_FREEHAND_POINTS} points."
                    ));
                }
                if !points.iter().all(|point| {
                    [point.x, point.y]
                        .iter()
                        .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
                }) || !width.is_finite()
                    || *width <= 0.0
                    || *width > 512.0
                {
                    return Err("Freehand coordinates must be finite and bounded, and line width must be between 0 and 512 pixels.".into());
                }
            }
            Annotation::Redact { rect } => {
                if clipped_rect(*rect, doc.source.width(), doc.source.height()).is_none() {
                    return Err("A redaction must cover at least one source pixel.".into());
                }
            }
        }
    }
    Ok(())
}

/// Render annotations, then crop, then add presentation styling.
/// Redactions always obscure other annotations, regardless of creation order.
pub fn render(doc: &Document) -> Result<RgbaImage, String> {
    let (width, height) = dimensions(doc)?;
    let crop = crop_rect(doc)?;
    validate_annotations(doc)?;
    let mut source = Cow::Borrowed(&doc.source);
    if !doc.annotations.is_empty() {
        let target = source.to_mut();
        for annotation in &doc.annotations {
            match annotation {
                Annotation::Arrow {
                    from,
                    to,
                    colour,
                    width,
                } => draw_arrow(target, *from, *to, *colour, *width),
                Annotation::Rectangle {
                    rect,
                    colour,
                    width,
                } => draw_rectangle(target, *rect, *colour, *width),
                Annotation::Highlight { rect, colour } => draw_highlight(target, *rect, *colour),
                Annotation::Freehand {
                    points,
                    colour,
                    width,
                } => draw_freehand(target, points, *colour, *width),
                Annotation::Redact { .. } => {}
            }
        }
        // Never alpha-blend redactions. Opaque replacement destroys source pixels.
        for annotation in &doc.annotations {
            if let Annotation::Redact { rect } = annotation {
                fill_rect(target, *rect, REDACTION_COLOUR);
            }
        }
    }

    let padding = doc.style.padding;
    if padding == 0 && doc.style.radius == 0 && !doc.style.shadow {
        return Ok(image::imageops::crop_imm(
            source.as_ref(),
            crop.x,
            crop.y,
            crop.width,
            crop.height,
        )
        .to_image());
    }

    let mut output = RgbaImage::new(width, height);
    let radius = doc.style.radius.min(crop.width / 2).min(crop.height / 2) as f32;
    let half_width = crop.width as f32 / 2.0;
    let half_height = crop.height as f32 / 2.0;
    let shadow_size = (padding as f32 * 0.3).clamp(4.0, 26.0);
    let shadow_offset = (padding as f32 * 0.2).clamp(2.0, 16.0);

    for (x, y, pixel) in output.enumerate_pixels_mut() {
        *pixel = if padding > 0 {
            gradient(doc.style.theme, x, y, width, height)
        } else {
            Rgba([0, 0, 0, 0])
        };
        let local_x = x as f32 + 0.5 - padding as f32;
        let local_y = y as f32 + 0.5 - padding as f32;
        if doc.style.shadow && padding > 0 {
            let distance = rounded_distance(
                local_x - half_width,
                local_y - half_height - shadow_offset,
                half_width,
                half_height,
                radius,
            );
            // Analytic soft shadow avoids allocating and blurring another image.
            let alpha = if distance <= 0.0 {
                0.30
            } else if distance < shadow_size * 3.0 {
                0.30 * (-distance * distance / (2.0 * shadow_size * shadow_size)).exp()
            } else {
                0.0
            };
            blend(pixel, [5, 9, 18, (alpha * 255.0).round() as u8], 1.0);
        }
        if x >= padding && y >= padding && x - padding < crop.width && y - padding < crop.height {
            let coverage = if radius == 0.0 {
                1.0
            } else {
                (0.5 - rounded_distance(
                    local_x - half_width,
                    local_y - half_height,
                    half_width,
                    half_height,
                    radius,
                ))
                .clamp(0.0, 1.0)
            };
            if coverage > 0.0 {
                let source_pixel = source.get_pixel(crop.x + x - padding, crop.y + y - padding);
                blend(pixel, source_pixel.0, coverage);
            }
        }
    }
    Ok(output)
}

fn rounded_distance(x: f32, y: f32, half_width: f32, half_height: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half_width - radius);
    let qy = y.abs() - (half_height - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

fn gradient(theme: Theme, x: u32, y: u32, width: u32, height: u32) -> Rgba<u8> {
    let (start, end, glow) = theme.colours();
    let nx = x as f32 / width.max(1) as f32;
    let ny = y as f32 / height.max(1) as f32;
    let t = (nx * 0.55 + ny * 0.45).clamp(0.0, 1.0);
    let light = (1.0 - ((nx - 0.82).powi(2) + (ny - 0.18).powi(2)) * 1.5).clamp(0.0, 1.0) * 0.35;
    let mut colour = [0, 0, 0, 255];
    for channel in 0..3 {
        let base = start[channel] as f32 * (1.0 - t) + end[channel] as f32 * t;
        colour[channel] = (base * (1.0 - light) + glow[channel] as f32 * light).round() as u8;
    }
    Rgba(colour)
}

/// Straight-alpha source-over. RGB never retains hidden source pixels at alpha 0.
fn blend(destination: &mut Rgba<u8>, source: [u8; 4], coverage: f32) {
    let source_alpha = source[3] as f32 / 255.0 * coverage.clamp(0.0, 1.0);
    if source_alpha <= 0.0 {
        return;
    }
    if source_alpha >= 1.0 {
        *destination = Rgba(source);
        return;
    }
    let destination_alpha = destination[3] as f32 / 255.0;
    let output_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
    for channel in 0..3 {
        destination[channel] = ((source[channel] as f32 * source_alpha
            + destination[channel] as f32 * destination_alpha * (1.0 - source_alpha))
            / output_alpha)
            .round() as u8;
    }
    destination[3] = (output_alpha * 255.0).round() as u8;
    if destination[3] == 0 {
        *destination = Rgba([0, 0, 0, 0]);
    }
}

fn fill_rect(image: &mut RgbaImage, rect: Rect, colour: Rgba<u8>) {
    if let Some(rect) = clipped_rect(rect, image.width(), image.height()) {
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                image.put_pixel(x, y, colour);
            }
        }
    }
}

fn draw_rectangle(image: &mut RgbaImage, rect: Rect, colour: [u8; 4], width: u32) {
    if let Some(clipped) = clipped_rect(rect, image.width(), image.height()) {
        let right = u64::from(rect.x) + u64::from(rect.width);
        let bottom = u64::from(rect.y) + u64::from(rect.height);
        for y in clipped.y..clipped.y + clipped.height {
            for x in clipped.x..clipped.x + clipped.width {
                if x - rect.x < width
                    || y - rect.y < width
                    || right - u64::from(x) <= u64::from(width)
                    || bottom - u64::from(y) <= u64::from(width)
                {
                    blend(image.get_pixel_mut(x, y), colour, 1.0);
                }
            }
        }
    }
}

fn draw_highlight(image: &mut RgbaImage, rect: Rect, colour: [u8; 4]) {
    if let Some(rect) = clipped_rect(rect, image.width(), image.height()) {
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                blend(image.get_pixel_mut(x, y), colour, 1.0);
            }
        }
    }
}

fn distance_to_segment(x: f32, y: f32, from: Point, to: Point) -> f32 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let denominator = dx * dx + dy * dy;
    let t = if denominator <= f32::EPSILON {
        0.0
    } else {
        ((x - from.x) * dx + (y - from.y) * dy) / denominator
    }
    .clamp(0.0, 1.0);
    (x - (from.x + t * dx)).hypot(y - (from.y + t * dy))
}

fn draw_arrow(image: &mut RgbaImage, from: Point, to: Point, colour: [u8; 4], width: f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length = dx.hypot(dy);
    let angle = dy.atan2(dx);
    let head_length = (width * 4.0).max(14.0).min(length * 0.45);
    let a = Point {
        x: to.x - head_length * (angle - 0.5).cos(),
        y: to.y - head_length * (angle - 0.5).sin(),
    };
    let b = Point {
        x: to.x - head_length * (angle + 0.5).cos(),
        y: to.y - head_length * (angle + 0.5).sin(),
    };
    draw_segments(image, &[(from, to), (to, a), (to, b)], colour, width);
}

fn draw_freehand(image: &mut RgbaImage, points: &[Point], colour: [u8; 4], width: f32) {
    if let [point] = points {
        draw_segments(image, &[(*point, *point)], colour, width);
    } else {
        let segments: Vec<_> = points.windows(2).map(|pair| (pair[0], pair[1])).collect();
        draw_segments(image, &segments, colour, width);
    }
}

fn stroke_bounds(
    points: impl IntoIterator<Item = Point>,
    width: f32,
    image_width: u32,
    image_height: u32,
) -> Option<Rect> {
    let margin = width / 2.0 + 1.0;
    let mut minimum = Point {
        x: f32::INFINITY,
        y: f32::INFINITY,
    };
    let mut maximum = Point {
        x: f32::NEG_INFINITY,
        y: f32::NEG_INFINITY,
    };
    for point in points {
        minimum.x = minimum.x.min(point.x);
        minimum.y = minimum.y.min(point.y);
        maximum.x = maximum.x.max(point.x);
        maximum.y = maximum.y.max(point.y);
    }
    let min_x = (minimum.x - margin).floor().clamp(0.0, image_width as f32) as u32;
    let max_x = (maximum.x + margin).ceil().clamp(0.0, image_width as f32) as u32;
    let min_y = (minimum.y - margin).floor().clamp(0.0, image_height as f32) as u32;
    let max_y = (maximum.y + margin).ceil().clamp(0.0, image_height as f32) as u32;
    if min_x >= max_x || min_y >= max_y {
        None
    } else {
        Some(Rect {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        })
    }
}

fn draw_segments(image: &mut RgbaImage, segments: &[(Point, Point)], colour: [u8; 4], width: f32) {
    if colour[3] == 0 {
        return;
    }
    let Some(bounds) = stroke_bounds(
        segments.iter().flat_map(|(from, to)| [*from, *to]),
        width,
        image.width(),
        image.height(),
    ) else {
        return;
    };
    // Union coverage before blending so joins and self-intersections do not
    // apply the stroke's alpha repeatedly. Allocation is bounded by the image.
    let mut coverage = vec![0_u8; bounds.width as usize * bounds.height as usize];
    for (from, to) in segments {
        let Some(segment_bounds) =
            stroke_bounds([*from, *to], width, image.width(), image.height())
        else {
            continue;
        };
        for y in segment_bounds.y..segment_bounds.y + segment_bounds.height {
            for x in segment_bounds.x..segment_bounds.x + segment_bounds.width {
                let distance = distance_to_segment(x as f32 + 0.5, y as f32 + 0.5, *from, *to);
                let value = ((width / 2.0 + 0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u8;
                let index =
                    (y - bounds.y) as usize * bounds.width as usize + (x - bounds.x) as usize;
                coverage[index] = coverage[index].max(value);
            }
        }
    }
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let index = (y - bounds.y) as usize * bounds.width as usize + (x - bounds.x) as usize;
            if coverage[index] > 0 {
                blend(
                    image.get_pixel_mut(x, y),
                    colour,
                    coverage[index] as f32 / 255.0,
                );
            }
        }
    }
}

/// A generated local sample. It does not depict or contain real user data.
pub fn demo_image() -> RgbaImage {
    let mut image = RgbaImage::from_pixel(1120, 700, Rgba([19, 24, 34, 255]));
    fill_rect(
        &mut image,
        Rect {
            x: 0,
            y: 0,
            width: 1120,
            height: 42,
        },
        Rgba([31, 37, 49, 255]),
    );
    for (x, colour) in [
        (22, [238, 110, 104, 255]),
        (44, [238, 194, 104, 255]),
        (66, [114, 193, 150, 255]),
    ] {
        for y in 16..26 {
            for dx in 0..10 {
                if (dx as f32 - 4.5).hypot(y as f32 - 20.5) <= 5.0 {
                    image.put_pixel(x + dx, y, Rgba(colour));
                }
            }
        }
    }
    fill_rect(
        &mut image,
        Rect {
            x: 0,
            y: 42,
            width: 202,
            height: 658,
        },
        Rgba([24, 30, 41, 255]),
    );
    fill_rect(
        &mut image,
        Rect {
            x: 25,
            y: 72,
            width: 30,
            height: 30,
        },
        Rgba([108, 220, 202, 255]),
    );
    fill_rect(
        &mut image,
        Rect {
            x: 68,
            y: 79,
            width: 96,
            height: 12,
        },
        Rgba([212, 220, 231, 255]),
    );
    for row in 0..6 {
        let y = 147 + row * 48;
        if row == 1 {
            fill_rect(
                &mut image,
                Rect {
                    x: 14,
                    y: y - 12,
                    width: 173,
                    height: 38,
                },
                Rgba([38, 59, 67, 255]),
            );
        }
        fill_rect(
            &mut image,
            Rect {
                x: 28,
                y,
                width: 12,
                height: 12,
            },
            Rgba(if row == 1 {
                [108, 220, 202, 255]
            } else {
                [100, 115, 133, 255]
            }),
        );
        fill_rect(
            &mut image,
            Rect {
                x: 55,
                y: y + 2,
                width: 76 + row * 5,
                height: 8,
            },
            Rgba([127, 140, 157, 255]),
        );
    }
    fill_rect(
        &mut image,
        Rect {
            x: 238,
            y: 79,
            width: 270,
            height: 19,
        },
        Rgba([224, 230, 239, 255]),
    );
    fill_rect(
        &mut image,
        Rect {
            x: 238,
            y: 113,
            width: 360,
            height: 8,
        },
        Rgba([112, 127, 147, 255]),
    );
    fill_rect(
        &mut image,
        Rect {
            x: 952,
            y: 78,
            width: 130,
            height: 33,
        },
        Rgba([71, 167, 156, 255]),
    );
    for card in 0..3 {
        let x = 238 + card * 288;
        fill_rect(
            &mut image,
            Rect {
                x,
                y: 154,
                width: 268,
                height: 111,
            },
            Rgba([30, 37, 50, 255]),
        );
        fill_rect(
            &mut image,
            Rect {
                x: x + 20,
                y: 174,
                width: 102,
                height: 8,
            },
            Rgba([122, 137, 157, 255]),
        );
        fill_rect(
            &mut image,
            Rect {
                x: x + 20,
                y: 199,
                width: 117,
                height: 23,
            },
            Rgba([222, 231, 239, 255]),
        );
        fill_rect(
            &mut image,
            Rect {
                x: x + 20,
                y: 237,
                width: 75,
                height: 6,
            },
            Rgba([108, 220, 202, 255]),
        );
    }
    fill_rect(
        &mut image,
        Rect {
            x: 238,
            y: 287,
            width: 844,
            height: 270,
        },
        Rgba([28, 35, 48, 255]),
    );
    fill_rect(
        &mut image,
        Rect {
            x: 260,
            y: 311,
            width: 145,
            height: 10,
        },
        Rgba([196, 207, 222, 255]),
    );
    for row in 0..4 {
        fill_rect(
            &mut image,
            Rect {
                x: 266,
                y: 354 + row * 48,
                width: 786,
                height: 1,
            },
            Rgba([45, 54, 70, 255]),
        );
    }
    let values = [
        477., 482., 461., 467., 437., 450., 408., 418., 389., 400., 370., 348.,
    ];
    for index in 1..values.len() {
        // Draw the chart directly so the sample needs no font or image assets.
        let from = Point {
            x: 280. + (index - 1) as f32 * 68.,
            y: values[index - 1],
        };
        let to = Point {
            x: 280. + index as f32 * 68.,
            y: values[index],
        };
        for y in (from.y.min(to.y) as u32).saturating_sub(3)..(from.y.max(to.y) as u32 + 4) {
            for x in from.x as u32..to.x as u32 + 2 {
                let coverage = (2.5
                    - distance_to_segment(x as f32 + 0.5, y as f32 + 0.5, from, to))
                .clamp(0.0, 1.0);
                blend(image.get_pixel_mut(x, y), [108, 220, 202, 255], coverage);
            }
        }
    }
    for row in 0..3 {
        let y = 586 + row * 33;
        fill_rect(
            &mut image,
            Rect {
                x: 242,
                y,
                width: 21,
                height: 21,
            },
            Rgba([53, 76, 87, 255]),
        );
        fill_rect(
            &mut image,
            Rect {
                x: 279,
                y: y + 5,
                width: 215 - row * 21,
                height: 7,
            },
            Rgba([126, 141, 161, 255]),
        );
        fill_rect(
            &mut image,
            Rect {
                x: 759,
                y: y + 5,
                width: 91,
                height: 7,
            },
            Rgba([96, 114, 136, 255]),
        );
        fill_rect(
            &mut image,
            Rect {
                x: 967,
                y: y + 3,
                width: 92,
                height: 14,
            },
            Rgba([48, 91, 86, 255]),
        );
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare_document() -> Document {
        Document {
            source: RgbaImage::from_fn(12, 10, |x, y| {
                Rgba([x as u8 * 17, y as u8 * 20, 84, (x * 19) as u8])
            }),
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

    #[test]
    fn bare_export_preserves_every_channel() {
        let doc = bare_document();
        assert_eq!(render(&doc).unwrap(), doc.source);
    }

    #[test]
    fn crop_uses_original_source_coordinates() {
        let mut doc = bare_document();
        doc.crop = Some(Rect {
            x: 3,
            y: 2,
            width: 5,
            height: 4,
        });
        let result = render(&doc).unwrap();
        assert_eq!(result.dimensions(), (5, 4));
        assert_eq!(result.get_pixel(0, 0), doc.source.get_pixel(3, 2));
        assert_eq!(result.get_pixel(4, 3), doc.source.get_pixel(7, 5));
    }

    #[test]
    fn invalid_crop_and_padding_are_rejected_without_allocation() {
        let mut doc = bare_document();
        for rect in [
            Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
            Rect {
                x: 11,
                y: 0,
                width: 2,
                height: 1,
            },
            Rect {
                x: u32::MAX,
                y: 0,
                width: 2,
                height: 1,
            },
        ] {
            doc.crop = Some(rect);
            assert!(dimensions(&doc).is_err());
        }
        doc.crop = None;
        doc.style.padding = u32::MAX;
        assert!(dimensions(&doc).is_err());
        assert!(check_dimensions(8_001, 8_000).is_err());
        assert!(check_dimensions(MAX_DIMENSION + 1, 1).is_err());
        assert!(check_dimensions(0, 1).is_err());
    }

    #[test]
    fn padding_sets_exact_output_dimensions() {
        let mut doc = bare_document();
        doc.style.padding = 7;
        assert_eq!(dimensions(&doc).unwrap(), (26, 24));
        assert_eq!(render(&doc).unwrap().dimensions(), (26, 24));
    }

    #[test]
    fn highlight_blends_rgba_instead_of_replacing_source() {
        let mut doc = bare_document();
        doc.source = RgbaImage::from_pixel(12, 10, Rgba([0, 0, 255, 128]));
        doc.annotations.push(Annotation::Highlight {
            rect: Rect {
                x: 3,
                y: 2,
                width: 4,
                height: 5,
            },
            colour: [255, 0, 0, 128],
        });
        let result = render(&doc).unwrap();
        assert_eq!(*result.get_pixel(3, 2), Rgba([170, 0, 85, 192]));
        assert_eq!(*result.get_pixel(6, 6), Rgba([170, 0, 85, 192]));
        assert_eq!(result.get_pixel(2, 2), doc.source.get_pixel(2, 2));
        assert_eq!(result.get_pixel(7, 6), doc.source.get_pixel(7, 6));
    }

    #[test]
    fn highlight_clips_large_bounds_and_zero_alpha_preserves_pixels() {
        let mut doc = bare_document();
        let rect = Rect {
            x: 10,
            y: 8,
            width: u32::MAX,
            height: u32::MAX,
        };
        doc.annotations.push(Annotation::Highlight {
            rect,
            colour: [255, 255, 0, 255],
        });
        let result = render(&doc).unwrap();
        assert_eq!(*result.get_pixel(11, 9), Rgba([255, 255, 0, 255]));
        assert_eq!(result.get_pixel(9, 8), doc.source.get_pixel(9, 8));
        doc.annotations = vec![Annotation::Highlight {
            rect,
            colour: [255, 255, 0, 0],
        }];
        assert_eq!(render(&doc).unwrap(), doc.source);
    }

    #[test]
    fn highlights_must_cover_a_source_pixel() {
        let mut doc = bare_document();
        for rect in [
            Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
            Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 0,
            },
            Rect {
                x: 12,
                y: 0,
                width: 1,
                height: 1,
            },
            Rect {
                x: u32::MAX,
                y: u32::MAX,
                width: 2,
                height: 2,
            },
        ] {
            doc.annotations = vec![Annotation::Highlight {
                rect,
                colour: [255; 4],
            }];
            assert!(render(&doc).is_err());
        }
    }

    #[test]
    fn freehand_draws_connected_segments_with_single_alpha_at_joins() {
        let mut doc = bare_document();
        doc.source = RgbaImage::from_pixel(12, 10, Rgba([0, 0, 255, 255]));
        doc.annotations.push(Annotation::Freehand {
            points: vec![
                Point { x: 1.5, y: 4.5 },
                Point { x: 6.5, y: 4.5 },
                Point { x: 10.5, y: 4.5 },
                Point { x: 6.5, y: 4.5 },
            ],
            colour: [255, 0, 0, 128],
            width: 2.0,
        });
        let result = render(&doc).unwrap();
        for x in 1..=10 {
            assert_eq!(*result.get_pixel(x, 4), Rgba([128, 0, 127, 255]));
        }
        assert_eq!(result.get_pixel(6, 1), doc.source.get_pixel(6, 1));
    }

    #[test]
    fn freehand_handles_dots_and_clips_offscreen_segments() {
        let mut doc = bare_document();
        doc.annotations.push(Annotation::Freehand {
            points: vec![Point { x: 5.5, y: 4.5 }],
            colour: [255, 0, 0, 255],
            width: 2.0,
        });
        let result = render(&doc).unwrap();
        assert_eq!(*result.get_pixel(5, 4), Rgba([255, 0, 0, 255]));
        assert_eq!(result.get_pixel(1, 1), doc.source.get_pixel(1, 1));
        doc.annotations = vec![Annotation::Freehand {
            points: vec![
                Point {
                    x: -1_000_000.0,
                    y: 4.5,
                },
                Point {
                    x: 1_000_000.0,
                    y: 4.5,
                },
            ],
            colour: [255, 0, 0, 255],
            width: 2.0,
        }];
        let result = render(&doc).unwrap();
        assert_eq!(*result.get_pixel(0, 4), Rgba([255, 0, 0, 255]));
        assert_eq!(*result.get_pixel(11, 4), Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn freehand_rejects_invalid_coordinates_widths_and_point_counts() {
        let mut doc = bare_document();
        let point = Point { x: 5.0, y: 5.0 };
        for points in [vec![], vec![point; MAX_FREEHAND_POINTS + 1]] {
            doc.annotations = vec![Annotation::Freehand {
                points,
                colour: [255; 4],
                width: 2.0,
            }];
            assert!(render(&doc).is_err());
        }
        for invalid in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1_000_001.0,
            1_000_001.0,
        ] {
            for invalid_point in [Point { x: invalid, y: 0.0 }, Point { x: 0.0, y: invalid }] {
                doc.annotations = vec![Annotation::Freehand {
                    points: vec![invalid_point],
                    colour: [255; 4],
                    width: 2.0,
                }];
                assert!(render(&doc).is_err());
            }
        }
        for width in [
            0.0,
            -1.0,
            512.01,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            doc.annotations = vec![Annotation::Freehand {
                points: vec![point],
                colour: [255; 4],
                width,
            }];
            assert!(render(&doc).is_err());
        }
        doc.annotations = vec![Annotation::Freehand {
            points: vec![point; MAX_FREEHAND_POINTS],
            colour: [255; 4],
            width: 512.0,
        }];
        assert!(validate_annotations(&doc).is_ok());
    }

    #[test]
    fn redaction_is_opaque_and_last_even_after_translucent_annotations() {
        let mut doc = bare_document();
        let rect = Rect {
            x: 2,
            y: 2,
            width: 5,
            height: 5,
        };
        doc.annotations = vec![
            Annotation::Redact { rect },
            Annotation::Rectangle {
                rect,
                colour: [255, 0, 0, 100],
                width: 3,
            },
            Annotation::Arrow {
                from: Point { x: 0., y: 4. },
                to: Point { x: 11., y: 4. },
                colour: [255, 255, 255, 255],
                width: 2.,
            },
            Annotation::Highlight {
                rect,
                colour: [255, 255, 0, 128],
            },
            Annotation::Freehand {
                points: vec![Point { x: 0., y: 4. }, Point { x: 11., y: 4. }],
                colour: [0, 255, 0, 255],
                width: 4.,
            },
        ];
        let result = render(&doc).unwrap();
        for y in 2..7 {
            for x in 2..7 {
                assert_eq!(*result.get_pixel(x, y), REDACTION_COLOUR);
            }
        }
        assert_ne!(*doc.source.get_pixel(3, 3), REDACTION_COLOUR);
    }

    #[test]
    fn redaction_coordinates_survive_crop_and_partial_bounds() {
        let mut doc = bare_document();
        doc.crop = Some(Rect {
            x: 9,
            y: 7,
            width: 3,
            height: 3,
        });
        doc.annotations = vec![Annotation::Redact {
            rect: Rect {
                x: 10,
                y: 8,
                width: u32::MAX,
                height: u32::MAX,
            },
        }];
        let result = render(&doc).unwrap();
        assert_eq!(*result.get_pixel(1, 1), REDACTION_COLOUR);
        assert_eq!(*result.get_pixel(2, 2), REDACTION_COLOUR);
        assert_eq!(result.get_pixel(0, 0), doc.source.get_pixel(9, 7));
    }

    #[test]
    fn invalid_redaction_and_non_finite_arrow_fail() {
        let mut doc = bare_document();
        doc.annotations.push(Annotation::Redact {
            rect: Rect {
                x: 100,
                y: 100,
                width: 2,
                height: 2,
            },
        });
        assert!(render(&doc).is_err());
        doc.annotations = vec![Annotation::Arrow {
            from: Point { x: f32::NAN, y: 0. },
            to: Point { x: 5., y: 5. },
            colour: [255; 4],
            width: 2.,
        }];
        assert!(render(&doc).is_err());
    }

    #[test]
    fn corners_without_padding_are_transparent_without_hidden_source_rgb() {
        let mut doc = bare_document();
        doc.style.radius = 5;
        let result = render(&doc).unwrap();
        assert_eq!(*result.get_pixel(0, 0), Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn source_over_alpha_is_correct() {
        let mut pixel = Rgba([0, 0, 255, 128]);
        blend(&mut pixel, [255, 0, 0, 128], 1.0);
        assert_eq!(pixel, Rgba([170, 0, 85, 192]));
    }

    #[test]
    fn demo_is_valid_and_fully_opaque() {
        let demo = demo_image();
        assert_eq!(demo.dimensions(), (1120, 700));
        assert!(demo.pixels().all(|p| p[3] == 255));
    }
}
