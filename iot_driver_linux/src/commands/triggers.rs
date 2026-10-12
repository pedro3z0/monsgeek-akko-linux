//! Trigger-related command handlers.

use super::CommandResult;
use iot_driver::key_action::KeyAction;
use iot_driver::keyclass::KeySelector;
use monsgeek_keyboard::{
    DksAction, DksBinding, DksConfig, DksPhase, KeyMode, KeyTriggerSettings, KeyboardInterface,
    ModeByte, TravelDepth, settings::Precision,
};
use monsgeek_transport::protocol::{KeymatrixLayer, MatrixPos};

/// Resolve key selectors to matrix positions, using the connected board's own
/// key names before the generic table.
///
/// A selector is written in the user's vocabulary — `A`, `RShift`, `alpha`,
/// `#9` — and each of those means "the key called A *on this board*". The
/// generic table is the M1 V5 TKL layout, which a 60% board disagrees with past
/// the alpha block, so the board is asked first.
fn resolve_keys(keyboard: &KeyboardInterface, keys: &[KeySelector]) -> Result<Vec<u8>, String> {
    KeySelector::resolve_with(keys, |name| {
        keyboard.matrix_key_index(name).map(MatrixPos::new)
    })
    .map(|positions| positions.into_iter().map(|p| p.get()).collect())
}

/// How a position is written back to the user: the key's name with its position
/// in brackets (`A (#9)`), so a printed number is never the only handle on which
/// key it was, and `#9` alone where the board has no name for that slot.
fn key_label(keyboard: &KeyboardInterface, key: u8) -> String {
    let name = keyboard.matrix_key_name(key as usize);
    if name.is_empty() || name == "?" {
        format!("#{key}")
    } else {
        format!("{name} (#{key})")
    }
}
use std::collections::{BTreeSet, HashSet};
use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Stop calibration with retry (keyboard can be sluggish in cal mode).
/// Sends both min and max stop commands to ensure clean exit.
fn stop_calibration(keyboard: &KeyboardInterface) {
    // Stop max calibration (the active phase)
    for attempt in 0..5 {
        match keyboard.calibrate_max(false) {
            Ok(_) => break,
            Err(e) => {
                if attempt < 4 {
                    std::thread::sleep(Duration::from_millis(100));
                } else {
                    eprintln!("Warning: failed to stop max calibration after 5 attempts: {e}");
                }
            }
        }
    }
    // Also stop min calibration (belt-and-suspenders)
    let _ = keyboard.calibrate_min(false);
    // Give firmware time to save calibration data to flash
    std::thread::sleep(Duration::from_millis(300));
}

