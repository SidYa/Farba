//! AI background removal: BiRefNet ONNX models run with ONNX Runtime (DirectML on the GPU,
//! CPU as fallback). A port of the owner's own Pinta add-in `Pinta.AiBackgroundRemoval`,
//! with the same models, cache folder layout, download checks and pipeline, so results
//! match:
//!
//! 1. the layer as straight RGB (alpha ignored; transparent pixels are black),
//! 2. resized to 1024 x 1024 (no aspect ratio, linear filter),
//! 3. normalised `(v / 255 - mean) / std`, NCHW, f16 when the model takes f16,
//! 4. the **last** output, `sigmoid * 255`, no threshold,
//! 5. resized back to the layer size; the caller multiplies the layer's alpha by it.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use farba_core::transform::{Interpolation, resample};
#[cfg(windows)]
use ort::ep::DirectML;
pub use ort::session::RunOptions;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::{Tensor, TensorElementType, ValueType};

/// Model input size (all BiRefNet exports).
pub const INPUT: usize = 1024;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    /// Download size (Content-Length of the published file).
    pub size: u64,
    pub license: &'static str,
}

/// The models, default first (sizes checked against huggingface.co, as in Pinta).
pub const MODELS: &[Model] = &[
    Model {
        id: "birefnet-lite-fp16",
        name: "BiRefNet lite (fp16, fastest)",
        file: "birefnet-lite-fp16.onnx",
        url: "https://huggingface.co/onnx-community/BiRefNet_lite-ONNX/resolve/main/onnx/model_fp16.onnx",
        size: 114_538_221,
        license: "MIT",
    },
    Model {
        id: "birefnet-fp16",
        name: "BiRefNet full (fp16, best quality)",
        file: "birefnet-fp16.onnx",
        url: "https://huggingface.co/onnx-community/BiRefNet-ONNX/resolve/main/onnx/model_fp16.onnx",
        size: 489_666_272,
        license: "MIT",
    },
];

/// Files of models no longer offered (the fp32 ones, removed 2026-10-14: they loaded the
/// whole PC heavily for no visible gain); deleted when the add-on starts.
pub const RETIRED_FILES: &[&str] = &["birefnet-lite.onnx", "birefnet.onnx"];

/// A model by id (the default for an unknown id).
pub fn model(id: &str) -> &'static Model {
    MODELS.iter().find(|m| m.id == id).unwrap_or(&MODELS[0])
}

pub mod addon;
#[cfg(target_os = "linux")]
mod glibc_compat;

pub type Result<T> = std::result::Result<T, String>;

/// Raised by long operations when the user cancelled.
pub const CANCELLED: &str = "cancelled";

struct Cached {
    path: PathBuf,
    gpu: bool,
    session: Session,
    provider: &'static str,
}

/// Model cache folder and the cached inference session.
pub struct Ai {
    dir: PathBuf,
    session: Option<Cached>,
}

/// Run options for one inference; `terminate()` on it (from any thread) stops the run.
pub fn new_run() -> Result<Arc<RunOptions>> {
    RunOptions::new().map(Arc::new).map_err(|e| e.to_string())
}

