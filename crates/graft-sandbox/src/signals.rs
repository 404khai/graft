//! Ignoring terminal interrupts in the parent of a confined command.

#![allow(unsafe_code)]

/// While alive, the current process ignores `SIGINT` and `SIGQUIT`, so
/// Ctrl-C in a terminal reaches the command `graft run` started (which
/// shares the terminal's process group) without killing `graft run` first.
/// Create it only after the child has been spawned: an ignored signal is
/// inherited across `exec`. Dropping it restores the previous handlers.
#[derive(Debug)]
pub struct IgnoreInterrupts {
    previous: [(libc::c_int, libc::sighandler_t); 2],
}

impl IgnoreInterrupts {
    /// Starts ignoring the signals.
    #[must_use]
    pub fn new() -> Self {
        let mut previous = [
            (libc::SIGINT, libc::SIG_DFL),
            (libc::SIGQUIT, libc::SIG_DFL),
        ];
        for (sig, old) in &mut previous {
            // SAFETY: `signal` with `SIG_IGN` installs no Rust code as a
            // handler; it only changes the disposition of a valid signal
            // number. The previous disposition is kept to restore it.
            let prev = unsafe { libc::signal(*sig, libc::SIG_IGN) };
            if prev != libc::SIG_ERR {
                *old = prev;
            }
        }
        Self { previous }
    }
}

impl Default for IgnoreInterrupts {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for IgnoreInterrupts {
    fn drop(&mut self) {
        for (sig, old) in self.previous {
            // SAFETY: restores a disposition `signal` returned earlier for
            // the same signal, which is either a default/ignore constant or
            // a handler that was already installed in this process.
            unsafe {
                libc::signal(sig, old);
            }
        }
    }
}