/// Run calibration (min + max) with per-key progress display
pub fn calibrate(keyboard: &KeyboardInterface) -> CommandResult {
    let positions = keyboard.matrix_positions() as usize;

    // Determine which matrix indices have real analog (magnetic) keys.
    // Excluded: empty-name positions (gaps), non-analog positions (encoder/GPIO).
    let has_key_names = !keyboard.matrix_key_name(0).is_empty();
    let real_keys: HashSet<usize> = if has_key_names {
        (0..positions)
            .filter(|&i| {
                let name = keyboard.matrix_key_name(i);
                !name.is_empty() && name != "?" && !keyboard.is_non_analog(i)
            })
            .collect()
    } else {
        // No profile key names available — exclude non-analog but include rest
        (0..positions)
            .filter(|&i| !keyboard.is_non_analog(i))
            .collect()
    };
    let real_count = real_keys.len();

    // The process-wide Ctrl-C flag, shared with the profile guard: a handler can
    // only be installed once, and installing a second one here would leave this
    // loop polling something nothing ever sets.
    let running = super::setup_interrupt_handler();

    println!("Starting calibration for {real_count} keys ({positions} matrix positions)...");
    if !has_key_names {
        println!("  (No device profile found — key names unavailable)");
    }
    println!();
    println!("  To stop: Ctrl+C, any key, mouse click, or encoder knob.");
    println!("  Auto-stops after 10s of no progress.");
    println!();

    // Phase 1: Min calibration (released position)
    println!("Step 1: Calibrating minimum (released) position");
    println!("        Keep all keys RELEASED for 2 seconds...");

    if let Err(e) = keyboard.calibrate_min(true) {
        eprintln!("Failed to start min calibration: {e}");
        return Ok(());
    }

    // Show countdown (check for interrupt)
    for i in (1..=2).rev() {
        if !running.load(Ordering::SeqCst) {
            println!("\n\nAborted during min calibration.");
            let _ = keyboard.calibrate_min(false);
            return Ok(());
        }
        print!("\r        {} seconds remaining...", i);
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(1));
    }
    println!("\r        Done.                    ");
    let _ = keyboard.calibrate_min(false);

    // Phase 2: Max calibration with progress display
    println!();
    println!("Step 2: Calibrating maximum (pressed) position");
    println!("        Press ALL keys firmly and hold...");

    if let Err(e) = keyboard.calibrate_max(true) {
        eprintln!("Failed to start max calibration: {e}");
        return Ok(());
    }

    // Poll and display progress
    let mut finished = BTreeSet::new();
    let pages = positions.div_ceil(32);

    // Set up input monitoring: stdin + mouse clicks + encoder knob (evdev)
    let input = setup_input_monitor(keyboard.vid(), keyboard.pid());

    // Idle timeout: auto-stop if no new key calibrates in 10 seconds
    let mut last_progress_time = std::time::Instant::now();
    let mut last_finished_count = 0usize;
    let idle_timeout = Duration::from_secs(10);

    loop {
        // Check for Ctrl+C (abort without saving)
        if !running.load(Ordering::SeqCst) {
            stop_calibration(keyboard);
            println!("\n\nCalibration aborted (not saved).");
            restore_input(&input);
            return Ok(());
        }

        // Check for Enter key or any stdin input (graceful save)
        if check_input(&input) {
            stop_calibration(keyboard);
            println!(
                "\n\nPartial calibration saved ({}/{} keys).",
                finished.len(),
                real_count,
            );
            restore_input(&input);
            return Ok(());
        }

        // Poll each page for calibration progress
        for page in 0..pages as u8 {
            match keyboard.get_calibration_progress(page) {
                Ok(values) => {
                    for (i, &val) in values.iter().enumerate() {
                        let key_idx = page as usize * 32 + i;
                        if real_keys.contains(&key_idx)
                            && val >= 300
                            && !finished.contains(&key_idx)
                        {
                            finished.insert(key_idx);
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        // Track progress for idle timeout
        if finished.len() > last_finished_count {
            last_finished_count = finished.len();
            last_progress_time = std::time::Instant::now();
        }

        // Build sorted list of missing key names (only real keys)
        let mut missing: Vec<&str> = real_keys
            .iter()
            .filter(|i| !finished.contains(i))
            .map(|&i| keyboard.matrix_key_name(i))
            .filter(|s| !s.is_empty())
            .collect();
        missing.sort_unstable();

        // Clear line and print progress + missing keys (elided if many)
        let idle_secs = last_progress_time.elapsed().as_secs();
        print!(
            "\x1b[2K\r        Progress: {}/{} keys",
            finished.len(),
            real_count,
        );
        if idle_secs >= 3 && !missing.is_empty() {
            print!(" (idle {idle_secs}s/10s)");
        }
        if !missing.is_empty() {
            let max_show = 10;
            if missing.len() <= max_show {
                print!("  Remaining: {}", missing.join(", "));
            } else {
                let shown: Vec<&str> = missing.iter().copied().take(max_show).collect();
                print!(
                    "  Remaining: {}, ... (+{})",
                    shown.join(", "),
                    missing.len() - max_show
                );
            }
        }
        let _ = std::io::stdout().flush();

        // Check completion
        if finished.len() >= real_count {
            break;
        }

        // Idle timeout
        if last_progress_time.elapsed() >= idle_timeout && !finished.is_empty() {
            stop_calibration(keyboard);
            restore_input(&input);
            let uncalibrated: Vec<&str> = missing.clone();
            println!(
                "\n\nAuto-stopped: no progress for 10s. Calibrated {}/{} keys.",
                finished.len(),
                real_count,
            );
            if !uncalibrated.is_empty() {
                println!("  Uncalibrated: {}", uncalibrated.join(", "));
            }
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    stop_calibration(keyboard);
    restore_input(&input);
    println!("\n\nCalibration complete! All {real_count} keys calibrated.");
    Ok(())
}

/// Input monitor: watches stdin (keyboard + mouse clicks) and evdev (encoder knob).
#[cfg(unix)]
struct InputMonitor {
    old_termios: Option<libc::termios>,
    evdev_fds: Vec<std::os::unix::io::RawFd>,
}

#[cfg(unix)]
fn setup_input_monitor(vid: u16, pid: u16) -> InputMonitor {
    use std::os::unix::io::AsRawFd;

    let fd = std::io::stdin().as_raw_fd();
    let mut old_termios: libc::termios = unsafe { std::mem::zeroed() };

    let termios_ok = unsafe { libc::tcgetattr(fd, &mut old_termios) } == 0;

    if termios_ok {
        let mut new_termios = old_termios;
        new_termios.c_lflag &= !(libc::ICANON | libc::ECHO);
        new_termios.c_cc[libc::VMIN] = 0;
        new_termios.c_cc[libc::VTIME] = 0;
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, &new_termios);
        }
    }

    // Enable terminal mouse click tracking (X10 mode: report button presses)
    // SGR extended mode for better compatibility with modern terminals
    print!("\x1b[?1000h\x1b[?1006h");
    let _ = std::io::stdout().flush();

    // Find evdev devices matching our keyboard's VID/PID (for encoder knob)
    let evdev_fds = find_evdev_devices(vid, pid);
    if !evdev_fds.is_empty() {
        eprintln!(
            "  Monitoring {} input device(s) for encoder/knob events.",
            evdev_fds.len()
        );
    }

    InputMonitor {
        old_termios: if termios_ok { Some(old_termios) } else { None },
        evdev_fds,
    }
}

/// Scan sysfs for /dev/input/eventN devices matching VID:PID, open non-blocking.
#[cfg(unix)]
fn find_evdev_devices(vid: u16, pid: u16) -> Vec<std::os::unix::io::RawFd> {
    use std::os::unix::io::RawFd;

    let vid_hex = format!("{:04x}", vid);
    let pid_hex = format!("{:04x}", pid);
    let mut fds: Vec<RawFd> = Vec::new();

    let Ok(entries) = std::fs::read_dir("/sys/class/input") else {
        return fds;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("event") {
            continue;
        }

        let id_dir = entry.path().join("device/id");
        let vendor = std::fs::read_to_string(id_dir.join("vendor")).unwrap_or_default();
        let product = std::fs::read_to_string(id_dir.join("product")).unwrap_or_default();

        if vendor.trim() == vid_hex && product.trim() == pid_hex {
            let dev_path = format!("/dev/input/{name_str}");
            let c_path = match std::ffi::CString::new(dev_path.as_str()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
            if fd >= 0 {
                fds.push(fd);
            }
        }
    }

    fds
}

#[cfg(unix)]
fn check_input(monitor: &InputMonitor) -> bool {
    use std::os::unix::io::AsRawFd;

    if monitor.old_termios.is_none() && monitor.evdev_fds.is_empty() {
        return false;
    }

    let stdin_fd = std::io::stdin().as_raw_fd();
    let mut read_fds: libc::fd_set = unsafe { std::mem::zeroed() };
    let mut max_fd = stdin_fd;

    unsafe {
        libc::FD_ZERO(&mut read_fds);
        if monitor.old_termios.is_some() {
            libc::FD_SET(stdin_fd, &mut read_fds);
        }
        for &fd in &monitor.evdev_fds {
            libc::FD_SET(fd, &mut read_fds);
            if fd > max_fd {
                max_fd = fd;
            }
        }
    }

    let mut timeout = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };

    let result = unsafe {
        libc::select(
            max_fd + 1,
            &mut read_fds,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut timeout,
        )
    };

    if result <= 0 {
        return false;
    }

    // Check stdin (keyboard on other KB, or mouse click in terminal)
    if monitor.old_termios.is_some() && unsafe { libc::FD_ISSET(stdin_fd, &read_fds) } {
        let mut buf = [0u8; 64];
        let _ = std::io::Read::read(&mut std::io::stdin(), &mut buf);
        return true;
    }

    // Check evdev devices (encoder knob rotation or press)
    for &fd in &monitor.evdev_fds {
        if unsafe { libc::FD_ISSET(fd, &read_fds) } {
            // Drain all pending events
            let mut buf = [0u8; 256];
            while unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) } > 0 {}
            return true;
        }
    }

    false
}

#[cfg(unix)]
fn restore_input(monitor: &InputMonitor) {
    // Disable mouse tracking
    print!("\x1b[?1006l\x1b[?1000l");
    let _ = std::io::stdout().flush();

    // Restore terminal settings
    if let Some(ref old_termios) = monitor.old_termios {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, old_termios);
        }
    }

    // Close evdev fds
    for &fd in &monitor.evdev_fds {
        unsafe {
            libc::close(fd);
        }
    }
}

#[cfg(not(unix))]
struct InputMonitor;

#[cfg(not(unix))]
fn setup_input_monitor(_vid: u16, _pid: u16) -> InputMonitor {
    InputMonitor
}

#[cfg(not(unix))]
fn check_input(_monitor: &InputMonitor) -> bool {
    false
}

#[cfg(not(unix))]
fn restore_input(_monitor: &InputMonitor) {}

/// Show current trigger settings
pub fn triggers(keyboard: &KeyboardInterface) -> CommandResult {
    let version = keyboard.get_version().unwrap_or_default();
    let precision = keyboard.get_precision().unwrap_or_default();
    println!(
        "Trigger Settings (firmware {}, precision: {})",
        version.format(),
        precision.as_str()
    );
    println!();

    match keyboard.get_all_triggers() {
        Ok(triggers) => {
            let first_press = triggers.press_travel.first().copied().unwrap_or(0);
            let first_lift = triggers.lift_travel.first().copied().unwrap_or(0);
            let first_rt_press = triggers.rt_press.first().copied().unwrap_or(0);
            let first_rt_lift = triggers.rt_lift.first().copied().unwrap_or(0);
            let first_mode = triggers.key_modes.first().copied().unwrap_or(0);

            let num_keys = triggers.key_modes.len().min(triggers.press_travel.len());

            // Fixed point at the device's own resolution: a 1-decimal render
            // showed raw 204 as "2.0mm", hiding the 0.04mm it actually holds.
            let mm = |raw: u16| TravelDepth::from_raw(raw).format(precision);
            println!("First key settings (as sample):");
            println!(
                "  Actuation:     {} (raw: {})",
                mm(first_press),
                first_press
            );
            println!("  Release:       {} (raw: {})", mm(first_lift), first_lift);
            println!(
                "  RT Press:      {} (raw: {})",
                mm(first_rt_press),
                first_rt_press
            );
            println!(
                "  RT Release:    {} (raw: {})",
                mm(first_rt_lift),
                first_rt_lift
            );
            println!(
                "  Mode:          {} ({})",
                first_mode,
                ModeByte::from_u8(first_mode)
            );
            println!();

            let all_same_press = triggers
                .press_travel
                .iter()
                .take(num_keys)
                .all(|&v| v == first_press);
            let all_same_mode = triggers
                .key_modes
                .iter()
                .take(num_keys)
                .all(|&v| v == first_mode);

            if all_same_press && all_same_mode {
                println!("All {num_keys} matrix positions have identical settings");
            } else {
                println!("Settings vary across the {num_keys} matrix positions");
                println!("\nFirst 10 key values:");
                for i in 0..10.min(num_keys) {
                    let press = triggers.press_travel.get(i).copied().unwrap_or(0);
                    let mode = triggers.key_modes.get(i).copied().unwrap_or(0);
                    // Name the key, not just its slot: a position on its own
                    // says nothing about which key it is on the board in front
                    // of the user, and that is the number they want to act on.
                    let label = key_label(keyboard, i as u8);
                    println!("  {label}: {} mode={}", mm(press), mode);
                }
            }
        }
        Err(e) => eprintln!("Failed to read trigger settings: {e}"),
    }
    Ok(())
}

