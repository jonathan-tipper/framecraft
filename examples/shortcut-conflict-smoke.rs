//! Native Carbon check with two processes and no GUI, key injection, clipboard or capture.
#[cfg(all(target_os = "macos", feature = "ui"))]
#[path = "../src/desktop/exclusive_shortcut.rs"]
mod exclusive_shortcut;

#[cfg(all(target_os = "macos", feature = "ui"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use exclusive_shortcut::ExclusiveShortcuts;
    use global_hotkey::{hotkey::HotKey, GlobalHotKeyManager};
    let key = "Control+Alt+F18".parse::<HotKey>()?;
    let capture = ExclusiveShortcuts::new(|_| {})?;
    match std::env::args().nth(1).as_deref() {
        Some("busy") => {
            let error = capture
                .register(key)
                .expect_err("two apps claimed the same shortcut");
            if !error.contains("already in use") {
                return Err(error.into());
            }
        }
        Some("free") => {
            capture.register(key)?;
            capture.release(key)?;
        }
        Some("shared") => match capture.register(key) {
            Ok(()) => {
                capture.release(key)?;
                println!("OBSERVED macOS allowed an exclusive claim over a shared registration");
            }
            Err(error) if error.contains("already in use") => {
                println!("OBSERVED macOS reported the shared registration as occupied");
            }
            Err(error) => return Err(error.into()),
        },
        None => {
            capture.register(key)?;
            check_child("busy")?;
            capture.release(key)?;
            check_child("free")?;
            println!(
                "PASS exclusive holder rejected a second process and released Control+Option+F18"
            );
            let shared = GlobalHotKeyManager::new()?;
            shared.register(key)?;
            check_child("shared")?;
            shared.unregister(key)?;
            check_child("free")?;
            println!("PASS fixture released all registrations after the shared-holder check");
        }
        _ => return Err("Run without arguments.".into()),
    }
    Ok(())
}

#[cfg(all(target_os = "macos", feature = "ui"))]
fn check_child(expect: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        process::Command,
        time::{Duration, Instant},
    };
    let mut child = Command::new(std::env::current_exe()?).arg(expect).spawn()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("Expected {expect} registration from child, got {status}").into())
            };
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err("Child registration check timed out.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(all(target_os = "macos", feature = "ui")))]
fn main() {
    eprintln!("This check requires macOS and the ui feature.");
}
