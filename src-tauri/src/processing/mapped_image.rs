//! Images temporaires stockées sur disque pour borner la mémoire vive des grands traitements.

use image::{
    codecs::{
        jpeg::JpegEncoder,
        png::{CompressionType, FilterType, PngEncoder},
        webp::WebPEncoder,
    },
    metadata::Orientation,
    ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader,
};
use memmap2::{MmapMut, MmapOptions};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Fichier temporaire mappé : seules les pages consultées doivent résider en RAM.
pub(crate) struct MappedBuffer {
    path: PathBuf,
    file: Option<File>,
    map: Option<MmapMut>,
}

impl MappedBuffer {
    pub(crate) fn new(bytes: usize) -> Result<Self, String> {
        let root = std::env::temp_dir();
        for _ in 0..10 {
            let path = root.join(format!(
                "komatool-image-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let file = match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("Impossible de créer l'image temporaire : {error}"))
                }
            };
            if let Err(error) = file.set_len(bytes as u64) {
                let _ = fs::remove_file(&path);
                return Err(format!(
                    "Impossible de réserver l'espace temporaire : {error}"
                ));
            }
            // Le fichier vient d'être créé et sa longueur correspond exactement à la zone mappée.
            let map = match unsafe { MmapOptions::new().len(bytes).map_mut(&file) } {
                Ok(map) => map,
                Err(error) => {
                    let _ = fs::remove_file(&path);
                    return Err(format!("Impossible de mapper l'image temporaire : {error}"));
                }
            };
            return Ok(Self {
                path,
                file: Some(file),
                map: Some(map),
            });
        }
        Err("Impossible de réserver un fichier temporaire unique.".into())
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        self.map.as_deref().expect("zone mappée active")
    }

    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        self.map.as_deref_mut().expect("zone mappée active")
    }

    fn flush(&self) -> Result<(), String> {
        self.map
            .as_ref()
            .expect("zone mappée active")
            .flush()
            .map_err(|error| format!("Impossible d'écrire les pixels temporaires : {error}"))
    }
}

impl Drop for MappedBuffer {
    fn drop(&mut self) {
        // Windows interdit parfois la suppression tant que le fichier est encore mappé.
        drop(self.map.take());
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

/// Image RGBA8 dont le stockage temporaire est adossé au disque.
pub(crate) struct MappedRgba {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pixels: MappedBuffer,
}

impl MappedRgba {
    fn byte_len(width: u32, height: u32, channels: usize) -> Result<usize, String> {
        let bytes = (width as u64)
            .checked_mul(height as u64)
            .and_then(|pixels| pixels.checked_mul(channels as u64))
            .ok_or_else(|| "Les dimensions de l'image sont trop grandes.".to_string())?;
        usize::try_from(bytes)
            .map_err(|_| "L'image est trop grande pour cette version du système.".to_string())
    }

    pub(crate) fn blank(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("L'image est vide.".into());
        }
        let mut pixels = MappedBuffer::new(Self::byte_len(width, height, 4)?)?;
        pixels.bytes_mut().fill(0);
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Décode une source dans le fichier mappé, puis convertit les pixels en RGBA8 sur place.
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let mut reader = ImageReader::open(path)
            .map_err(|error| format!("Impossible d'ouvrir {} : {error}", path.display()))?
            .with_guessed_format()
            .map_err(|error| format!("Impossible d'identifier {} : {error}", path.display()))?;
        reader.no_limits();
        let mut decoder = reader
            .into_decoder()
            .map_err(|error| format!("Impossible de préparer le décodage : {error}"))?;
        let (width, height) = decoder.dimensions();
        let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
        let color = decoder.color_type();
        let pixel_count = (width as u64)
            .checked_mul(height as u64)
            .ok_or_else(|| "Les dimensions de l'image sont trop grandes.".to_string())?;
        let pixels = usize::try_from(pixel_count)
            .map_err(|_| "L'image est trop grande pour cette version du système.".to_string())?;
        let output_len = Self::byte_len(width, height, 4)?;
        let source_len = usize::try_from(decoder.total_bytes())
            .map_err(|_| "Le décodage dépasse la capacité du système.".to_string())?;
        if source_len > output_len {
            return Err(
                "Ce format utilise une profondeur trop élevée pour le traitement par blocs.".into(),
            );
        }
        let mut mapped = MappedBuffer::new(output_len)?;
        decoder
            .read_image(&mut mapped.bytes_mut()[..source_len])
            .map_err(|error| format!("Impossible de décoder {} : {error}", path.display()))?;
        convert_to_rgba(mapped.bytes_mut(), pixels, color)?;
        let decoded = Self {
            width,
            height,
            pixels: mapped,
        };
        if orientation == Orientation::NoTransforms {
            return Ok(decoded);
        }
        let swaps_dimensions = matches!(
            orientation,
            Orientation::Rotate90
                | Orientation::Rotate270
                | Orientation::Rotate90FlipH
                | Orientation::Rotate270FlipH
        );
        let (output_width, output_height) = if swaps_dimensions {
            (height, width)
        } else {
            (width, height)
        };
        let mut oriented = Self::blank(output_width, output_height)?;
        for y in 0..height {
            for x in 0..width {
                let (dest_x, dest_y) = match orientation {
                    Orientation::NoTransforms => (x, y),
                    Orientation::Rotate90 => (height - 1 - y, x),
                    Orientation::Rotate180 => (width - 1 - x, height - 1 - y),
                    Orientation::Rotate270 => (y, width - 1 - x),
                    Orientation::FlipHorizontal => (width - 1 - x, y),
                    Orientation::FlipVertical => (x, height - 1 - y),
                    Orientation::Rotate90FlipH => (y, x),
                    Orientation::Rotate270FlipH => (height - 1 - y, width - 1 - x),
                };
                let source_offset = (y as usize * width as usize + x as usize) * 4;
                let dest_offset = (dest_y as usize * output_width as usize + dest_x as usize) * 4;
                oriented.bytes_mut()[dest_offset..dest_offset + 4]
                    .copy_from_slice(&decoded.bytes()[source_offset..source_offset + 4]);
            }
        }
        Ok(oriented)
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        self.pixels.bytes()
    }
    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        self.pixels.bytes_mut()
    }