/// Set actuation point for all keys
pub fn set_actuation(keyboard: &KeyboardInterface, mm: f32) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    // `from_mm` rounds; `(mm * factor) as u16` truncated, so 1.05mm stored as 104.
    let travel = TravelDepth::from_mm(mm, precision);
    match keyboard.set_actuation_all(travel) {
        Ok(_) => println!(
            "Actuation point set to {mm:.2}mm (raw: {}) for all keys",
            travel.raw()
        ),
        Err(e) => eprintln!("Failed to set actuation point: {e}"),
    }
    Ok(())
}

/// Raw (firmware-unit) values that make a board behave as standard, i.e.
/// non-continuous, Rapid Trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StandardRtPlan {
    actuation: u16,
    release: u16,
    sensitivity: u16,
}

/// Compute the travel values a standard-RT mapping needs.
///
/// RT itself is only the orthogonal `0x80` flag on the mode byte; what makes it
/// *standard* is the relationship between the travel fields. The release point
/// has to sit exactly one sensitivity below the actuation point, so the key
/// fully un-arms and cannot re-fire while still held above it. The vendor
/// presets leave a much lower release point instead (1.50/0.50 with a 0.40
/// sensitivity on the FUN60 Pro), which is the continuous variant: the key
/// keeps firing inside that band.
///
/// Values come back in firmware units at the device's precision, and the
/// release point is derived from the *rounded* values, so what gets printed and
/// what lands on the board are the same number. Inputs that would produce a
/// degenerate table -- a release point of zero, or a sensitivity that rounds
/// away at this precision -- are rejected instead of being written.
fn standard_rt_plan(
    actuation_mm: f32,
    sensitivity_mm: f32,
    release_mm: Option<f32>,
    precision: Precision,
) -> Result<StandardRtPlan, String> {
    if !actuation_mm.is_finite() || actuation_mm <= 0.0 {
        return Err(format!("actuation must be above 0mm (got {actuation_mm})"));
    }
    if !sensitivity_mm.is_finite() || sensitivity_mm <= 0.0 {
        return Err(format!(
            "RT sensitivity must be above 0mm (got {sensitivity_mm})"
        ));
    }
    let rel_mm = release_mm.unwrap_or(actuation_mm);
    if !rel_mm.is_finite() || rel_mm <= 0.0 {
        return Err(format!("release must be above 0mm (got {rel_mm})"));
    }

    let actuation = TravelDepth::from_mm(actuation_mm, precision).raw();
    let sensitivity = TravelDepth::from_mm(sensitivity_mm, precision).raw();
    let release = TravelDepth::from_mm(rel_mm, precision).raw();

    if sensitivity == 0 {
        return Err(format!(
            "{sensitivity_mm:.3}mm rounds to 0 units at {} precision",
            precision.as_str()
        ));
    }

    Ok(StandardRtPlan {
        actuation,
        release,
        sensitivity,
    })
}

/// Enable/disable Rapid Trigger, set its sensitivity, or write a standard-RT
/// mapping for every key.
pub fn set_rt(
    keyboard: &KeyboardInterface,
    value: &str,
    actuation: Option<f32>,
    sensitivity: Option<f32>,
    release: Option<f32>,
) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();

    if value.eq_ignore_ascii_case("standard") {
        let (Some(actuation_mm), Some(sensitivity_mm)) = (actuation, sensitivity) else {
            return Err("standard RT needs at least actuation and sensitivity: set-rt standard \
                        <actuation-mm> <sensitivity-mm> [release-mm] (e.g. set-rt standard 0.8 0.2)"
                .into());
        };
        let plan = standard_rt_plan(actuation_mm, sensitivity_mm, release, precision)?;
        let act = TravelDepth::from_raw(plan.actuation);
        let rel = TravelDepth::from_raw(plan.release);
        let sens = TravelDepth::from_raw(plan.sensitivity);

        // On boards like FUN60 Pro (dev_id 2304) that drop paged writes, apply per-key:
        let dev_id = keyboard.get_device_id().unwrap_or(0);
        if dev_id == 2304 {
            let all = keyboard
                .get_all_triggers()
                .map_err(|e| format!("reading trigger tables: {e}"))?;
            let total = all
                .press_travel
                .len()
                .max(all.lift_travel.len())
                .max(all.rt_press.len())
                .max(all.rt_lift.len())
                .max(all.key_modes.len());
            for idx in 0..total {
                let cur_mode = ModeByte::from_u8(all.key_modes.get(idx).copied().unwrap_or(0));
                let settings = KeyTriggerSettings {
                    key_index: idx as u8,
                    actuation: act.raw(),
                    deactuation: rel.raw(),
                    rt_press: sens.raw(),
                    rt_lift: sens.raw(),
                    mode: cur_mode.base,
                    rapid_trigger: true,
                };
                keyboard.set_key_trigger(&settings)?;
            }
        } else {
            keyboard.set_actuation_all(act)?;
            keyboard.set_release_all(rel)?;
            keyboard.set_rt_press_all(sens)?;
            keyboard.set_rt_lift_all(sens)?;
            keyboard.set_rapid_trigger_all(true)?;
        }

        println!(
            "Standard RT applied to all keys (firmware precision: {})",
            precision.as_str()
        );
        println!("  Actuation:   {}", act.format(precision));
        println!("  Sensitivity: {}", sens.format(precision));
        println!("  Release:     {}", rel.format(precision));
        println!("  Rapid Trigger: on  (0x80 flag set, base modes preserved)");
        println!();
        println!(
            "The key now disarms whenever switch travel rises above {}.",
            act.format(precision)
        );
        return Ok(());
    }

    if actuation.is_some() || sensitivity.is_some() {
        // `set-rt on 0.3` used to appear in the docs but never parsed (the value
        // was a single argument); point at the form that does.
        let hint = match value.to_lowercase().as_str() {
            "on" | "enable" | "off" | "0" | "disable" => {
                " — did you mean `set-rt <sensitivity-mm>`? 'on' already means 0.3mm"
            }
            _ => " — only 'standard' takes these two numbers",
        };
        return Err(
            format!("actuation/sensitivity are only valid with 'set-rt standard'{hint}").into(),
        );
    }

    match value.to_lowercase().as_str() {
        "off" | "0" | "disable" => match keyboard.set_rapid_trigger_all(false) {
            Ok(_) => println!("Rapid Trigger disabled for all keys"),
            Err(e) => eprintln!("Failed to disable Rapid Trigger: {e}"),
        },
        "on" | "enable" => {
            let sensitivity = TravelDepth::from_mm(0.3, precision);
            let _ = keyboard.set_rapid_trigger_all(true);
            let _ = keyboard.set_rt_press_all(sensitivity);
            let _ = keyboard.set_rt_lift_all(sensitivity);
            println!("Rapid Trigger enabled with 0.3mm sensitivity for all keys");
        }
        _ => {
            let mm: f32 = value.parse().unwrap_or(0.3);
            let sensitivity = TravelDepth::from_mm(mm, precision);
            let _ = keyboard.set_rapid_trigger_all(true);
            let _ = keyboard.set_rt_press_all(sensitivity);
            let _ = keyboard.set_rt_lift_all(sensitivity);
            println!("Rapid Trigger enabled with {mm:.2}mm sensitivity for all keys");
        }
    }
    Ok(())
}

