use crate::session::LocalSessionSpec;
use easyterm_core::Terminal;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

#[derive(Debug)]
pub struct SessionTranscript {
    pub output: Vec<u8>,
    pub status: ExitStatus,
}

impl SessionTranscript {
    pub fn render(&self, size: PtySize) -> Terminal {
        let mut terminal = Terminal::new(size.cols as usize, size.rows as usize);
        terminal.feed(&self.output);
        terminal
    }
}

#[derive(Debug)]
pub enum LocalPtyError {
    Open(io::Error),
    Duplicate(io::Error),
    Spawn(io::Error),
    Wait(io::Error),
    Read(io::Error),
    Write(io::Error),
    Poll(io::Error),
    TerminalMode(io::Error),
    ReaderPanicked,
}

impl std::fmt::Display for LocalPtyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocalPtyError::Open(err) => write!(f, "failed to open PTY: {err}"),
            LocalPtyError::Duplicate(err) => write!(f, "failed to duplicate PTY fd: {err}"),
            LocalPtyError::Spawn(err) => write!(f, "failed to spawn PTY child: {err}"),
            LocalPtyError::Wait(err) => write!(f, "failed waiting for PTY child: {err}"),
            LocalPtyError::Read(err) => write!(f, "failed reading PTY output: {err}"),
            LocalPtyError::Write(err) => write!(f, "failed writing PTY output: {err}"),
            LocalPtyError::Poll(err) => write!(f, "failed polling PTY descriptors: {err}"),
            LocalPtyError::TerminalMode(err) => {
                write!(f, "failed configuring terminal raw mode: {err}")
            }
            LocalPtyError::ReaderPanicked => write!(f, "PTY reader thread panicked"),
        }
    }
}

impl std::error::Error for LocalPtyError {}

pub struct PtyRuntime {
    child: Child,
    master_writer: OwnedFd,
    output_rx: Receiver<Vec<u8>>,
    pid: i32,
}

impl PtyRuntime {
    pub fn write_input(&mut self, bytes: &[u8]) -> Result<(), LocalPtyError> {
        write_all_fd(self.master_writer.as_raw_fd(), bytes).map_err(LocalPtyError::Write)
    }

    pub fn resize(&mut self, size: PtySize) -> Result<(), LocalPtyError> {
        let winsize = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let result =
            unsafe { libc::ioctl(self.master_writer.as_raw_fd(), libc::TIOCSWINSZ, &winsize) };
        if result == -1 {
            return Err(LocalPtyError::Write(io::Error::last_os_error()));
        }

        let signal_result = unsafe { libc::kill(self.pid, libc::SIGWINCH) };
        if signal_result == -1 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                return Err(LocalPtyError::Write(err));
            }
        }

        Ok(())
    }

    pub fn drain_output(&mut self) -> Vec<Vec<u8>> {
        let mut chunks = Vec::new();
        loop {
            match self.output_rx.try_recv() {
                Ok(bytes) => chunks.push(bytes),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        chunks
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, LocalPtyError> {
        self.child.try_wait().map_err(LocalPtyError::Wait)
    }

    pub fn terminate(&mut self) -> Result<(), LocalPtyError> {
        match self.child.kill() {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::InvalidInput => {}
            Err(err) => return Err(LocalPtyError::Wait(err)),
        }
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for PtyRuntime {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

pub fn spawn_local_runtime(
    spec: &LocalSessionSpec,
    term: &str,
    size: PtySize,
) -> Result<PtyRuntime, LocalPtyError> {
    let (master, slave) = open_pty(size).map_err(LocalPtyError::Open)?;
    let master_writer = dup_fd(master.as_raw_fd()).map_err(LocalPtyError::Duplicate)?;
    let child = spawn_pty_child(spec, term, &slave).map_err(LocalPtyError::Spawn)?;
    let pid = child.id() as i32;
    drop(slave);

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut file = file_from_fd(master);
        let mut buffer = [0_u8; 4096];

        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.raw_os_error() == Some(libc::EIO) => break,
                Err(_) => break,
            }
        }
    });

    Ok(PtyRuntime {
        child,
        master_writer,
        output_rx: rx,
        pid,
    })
}

pub fn run_local_session(spec: &LocalSessionSpec, term: &str) -> Result<ExitStatus, LocalPtyError> {
    let size = current_pty_size();
    let (master, slave) = open_pty(size).map_err(LocalPtyError::Open)?;
    let mut child = spawn_pty_child(spec, term, &slave).map_err(LocalPtyError::Spawn)?;
    drop(slave);

    let _raw_mode_guard = RawModeGuard::new().map_err(LocalPtyError::TerminalMode)?;
    stream_pty(master.as_raw_fd())?;

    child.wait().map_err(LocalPtyError::Wait)
}

pub fn capture_local_session(
    spec: &LocalSessionSpec,
    term: &str,
    size: PtySize,
) -> Result<SessionTranscript, LocalPtyError> {
    let (master, slave) = open_pty(size).map_err(LocalPtyError::Open)?;
    let mut child = spawn_pty_child(spec, term, &slave).map_err(LocalPtyError::Spawn)?;
    drop(slave);

    let reader = thread::spawn(move || read_master(master));
    let status = child.wait().map_err(LocalPtyError::Wait)?;
    let output = reader
        .join()
        .map_err(|_| LocalPtyError::ReaderPanicked)?
        .map_err(LocalPtyError::Read)?;

    Ok(SessionTranscript { output, status })
}

pub fn current_pty_size() -> PtySize {
    query_winsize(libc::STDOUT_FILENO)
        .or_else(|| query_winsize(libc::STDIN_FILENO))
        .unwrap_or_default()
}

fn open_pty(size: PtySize) -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master = -1;
    let mut slave = -1;
    let winsize = libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            &winsize,
        )
    };

    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    let master = unsafe { OwnedFd::from_raw_fd(master) };
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };
    Ok((master, slave))
}

