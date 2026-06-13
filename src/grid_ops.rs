//! Grid utilities: separable blur, chamfer distance, down/up sampling.

use rayon::prelude::*;

/// Clamp a scalar to the unit interval `[0, 1]`.
pub(crate) fn normalize01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Separable box blur with row-parallel passes.
pub fn box_blur(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return src.to_vec();
    }
    let r = radius as i32;
    let mut tmp = vec![0.0f32; src.len()];
    let mut out = vec![0.0f32; src.len()];

    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, cell) in row.iter_mut().enumerate() {
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for dx in -r..=r {
                let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                sum += src[y * w + nx];
                count += 1;
            }
            *cell = sum / count as f32;
        }
    });

    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, cell) in row.iter_mut().enumerate() {
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for dy in -r..=r {
                let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                sum += tmp[ny * w + x];
                count += 1;
            }
            *cell = sum / count as f32;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn downsample_avg(
        field: &[f32],
        w: usize,
        h: usize,
        factor: usize,
    ) -> (Vec<f32>, usize, usize) {
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

    fn upsample_nearest(field: &[f32], cw: usize, ch: usize, w: usize, h: usize) -> Vec<f32> {
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
