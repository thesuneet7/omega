use image::{imageops::FilterType, DynamicImage, Luma};

/// Simple perceptual hash implementation using DCT on a downscaled grayscale image.
pub fn compute_phash(img: &DynamicImage) -> [u8; 8] {
    // Downscale to a small fixed size and convert to luma.
    let resized = img.resize_exact(32, 32, FilterType::Lanczos3).to_luma8();

    // Convert pixel values to f32 matrix.
    let mut pixels = [0f32; 32 * 32];
    for y in 0..32 {
        for x in 0..32 {
            let Luma([v]) = resized.get_pixel(x, y);
            pixels[(y as usize) * 32 + (x as usize)] = *v as f32;
        }
    }

    // Compute a very small 8x8 DCT on the top-left corner.
    let mut dct = [0f32; 8 * 8];
    for u in 0..8 {
        for v in 0..8 {
            let mut sum = 0.0;
            for x in 0..32 {
                for y in 0..32 {
                    let pixel = pixels[(y as usize) * 32 + (x as usize)];
                    let cu = if u == 0 { (1.0 / 2.0f32).sqrt() } else { 1.0 };
                    let cv = if v == 0 { (1.0 / 2.0f32).sqrt() } else { 1.0 };
                    let theta_u = (std::f32::consts::PI * (2 * x + 1) as f32 * u as f32) / 64.0;
                    let theta_v = (std::f32::consts::PI * (2 * y + 1) as f32 * v as f32) / 64.0;
                    sum += pixel * (theta_u.cos()) * (theta_v.cos()) * cu * cv;
                }
            }
            dct[v * 8 + u] = sum;
        }
    }

    // Compute median of DCT coefficients excluding DC.
    let mut vals: Vec<f32> = dct.iter().copied().collect();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = vals[vals.len() / 2];

    // Build 64-bit hash: bit = 1 if coeff > median.
    let mut hash: [u8; 8] = [0; 8];
    for (i, coeff) in dct.iter().enumerate() {
        if *coeff > median {
            let byte = i / 8;
            let bit = 7 - (i % 8);
            hash[byte] |= 1 << bit;
        }
    }
    hash
}

/// Hamming distance between two 64-bit pHashes.
pub fn hamming_distance(a: &[u8; 8], b: &[u8; 8]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

/// Convert Hamming distance into similarity in \[0, 1\].
pub fn similarity(a: &[u8; 8], b: &[u8; 8]) -> f32 {
    let dist = hamming_distance(a, b) as f32;
    1.0 - dist / 64.0
}
