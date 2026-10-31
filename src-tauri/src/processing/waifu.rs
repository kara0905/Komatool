//! Adaptateur du moteur Waifu2x intégré aux espaces de travail et aux pipelines.

use super::common;
use std::{collections::HashSet, fs, path::Path, process::Command};
use tauri::{AppHandle, Manager};

/// Lance une tentative avec une taille de tuile donnée (ou le réglage automatique).
fn run_attempt(
    binary: &Path,
    resources: &Path,
    input: &Path,
    destination: &Path,
    noise_level: i32,
    scale: u32,
    model_folder: &str,
    device: &str,
    tile_size: Option<u32>,
) -> Result<std::process::Output, String> {
    let mut command = Command::new(binary);
    command
        // Le moteur résout les modèles depuis son dossier courant ; un chemin absolu Windows
        // serait sinon préfixé une seconde fois par Waifu2x.
        .current_dir(resources)
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(destination)
        .arg("-n")
        .arg(noise_level.to_string())
        .arg("-s")
        .arg(scale.to_string())
        .arg("-m")
        .arg(model_folder);
    if let Some(tile_size) = tile_size {
        command.arg("-t").arg(tile_size.to_string());
    }
    if device == "cpu" {
        command.arg("-g").arg("-1");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Évite l'ouverture d'une fenêtre console pour le moteur natif.
        command.creation_flags(0x0800_0000);
    }
    command
        .output()
        .map_err(|error| format!("Impossible de lancer Waifu2x : {error}"))
}

/// Repère les erreurs de mémoire GPU signalées par Vulkan ou Waifu2x.
fn is_memory_error(output: &std::process::Output) -> bool {
    let details = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();
    [
        "memory exceed",
        "out of memory",
        "failed to allocate",
        "error_out_of_device_memory",
        "vk_error_out_of_device_memory",
    ]
    .iter()
    .any(|marker| details.contains(marker))
}

/// Améliore une collection d'images et renvoie les chemins des PNG produits.
pub fn process(
    app: &AppHandle,
    files: &[String],
    output: &Path,
    scale: u32,
    noise_level: i32,
    model: &str,
    device: &str,
) -> Result<Vec<String>, String> {
    if files.is_empty() {
        return Err("Ajoute au moins une image à améliorer.".into());
    }
    if !matches!(scale, 1 | 2 | 4) || !(-1..=3).contains(&noise_level) {
        return Err("Les réglages Waifu2x ne sont pas valides.".into());
    }
    if !matches!(device, "auto" | "cpu") {
        return Err("Le mode de calcul Waifu2x n'est pas valide.".into());
    }
    let model_folder = match model {
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
    if !binary.is_file() || !resources.join(model_folder).is_dir() {
        return Err("Le moteur Waifu2x intégré est introuvable. Réinstalle KomaTool.".into());
    }

    fs::create_dir_all(output)
        .map_err(|error| format!("Impossible de créer le dossier de sortie : {error}"))?;
    let mut jobs = Vec::with_capacity(files.len());
    let mut destinations = HashSet::new();
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
        let suffix = if scale == 1 {
            "denoised".to_string()
        } else {
            format!("x{scale}")
        };
        let destination = output.join(format!("{:03}_{stem}_{suffix}.png", index + 1));
        if !destinations.insert(destination.clone()) || destination.exists() {
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
    let mut outputs = Vec::with_capacity(total);
    for (index, (input, destination)) in jobs.iter().enumerate() {
        let filename = input.file_name().unwrap_or_default().to_string_lossy();
        common::emit_progress(
            app,
            index as f32 / total as f32 * 100.0,
            format!("Amélioration {}/{} : {}", index + 1, total, filename),
        );
        // Une tuile plus petite limite fortement la mémoire GPU requise. On ne fait ces
        // nouvelles tentatives que si le moteur signale explicitement une saturation mémoire.
        let tile_sizes = if device == "auto" {
            vec![None, Some(256), Some(128), Some(64), Some(32)]
        } else {
            vec![None]
        };
        let mut last_result = None;
        for (attempt, tile_size) in tile_sizes.iter().enumerate() {
            if attempt > 0 {
                common::emit_progress(
                    app,
                    index as f32 / total as f32 * 100.0,
                    format!(
                        "Mémoire GPU limitée : nouvel essai avec des blocs de {} px",
                        tile_size.unwrap_or_default()
                    ),
                );
                let _ = fs::remove_file(destination);
            }
            let result = run_attempt(
                &binary,
                &resources,
                input,
                destination,
                noise_level,
                scale,
                model_folder,
                device,
                *tile_size,
            )?;
            if result.status.success() {
                last_result = Some(result);
                break;
            }
            let retryable = is_memory_error(&result);
            last_result = Some(result);
            if !retryable || attempt + 1 == tile_sizes.len() {
                break;
            }
        }
        let result = last_result.expect("au moins une tentative Waifu2x doit être effectuée");
        if !result.status.success() {
            let details = String::from_utf8_lossy(&result.stderr).trim().to_owned();
            let _ = fs::remove_file(destination);
            return Err(if is_memory_error(&result) {
                format!(
                    "Mémoire GPU insuffisante pour traiter {filename}, même avec des blocs de 32 px. Essaie le mode CPU ou réduis la taille de l'image. {}",
                    details
                )
            } else if details.is_empty() {
                format!("Waifu2x a échoué pour {filename}.")
            } else {
                format!("Waifu2x a échoué pour {filename} : {details}")
            });
        }
        outputs.push(destination.to_string_lossy().into_owned());
    }
    Ok(outputs)
}
