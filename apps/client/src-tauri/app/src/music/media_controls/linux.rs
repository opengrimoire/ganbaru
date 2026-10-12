//! Linux MPRIS integration.

use super::{
    MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE, MediaControlsUpdate, clamp_playback_rate, clamp_unit,
    control, control_delta, control_position, control_rate, control_shuffle, control_volume,
    emit_control, ms_to_us_i64, us_to_ms_i64, us_to_ms_u64,
};
use gtk::gio;
use gtk::glib;
use gtk::glib::prelude::*;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tauri::Manager;

const BUS_NAME: &str = "org.mpris.MediaPlayer2.ganbaru-ai";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const CURRENT_TRACK_PATH: &str = "/org/mpris/MediaPlayer2/current";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

const MPRIS_XML: &str = r#"
<node>
  <interface name="org.mpris.MediaPlayer2">
<method name="Raise"/>
<method name="Quit"/>
<property name="CanQuit" type="b" access="read"/>
<property name="CanRaise" type="b" access="read"/>
<property name="HasTrackList" type="b" access="read"/>
<property name="Identity" type="s" access="read"/>
<property name="DesktopEntry" type="s" access="read"/>
<property name="SupportedUriSchemes" type="as" access="read"/>
<property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
<method name="Next"/>
<method name="Previous"/>
<method name="Pause"/>
<method name="PlayPause"/>
<method name="Stop"/>
<method name="Play"/>
<method name="Seek">
  <arg name="Offset" type="x" direction="in"/>
</method>
<method name="SetPosition">
  <arg name="TrackId" type="o" direction="in"/>
  <arg name="Position" type="x" direction="in"/>
</method>
<method name="OpenUri">
  <arg name="Uri" type="s" direction="in"/>
</method>
<property name="PlaybackStatus" type="s" access="read"/>
<property name="LoopStatus" type="s" access="read"/>
<property name="Rate" type="d" access="readwrite"/>
<property name="Shuffle" type="b" access="readwrite"/>
<property name="Metadata" type="a{sv}" access="read"/>
<property name="Volume" type="d" access="readwrite"/>
<property name="Position" type="x" access="read"/>
<property name="MinimumRate" type="d" access="read"/>
<property name="MaximumRate" type="d" access="read"/>
<property name="CanGoNext" type="b" access="read"/>
<property name="CanGoPrevious" type="b" access="read"/>
<property name="CanPlay" type="b" access="read"/>
<property name="CanPause" type="b" access="read"/>
<property name="CanSeek" type="b" access="read"/>
<property name="CanControl" type="b" access="read"/>
  </interface>
</node>
"#;

#[derive(Debug, Clone, PartialEq)]
struct MprisState {
    status: String,
    title: Option<String>,
    source_kind_label: Option<String>,
    artwork_url: Option<String>,
    can_play_pause: bool,
    can_previous: bool,
    can_next: bool,
    can_seek: bool,
    position_us: i64,
    duration_us: Option<i64>,
    volume: f64,
    muted: bool,
    rate: f64,
    shuffle_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct MprisSignalShape {
    status: String,
    title: Option<String>,
    source_kind_label: Option<String>,
    artwork_url: Option<String>,
    can_play_pause: bool,
    can_previous: bool,
    can_next: bool,
    can_seek: bool,
    duration_us: Option<i64>,
    volume: f64,
    muted: bool,
    rate: f64,
    shuffle_enabled: bool,
}

static MPRIS_STATE: LazyLock<Mutex<MprisState>> =
    LazyLock::new(|| Mutex::new(MprisState::default()));
static MPRIS_CONNECTION: LazyLock<Mutex<Option<gio::DBusConnection>>> =
    LazyLock::new(|| Mutex::new(None));
static MPRIS_OWNER_ID: LazyLock<Mutex<Option<gio::OwnerId>>> = LazyLock::new(|| Mutex::new(None));
static LAST_SIGNAL_SHAPE: LazyLock<Mutex<MprisSignalShape>> =
    LazyLock::new(|| Mutex::new(MprisState::default().signal_shape()));

impl Default for MprisState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            title: None,
            source_kind_label: None,
            artwork_url: None,
            can_play_pause: false,
            can_previous: false,
            can_next: false,
            can_seek: false,
            position_us: 0,
            duration_us: None,
            volume: 0.8,
            muted: false,
            rate: 1.0,
            shuffle_enabled: true,
        }
    }
}

