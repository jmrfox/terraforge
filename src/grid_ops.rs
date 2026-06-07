//! Grid utilities: separable blur, chamfer distance, down/up sampling.

use rayon::prelude::*;

/// Separable box blur with row-parallel passes.
pub fn box_blur(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return src.to_vec();
    }
    let r = radius as i32;
    let mut tmp = vec![0.0f32; src.len()];
    let mut out = vec![0.0f32; src.len()];

    tmp.par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let mut sum = 0.0f32;
                let mut count = 0u32;
                for dx in -r..=r {
                    let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    sum += src[y * w + nx];
                    count += 1;
                }
                row[x] = sum / count as f32;
            }
        });

    out.par_chunks_mut(w)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let mut sum = 0.0f32;
                let mut count = 0u32;
                for dy in -r..=r {
                    let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    sum += tmp[ny * w + x];
                    count += 1;
                }
                row[x] = sum / count as f32;
            }
        });

    out
}

fn relax_dist(dist: &mut [u32], idx: usize, neighbor: usize) {
    let nd = dist[neighbor];
    if nd != u32::MAX {
        dist[idx] = dist[idx].min(nd.saturating_add(1));
    }
}

/// Chamfer (two-pass) distance from seed cells. Approximate geodesic, fully deterministic.
pub fn chamfer_distance<F>(w: usize, h: usize, is_seed: F) -> Vec<u32>
where
    F: Fn(usize) -> bool + Sync,
{
    let len = w * h;
    let mut dist: Vec<u32> = (0..len)
        .map(|idx| if is_seed(idx) { 0 } else { u32::MAX })
        .collect();

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            if x > 0 {
                relax_dist(&mut dist, idx, idx - 1);
            }
            if y > 0 {
                relax_dist(&mut dist, idx, idx - w);
            }
        }
    }

    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let idx = y * w + x;
            if x + 1 < w {
                relax_dist(&mut dist, idx, idx + 1);
            }
            if y + 1 < h {
                relax_dist(&mut dist, idx, idx + w);
            }
        }
    }

    dist
}

pub fn chamfer_distance_water(w: usize, h: usize, water: &[bool]) -> Vec<u32> {
    chamfer_distance(w, h, |idx| water[idx])
}

#[allow(dead_code)]
pub fn chamfer_distance_below(w: usize, h: usize, field: &[f32], threshold: f32) -> Vec<u32> {
    chamfer_distance(w, h, |idx| field[idx] < threshold)
}

#[allow(dead_code)]
pub fn chamfer_distance_above(w: usize, h: usize, field: &[f32], threshold: f32) -> Vec<u32> {
    chamfer_distance(w, h, |idx| field[idx] >= threshold)
}

/// Block-average downsample by `factor` (deterministic).
pub fn downsample_avg(field: &[f32], w: usize, h: usize, factor: usize) -> (Vec<f32>, usize, usize) {
    let factor = factor.max(1);
    let cw = (w / factor).max(1);
    let ch = (h / factor).max(1);
    let mut out = vec![0.0f32; cw * ch];

    for cy in 0..ch {
        for cx in 0..cw {
            let mut sum = 0.0f32;
            let mut count = 0u32;
            let y0 = cy * factor;
            let x0 = cx * factor;
            for dy in 0..factor {
                let y = y0 + dy;
                if y >= h {
                    break;
                }
                for dx in 0..factor {
                    let x = x0 + dx;
                    if x >= w {
                        break;
                    }
                    sum += field[y * w + x];
                    count += 1;
                }
            }
            out[cy * cw + cx] = sum / count.max(1) as f32;
        }
    }

    (out, cw, ch)
}

/// Nearest-neighbor upsample from coarse to full resolution.
#[allow(dead_code)]
pub fn upsample_nearest(field: &[f32], cw: usize, ch: usize, w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        let cy = (y * ch / h).min(ch.saturating_sub(1));
        for x in 0..w {
            let cx = (x * cw / w).min(cw.saturating_sub(1));
            out[y * w + x] = field[cy * cw + cx];
        }
    }
    out
}

/// Bilinear upsample from coarse to full resolution.
pub fn upsample_bilinear(field: &[f32], cw: usize, ch: usize, w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    if cw == 0 || ch == 0 {
        return out;
    }

    for y in 0..h {
        let gy = (y as f32 + 0.5) * ch as f32 / h as f32 - 0.5;
        let y0 = gy.floor().clamp(0.0, (ch - 1) as f32) as usize;
        let y1 = (y0 + 1).min(ch - 1);
        let ty = (gy - y0 as f32).clamp(0.0, 1.0);

        for x in 0..w {
            let gx = (x as f32 + 0.5) * cw as f32 / w as f32 - 0.5;
            let x0 = gx.floor().clamp(0.0, (cw - 1) as f32) as usize;
            let x1 = (x0 + 1).min(cw - 1);
            let tx = (gx - x0 as f32).clamp(0.0, 1.0);

            let v00 = field[y0 * cw + x0];
            let v10 = field[y0 * cw + x1];
            let v01 = field[y1 * cw + x0];
            let v11 = field[y1 * cw + x1];
            let top = v00 * (1.0 - tx) + v10 * tx;
            let bot = v01 * (1.0 - tx) + v11 * tx;
            out[y * w + x] = top * (1.0 - ty) + bot * ty;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chamfer_water_matches_bfs_on_tiny_grid() {
        let w = 8usize;
        let h = 8usize;
        let mut water = vec![false; w * h];
        for x in 0..w {
            water[x] = true;
            water[(h - 1) * w + x] = true;
        }
        let chamfer = chamfer_distance_water(w, h, &water);
        for idx in 0..w * h {
            if water[idx] {
                assert_eq!(chamfer[idx], 0);
            } else {
                assert!(chamfer[idx] < u32::MAX);
            }
        }
    }

    #[test]
    fn downsample_upsample_nearest_roundtrip_size() {
        let w = 64usize;
        let h = 64usize;
        let field: Vec<f32> = (0..w * h).map(|i| i as f32).collect();
        let (coarse, cw, ch) = downsample_avg(&field, w, h, 4);
        assert_eq!(coarse.len(), cw * ch);
        let up = upsample_nearest(&coarse, cw, ch, w, h);
        assert_eq!(up.len(), w * h);
    }
}
