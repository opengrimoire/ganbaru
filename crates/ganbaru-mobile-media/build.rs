fn main() {
    tauri_plugin::Builder::new(&[
        "probe",
        "attachSession",
        "pickMediaTree",
        "scanMediaTree",
        "pickArtworkFile",
        "artworkDataUrl",
        "youtubeOpen",
        "youtubeBounds",
        "youtubePost",
        "youtubeClose",
    ])
    .android_path("android")
    .build();
}
