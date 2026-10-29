use std::{sync::mpsc::SyncSender, thread};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Active,
    QuitRequested,
    Closed,
}

impl Lifecycle {
    pub fn request_quit<T: Send + 'static>(&mut self, output: SyncSender<T>, quit: T) {
        if *self == Self::Active {
            *self = Self::QuitRequested;
            thread::spawn(move || {
                let _ = output.send(quit);
            });
        }
    }

    pub fn dismiss(&mut self) -> bool {
        if *self == Self::Active {
            *self = Self::Closed;
            true
        } else {
            false
        }
    }

    pub fn close(&mut self) -> bool {
        if *self == Self::Closed {
            false
        } else {
            *self = Self::Closed;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn quit_survives_full_queue_and_dismiss_until_host_acknowledges() {
        let (output, input) = mpsc::sync_channel(1);
        output.send("refresh").unwrap();
        let mut state = Lifecycle::Active;
        state.request_quit(output.clone(), "quit");
        state.request_quit(output, "duplicate quit");
        assert!(!state.dismiss());
        assert_eq!(input.recv().unwrap(), "refresh");
        assert_eq!(input.recv_timeout(Duration::from_secs(1)).unwrap(), "quit");
        assert!(input.recv_timeout(Duration::from_millis(20)).is_err());
        assert!(state.close());
        assert!(!state.close());
    }
}