/// Get or set the Rapid Trigger flavour on a ry5088-flasher custom-firmware board.
///
/// Both Rapid-Trigger flavours, on **either** firmware.
///
/// Custom (ry5088-flasher) firmware has a real engine switch, so we use it: the
/// `SET_RTMODE`/`GET_RTMODE` extension (0x1F/0x9F). Stock firmware always runs
/// the continuous engine and has no such byte — there the flavour is expressed
/// through the travel table, the only lever stock gives us:
///
/// * **traditional** — release point one sensitivity *below* actuation
///   (`lift = press - rt`), so the key fully un-arms before it can re-fire.
/// * **continuous** — the vendor preset shape: a *low* release point, so the key
///   keeps re-firing inside the `lift .. press` band (CRT).
///
/// These two stock mappings are the ones already verified on this board and
/// captured in `tests/fixtures/fun60pro/`.
const RT_TRADITIONAL_ACTUATION_MM: f32 = 1.5;
const RT_TRADITIONAL_SENSITIVITY_MM: f32 = 0.4;
/// Vendor-preset release point for the continuous variant (raw 50 = 0.50 mm).
const RT_CONTINUOUS_LIFT_MM: f32 = 0.5;

/// Builds that answer the RT-flavour extension report v400 or newer; stock
/// FUN60 Pro firmware is v309.
const RT_FLAVOUR_MIN_VERSION: u16 = 0x0400;

fn is_flavour_capable(keyboard: &KeyboardInterface) -> bool {
    if keyboard
        .get_version()
        .map(|v| v.raw >= RT_FLAVOUR_MIN_VERSION)
        .unwrap_or(false)
    {
        return true;
    }
    // Patched firmware with standard_rt capability implements 0x1F/0x9F engine switch
    if let Ok(Some(patch)) = keyboard.get_patch_info() {
        if patch.capabilities & (1 << 7) != 0 {
            return true;
        }
    }
    false
}

/// Turn the current travel tables into a flavour — for stock firmware, where
/// there is no flavour byte to read. Traditional puts the release point exactly
/// one sensitivity under actuation; continuous leaves it far below.
fn stock_flavour(keyboard: &KeyboardInterface) -> Result<bool, String> {
    let t = keyboard
        .get_all_triggers()
        .map_err(|e| format!("reading trigger tables: {e}"))?;
    let press = t.press_travel.first().copied().unwrap_or(0) as u32;
    let lift = t.lift_travel.first().copied().unwrap_or(0) as u32;
    let sens = t.rt_press.first().copied().unwrap_or(0) as u32;
    if sens == 0 || press == 0 {
        return Err("the trigger table is all zeros — set a mapping first \
                    (`set-rt standard <act> <sens>`) or pass an explicit flavour"
            .into());
    }
    Ok(lift < press.saturating_sub(sens))
}

/// Write the stock-firmware equivalent of a flavour (travel tables + RT flag).
///
/// The all-keys paged SET form (`set_magnetism_u16`) is not honoured by this
/// firmware — the board acks it but keeps serving the old table. The vendor
/// web app never writes whole tables either; it writes each key through the
/// per-key "simple" form (`set_key_trigger`), which does land. So apply the
/// mapping key by key, preserving each key's base mode and arming RT as we go.
fn apply_stock_flavour(keyboard: &KeyboardInterface, continuous: bool) -> Result<(), String> {
    let precision = keyboard.get_precision().unwrap_or_default();
    let act = TravelDepth::from_mm(RT_TRADITIONAL_ACTUATION_MM, precision);
    let sens = TravelDepth::from_mm(RT_TRADITIONAL_SENSITIVITY_MM, precision);
    let plan = standard_rt_plan(
        RT_TRADITIONAL_ACTUATION_MM,
        RT_TRADITIONAL_SENSITIVITY_MM,
        None,
        precision,
    )?;
    let lift = if continuous {
        TravelDepth::from_mm(RT_CONTINUOUS_LIFT_MM, precision)
    } else {
        TravelDepth::from_raw(plan.release)
    };

    // Per-key "simple" writes (the vendor-webapp form), not the all-keys paged
    // SET the board silently drops. Only keys that differ are touched: 5
    // simple-writes per key at ~30-250ms each, so a full rewrite is slow.
    // `key_count` (82 here) only covers the populated slots; the tail of the
    // 84-position matrix keeps its own values, so iterate the whole decoded
    // table instead of stopping at key_count.
    let all = keyboard
        .get_all_triggers()
        .map_err(|e| format!("reading trigger tables: {e}"))?;
    let total = all
        .press_travel
        .len()
        .max(all.lift_travel.len())
        .max(all.rt_press.len())
        .max(all.rt_lift.len())
        .max(all.key_modes.len());
    let mut changed = 0u32;
    for idx in 0..total {
        let cur_mode = ModeByte::from_u8(all.key_modes.get(idx).copied().unwrap_or(0));
        let same = all.press_travel.get(idx).copied().unwrap_or(u16::MAX) == act.raw()
            && all.lift_travel.get(idx).copied().unwrap_or(u16::MAX) == lift.raw()
            && all.rt_press.get(idx).copied().unwrap_or(u16::MAX) == sens.raw()
            && all.rt_lift.get(idx).copied().unwrap_or(u16::MAX) == sens.raw()
            && cur_mode.rapid_trigger;
        if same {
            continue;
        }
        let settings = KeyTriggerSettings {
            key_index: idx as u8,
            actuation: act.raw(),
            deactuation: lift.raw(),
            rt_press: sens.raw(),
            rt_lift: sens.raw(),
            mode: cur_mode.base,
            rapid_trigger: true,
        };
        keyboard
            .set_key_trigger(&settings)
            .map_err(|e| format!("key {idx}: {e}"))?;
        changed += 1;
    }

    println!(
        "  Actuation:   {}   RT sensitivity: {}   Release: {}   ({} of {} table slots rewritten)",
        act.format(precision),
        sens.format(precision),
        lift.format(precision),
        changed,
        total,
    );
    Ok(())
}
pub fn rt_flavour(keyboard: &KeyboardInterface, mode: Option<&str>) -> CommandResult {
    let native = is_flavour_capable(keyboard);

    match mode {
        None => {
            if native {
                match keyboard.get_rt_flavour() {
                    Ok(1) => println!("Rapid Trigger flavour: continuous (CRT)  [engine switch]"),
                    Ok(_) => println!("Rapid Trigger flavour: traditional      [engine switch]"),
                    Err(e) => return Err(format!("Failed to read RT flavour: {e}").into()),
                }
            } else {
                match stock_flavour(keyboard) {
                    Ok(true) => println!(
                        "Rapid Trigger flavour: continuous (CRT)  [inferred: release sits far \
                         below actuation]"
                    ),
                    Ok(false) => println!(
                        "Rapid Trigger flavour: traditional      [inferred: release = \
                         actuation - sensitivity]"
                    ),
                    Err(e) => return Err(e.into()),
                }
                println!(
                    "  note: stock firmware has no flavour switch, so this is read from the \
                     travel tables. `rt-flavour traditional|continuous` rewrites them."
                );
            }
        }
        Some(m) => {
            let continuous = match m.to_lowercase().as_str() {
                "continuous" | "crt" | "cont" | "1" | "stock" => true,
                "traditional" | "trad" | "classic" | "normal" | "0" => false,
                other => {
                    return Err(format!(
                        "unknown RT flavour {other:?} — use 'traditional' or 'continuous'"
                    )
                    .into());
                }
            };

            if native {
                keyboard
                    .set_rt_flavour(continuous)
                    .map_err(|e| format!("Failed to set RT flavour: {e}"))?;
                let now = keyboard.get_rt_flavour().unwrap_or(u8::MAX);
                println!(
                    "Rapid Trigger flavour set to {} (engine switch)",
                    if continuous { "continuous (CRT)" } else { "traditional" }
                );
                if now != u8::from(continuous) {
                    println!("  warning: read back {now}, expected {}", u8::from(continuous));
                }
            } else {
                println!(
                    "Stock firmware: no flavour switch — writing the equivalent travel table \
                     for {}",
                    if continuous { "continuous (CRT)" } else { "traditional" }
                );
                apply_stock_flavour(keyboard, continuous)?;
                // This firmware serves the previous table for up to ~2 s after a
                // commit (docs/FUN60_PRO.md §7), so settle before verifying.
                std::thread::sleep(std::time::Duration::from_millis(2100));
                match stock_flavour(keyboard) {
                    Ok(true) => println!("  verified: continuous (CRT)"),
                    Ok(false) => println!("  verified: traditional"),
                    Err(e) => println!("  could not verify: {e}"),
                }
                println!(
                    "\n  note: unpatched stock firmware uses continuous RT. For true standard RT, \
                     flash the binary patch (firmwares/2304-v309/patch/)."
                );
            }
        }
    }
    Ok(())
}

