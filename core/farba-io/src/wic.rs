//! HEIF / HEIC and AVIF through WIC, the Windows Imaging Component, with the codecs installed
//! in Windows (HEIF Image Extensions + HEVC Video Extensions, AV1 Video Extension). Nothing
//! is shipped with Farba; without the extensions these formats report a clear error.
//!
//! Farba uses WIC for HEIC / HEIF only: AVIF is in `avif.rs`, because Windows' AVIF decoder
//! drops the alpha and there is no AV1 encoder. The HEIF encoder drops alpha too, so HEIC is
//! written opaque (the caller flattens over the background colour, as for JPEG).

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatHeif, GUID_WICPixelFormat24bppBGR,
    GUID_WICPixelFormat24bppRGB, GUID_WICPixelFormat32bppRGBA, IWICBitmapFrameDecode,
    IWICBitmapFrameEncode, IWICBitmapSource, IWICImagingFactory, WICBitmapEncoderNoCache,
    WICConvertBitmapSource, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{
    IPropertyBag2, PROPBAG2, PROPVARIANT, PropVariantClear,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Variant::{VARIANT, VT_UI2};
use windows::core::{HSTRING, PWSTR, w};

use crate::raster::Decoded;
use crate::stack::MAX_SIDE;

fn com() {
    // S_FALSE / RPC_E_CHANGED_MODE mean it is already initialised, which is fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

fn factory() -> Result<IWICImagingFactory, String> {
    com();
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| e.message().to_string())
}

/// WINCODEC_ERR_COMPONENTNOTFOUND: no codec for the file (the extension is not installed).
const NO_CODEC: i32 = 0x8898_2F50_u32 as i32;

/// MF_E_TOPO_CODEC_NOT_FOUND / MF_E_TRANSFORM_NOT_POSSIBLE_FOR_CURRENT_MEDIATYPE: the HEIF
/// container is known but the video codec inside (HEVC) is not installed.
const NO_VIDEO_CODEC: [i32; 2] = [0xC00D_5212_u32 as i32, 0xC00D_36B4_u32 as i32];

/// Which Windows extension a file needs (the frontend offers its Microsoft Store page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Heif,
    Raw,
}

/// The error text for a missing codec is `codec-missing:heif` / `codec-missing:raw` (the
/// frontend recognises it); anything else is Windows' message.
fn codec_error(e: windows::core::Error, codec: Codec) -> String {
    let code = e.code().0;
    if code == NO_CODEC || NO_VIDEO_CODEC.contains(&code) {
        match codec {
            Codec::Heif => "codec-missing:heif".into(),
            Codec::Raw => "codec-missing:raw".into(),
        }
    } else {
        e.message().to_string()
    }
}

/// Camera RAW extensions, opened through Windows' Raw Image Extension.
pub const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "ari", "arw", "bay", "cap", "cr2", "cr3", "crw", "dcr", "dcs", "dng", "drf", "eip",
    "erf", "fff", "iiq", "k25", "kdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "ori", "pef",
    "ptx", "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "x3f",
];

/// An upright WIC image whose pixels are decoded by [`Source::copy_to`].
pub(crate) struct Source {
    rgba: IWICBitmapSource,
    pub size: (u32, u32),
    codec: Codec,
}

impl Source {
    /// Decodes into `dst` (straight RGBA8, `width * height * 4` bytes).
    pub fn copy_to(&self, dst: &mut [u8]) -> Result<(), String> {
        // Decoding happens here (a missing HEVC codec shows up only now).
        unsafe { self.rgba.CopyPixels(std::ptr::null(), self.size.0 * 4, dst) }
            .map_err(|e| codec_error(e, self.codec))
    }
}

/// The EXIF orientation (1-8) of a frame, 1 when it has none.
fn orientation(frame: &IWICBitmapFrameDecode) -> u8 {
    unsafe {
        let Ok(reader) = frame.GetMetadataQueryReader() else {
            return 1;
        };
        let mut value = PROPVARIANT::default();
        if reader
            .GetMetadataByName(w!("System.Photo.Orientation"), &mut value)
            .is_err()
        {
            return 1;
        }
        let v = &value.Anonymous.Anonymous;
        let o = if v.vt == VT_UI2 { v.Anonymous.uiVal } else { 1 };
        let _ = PropVariantClear(&mut value);
        if (1..=8).contains(&o) { o as u8 } else { 1 }
    }
}

