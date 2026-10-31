mod commands;
mod processing;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_images,
            commands::split,
            commands::merge,
            commands::merge_advanced,
            commands::merge_and_split,
            commands::preview_split,
            commands::split_advanced,
            commands::batch_split,
            commands::export_reading,
            commands::export_cbz_metadata,
            commands::convert_images,
            commands::waifu2x_process,
            commands::run_pipeline,
        ])
        .run(tauri::generate_context!())
        .expect("Erreur au démarrage de Komatool");
}
