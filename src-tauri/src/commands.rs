//! Commandes Tauri appelées depuis l'interface Svelte.
//!
//! La validation des paramètres commence ici ; les traitements d'image sont
//! délégués aux modules `processing` et aux tâches `spawn_blocking` pour ne
//! pas bloquer le fil d'affichage.

use crate::processing::{
    advanced_split::{self, AdvancedSplitRequest},
    archive, common, convert, merge,
    pipeline::{self, PipelineRequest},
    preview, split,
    transform::ImageAdjustments,
    waifu,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;

struct TemporaryImage(PathBuf);

impl Drop for TemporaryImage {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[tauri::command]
pub async fn waifu2x_process(
    app: AppHandle,
    files: Vec<String>,
    output: String,
    scale: u32,
    noise_level: i32,
    model: String,
    device: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        waifu::process(
            &app,
            &files,
            Path::new(&output),
            scale,
            noise_level,
            &model,
            &device,
        )?;
        common::emit_progress(&app, 100.0, "Amélioration Waifu2x terminée");
        Ok(format!("{} images améliorées avec Waifu2x.", files.len()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn list_images(folder: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        common::image_files_in_dir(Path::new(&folder)).map(|v| {
            v.into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn split(
    app: AppHandle,
    source: String,
    output: String,
    height: u32,
    mode: String,
    png: bool,
    same_size: bool,
) -> Result<String, String> {
    common::validate_split_options(height, &mode, same_size)?;
    tauri::async_runtime::spawn_blocking(move || {
        let n = split::split_image(
            &app,
            Path::new(&source),
            Path::new(&output),
            height,
            &mode,
            png,
            same_size,
        )?;
        Ok(format!("{} pages créées avec succès.", n))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn merge(app: AppHandle, files: Vec<String>, output: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        merge::merge_images(
            &app,
            &files,
            Path::new(&output),
            &ImageAdjustments::default(),
        )?;
        Ok("Fusion terminée avec succès.".into())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn merge_advanced(
    app: AppHandle,
    files: Vec<String>,
    output: String,
    adjustments: ImageAdjustments,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        merge::merge_images(&app, &files, Path::new(&output), &adjustments)?;
        Ok("Fusion terminée avec succès.".into())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn merge_and_split(
    app: AppHandle,
    files: Vec<String>,
    output: String,
    height: u32,
    mode: String,
    png: bool,
    same_size: bool,
    adjustments: ImageAdjustments,
) -> Result<String, String> {
    common::validate_split_options(height, &mode, same_size)?;
    tauri::async_runtime::spawn_blocking(move || {
        common::ensure_no_existing_pages(Path::new(&output))?;
        if merge::requires_mapped_processing(&files, &adjustments)? {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let merged = TemporaryImage(std::env::temp_dir().join(format!(
                "komatool-fusion-decoupe-{}-{stamp}.png",
                std::process::id()
            )));
            merge::merge_images(&app, &files, &merged.0, &adjustments)?;
            common::emit_progress(&app, 92.0, "Préparation du découpage par blocs…");
            let request = AdvancedSplitRequest {
                source: merged.0.to_string_lossy().into_owned(),
                output: output.clone(),
                height,
                mode: mode.clone(),
                png,
                same_size,
                rows: 1,
                columns: 1,
                prefix: "page_".into(),
                start_number: 1,
                adjustments: ImageAdjustments::default(),
            };
            let count = advanced_split::run(&app, &request, None, 92.0, 8.0)?;
            return Ok(format!(
                "Fusion et découpe terminées : {count} pages créées."
            ));
        }
        // Pour les formats usuels, garder le chemin direct évite l'écriture puis la relecture.
        let merged = merge::compose_images(&app, &files, false, &adjustments)?;
        common::emit_progress(&app, 92.0, "Préparation du découpage…");
        let count = split::split_decoded(
            &app,
            merged,
            Path::new(&output),
            height,
            &mode,
            png,
            same_size,
            92.0,
            8.0,
        )?;
        Ok(format!(
            "Fusion et découpe terminées : {count} pages créées."
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn preview_split(
    source: String,
    height: u32,
    mode: String,
    same_size: bool,
    rows: u32,
    columns: u32,
    adjustments: ImageAdjustments,
) -> Result<preview::SplitPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        preview::create(
            Path::new(&source),
            height,
            &mode,
            same_size,
            rows,
            columns,
            &adjustments,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn split_advanced(
    app: AppHandle,
    request: AdvancedSplitRequest,
) -> Result<String, String> {
    common::validate_split_options(request.height, &request.mode, request.same_size)?;
    tauri::async_runtime::spawn_blocking(move || {
        let count = advanced_split::run(&app, &request, None, 0.0, 100.0)?;
        Ok(format!("{count} pages exportées."))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn batch_split(
    app: AppHandle,
    requests: Vec<AdvancedSplitRequest>,
) -> Result<String, String> {
    if requests.is_empty() {
        return Err("Ajoute au moins une image à traiter.".into());
    }
    if requests.len() > 500 {
        return Err("Le traitement par lot est limité à 500 images.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let total = requests.len();
        for (index, request) in requests.iter().enumerate() {
            common::validate_split_options(request.height, &request.mode, request.same_size)?;
            let filename = Path::new(&request.source)
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("image");
            let safe_name: String = filename
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let output = Path::new(&request.output).join(format!("{:03}_{}", index + 1, safe_name));
            let span = 100.0 / total as f32;
            advanced_split::run(&app, request, Some(&output), index as f32 * span, span)?;
        }
        Ok(format!("Traitement terminé : {total} images traitées."))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_reading(
    app: AppHandle,
    files: Vec<String>,
    output: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        archive::export(&files, Path::new(&output), &app, None)?;
        common::emit_progress(&app, 100.0, "Export terminé");
        Ok("Export créé avec succès.".into())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_cbz_metadata(
    app: AppHandle,
    files: Vec<String>,
    output: String,
    title: String,
    series: String,
    number: String,
    writer: String,
    summary: String,
    year: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if Path::new(&output)
            .extension()
            .and_then(|v| v.to_str())
            .map(|v| v.eq_ignore_ascii_case("cbz"))
            != Some(true)
        {
            return Err(
                "Les métadonnées ComicInfo.xml sont disponibles pour les fichiers CBZ.".into(),
            );
        }
        let metadata = archive::ComicMetadata {
            title,
            series,
            number,
            writer,
            summary,
            year,
        };
        archive::export(&files, Path::new(&output), &app, Some(&metadata))?;
        common::emit_progress(&app, 100.0, "CBZ avec métadonnées créé");
        Ok("CBZ créé avec ses métadonnées ComicInfo.xml.".into())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn convert_images(
    app: AppHandle,
    files: Vec<String>,
    output: String,
    format: String,
    quality: u8,
) -> Result<String, String> {
    let format = convert::parse_format(&format)?;
    tauri::async_runtime::spawn_blocking(move || {
        convert::process(&app, &files, Path::new(&output), format, quality)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn run_pipeline(
    app: AppHandle,
    files: Vec<String>,
    output: String,
    pipeline: PipelineRequest,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        pipeline::run(&app, &files, Path::new(&output), &pipeline)
    })
    .await
    .map_err(|error| error.to_string())?
}
