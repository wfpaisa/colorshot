//! Punto de entrada: interpreta la orden y la ejecuta en la instancia viva o en esta.
mod flows;
mod instance;
mod windows;

use rapidshot_core::command::{Command, USAGE};
use std::sync::mpsc;

fn main() -> anyhow::Result<()> {
    env_logger::init();
    rapidshot_core::i18n::set(rapidshot_core::settings::Settings::load().language);
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(command) = Command::parse(&args) else {
        anyhow::bail!(USAGE);
    };
    let runtime = tokio::runtime::Runtime::new()?;
    // Un proceso hijo de la instancia (o un Pin) ejecuta su orden sin pasar por el socket.
    if std::env::var_os(CHILD).is_some() || matches!(command, Command::Pin(_)) {
        let result = flows::run(&command, &runtime);
        if let Err(error) = &result {
            notify_error(error);
        }
        return result;
    }
    let Some(owner) = instance::acquire(&instance::socket_path(), &command)? else {
        // La instancia viva se encarga.
        return Ok(());
    };
    // Sin instancia previa: una orden suelta se ejecuta y termina; sin orden queda residente.
    if command != Command::Daemon {
        drop(owner);
        return flows::run(&command, &runtime);
    }
    let (sender, commands) = mpsc::channel();
    rapidshot_ui::tray::spawn(runtime.handle(), sender.clone());
    std::thread::spawn(move || {
        for stream in owner.listener.incoming().flatten() {
            if let Some(command) = instance::read(stream) {
                let _ = sender.send(command);
            }
        }
    });
    for command in commands {
        match command {
            Command::Quit => break,
            Command::Daemon => {}
            command => {
                if let Err(error) = spawn_child(&command) {
                    log::error!("{error:#}");
                    notify_error(&error);
                }
            }
        }
    }
    Ok(())
}

/// Ruta del binario para lanzar hijos. Si se reinstaló con la instancia viva, Linux añade
/// « (deleted)» a la ruta vieja: se usa la nueva, que está en el mismo sitio.
pub fn exe() -> std::io::Result<std::path::PathBuf> {
    let path = std::env::current_exe()?;
    Ok(
        match path.to_str().and_then(|p| p.strip_suffix(" (deleted)")) {
            Some(fresh) => fresh.into(),
            None => path,
        },
    )
}

/// Marca del proceso hijo que ejecuta una orden de la instancia residente.
pub const CHILD: &str = "RAPIDSHOT_CHILD";

// Cada orden corre en su propio proceso. eframe reutiliza un único bucle de eventos: al
// volver de una ventana nadie atiende la conexión Wayland, y la ventana cerrada puede
// quedarse en pantalla sin responder («Not Responding») y bloquear las órdenes siguientes.
// Al terminar el proceso, el compositor retira sus ventanas.
fn spawn_child(command: &Command) -> anyhow::Result<()> {
    let line = command.to_line();
    let args: Vec<&str> = match line.split_once(' ') {
        Some((verb, path)) => vec![verb, path],
        None => vec![line.as_str()],
    };
    let mut child = std::process::Command::new(exe()?)
        .args(args)
        .env(CHILD, "1")
        .spawn()?;
    // Recoge al hijo sin bloquear la cola de órdenes.
    std::thread::spawn(move || child.wait());
    Ok(())
}

// Los errores visibles («No area to repeat»…) van como notificación del escritorio.
pub fn notify_error(error: &anyhow::Error) {
    let _ = std::process::Command::new("notify-send")
        .args([
            "--app-name=rapidshot",
            "rapidshot",
            rapidshot_core::i18n::t(&error.to_string()),
        ])
        .spawn()
        .and_then(|mut child| child.wait());
}
