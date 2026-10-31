//! Commandes Tauri appelées depuis l'interface Svelte.
//!
//! La validation des paramètres commence ici ; les traitements d'image sont
//! délégués aux modules `processing` et aux tâches `spawn_blocking` pour ne
//! pas bloquer le fil d'affichage.

use crate::processing::{
    advanced_split::{self, AdvancedSplitRequest},
    archive, common, convert, merge, preview, split,
    transform::ImageAdjustments,
};
use std::path::Path;
use tauri::{AppHandle, Manager};

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
        if files.is_empty() {
            return Err("Ajoute au moins une image à améliorer.".into());
        }
        if !matches!(scale, 2 | 4) || !(-1..=3).contains(&noise_level) {
            return Err("Les réglages Waifu2x ne sont pas valides.".into());
        }
        if !matches!(device.as_str(), "auto" | "cpu") {
            return Err("Le mode de calcul Waifu2x n'est pas valide.".into());
        }
        let model_folder = match model.as_str() {
            "cunet" => "models-cunet",
            "anime" => "models-upconv_7_anime_style_art_rgb",
            _ => return Err("Le modèle Waifu2x demandé n'existe pas.".into()),
        };
        let resources = app
            .path()
            .resource_dir()
            .map_err(|error| format!("Impossible de localiser les ressources : {error}"))?
            .join("waifu2x");
        let binary = resources.join(if cfg!(windows) {
            "waifu2x-ncnn-vulkan.exe"
        } else {
            "waifu2x-ncnn-vulkan"
        });
        let model_path = resources.join(model_folder);
        if !binary.is_file() || !model_path.is_dir() {
            return Err("Le moteur Waifu2x intégré est introuvable. Réinstalle KomaTool.".into());
        }

        let output_dir = Path::new(&output);
        std::fs::create_dir_all(output_dir)
            .map_err(|error| format!("Impossible de créer le dossier de sortie : {error}"))?;
        let mut jobs = Vec::with_capacity(files.len());
        let mut outputs = std::collections::HashSet::new();
        for (index, file) in files.iter().enumerate() {
            let input = Path::new(file);
            let extension = input
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                return Err(format!(
                    "Format non pris en charge par Waifu2x : {}.",
                    input.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            let stem = input
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty())
                .unwrap_or("image");
            let destination = output_dir.join(format!("{:03}_{stem}_x{scale}.png", index + 1));
            if !outputs.insert(destination.clone()) || destination.exists() {
                return Err(format!(
                    "Le fichier de sortie existe déjà : {}",
                    destination
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ));
            }
            jobs.push((input.to_path_buf(), destination));
        }

        let total = jobs.len();
        for (index, (input, destination)) in jobs.iter().enumerate() {
            let filename = input.file_name().unwrap_or_default().to_string_lossy();
            common::emit_progress(
                &app,
                index as f32 / total as f32 * 100.0,
                format!("Amélioration {}/{} : {}", index + 1, total, filename),
            );
            let mut process = std::process::Command::new(&binary);
            process
                .arg("-i")
                .arg(input)
                .arg("-o")
                .arg(destination)
                .arg("-n")
                .arg(noise_level.to_string())
                .arg("-s")
                .arg(scale.to_string())
                .arg("-m")
                .arg(&model_path);
            if device == "cpu" {
                process.arg("-g").arg("-1");
            }
            let result = process
                .output()
                .map_err(|error| format!("Impossible de lancer Waifu2x : {error}"))?;
            if !result.status.success() {
                let details = String::from_utf8_lossy(&result.stderr).trim().to_owned();
                let _ = std::fs::remove_file(destination);
                return Err(if details.is_empty() {
                    format!("Waifu2x a échoué pour {filename}.")
                } else {
                    format!("Waifu2x a échoué pour {filename} : {details}")
                });
            }
        }
        common::emit_progress(&app, 100.0, "Amélioration Waifu2x terminée");
        Ok(format!("{} images améliorées avec Waifu2x.", total))
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
        // Évite l'écriture puis la relecture d'un énorme fichier temporaire.
        common::ensure_no_existing_pages(Path::new(&output))?;
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
