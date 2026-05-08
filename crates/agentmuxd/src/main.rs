mod layout_ops;
mod pty;
mod server;
mod vt;

use anyhow::Result;

fn main() -> Result<()> {
    init_tracing();

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    rt.spawn(async move {
        if let Err(e) = server::listener::run_server(shutdown_rx).await {
            tracing::error!("agentmuxd server: {e}");
        }
    });

    run_tray(shutdown_tx)?;

    rt.shutdown_timeout(std::time::Duration::from_secs(5));
    Ok(())
}

fn run_tray(shutdown_tx: tokio::sync::oneshot::Sender<()>) -> Result<()> {
    use tray_icon::{
        menu::{Menu, MenuItem, PredefinedMenuItem},
        TrayIconBuilder,
    };

    let quit_item = MenuItem::new("Quit agentmuxd", true, None);
    let open_item = MenuItem::new("New Session", true, None);
    let quit_id = quit_item.id().clone();
    let open_id = open_item.id().clone();

    let menu = Menu::new();
    let _ = menu.append_items(&[
        &open_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ]);

    let _tray = TrayIconBuilder::new()
        .with_icon(amber_icon())
        .with_tooltip("agentmuxd")
        .with_menu(Box::new(menu))
        .build()?;

    let mut quit_tx = Some(shutdown_tx);

    message_loop(|event_id| {
        if event_id == quit_id {
            if let Some(tx) = quit_tx.take() {
                let _ = tx.send(());
            }
            return true; // exit loop
        }
        if event_id == open_id {
            launch_tui();
        }
        false
    });

    Ok(())
}

fn message_loop(mut on_menu: impl FnMut(tray_icon::menu::MenuId) -> bool) {
    use tray_icon::menu::MenuEvent;

    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let mut msg: MSG = std::mem::zeroed();
        loop {
            while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    return;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if on_menu(event.id) {
                    PostQuitMessage(0);
                    return;
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[cfg(not(windows))]
    {
        eprintln!("agentmuxd tray is Windows-only");
        loop {
            if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                if on_menu(event.id) {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

fn launch_tui() {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    let tui = exe_dir.join("agentmux.exe");
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", tui.to_str().unwrap_or("agentmux")])
        .spawn();
}

fn amber_icon() -> tray_icon::Icon {
    const SZ: u32 = 16;
    let rgba: Vec<u8> = (0..(SZ * SZ) as usize)
        .flat_map(|_| [0xFF_u8, 0xB0, 0x00, 0xFF])
        .collect();
    tray_icon::Icon::from_rgba(rgba, SZ, SZ).expect("tray icon creation failed")
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let log_path = "agentmuxd.log";
    if let Ok(f) = std::fs::File::create(log_path) {
        let _ = fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(std::sync::Mutex::new(f))
            .try_init();
    }
}
