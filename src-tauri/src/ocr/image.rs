//! Minimal image handling for OCR (no extra crates): PNG decoding, crop, grayscale, 2× box
//! downscale, 2× Catmull-Rom upscale (the filter measured in Task 08), and PNM output, which
//! Tesseract (Leptonica) reads directly without compression cost.

use std::io::{BufRead, Seek, Write};

use super::OcrBox;

/// 8-bit image, 1 (gray) or 3 (RGB) channels, rows tightly packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("could not read the screenshot: {0}")]
    Decode(#[from] png::DecodingError),
    #[error("unsupported screenshot format")]
    Unsupported,
}

impl Image {
    pub fn rgb(width: u32, height: u32, data: Vec<u8>) -> Self {
        debug_assert_eq!(data.len(), width as usize * height as usize * 3);
        Self {
            width,
            height,
            channels: 3,
            data,
        }
    }

    /// Decodes a PNG into RGB (alpha dropped, 16-bit and palettes expanded).
    pub fn decode_png(reader: impl BufRead + Seek) -> Result<Self, ImageError> {
        let mut decoder = png::Decoder::new(reader);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info()?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or(ImageError::Unsupported)?];
        let info = reader.next_frame(&mut buf)?;
        buf.truncate(info.buffer_size());
        let (w, h) = (info.width, info.height);
        let pixels = w as usize * h as usize;
        let data = match info.color_type {
            png::ColorType::Rgb => buf,
            png::ColorType::Rgba => buf
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect(),
            png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g]).collect(),
            png::ColorType::GrayscaleAlpha => buf
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0]])
                .collect(),
            png::ColorType::Indexed => return Err(ImageError::Unsupported),
        };
        if data.len() != pixels * 3 {
            return Err(ImageError::Unsupported);
        }
        Ok(Self::rgb(w, h, data))
    }

    pub fn crop(&self, b: OcrBox) -> Image {
        let c = self.channels as usize;
        let x0 = (b.x.max(0) as u32).min(self.width);
        let y0 = (b.y.max(0) as u32).min(self.height);
        let w = (b.width.max(0) as u32).min(self.width - x0);
        let h = (b.height.max(0) as u32).min(self.height - y0);
        let mut data = Vec::with_capacity(w as usize * h as usize * c);
        for y in y0..y0 + h {
            let start = (y as usize * self.width as usize + x0 as usize) * c;
            data.extend_from_slice(&self.data[start..start + w as usize * c]);
        }
        Image {
            width: w,
            height: h,
            channels: self.channels,
            data,
        }
    }

    /// Rec. 601 luma, like common image libraries.
    pub fn gray(&self) -> Image {
        if self.channels == 1 {
            return self.clone();
        }
        let data = self
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114 + 500)
                    / 1000) as u8
            })
            .collect();
        Image {
            width: self.width,
            height: self.height,
            channels: 1,
            data,
        }
    }

    /// Halves both sides by averaging 2×2 blocks (for the fast anchor pass).
    pub fn half(&self) -> Image {
        let c = self.channels as usize;
        let (w, h) = (self.width / 2, self.height / 2);
        let stride = self.width as usize * c;
        let mut data = Vec::with_capacity(w as usize * h as usize * c);
        for y in 0..h as usize {
            let (r0, r1) = (2 * y * stride, (2 * y + 1) * stride);
            for x in 0..w as usize {
                for ch in 0..c {
                    let i = 2 * x * c + ch;
                    let sum = u32::from(self.data[r0 + i])
                        + u32::from(self.data[r0 + i + c])
                        + u32::from(self.data[r1 + i])
                        + u32::from(self.data[r1 + i + c]);
                    data.push(((sum + 2) / 4) as u8);
                }
            }
        }
        Image {
            width: w,
            height: h,
            channels: self.channels,
            data,
        }
    }

    /// Doubles both sides with a separable Catmull-Rom filter.
    pub fn double(&self) -> Image {
        let c = self.channels as usize;
        let (w, h) = (self.width as usize, self.height as usize);
        let (nw, nh) = (w * 2, h * 2);
        // Output pixel i samples source position (i + 0.5) / 2 - 0.5: weights for the two phases.
        const EVEN: [f32; 4] = [-0.0234375, 0.2265625, 0.8671875, -0.0703125]; // t = 0.75 from x-1
        const ODD: [f32; 4] = [-0.0703125, 0.8671875, 0.2265625, -0.0234375]; // t = 0.25 from x
        let taps = |i: usize, n: usize| -> ([usize; 4], [f32; 4]) {
            let (base, weights) = if i.is_multiple_of(2) {
                (i as isize / 2 - 2, EVEN)
            } else {
                (i as isize / 2 - 1, ODD)
            };
            let idx = [0, 1, 2, 3].map(|k| (base + k).clamp(0, n as isize - 1) as usize);
            (idx, weights)
        };
        // Horizontal pass into f32.
        let mut tmp = vec![0f32; nw * h * c];
        for x in 0..nw {
            let (idx, wt) = taps(x, w);
            for y in 0..h {
                for ch in 0..c {
                    let row = y * w * c;
                    tmp[(y * nw + x) * c + ch] = (0..4)
                        .map(|k| wt[k] * f32::from(self.data[row + idx[k] * c + ch]))
                        .sum();
                }
            }
        }
        // Vertical pass.
        let mut data = vec![0u8; nw * nh * c];
        for y in 0..nh {
            let (idx, wt) = taps(y, h);
            for x in 0..nw * c {
                let v: f32 = (0..4).map(|k| wt[k] * tmp[idx[k] * nw * c + x]).sum();
                data[y * nw * c + x] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
        Image {
            width: nw as u32,
            height: nh as u32,
            channels: self.channels,
            data,
        }
    }

    /// Binary PNM: P5 (gray) or P6 (RGB).
    pub fn write_pnm(&self, mut out: impl Write) -> std::io::Result<()> {
        let magic = if self.channels == 1 { "P5" } else { "P6" };
        write!(out, "{magic}\n{} {}\n255\n", self.width, self.height)?;
        out.write_all(&self.data)?;
        out.flush()
    }
}
