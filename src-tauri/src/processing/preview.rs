use base64::{engine::general_purpose::STANDARD, Engine};
use image::{imageops, DynamicImage, ImageFormat, RgbaImage};
use serde::Serialize;
use std::{io::Cursor, path::Path};

use super::{
    common::{encode_image, exceeds_in_memory_limit, validate_image_size},
    mapped_image::MappedRgba,
    split::{smart_boundaries, smart_boundaries_raw},
    transform::{self, ImageAdjustments},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitPreview {
    pub width: u32,
    pub height: u32,
    pub pages: Vec<PagePreview>,
    pub pages_truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagePreview {
    pub number: usize,
    pub width: u32,
    pub height: u32,
    pub data_url: String,
}

/// Encode une vignette bornée pour garder l'aperçu rapide sur les longues planches.
fn encode_preview(image: &RgbaImage, max_width: u32, max_height: u32) -> Result<String, String> {
    let (width, height) = image.dimensions();
    let scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64);
    let thumb_width = ((width as f64 * scale).round() as u32).max(1);
    let thumb_height = ((height as f64 * scale).round() as u32).max(1);
    let thumb = imageops::thumbnail(image, thumb_width, thumb_height);
    let mut png = Cursor::new(Vec::new());
    encode_image(&DynamicImage::ImageRgba8(thumb), &mut png, ImageFormat::Png)?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(png.into_inner())
    ))
}

fn map_edge(edge: u32, source_size: u32, preview_size: u32) -> u32 {
    ((edge as u64 * preview_size as u64) / source_size as u64) as u32
}

