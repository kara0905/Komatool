//! Assemblage vertical d'images, avec estimation préalable des dimensions.

use super::common::*;
use super::mapped_image::MappedRgba;
use super::transform::{self, ImageAdjustments};
use image::{DynamicImage, GenericImage, ImageFormat, RgbImage, RgbaImage};
use std::{
    fs,
    io::{BufWriter, Write},
    path::Path,
};
use tauri::AppHandle;

/// Dit si le bitmap complet ou le canevas final dépasse le budget RAM classique.
pub(crate) fn requires_mapped_processing(
    files: &[String],
    adjustments: &ImageAdjustments,
) -> Result<bool, String> {
    let mut has_large_source = false;
    let mut max_width = 0u32;
    let mut total_height = 0u64;
    for file in files {
        let (source_width, source_height) = image::image_dimensions(file)
            .map_err(|error| format!("Impossible de lire les dimensions de {} : {error}", file))?;
        has_large_source |= exceeds_in_memory_limit(source_width, source_height);
        let (width, height) = if let Some(target_width) = adjustments.resize_width {
            if has_large_source {
                return Ok(true);
            }
            let crop_width = source_width
                .saturating_sub(adjustments.crop_left.saturating_add(adjustments.crop_right));
            let crop_height = source_height
                .saturating_sub(adjustments.crop_top.saturating_add(adjustments.crop_bottom));
            if crop_width == 0 || crop_height == 0 {
                return Err("Le recadrage supprimerait toute l'image.".into());
            }
            (
                target_width,
                ((crop_height as f64 * target_width as f64 / crop_width as f64).round() as u32)
                    .max(1),
            )
        } else {
            let width = source_width
                .saturating_sub(adjustments.crop_left.saturating_add(adjustments.crop_right));
            let height = source_height
                .saturating_sub(adjustments.crop_top.saturating_add(adjustments.crop_bottom));
            if width == 0 || height == 0 {
                return Err("Le recadrage supprimerait toute l'image.".into());
            }
            let margin = adjustments
                .margin
                .checked_mul(2)
                .ok_or_else(|| "Marge trop importante.".to_string())?;
            (
                width
                    .checked_add(margin)
                    .ok_or_else(|| "Dimensions trop importantes.".to_string())?,
                height
                    .checked_add(margin)
                    .ok_or_else(|| "Dimensions trop importantes.".to_string())?,
            )
        };
        max_width = max_width.max(width);
        total_height = total_height
            .checked_add(height as u64)
            .ok_or_else(|| "Dimensions finales trop importantes.".to_string())?;
    }
    Ok(has_large_source
        || total_height > u32::MAX as u64
        || exceeds_in_memory_limit(max_width, total_height.min(u32::MAX as u64) as u32))
}

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
    let requires_mapped = requires_mapped_processing(files, adjustments)?;
    if requires_mapped && adjustments.resize_width.is_some() {
        return Err("Le redimensionnement d'une très grande fusion demande encore une grande zone de travail. Enlève ce réglage ou réduis les images avant la fusion.".into());
    }
    if adjustments.resize_width.is_none() && files.len() >= 2 {
        let mut max_width = 0u32;
        let mut total_height = 0u64;
        for file in files {
            let (source_width, source_height) = image::image_dimensions(file).map_err(|error| {
                format!("Impossible de lire les dimensions de {} : {error}", file)
            })?;
            let width = source_width
                .saturating_sub(adjustments.crop_left.saturating_add(adjustments.crop_right));
            let height = source_height
                .saturating_sub(adjustments.crop_top.saturating_add(adjustments.crop_bottom));
            if width == 0 || height == 0 {
                return Err("Le recadrage supprimerait toute l'image.".into());
            }
            let margin = adjustments
                .margin
                .checked_mul(2)
                .ok_or_else(|| "Marge trop importante.".to_string())?;
            let width = width
                .checked_add(margin)
                .ok_or_else(|| "Dimensions trop importantes.".to_string())?;
            let height = height
                .checked_add(margin)
                .ok_or_else(|| "Dimensions trop importantes.".to_string())?;
            max_width = max_width.max(width);
            total_height = total_height
                .checked_add(height as u64)
                .ok_or_else(|| "Dimensions finales trop importantes.".to_string())?;
        }
        if total_height > u32::MAX as u64 {
            return Err("La hauteur finale dépasse la limite prise en charge.".into());
        }
        let total_height = total_height as u32;
        if requires_mapped || exceeds_in_memory_limit(max_width, total_height) {
            emit_progress(app, 1.0, "Préparation de la fusion par blocs…");
            let mut canvas = MappedRgba::blank(max_width, total_height)?;
            let mut top = 0u32;
            for (index, file) in files.iter().enumerate() {
                let decoded = MappedRgba::open(Path::new(file))?;
                let source = decoded.crop_and_margin(
                    adjustments.crop_left,
                    adjustments.crop_top,
                    adjustments.crop_right,
                    adjustments.crop_bottom,
                    adjustments.margin,
                )?;
                canvas.copy_rect_from(&source, 0, 0, source.width, source.height, top)?;
                top += source.height;
                emit_progress(
                    app,
                    8.0 + (index + 1) as f32 / files.len() as f32 * 82.0,
                    format!("Fusion par blocs : {} / {}", index + 1, files.len()),
                );
            }
            emit_progress(app, 93.0, "Enregistrement de la grande image…");
            canvas.save(output, format, 85)?;
            emit_progress(app, 100.0, "Fusion terminée");
            return Ok(());
        }
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
