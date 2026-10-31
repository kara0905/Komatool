use super::common::*;
use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

/// Évalue une ligne en additionnant les écarts RVB entre pixels échantillonnés.
#[inline]
pub(super) fn row_difference_score(pixels: &[u8], width: usize, y: usize, step: usize) -> u64 {
    let row_start = y * width * 4;
    let row = &pixels[row_start..row_start + width * 4];
    let mut previous: Option<[u8; 3]> = None;
    let mut total = 0u64;
    for x in (0..width).step_by(step) {
        let offset = x * 4;
        let current = [row[offset], row[offset + 1], row[offset + 2]];
        if let Some(prev) = previous {
            total += current[0].abs_diff(prev[0]) as u64
                + current[1].abs_diff(prev[1]) as u64
                + current[2].abs_diff(prev[2]) as u64;
        }
        previous = Some(current);
    }
    total
}

/// Recherche des séparations proches des hauteurs cibles, de façon déterministe.
pub(crate) fn smart_boundaries(
    img: &RgbaImage,
    target_height: u32,
    parallel_threshold: u32,
    mut report_progress: impl FnMut(usize, usize),
) -> Result<Vec<(u32, u32)>, String> {
    // Cette heuristique mesure les changements de couleur, sans reconnaître
    // les cases ni les bulles : elle choisit la ligne la plus uniforme près
    // de chaque hauteur cible.
    let (width, height) = img.dimensions();
    let pixels = img.as_raw();
    let step = (width / 700).max(1) as usize;
    let mut bounds = Vec::new();
    let mut top = 0u32;
    while top < height {
        let target = top.saturating_add(target_height).min(height);
        if target >= height {
            bounds.push((top, height));
            break;
        }
        let search_start = target.saturating_sub(80).max(top + 1);
        let search_end = target
            .saturating_add((target_height / 3).max(80))
            .min(height - 1);
        // Les lignes candidates sont indépendantes : leur analyse est parallélisée avec Rayon.
        let score = |y| {
            (
                y,
                row_difference_score(pixels, width as usize, y as usize, step),
            )
        };
        let candidate_count = search_end - search_start + 1;
        let best = if candidate_count >= parallel_threshold {
            (search_start..=search_end)
                .into_par_iter()
                .map(score)
                .min_by_key(|candidate| (candidate.1, candidate.0))
        } else {
            (search_start..=search_end)
                .map(score)
                .min_by_key(|candidate| (candidate.1, candidate.0))
        };
        let best_y = best.map(|(y, _)| y).unwrap_or(target);
        let cut = if best_y <= top { target } else { best_y };
        bounds.push((top, cut));
        top = cut;
        report_progress(top as usize, height as usize);
        if bounds.len() > 10_000 {
            return Err("Trop de pages générées. Vérifie la hauteur demandée.".into());
        }
    }
    Ok(bounds)
}

/// Découpe une image déjà décodée et répartit la progression sur une phase donnée.
pub(crate) fn split_decoded(
    app: &AppHandle,
    img: DynamicImage,
    output: &Path,
    slice_height: u32,
    mode: &str,
    png: bool,
    same_size: bool,
    progress_base: f32,
    progress_span: f32,
) -> Result<usize, String> {
    validate_split_options(slice_height, mode, same_size)?;
    validate_image_size(img.width(), img.height())?;
    fs::create_dir_all(output)
        .map_err(|e| format!("Impossible de créer le dossier de sortie : {e}"))?;
    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return Err("L'image est vide.".into());
    }
    emit_progress(
        app,
        progress_base + progress_span * 0.03,
        "Analyse de l'image…",
    );
    let mut last_analysis_percent = (progress_base + progress_span * 0.03) as u8;
    let bounds = if mode == "smart" {
        smart_boundaries(&rgba, slice_height, 512, |completed, estimated| {
            let fraction = (completed as f32 / estimated as f32).min(1.0);
            let percent = (progress_base + progress_span * (0.03 + 0.17 * fraction)) as u8;
            if percent > last_analysis_percent {
                emit_progress(app, percent as f32, "Recherche des lignes de séparation…");
                last_analysis_percent = percent;
            }
        })?
    } else {
        let count = height.div_ceil(slice_height);
        let actual = if same_size {
            height.div_ceil(count).max(1)
        } else {
            slice_height
        };
        let mut result = Vec::new();
        let mut top = 0u32;
        while top < height {
            let bottom = top.saturating_add(actual).min(height);
            result.push((top, bottom));
            top = bottom;
        }
        result
    };
    let output_phase_start = if mode == "smart" { 0.20 } else { 0.05 };
    emit_progress(
        app,
        progress_base + progress_span * output_phase_start,
        "Préparation des pages…",
    );
    let total = bounds.len();
    let ext = if png { "png" } else { "jpg" };
    let output_paths: Vec<PathBuf> = (0..total)
        .map(|idx| output.join(format!("{:04}.{ext}", idx + 1)))
        .collect();
    let mut last_output_percent = (progress_base + progress_span * output_phase_start) as u8;
    let parallel_batch_size = if (width as u64 * height as u64) >= 10_000_000 && total > 1 {
        2
    } else {
        1
    };
    let mut created_paths = Vec::with_capacity(total);
    for start in (0..total).step_by(parallel_batch_size) {
        let end = (start + parallel_batch_size).min(total);
        let batch = &bounds[start..end];
        let results: Vec<(usize, Result<(), String>)> = if parallel_batch_size == 2 {
            batch
                .par_iter()
                .enumerate()
                .map(|(offset, (top, bottom))| {
                    let idx = start + offset;
                    (
                        idx,
                        save_slice(&rgba, *top, *bottom, &output_paths[idx], png),
                    )
                })
                .collect()
        } else {
            batch
                .iter()
                .enumerate()
                .map(|(offset, (top, bottom))| {
                    let idx = start + offset;
                    (
                        idx,
                        save_slice(&rgba, *top, *bottom, &output_paths[idx], png),
                    )
                })
                .collect()
        };
        let mut failure = None;
        for (idx, result) in results {
            match result {
                Ok(()) => created_paths.push(output_paths[idx].clone()),
                Err(error) if failure.is_none() => failure = Some(error),
                Err(_) => {}
            }
        }
        if let Some(error) = failure {
            for created in &created_paths {
                let _ = fs::remove_file(created);
            }
            return Err(error);
        }
        let fraction = end as f32 / total as f32;
        let percent = (progress_base
            + progress_span * (output_phase_start + fraction * (1.0 - output_phase_start)))
            as u8;
        if percent > last_output_percent || end == total {
            emit_progress(
                app,
                percent as f32,
                format!("Découpe : page {} sur {}", end, total),
            );
            last_output_percent = percent;
        }
    }
    Ok(total)
}

/// Ouvre et valide une source, puis lance une découpe simple vers un dossier.
pub(crate) fn split_image(
    app: &AppHandle,
    source: &Path,
    output: &Path,
    slice_height: u32,
    mode: &str,
    png: bool,
    same_size: bool,
) -> Result<usize, String> {
    emit_progress(app, 1.0, "Ouverture de l'image…");
    ensure_no_existing_pages(output)?;
    let (width, height) = image::image_dimensions(source)
        .map_err(|e| format!("Impossible de lire les dimensions de l'image : {e}"))?;
    validate_image_size(width, height)?;
    let img = image::open(source).map_err(|e| format!("Impossible d'ouvrir l'image : {e}"))?;
    split_decoded(
        app,
        img,
        output,
        slice_height,
        mode,
        png,
        same_size,
        0.0,
        100.0,
    )
}