/// Query or configure competitive Turbo Mode (pure on/off).
///
/// Turbo and priority scan are orthogonal and compose freely:
/// - `off` (0): Stock matrix scanning rate with RGB animations and normal housekeeping.
/// - `on` (1): Zero-jitter tight scan loop; main loop background housekeeping tick is suppressed.
///
/// Priority clusters (`fps`/`moba`/custom) are controlled separately via
/// [`priority`]; turbo does not imply any scan cluster.
pub fn turbo(keyboard: &KeyboardInterface, mode: Option<&str>) -> CommandResult {
    let patch = keyboard.get_patch_info().ok().flatten();
    let has_turbo = patch.as_ref().map(|p| p.has_turbo()).unwrap_or(false);

    match mode {
        None => {
            if !has_turbo {
                println!(
                    "Turbo mode: not supported (requires custom firmware patch with CAP_TURBO)"
                );
                return Ok(());
            }
            match keyboard.get_turbo_mode() {
                Ok(0) => println!("Turbo mode: off (normal scan rate + full RGB)"),
                Ok(1) => println!("Turbo mode: on (zero-jitter tight loop, tick suppressed)"),
                Ok(other) => println!("Turbo mode: unknown ({other})"),
                Err(e) => return Err(format!("Failed to read turbo mode: {e}").into()),
            }
        }
        Some(m) => {
            if !has_turbo {
                return Err(
                    "Turbo mode is not supported on this firmware (requires CAP_TURBO patch)".into(),
                );
            }
            let val = match m.to_lowercase().as_str() {
                "0" | "off" | "disable" | "disabled" | "normal" => 0,
                "1" | "on" | "turbo" | "enable" | "enabled" => 1,
                other => {
                    return Err(format!(
                        "unknown turbo mode {other:?} — use 'off' (0) or 'on' (1); priority clusters live under the 'priority' command"
                    )
                    .into());
                }
            };
            // Build LED off/on params using raw option=0x00.
            // DAZZLE_OFF=0x08 (sent by set_led/set_led_params when dazzle=false) triggers
            // a cyan palette fallback on this firmware when brightness=0.
            // option=0x00 cuts all PWM output cleanly.

            if val != 0 {
                // Blank before enabling turbo so hardware latches black before tick suppression.
                let _ = keyboard.set_led_raw_option(1, 0, 0, 0, 0, 0, 0x00);
                // Allow the firmware main loop one full frame cycle (~2-5ms minimum, 60ms safe)
                // to push the black frame through the DMA / WS2812 shift register chain
                // before tick suppression halts the LED pipeline.
                std::thread::sleep(std::time::Duration::from_millis(60));
            }

            keyboard
                .set_turbo_mode(val)
                .map_err(|e| format!("Failed to set turbo mode: {e}"))?;

            if val == 0 {
                // Give main loop tick a moment to resume before commanding LED restore
                std::thread::sleep(std::time::Duration::from_millis(30));
                // Restore standard white lighting once turbo is off
                let _ = keyboard.set_led_raw_option(1, 4, 2, 255, 255, 255, 0x00);
            }

            let now = keyboard.get_turbo_mode().unwrap_or(u8::MAX);
            println!(
                "Turbo mode set to {}",
                match val {
                    0 => "off (normal scan rate + full RGB)",
                    1 => "on (zero-jitter tight loop)",
                    _ => "unknown",
                }
            );
            if now != val {
                println!("  warning: read back {now}, expected {val}");
            }
        }
    }
    Ok(())
}

/// Query or configure Priority Scan preset.
///
/// Priority scan interleaves 3 high-rate passes on the selected key cluster for every
/// 1 full-matrix pass (~38 kHz effective scan on those keys, ~9.5 kHz for the rest).
///
/// Presets:
/// - `off` (0): full matrix every pass (stock behaviour)
/// - `fps` / `wasd` (1): FPS/WASD cluster (LShift, LCtrl, Q A W S E D R F Space)
/// - `moba` (2): MOBA cluster (1-4, Q W E R D F B Space)
pub fn priority(keyboard: &KeyboardInterface, preset: Option<&str>) -> CommandResult {
    let patch = keyboard.get_patch_info().ok().flatten();
    let has_priority = patch.as_ref().map(|p| p.has_priority_scan()).unwrap_or(false);

    match preset {
        None => {
            if !has_priority {
                println!(
                    "Priority scan: not supported (requires custom firmware patch with CAP_PRIORITY_SCAN)"
                );
                return Ok(());
            }
            match keyboard.get_priority_mask() {
                Ok((0, _)) => println!("Priority scan: off (full matrix every pass)"),
                Ok((1, _)) => println!("Priority scan: fps — FPS/WASD cluster (LShift, LCtrl, Q A W S E D R F Space)"),
                Ok((2, _)) => println!("Priority scan: moba — MOBA cluster (1-4, Q W E R D F B Space)"),
                Ok((0xFF, mask)) => println!("Priority scan: custom (mask: 0x{mask:016X})"),
                Ok((other, _)) => println!("Priority scan: unknown preset ({other})"),
                Err(e) => return Err(format!("Failed to read priority mask: {e}").into()),
            }
        }
        Some(p) => {
            if !has_priority {
                return Err(
                    "Priority scan is not supported on this firmware (requires CAP_PRIORITY_SCAN patch)"
                        .into(),
                );
            }
            let (mode, label) = match p.to_lowercase().as_str() {
                "0" | "off" | "disable" | "disabled" | "none" => (0u8, "off (full matrix)"),
                "1" | "fps" | "wasd" | "gaming" => (1u8, "fps — FPS/WASD cluster"),
                "2" | "moba" => (2u8, "moba — MOBA cluster"),
                other => {
                    return Err(format!(
                        "unknown priority preset {other:?} — use 'off', 'fps' (or 'wasd'), or 'moba'"
                    )
                    .into());
                }
            };
            keyboard
                .set_priority_mask(mode, 0)
                .map_err(|e| format!("Failed to set priority mask: {e}"))?;

            let now = keyboard.get_priority_mask().map(|(m, _)| m).unwrap_or(u8::MAX);
            println!("Priority scan set to {label}");
            if now != mode {
                println!("  warning: read back {now}, expected {mode}");
            }
        }
    }
    Ok(())
}

