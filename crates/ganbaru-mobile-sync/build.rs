fn main() {
    tauri_plugin::Builder::new(&["configure", "connectivityStatus", "setSecureWindow"])
        .android_path("android")
        .build();
}
