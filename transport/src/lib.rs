pub mod adapter;
mod payload;

use std::{
    collections::HashMap,
    io::{Error, Read},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicU32, AtomicU64},
        mpsc::{channel, Sender},
        Arc, RwLock,
    },
    thread,
};

use adapter::StreamReceiverAdapter;
use bytes::BytesMut;
use common::atomic::EasyAtomic;
use service::{signal::Signal, SocketKind, StreamInfo};
use smallvec::SmallVec;

use crate::{
    adapter::StreamSenderAdapter,
    payload::{Muxer, PacketInfo, Remuxer},
};

pub fn init() -> bool {
    srt::startup()
}

pub fn exit() {
    srt::cleanup()
}

#[derive(Debug, Clone, Copy)]
pub struct TransportOptions {
    pub server: SocketAddr,
    pub multicast: Ipv4Addr,
    pub mtu: usize,
}

#[derive(Debug)]
pub struct Transport {
    index: AtomicU32,
    options: TransportOptions,
    channels: Arc<RwLock<HashMap<u32, Sender<Signal>>>>,
}

impl Transport {
    pub fn new(options: TransportOptions) -> Result<Self, Error> {
        let channels: Arc<RwLock<HashMap<u32, Sender<Signal>>>> = Default::default();
        let mut socket = TcpStream::connect(options.server)?;

        let channels_ = Arc::downgrade(&channels);
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            let mut bytes = BytesMut::with_capacity(2000);

            while let Ok(size) = socket.read(&mut buf) {
                if size == 0 {
                    break;
                }

                bytes.extend_from_slice(&buf[..size]);
                if let Some((size, signal)) = Signal::decode(&bytes) {
                    let _ = bytes.split_to(size);

                    if let Some(channels) = channels_.upgrade() {
                        let mut closeds: SmallVec<[u32; 10]> = SmallVec::with_capacity(10);

                        {
                            for (id, tx) in channels.read().unwrap().iter() {
                                if tx.send(signal).is_err() {
                                    closeds.push(*id);
                                }
                            }
                        }

                        if !closeds.is_empty() {
                            for id in closeds {
                                if channels.write().unwrap().remove(&id).is_some() {
                                    log::error!("channel is close, id={}", id)
                                }
                            }
                        }
                    } else {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            index: AtomicU32::new(0),
            options,
            channels,
        })
    }

    pub fn create_sender(&self, id: u32, adapter: &Arc<StreamSenderAdapter>) -> Result<(), Error> {
        let mut mcast_sender = multicast::Server::new(
            self.options.multicast,
            "0.0.0.0".parse().unwrap(),
            self.options.mtu,
        )?;

        let mut opt = srt::Options::default();
        opt.latency = 20;
        opt.mtu = self.options.mtu as u32;
        opt.stream_id = Some(
            StreamInfo {
                port: Some(mcast_sender.local_addr().port()),
                kind: SocketKind::Publisher,
                id,
            }
            .encode(),
        );

        let mut encoder = srt::FragmentEncoder::new(self.options.mtu);
        let sender = srt::Socket::connect(self.options.server, opt)?;
        log::info!("sender connect to server={}", self.options.server);

        let adapter_ = Arc::downgrade(adapter);
        thread::spawn(move || {
            'a: while let Some(adapter) = adapter_.upgrade() {
                if let Some((buf, kind, flags, timestamp)) = adapter.next() {
                    let payload = Muxer::mux(
                        PacketInfo {
                            kind,
                            flags,
                            timestamp,
                        },
                        buf.as_ref(),
                    );

                    if adapter.get_multicast() {
                        if let Err(e) = mcast_sender.send(&payload) {
                            log::error!("failed to send buf in multicast, err={:?}", e);

                            break 'a;
                        }
                    } else {
                        for chunk in encoder.encode(&payload) {
                            if let Err(e) = sender.send(chunk) {
                                log::error!("failed to send buf in srt, err={:?}", e);

                                break 'a;
                            }
                        }
                    }
                } else {
                    break;
                }
            }

            log::info!("adapter recv a none, close the worker.");

            if let Some(adapter) = adapter_.upgrade() {
                adapter.close();
                sender.close();
            }
        });

        Ok(())
    }

