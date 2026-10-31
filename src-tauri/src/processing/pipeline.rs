//! Exécution séquentielle d'un pipeline d'images à partir des outils existants.

use super::{
    advanced_split::{self, AdvancedSplitRequest},
    archive, common, convert, merge,
    transform::ImageAdjustments,
    waifu,
};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PipelineStep {
    Upscale {
        scale: u32,
        noise_level: i32,
        model: String,
        device: String,
    },
    Convert {
        format: String,
        quality: u8,
    },
    Optimize {
        lossless: bool,
        quality: u8,
    },
    Split {
        height: u32,
        mode: String,
        png: bool,
        same_size: bool,
        rows: u32,
        columns: u32,
        #[serde(default)]
        adjustments: ImageAdjustments,
    },
    Merge {
        format: String,
        #[serde(default)]
        adjustments: ImageAdjustments,
    },
    Export {
        format: String,
        #[serde(default)]
        include_comic_info: bool,
        #[serde(default)]
        title: String,
        #[serde(default)]
        series: String,
        #[serde(default)]
        number: String,
        #[serde(default)]
        writer: String,
        #[serde(default)]
        summary: String,
        #[serde(default)]
        year: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRequest {
    pub name: String,
    pub steps: Vec<PipelineStep>,
    #[serde(default)]
    pub separate_step_folders: bool,
}

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn create() -> Result<Self, String> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir();
        for _ in 0..10 {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = root.join(format!(
                "komatool-pipeline-{}-{stamp}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Impossible de préparer le pipeline : {error}")),
            }
        }
        Err("Impossible de créer un dossier temporaire pour le pipeline.".into())
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn safe_name(name: &str) -> String {
    let value: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    if value.is_empty() {
        "pipeline".into()
    } else {
        value
    }
}

fn step_folder_name(step: &PipelineStep) -> &'static str {
    match step {
        PipelineStep::Upscale { .. } => "upscale",
        PipelineStep::Convert { .. } => "conversion",
        PipelineStep::Optimize { .. } => "optimisation",
        PipelineStep::Split { .. } => "decoupe",
        PipelineStep::Merge { .. } => "fusion",
        PipelineStep::Export { .. } => "export",
    }
}