    /// Produit une vignette en lisant quelques pixels par zone, sans matérialiser l'original.
    pub(crate) fn thumbnail(&self, max_width: u32, max_height: u32) -> image::RgbaImage {
        let scale = (max_width as f64 / self.width as f64)
            .min(max_height as f64 / self.height as f64)
            .min(1.0);
        let width = ((self.width as f64 * scale).round() as u32).max(1);
        let height = ((self.height as f64 * scale).round() as u32).max(1);
        let mut output = image::RgbaImage::new(width, height);
        for y in 0..height {
            let source_y = (((y as f64 + 0.5) * self.height as f64 / height as f64) as u32)
                .min(self.height - 1);
            for x in 0..width {
                let source_x = (((x as f64 + 0.5) * self.width as f64 / width as f64) as u32)
                    .min(self.width - 1);
                let offset = (source_y as usize * self.width as usize + source_x as usize) * 4;
                let pixel = &self.bytes()[offset..offset + 4];
                output.put_pixel(x, y, image::Rgba([pixel[0], pixel[1], pixel[2], pixel[3]]));
            }
        }
        output
    }

    pub(crate) fn copy_rect_from(
        &mut self,
        source: &MappedRgba,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        dest_y: u32,
    ) -> Result<(), String> {
        self.copy_rect_from_at(source, x, y, width, height, 0, dest_y)
    }

    pub(crate) fn copy_rect_from_at(
        &mut self,
        source: &MappedRgba,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        dest_x: u32,
        dest_y: u32,
    ) -> Result<(), String> {
        if x.checked_add(width)
            .filter(|right| *right <= source.width)
            .is_none()
            || y.checked_add(height)
                .filter(|bottom| *bottom <= source.height)
                .is_none()
            || dest_y
                .checked_add(height)
                .filter(|bottom| *bottom <= self.height)
                .is_none()
            || dest_x
                .checked_add(width)
                .filter(|right| *right <= self.width)
                .is_none()
        {
            return Err("La zone à copier dépasse les dimensions de l'image.".into());
        }
        let row_bytes = width as usize * 4;
        let source_stride = source.width as usize * 4;
        let dest_stride = self.width as usize * 4;
        for row in 0..height as usize {
            let source_start = (y as usize + row) * source_stride + x as usize * 4;
            let dest_start = (dest_y as usize + row) * dest_stride + dest_x as usize * 4;
            self.bytes_mut()[dest_start..dest_start + row_bytes]
                .copy_from_slice(&source.bytes()[source_start..source_start + row_bytes]);
        }
        Ok(())
    }

