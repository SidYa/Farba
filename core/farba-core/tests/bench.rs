//! Timings of the heavy operations on a 24 MP photo-like layer (ignored; run with
//! `cargo test --release -p farba-core --test bench -- --ignored --nocapture`, and
//! `FARBA_THREADS=1` for one core).

use std::time::Instant;

use farba_core::effects::Effect;
use farba_core::filters::{self, Filter};
use farba_core::projective::{self, Matrix};
use farba_core::surface::Surface;
use farba_core::transform::{self, Interpolation};

fn photo(w: u32, h: u32) -> Vec<u8> {
    let mut s = 1u32;
    (0..w * h)
        .flat_map(|i| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let (x, y) = (i % w, i / w);
            let n = (s & 31) as u8;
            [
                ((x / 7 + y / 11) % 200) as u8 + n,
                ((x * y / 997) % 200) as u8 + n,
                (y / 16 % 200) as u8 + n,
                255,
            ]
        })
        .collect()
}

fn time(name: &str, f: impl FnOnce()) {
    let t = Instant::now();
    f();
    eprintln!("{name:<28} {:>7.0} ms", t.elapsed().as_secs_f64() * 1000.0);
}

#[test]
#[ignore]
fn heavy_operations_24mp() {
    let (w, h) = (6000u32, 4000u32);
    let rgba = photo(w, h);
    let surface = Surface::from_rgba(w, h, &rgba);
    eprintln!("threads: {}", farba_core::par::threads());
    let run = |name: &str, filter: Filter| {
        let mut s = surface.clone();
        time(name, || {
            filters::apply(&mut s, (0, 0), (w, h), None, filter)
        });
    };
    run(
        "exposure",
        Filter::Exposure {
            exposure: 1.0,
            black: 0.0,
        },
    );
    run("saturation", Filter::Saturation { scale: 1.5 });
    run(
        "black & white",
        Filter::Effect(Effect::BlackWhite {
            red: 30.0,
            green: 59.0,
            blue: 11.0,
            contrast: 0.0,
            sepia: 0.0,
        }),
    );
    run(
        "enhance colors",
        Filter::Effect(Effect::EnhanceColors {
            auto: 100.0,
            vibrance: 30.0,
        }),
    );
    run(
        "retouch",
        Filter::Effect(Effect::Retouch {
            radius: 8.0,
            smoothness: 40.0,
            amount: 100.0,
        }),
    );
    run(
        "sharpen",
        Filter::Effect(Effect::Sharpen {
            radius: 2.0,
            amount: 100.0,
            threshold: 0.0,
        }),
    );
    run(
        "gaussian blur 5",
        Filter::Effect(Effect::GaussianBlur { sigma: 5.0 }),
    );
    run(
        "vignette",
        Filter::Effect(Effect::Vignette {
            amount: -50.0,
            size: 50.0,
            softness: 50.0,
        }),
    );
    time("scale 50% lanczos", || {
        transform::resample(&rgba, 6000, 4000, 4, 3000, 2000, Interpolation::Lanczos);
    });
    time("scale 200% cubic", || {
        transform::resample(&rgba, 6000, 4000, 4, 8485, 5657, Interpolation::Cubic);
    });
    let m = Matrix::rotate_around(17f64.to_radians(), 3000.0, 2000.0);
    for interp in [
        Interpolation::Linear,
        Interpolation::Cubic,
        Interpolation::Lanczos,
    ] {
        time(&format!("rotate 17° {interp:?}"), || {
            projective::render(&rgba, 6000, 4000, 4, &m, interp).unwrap();
        });
    }
}

/// The parallel code paths give exactly the single-threaded results.
#[test]
fn parallel_matches_one_thread() {
    use farba_core::par::with_threads;
    use farba_core::selection::{Mask, SelectMode, rectangle};
    let (w, h) = (300u32, 700u32);
    let rgba: Vec<u8> = photo(w, h)
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .flat_map(|(i, p)| [p[0], p[1], p[2], (i % 251) as u8])
        .collect();
    let both = |f: &dyn Fn() -> Vec<u8>| {
        let one = with_threads(1, f);
        let all = with_threads(8, f);
        assert!(one == all, "parallel result differs");
    };
    both(&|| transform::resample(&rgba, 300, 700, 4, 170, 333, Interpolation::Lanczos));
    both(&|| transform::resample(&rgba, 300, 700, 4, 640, 1500, Interpolation::Cubic));
    let m = Matrix::rotate_around(0.3, 150.0, 350.0);
    both(&|| {
        projective::render(&rgba, 300, 700, 4, &m, Interpolation::Cubic)
            .unwrap()
            .data
    });
    let mut mask = Mask::new(w, h);
    mask.combine(
        &rectangle(20.0, 50.0, 200.0, 500.0, 10.0, true, w, h),
        SelectMode::Replace,
    );
    let surface = Surface::from_rgba(w, h, &rgba);
    for filter in [
        Filter::Saturation { scale: 0.4 },
        Filter::Effect(Effect::GaussianBlur { sigma: 4.0 }),
        Filter::Effect(Effect::Retouch {
            radius: 5.0,
            smoothness: 40.0,
            amount: 80.0,
        }),
        Filter::Effect(Effect::Sharpen {
            radius: 2.0,
            amount: 120.0,
            threshold: 3.0,
        }),
        Filter::Effect(Effect::EnhanceColors {
            auto: 100.0,
            vibrance: 30.0,
        }),
    ] {
        both(&|| {
            let mut s = surface.clone();
            filters::apply(&mut s, (3, -2), (310, 690), Some(&mask), filter);
            s.to_rgba()
        });
    }
}
