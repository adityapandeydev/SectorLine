use sectorline::terminal::{TerminalControl, TerminalSession};
use std::{cell::RefCell, io, rc::Rc};
#[derive(Default, Debug)]
struct State {
    raw: bool,
    alternate: bool,
    hidden: bool,
    fail: Option<&'static str>,
    calls: Vec<&'static str>,
}
#[derive(Clone)]
struct Control(Rc<RefCell<State>>);
impl Control {
    fn change(
        &mut self,
        operation: &'static str,
        raw: Option<bool>,
        alt: Option<bool>,
        cursor: Option<bool>,
    ) -> io::Result<()> {
        let mut s = self.0.borrow_mut();
        s.calls.push(operation);
        if let Some(v) = raw {
            s.raw = v;
        }
        if let Some(v) = alt {
            s.alternate = v;
        }
        if let Some(v) = cursor {
            s.hidden = !v;
        }
        if s.fail == Some(operation) {
            s.fail = None;
            return Err(io::Error::other(operation));
        }
        Ok(())
    }
}
impl TerminalControl for Control {
    fn raw_mode(&mut self, on: bool) -> io::Result<()> {
        self.change(if on { "raw-on" } else { "raw-off" }, Some(on), None, None)
    }
    fn alternate_screen(&mut self, on: bool) -> io::Result<()> {
        self.change(if on { "alt-on" } else { "alt-off" }, None, Some(on), None)
    }
    fn cursor_visible(&mut self, on: bool) -> io::Result<()> {
        self.change(if on { "show" } else { "hide" }, None, None, Some(on))
    }
}
fn clean(s: &State) {
    assert!(!s.raw, "Raw input must be restored");
    assert!(!s.alternate, "Original screen must be restored");
    assert!(!s.hidden, "Cursor must be visible");
}
#[test]
fn terminal_layout_normal_and_error_unwind_restore_all_modes() {
    for error in [false, true] {
        let state = Rc::new(RefCell::new(State::default()));
        let run = || -> io::Result<()> {
            let _session = TerminalSession::start(Control(state.clone()))?;
            assert!(state.borrow().raw && state.borrow().alternate && state.borrow().hidden);
            if error {
                return Err(io::Error::other("draw error"));
            }
            Ok(())
        };
        assert_eq!(run().is_err(), error);
        clean(&state.borrow());
    }
}
#[test]
fn terminal_layout_partial_setup_failure_restores_modes_and_preserves_error() {
    for step in ["raw-on", "alt-on", "hide"] {
        let state = Rc::new(RefCell::new(State {
            fail: Some(step),
            ..State::default()
        }));
        let error = match TerminalSession::start(Control(state.clone())) {
            Ok(_) => panic!("Expected setup failure"),
            Err(e) => e,
        };
        assert_eq!(error.to_string(), step);
        clean(&state.borrow());
    }
}
#[test]
fn terminal_layout_cleanup_failure_does_not_skip_other_steps_and_drop_retries() {
    for step in ["raw-off", "alt-off", "show"] {
        let state = Rc::new(RefCell::new(State::default()));
        let mut guard = TerminalSession::start(Control(state.clone())).unwrap();
        state.borrow_mut().fail = Some(step);
        assert_eq!(guard.restore().unwrap_err().to_string(), step);
        clean(&state.borrow());
        let before = state.borrow().calls.len();
        drop(guard);
        assert!(
            state.borrow().calls.len() > before,
            "Failed cleanup must be retried"
        );
        clean(&state.borrow());
    }
}
#[test]
fn terminal_layout_panic_unwind_restores_and_cleanup_is_idempotent() {
    let state = Rc::new(RefCell::new(State::default()));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = TerminalSession::start(Control(state.clone())).unwrap();
        panic!("UI draw panic");
    }));
    assert!(r.is_err());
    clean(&state.borrow());
    let mut guard = TerminalSession::start(Control(state.clone())).unwrap();
    guard.restore().unwrap();
    let calls = state.borrow().calls.len();
    guard.restore().unwrap();
    drop(guard);
    assert_eq!(calls, state.borrow().calls.len());
}
