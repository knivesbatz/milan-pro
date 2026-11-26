//! A4: frames between Milan's shared memory (RGBA, premultiplied, bottom row first: the OFX convention of `Render`) and
//! the 8-bit worlds an AE-format effect gets (A, R, G, B from the low byte, G: PF_EffectWorld page; top row first and
//! premultiplied alpha, R: UNVERIFIED until an open-source plug-in runs, `docs/compat/aex-provenance.md`). The worlds
//! are separate buffers with their own row padding; shortcut: one copy each way.

/// Milan's `w` x `h` RGBA rows (bottom first) as ARGB rows (top first) of `rb` bytes.
pub fn to_argb(src: &[u8], w: usize, h: usize, rb: usize) -> Vec<u8> {
    let mut out = vec![0u8; rb * h];
    for y in 0..h {
        let s = &src[(h - 1 - y) * w * 4..][..w * 4];
        let d = &mut out[y * rb..][..w * 4];
        for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
            d.copy_from_slice(&[s[3], s[0], s[1], s[2]]);
        }
    }
    out
}

/// ARGB rows (top first, `rb` bytes each) back into Milan's RGBA rows (bottom first), each colour kept within its alpha
/// (a valid premultiplied pixel).
pub fn from_argb(src: &[u8], w: usize, h: usize, rb: usize, out: &mut [u8]) {
    for y in 0..h {
        let s = &src[y * rb..][..w * 4];
        let d = &mut out[(h - 1 - y) * w * 4..][..w * 4];
        for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
            let a = s[0];
            d.copy_from_slice(&[s[1].min(a), s[2].min(a), s[3].min(a), a]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bottom row in Milan's memory is the last row of the world; channels move to A, R, G, B and back; rows keep
    /// their padding; colours above alpha are clamped on the way back.
    #[test]
    fn rows_flip_and_channels_reorder() {
        // 2 x 2: bottom row (first in memory) red and green, top row blue and white.
        let milan = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let rb = 16;
        let w = to_argb(&milan, 2, 2, rb);
        assert_eq!(&w[0..8], &[255, 0, 0, 255, 255, 255, 255, 255], "top row first: blue, white");
        assert_eq!(&w[16..24], &[255, 255, 0, 0, 255, 0, 255, 0], "then red, green");
        let mut back = [0u8; 16];
        from_argb(&w, 2, 2, rb, &mut back);
        assert_eq!(back, milan);
        let mut one = [0u8; 4];
        from_argb(&[100, 200, 50, 255], 1, 1, 4, &mut one);
        assert_eq!(one, [100, 50, 100, 100]);
    }
}