    /// Applique un recadrage et des marges blanches sans charger le bitmap en mémoire vive.
    pub(crate) fn crop_and_margin(
        &self,
        left: u32,
        top: u32,
        right: u32,
        bottom: u32,
        margin: u32,
    ) -> Result<Self, String> {
        let width = self.width.saturating_sub(left.saturating_add(right));
        let height = self.height.saturating_sub(top.saturating_add(bottom));
        if width == 0 || height == 0 {
            return Err("Le recadrage supprimerait toute l'image.".into());
        }
        if margin > 2_000 {
            return Err("La marge maximale est de 2 000 pixels.".into());
        }
        let twice_margin = margin
            .checked_mul(2)
            .ok_or_else(|| "Marge trop importante.".to_string())?;
        let output_width = width
            .checked_add(twice_margin)
            .ok_or_else(|| "Dimensions trop importantes.".to_string())?;
        let output_height = height
            .checked_add(twice_margin)
            .ok_or_else(|| "Dimensions trop importantes.".to_string())?;
        let mut output = Self::blank(output_width, output_height)?;
        if margin > 0 {
            for pixel in output.bytes_mut().chunks_exact_mut(4) {
                pixel.copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        output.copy_rect_from_at(self, left, top, width, height, margin, margin)?;
        Ok(output)
    }

    /// Enregistre l'image sans créer de copie complète en mémoire vive.
    pub(crate) fn save(&self, path: &Path, format: ImageFormat, quality: u8) -> Result<(), String> {
        if format == ImageFormat::Jpeg {
            super::common::validate_jpeg_dimensions(self.width, self.height)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("Impossible de créer {} : {error}", path.display()))?;
        let result = (|| {
            let mut writer = BufWriter::with_capacity(256 * 1024, file);
            match format {
                ImageFormat::Png => PngEncoder::new_with_quality(
                    &mut writer,
                    CompressionType::Fast,
                    FilterType::NoFilter,
                )
                .write_image(
                    self.bytes(),
                    self.width,
                    self.height,
                    ExtendedColorType::Rgba8,
                ),
                ImageFormat::Jpeg => {
                    let rgb_len = Self::byte_len(self.width, self.height, 3)?;
                    let mut rgb = MappedBuffer::new(rgb_len)?;
                    for (rgba, output) in self
                        .bytes()
                        .chunks_exact(4)
                        .zip(rgb.bytes_mut().chunks_exact_mut(3))
                    {
                        let alpha = rgba[3] as u16;
                        for channel in 0..3 {
                            output[channel] =
                                ((rgba[channel] as u16 * alpha + 255 * (255 - alpha) + 127) / 255)
                                    as u8;
                        }
                    }
                    JpegEncoder::new_with_quality(&mut writer, quality.clamp(1, 100)).write_image(
                        rgb.bytes(),
                        self.width,
                        self.height,
                        ExtendedColorType::Rgb8,
                    )
                }
                ImageFormat::WebP => WebPEncoder::new_lossless(&mut writer).write_image(
                    self.bytes(),
                    self.width,
                    self.height,
                    ExtendedColorType::Rgba8,
                ),
                _ => {
                    return Err(
                        "Format de sortie non pris en charge pour les grandes images.".into(),
                    )
                }
            }
            .map_err(|error| format!("Échec de l'encodage : {error}"))?;
            self.pixels.flush()?;
            writer
                .flush()
                .map_err(|error| format!("Échec de l'écriture : {error}"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result
    }

    /// Extrait une page dans un tampon temporaire puis l'encode sans copie complète en RAM.
    pub(crate) fn save_region(
        &self,
        path: &Path,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        png: bool,
    ) -> Result<(), String> {
        let mut page = Self::blank(width, height)?;
        page.copy_rect_from(self, x, y, width, height, 0)?;
        page.save(
            path,
            if png {
                ImageFormat::Png
            } else {
                ImageFormat::Jpeg
            },
            85,
        )
    }

    pub(crate) fn save_temp_jpeg(&self, quality: u8) -> Result<PathBuf, String> {
        let path = std::env::temp_dir().join(format!(
            "komatool-pdf-{}-{}.jpg",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        self.save(&path, ImageFormat::Jpeg, quality)?;
        Ok(path)
    }
}

/// Convertit les profondeurs usuelles en RGBA8, de la fin vers le début pour autoriser l'expansion.
fn convert_to_rgba(bytes: &mut [u8], pixels: usize, color: image::ColorType) -> Result<(), String> {
    use image::ColorType::*;
    let (channels, bytes_per_channel) = match color {
        L8 => (1, 1),
        La8 => (2, 1),
        Rgb8 => (3, 1),
        Rgba8 => (4, 1),
        L16 => (1, 2),
        La16 => (2, 2),
        Rgb16 => (3, 2),
        Rgba16 => (4, 2),
        _ => {
            return Err(
                "Ce type de pixels n'est pas pris en charge pour les images géantes.".into(),
            )
        }
    };
    let stride = channels * bytes_per_channel;
    for index in (0..pixels).rev() {
        let start = index * stride;
        let read = |channel: usize| -> u8 {
            let offset = start + channel * bytes_per_channel;
            if bytes_per_channel == 1 {
                bytes[offset]
            } else {
                (u16::from_ne_bytes([bytes[offset], bytes[offset + 1]]) >> 8) as u8
            }
        };
        let (red, green, blue, alpha) = match channels {
            1 => {
                let value = read(0);
                (value, value, value, 255)
            }
            2 => {
                let value = read(0);
                (value, value, value, read(1))
            }
            3 => (read(0), read(1), read(2), 255),
            4 => (read(0), read(1), read(2), read(3)),
            _ => unreachable!(),
        };
        let output = index * 4;
        bytes[output..output + 4].copy_from_slice(&[red, green, blue, alpha]);
    }
    Ok(())
}
