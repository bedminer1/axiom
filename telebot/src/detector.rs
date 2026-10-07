use std::{io::Cursor, path::Path};

use anyhow::{Context, Result, ensure};
use font8x8::UnicodeFonts;
use image::{ImageDecoder, ImageReader, Rgb, RgbImage, codecs::jpeg::JpegEncoder, imageops};
use ort::{session::Session, value::Tensor};

const INPUT: u32 = 640;
const OVERLAP: u32 = 128;
const MAX_SIDE: u32 = 3072;
const MAX_BOXES: usize = 100;

#[derive(Clone, Copy, Debug)]
pub struct Detection {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub confidence: f32,
}

pub struct ProcessedImage {
    pub jpeg: Vec<u8>,
    pub detections: usize,
    pub passes: usize,
}

pub struct Detector {
    session: Session,
    confidence: f32,
    iou: f32,
}

#[derive(Clone, Copy)]
struct Letterbox {
    scale_x: f32,
    scale_y: f32,
    pad_x: f32,
    pad_y: f32,
    width: u32,
    height: u32,
}

impl Detector {
    pub fn load(model: &Path, runtime: &Path, confidence: f32, iou: f32) -> Result<Self> {
        ensure!(
            model.is_file(),
            "YOLO model missing: run uv run --python 3.12 telebot/setup_model.py"
        );
        ensure!(
            runtime.is_file(),
            "ONNX Runtime missing: run telebot/setup_model.py"
        );
        ensure!(
            confidence.is_finite() && confidence > 0.0 && confidence <= 1.0,
            "YOLO_CONFIDENCE must be in (0, 1]"
        );
        ensure!(
            iou.is_finite() && iou > 0.0 && iou <= 1.0,
            "YOLO_IOU must be in (0, 1]"
        );
        ort::init_from(runtime)?.with_name("axiom-telebot").commit();
        let session = Session::builder()?
            .with_intra_threads(4)
            .map_err(|error| anyhow::anyhow!("{error}"))?
            .commit_from_file(model)
            .context("cannot load crack ONNX model")?;
        ensure!(
            session.inputs().len() == 1 && session.outputs().len() == 1,
            "expected a single-input, single-output crack detector"
        );
        Ok(Self {
            session,
            confidence,
            iou,
        })
    }

    pub fn process(&mut self, bytes: &[u8]) -> Result<ProcessedImage> {
        let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        // Apply phone orientation before predicting or drawing boxes.
        let mut decoder = reader.into_decoder()?;
        let orientation = decoder.orientation()?;
        let mut decoded = image::DynamicImage::from_decoder(decoder)?;
        decoded.apply_orientation(orientation);
        let mut image = decoded.to_rgb8();
        let (w, h) = image.dimensions();
        ensure!(w > 0 && h > 0, "empty image");
        ensure!(
            w.max(h) as f64 / w.min(h) as f64 <= 20.0,
            "image is too panoramic for a Telegram photo reply"
        );
        if w.max(h) > MAX_SIDE {
            let scale = MAX_SIDE as f64 / w.max(h) as f64;
            image = imageops::resize(
                &image,
                (w as f64 * scale).round().max(1.0) as u32,
                (h as f64 * scale).round().max(1.0) as u32,
                imageops::FilterType::Triangle,
            );
        }
        // A whole-image pass keeps large cracks intact; overlapping native-resolution
        // crops prevent small, distant cracks being lost in a full-image resize.
        let mut boxes = self.predict(&image)?;
        let mut passes = 1;
        if image.width() > INPUT || image.height() > INPUT {
            for y in tile_starts(image.height()) {
                for x in tile_starts(image.width()) {
                    let tile = imageops::crop_imm(
                        &image,
                        x,
                        y,
                        INPUT.min(image.width()),
                        INPUT.min(image.height()),
                    )
                    .to_image();
                    for mut detection in self.predict(&tile)? {
                        detection.x1 += x as f32;
                        detection.x2 += x as f32;
                        detection.y1 += y as f32;
                        detection.y2 += y as f32;
                        boxes.push(detection);
                    }
                    passes += 1;
                }
            }
        }
        let boxes = nms(boxes, self.iou);
        for detection in &boxes {
            draw_detection(&mut image, *detection);
        }
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 88).encode_image(&image)?;
        ensure!(
            jpeg.len() < 10 * 1024 * 1024,
            "annotated image exceeds Telegram's photo limit"
        );
        Ok(ProcessedImage {
            jpeg,
            detections: boxes.len(),
            passes,
        })
    }

    fn predict(&mut self, image: &RgbImage) -> Result<Vec<Detection>> {
        let (values, transform) = letterbox(image);
        let input = Tensor::from_array((
            [1usize, 3, INPUT as usize, INPUT as usize],
            values.into_boxed_slice(),
        ))?;
        let outputs = self.session.run(ort::inputs![input])?;
        let (shape, values) = outputs[0].try_extract_tensor::<f32>()?;
        // Ultralytics raw, single-class YOLOv8 detection export: [1, 5, anchors].
        // Reject other layouts instead of silently decoding wrong classes/models.
        ensure!(
            shape.len() == 3 && shape[0] == 1 && shape[1] == 5,
            "expected single-class YOLO output [1, 5, N], got {shape:?}"
        );
        let anchors = shape[2] as usize;
        let mut boxes = Vec::new();
        for index in 0..anchors {
            let confidence = values[4 * anchors + index];
            if !confidence.is_finite() || confidence < self.confidence {
                continue;
            }
            let cx = values[index];
            let cy = values[anchors + index];
            let width = values[2 * anchors + index];
            let height = values[3 * anchors + index];
            if let Some(detection) = transform.decode(cx, cy, width, height, confidence) {
                boxes.push(detection);
            }
        }
        Ok(nms(boxes, self.iou))
    }
}

