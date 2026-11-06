//! Instancia única por sesión: el primero escucha en un socket, los demás le envían la orden.
use colorshot_core::command::Command;
use std::{
    io::{self, BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
};

pub struct Instance {
    pub listener: UnixListener,
    path: PathBuf,
    // El candado vive lo mismo que la instancia.
    _lock: std::fs::File,
}

pub fn socket_path() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("colorshot.sock")
}

/// `Ok(None)` si otra instancia recibió la orden.
pub fn acquire(path: &Path, command: &Command) -> io::Result<Option<Instance>> {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))?;
    // Un candado no bloqueante: si lo tiene otra instancia, se le envía la orden.
    if lock.try_lock().is_err() {
        send(path, command)?;
        return Ok(None);
    }
    // Con el candado en mano, cualquier socket que quede es de una instancia muerta.
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(Some(Instance {
        listener: UnixListener::bind(path)?,
        path: path.into(),
        _lock: lock,
    }))
}

fn send(path: &Path, command: &Command) -> io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.write_all(format!("{}\n", command.to_line()).as_bytes())
}

pub fn read(stream: UnixStream) -> Option<Command> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    Command::from_line(&line)
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn second_start_forwards_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sock");
        let owner = acquire(&path, &Command::Daemon).unwrap().unwrap();
        let forwarded = Command::Edit("/tmp/x.png".into());
        assert!(acquire(&path, &forwarded).unwrap().is_none());
        let (stream, _) = owner.listener.accept().unwrap();
        assert_eq!(read(stream), Some(forwarded));
    }
}
