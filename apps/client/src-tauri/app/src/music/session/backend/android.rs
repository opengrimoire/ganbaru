//! A process-lifetime JNI handle delivers effects directly to the Media3 service.
//!
//! Initial capture needs the Activity class loader. Later dispatch attaches the worker
//! to its JavaVM and uses a global class reference, without the Activity or WebView.

use super::super::{models::*, runtime};
use super::delivery::{DeliveryKind, Registry};
use ganbaru_mobile_media::MobileMediaExt;
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tauri::{
    Manager,
    ipc::Channel,
    wry::prelude::jni::{
        JavaVM,
        objects::{GlobalRef, JClass, JValue},
    },
};
use tokio::sync::oneshot;

struct AndroidBridge {
    vm: JavaVM,
    class: GlobalRef,
}
static BRIDGE: OnceLock<AndroidBridge> = OnceLock::new();
static DELIVERIES: OnceLock<Arc<Registry>> = OnceLock::new();
static AUTHORITY: OnceLock<Result<(), String>> = OnceLock::new();
static JNI_WORKER: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(8);
const JNI_TIMEOUT: Duration = Duration::from_secs(2);

enum BridgeCall {
    Active,
    Dispatch(String),
    Cancel(i64),
}

fn deliveries() -> &'static Arc<Registry> {
    DELIVERIES.get_or_init(|| Arc::new(Registry::default()))
}

#[derive(serde::Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ServiceEvent {
    Observation {
        observation: SessionObservation,
    },
    Control {
        intent: SessionIntent,
    },
    Unavailable {
        session_id: String,
        generation: u64,
        reason: AndroidInterruption,
    },
    Applied {
        delivery_id: i64,
        error: Option<String>,
    },
}

pub(in super::super) async fn is_active() -> Result<bool, String> {
    call_bridge(BridgeCall::Active).await
}

/// One retained worker bounds JNI stalls even after the async waiter times out.
async fn call_bridge(call: BridgeCall) -> Result<bool, String> {
    let permit = JNI_WORKER
        .try_acquire()
        .map_err(|error| format!("Android Music JNI worker is still draining: {error}"))?;
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let Some(bridge) = BRIDGE.get() else {
            return match call {
                BridgeCall::Active => Ok(false),
                _ => Err("Android Music bridge is not attached".into()),
            };
        };
        let mut env = bridge
            .vm
            .attach_current_thread()
            .map_err(|error| error.to_string())?;
        let class: &JClass<'_> = bridge.class.as_obj().into();
        let result = match call {
            BridgeCall::Active => env.call_static_method(class, "isActive", "()Z", &[]),
            BridgeCall::Cancel(id) => {
                env.call_static_method(class, "cancelDelivery", "(J)Z", &[JValue::Long(id)])
            }
            BridgeCall::Dispatch(encoded) => {
                let payload = env.new_string(encoded).map_err(|error| error.to_string())?;
                env.call_static_method(
                    class,
                    "dispatch",
                    "(Ljava/lang/String;)Z",
                    &[JValue::Object(&payload)],
                )
            }
        }
        .and_then(|value| value.z())
        .map_err(|error| error.to_string());
        if env
            .exception_check()
            .map_err(|error| format!("Check Android Music JNI exception: {error}"))?
        {
            env.exception_clear()
                .map_err(|error| format!("Clear Android Music JNI exception: {error}"))?;
        }
        result
    });
    tokio::time::timeout(JNI_TIMEOUT, worker)
        .await
        .map_err(|_| "Android Music JNI worker did not finish in time".to_string())?
        .map_err(|error| error.to_string())?
}

/// Explicit playback can reattach a stopped service through the available platform host.
pub(in super::super) async fn restart_for_play(app: &tauri::AppHandle) -> Result<bool, String> {
    if is_active().await? {
        return Ok(false);
    }
    attach(app).await?;
    Ok(true)
}

