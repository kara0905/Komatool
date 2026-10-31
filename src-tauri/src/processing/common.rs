//! Fonctions communes aux pipelines d'image : validation, export et progression.

use image::{
    codecs::{
        jpeg::JpegEncoder,
        png::{CompressionType, FilterType, PngEncoder},
        webp::WebPEncoder,
    },
    DynamicImage, ImageFormat, RgbImage, RgbaImage,
};
use serde::Serialize;
use std::{
    cell::Cell,
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Emitter};

thread_local! {
    static PROGRESS_RANGE: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
}

/// Convertit la progression locale d'une étape en progression du pipeline entier.
pub(crate) struct ProgressRange(Option<(f32, f32)>);

impl ProgressRange {
    pub(crate) fn new(base: f32, span: f32) -> Self {
        Self(PROGRESS_RANGE.with(|range| range.replace(Some((base, span)))))
    }
}

impl Drop for ProgressRange {
    fn drop(&mut self) {
        PROGRESS_RANGE.with(|range| range.set(self.0));
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TaskProgress {
    percent: u8,
    message: String,
}

/// Transmet une étape du traitement à l'interface sans bloquer l'opération.
pub(crate) fn emit_progress(app: &AppHandle, percent: f32, message: impl Into<String>) {
    let percent = PROGRESS_RANGE.with(|range| {
        range
            .get()
            .map(|(base, span)| base + percent * span)
            .unwrap_or(percent)
    });
    let _ = app.emit(
        "task-progress",
        TaskProgress {
            percent: percent.clamp(0.0, 100.0) as u8,
            message: message.into(),
        },
    );
}

pub(crate) fn emit_progress_on_change(
    app: &AppHandle,
    last_percent: &mut u8,
    percent: f32,
    message: impl Into<String>,
) {
    // N'envoie un événement que lorsque le pourcentage entier avance, afin de
    // limiter le trafic entre le moteur natif et l'interface.
    let percent = percent.clamp(0.0, 100.0) as u8;
    if percent > *last_percent {
        emit_progress(app, percent as f32, message);
        *last_percent = percent;
    }
}

/// Encode une image avec les paramètres rapides communs aux exports.
pub(crate) fn encode_image<W: Write>(
    image: &DynamicImage,
    writer: &mut W,
    format: ImageFormat,
) -> Result<(), String> {
    let result = match format {
        ImageFormat::Png => image.write_with_encoder(PngEncoder::new_with_quality(
            writer,
            CompressionType::Fast,
            FilterType::NoFilter,
        )),
        ImageFormat::Jpeg => image.write_with_encoder(JpegEncoder::new_with_quality(writer, 75)),
        ImageFormat::WebP => image.write_with_encoder(WebPEncoder::new_lossless(writer)),
        _ => return Err("Format d'image non pris en charge pour l'export.".into()),
    };
    result.map_err(|error| error.to_string())
}

/// Liste les images d'un dossier et les trie dans l'ordre naturel des pages.
pub(crate) fn image_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Impossible de lire le dossier : {e}"))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|x| x.to_str())
                    .map(|x| {
                        matches!(
                            x.to_ascii_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "tif" | "tiff"
                        )
                    })
                    .unwrap_or(false)
        })
        .collect();
    files.sort_by(|a, b| {
        natural_cmp(
            &a.file_name().unwrap_or_default().to_string_lossy(),
            &b.file_name().unwrap_or_default().to_string_lossy(),
        )
    });
    if files.is_empty() {
        return Err("Aucune image prise en charge dans ce dossier.".into());
    }
    Ok(files)
}

/// Compare les noms en tenant compte des suites de chiffres (page2 avant page10).
pub(super) fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (aa, bb) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    let (mut i, mut j) = (0, 0);
    let (ab, bbytes) = (aa.as_bytes(), bb.as_bytes());
    while i < ab.len() && j < bbytes.len() {
        if ab[i].is_ascii_digit() && bbytes[j].is_ascii_digit() {
            let si = i;
            let sj = j;
            while i < ab.len() && ab[i].is_ascii_digit() {
                i += 1;
            }
            while j < bbytes.len() && bbytes[j].is_ascii_digit() {
                j += 1;
            }
            let na = std::str::from_utf8(&ab[si..i])
                .unwrap_or("0")
                .trim_start_matches('0');
            let nb = std::str::from_utf8(&bbytes[sj..j])
                .unwrap_or("0")
                .trim_start_matches('0');
            let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        } else {
            let ord = ab[i].cmp(&bbytes[j]);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
            i += 1;
            j += 1;
        }
    }
    ab.len().cmp(&bbytes.len())
}

const MAX_IMAGE_PIXELS: u64 = 250_000_000;
const MAX_JPEG_SIDE: u32 = u16::MAX as u32;

/// Refuse les dimensions qui dépassent la limite de côté définie par JPEG.
pub(crate) fn validate_jpeg_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width > MAX_JPEG_SIDE || height > MAX_JPEG_SIDE {
        return Err("Le format JPEG est limité à 65 535 pixels par côté. Choisis PNG ou réduis les dimensions de la planche.".into());
    }
    Ok(())
}

