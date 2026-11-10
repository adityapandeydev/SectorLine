//! Explicit standalone commands. Parse before filesystem or provider work.
use std::{collections::BTreeSet, path::PathBuf};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Interactive { session: Option<u32>, offline: bool },
    Demo,
    Download(u32),
    Catalogue,
    Paths,
    Help,
    Version,
}
#[derive(Debug)]
pub struct Options {
    pub mode: Mode,
    pub catalogue_path: Option<PathBuf>,
    pub recording_root: Option<PathBuf>,
    pub ascii: bool,
    pub no_color: bool,
}
impl Options {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        if args
            .iter()
            .skip(1)
            .any(|a| a == "--live" || a.eq_ignore_ascii_case("--gp=live"))
            || args
                .windows(2)
                .any(|a| a[0] == "--gp" && a[1].eq_ignore_ascii_case("live"))
        {
            return Err(crate::live::LIVE_UNAVAILABLE.into());
        }
        if args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h" | "--version" | "-V") {
            return Ok(Self {
                mode: if matches!(args[1].as_str(), "--help" | "-h") {
                    Mode::Help
                } else {
                    Mode::Version
                },
                catalogue_path: None,
                recording_root: None,
                ascii: false,
                no_color: false,
            });
        }
        // The metadata CLI owns its stricter catalogue-specific validation.
        if args.iter().any(|a| a == "--catalogue") {
            return Ok(Self {
                mode: Mode::Catalogue,
                catalogue_path: None,
                recording_root: None,
                ascii: false,
                no_color: false,
            });
        }
        let mut options = Self {
            mode: Mode::Interactive {
                session: None,
                offline: false,
            },
            catalogue_path: None,
            recording_root: None,
            ascii: false,
            no_color: false,
        };
        let mut selected = None;
        let mut offline = false;
        let mut seen = BTreeSet::new();
        let mut i = 1;
        while i < args.len() {
            let flag = args[i].as_str();
            let canonical = if flag == "--headless" { "--demo" } else { flag };
            if !seen.insert(canonical) {
                return Err(format!("Duplicate option: {flag}"));
            }
            match flag {
                "--ascii" => options.ascii = true,
                "--no-color" => options.no_color = true,
                "--offline" => offline = true,
                "--demo" | "--headless" | "--paths" => {
                    if selected.is_some() {
                        return Err(
                            "Choose one command: --demo, --download, --replay or --prepare-session"
                                .into(),
                        );
                    }
                    selected = Some(if flag == "--paths" {
                        Mode::Paths
                    } else {
                        Mode::Demo
                    });
                }
                "--download" | "--replay" | "--prepare-session" | "--catalogue-cache"
                | "--recording-root" => {
                    i += 1;
                    let value = args
                        .get(i)
                        .filter(|s| !s.starts_with('-') && !s.is_empty())
                        .ok_or_else(|| format!("{flag} needs a value"))?;
                    match flag {
                        "--catalogue-cache" => options.catalogue_path = Some(PathBuf::from(value)),
                        "--recording-root" => options.recording_root = Some(PathBuf::from(value)),
                        _ => {
                            if selected.is_some() {
                                return Err("Choose one playback/download command".into());
                            }
                            let key =
                                value
                                    .parse::<u32>()
                                    .ok()
                                    .filter(|k| *k > 0)
                                    .ok_or_else(|| {
                                        format!("{flag} needs an exact nonzero numeric session ID")
                                    })?;
                            selected = Some(if flag == "--download" {
                                Mode::Download(key)
                            } else {
                                Mode::Interactive {
                                    session: Some(key),
                                    offline: flag == "--replay",
                                }
                            });
                        }
                    }
                }
                "--gp" | "--year" => {
                    return Err(concat!(
                        "Historical demo generation is an optional developer Python bridge; ",
                        "use --demo or --catalogue --refresh --year YYYY for native recordings"
                    )
                    .into());
                }
                _ => return Err(format!("Unknown option: {flag}; see --help")),
            }
            i += 1;
        }
        options.mode = selected.unwrap_or(Mode::Interactive {
            session: None,
            offline: false,
        });
        if offline {
            match &mut options.mode {
                Mode::Interactive { offline, .. } => *offline = true,
                _ => {
                    return Err(
                        "--offline applies to interactive replay, not downloads or demo checks"
                            .into(),
                    );
                }
            }
        }
        if matches!(options.mode, Mode::Demo)
            && (options.catalogue_path.is_some() || options.recording_root.is_some())
        {
            return Err("The bundled demo does not use recording/catalogue paths".into());
        }
        Ok(options)
    }
}
pub const HELP: &str = "Sector Line - recorded timing and generated demo
Usage: sectorline [COMMAND] [OPTIONS]
  (no command)           Open the bundled generated demo and session selector
  --demo, --headless     Verify 20 generated frames without a terminal or Python
  --catalogue --refresh --year YYYY   Fetch historical metadata explicitly
  --catalogue --year YYYY --search TEXT  Search cached metadata offline
  --download ID          Download/resume an exact historical session, then validate it
  --replay ID            Open an exact completed cache offline (terminal required)
  --prepare-session ID   Open cached timing or download it in the background
  --offline              Disable recording acquisition for the entire TUI session
  --catalogue-cache PATH Override user catalogue location
  --recording-root PATH  Override user recording storage
  --paths                Print resolved cache locations without creating them
  --ascii, --no-color    Plain terminal glyphs / text-only palette
  --version, -V          Print the binary version
  --help, -h             Show this help

Historical acquisition requires provider connectivity, not Python or a Sector Line login.
The demo is generated simulation; recorded snapshots use event-time estimates.
Live access is unavailable. --live and --gp live are disabled.
Recordings open paused: Space plays; 1/2/5/0 speeds; Home restarts.
";