impl MprisState {
    fn from_update(update: MediaControlsUpdate) -> Self {
        Self {
            status: update.status,
            title: update.title,
            source_kind_label: update.source_kind_label,
            artwork_url: update.artwork_url,
            can_play_pause: update.can_play_pause,
            can_previous: update.can_previous,
            can_next: update.can_next,
            can_seek: update.can_seek,
            position_us: ms_to_us_i64(update.position_ms),
            duration_us: update.duration_ms.map(ms_to_us_i64),
            volume: clamp_unit(update.volume),
            muted: update.muted,
            rate: clamp_playback_rate(update.rate),
            shuffle_enabled: update.shuffle_enabled,
        }
    }

    fn has_track(&self) -> bool {
        self.title
            .as_deref()
            .is_some_and(|title| !title.trim().is_empty())
    }

    fn effective_volume(&self) -> f64 {
        if self.muted { 0.0 } else { self.volume }
    }

    fn signal_shape(&self) -> MprisSignalShape {
        MprisSignalShape {
            status: self.status.clone(),
            title: self.title.clone(),
            source_kind_label: self.source_kind_label.clone(),
            artwork_url: self.artwork_url.clone(),
            can_play_pause: self.can_play_pause,
            can_previous: self.can_previous,
            can_next: self.can_next,
            can_seek: self.can_seek,
            duration_us: self.duration_us,
            volume: self.volume,
            muted: self.muted,
            rate: self.rate,
            shuffle_enabled: self.shuffle_enabled,
        }
    }
}

pub(super) fn setup_mpris(app: &tauri::AppHandle) {
    let app_for_bus = app.clone();
    let owner_id = gio::bus_own_name(
        gio::BusType::Session,
        BUS_NAME,
        gio::BusNameOwnerFlags::NONE,
        move |connection, _name| {
            if let Err(err) = register_mpris_object(&app_for_bus, &connection) {
                eprintln!("failed to register MPRIS media controls: {err}");
            }
        },
        |_connection, _name| {},
        |_connection, _name| {
            if let Ok(mut connection) = MPRIS_CONNECTION.lock() {
                *connection = None;
            }
        },
    );

    if let Ok(mut stored) = MPRIS_OWNER_ID.lock() {
        *stored = Some(owner_id);
    }
}

pub(super) fn update_mpris_state(update: MediaControlsUpdate) {
    let state = MprisState::from_update(update);
    {
        let Ok(mut stored) = MPRIS_STATE.lock() else {
            return;
        };
        *stored = state.clone();
    }

    let shape = state.signal_shape();
    let should_emit = {
        let Ok(mut last_shape) = LAST_SIGNAL_SHAPE.lock() else {
            return;
        };
        if *last_shape == shape {
            false
        } else {
            *last_shape = shape;
            true
        }
    };

    if should_emit {
        emit_player_properties_changed(&state);
    }
}

fn register_mpris_object(
    app: &tauri::AppHandle,
    connection: &gio::DBusConnection,
) -> Result<(), String> {
    let node_info = Box::leak(Box::new(
        gio::DBusNodeInfo::for_xml(MPRIS_XML).map_err(|e| e.to_string())?,
    ));
    let root_interface = Box::leak(Box::new(
        node_info
            .lookup_interface(ROOT_INTERFACE)
            .ok_or_else(|| "missing root MPRIS interface".to_string())?,
    ));
    let player_interface = Box::leak(Box::new(
        node_info
            .lookup_interface(PLAYER_INTERFACE)
            .ok_or_else(|| "missing player MPRIS interface".to_string())?,
    ));

    let root_app = app.clone();
    connection
        .register_object(
            OBJECT_PATH,
            root_interface,
            move |_connection,
                  _sender,
                  _object_path,
                  _interface_name,
                  method_name,
                  _parameters,
                  invocation| {
                handle_root_method(&root_app, method_name);
                invocation.return_value(Some(&().to_variant()));
            },
            |_connection, _sender, _object_path, _interface_name, property_name| {
                root_property_variant(property_name)
            },
            |_connection, _sender, _object_path, _interface_name, _property_name, _value| false,
        )
        .map_err(|e| e.to_string())?;

    let player_app = app.clone();
    connection
        .register_object(
            OBJECT_PATH,
            player_interface,
            move |_connection,
                  _sender,
                  _object_path,
                  _interface_name,
                  method_name,
                  parameters,
                  invocation| {
                handle_player_method(&player_app, method_name, &parameters);
                invocation.return_value(Some(&().to_variant()));
            },
            |_connection, _sender, _object_path, _interface_name, property_name| {
                player_property_variant(property_name)
            },
            {
                let set_app = app.clone();
                move |_connection, _sender, _object_path, _interface_name, property_name, value| {
                    handle_player_set_property(&set_app, property_name, &value)
                }
            },
        )
        .map_err(|e| e.to_string())?;

    if let Ok(mut stored) = MPRIS_CONNECTION.lock() {
        *stored = Some(connection.clone());
    }
    emit_player_properties_changed(&current_state());
    Ok(())
}

