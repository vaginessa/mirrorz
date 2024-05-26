use std::{
    io::{Error, Write},
    net::{SocketAddr, TcpListener},
    sync::Arc,
    thread,
};

use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};

use crate::route::Route;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum Signal {
    /// Start publishing a channel. The port number is the publisher's multicast
    /// port.
    Start { id: u32, port: u16 },
    /// Stop publishing to a channel
    Stop { id: u32 },
}

impl Signal {
    pub fn encode(&self) -> Bytes {
        let payload = rmp_serde::to_vec(&self).unwrap();
        let mut buf = BytesMut::with_capacity(payload.len() + 2);
        buf.put_u16(buf.capacity() as u16);
        buf.extend_from_slice(&payload);
        buf.freeze()
    }

    #[rustfmt::skip]
    pub fn decode(buf: &[u8]) -> Option<(usize, Self)> {
        if buf.len() > 2 {
            let size = u16::from_be_bytes([
                buf[0],
                buf[1],
            ]) as usize;

            if size <= buf.len() {
                return rmp_serde::from_slice(&buf[2..size]).ok().map(|it| (size, it))
            }
        }

        None
    }
}

pub fn start_server(bind: SocketAddr, route: Arc<Route>) -> Result<(), Error> {
    let listener = TcpListener::bind(bind)?;
    while let Ok((mut socket, addr)) = listener.accept() {
        log::info!("new signal socket, addr={}", addr);

        let route = route.clone();
        thread::spawn(move || {
            // Every time a new connection comes online, notify the current link of all
            // published channels.
            {
                for (id, port) in route.get_channels() {
                    if socket
                        .write_all(&Signal::Start { id, port }.encode())
                        .is_err()
                    {
                        return;
                    }
                }
            }

            // todo: The link is closed and the thread cannot be released in time

            // Every time a new publisher comes online, the current connection is notified
            let changer = route.get_changer();
            while let Some(signal) = changer.change() {
                if socket.write_all(&signal.encode()).is_err() {
                    break;
                }
            }

            log::info!("signal socket close, addr={}", addr);
        });
    }

    Ok(())
}