/// Set release point for all keys
pub fn set_release(keyboard: &KeyboardInterface, mm: f32) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    // `from_mm` rounds; `(mm * factor) as u16` truncated, so 1.05mm stored as 104.
    let travel = TravelDepth::from_mm(mm, precision);
    match keyboard.set_release_all(travel) {
        Ok(_) => println!(
            "Release point set to {mm:.2}mm (raw: {}) for all keys",
            travel.raw()
        ),
        Err(e) => eprintln!("Failed to set release point: {e}"),
    }
    Ok(())
}

/// Set bottom deadzone for all keys
pub fn set_bottom_deadzone(keyboard: &KeyboardInterface, mm: f32) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    // `from_mm` rounds; `(mm * factor) as u16` truncated, so 1.05mm stored as 104.
    let travel = TravelDepth::from_mm(mm, precision);
    match keyboard.set_bottom_deadzone_all(travel) {
        Ok(_) => println!(
            "Bottom deadzone set to {mm:.2}mm (raw: {}) for all keys",
            travel.raw()
        ),
        Err(e) => eprintln!("Failed to set bottom deadzone: {e}"),
    }
    Ok(())
}

/// Set top deadzone for all keys
pub fn set_top_deadzone(keyboard: &KeyboardInterface, mm: f32) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();
    // `from_mm` rounds; `(mm * factor) as u16` truncated, so 1.05mm stored as 104.
    let travel = TravelDepth::from_mm(mm, precision);
    match keyboard.set_top_deadzone_all(travel) {
        Ok(_) => println!(
            "Top deadzone set to {mm:.2}mm (raw: {}) for all keys",
            travel.raw()
        ),
        Err(e) => eprintln!("Failed to set top deadzone: {e}"),
    }
    Ok(())
}

/// Set trigger settings for one or more keys
///
/// One optional argument per field the command can change, so the list grows
/// with the table rather than needing a struct. `standard` is the exception: it
/// derives three of those fields at once, so it is planned once up front and
/// applied to every selected key.
#[allow(clippy::too_many_arguments)]
pub fn set_key_trigger(
    keyboard: &KeyboardInterface,
    keys: &[KeySelector],
    actuation: Option<f32>,
    release: Option<f32>,
    rt_press: Option<f32>,
    rt_lift: Option<f32>,
    mode: Option<KeyMode>,
    rt: Option<bool>,
    standard: Option<(f32, f32)>,
) -> CommandResult {
    // Resolve the key list first: a selector matching nothing should be reported
    // before a precision query or a write, so a typo cannot touch half a board.
    let resolved = resolve_keys(keyboard, keys)?;

    // `--standard` is planned once, so every key gets the same raw values and
    // the validation runs before the first write rather than per key.
    let plan = match standard {
        Some((actuation_mm, sensitivity_mm)) => {
            let precision = keyboard.get_precision().unwrap_or_default();
            Some(standard_rt_plan(actuation_mm, sensitivity_mm, None, precision)?)
        }
        None => None,
    };

    for key in resolved {
        set_one_key_trigger(
            keyboard, key, actuation, release, rt_press, rt_lift, mode, rt, plan,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn set_one_key_trigger(
    keyboard: &KeyboardInterface,
    key: u8,
    actuation: Option<f32>,
    release: Option<f32>,
    rt_press: Option<f32>,
    rt_lift: Option<f32>,
    mode: Option<KeyMode>,
    rt: Option<bool>,
    plan: Option<StandardRtPlan>,
) -> CommandResult {
    // Get current settings first
    let current = match keyboard.get_key_trigger(key) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "Failed to get current settings for {}: {e}",
                key_label(keyboard, key)
            );
            return Ok(());
        }
    };

    let precision = keyboard.get_precision().unwrap_or_default();
    // Per-key config uses the same u16 precision as the bulk table (0.01mm here).

    // A standard-RT plan supplies all four travel values and turns RT on, so it
    // takes precedence over the individual fields (clap already rejects the two
    // being given together). `mode` is left alone either way: standard RT is a
    // travel relationship, not a base mode, and a key set to SnapTap or DKS
    // should keep it.
    let (actuation, deactuation, press, lift, rapid_trigger) = match plan {
        Some(p) => (
            Some(p.actuation),
            Some(p.release),
            Some(p.sensitivity),
            Some(p.sensitivity),
            true,
        ),
        None => (
            actuation.map(|mm| TravelDepth::from_mm(mm, precision).raw()),
            release.map(|mm| TravelDepth::from_mm(mm, precision).raw()),
            rt_press.map(|mm| TravelDepth::from_mm(mm, precision).raw()),
            rt_lift.map(|mm| TravelDepth::from_mm(mm, precision).raw()),
            rt.unwrap_or(current.rapid_trigger),
        ),
    };

    let settings = KeyTriggerSettings {
        key_index: key,
        actuation: actuation.unwrap_or(current.actuation),
        deactuation: deactuation.unwrap_or(current.deactuation),
        rt_press: press.unwrap_or(current.rt_press),
        rt_lift: lift.unwrap_or(current.rt_lift),
        mode: mode.unwrap_or(current.mode),
        rapid_trigger,
    };

    match keyboard.set_key_trigger(&settings) {
        Ok(_) => {
            println!("{} trigger settings updated:", key_label(keyboard, key));
            println!(
                "  Actuation: {}, Release: {}, Mode: {}  (precision: {})",
                TravelDepth::from_raw(settings.actuation).format(precision),
                TravelDepth::from_raw(settings.deactuation).format(precision),
                ModeByte::new(settings.mode, settings.rapid_trigger),
                precision.as_str(),
            );
            println!(
                "  RT press: {}, RT lift: {}",
                TravelDepth::from_raw(settings.rt_press).format(precision),
                TravelDepth::from_raw(settings.rt_lift).format(precision),
            );
            // Spell out the derived release point: under --standard the release
            // value is not something the user typed, and it is the whole point of
            // the mapping, so it should be visible per key.
            if plan.is_some() {
                println!(
                    "  standard RT: must return below {:.2}mm before re-firing",
                    TravelDepth::from_raw(settings.deactuation).to_mm(precision)
                );
            }
        }
        Err(e) => eprintln!(
            "Failed to set trigger for {}: {e}",
            key_label(keyboard, key)
        ),
    }
    Ok(())
}

/// Set the base mode (and optionally the RT flag) for all keys at once.
pub fn set_mode_all(keyboard: &KeyboardInterface, mode: KeyMode, rt: bool) -> CommandResult {
    let mode_byte = ModeByte::new(mode, rt);
    match keyboard.set_mode_all(mode_byte) {
        Ok(_) => println!("Set all keys to {mode_byte}"),
        Err(e) => eprintln!("Failed to set mode for all keys: {e}"),
    }
    Ok(())
}