fn handle_root_method(app: &tauri::AppHandle, method_name: &str) {
    if method_name != "Raise" {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn handle_player_method(app: &tauri::AppHandle, method_name: &str, parameters: &glib::Variant) {
    match method_name {
        "Next" => emit_control(app, control("nextTrack")),
        "Previous" => emit_control(app, control("previousTrack")),
        "Pause" => emit_control(app, control("pause")),
        "PlayPause" => emit_control(app, control("playPause")),
        "Stop" => emit_control(app, control("stop")),
        "Play" => emit_control(app, control("play")),
        "Seek" => {
            if let Ok(Some(delta_us)) = parameters.try_child_get::<i64>(0) {
                emit_control(app, control_delta("seekBy", us_to_ms_i64(delta_us)));
            }
        }
        "SetPosition" => {
            if let Ok(Some(position_us)) = parameters.try_child_get::<i64>(1) {
                emit_control(app, control_position("seekTo", us_to_ms_u64(position_us)));
            }
        }
        "OpenUri" => {}
        _ => {}
    }
}

fn handle_player_set_property(
    app: &tauri::AppHandle,
    property_name: &str,
    value: &glib::Variant,
) -> bool {
    match property_name {
        "Volume" => {
            if let Some(volume) = value.get::<f64>() {
                emit_control(app, control_volume("setVolume", clamp_unit(volume)));
                return true;
            }
            false
        }
        "Rate" => {
            if let Some(rate) = value.get::<f64>() {
                emit_control(app, control_rate("setRate", rate));
                return true;
            }
            false
        }
        "Shuffle" => {
            if let Some(shuffle_enabled) = value.get::<bool>() {
                emit_control(app, control_shuffle("setShuffle", shuffle_enabled));
                return true;
            }
            false
        }
        _ => false,
    }
}

fn root_property_variant(property_name: &str) -> glib::Variant {
    match property_name {
        "CanQuit" => false.to_variant(),
        "CanRaise" => true.to_variant(),
        "HasTrackList" => false.to_variant(),
        "Identity" => "Ganbaru AI".to_variant(),
        "DesktopEntry" => "ganbaru-ai".to_variant(),
        "SupportedUriSchemes" => Vec::<String>::new().to_variant(),
        "SupportedMimeTypes" => Vec::<String>::new().to_variant(),
        _ => ().to_variant(),
    }
}

fn player_property_variant(property_name: &str) -> glib::Variant {
    let state = current_state();
    match property_name {
        "PlaybackStatus" => mpris_playback_status(&state.status).to_variant(),
        "LoopStatus" => "None".to_variant(),
        "Rate" => state.rate.to_variant(),
        "Shuffle" => state.shuffle_enabled.to_variant(),
        "Metadata" => metadata_variant(&state),
        "Volume" => state.effective_volume().to_variant(),
        "Position" => state.position_us.to_variant(),
        "MinimumRate" => MIN_PLAYBACK_RATE.to_variant(),
        "MaximumRate" => MAX_PLAYBACK_RATE.to_variant(),
        "CanGoNext" => state.can_next.to_variant(),
        "CanGoPrevious" => state.can_previous.to_variant(),
        "CanPlay" => state.can_play_pause.to_variant(),
        "CanPause" => state.can_play_pause.to_variant(),
        "CanSeek" => state.can_seek.to_variant(),
        "CanControl" => state.has_track().to_variant(),
        _ => ().to_variant(),
    }
}

fn metadata_variant(state: &MprisState) -> glib::Variant {
    let mut metadata = HashMap::<String, glib::Variant>::new();
    let track_id = glib::variant::ObjectPath::try_from(CURRENT_TRACK_PATH)
        .expect("static track path is valid");
    metadata.insert("mpris:trackid".to_string(), track_id.to_variant());
    if let Some(title) = state
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        metadata.insert("xesam:title".to_string(), title.to_variant());
    }
    if let Some(label) = state
        .source_kind_label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
    {
        metadata.insert(
            "xesam:artist".to_string(),
            vec![label.to_string()].to_variant(),
        );
    }
    if let Some(duration_us) = state.duration_us {
        metadata.insert("mpris:length".to_string(), duration_us.to_variant());
    }
    if let Some(artwork_url) = state
        .artwork_url
        .as_deref()
        .map(str::trim)
        .filter(|artwork_url| !artwork_url.is_empty())
    {
        metadata.insert("mpris:artUrl".to_string(), artwork_url.to_variant());
    }
    metadata.to_variant()
}

fn emit_player_properties_changed(state: &MprisState) {
    let connection = {
        let Ok(stored) = MPRIS_CONNECTION.lock() else {
            return;
        };
        stored.clone()
    };
    let Some(connection) = connection else {
        return;
    };

    let changed = player_properties_changed_variant(state);
    let invalidated = Vec::<String>::new();
    let parameters = (PLAYER_INTERFACE, changed, invalidated).to_variant();
    let _ = connection.emit_signal(
        None,
        OBJECT_PATH,
        PROPERTIES_INTERFACE,
        "PropertiesChanged",
        Some(&parameters),
    );
}

fn player_properties_changed_variant(state: &MprisState) -> HashMap<String, glib::Variant> {
    HashMap::from([
        (
            "PlaybackStatus".to_string(),
            mpris_playback_status(&state.status).to_variant(),
        ),
        ("Rate".to_string(), state.rate.to_variant()),
        ("Shuffle".to_string(), state.shuffle_enabled.to_variant()),
        ("Metadata".to_string(), metadata_variant(state)),
        ("Volume".to_string(), state.effective_volume().to_variant()),
        ("MinimumRate".to_string(), MIN_PLAYBACK_RATE.to_variant()),
        ("MaximumRate".to_string(), MAX_PLAYBACK_RATE.to_variant()),
        ("CanGoNext".to_string(), state.can_next.to_variant()),
        ("CanGoPrevious".to_string(), state.can_previous.to_variant()),
        ("CanPlay".to_string(), state.can_play_pause.to_variant()),
        ("CanPause".to_string(), state.can_play_pause.to_variant()),
        ("CanSeek".to_string(), state.can_seek.to_variant()),
        ("CanControl".to_string(), state.has_track().to_variant()),
    ])
}

fn current_state() -> MprisState {
    MPRIS_STATE
        .lock()
        .map(|state| state.clone())
        .unwrap_or_else(|_| MprisState::default())
}

fn mpris_playback_status(status: &str) -> &'static str {
    match status {
        "playing" => "Playing",
        "paused" | "ready" => "Paused",
        _ => "Stopped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpris_status_maps_player_states() {
        assert_eq!(mpris_playback_status("playing"), "Playing");
        assert_eq!(mpris_playback_status("paused"), "Paused");
        assert_eq!(mpris_playback_status("ready"), "Paused");
        assert_eq!(mpris_playback_status("idle"), "Stopped");
    }

    #[test]
    fn signal_shape_ignores_position_only_changes() {
        let first = MprisState {
            title: Some("Focus track".to_string()),
            ..MprisState::default()
        };
        let mut second = first.clone();
        second.position_us = 30_000_000;

        assert_eq!(first.signal_shape(), second.signal_shape());
    }

    #[test]
    fn metadata_uses_mpris_variant_shape() {
        let state = MprisState {
            title: Some("Focus track".to_string()),
            source_kind_label: Some("Local file".to_string()),
            duration_us: Some(120_000_000),
            ..MprisState::default()
        };

        assert_eq!(metadata_variant(&state).type_().as_str(), "a{sv}");
    }
}
