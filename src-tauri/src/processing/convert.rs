//! Conversion et optimisation d'images en lot, avec refus d'écraser les sorties.

use super::{
    common::{
        emit_progress, exceeds_in_memory_limit, validate_image_size, validate_jpeg_dimensions,
    },
    mapped_image::MappedRgba,
};
use image::{
    codecs::{
        jpeg::JpegEncoder,
        png::{CompressionType, FilterType, PngEncoder},
        webp::WebPEncoder,
    },
    DynamicImage, ImageEncoder, RgbaImage,
};
use std::{
    fs::{self, OpenOptions},
    io::BufWriter,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

/// Formats actuellement proposés par les espaces Conversion et Optimisation.
#[derive(Clone, Copy)]
pub enum OutputFormat {
    Png,
    Jpeg,
    Webp,
}

impl OutputFormat {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "webp" => Ok(Self::Webp),
            _ => Err("Choisis le format PNG, JPG ou WebP.".into()),
        }
    }
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
}

fn encode(
    image: &DynamicImage,
    path: &Path,
    format: OutputFormat,
    quality: u8,
) -> Result<(), String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Impossible de créer {} : {e}", path.display()))?;
    let result = (|| {
        let mut writer = BufWriter::new(file);
        match format {
            OutputFormat::Png => image.write_with_encoder(PngEncoder::new_with_quality(
                &mut writer,
                CompressionType::Best,
                FilterType::Adaptive,
            )),
            OutputFormat::Jpeg => {
                // JPEG ne gère pas la transparence : les pixels transparents
                // sont composités sur un fond blanc avant l'encodage.
                let rgba = image.to_rgba8();
                let mut white = RgbaImage::from_pixel(
                    rgba.width(),
                    rgba.height(),
                    image::Rgba([255, 255, 255, 255]),
                );
                image::imageops::overlay(&mut white, &rgba, 0, 0);
                let rgb = DynamicImage::ImageRgba8(white).to_rgb8();
                JpegEncoder::new_with_quality(&mut writer, quality).write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
            }
            OutputFormat::Webp => image.write_with_encoder(WebPEncoder::new_lossless(&mut writer)),
        }
        .map_err(|e| e.to_string())?;
        use std::io::Write;
        writer.flush().map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

/// Convertit un lot après avoir vérifié les sorties et les limites de chaque image.
pub fn process(
    app: &AppHandle,
    files: &[String],
    output: &Path,
    format: OutputFormat,
    quality: u8,
) -> Result<String, String> {
    if files.is_empty() {
        return Err("Ajoute au moins une image.".into());
    }
    if !(1..=100).contains(&quality) {
        return Err("La qualité doit être comprise entre 1 et 100.".into());
    }
    fs::create_dir_all(output)
        .map_err(|e| format!("Impossible de créer le dossier de sortie : {e}"))?;
    let mut destinations = Vec::<PathBuf>::with_capacity(files.len());
    // Vérifie toutes les destinations avant de commencer pour éviter qu'une
    // collision découverte en cours de lot laisse un résultat partiel.
    for (index, source) in files.iter().enumerate() {
        let input = Path::new(source);
        let stem = input
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("image");
        let dest = output.join(format!("{:03}_{stem}.{}", index + 1, format.extension()));
        if dest.exists() || destinations.contains(&dest) {
            return Err(format!(
                "Le fichier de sortie existe déjà : {}",
                dest.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        destinations.push(dest);
    }
    let total_before: u64 = files
        .iter()
        .filter_map(|f| fs::metadata(f).ok().map(|m| m.len()))
        .sum();
    let mut total_after = 0u64;
    for (index, (source, dest)) in files.iter().zip(destinations.iter()).enumerate() {
        let (width, height) = image::image_dimensions(source)
            .map_err(|e| format!("Impossible de lire les dimensions de {} : {e}", source))?;
        let start = index as f32 / files.len() as f32 * 100.0;
        let image_progress = 100.0 / files.len() as f32;
        if exceeds_in_memory_limit(width, height) {
            emit_progress(
                app,
                start + image_progress * 0.1,
                format!("Lecture par blocs de l’image {}/{}", index + 1, files.len()),
            );
            if matches!(format, OutputFormat::Jpeg) {
                validate_jpeg_dimensions(width, height)?;
            }
            let image = MappedRgba::open(Path::new(source))?;
            let image_format = match format {
                OutputFormat::Png => image::ImageFormat::Png,
                OutputFormat::Jpeg => image::ImageFormat::Jpeg,
                OutputFormat::Webp => image::ImageFormat::WebP,
            };
            image.save(dest, image_format, quality)?;
            total_after += fs::metadata(dest)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            emit_progress(
                app,
                (index + 1) as f32 / files.len() as f32 * 100.0,
                format!("Image {}/{} terminée", index + 1, files.len()),
            );
            continue;
        }
        validate_image_size(width, height)?;
        if matches!(format, OutputFormat::Jpeg) {
            validate_jpeg_dimensions(width, height)?;
        }
        let image =
            image::open(source).map_err(|e| format!("Impossible d'ouvrir {} : {e}", source))?;
        emit_progress(
            app,
            start + image_progress * 0.1,
            format!("Lecture de l’image {}/{}", index + 1, files.len()),
        );
        emit_progress(
            app,
            start + image_progress * 0.2,
            format!(
                "Encodage {:?} · {} × {} px",
                match format {
                    OutputFormat::Png => "PNG sans perte",
                    OutputFormat::Jpeg => "JPEG",
                    OutputFormat::Webp => "WebP sans perte",
                },
                image.width(),
                image.height()
            ),
        );
        encode(&image, dest, format, quality)?;
        total_after += fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        emit_progress(
            app,
            (index + 1) as f32 / files.len() as f32 * 100.0,
            format!("Image {}/{} terminée", index + 1, files.len()),
        );
    }
    let ratio = if total_before == 0 {
        0.0
    } else {
        (1.0 - total_after as f64 / total_before as f64) * 100.0
    };
    let message = if ratio >= 0.0 {
        format!(
            "{} images traitées · {:.1}% de poids en moins",
            files.len(),
            ratio
        )
    } else {
        format!(
            "{} images traitées · poids augmenté de {:.1}%",
            files.len(),
            -ratio
        )
    };
    Ok(message)
}

pub fn parse_format(value: &str) -> Result<OutputFormat, String> {
    OutputFormat::parse(value)
}
