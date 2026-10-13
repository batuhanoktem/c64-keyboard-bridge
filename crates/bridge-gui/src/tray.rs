//! Platform tray events wake the GUI even while its window is hidden.
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    Open,
    Pause,
    Enable,
    Stop,
    Quit,
    Available(bool),
}
pub type TrayCallback = Arc<dyn Fn(TrayEvent) + Send + Sync>;

/// Native icon resources are retained for the lifetime of the desktop app.
pub struct Tray {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    icon: tray_icon::TrayIcon,
    #[cfg(target_os = "linux")]
    stop: tokio::sync::watch::Sender<bool>,
    #[cfg(target_os = "linux")]
    updates: tokio::sync::watch::Sender<bool>,
    #[cfg(target_os = "linux")]
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Tray {
    pub fn new(callback: TrayCallback) -> Result<Self> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            use tray_icon::{
                Icon, TrayIconBuilder, TrayIconEvent,
                menu::{Menu, MenuEvent, MenuItem},
            };
            let menu = Menu::new();
            for (id, label) in [
                ("open", "Open C64 Keyboard Bridge"),
                ("pause", "Pause input"),
                ("enable", "Enable input"),
                ("stop", "Stop bridge"),
                ("quit", "Quit"),
            ] {
                menu.append(&MenuItem::with_id(id, label, true, None))?;
            }
            let command = callback.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let action = match event.id.as_ref() {
                    "open" => TrayEvent::Open,
                    "pause" => TrayEvent::Pause,
                    "enable" => TrayEvent::Enable,
                    "stop" => TrayEvent::Stop,
                    "quit" => TrayEvent::Quit,
                    _ => return,
                };
                command(action);
            }));
            let activate = callback.clone();
            TrayIconEvent::set_event_handler(Some(move |event| {
                if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                    activate(TrayEvent::Open);
                }
            }));
            let image = image::load_from_memory_with_format(
                include_bytes!("../../../assets/c64bridge.png"),
                image::ImageFormat::Png,
            )?
            .into_rgba8();
            let icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_icon(Icon::from_rgba(
                    image.as_raw().clone(),
                    image.width(),
                    image.height(),
                )?)
                .with_tooltip("C64 Keyboard Bridge · input paused")
                .with_menu_on_left_click(true)
                .build()?;
            callback(TrayEvent::Available(true));
            Ok(Self { icon })
        }
        #[cfg(target_os = "linux")]
        {
            let (stop, mut stopping) = tokio::sync::watch::channel(false);
            let (updates, mut enabled) = tokio::sync::watch::channel(false);
            let thread = std::thread::Builder::new().name("c64bridge-tray".into()).spawn(move || {
                use ksni::TrayMethods;
                let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(_) => { callback(TrayEvent::Available(false)); return; }
                };
                runtime.block_on(async move {
                    let monitor = match tokio::time::timeout(std::time::Duration::from_secs(2), zbus::Connection::session()).await {
                        Ok(Ok(connection)) => connection,
                        _ => { callback(TrayEvent::Available(false)); return; }
                    };
                    let available = Arc::new(std::sync::atomic::AtomicBool::new(true));
                    let item = LinuxTray { callback: callback.clone(), available: available.clone(), enabled: false };
                    let handle = match tokio::time::timeout(std::time::Duration::from_secs(2), item.assume_sni_available(true).spawn()).await {
                        Ok(Ok(handle)) => handle,
                        _ => { callback(TrayEvent::Available(false)); return; }
                    };
                    // Build success with assume_sni_available is not proof of a
                    // visible tray. Verify the watcher and this registered item.
                    let mut reported = false;
                    callback(TrayEvent::Available(false));
                    let mut heartbeat = tokio::time::interval(std::time::Duration::from_millis(500));
                    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        tokio::select! {
                            biased;
                            changed = stopping.changed() => { if changed.is_err() || *stopping.borrow() { break; } },
                            _ = monitor.closed() => { callback(TrayEvent::Available(false)); break; },
                            changed = enabled.changed() => {
                                if changed.is_err() { break; }
                                let value = *enabled.borrow_and_update();
                                if !matches!(tokio::time::timeout(std::time::Duration::from_secs(2), handle.update(move |tray| tray.enabled = value)).await, Ok(Some(()))) {
                                    callback(TrayEvent::Available(false)); break;
                                }
                            }
                            _ = heartbeat.tick() => {
                                let visible = available.load(std::sync::atomic::Ordering::SeqCst)
                                    && matches!(tokio::time::timeout(std::time::Duration::from_secs(2), registered_on_bus(&monitor)).await, Ok(Ok(true)));
                                if visible != reported { callback(TrayEvent::Available(visible)); reported = visible; }
                            }
                        }
                    }
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle.shutdown()).await;
                });
            })?;
            Ok(Self {
                stop,
                updates,
                thread: Some(thread),
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            callback(TrayEvent::Available(false));
            anyhow::bail!("No tray implementation is available on this platform")
        }
    }
    pub fn set_enabled(&self, enabled: bool) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let _ = self.icon.set_tooltip(Some(if enabled {
            "C64 Keyboard Bridge · input enabled"
        } else {
            "C64 Keyboard Bridge · input paused"
        }));
        #[cfg(target_os = "linux")]
        self.updates.send_if_modified(|current| {
            if *current == enabled {
                false
            } else {
                *current = enabled;
                true
            }
        });
    }
}
#[cfg(target_os = "linux")]
impl Drop for Tray {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "linux")]
struct LinuxTray {
    callback: TrayCallback,
    available: Arc<std::sync::atomic::AtomicBool>,
    enabled: bool,
}
#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "com.flameliongames.c64bridge".into()
    }
    fn title(&self) -> String {
        "C64 Keyboard Bridge".into()
    }
    fn activate(&mut self, _: i32, _: i32) {
        (self.callback)(TrayEvent::Open);
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let Ok(image) = image::load_from_memory_with_format(
            include_bytes!("../../../assets/c64bridge.png"),
            image::ImageFormat::Png,
        ) else {
            return Vec::new();
        };
        let image = image.into_rgba8();
        let (width, height) = image.dimensions();
        let mut data = image.into_raw();
        for pixel in data.as_chunks_mut::<4>().0 {
            pixel.rotate_right(1);
        }
        vec![ksni::Icon {
            width: width as i32,
            height: height as i32,
            data,
        }]
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "C64 Keyboard Bridge".into(),
            description: if self.enabled {
                "Input enabled"
            } else {
                "Input paused"
            }
            .into(),
            ..Default::default()
        }
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        [
            ("Open C64 Keyboard Bridge", TrayEvent::Open),
            ("Pause input", TrayEvent::Pause),
            ("Enable input", TrayEvent::Enable),
            ("Stop bridge", TrayEvent::Stop),
            ("Quit", TrayEvent::Quit),
        ]
        .into_iter()
        .map(|(label, event)| {
            ksni::menu::StandardItem {
                label: label.into(),
                activate: Box::new(move |tray: &mut Self| (tray.callback)(event)),
                ..Default::default()
            }
            .into()
        })
        .collect()
    }
    fn watcher_online(&self) {
        self.available
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    fn watcher_offline(&self, _: ksni::OfflineReason) -> bool {
        self.available
            .store(false, std::sync::atomic::Ordering::SeqCst);
        (self.callback)(TrayEvent::Available(false));
        true
    }
}

#[cfg(target_os = "linux")]
async fn registered_on_bus(connection: &zbus::Connection) -> zbus::Result<bool> {
    const WATCHER: &str = "org.kde.StatusNotifierWatcher";
    let bus = zbus::fdo::DBusProxy::new(connection).await?;
    if !bus.name_has_owner(WATCHER.try_into()?).await? {
        return Ok(false);
    }
    let watcher = zbus::Proxy::new(connection, WATCHER, "/StatusNotifierWatcher", WATCHER).await?;
    if !watcher
        .get_property::<bool>("IsStatusNotifierHostRegistered")
        .await?
    {
        return Ok(false);
    }
    let items = watcher
        .get_property::<Vec<String>>("RegisteredStatusNotifierItems")
        .await?;
    let prefix = format!("org.kde.StatusNotifierItem-{}-", std::process::id());
    Ok(bus.list_names().await?.iter().any(|name| {
        let name = name.as_str();
        name.starts_with(&prefix)
            && items.iter().any(|item| {
                item == name
                    || item
                        .strip_prefix(name)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
    }))
}
