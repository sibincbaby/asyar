//! Frosted-glass backdrop for X11.
//!
//! X11 window managers like Cinnamon's Muffin offer apps no blur-behind, so
//! the launcher captures what sits behind its panel just before mapping,
//! blurs a downscaled copy and draws it under a translucent tint. The copy is
//! a still snapshot; it does not follow changes behind the open launcher.

use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, ImageBuffer};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};

use crate::x11_place::{Rect, connection};

/// Downscale factor; with the GPU's bilinear upscale this is most of the blur.
const FACTOR: usize = 8;

/// Captures `rect` (root coordinates, device pixels) and returns it blurred.
/// The launcher must still be unmapped, or it would capture itself.
pub fn capture(rect: Rect) -> Option<Arc<RenderImage>> {
    let (conn, screen) = connection()?;
    let root = conn.setup().roots[*screen].root;
    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            root,
            rect.x as i16,
            rect.y as i16,
            rect.width as u16,
            rect.height as u16,
            !0,
        )
        .ok()?
        .reply()
        .ok()?;
    let (w, h) = (rect.width as usize, rect.height as usize);
    // 24-bit TrueColor at 32 bits per pixel: B, G, R, pad.
    if reply.data.len() != w * h * 4 {
        return None;
    }
    let (sw, sh, small) = downscale_blur(&reply.data, w, h);
    let buffer = ImageBuffer::from_raw(sw as u32, sh as u32, small)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
}

/// Box-averages `FACTOR`×`FACTOR` blocks, then runs two 5-tap box blurs.
/// Returns opaque BGRA pixels, which is the byte order GPUI expects.
fn downscale_blur(bgrx: &[u8], w: usize, h: usize) -> (usize, usize, Vec<u8>) {
    let (sw, sh) = (w.div_ceil(FACTOR).max(1), h.div_ceil(FACTOR).max(1));
    let mut px = vec![[0f32; 3]; sw * sh];
    for (sy, row) in px.chunks_mut(sw).enumerate() {
        for (sx, out) in row.iter_mut().enumerate() {
            let (mut sum, mut n) = ([0f32; 3], 0f32);
            for y in sy * FACTOR..((sy + 1) * FACTOR).min(h) {
                for x in sx * FACTOR..((sx + 1) * FACTOR).min(w) {
                    let i = (y * w + x) * 4;
                    for c in 0..3 {
                        sum[c] += bgrx[i + c] as f32;
                    }
                    n += 1.;
                }
            }
            *out = sum.map(|s| s / n.max(1.));
        }
    }
    for _ in 0..2 {
        px = blur_axis(&px, sw, sh, true);
        px = blur_axis(&px, sw, sh, false);
    }
    let bytes = px
        .iter()
        .flat_map(|p| [p[0] as u8, p[1] as u8, p[2] as u8, 255])
        .collect();
    (sw, sh, bytes)
}

fn blur_axis(px: &[[f32; 3]], w: usize, h: usize, horizontal: bool) -> Vec<[f32; 3]> {
    const R: isize = 2;
    let mut out = vec![[0f32; 3]; px.len()];
    for y in 0..h {
        for x in 0..w {
            let (mut sum, mut n) = ([0f32; 3], 0f32);
            for d in -R..=R {
                let (nx, ny) = if horizontal {
                    (x as isize + d, y as isize)
                } else {
                    (x as isize, y as isize + d)
                };
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                let p = px[ny as usize * w + nx as usize];
                for c in 0..3 {
                    sum[c] += p[c];
                }
                n += 1.;
            }
            out[y * w + x] = sum.map(|s| s / n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_input_stays_uniform_and_shrinks_by_the_factor() {
        let (w, h) = (100, 41);
        let bgrx: Vec<u8> = std::iter::repeat_n([10u8, 20, 30, 0], w * h)
            .flatten()
            .collect();
        let (sw, sh, out) = downscale_blur(&bgrx, w, h);
        assert_eq!((sw, sh), (13, 6));
        assert_eq!(out.len(), sw * sh * 4);
        assert!(out.chunks(4).all(|p| p == [10, 20, 30, 255]));
    }

    #[test]
    fn blur_spreads_a_bright_spot() {
        let mut px = vec![[0f32; 3]; 25];
        px[12] = [250., 250., 250.];
        let out = blur_axis(&blur_axis(&px, 5, 5, true), 5, 5, false);
        assert!(out[12][0] < 250. && out[0][0] > 0.);
    }
}