/// Bind, clear, or show a key's Snap-Tap (SOCD) pairing.
pub fn set_snaptap(
    keyboard: &KeyboardInterface,
    keys: &[KeySelector],
    with: Option<&KeySelector>,
    clear: bool,
) -> CommandResult {
    // The partner has to be one key: a Snap-Tap pair is between two specific
    // keys, so a selector matching several is a mistake worth reporting.
    let partner = match with {
        Some(sel) => match resolve_keys(keyboard, std::slice::from_ref(sel))?.as_slice() {
            [only] => Some(*only),
            many => {
                return Err(format!(
                    "--with must name exactly one key, but it matched {}",
                    many.len()
                )
                .into());
            }
        },
        None => None,
    };

    for key in resolve_keys(keyboard, keys)? {
        if clear {
            match keyboard.clear_snaptap(key) {
                Ok(_) => println!("Cleared Snap-Tap binding for {}", key_label(keyboard, key)),
                Err(e) => eprintln!("Failed to clear Snap-Tap binding: {e}"),
            }
        } else if let Some(partner) = partner {
            match keyboard.set_snaptap_pair(key, partner) {
                Ok(_) => println!(
                    "Bound {} <-> {} as a Snap-Tap pair",
                    key_label(keyboard, key),
                    key_label(keyboard, partner)
                ),
                Err(e) => eprintln!("Failed to set Snap-Tap pair: {e}"),
            }
        } else {
            match keyboard.get_snaptap_binds() {
                Ok(binds) => {
                    let bound = binds
                        .get(key as usize)
                        .copied()
                        .unwrap_or(monsgeek_keyboard::SNAPTAP_UNBOUND);
                    if bound == monsgeek_keyboard::SNAPTAP_UNBOUND {
                        println!("{}: no Snap-Tap binding", key_label(keyboard, key));
                    } else {
                        println!(
                            "{} is bound to {} (Snap-Tap)",
                            key_label(keyboard, key),
                            key_label(keyboard, bound)
                        );
                    }
                }
                Err(e) => eprintln!("Failed to read Snap-Tap bindings: {e}"),
            }
        }
    }
    Ok(())
}

/// Set a key's Mod-Tap tap-vs-hold decision time (ms, quantized to 10 ms).
pub fn set_modtap_time(
    keyboard: &KeyboardInterface,
    keys: &[KeySelector],
    ms: u16,
) -> CommandResult {
    for key in resolve_keys(keyboard, keys)? {
        match keyboard.set_modtap_time(key, ms) {
            Ok(_) => println!(
                "{} Mod-Tap decision time set to {}ms",
                key_label(keyboard, key),
                ms / 10 * 10
            ),
            Err(e) => eprintln!("Failed to set Mod-Tap time: {e}"),
        }
    }
    Ok(())
}

/// Parse one DKS output slot. A slot is an ordinary keymatrix entry — the firmware
/// dispatches it through the same `keycode_dispatch` as a normal keypress — so it
/// accepts any action, e.g. `"Ctrl+C"`, `"Macro(0)"` or a media key. Comma-separated
/// keys are also accepted as a chord shorthand.
fn parse_dks_slot(spec: &str) -> Result<[u8; 4], String> {
    let spec = spec.trim();
    let action: KeyAction = if spec.contains(',') {
        spec.replace(',', "+")
            .parse()
            .map_err(|e| format!("slot '{spec}': {e}"))?
    } else {
        spec.parse().map_err(|e| format!("slot '{spec}': {e}"))?
    };
    Ok(action.to_config_bytes())
}

fn parse_dks_actions(spec: &str) -> Result<[DksAction; 4], String> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(format!(
            "expected 4 comma-separated actions, got {}",
            parts.len()
        ));
    }
    let mut out = [DksAction::None; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = match *p {
            "0" | "none" => DksAction::None,
            "1" | "single" => DksAction::SingleTrigger,
            "2" | "until_next" => DksAction::ContinuousUntilNext,
            "3" | "across" => DksAction::ContinuousAcross,
            other => return Err(format!("unknown DKS action '{other}'")),
        };
    }
    Ok(out)
}

/// Show or configure DKS (Dynamic Keystroke) for a key.
/// Show or configure DKS (Dynamic Keystroke) for one or more keys.
pub fn dks(
    keyboard: &KeyboardInterface,
    keys: &[KeySelector],
    travel_mm: Option<f32>,
    modes: Option<String>,
    slots: Option<String>,
    rt: Option<bool>,
) -> CommandResult {
    // The spec strings are parsed per key, so the owned inputs are cloned into
    // the loop rather than consumed by the first key.
    for key in resolve_keys(keyboard, keys)? {
        dks_one_key(
            keyboard,
            key,
            travel_mm,
            modes.as_deref(),
            slots.as_deref(),
            rt,
        )?;
    }
    Ok(())
}

fn dks_one_key(
    keyboard: &KeyboardInterface,
    key: u8,
    travel_mm: Option<f32>,
    modes: Option<&str>,
    slots: Option<&str>,
    rt: Option<bool>,
) -> CommandResult {
    let setting = travel_mm.is_some() || modes.is_some() || slots.is_some();
    if !setting {
        return show_dks(keyboard, key);
    }

    let mut config = keyboard.get_dks_config(key).unwrap_or(DksConfig {
        trigger_point_travel_raw: 0,
        bindings: [DksBinding::default(); 4],
    });

    if let Some(mm) = travel_mm {
        let precision = keyboard.get_precision().unwrap_or_default();
        config.trigger_point_travel_raw = precision.mm_to_raw(mm as f64);
    }

    if let Some(spec) = modes {
        let bytes: Result<Vec<u8>, String> = spec
            .split(',')
            .map(str::trim)
            .map(|s| {
                u8::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16)
                    .map_err(|_| format!("invalid mode byte '{s}'"))
            })
            .collect();
        let bytes = bytes?;
        if bytes.len() != 4 {
            return Err(format!("--modes requires 4 bytes, got {}", bytes.len()).into());
        }
        for (i, &b) in bytes.iter().enumerate() {
            config.bindings[i] = DksBinding::from_packed_mode(b, config.bindings[i].config);
        }
    }

    if let Some(spec) = slots {
        let binding_specs: Vec<&str> = spec.split(';').collect();
        if binding_specs.len() != 4 {
            return Err(format!(
                "--slots requires 4 semicolon-separated binding specs, got {}",
                binding_specs.len()
            )
            .into());
        }
        for (i, binding_spec) in binding_specs.iter().enumerate() {
            if binding_spec.is_empty() {
                // An empty spec clears the whole slot. Leaving the phase roles behind
                // would keep a packed mode byte for a slot that emits nothing.
                config.bindings[i] = DksBinding::default();
                continue;
            }
            let parts: Vec<&str> = binding_spec.split(':').collect();
            config.bindings[i].config = parse_dks_slot(parts[0])?;
            if parts.len() > 1 {
                config.bindings[i].phase_actions = parse_dks_actions(parts[1])?;
            }
        }
    }

    match keyboard.set_dks_config(key, &config, rt) {
        Ok(_) => {
            println!("DKS configuration written for {}", key_label(keyboard, key));
            show_dks(keyboard, key)?;
        }
        Err(e) => eprintln!("Failed to set DKS config: {e}"),
    }
    Ok(())
}

fn show_dks(keyboard: &KeyboardInterface, key: u8) -> CommandResult {
    let precision = keyboard.get_precision().unwrap_or_default();

    match keyboard.get_dks_config(key) {
        Ok(config) => {
            let trigger = keyboard.get_key_trigger(key).ok();
            println!("DKS config for {}:", key_label(keyboard, key));
            if let Some(t) = trigger {
                println!("  Mode: {}", ModeByte::new(t.mode, t.rapid_trigger));
            }
            println!(
                "  Trigger-point travel: {} (raw {})",
                TravelDepth::from_raw(config.trigger_point_travel_raw).format(precision),
                config.trigger_point_travel_raw
            );
            println!("  Binding rows (packed): {:02X?}", config.trigger_modes());
            for (i, binding) in config.bindings.iter().enumerate() {
                let combo = if binding.is_empty() {
                    "(empty)".to_string()
                } else {
                    KeyAction::from_config_bytes(binding.config).to_string()
                };
                let phases: Vec<String> = DksPhase::ALL
                    .iter()
                    .map(|p| format!("{}={}", p.short_label(), binding.phase_actions[p.index()]))
                    .collect();
                println!(
                    "  Binding {i}: combo={combo}  phases=[{}]",
                    phases.join(", ")
                );
            }
        }
        Err(e) => eprintln!("Failed to read DKS config: {e}"),
    }
    Ok(())
}