fn letterbox(image: &RgbImage) -> (Vec<f32>, Letterbox) {
    let (width, height) = image.dimensions();
    let scale = (INPUT as f64 / width as f64).min(INPUT as f64 / height as f64);
    let rw = (width as f64 * scale).round().clamp(1.0, INPUT as f64) as u32;
    let rh = (height as f64 * scale).round().clamp(1.0, INPUT as f64) as u32;
    let pad_x = (INPUT - rw) / 2;
    let pad_y = (INPUT - rh) / 2;
    let resized = imageops::resize(image, rw, rh, imageops::FilterType::Triangle);
    let plane = (INPUT * INPUT) as usize;
    let mut values = vec![114.0 / 255.0; plane * 3];
    for (x, y, pixel) in resized.enumerate_pixels() {
        let offset = ((y + pad_y) * INPUT + x + pad_x) as usize;
        for channel in 0..3 {
            values[channel * plane + offset] = pixel[channel] as f32 / 255.0;
        }
    }
    (
        values,
        Letterbox {
            scale_x: rw as f32 / width as f32,
            scale_y: rh as f32 / height as f32,
            pad_x: pad_x as f32,
            pad_y: pad_y as f32,
            width,
            height,
        },
    )
}

impl Letterbox {
    fn decode(
        self,
        cx: f32,
        cy: f32,
        width: f32,
        height: f32,
        confidence: f32,
    ) -> Option<Detection> {
        if ![cx, cy, width, height].iter().all(|v| v.is_finite()) || width <= 0.0 || height <= 0.0 {
            return None;
        }
        let detection = Detection {
            x1: ((cx - width / 2.0 - self.pad_x) / self.scale_x).clamp(0.0, self.width as f32),
            y1: ((cy - height / 2.0 - self.pad_y) / self.scale_y).clamp(0.0, self.height as f32),
            x2: ((cx + width / 2.0 - self.pad_x) / self.scale_x).clamp(0.0, self.width as f32),
            y2: ((cy + height / 2.0 - self.pad_y) / self.scale_y).clamp(0.0, self.height as f32),
            confidence,
        };
        (detection.x2 > detection.x1 && detection.y2 > detection.y1).then_some(detection)
    }
}

fn tile_starts(length: u32) -> Vec<u32> {
    if length <= INPUT {
        return vec![0];
    }
    let last = length - INPUT;
    let mut starts: Vec<u32> = (0..last).step_by((INPUT - OVERLAP) as usize).collect();
    starts.push(last);
    starts
}

fn nms(mut boxes: Vec<Detection>, threshold: f32) -> Vec<Detection> {
    boxes.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let mut kept: Vec<Detection> = Vec::new();
    for candidate in boxes {
        if kept
            .iter()
            .all(|other| intersection_over_union(candidate, *other) <= threshold)
        {
            kept.push(candidate);
            if kept.len() >= MAX_BOXES {
                break;
            }
        }
    }
    kept
}

