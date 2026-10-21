mod app_state;
mod engine;
mod models;
mod store;
mod ui;

use std::time::Instant;

use futures::StreamExt as _;
use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, actions, point, px, size,
};

use app_state::AppModel;
use engine::{hotkey, tray};
use ui::{
    Shell,
    pill::{PILL_HEIGHT, PILL_WIDTH, Pill},
};

actions!(zappp, [Quit, CloseWindow]);

fn main() {
    init_logging();

    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--bench") {
        bench(args.get(2).map(String::as_str), args.get(3).map(String::as_str));
        return;
    }
    if args.get(1).map(String::as_str) == Some("--install") {
        let spec = models::find(args.get(2).map(String::as_str).unwrap_or(models::DEFAULT_MODEL));
        let last = std::cell::Cell::new(-1i32);
        let result = app_state::install_model(spec, |p| {
            let pct = (p * 100.) as i32;
            if pct / 10 != last.get() / 10 {
                last.set(pct);
                eprint!("\r{}: {pct}%", spec.id);
            }
        });
        eprintln!();
        match result {
            Ok(()) => println!("installed {} → {}", spec.id, spec.path().display()),
            Err(e) => println!("install failed: {e:#}"),
        }
        return;
    }

    let app = Application::new().with_assets(zui::Assets);
    // Launching the app again (Finder, Spotlight, `open`) brings the window back.
    app.on_reopen(|_| tray::send(tray::TrayEvent::Open));
    app.run(|cx: &mut App| {
        zui::init(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let (hotkey_tx, mut hotkey_rx) = futures::channel::mpsc::unbounded();
        let hotkeys = {
            let settings = store::load_settings();
            hotkey::HotkeyConfig::new(settings.hotkey, settings.command_hotkey)
        };
        hotkey::spawn(hotkeys.clone(), hotkey_tx);

        let model = cx.new(|cx| AppModel::new(hotkeys, cx));

        let pill = open_pill(model.clone(), cx);
        model.update(cx, |m, cx| {
            m.pill = Some(pill.into());
            m.sync_pill(cx);
        });

        // zappp lives in the menu bar. The window opens on first run (or with
        // ZAPPP_PAGE / ZAPPP_DEMO during development); after that it's opened
        // from the menu bar or by launching the app again.
        let first_run = !model.read(cx).settings.onboarded;
        let dev = ["ZAPPP_PAGE", "ZAPPP_DEMO", "ZAPPP_ONBOARDING"]
            .iter()
            .any(|v| std::env::var_os(v).is_some());
        // First run opens the welcome guide; it marks setup done when finished.
        let main_window = (first_run || dev).then(|| open_main(model.clone(), first_run && !dev, cx));
        if std::env::var_os("ZAPPP_DEMO").is_some() {
            model.update(cx, |m, cx| m.start_demo(cx));
        }

        // Dev self-test: ZAPPP_SELFTEST=1 holds the dictation key for 5 s once
        // the model is ready, exercising record → transcribe → paste without
        // needing Input Monitoring.
        if std::env::var_os("ZAPPP_SELFTEST").is_some() {
            let test_model = model.clone();
            cx.spawn(async move |cx| {
                use engine::hotkey::HotkeyEvent;
                let executor = cx.background_executor().clone();
                let timer = |ms| executor.timer(std::time::Duration::from_millis(ms));
                loop {
                    timer(250).await;
                    let ready = test_model
                        .read_with(cx, |m, _| matches!(m.asr_status, app_state::AsrStatus::Ready { .. }))
                        .unwrap_or(false);
                    if ready {
                        break;
                    }
                }
                log::info!("selftest: holding the dictation key");
                test_model.update(cx, |m, cx| m.handle_hotkey(HotkeyEvent::DictateDown, cx)).ok();
                timer(5000).await;
                let live = test_model.read_with(cx, |m, _| m.live_text.clone()).unwrap_or_default();
                log::info!("selftest: live preview before release: {live:?}");
                test_model.update(cx, |m, cx| m.handle_hotkey(HotkeyEvent::DictateUp, cx)).ok();
                timer(3000).await;
                let last = test_model
                    .read_with(cx, |m, _| m.history.first().map(|e| (e.text.clone(), e.total_ms)))
                    .ok()
                    .flatten();
                log::info!("selftest: result {last:?}");
            })
            .detach();
        }

        // Hotkey events → model.
        let hk_model = model.clone();
        cx.spawn(async move |cx| {
            while let Some(event) = hotkey_rx.next().await {
                log::info!("key: {event:?}");
                // gpui refuses an update while the app is busy (e.g. a native
                // callback is on the stack). Retry instead of dropping the key
                // event, and never let one failure end this loop.
                for attempt in 0..50 {
                    match hk_model.update(cx, |m, cx| m.handle_hotkey(event, cx)) {
                        Ok(()) => break,
                        Err(err) => {
                            if attempt == 0 {
                                log::warn!("deferring {event:?}: {err}");
                            }
                            cx.background_executor().timer(std::time::Duration::from_millis(10)).await;
                        }
                    }
                }
            }
        })
        .detach();

        // Menu bar item, kept in sync with the app's state.
        let (tray_tx, mut tray_rx) = futures::channel::mpsc::unbounded();
        tray::install(tray_tx);
        tray::set_state(model.read(cx).menu_state());
        cx.observe(&model, |model, cx| tray::set_state(model.read(cx).menu_state()))
            .detach();
        let main_open = main_window.is_some();
        let main = std::rc::Rc::new(std::cell::Cell::new(main_window));
        let tray_model = model.clone();
        cx.spawn(async move |cx| {
            while let Some(event) = tray_rx.next().await {
                let main = main.clone();
                let model = tray_model.clone();
                let _ = cx.update(|cx| {
                    use tray::TrayEvent as E;
                    // Brings the main window up (opening it if closed), then
                    // optionally switches page or starts the welcome guide.
                    let show = |cx: &mut App, f: Option<Box<dyn FnOnce(&mut Shell, &mut gpui::Context<Shell>)>>| {
                        let window = match main.get().filter(|w| w.update(cx, |_, w, _| w.activate_window()).is_ok()) {
                            Some(w) => {
                                show_in_dock(cx);
                                w
                            }
                            None => {
                                let w = open_main(model.clone(), false, cx);
                                main.set(Some(w));
                                w
                            }
                        };
                        if let Some(f) = f {
                            window.update(cx, |shell, _, cx| f(shell, cx)).ok();
                        }
                    };
                    match event {
                        E::Quit => cx.quit(),
                        E::Open => show(cx, None),
                        E::History => show(cx, Some(Box::new(|s, cx| s.set_page(ui::Page::History, cx)))),
                        E::Settings => show(cx, Some(Box::new(|s, cx| s.set_page(ui::Page::Settings, cx)))),
                        E::Welcome => show(cx, Some(Box::new(|s, cx| s.start_onboarding(cx)))),
                        E::ToggleDictation => model.update(cx, |m, cx| m.toggle_dictation(cx)),
                        E::CopyLast | E::CopyRecent(_) => {
                            let i = if let E::CopyRecent(i) = event { i } else { 0 };
                            if let Some(text) = model.read(cx).history.get(i).map(|e| e.text.clone()) {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                            }
                        }
                        E::PasteLast => model.update(cx, |m, cx| {
                            if let Some(text) = m.history.first().map(|e| e.text.clone()) {
                                m.insert_text(&text, false, cx);
                            }
                        }),
                        E::ToggleAutoSubmit => {
                            model.update(cx, |m, cx| m.update_settings(cx, |s| s.auto_submit = !s.auto_submit))
                        }
                        E::Microphone(name) => model.update(cx, |m, cx| m.update_settings(cx, |s| s.microphone = name)),
                        E::Model(id) => model.update(cx, |m, cx| m.select_model(id, cx)),
                    }
                });
            }
        })
        .detach();

        cx.on_action(|_: &CloseWindow, cx| {
            if let Some(window) = cx.active_window() {
                window.update(cx, |_, window, _| window.remove_window()).ok();
            }
        });

        if main_open {
            cx.activate(true);
        }

        // Back to menu-bar-only once the main window closes.
        cx.on_window_closed(|cx| {
            let any_main = cx.windows().iter().any(|w| w.downcast::<Shell>().is_some());
            if !any_main {
                cx.spawn(async |_| engine::system::set_dock_visible(false)).detach();
            }
        })
        .detach();
    });
}

