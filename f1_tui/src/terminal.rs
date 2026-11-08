//! Terminal ownership: fallible setup, best-effort all-step restoration, UI-thread panic cleanup.
use crossterm::{
    cursor::{Hide, Show},
    execute,
    style::ResetColor,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::{
    io,
    panic::{self, PanicHookInfo},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
/// Small I/O seam for fault injection; production uses Crossterm on the attached console.
pub trait TerminalControl {
    fn raw_mode(&mut self, enabled: bool) -> io::Result<()>;
    fn alternate_screen(&mut self, enabled: bool) -> io::Result<()>;
    fn cursor_visible(&mut self, visible: bool) -> io::Result<()>;
}
pub struct SystemControl;
impl TerminalControl for SystemControl {
    fn raw_mode(&mut self, on: bool) -> io::Result<()> {
        if on {
            enable_raw_mode()
        } else {
            disable_raw_mode()
        }
    }
    fn alternate_screen(&mut self, on: bool) -> io::Result<()> {
        if on {
            execute!(io::stdout(), EnterAlternateScreen)
        } else {
            execute!(io::stdout(), LeaveAlternateScreen)
        }
    }
    fn cursor_visible(&mut self, on: bool) -> io::Result<()> {
        if on {
            execute!(io::stdout(), ResetColor, Show)
        } else {
            execute!(io::stdout(), Hide)
        }
    }
}
pub struct TerminalSession<C: TerminalControl = SystemControl> {
    control: C,
    raw: bool,
    alternate: bool,
    cursor: bool,
}
impl<C: TerminalControl> TerminalSession<C> {
    pub fn start(control: C) -> io::Result<Self> {
        let mut guard = Self {
            control,
            raw: false,
            alternate: false,
            cursor: false,
        };
        // Arm before each write: an error may have partially changed console state.
        guard.raw = true;
        guard.control.raw_mode(true)?;
        guard.alternate = true;
        guard.control.alternate_screen(true)?;
        guard.cursor = true;
        guard.control.cursor_visible(false)?;
        Ok(guard)
    }
    pub fn restore(&mut self) -> io::Result<()> {
        let mut error = None;
        if self.raw {
            record(self.control.raw_mode(false), &mut self.raw, &mut error);
        }
        if self.alternate {
            record(
                self.control.alternate_screen(false),
                &mut self.alternate,
                &mut error,
            );
        }
        if self.cursor {
            record(
                self.control.cursor_visible(true),
                &mut self.cursor,
                &mut error,
            );
        }
        error.map_or(Ok(()), Err)
    }
}
fn record(result: io::Result<()>, armed: &mut bool, first: &mut Option<io::Error>) {
    match result {
        Ok(()) => *armed = false,
        Err(error) => {
            if first.is_none() {
                *first = Some(error);
            }
        }
    }
}
impl<C: TerminalControl> Drop for TerminalSession<C> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
fn restore_system() {
    let mut session = TerminalSession {
        control: SystemControl,
        raw: true,
        alternate: true,
        cursor: true,
    };
    let _ = session.restore();
}
type Hook = dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static;
pub struct PanicCleanup {
    previous: Option<Arc<Hook>>,
    _owner: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl PanicCleanup {
    pub fn install() -> Self {
        let previous: Arc<Hook> = Arc::from(panic::take_hook());
        let hook = previous.clone();
        let ui_thread = std::thread::current().id();
        panic::set_hook(Box::new(move |info| {
            // Loader panics are caught by its worker. They must not tear down the UI.
            if std::thread::current().id() == ui_thread {
                restore_system();
            }
            hook(info);
        }));
        Self {
            previous: Some(previous),
            _owner: std::marker::PhantomData,
        }
    }
}
impl Drop for PanicCleanup {
    fn drop(&mut self) {
        // set_hook itself panics during unwinding. A fatal UI panic exits this process;
        // normal/error returns restore the previous process hook.
        if !std::thread::panicking()
            && let Some(previous) = self.previous.take()
        {
            panic::set_hook(Box::new(move |info| previous(info)));
        }
    }
}
#[derive(Clone, Default)]
pub struct Shutdown(Arc<AtomicBool>);
impl Shutdown {
    pub fn install() -> Result<Self, ctrlc::Error> {
        let flag = Self::default();
        let signal = flag.clone();
        ctrlc::try_set_handler(move || signal.0.store(true, Ordering::SeqCst))?;
        Ok(flag)
    }
    pub fn requested(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