impl Ai {
    /// `dir`: where downloaded models live (the app data folder's `ai-models`).
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, session: None }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn model_path(&self, m: &Model) -> PathBuf {
        self.dir.join(m.file)
    }

    /// A model is complete once it has its final name (downloads go to `.part` first).
    pub fn is_downloaded(&self, m: &Model) -> bool {
        self.model_path(m).is_file()
    }

    /// Bytes used by the cache folder (everything in it).
    pub fn downloaded_bytes(&self) -> u64 {
        fs::read_dir(&self.dir)
            .map(|d| {
                d.flatten()
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Releases the session (Windows keeps an open model file locked).
    pub fn release(&mut self) {
        self.session = None;
    }

    /// Deletes every downloaded model (and partial downloads). Returns how many files.
    pub fn delete_downloaded(&mut self) -> Result<usize> {
        self.release();
        let Ok(dir) = fs::read_dir(&self.dir) else {
            return Ok(0);
        };
        let mut n = 0;
        for e in dir.flatten() {
            if e.path().is_file() {
                fs::remove_file(e.path()).map_err(|e| e.to_string())?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Downloads the model if needed. `progress(done, total)`; `cancel` stops it.
    pub fn ensure_downloaded(
        &self,
        m: &Model,
        mut progress: impl FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<PathBuf> {
        let path = self.model_path(m);
        if path.is_file() {
            return Ok(path);
        }
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        // Into a temporary file first: an interrupted download never looks like a model.
        let part = path.with_extension("onnx.part");
        let _ = fs::remove_file(&part);
        // TLS through Windows (SChannel), not rustls.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .build(),
            )
            .build()
            .into();
        // Hugging Face's CDN sometimes answers 5xx for a moment: try again twice.
        let mut attempt = 0;
        let mut response = match loop {
            let r = agent.get(m.url).call();
            let transient = matches!(&r, Err(ureq::Error::StatusCode(500..=599)))
                || matches!(&r, Err(ureq::Error::Io(_) | ureq::Error::ConnectionFailed));
            if !transient || attempt == 2 || cancel.load(Ordering::Relaxed) {
                break r;
            }
            attempt += 1;
            std::thread::sleep(std::time::Duration::from_secs([2, 5][attempt - 1]));
        } {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(code @ 500..=599)) => {
                return Err(format!(
                    "{} could not be downloaded: the Hugging Face server reported an error \
                     ({code}). This is usually temporary; please try again in a few minutes.",
                    m.name
                ));
            }
            // Hugging Face answers 401 both for a private and for a missing repository.
            Err(ureq::Error::StatusCode(code @ (401 | 403))) => {
                return Err(format!(
                    "{} could not be downloaded: the server refused the request ({code}). The \
                     model may have been moved or may now need an account. You can also place it \
                     by hand in {} as \"{}\".",
                    m.name,
                    self.dir.display(),
                    m.file
                ));
            }
            Err(e) => return Err(format!("Download failed: {e}")),
        };
        let total = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let mut reader = response.body_mut().as_reader();
        let mut file = fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        let result = (|| -> Result<()> {
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(CANCELLED.into());
                }
                let n = reader
                    .read(&mut buf)
                    .map_err(|e| format!("Download failed: {e}"))?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                done += n as u64;
                progress(done, total);
            }
            file.flush().map_err(|e| e.to_string())?;
            // The server's own Content-Length decides what "complete" is.
            if let Some(t) = total
                && t != done
            {
                return Err(format!(
                    "The downloaded model has an unexpected size: {done} bytes instead of {t}."
                ));
            }
            Ok(())
        })();
        drop(file);
        if let Err(e) = result {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        fs::rename(&part, &path).map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// The cached session for this model and device, created when needed. With `gpu`,
    /// DirectML (device 0) is tried first and the CPU is used when it fails.
    fn session(&mut self, path: &Path, gpu: bool) -> Result<&mut Cached> {
        let reuse = self
            .session
            .as_ref()
            .is_some_and(|c| c.path == path && c.gpu == gpu);
        if !reuse {
            self.session = None;
            let mut cached = None;
            // DirectML is Windows only; elsewhere always the CPU.
            #[cfg(windows)]
            if gpu {
                cached = gpu_session(path).ok().map(|session| Cached {
                    path: path.into(),
                    gpu,
                    session,
                    provider: "GPU (DirectML)",
                });
            }
            if cached.is_none() {
                cached = Some(Cached {
                    path: path.into(),
                    gpu,
                    session: cpu_session(path)?,
                    provider: "CPU",
                });
            }
            self.session = cached;
        }
        Ok(self.session.as_mut().unwrap())
    }

    /// The foreground mask of a straight RGBA8 image (`width * height` bytes, 255 = keep)
    /// and the device that computed it. `run` (see [`new_run`]) can stop it from another thread.
    #[allow(clippy::too_many_arguments)]
    pub fn mask(
        &mut self,
        rgba: &[u8],
        width: usize,
        height: usize,
        model_path: &Path,
        gpu: bool,
        cancel: &AtomicBool,
        run: &RunOptions,
    ) -> Result<(Vec<u8>, &'static str)> {
        let small = resample(rgba, width, height, 4, INPUT, INPUT, Interpolation::Linear);
        let plane = INPUT * INPUT;
        let mut input = vec![0f32; 3 * plane];
        for (i, p) in small.as_chunks::<4>().0.iter().enumerate() {
            for c in 0..3 {
                input[c * plane + i] = (p[c] as f32 / 255.0 - MEAN[c]) / STD[c];
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        let cached = self.session(model_path, gpu)?;
        let provider = cached.provider;
        let half = matches!(
            cached.session.inputs().first().map(|i| i.dtype()),
            Some(ValueType::Tensor {
                ty: TensorElementType::Float16,
                ..
            })
        );
        let shape = [1usize, 3, INPUT, INPUT];
        let outputs = if half {
            let data: Vec<half::f16> = input.iter().map(|&v| half::f16::from_f32(v)).collect();
            let t = Tensor::from_array((shape, data)).map_err(|e| e.to_string())?;
            cached.session.run_with_options(ort::inputs![t], run)
        } else {
            let t = Tensor::from_array((shape, input)).map_err(|e| e.to_string())?;
            cached.session.run_with_options(ort::inputs![t], run)
        };
        let outputs = match outputs {
            Ok(o) => o,
            Err(_) if cancel.load(Ordering::Relaxed) => return Err(CANCELLED.into()),
            Err(e) => return Err(format!("The model failed: {e}")),
        };
        // BiRefNet outputs increasingly refined maps; the last one is the result.
        let last = &outputs[outputs.len() - 1];
        let logits: Vec<f32> = match last.try_extract_tensor::<f32>() {
            Ok((_, v)) => v.to_vec(),
            Err(_) => {
                let (_, v) = last
                    .try_extract_tensor::<half::f16>()
                    .map_err(|e| format!("Unexpected model output: {e}"))?;
                v.iter().map(|h| h.to_f32()).collect()
            }
        };
        if logits.len() < plane {
            return Err(format!(
                "The model returned {} values, expected at least {plane}.",
                logits.len()
            ));
        }
        let small_mask: Vec<u8> = logits[..plane]
            .iter()
            .map(|&x| (1.0 / (1.0 + (-x).exp()) * 255.0 + 0.5).clamp(0.0, 255.0) as u8)
            .collect();
        let mask = resample(
            &small_mask,
            INPUT,
            INPUT,
            1,
            width,
            height,
            Interpolation::Linear,
        );
        Ok((mask, provider))
    }
}

#[cfg(windows)]
fn gpu_session(path: &Path) -> Result<Session> {
    Session::builder()
        .map_err(|e| e.to_string())?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| e.to_string())?
        // DirectML's graph fusion compiles BiRefNet into one graph that needs ~13 GB at
        // 1024 x 1024: on an 8 GB card ~5 GB went to shared system memory and every run took
        // 3 s (lite) / 1.4 s (full). Without it: 0.3 s / 0.48 s, no shared memory, same mask.
        .with_config_entry("ep.dml.disable_graph_fusion", "1")
        .map_err(|e| e.to_string())?
        // Both required by the DirectML execution provider.
        .with_memory_pattern(false)
        .map_err(|e| e.to_string())?
        .with_parallel_execution(false)
        .map_err(|e| e.to_string())?
        .with_execution_providers([DirectML::default()
            .with_device_id(0)
            .build()
            .error_on_failure()])
        .map_err(|e| e.to_string())?
        .commit_from_file(path)
        .map_err(|e| e.to_string())
}

fn cpu_session(path: &Path) -> Result<Session> {
    // Leave a core free so the UI stays responsive.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).max(1))
        .unwrap_or(1);
    Session::builder()
        .map_err(|e| e.to_string())?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| e.to_string())?
        .with_intra_threads(threads)
        .map_err(|e| e.to_string())?
        .commit_from_file(path)
        .map_err(|e| format!("Could not load the model: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue() {
        assert_eq!(model("nope").id, "birefnet-lite-fp16");
        assert_eq!(model("birefnet").id, "birefnet-lite-fp16");
        assert_eq!(model("birefnet-fp16").file, "birefnet-fp16.onnx");
        assert_eq!(MODELS.len(), 2);
        assert!(
            MODELS
                .iter()
                .all(|m| m.url.starts_with("https://huggingface.co/"))
        );
    }

    #[test]
    fn cache_folder_bookkeeping() {
        let dir = std::env::temp_dir().join(format!("farba-ai-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut ai = Ai::new(dir.clone());
        assert_eq!(ai.downloaded_bytes(), 0);
        assert!(!ai.is_downloaded(&MODELS[0]));
        fs::create_dir_all(&dir).unwrap();
        fs::write(ai.model_path(&MODELS[0]), [0u8; 10]).unwrap();
        fs::write(dir.join("x.onnx.part"), [0u8; 5]).unwrap();
        assert!(ai.is_downloaded(&MODELS[0]));
        assert_eq!(ai.downloaded_bytes(), 15);
        assert_eq!(ai.delete_downloaded().unwrap(), 2);
        assert_eq!(ai.downloaded_bytes(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