/// Logs go to stderr and to ~/Library/Logs/zappp.log, so problems in the
/// bundled app (which has no terminal) can still be diagnosed.
fn init_logging() {
    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,ort=warn,transcribe_rs=warn"),
    );
    if let Some(dir) = dirs::home_dir().map(|h| h.join("Library/Logs")) {
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("zappp.log");
        // Keep the log small: start fresh once it passes 2 MB.
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000) {
            let _ = std::fs::remove_file(&path);
        }
        if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    // gpui reports some errors with only a file and line, so include them.
    builder.format(|buf, record| {
        use std::io::Write as _;
        let origin = match (record.module_path(), record.file(), record.line()) {
            (Some(module), _, _) => module.to_string(),
            (None, Some(file), Some(line)) => format!("{file}:{line}"),
            _ => String::new(),
        };
        writeln!(buf, "[{} {:<5} {origin}] {}", buf.timestamp(), record.level(), record.args())
    });
    builder.init();
}

/// While the window is open zappp appears in the Dock and ⌘-Tab with its
/// icon, like any app; AppKit is called outside the current update.
fn show_in_dock(cx: &mut App) {
    cx.spawn(async |cx| {
        engine::system::set_dock_visible(true);
        cx.update(|cx| cx.activate(true)).ok();
    })
    .detach();
}

