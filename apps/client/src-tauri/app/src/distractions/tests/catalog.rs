#[cfg(target_os = "linux")]
#[test]
fn parses_visible_linux_desktop_entries() {
    let app = super::parse_desktop_entry(
        r#"
[Desktop Entry]
Type=Application
Name=Visual\sStudio\sCode
Exec=code
"#,
        "code.desktop".to_string(),
    )
    .unwrap();

    assert_eq!(app.name, "Visual Studio Code");
    assert_eq!(app.source, "Installed app");
    assert_eq!(
        app.process_names,
        vec!["Visual Studio Code", "code", "code.desktop"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn extracts_linux_process_name_from_exec() {
    assert_eq!(
        super::process_name_from_exec(r#""/usr/bin/gnome-calculator" %U"#).as_deref(),
        Some("gnome-calculator")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn skips_hidden_linux_desktop_entries() {
    assert!(
        super::parse_desktop_entry(
            r#"
[Desktop Entry]
Type=Application
Name=Hidden app
NoDisplay=true
"#,
            "hidden.desktop".to_string(),
        )
        .is_none()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn skips_linux_system_utility_desktop_entries() {
    assert!(
        super::parse_desktop_entry(
            r#"
[Desktop Entry]
Type=Application
Name=Settings
Categories=GNOME;GTK;Settings;
Exec=gnome-control-center
"#,
            "org.gnome.Settings.desktop".to_string(),
        )
        .is_none()
    );
    assert!(
        super::parse_desktop_entry(
            r#"
[Desktop Entry]
Type=Application
Name=System Monitor
Categories=GNOME;GTK;System;Monitor;
Exec=gnome-system-monitor
"#,
            "org.gnome.SystemMonitor.desktop".to_string(),
        )
        .is_none()
    );
}