/// Prépare les dimensions et les vignettes utilisées par l'aperçu de découpe.
pub fn create(
    source: &Path,
    height: u32,
    mode: &str,
    same_size: bool,
    rows: u32,
    columns: u32,
    adjustments: &ImageAdjustments,
) -> Result<SplitPreview, String> {
    // L'aperçu réutilise les réglages et les séparations de l'export, mais ne
    // produit que de petites vignettes et n'écrit aucun fichier de sortie.
    let (source_width, source_height) = image::image_dimensions(source)
        .map_err(|e| format!("Impossible de lire les dimensions : {e}"))?;
    let large = exceeds_in_memory_limit(source_width, source_height);
    let (image, mapped) = if large {
        if adjustments.resize_width.is_some() {
            return Err("L'aperçu d'une très grande image ne peut pas appliquer le redimensionnement sans dépasser la mémoire.".into());
        }
        let decoded = MappedRgba::open(source)?;
        let mapped = decoded.crop_and_margin(
            adjustments.crop_left,
            adjustments.crop_top,
            adjustments.crop_right,
            adjustments.crop_bottom,
            adjustments.margin,
        )?;
        (None, Some(mapped))
    } else {
        validate_image_size(source_width, source_height)?;
        let decoded =
            image::open(source).map_err(|e| format!("Impossible d'ouvrir l'image : {e}"))?;
        let image = transform::apply(decoded, adjustments)?.into_rgba8();
        (Some(image), None)
    };
    let (width, image_height) = if let Some(image) = image.as_ref() {
        image.dimensions()
    } else {
        let mapped = mapped.as_ref().expect("image mappée");
        (mapped.width, mapped.height)
    };
    let (horizontal_cuts, vertical_cuts): (Vec<u32>, Vec<u32>) = match mode {
        "grid" => {
            if rows == 0 || rows > 100 || columns == 0 || columns > 100 {
                return Err("La grille doit contenir entre 1 et 100 lignes et colonnes.".into());
            }
            (
                (1..rows).map(|row| image_height * row / rows).collect(),
                (1..columns).map(|col| width * col / columns).collect(),
            )
        }
        "smart" => {
            if !(100..=1_000_000).contains(&height) {
                return Err("La hauteur doit être comprise entre 100 et 1 000 000 pixels.".into());
            }
            let cuts = if let Some(mapped) = &mapped {
                smart_boundaries_raw(
                    mapped.bytes(),
                    width,
                    image_height,
                    height,
                    u32::MAX,
                    |_, _| {},
                )?
            } else {
                smart_boundaries(
                    image.as_ref().expect("image standard"),
                    height,
                    512,
                    |_, _| {},
                )?
            }
            .into_iter()
            .take_while(|(_, bottom)| *bottom < image_height)
            .map(|(_, bottom)| bottom)
            .collect();
            (cuts, Vec::new())
        }
        "manual" => {
            if !(100..=1_000_000).contains(&height) {
                return Err("La hauteur doit être comprise entre 100 et 1 000 000 pixels.".into());
            }
            let count = image_height.div_ceil(height);
            let page_height = if same_size {
                image_height.div_ceil(count).max(1)
            } else {
                height
            };
            (
                (1..count)
                    .map(|index| index * page_height)
                    .filter(|cut| *cut < image_height)
                    .collect(),
                Vec::new(),
            )
        }
        _ => {
            if !(100..=1_000_000).contains(&height) {
                return Err("La hauteur doit être comprise entre 100 et 1 000 000 pixels.".into());
            }
            let count = image_height.div_ceil(height);
            let page_height = if same_size {
                image_height.div_ceil(count).max(1)
            } else {
                height
            };
            let cuts = (1..count)
                .map(|index| (index * page_height).min(image_height))
                .filter(|cut| *cut < image_height)
                .collect();
            (cuts, Vec::new())
        }
    };

    let mut y_edges = Vec::with_capacity(horizontal_cuts.len() + 2);
    y_edges.push(0);
    y_edges.extend(&horizontal_cuts);
    y_edges.push(image_height);
    let mut x_edges = Vec::with_capacity(vertical_cuts.len() + 2);
    x_edges.push(0);
    x_edges.extend(&vertical_cuts);
    x_edges.push(width);

    // Réduit l'image une seule fois, puis découpe les vignettes dans cette
    // copie légère. Repartir de l'original pour chaque page ralentirait les
    // planches très hautes.
    const PREVIEW_MAX_WIDTH: u32 = 320;
    const PREVIEW_MAX_HEIGHT: u32 = 18_000;
    let proxy_scale = (PREVIEW_MAX_WIDTH as f64 / width as f64)
        .min(PREVIEW_MAX_HEIGHT as f64 / image_height as f64)
        .min(1.0);
    let proxy_width = ((width as f64 * proxy_scale).round() as u32).max(1);
    let proxy_height = ((image_height as f64 * proxy_scale).round() as u32).max(1);
    let proxy = if let Some(mapped) = &mapped {
        mapped.thumbnail(proxy_width, proxy_height)
    } else {
        imageops::thumbnail(
            image.as_ref().expect("image standard"),
            proxy_width,
            proxy_height,
        )
    };

    const MAX_PAGE_PREVIEWS: usize = 24;
    let page_count = (y_edges.len() - 1) * (x_edges.len() - 1);
    let mut pages = Vec::with_capacity(page_count.min(MAX_PAGE_PREVIEWS));
    for row in 0..y_edges.len() - 1 {
        for column in 0..x_edges.len() - 1 {
            if pages.len() == MAX_PAGE_PREVIEWS {
                break;
            }
            let page_width = x_edges[column + 1] - x_edges[column];
            let page_height = y_edges[row + 1] - y_edges[row];
            let x = map_edge(x_edges[column], width, proxy_width).min(proxy_width - 1);
            let y = map_edge(y_edges[row], image_height, proxy_height).min(proxy_height - 1);
            let right = map_edge(x_edges[column + 1], width, proxy_width)
                .max(x + 1)
                .min(proxy_width);
            let bottom = map_edge(y_edges[row + 1], image_height, proxy_height)
                .max(y + 1)
                .min(proxy_height);
            let page = imageops::crop_imm(&proxy, x, y, right - x, bottom - y).to_image();
            pages.push(PagePreview {
                number: row * (x_edges.len() - 1) + column + 1,
                width: page_width,
                height: page_height,
                data_url: encode_preview(&page, 150, 720)?,
            });
        }
        if pages.len() == MAX_PAGE_PREVIEWS {
            break;
        }
    }
    Ok(SplitPreview {
        width,
        height: image_height,
        pages_truncated: page_count > MAX_PAGE_PREVIEWS,
        pages,
    })
}