fn spawn_pty_child(spec: &LocalSessionSpec, term: &str, slave: &OwnedFd) -> io::Result<Child> {
    let stdin = file_from_fd(dup_fd(slave.as_raw_fd())?);
    let stdout = file_from_fd(dup_fd(slave.as_raw_fd())?);
    let stderr = file_from_fd(dup_fd(slave.as_raw_fd())?);

    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    command.env("TERM", term);
    command.stdin(Stdio::from(stdin));
    command.stdout(Stdio::from(stdout));
    command.stderr(Stdio::from(stderr));

    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }

            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(io::Error::last_os_error());
            }

            Ok(())
        });
    }

    let child = command.spawn()?;
    drop(command);
    Ok(child)
}

fn dup_fd(fd: RawFd) -> io::Result<OwnedFd> {
    let duplicated = unsafe { libc::dup(fd) };
    if duplicated == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

fn file_from_fd(fd: OwnedFd) -> File {
    File::from(fd)
}

fn read_master(master: OwnedFd) -> io::Result<Vec<u8>> {
    let mut file = file_from_fd(master);
    let mut output = Vec::new();
    let mut buffer = [0_u8; 4096];

    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(err) if err.raw_os_error() == Some(libc::EIO) => break,
            Err(err) => return Err(err),
        }
    }

    Ok(output)
}

fn stream_pty(master_fd: RawFd) -> Result<(), LocalPtyError> {
    let mut stdin_open = true;
    let mut stdin_buffer = [0_u8; 4096];
    let mut master_buffer = [0_u8; 4096];
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    loop {
        let mut poll_fds = [
            libc::pollfd {
                fd: master_fd,
                events: libc::POLLIN | libc::POLLHUP,
                revents: 0,
            },
            libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: if stdin_open { libc::POLLIN } else { 0 },
                revents: 0,
            },
        ];

        let ready = unsafe { libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as _, -1) };
        if ready == -1 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(LocalPtyError::Poll(err));
        }

        if stdin_open && (poll_fds[1].revents & libc::POLLIN) != 0 {
            match read_fd(libc::STDIN_FILENO, &mut stdin_buffer).map_err(LocalPtyError::Read)? {
                Some(0) | None => stdin_open = false,
                Some(read) => {
                    if let Err(err) = write_all_fd(master_fd, &stdin_buffer[..read]) {
                        match err.raw_os_error() {
                            Some(code) if code == libc::EIO || code == libc::EPIPE => {
                                stdin_open = false;
                            }
                            _ => return Err(LocalPtyError::Write(err)),
                        }
                    }
                }
            }
        }

        let master_has_event =
            (poll_fds[0].revents & libc::POLLIN) != 0 || (poll_fds[0].revents & libc::POLLHUP) != 0;
        if master_has_event {
            match read_fd(master_fd, &mut master_buffer).map_err(LocalPtyError::Read)? {
                Some(0) | None => break,
                Some(read) => {
                    stdout
                        .write_all(&master_buffer[..read])
                        .map_err(LocalPtyError::Write)?;
                    stdout.flush().map_err(LocalPtyError::Write)?;
                }
            }
        }
    }

    Ok(())
}

fn read_fd(fd: RawFd, buffer: &mut [u8]) -> io::Result<Option<usize>> {
    let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
    if read == 0 {
        return Ok(Some(0));
    }
    if read == -1 {
        let err = io::Error::last_os_error();
        return match err.raw_os_error() {
            Some(code) if code == libc::EIO => Ok(None),
            Some(code) if code == libc::EINTR => Ok(Some(0)),
            _ => Err(err),
        };
    }

    Ok(Some(read as usize))
}

fn write_all_fd(fd: RawFd, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let written = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
        if written == -1 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(err);
        }
        data = &data[written as usize..];
    }

    Ok(())
}

fn query_winsize(fd: RawFd) -> Option<PtySize> {
    if unsafe { libc::isatty(fd) } != 1 {
        return None;
    }

    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as _, &mut winsize) };

    if result == -1 || winsize.ws_col == 0 || winsize.ws_row == 0 {
        return None;
    }

    Some(PtySize {
        cols: winsize.ws_col,
        rows: winsize.ws_row,
    })
}

struct RawModeGuard {
    fd: RawFd,
    original: Option<libc::termios>,
}

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        let fd = libc::STDIN_FILENO;
        if unsafe { libc::isatty(fd) } != 1 {
            return Ok(Self { fd, original: None });
        }

        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut original) } == -1 {
            return Err(io::Error::last_os_error());
        }

        let mut raw = original;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } == -1 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            fd,
            original: Some(original),
        })
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if let Some(original) = self.original.take() {
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, &original);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{capture_local_session, PtySize};
    use crate::session::LocalSessionSpec;

    #[test]
    fn captures_shell_output_through_pty() {
        let spec = LocalSessionSpec::new("/bin/sh")
            .with_args(vec!["-lc".into(), "printf 'alpha\\nbeta\\n'".into()]);

        let transcript = capture_local_session(&spec, "xterm-256color", PtySize::default())
            .expect("pty capture should succeed");

        assert!(transcript.status.success());

        let terminal = transcript.render(PtySize::default());
        let lines = terminal.visible_lines();
        assert_eq!(lines[0], "alpha");
        assert_eq!(lines[1], "beta");
    }
}
