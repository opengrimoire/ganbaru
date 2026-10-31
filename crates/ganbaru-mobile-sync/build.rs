fn main() {
    tauri_plugin::Builder::new(&["configure", "connectivityStatus"])
        .android_path("android")
        .build();
}
