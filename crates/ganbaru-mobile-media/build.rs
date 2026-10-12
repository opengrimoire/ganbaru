fn main() {
    tauri_plugin::Builder::new(&[
        "probe",
        "attachSession",
        "pickMediaTree",
        "scanMediaTree",
        "pickArtworkFile",
        "artworkDataUrl",
    ])
    .android_path("android")
    .build();
}