/// Diagnostic: prove (or disprove) that setting one key to DKS disturbs other
/// keys' trigger state. Snapshots all keys, does a read-stability check (two
/// reads, no writes), sets `key` to DKS, snapshots again, diffs, then restores
/// the key's original mode/travel and snapshots once more.
pub fn dks_roundtrip(keyboard: &KeyboardInterface, key: u8, op: &str) -> CommandResult {
    let snap = |label: &str| match keyboard.get_all_triggers() {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("[{label}] read failed: {e}");
            None
        }
    };

    let orig_trigger = keyboard.get_key_trigger(key)?;
    println!(
        "Roundtrip op '{op}' on key {key} ({}), current mode {}\n",
        keyboard.matrix_key_name(key as usize),
        ModeByte::new(orig_trigger.mode, orig_trigger.rapid_trigger),
    );

    let Some(a) = snap("A") else { return Ok(()) };
    let Some(b) = snap("B") else { return Ok(()) };
    let read_noise = diff_triggers("read-stability (A vs B, no writes)", &a, &b, keyboard);

    // Isolate which write in the DKS sequence desyncs the read pipeline.
    match op {
        "travel" => keyboard.set_dks_trigger_point_travel_raw(key, 70)?,
        "modes" => keyboard.set_dks_trigger_modes(key, [0, 0, 0, 0])?,
        "combo" => keyboard.set_keymatrix_config(
            keyboard.active_profile(),
            key,
            KeymatrixLayer::BASE,
            [0; 4],
            true,
        )?,
        "mode-all" => keyboard.set_mode_all(ModeByte::new(KeyMode::Normal, false))?,
        "keytrig" => keyboard.set_key_trigger(&orig_trigger)?,
        _ => {
            let config = DksConfig {
                trigger_point_travel_raw: 0,
                bindings: [DksBinding::default(); 4],
            };
            keyboard.set_dks_config(key, &config, Some(orig_trigger.rapid_trigger))?;
        }
    }

    let Some(c) = snap("C") else { return Ok(()) };
    let after_write = diff_triggers(
        &format!("after set key {key} -> DKS (B vs C)"),
        &b,
        &c,
        keyboard,
    );

    keyboard.set_key_trigger(&orig_trigger)?;
    let Some(d) = snap("D") else { return Ok(()) };
    diff_triggers(
        &format!("after restoring key {key} (C vs D)"),
        &c,
        &d,
        keyboard,
    );
    diff_triggers("net change vs start (A vs D)", &a, &d, keyboard);

    println!("\nVerdict:");
    if read_noise > 0 {
        println!(
            "  ⚠ {read_noise} keys differ between two back-to-back reads with no writes \
             → the readback itself is unstable (response desync), not real corruption."
        );
    }
    let cross = after_write.saturating_sub(1); // the target key is expected to change
    if cross > 0 {
        println!(
            "  ✗ setting key {key} to DKS changed {cross} OTHER key(s) — real cross-key effect."
        );
    } else if read_noise == 0 {
        println!("  ✓ only key {key} changed; no other keys were disturbed.");
    }
    Ok(())
}

/// Compare two full trigger snapshots; print every key whose per-key state
/// changed and return the count of changed keys.
fn diff_triggers(
    label: &str,
    before: &monsgeek_keyboard::TriggerSettings,
    after: &monsgeek_keyboard::TriggerSettings,
    keyboard: &KeyboardInterface,
) -> usize {
    let n = before
        .key_modes
        .len()
        .min(after.key_modes.len())
        .max(before.press_travel.len().min(after.press_travel.len()));
    let mut changed = 0usize;
    println!("=== {label} ===");
    for i in 0..n {
        let mut fields = Vec::new();
        let bm = before.key_modes.get(i).copied();
        let am = after.key_modes.get(i).copied();
        if bm != am {
            let (bb, ab) = (bm.unwrap_or(0), am.unwrap_or(0));
            fields.push(format!(
                "mode 0x{bb:02X} ({}) -> 0x{ab:02X} ({})",
                ModeByte::from_u8(bb),
                ModeByte::from_u8(ab),
            ));
        }
        for (name, bv, av) in [
            ("press", &before.press_travel, &after.press_travel),
            ("lift", &before.lift_travel, &after.lift_travel),
            ("rt_press", &before.rt_press, &after.rt_press),
            ("rt_lift", &before.rt_lift, &after.rt_lift),
            ("bot_dz", &before.bottom_deadzone, &after.bottom_deadzone),
            ("top_dz", &before.top_deadzone, &after.top_deadzone),
        ] {
            let x = bv.get(i).copied();
            let y = av.get(i).copied();
            if x != y {
                fields.push(format!("{name} {} -> {}", x.unwrap_or(0), y.unwrap_or(0)));
            }
        }
        if !fields.is_empty() {
            changed += 1;
            println!(
                "  key {i:>3} ({:<8}): {}",
                keyboard.matrix_key_name(i),
                fields.join(", ")
            );
        }
    }
    if changed == 0 {
        println!("  (no changes)");
    } else {
        println!("  {changed} key(s) changed");
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_plan_default_release_equals_actuation() {
        let plan = standard_rt_plan(0.8, 0.2, None, Precision::Medium).expect("valid plan");
        assert_eq!(plan.actuation, 80);
        assert_eq!(plan.sensitivity, 20);
        assert_eq!(plan.release, 80);
    }

    #[test]
    fn standard_plan_custom_release() {
        let plan = standard_rt_plan(0.8, 0.2, Some(0.6), Precision::Medium).expect("valid plan");
        assert_eq!(plan.actuation, 80);
        assert_eq!(plan.sensitivity, 20);
        assert_eq!(plan.release, 60);
    }

    #[test]
    fn zero_and_negative_values_are_refused() {
        assert!(standard_rt_plan(0.0, 0.4, None, Precision::Medium).is_err());
        assert!(standard_rt_plan(-1.0, 0.4, None, Precision::Medium).is_err());
        assert!(standard_rt_plan(1.5, 0.0, None, Precision::Medium).is_err());
        assert!(standard_rt_plan(1.5, -0.4, None, Precision::Medium).is_err());
        assert!(standard_rt_plan(1.5, 0.4, Some(0.0), Precision::Medium).is_err());
    }

    /// NaN compares false against everything, so a plain `<= 0.0` guard would
    /// wave it through and write a garbage release point.
    #[test]
    fn nan_is_refused() {
        assert!(standard_rt_plan(f32::NAN, 0.4, None, Precision::Medium).is_err());
        assert!(standard_rt_plan(1.5, f32::NAN, None, Precision::Medium).is_err());
        assert!(standard_rt_plan(1.5, 0.4, Some(f32::NAN), Precision::Medium).is_err());
    }

    /// On 0.1 mm firmware, 0.04 mm rounds to nothing.
    #[test]
    fn sensitivity_that_rounds_away_is_refused() {
        assert!(standard_rt_plan(1.5, 0.04, None, Precision::Coarse).is_err());
        assert!(standard_rt_plan(1.5, 0.1, None, Precision::Coarse).is_ok());
    }
}
