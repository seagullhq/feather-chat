//! One running app at a time, the way Zed does it: the first instance holds a
//! local port; a second one asks it to come to the front, then exits.
//! ponytail: the port is per machine, not per user (two users logged in at
//! once can't both run it); a per-user socket / named pipe if that matters.

use gpui::{AnyWindowHandle, App};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const ADDR: &str = "127.0.0.1:47710";
const HELLO: &[u8] = b"feather-chat:show";
const ACK: &[u8] = b"ok";

/// `Err(())` when another instance answered and was asked to show itself:
/// quit. `Ok(None)` when the port belongs to something else: run anyway.
pub fn claim() -> Result<Option<TcpListener>, ()> {
    if let Ok(l) = TcpListener::bind(ADDR) {
        return Ok(Some(l));
    }
    // Port taken: only an instance of ours answers the hello with ACK.
    let answered = (|| {
        let mut s =
            TcpStream::connect_timeout(&ADDR.parse().ok()?, Duration::from_millis(300)).ok()?;
        s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
        s.write_all(HELLO).ok()?;
        let mut buf = [0; 2];
        s.read_exact(&mut buf).ok()?;
        (buf == ACK).then_some(())
    })();
    match answered {
        Some(()) => Err(()),
        None => Ok(None),
    }
}

/// Brings `window` to the front whenever another launch says hello.
pub fn serve(listener: TcpListener, window: AnyWindowHandle, cx: &mut App) {
    let (tx, rx) = async_channel::unbounded::<()>();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
            let mut buf = [0; HELLO.len()];
            if s.read_exact(&mut buf).is_ok() && buf == HELLO {
                let _ = s.write_all(ACK);
                let _ = tx.send_blocking(());
            }
        }
    });
    cx.spawn(async move |cx| {
        while rx.recv().await.is_ok() {
            let _ = cx.update(|cx| {
                cx.activate(true);
                let _ = window.update(cx, |_, window, _| window.activate_window());
            });
        }
    })
    .detach();
}
