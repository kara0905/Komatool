//! Assemblage vertical d'images, avec estimation préalable des dimensions.

use super::common::*;
use super::transform::{self, ImageAdjustments};
use image::{DynamicImage, GenericImage, ImageFormat, RgbImage, RgbaImage};
use std::{
    fs,
    io::{BufWriter, Write},
    path::Path,
};
use tauri::AppHandle;

/// Compose les sources de haut en bas et renvoie le canevas final en mémoire.
pub(crate) fn compose_images(
    app: &AppHandle,
    files: &[String],
    jpeg: bool,
    adjustments: &ImageAdjustments,
) -> Result<DynamicImage, String> {
    if files.len() < 2 {
        return Err("Il faut sélectionner au moins deux images.".into());
    }
    emit_progress(app, 1.0, "Analyse des dimensions…");
    // Lit d'abord les dimensions pour calculer le canevas sans garder toutes
    // les images décodées en mémoire.
    let mut dimensions = Vec::with_capacity(files.len());
    let mut total_height = 0u64;
    let mut max_width = 0u32;
    let mut last_progress = 1;
    for (index, file) in files.iter().enumerate() {
        let (width, height) = image::image_dimensions(file)
            .map_err(|e| format!("Impossible de lire les dimensions de {} : {e}", file))?;
        let (width, height) = transform::resulting_dimensions(width, height, adjustments)?;
        max_width = max_width.max(width);
        total_height = total_height
            .checked_add(height as u64)
            .ok_or_else(|| "Dimensions finales trop importantes.".to_string())?;
        dimensions.push((width, height));
        emit_progress_on_change(
            app,
            &mut last_progress,
            ((index + 1) as f32 / files.len() as f32) * 12.0,
            format!("Analyse des images : {} / {}", index + 1, files.len()),
        );
    }
    let total_height = u32::try_from(total_height)
        .map_err(|_| "La hauteur finale dépasse la limite prise en charge.".to_string())?;
    if jpeg {
        validate_jpeg_dimensions(max_width, total_height)?;
    }
    validate_image_size(max_width, total_height)?;
    let mut canvas = if jpeg {
        DynamicImage::ImageRgb8(RgbImage::new(max_width, total_height))
    } else {
        DynamicImage::ImageRgba8(RgbaImage::new(max_width, total_height))
    };
    let mut y = 0u32;
    // Insère une source à la fois et la convertit en RGB ou RGBA en la
    // consommant, pour limiter les copies complètes en mémoire.
    last_progress = 12;
    for (index, file) in files.iter().enumerate() {
        let decoded = transform::apply(
            image::open(file).map_err(|e| format!("Impossible d'ouvrir {} : {e}", file))?,
            adjustments,
        )?;
        match &mut canvas {
            DynamicImage::ImageRgb8(target) => {
                let rgb = decoded.into_rgb8();
                target
                    .copy_from(&rgb, 0, y)
                    .map_err(|e| format!("Impossible d'assembler {} : {e}", file))?;
            }
            DynamicImage::ImageRgba8(target) => {
                let rgba = decoded.into_rgba8();
                target
                    .copy_from(&rgba, 0, y)
                    .map_err(|e| format!("Impossible d'assembler {} : {e}", file))?;
            }
            _ => unreachable!("Le canevas de fusion utilise RGB ou RGBA."),
        }
        y += dimensions[index].1;
        emit_progress_on_change(
            app,
            &mut last_progress,
            12.0 + ((index + 1) as f32 / files.len() as f32) * 78.0,
            format!("Assemblage : {} / {}", index + 1, files.len()),
        );
    }
    Ok(canvas)
}

/// Vérifie le format, assemble les sources, puis enregistre le fichier final.
pub(crate) fn merge_images(
    app: &AppHandle,
    files: &[String],
    output: &Path,
    adjustments: &ImageAdjustments,
) -> Result<(), String> {
    let format = ImageFormat::from_path(output).map_err(|_| {
        "Extension de sortie non prise en charge. Utilise PNG, JPG ou WEBP.".to_string()
    })?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err("Format de sortie non pris en charge. Utilise PNG, JPG ou WEBP.".into());
    }
    if output.exists() {
        return Err(format!(
            "Le fichier {} existe déjà. Choisis un autre nom pour éviter de le remplacer.",
            output.display()
        ));
    }
    let canvas = compose_images(app, files, format == ImageFormat::Jpeg, adjustments)?;
    emit_progress(
        app,
        93.0,
        match format {
            ImageFormat::Png => "Enregistrement PNG (compression rapide)…",
            ImageFormat::Jpeg => "Enregistrement JPG…",
            ImageFormat::WebP => "Enregistrement WebP…",
            _ => "Enregistrement de l'image…",
        },
    );
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| {
            format!(
                "Impossible de créer {} sans remplacer un fichier existant : {e}",
                output.display()
            )
        })?;
    let mut writer = BufWriter::with_capacity(256 * 1024, file);
    if let Err(error) = encode_image(&canvas, &mut writer, format) {
        let _ = fs::remove_file(output);
        return Err(format!("Échec de l'enregistrement : {error}"));
    }
    if let Err(error) = writer.flush() {
        let _ = fs::remove_file(output);
        return Err(format!("Échec de l'écriture du fichier : {error}"));
    }
    emit_progress(app, 100.0, "Fusion terminée");
    Ok(())
}
