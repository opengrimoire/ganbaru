fn main() {
    tauri_plugin::Builder::new(&[
        "accessStatus",
        "openUsageAccessSettings",
        "openAccessibilitySettings",
        "listLaunchableApps",
        "applyRules",
        "invalidateRules",
        "pendingEvents",
        "accountingSnapshot",
        "acknowledgeEvents",
        "takeNotificationAction",
    ])
    .android_path("android")
    .build();
}