fn open_main(model: gpui::Entity<AppModel>, onboarding: bool, cx: &mut App) -> WindowHandle<Shell> {
    show_in_dock(cx);
    let bounds = Bounds::centered(None, size(px(1120.), px(760.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("zappp".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(16.), px(18.))),
            }),
            window_background: WindowBackgroundAppearance::Blurred,
            window_min_size: Some(size(px(900.), px(600.))),
            app_id: Some("dev.zappp.app".into()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Shell::new(model, onboarding, window, cx)),
    )
    .expect("open main window")
}

fn open_pill(model: gpui::Entity<AppModel>, cx: &mut App) -> WindowHandle<Pill> {
    let display = cx.primary_display();
    let screen = display
        .as_ref()
        .map(|d| d.bounds())
        .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))));
    let origin = point(
        screen.origin.x + (screen.size.width - px(PILL_WIDTH)) / 2.,
        screen.origin.y,
    );
    let notch = engine::system::notch();
    log::info!("notch: {notch:?}");
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    origin,
                    size(px(PILL_WIDTH), px(PILL_HEIGHT)),
                ))),
                titlebar: None,
                focus: false,
                show: false,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                display_id: display.map(|d| d.id()),
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Pill::new(model, notch, cx)),
        )
        .expect("open pill window");
    // Restyling the window resizes it, which calls back into gpui; do it once
    // the current update has finished.
    cx.spawn(async move |cx| {
        let native = cx
            .update(|cx| window.update(cx, |_, window, _| engine::NativeWindow::of(window)).ok().flatten())
            .ok()
            .flatten();
        if let Some(native) = native {
            native.make_overlay();
            native.pin_to_notch(PILL_WIDTH as f64, PILL_HEIGHT as f64);
            native.order_out();
        }
    })
    .detach();
    window
}

/// `zappp --bench file.wav [model-id]` — transcribes with every installed
/// model (or just one) and reports load time and speed.
fn bench(wav: Option<&str>, only: Option<&str>) {
    let Some(path) = wav else {
        eprintln!("usage: zappp --bench file.wav [model-id] (16 kHz mono)");
        return;
    };
    let samples = transcribe_rs::audio::read_wav_samples(std::path::Path::new(path)).expect("read 16 kHz mono wav");
    let audio_s = samples.len() as f64 / 16_000.0;
    println!("audio: {audio_s:.2}s");
    for spec in models::MODELS.iter().filter(|m| m.is_installed() && only.is_none_or(|id| id == m.id)) {
        let asr = engine::asr::Asr::new();
        let load = match asr.load(spec) {
            Ok(t) => t,
            Err(e) => {
                println!("{}: load failed: {e:#}", spec.id);
                continue;
            }
        };
        let mut best = f64::MAX;
        let mut text = String::new();
        for _ in 0..3 {
            let t = Instant::now();
            text = asr.transcribe(&samples).unwrap_or_else(|e| format!("<error: {e:#}>"));
            best = best.min(t.elapsed().as_secs_f64());
        }
        println!(
            "{}: load {:.2}s · best {:.0} ms · {:.0}x realtime\n  → {text}",
            spec.id,
            load.as_secs_f64(),
            best * 1000.0,
            audio_s / best
        );
    }
}
