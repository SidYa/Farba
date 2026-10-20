//! Colour type and the "normal" blend used by painting and compositing.

/// Straight (non-premultiplied) sRGB colour, 0..=255 per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const TRANSPARENT: Rgba8 = Rgba8::new(0, 0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// Composites straight pixel `src` over straight pixel `dst` ("normal" mode), with `src`'s
/// alpha scaled by `opacity` (0..=1). The maths is premultiplied; the result is straight.
#[inline]
pub fn blend_over(dst: &mut [u8; 4], src: &[u8; 4], opacity: f32) {
    let sa = src[3] as f32 * (opacity * (1.0 / 255.0));
    if sa <= 0.0 {
        return;
    }
    if sa >= 1.0 {
        *dst = *src;
        return;
    }
    let wd = dst[3] as f32 * (1.0 / 255.0) * (1.0 - sa);
    let out_a = sa + wd;
    let inv = 1.0 / out_a;
    for c in 0..3 {
        dst[c] = ((src[c] as f32 * sa + dst[c] as f32 * wd) * inv + 0.5) as u8;
    }
    dst[3] = (out_a * 255.0 + 0.5) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_over_basics() {
        let mut d = [0, 0, 255, 255];
        blend_over(&mut d, &[255, 0, 0, 255], 0.5);
        assert_eq!(d, [128, 0, 128, 255]);

        let mut d = [0, 0, 0, 0];
        blend_over(&mut d, &[10, 20, 30, 128], 1.0);
        assert_eq!(d, [10, 20, 30, 128]);

        let mut d = [1, 2, 3, 4];
        blend_over(&mut d, &[255, 255, 255, 255], 0.0);
        assert_eq!(d, [1, 2, 3, 4]);
    }
}