fn intersection_over_union(a: Detection, b: Detection) -> f32 {
    let intersection =
        (a.x2.min(b.x2) - a.x1.max(b.x1)).max(0.0) * (a.y2.min(b.y2) - a.y1.max(b.y1)).max(0.0);
    let union = (a.x2 - a.x1) * (a.y2 - a.y1) + (b.x2 - b.x1) * (b.y2 - b.y1) - intersection;
    if union > 0.0 {
        intersection / union
    } else {
        0.0
    }
}

fn draw_detection(image: &mut RgbImage, detection: Detection) {
    let color = Rgb([255, 115, 40]);
    let x1 = (detection.x1.floor() as u32).min(image.width() - 1);
    let y1 = (detection.y1.floor() as u32).min(image.height() - 1);
    let x2 = (detection.x2.ceil() as u32).min(image.width() - 1);
    let y2 = (detection.y2.ceil() as u32).min(image.height() - 1);
    for line in 0..3 {
        for x in x1..=x2 {
            image.put_pixel(x, (y1 + line).min(y2), color);
            image.put_pixel(x, y2.saturating_sub(line).max(y1), color);
        }
        for y in y1..=y2 {
            image.put_pixel((x1 + line).min(x2), y, color);
            image.put_pixel(x2.saturating_sub(line).max(x1), y, color);
        }
    }
    let text = format!("crack {:.0}%", detection.confidence * 100.0);
    let scale = if image.width().max(image.height()) > 1600 {
        3
    } else {
        2
    };
    let label_w = text.len() as u32 * 8 * scale + 6;
    let label_h = 8 * scale + 6;
    let lx = x1.min(image.width().saturating_sub(label_w));
    let ly = if y1 >= label_h { y1 - label_h } else { y1 };
    for y in ly..(ly + label_h).min(image.height()) {
        for x in lx..(lx + label_w).min(image.width()) {
            image.put_pixel(x, y, color);
        }
    }
    for (index, character) in text.chars().enumerate() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(character) {
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..8 {
                    if bits & (1 << column) == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = lx + 3 + (index as u32 * 8 + column) * scale + dx;
                            let y = ly + 3 + row as u32 * scale + dy;
                            if x < image.width() && y < image.height() {
                                image.put_pixel(x, y, Rgb([20, 20, 20]));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_reverses_padding_and_scale_for_portrait_and_landscape() {
        for (width, height) in [(1280, 640), (640, 1280), (853, 477)] {
            let (_, t) = letterbox(&RgbImage::new(width, height));
            let decoded = t
                .decode(
                    width as f32 * 0.5 * t.scale_x + t.pad_x,
                    height as f32 * 0.5 * t.scale_y + t.pad_y,
                    width as f32 * 0.5 * t.scale_x,
                    height as f32 * 0.5 * t.scale_y,
                    0.9,
                )
                .unwrap();
            assert!((decoded.x1 - width as f32 * 0.25).abs() < 0.01);
            assert!((decoded.y2 - height as f32 * 0.75).abs() < 0.01);
            assert!(t.decode(0.0, 0.0, -1.0, 4.0, 0.9).is_none());
            assert!(t.decode(f32::NAN, 1.0, 4.0, 4.0, 0.9).is_none());
        }
    }

    #[test]
    fn tiles_cover_the_edges_without_gaps_or_duplicates() {
        for length in [1, 640, 641, 1024, 1920, MAX_SIDE] {
            let starts = tile_starts(length);
            assert_eq!(starts[0], 0);
            assert_eq!(starts.last().copied().unwrap() + INPUT.min(length), length);
            for pair in starts.windows(2) {
                assert!(pair[1] > pair[0] && pair[1] - pair[0] <= INPUT);
            }
        }
    }

    #[test]
    fn nms_removes_overlap_but_keeps_separate_cracks() {
        let a = Detection {
            x1: 10.0,
            y1: 10.0,
            x2: 30.0,
            y2: 30.0,
            confidence: 0.9,
        };
        let duplicate = Detection {
            confidence: 0.6,
            ..a
        };
        let separate = Detection {
            x1: 100.0,
            x2: 120.0,
            ..a
        };
        let result = nms(vec![duplicate, separate, a], 0.45);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].confidence, 0.9);
    }
}
