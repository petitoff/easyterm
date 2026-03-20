mod config;
mod gui;
mod pty;
mod session;

use config::AppConfig;
use easyterm_core::Terminal;
use easyterm_remote::ProfileStore;
use easyterm_render::{select_backend, FrameModel};
use gui::run_gui;
use pty::{capture_local_session, run_local_session, PtySize};
use session::{LocalSessionSpec, SessionManager};
use std::path::Path;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        None => launch_gui()?,
        Some("help" | "--help" | "-h") => print_help(),
        Some("gui") => launch_gui()?,
        Some("cli") => launch_default_shell()?,
        Some("sample-config") => {
            println!("{}", AppConfig::sample_toml());
        }
        Some("inspect-config") => {
            let path = args.next().unwrap_or_else(|| "easyterm.toml".into());
            inspect_config(Path::new(&path))?;
        }
        Some("replay") => {
            let path = args
                .next()
                .ok_or("replay requires a path to an ANSI/text fixture")?;
            replay_fixture(Path::new(&path))?;
        }
        Some("capture-shell") => {
            let command = args.collect::<Vec<_>>().join(" ");
            if command.trim().is_empty() {
                return Err("capture-shell requires a shell command".into());
            }
            capture_shell(&command)?;
        }
        Some("capture-local") => {
            let program = args
                .next()
                .ok_or("capture-local requires a program path or name")?;
            let spec = LocalSessionSpec::new(program).with_args(args.collect());
            capture_and_print(&spec, &load_runtime_config()?.shell.term)?;
        }
        Some(other) => {
            return Err(format!("unknown command: {other}").into());
        }
    }

    Ok(())
}

fn launch_default_shell() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_runtime_config()?;
    let spec = LocalSessionSpec::new(config.shell.program.clone()).with_args(config.shell.args);
    let status = run_local_session(&spec, &config.shell.term)?;

    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(format!("shell exited with status {code}").into()),
        None => Err("shell exited from signal".into()),
    }
}

fn launch_gui() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_runtime_config()?;
    run_gui(config)
}

fn inspect_config(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load(path)?;
    let mut sessions = SessionManager::new();
    sessions.spawn_local(
        LocalSessionSpec::new(&config.shell.program).with_args(config.shell.args.clone()),
    );

    let profiles = ProfileStore {
        profiles: config.ssh_profiles.clone(),
    };
    if let Some(profile) = profiles.profiles.first() {
        let spec = profiles.quick_connect(&profile.name)?;
        sessions.spawn_ssh(spec);
    }

    println!("theme: {}", config.theme.name);
    println!("font: {} {}", config.font.family, config.font.size);
    println!("shell: {} {:?}", config.shell.program, config.shell.args);
    println!("term: {}", config.shell.term);
    println!("renderer: {:?}", config.renderer.preference);
    println!("scrollback_limit: {}", config.scrollback_limit);
    println!("ssh_profiles: {}", profiles.profiles.len());
    println!("sessions_bootstrapped: {}", sessions.sessions().len());

    Ok(())
}

fn capture_shell(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_runtime_config()?;
    let mut args = config.shell.args.clone();
    args.push("-lc".into());
    args.push(command.to_string());

    let spec = LocalSessionSpec::new(config.shell.program).with_args(args);
    capture_and_print(&spec, &config.shell.term)
}

fn capture_and_print(
    spec: &LocalSessionSpec,
    term: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let size = PtySize::default();
    let transcript = capture_local_session(spec, term, size)?;
    let terminal = transcript.render(size);

    for line in trim_trailing_empty_lines(terminal.visible_lines()) {
        println!("{line}");
    }

    match transcript.status.code() {
        Some(code) => println!("exit_code: {code}"),
        None => println!("exit_code: signal"),
    }
    println!("scrollback_lines: {}", terminal.scrollback().len());

    Ok(())
}

fn replay_fixture(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(path)?;
    let mut terminal = Terminal::new(80, 24);
    terminal.feed(&bytes);

    let mut renderer = select_backend(Default::default(), false);
    let stats = renderer.render(&FrameModel {
        lines: terminal.visible_lines(),
        cursor: terminal.cursor(),
    });

    for line in terminal.visible_lines() {
        println!("{line}");
    }
    println!("cells_drawn: {}", stats.cells_drawn);
    println!("scrollback_lines: {}", terminal.scrollback().len());

    Ok(())
}

fn print_help() {
    println!("easyterm");
    println!("commands:");
    println!("  <no args>                  launch the GUI");
    println!("  gui                        launch the GUI explicitly");
    println!("  cli                        launch the local shell in terminal passthrough mode");
    println!("  sample-config              print a sample TOML config");
    println!("  inspect-config [path]      validate config and summarize bootstrapped state");
    println!("  replay <path>              replay ANSI/text output through the terminal core");
    println!(
        "  capture-shell <command>    run a shell command inside a Linux PTY and render the result"
    );
    println!(
        "  capture-local <prog> ...   run a local program inside a Linux PTY and render the result"
    );
}

fn trim_trailing_empty_lines(mut lines: Vec<String>) -> Vec<String> {
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn load_runtime_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    let path = Path::new("easyterm.toml");
    if path.exists() {
        Ok(AppConfig::load(path)?)
    } else {
        Ok(AppConfig::default())
    }
}