/// Opens the first image of a file through WIC (HEIF / HEIC, camera RAW). The EXIF
/// orientation is applied: Windows' codecs leave it to the application.
pub(crate) fn open(path: &Path, codec: Codec) -> Result<Decoded, String> {
    let f = factory()?;
    unsafe {
        let decoder = f
            .CreateDecoderFromFilename(
                &HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|e| codec_error(e, codec))?;
        let frame = decoder.GetFrame(0).map_err(|e| codec_error(e, codec))?;
        let orientation = orientation(&frame);
        let rgba = WICConvertBitmapSource(&GUID_WICPixelFormat32bppRGBA, &frame)
            .map_err(|e| codec_error(e, codec))?;
        let (mut w, mut h) = (0, 0);
        rgba.GetSize(&mut w, &mut h)
            .map_err(|e| e.message().to_string())?;
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            return Err(format!("Unsupported image size {w} x {h}"));
        }
        let source = Source {
            rgba,
            size: (w, h),
            codec,
        };
        if orientation == 1 {
            return Ok(Decoded::wic(source));
        }
        let mut data = vec![0u8; w as usize * h as usize * 4];
        source.copy_to(&mut data)?;
        Ok(Decoded::rotated(data, w, h, orientation))
    }
}

/// Decodes the first image of a file through WIC (HEIF / HEIC, camera RAW) to straight
/// upright RGBA8.
pub fn decode(path: &Path, codec: Codec) -> Result<(u32, u32, Vec<u8>), String> {
    let d = open(path, codec)?;
    let (w, h) = (d.width, d.height);
    Ok((w, h, d.into_rgba()?))
}

/// Sets one encoder option.
unsafe fn set_option(bag: &IPropertyBag2, name: &str, value: VARIANT) -> windows::core::Result<()> {
    let name = HSTRING::from(name);
    let prop = PROPBAG2 {
        pstrName: PWSTR(name.as_ptr() as *mut u16),
        ..Default::default()
    };
    unsafe { bag.Write(1, &prop, &value) }
}

/// `WICHeifCompressionHEVC`.
const HEIF_COMPRESSION_HEVC: u8 = 2;

/// Encodes RGB8 as HEIC with `quality` 0..=100 to `path` (through a temporary file).
pub fn encode_heic(
    path: &Path,
    rgb: &[u8],
    width: u32,
    height: u32,
    quality: u8,
) -> Result<(), String> {
    if rgb.len() != width as usize * height as usize * 3 {
        return Err("pixel buffer size does not match the image size".into());
    }
    let f = factory()?;
    let mut tmp = PathBuf::from(path);
    tmp.as_mut_os_string().push(".farba-tmp");
    let err = |e: windows::core::Error| codec_error(e, Codec::Heif);
    let result = (|| unsafe {
        let stream = f.CreateStream().map_err(err)?;
        stream
            .InitializeFromFilename(&HSTRING::from(tmp.as_os_str()), GENERIC_WRITE.0)
            .map_err(|e| e.message().to_string())?;
        let encoder = f
            .CreateEncoder(&GUID_ContainerFormatHeif, std::ptr::null())
            .map_err(err)?;
        encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .map_err(err)?;
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut bag: Option<IPropertyBag2> = None;
        encoder.CreateNewFrame(&mut frame, &mut bag).map_err(err)?;
        let (frame, bag) = frame.zip(bag).ok_or("The encoder made no frame")?;
        set_option(
            &bag,
            "ImageQuality",
            VARIANT::from(quality.min(100) as f32 / 100.0),
        )
        .map_err(err)?;
        set_option(
            &bag,
            "HeifCompressionMethod",
            VARIANT::from(HEIF_COMPRESSION_HEVC),
        )
        .map_err(err)?;
        frame.Initialize(&bag).map_err(err)?;
        frame.SetSize(width, height).map_err(err)?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format).map_err(err)?;
        let bitmap = f
            .CreateBitmapFromMemory(width, height, &GUID_WICPixelFormat24bppRGB, width * 3, rgb)
            .map_err(err)?;
        let source = WICConvertBitmapSource(&format, &bitmap).map_err(err)?;
        frame.WriteSource(&source, std::ptr::null()).map_err(err)?;
        frame.Commit().map_err(err)?;
        encoder.Commit().map_err(err)?;
        drop(stream);
        Ok::<(), String>(())
    })()
    .and_then(|()| std::fs::rename(&tmp, path).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("farba-wic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn close(a: &[u8], b: &[u8]) -> bool {
        a.iter()
            .zip(b)
            .all(|(x, y)| (*x as i32 - *y as i32).abs() < 12)
    }

    #[test]
    fn heic_round_trip() {
        let rgb: Vec<u8> = (0..64 * 48u32)
            .flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 5) as u8, 128])
            .collect();
        let path = temp("t.heic");
        encode_heic(&path, &rgb, 64, 48, 90).unwrap();
        let (w, h, px) = decode(&path, Codec::Heif).unwrap();
        assert_eq!((w, h), (64, 48));
        let i = 24 * 64 + 48;
        assert!(close(&px[i * 4..i * 4 + 3], &rgb[i * 3..i * 3 + 3]));
        assert_eq!(px[i * 4 + 3], 255);
        std::fs::remove_file(&path).unwrap();
    }
}
