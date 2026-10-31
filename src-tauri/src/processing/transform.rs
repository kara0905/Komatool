use image::{imageops::FilterType, DynamicImage, GenericImage, Rgba, RgbaImage};
use serde::Deserialize;

use super::common::validate_image_size;

/// Réglages d'image communs à la fusion, à la découpe et à leurs aperçus.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageAdjustments {
    pub crop_left: u32,
    pub crop_top: u32,
    pub crop_right: u32,
    pub crop_bottom: u32,
    pub margin: u32,
    pub resize_width: Option<u32>,
}

/// Applique les réglages dans l'ordre recadrage, redimensionnement, puis marges.
pub fn apply(image: DynamicImage, adjustments: &ImageAdjustments) -> Result<DynamicImage, String> {
    let mut rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    let crop_width =
        width.saturating_sub(adjustments.crop_left.saturating_add(adjustments.crop_right));
    let crop_height =
        height.saturating_sub(adjustments.crop_top.saturating_add(adjustments.crop_bottom));
    if crop_width == 0 || crop_height == 0 {
        return Err("Le recadrage supprimerait toute l'image.".into());
    }
    if adjustments.crop_left > 0
        || adjustments.crop_top > 0
        || adjustments.crop_right > 0
        || adjustments.crop_bottom > 0
    {
        rgba = image::imageops::crop_imm(
            &rgba,
            adjustments.crop_left,
            adjustments.crop_top,
            crop_width,
            crop_height,
        )
        .to_image();
    }

    if let Some(target_width) = adjustments.resize_width {
        if !(1..=20_000).contains(&target_width) {
            return Err("La largeur cible doit être comprise entre 1 et 20 000 pixels.".into());
        }
        let target_height = ((rgba.height() as f64 * target_width as f64 / rgba.width() as f64)
            .round() as u32)
            .max(1);
        validate_image_size(target_width, target_height)?;
        rgba = image::imageops::resize(&rgba, target_width, target_height, FilterType::Lanczos3);
    }

    if adjustments.margin > 0 {
        if adjustments.margin > 2_000 {
            return Err("La marge maximale est de 2 000 pixels.".into());
        }
        let twice_margin = adjustments
            .margin
            .checked_mul(2)
            .ok_or_else(|| "Marge trop importante.".to_string())?;
        let width = rgba
            .width()
            .checked_add(twice_margin)
            .ok_or_else(|| "Dimensions trop importantes.".to_string())?;
        let height = rgba
            .height()
            .checked_add(twice_margin)
            .ok_or_else(|| "Dimensions trop importantes.".to_string())?;
        validate_image_size(width, height)?;
        let mut padded = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
        padded
            .copy_from(&rgba, adjustments.margin, adjustments.margin)
            .map_err(|e| e.to_string())?;
        rgba = padded;
    }
    validate_image_size(rgba.width(), rgba.height())?;
    Ok(DynamicImage::ImageRgba8(rgba))
}

/// Calcule les dimensions après transformation pour dimensionner le canevas de fusion.
pub fn resulting_dimensions(
    width: u32,
    height: u32,
    adjustments: &ImageAdjustments,
) -> Result<(u32, u32), String> {
    let cropped_width =
        width.saturating_sub(adjustments.crop_left.saturating_add(adjustments.crop_right));
    let cropped_height =
        height.saturating_sub(adjustments.crop_top.saturating_add(adjustments.crop_bottom));
    if cropped_width == 0 || cropped_height == 0 {
        return Err("Le recadrage supprimerait toute l'image.".into());
    }
    let (mut width, mut height) = if let Some(target_width) = adjustments.resize_width {
        if !(1..=20_000).contains(&target_width) {
            return Err("La largeur cible doit être comprise entre 1 et 20 000 pixels.".into());
        }
        (
            target_width,
            ((cropped_height as f64 * target_width as f64 / cropped_width as f64).round() as u32)
                .max(1),
        )
    } else {
        (cropped_width, cropped_height)
    };
    let margin = adjustments
        .margin
        .checked_mul(2)
        .ok_or_else(|| "Marge trop importante.".to_string())?;
    width = width
        .checked_add(margin)
        .ok_or_else(|| "Dimensions trop importantes.".to_string())?;
    height = height
        .checked_add(margin)
        .ok_or_else(|| "Dimensions trop importantes.".to_string())?;
    validate_image_size(width, height)?;
    Ok((width, height))
}
