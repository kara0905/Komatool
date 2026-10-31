//! Traitements d'image regroupés par opération proposée dans l'interface.
//!
//! Ces modules contiennent le moteur natif ; `commands` adapte leurs résultats
//! et leurs événements de progression à l'API Tauri utilisée par l'interface.

pub mod advanced_split;
pub mod archive;
pub mod common;
pub mod convert;
pub(crate) mod mapped_image;
pub mod merge;
pub mod pipeline;
pub mod preview;
pub mod split;
pub mod transform;
pub mod waifu;

#[cfg(test)]
mod tests {
    use super::{
        common::natural_cmp,
        split::{row_difference_score, smart_boundaries},
    };
    use image::RgbaImage;
    use std::time::Instant;
    #[test]
    fn natural_sort_orders_numeric_parts() {
        assert_eq!(
            natural_cmp("page2.png", "page10.png"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            natural_cmp("Page001.png", "page2.png"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn row_score_is_zero_for_uniform_rows() {
        let img = RgbaImage::from_pixel(64, 2, image::Rgba([20, 30, 40, 255]));
        assert_eq!(row_difference_score(img.as_raw(), 64, 0, 1), 0);
        assert_eq!(row_difference_score(img.as_raw(), 64, 1, 1), 0);
    }

    // Benchmark manuel : cargo test benchmark_smart_scan -- --ignored --nocapture
    #[test]
    #[ignore = "benchmark manuel, non exécuté dans la suite de tests normale"]
    fn benchmark_smart_scan() {
        let width = 1600usize;
        let height = 24_000usize;
        let img = RgbaImage::from_fn(width as u32, height as u32, |x, y| {
            let band = ((y / 120) % 2) as u8 * 18;
            let texture = (x.wrapping_mul(31).wrapping_add(y.wrapping_mul(17)) % 23) as u8;
            image::Rgba([band + texture, band + texture, band + texture, 255])
        });
        let start = Instant::now();
        let sequential_bounds = smart_boundaries(&img, 5_000, u32::MAX, |_, _| {}).unwrap();
        let sequential = start.elapsed();

        let start = Instant::now();
        let parallel_bounds = smart_boundaries(&img, 5_000, 0, |_, _| {}).unwrap();
        let parallel = start.elapsed();

        assert_eq!(sequential_bounds, parallel_bounds);
        println!("Découpe intelligente séquentielle : {sequential:?}");
        println!("Découpe intelligente parallèle : {parallel:?}");
        println!(
            "Accélération : {:.2}x",
            sequential.as_secs_f64() / parallel.as_secs_f64()
        );
    }
}