pub(in super::super) async fn attach(app: &tauri::AppHandle) -> Result<(), String> {
    AUTHORITY
        .get_or_init(|| {
            let registry = Arc::clone(deliveries());
            ganbaru_mobile_media::set_music_authority_checker(move |id| registry.current(id))
        })
        .clone()?;
    if BRIDGE.get().is_some() {
        let active = is_active().await?;
        if active {
            return Ok(());
        }
        return attach_channel(app).await;
    }
    let webview = app
        .get_webview_window(&runtime::primary_window_label(app))
        .ok_or_else(|| "Music bridge requires its initial platform window".to_string())?;
    let (sender, receiver) = oneshot::channel();
    webview
        .with_webview(move |platform| {
            platform.jni_handle().exec(move |env, activity, _| {
                let result = (|| {
                    let loader = env
                        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                        .l()?;
                    let name = env
                        .new_string("org.opengrimoire.ganbaruai.mobile.media.NativeMusicSession")?;
                    let class = env
                        .call_method(
                            &loader,
                            "loadClass",
                            "(Ljava/lang/String;)Ljava/lang/Class;",
                            &[JValue::Object(&name)],
                        )?
                        .l()?;
                    Ok::<_, tauri::wry::prelude::jni::errors::Error>(AndroidBridge {
                        vm: env.get_java_vm()?,
                        class: env.new_global_ref(class)?,
                    })
                })()
                .map_err(|error| error.to_string());
                let _ = sender.send(result);
            });
        })
        .map_err(|error| error.to_string())?;
    let bridge = receiver.await.map_err(|error| error.to_string())??;
    BRIDGE
        .set(bridge)
        .map_err(|_| "Music service bridge was already attached".to_string())?;
    attach_channel(app).await
}

async fn attach_channel(app: &tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    let channel = Channel::new(move |payload| {
        let event: ServiceEvent = payload.deserialize()?;
        let result = match event {
            ServiceEvent::Observation { observation } => {
                runtime::native_observation(&handle, observation)
            }
            ServiceEvent::Control { intent } => runtime::dispatch_control(&handle, intent),
            ServiceEvent::Unavailable {
                session_id,
                generation,
                reason,
            } => runtime::native_backend_unavailable(&handle, session_id, generation, reason),
            ServiceEvent::Applied { delivery_id, error } => {
                let result = match error {
                    Some(error) if error.len() <= 4096 => Err(error),
                    Some(_) => Err("Android Music execution error exceeded its limit".into()),
                    None => Ok(()),
                };
                // This completes the waiter directly. Queuing behind the waiting
                // Music owner would deadlock execution acknowledgement.
                if delivery_id <= 0 {
                    Err("Android Music delivery identity is invalid".into())
                } else {
                    deliveries().complete(delivery_id, result).map(|_| ())
                }
            }
        };
        if let Err(error) = result {
            eprintln!("Android music service event: {error}");
        }
        Ok(())
    });
    app.mobile_media().attach_session(channel).await
}

pub(in super::super) async fn apply(
    effect: SessionEffect,
    authority: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<(), String> {
    if let Some(id) = deliveries().canceled_pending()? {
        if !call_bridge(BridgeCall::Cancel(id)).await? {
            return Err("Earlier Android Music SDK delivery is still executing".into());
        }
        deliveries().complete(
            id,
            Err("Android Music delivery was canceled and drained".into()),
        )?;
    }
    let kind = match effect {
        SessionEffect::Load { .. } => DeliveryKind::Load,
        SessionEffect::Stop { .. } => DeliveryKind::Stop,
        _ => DeliveryKind::Control,
    };
    let delivery = deliveries().begin(kind, move || authority())?;
    // A missing service has no decoder only after its release completes. Clear
    // retained load authority even when Stop needs no live platform delivery.
    if matches!(effect, SessionEffect::Stop { .. }) && !is_active().await? {
        deliveries().complete(delivery.id(), Ok(()))?;
        return delivery.wait(DELIVERY_TIMEOUT).await;
    }
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        delivery_id: i64,
        #[serde(flatten)]
        effect: SessionEffect,
    }
    let encoded = serde_json::to_string(&Envelope {
        delivery_id: delivery.id(),
        effect,
    })
    .map_err(|error| error.to_string())?;
    if !call_bridge(BridgeCall::Dispatch(encoded)).await? {
        let error = "Android Music rejected delivery before SDK execution".to_string();
        delivery.rejected(error.clone())?;
        return Err(error);
    }
    delivery.wait(DELIVERY_TIMEOUT).await
}