/// Enregistre une bande verticale de l'image dans le format choisi.
pub(crate) fn save_slice(
    img: &RgbaImage,
    top: u32,
    bottom: u32,
    path: &Path,
    png: bool,
) -> Result<(), String> {
    if bottom <= top {
        return Err("Une tranche vide a été détectée.".into());
    }
    save_region(img, 0, top, img.width(), bottom - top, path, png)
}

/// Enregistre une région rectangulaire, sans remplacer un fichier existant.
pub(crate) fn save_region(
    img: &RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    path: &Path,
    png: bool,
) -> Result<(), String> {
    if width == 0
        || height == 0
        || x.saturating_add(width) > img.width()
        || y.saturating_add(height) > img.height()
    {
        return Err("La zone à exporter est invalide.".into());
    }
    if !png {
        validate_jpeg_dimensions(width, height)?;
    }
    let export = if png {
        DynamicImage::ImageRgba8(image::imageops::crop_imm(img, x, y, width, height).to_image())
    } else {
        // Le JPEG n'accepte pas l'alpha : on extrait directement les canaux RVB
        // de la zone utile au lieu de créer une image RGBA temporaire complète.
        let row_bytes = img.width() as usize * 4;
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for row_index in y as usize..(y + height) as usize {
            let row = &img.as_raw()[row_index * row_bytes..(row_index + 1) * row_bytes];
            let left = x as usize * 4;
            let right = (x + width) as usize * 4;
            for pixel in row[left..right].chunks_exact(4) {
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
        let image = RgbImage::from_raw(width, height, rgb)
            .ok_or_else(|| "Impossible de préparer la page JPG.".to_string())?;
        DynamicImage::ImageRgb8(image)
    };
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            format!(
                "Impossible de créer {} sans remplacer un fichier existant : {e}",
                path.display()
            )
        })?;
    let mut writer = BufWriter::with_capacity(256 * 1024, file);
    if let Err(error) = encode_image(
        &export,
        &mut writer,
        if png {
            ImageFormat::Png
        } else {
            ImageFormat::Jpeg
        },
    ) {
        let _ = fs::remove_file(path);
        return Err(format!(
            "Échec de l'export {} : {error}",
            if png { "PNG" } else { "JPG" }
        ));
    }
    if let Err(error) = writer.flush() {
        let _ = fs::remove_file(path);
        return Err(format!(
            "Échec de l'écriture du fichier {} : {error}",
            path.display()
        ));
    }
    Ok(())
}

/// Vérifie la cohérence entre hauteur, mode de découpe et taille uniforme.
pub(crate) fn validate_split_options(
    slice_height: u32,
    mode: &str,
    same_size: bool,
) -> Result<(), String> {
    if mode != "grid" && !(100..=1_000_000).contains(&slice_height) {
        return Err("La hauteur doit être comprise entre 100 et 1 000 000 pixels.".into());
    }
    if !matches!(mode, "fixed" | "smart" | "manual" | "grid") {
        return Err("Mode de découpe non pris en charge.".into());
    }
    if mode == "smart" && same_size {
        return Err(
            "Le découpage intelligent ne peut pas être combiné avec des pages de même taille."
                .into(),
        );
    }
    Ok(())
}

/// Limite les images à 250 millions de pixels pour borner la mémoire utilisée.
pub(crate) fn validate_image_size(width: u32, height: u32) -> Result<(), String> {
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .ok_or_else(|| "Dimensions de l'image trop importantes.".to_string())?;
    if pixels > MAX_IMAGE_PIXELS {
        return Err(
            "Image trop grande pour le traitement en mémoire (limite : 250 millions de pixels)."
                .into(),
        );
    }
    Ok(())
}

fn find_existing_numbered_page(output: &Path) -> Result<Option<PathBuf>, String> {
    for entry in fs::read_dir(output)
        .map_err(|e| format!("Impossible de vérifier le dossier de sortie : {e}"))?
    {
        let path = entry
            .map_err(|e| format!("Impossible de lire le contenu du dossier de sortie : {e}"))?
            .path();
        let numbered = path
            .file_stem()
            .and_then(|name| name.to_str())
            .map(|stem| stem.len() >= 4 && stem.bytes().all(|byte| byte.is_ascii_digit()))
            .unwrap_or(false);
        let supported = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
            .unwrap_or(false);
        if path.is_file() && numbered && supported {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Refuse un dossier qui contient déjà des pages numérotées prises en charge.
pub(crate) fn ensure_no_existing_pages(output: &Path) -> Result<(), String> {
    // La découpe produit des noms numérotés ; empêcher la réutilisation d'un
    // dossier évite de mélanger silencieusement deux séries de pages.
    if !output.exists() {
        return Ok(());
    }
    if !output.is_dir() {
        return Err(format!(
            "Le chemin de sortie {} n'est pas un dossier.",
            output.display()
        ));
    }
    if let Some(existing) = find_existing_numbered_page(output)? {
        return Err(format!("Une page numérotée existe déjà dans {} (ex. {}). Choisis un dossier de sortie sans anciennes pages.", output.display(), existing.file_name().unwrap_or_default().to_string_lossy()));
    }
    Ok(())
}
