//! Découpe avec réglages partagés, grille, préfixe et numérotation personnalisés.

use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

use super::{
    common::{
        emit_progress, ensure_no_existing_pages, save_region, validate_image_size,
        validate_split_options,
    },
    split::smart_boundaries,
    transform::{self, ImageAdjustments},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedSplitRequest {
    pub source: String,
    pub output: String,
    pub height: u32,
    pub mode: String,
    pub png: bool,
    pub same_size: bool,
    pub rows: u32,
    pub columns: u32,
    pub prefix: String,
    pub start_number: u32,
    pub adjustments: ImageAdjustments,
}

/// Calcule les séparations régulières et répartit le reliquat si les pages
/// doivent avoir une hauteur uniforme.
fn cuts_for_height(height: u32, target: u32, same_size: bool) -> Vec<u32> {
    let count = height.div_ceil(target);
    let page_height = if same_size {
        height.div_ceil(count).max(1)
    } else {
        target
    };
    (1..count)
        .map(|index| index * page_height)
        .filter(|cut| *cut < height)
        .collect()
}

/// Applique les réglages de découpe, puis exporte les tuiles dans l'ordre de lecture.
pub fn run(
    app: &AppHandle,
    request: &AdvancedSplitRequest,
    output_override: Option<&Path>,
    progress_base: f32,
    progress_span: f32,
) -> Result<usize, String> {
    validate_split_options(request.height, &request.mode, request.same_size)?;
    if request.mode == "grid"
        && (request.rows == 0
            || request.rows > 100
            || request.columns == 0
            || request.columns > 100)
    {
        return Err("La grille doit contenir entre 1 et 100 lignes et colonnes.".into());
    }
    if request.prefix.len() > 40
        || !request
            .prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return Err("Le préfixe de nommage accepte uniquement lettres, chiffres, tirets et soulignés (40 caractères max).".into());
    }
    let source = Path::new(&request.source);
    let output = output_override.unwrap_or_else(|| Path::new(&request.output));
    ensure_no_existing_pages(output)?;
    let (source_width, source_height) = image::image_dimensions(source)
        .map_err(|e| format!("Impossible de lire les dimensions : {e}"))?;
    validate_image_size(source_width, source_height)?;
    let image = transform::apply(
        image::open(source).map_err(|e| format!("Impossible d'ouvrir l'image : {e}"))?,
        &request.adjustments,
    )?;
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();

    let (horizontal, vertical) = if request.mode == "grid" {
        let horizontal = (1..request.rows)
            .map(|row| height * row / request.rows)
            .collect();
        let vertical = (1..request.columns)
            .map(|col| width * col / request.columns)
            .collect();
        (horizontal, vertical)
    } else {
        let horizontal = if request.mode == "smart" {
            smart_boundaries(&rgba, request.height, 512, |_, _| {})?
                .into_iter()
                .filter_map(|(_, bottom)| (bottom < height).then_some(bottom))
                .collect()
        } else {
            cuts_for_height(height, request.height, request.same_size)
        };
        (horizontal, Vec::new())
    };

    let mut y_edges = Vec::with_capacity(horizontal.len() + 2);
    y_edges.push(0);
    y_edges.extend(horizontal);
    y_edges.push(height);
    let mut x_edges = Vec::with_capacity(vertical.len() + 2);
    x_edges.push(0);
    x_edges.extend(vertical);
    x_edges.push(width);

    let tile_count = (y_edges.len() - 1) * (x_edges.len() - 1);
    if tile_count > 10_000 {
        return Err("Cette grille générerait plus de 10 000 fichiers.".into());
    }
    let ext = if request.png { "png" } else { "jpg" };
    let mut paths = Vec::with_capacity(tile_count);
    for index in 0..tile_count {
        let number = request
            .start_number
            .checked_add(index as u32)
            .ok_or_else(|| "Numérotation trop importante.".to_string())?;
        paths.push(output.join(format!("{}{:04}.{ext}", request.prefix, number)));
    }
    if let Some(path) = paths.iter().find(|path| path.exists()) {
        return Err(format!(
            "Le fichier {} existe déjà. Choisis un autre préfixe ou un dossier vide.",
            path.display()
        ));
    }
    fs::create_dir_all(output)
        .map_err(|e| format!("Impossible de créer le dossier de sortie : {e}"))?;
    let mut created: Vec<PathBuf> = Vec::with_capacity(paths.len());
    for row in 0..y_edges.len() - 1 {
        for column in 0..x_edges.len() - 1 {
            let index = row * (x_edges.len() - 1) + column;
            if let Err(error) = save_region(
                &rgba,
                x_edges[column],
                y_edges[row],
                x_edges[column + 1] - x_edges[column],
                y_edges[row + 1] - y_edges[row],
                &paths[index],
                request.png,
            ) {
                for path in created {
                    let _ = fs::remove_file(path);
                }
                return Err(error);
            }
            created.push(paths[index].clone());
            emit_progress(
                app,
                progress_base + (index + 1) as f32 / tile_count as f32 * progress_span,
                format!("Export : {} / {}", index + 1, tile_count),
            );
        }
    }
    Ok(tile_count)
}