/// Enchaîne les opérations sur la collection complète produite par chaque étape.
pub fn run(
    app: &AppHandle,
    input_files: &[String],
    output: &Path,
    pipeline: &PipelineRequest,
) -> Result<String, String> {
    if input_files.is_empty() {
        return Err("Ajoute au moins une image au pipeline.".into());
    }
    if input_files.len() > 500 {
        return Err("Un pipeline est limité à 500 images en entrée.".into());
    }
    if pipeline.steps.is_empty() || pipeline.steps.len() > 20 {
        return Err("Un pipeline doit contenir de 1 à 20 étapes.".into());
    }
    if pipeline
        .steps
        .iter()
        .take(pipeline.steps.len() - 1)
        .any(|step| matches!(step, PipelineStep::Export { .. }))
    {
        return Err("L'export CBZ ou PDF doit être la dernière étape du pipeline.".into());
    }
    for file in input_files {
        if !Path::new(file).is_file() {
            return Err(format!("Fichier d'entrée introuvable : {file}"));
        }
    }
    fs::create_dir_all(output)
        .map_err(|error| format!("Impossible de créer le dossier de sortie : {error}"))?;

    let temporary = TemporaryDirectory::create()?;
    let label = safe_name(&pipeline.name);
    let mut files = input_files.to_vec();
    let step_count = pipeline.steps.len();

    for (index, step) in pipeline.steps.iter().enumerate() {
        let _progress_range =
            common::ProgressRange::new(index as f32 / step_count as f32, 1.0 / step_count as f32);
        let is_last = index + 1 == step_count;
        let step_dir = if pipeline.separate_step_folders {
            output.join(format!("{:02}_{}", index + 1, step_folder_name(step)))
        } else if is_last {
            output.to_path_buf()
        } else {
            temporary.0.join(format!("etape_{:02}", index + 1))
        };
        fs::create_dir_all(&step_dir)
            .map_err(|error| format!("Impossible de préparer l'étape {} : {error}", index + 1))?;
        common::emit_progress(
            app,
            index as f32 / step_count as f32 * 100.0,
            format!("Étape {}/{} : préparation", index + 1, step_count),
        );

        match step {
            PipelineStep::Upscale {
                scale,
                noise_level,
                model,
                device,
            } => {
                files =
                    waifu::process(app, &files, &step_dir, *scale, *noise_level, model, device)?;
            }
            PipelineStep::Convert { format, quality } => {
                let format = convert::parse_format(format)?;
                convert::process(app, &files, &step_dir, format, *quality)?;
                files = common::image_files_in_dir(&step_dir)?
                    .into_iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
            }
            PipelineStep::Optimize { lossless, quality } => {
                let format = if *lossless { "png" } else { "jpg" };
                convert::process(
                    app,
                    &files,
                    &step_dir,
                    convert::parse_format(format)?,
                    *quality,
                )?;
                files = common::image_files_in_dir(&step_dir)?
                    .into_iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
            }
            PipelineStep::Split {
                height,
                mode,
                png,
                same_size,
                rows,
                columns,
                adjustments,
            } => {
                common::validate_split_options(*height, mode, *same_size)?;
                if mode == "grid" && (*rows == 0 || *rows > 100 || *columns == 0 || *columns > 100)
                {
                    return Err("La grille doit contenir entre 1 et 100 lignes et colonnes.".into());
                }
                let mut pages = Vec::new();
                for (file_index, source) in files.iter().enumerate() {
                    let stem = Path::new(source)
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or("image");
                    let source_dir =
                        step_dir.join(format!("{:03}_{}", file_index + 1, safe_name(stem)));
                    let request = AdvancedSplitRequest {
                        source: source.clone(),
                        output: source_dir.to_string_lossy().into_owned(),
                        height: *height,
                        mode: mode.clone(),
                        png: *png,
                        same_size: *same_size,
                        rows: *rows,
                        columns: *columns,
                        prefix: "page_".into(),
                        start_number: 1,
                        adjustments: adjustments.clone(),
                    };
                    advanced_split::run(app, &request, None, 0.0, 100.0)?;
                    pages.extend(
                        common::image_files_in_dir(&source_dir)?
                            .into_iter()
                            .map(|path| path.to_string_lossy().into_owned()),
                    );
                }
                files = pages;
            }
            PipelineStep::Merge {
                format,
                adjustments,
            } => {
                if files.len() < 2 {
                    return Err("L'étape Fusion nécessite au moins deux images à réunir.".into());
                }
                let extension = match format.as_str() {
                    "png" | "jpg" | "jpeg" | "webp" => format.as_str(),
                    _ => return Err("Le format de fusion doit être PNG, JPG ou WebP.".into()),
                };
                let merged = step_dir.join(format!("{label}_fusion.{extension}"));
                merge::merge_images(app, &files, &merged, adjustments)?;
                files = vec![merged.to_string_lossy().into_owned()];
            }
            PipelineStep::Export {
                format,
                include_comic_info,
                title,
                series,
                number,
                writer,
                summary,
                year,
            } => {
                let extension = match format.as_str() {
                    "cbz" | "pdf" => format.as_str(),
                    _ => return Err("Le format d'export doit être CBZ ou PDF.".into()),
                };
                if *include_comic_info && extension != "cbz" {
                    return Err("Les métadonnées ComicInfo sont réservées aux exports CBZ.".into());
                }
                let destination = step_dir.join(format!("{label}.{extension}"));
                let metadata = include_comic_info.then(|| archive::ComicMetadata {
                    title: title.clone(),
                    series: series.clone(),
                    number: number.clone(),
                    writer: writer.clone(),
                    summary: summary.clone(),
                    year: year.clone(),
                });
                archive::export(&files, &destination, app, metadata.as_ref())?;
                common::emit_progress(app, 100.0, "Export du pipeline terminé");
                return Ok(format!(
                    "Pipeline « {} » terminé : {} images traitées.",
                    pipeline.name,
                    input_files.len()
                ));
            }
        }
        if files.is_empty() {
            return Err(format!("L'étape {} n'a produit aucune image.", index + 1));
        }
    }

    common::emit_progress(app, 100.0, "Pipeline terminé");
    Ok(format!(
        "Pipeline « {} » terminé : {} fichiers générés.",
        pipeline.name,
        files.len()
    ))
}
