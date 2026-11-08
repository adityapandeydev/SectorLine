//! Run only inside a disposable real terminal/PTY; modes intentionally inject failures.
//! cargo run --example terminal_smoke -- normal|error|panic|worker-panic|startup-error|signal
use sectorline::terminal::{
    PanicCleanup, Shutdown, SystemControl, TerminalControl, TerminalSession,
};
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};
struct FailAlt;
impl TerminalControl for FailAlt {
    fn raw_mode(&mut self, v: bool) -> io::Result<()> {
        SystemControl.raw_mode(v)
    }
    fn alternate_screen(&mut self, v: bool) -> io::Result<()> {
        SystemControl.alternate_screen(v)?;
        if v {
            Err(io::Error::other("injected alternate-screen setup failure"))
        } else {
            Ok(())
        }
    }
    fn cursor_visible(&mut self, v: bool) -> io::Result<()> {
        SystemControl.cursor_visible(v)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "normal".into());
    if ![
        "normal",
        "error",
        "panic",
        "worker-panic",
        "startup-error",
        "signal",
    ]
    .contains(&mode.as_str())
    {
        return Err("Unknown terminal smoke mode".into());
    }
    let shutdown = Shutdown::install()?;
    let _hook = PanicCleanup::install();
    let initial = crossterm::terminal::is_raw_mode_enabled()?;
    let result = std::panic::catch_unwind(|| -> io::Result<()> {
        if mode == "startup-error" {
            let _session = TerminalSession::start(FailAlt)?;
            return Ok(());
        }
        let mut guard = TerminalSession::start(SystemControl)?;
        write!(
            io::stdout(),
            "ACTIVE raw={}\r\n",
            crossterm::terminal::is_raw_mode_enabled()?
        )?;
        io::stdout().flush()?;
        match mode.as_str() {
            "error" => return Err(io::Error::other("injected draw/read error")),
            "panic" => panic!("injected UI draw panic"),
            "worker-panic" => {
                std::thread::spawn(|| {
                    let _ = std::panic::catch_unwind(|| panic!("injected caught worker panic"));
                })
                .join()
                .unwrap();
                assert!(crossterm::terminal::is_raw_mode_enabled()?);
                write!(io::stdout(), "WORKER_UI_RETAINED\r\n")?;
            }
            "signal" => {
                let start = Instant::now();
                while !shutdown.requested() {
                    if start.elapsed() > Duration::from_secs(15) {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "No signal received",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            _ => {}
        }
        guard.restore()
    });
    let restored = !crossterm::terminal::is_raw_mode_enabled()?;
    println!("RESTORED initial_raw={initial} raw_off={restored}");
    if !restored {
        return Err("Raw terminal input was not restored".into());
    }
    match result {
        Ok(result) => Ok(result?),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
