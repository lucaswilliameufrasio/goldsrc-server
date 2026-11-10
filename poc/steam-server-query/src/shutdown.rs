use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_termination_signal(_signal: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
pub fn install() -> io::Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handle_termination_signal as *const () as libc::sighandler_t;
    action.sa_flags = 0;

    // SAFETY: `action` is initialized, its mask is valid, and the handler only
    // stores to a lock-free atomic. The old action is not needed.
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0 {
        return Err(io::Error::last_os_error());
    }

    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: `action` remains alive for this call and contains a valid
        // handler/mask. A null old-action pointer is explicitly permitted.
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}

#[cfg(unix)]
pub fn is_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Relaxed)
}

#[cfg(not(unix))]
pub fn install() -> io::Result<()> {
    Ok(())
}

#[cfg(not(unix))]
pub fn is_requested() -> bool {
    false
}