    pub fn create_receiver(
        &self,
        id: u32,
        adapter: &Arc<StreamReceiverAdapter>,
    ) -> Result<(), Error> {
        let mut opt = srt::Options::default();
        opt.latency = 20;
        opt.mtu = self.options.mtu as u32;
        opt.stream_id = Some(
            StreamInfo {
                kind: SocketKind::Subscriber,
                port: None,
                id,
            }
            .encode(),
        );

        let sequence = Arc::new(AtomicU64::new(0));
        let mut decoder = srt::FragmentDecoder::new();
        let receiver = Arc::new(srt::Socket::connect(self.options.server, opt)?);
        log::info!("receiver connect to server={}", self.options.server);

        {
            let index = self.index.get();
            self.index
                .update(if index == u32::MAX { 0 } else { index + 1 });

            let (tx, rx) = channel();
            self.channels.write().unwrap().insert(index, tx);

            let local_id = id;
            let multicast = self.options.multicast;
            let sequence_ = sequence.clone();
            let receiver_ = Arc::downgrade(&receiver);
            let adapter_ = Arc::downgrade(adapter);
            thread::spawn(move || {
                while let Ok(signal) = rx.recv() {
                    match signal {
                        Signal::Start { id, port } => {
                            if id == local_id {
                                let mcast_rceiver = if let Ok(socket) = multicast::Socket::new(
                                    multicast,
                                    SocketAddr::new("0.0.0.0".parse().unwrap(), port),
                                ) {
                                    socket
                                } else {
                                    break;
                                };

                                let sequence_ = sequence_.clone();
                                let adapter_ = adapter_.clone();
                                thread::spawn(move || {
                                    while let Some((seq, bytes)) = mcast_rceiver.read() {
                                        if let Some(adapter) = adapter_.upgrade() {
                                            if seq + 1 == sequence_.get() {
                                                if let Some((offset, info)) = Remuxer::remux(&bytes)
                                                {
                                                    if !adapter.send(
                                                        bytes.slice(offset..),
                                                        info.kind,
                                                        info.flags,
                                                        info.timestamp,
                                                    ) {
                                                        log::error!("adapter on buf failed.");

                                                        break;
                                                    }
                                                } else {
                                                    adapter.loss_pkt();
                                                }
                                            } else {
                                                adapter.loss_pkt()
                                            }
                                        }

                                        sequence_.update(seq);
                                    }
                                });
                            }
                        }
                        Signal::Stop { id } => {
                            if id == local_id {
                                break;
                            }
                        }
                    }
                }

                if let (Some(adapter), Some(receiver)) = (adapter_.upgrade(), receiver_.upgrade()) {
                    adapter.close();
                    receiver.close();
                }
            });
        }

        let adapter_ = Arc::downgrade(adapter);
        thread::spawn(move || {
            let mut buf = [0u8; 2000];

            while let Ok(size) = receiver.read(&mut buf) {
                if let Some((seq, bytes)) = decoder.decode(&buf[..size]) {
                    if let Some(adapter) = adapter_.upgrade() {
                        if seq + 1 == sequence.get() {
                            if let Some((offset, info)) = Remuxer::remux(&bytes) {
                                if !adapter.send(
                                    bytes.slice(offset..),
                                    info.kind,
                                    info.flags,
                                    info.timestamp,
                                ) {
                                    log::error!("adapter on buf failed.");

                                    break;
                                }
                            } else {
                                adapter.loss_pkt();
                            }
                        } else {
                            adapter.loss_pkt()
                        }
                    }

                    sequence.update(seq);
                }
            }

            log::warn!("receiver is closed, id={}", id);

            if let Some(adapter) = adapter_.upgrade() {
                adapter.close();
                receiver.close();
            }
        });

        Ok(())
    }
}
