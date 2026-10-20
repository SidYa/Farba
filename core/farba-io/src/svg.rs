//! SVG / SVGZ, rendered with `resvg` (MIT / Apache-2.0; `tiny-skia` BSD-3-Clause) at the
//! size the user picks (GIMP's "Render Scalable Vector Graphics" dialog). Text uses the
//! installed fonts, loaded once on first use.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, fontdb};

use crate::stack::MAX_SIDE;

fn fonts() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn parse(path: &Path) -> Result<usvg::Tree, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut options = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        ..Default::default()
    };
    options.fontdb = fonts();
    // `from_data` also unpacks .svgz (gzip).
    usvg::Tree::from_data(&data, &options).map_err(|e| format!("Not a valid SVG file: {e}"))
}

/// The document size in CSS pixels (96 per inch).
pub fn size(path: &Path) -> Result<(f64, f64), String> {
    let s = parse(path)?.size();
    Ok((s.width() as f64, s.height() as f64))
}

/// The intrinsic size rounded to whole pixels (at least 1).
pub fn default_size(path: &Path) -> Result<(u32, u32), String> {
    let (w, h) = size(path)?;
    Ok(((w.round() as u32).max(1), (h.round() as u32).max(1)))
}

/// Renders the SVG stretched to `width` x `height` as straight RGBA8.
pub fn render(path: &Path, width: u32, height: u32) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("Unsupported image size {width} x {height}"));
    }
    let tree = parse(path)?;
    let mut pixmap = Pixmap::new(width, height).ok_or("The image is too large")?;
    let s = tree.size();
    let transform = Transform::from_scale(width as f32 / s.width(), height as f32 / s.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // tiny-skia stores premultiplied RGBA.
    let mut rgba = pixmap.take();
    for p in rgba.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_at_the_chosen_size() {
        let path = std::env::temp_dir().join(format!("farba-svg-{}.svg", std::process::id()));
        std::fs::write(
            &path,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
                 <rect x="0" y="0" width="20" height="20" fill="#ff0000"/>
                 <rect x="20" y="0" width="20" height="20" fill="#0000ff" fill-opacity="0.5"/>
               </svg>"##,
        )
        .unwrap();
        assert_eq!(default_size(&path).unwrap(), (40, 20));
        let px = render(&path, 80, 40).unwrap();
        assert_eq!(px.len(), 80 * 40 * 4);
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
        let i = (20 * 80 + 60) * 4;
        assert_eq!(&px[i..i + 4], &[0, 0, 255, 128]);
        std::fs::remove_file(&path).unwrap();
    }
}
