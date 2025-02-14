#![cfg(not(target_os = "macos"))]

mod audio;

#[cfg(target_os = "windows")]
mod win32;

#[cfg(target_os = "linux")]
mod unix;

#[cfg(target_os = "windows")]
use win32::{CameraCapture, ScreenCapture};

#[cfg(target_os = "linux")]
use unix::ScreenCapture;

use self::audio::AudioCapture;

use anyhow::Result;
use frame::{AudioFrame, VideoFrame};

/// Don't forget to initialize the environment, this is necessary for the
/// capture module.
pub fn startup() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        log::info!("capture MediaFoundation satrtup");

        self::win32::startup()?;
    }

    Ok(())
}

pub fn shutdown() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        log::info!("capture MediaFoundation shutdown");

        self::win32::shutdown()?;
    }

    Ok(())
}

pub trait FrameArrived: Sync + Send {
    /// The type of data captured, such as video frames.
    type Frame;

    /// This method is called when the capture source captures new data. If it
    /// returns false, the source stops capturing.
    fn sink(&mut self, frame: &Self::Frame) -> bool;
}

pub trait CaptureHandler: Sync + Send {
    type Error;

    /// The type of data captured, such as video frames.
    type Frame;

    /// Start capturing configuration information, which may be different for
    /// each source.
    type CaptureOptions;

    /// Get a list of sources, such as multiple screens in a display source.
    fn get_sources() -> Result<Vec<Source>, Self::Error>;

    /// Stop capturing the current source.
    fn stop(&self) -> Result<(), Self::Error>;

    /// Start capturing. This function will not block until capturing is
    /// stopped, and it maintains its own capture thread internally.
    fn start<S: FrameArrived<Frame = Self::Frame> + 'static>(
        &self,
        options: Self::CaptureOptions,
        arrived: S,
    ) -> Result<(), Self::Error>;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Camera = 1,
    Screen = 2,
    Audio = 3,
}

#[derive(Debug, Clone)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub index: usize,
    pub kind: SourceType,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct VideoCaptureSourceDescription {
    pub source: Source,
    pub size: Size,
    pub fps: u8,
}

#[derive(Debug, Clone)]
pub struct AudioCaptureSourceDescription {
    pub source: Source,
    pub sample_rate: u32,
}

pub struct SourceCaptureOptions<T, P> {
    pub description: P,
    pub arrived: T,
}

pub struct CaptureOptions<V, A>
where
    V: FrameArrived<Frame = VideoFrame>,
    A: FrameArrived<Frame = AudioFrame>,
{
    pub video: Option<SourceCaptureOptions<V, VideoCaptureSourceDescription>>,
    pub audio: Option<SourceCaptureOptions<A, AudioCaptureSourceDescription>>,
}

impl<V, A> Default for CaptureOptions<V, A>
where
    V: FrameArrived<Frame = VideoFrame>,
    A: FrameArrived<Frame = AudioFrame>,
{
    fn default() -> Self {
        Self {
            video: None,
            audio: None,
        }
    }
}

enum CaptureImplement {
    // Camera(CameraCapture),
    Screen(ScreenCapture),
    Audio(AudioCapture),
}

#[derive(Default)]
pub struct Capture(Vec<CaptureImplement>);

impl Capture {
    /// Returns a list of devices by type.
    #[allow(unreachable_patterns)]
    pub fn get_sources(kind: SourceType) -> Result<Vec<Source>> {
        log::info!("capture get sources, kind={:?}", kind);

        Ok(match kind {
            // SourceType::Camera => CameraCapture::get_sources()?,
            SourceType::Screen => ScreenCapture::get_sources()?,
            SourceType::Audio => AudioCapture::get_sources()?,
            _ => Vec::new(),
        })
    }

    pub fn new<V, A>(CaptureOptions { video, audio }: CaptureOptions<V, A>) -> Result<Self>
    where
        V: FrameArrived<Frame = VideoFrame> + 'static,
        A: FrameArrived<Frame = AudioFrame> + 'static,
    {
        let mut devices = Vec::with_capacity(3);

        if let Some(SourceCaptureOptions {
            description,
            arrived,
        }) = video
        {
            match description.source.kind {
                // SourceType::Camera => {
                //     let camera = CameraCapture::default();
                //     camera.start(description, arrived)?;
                //     devices.push(CaptureImplement::Camera(camera));
                // }
                SourceType::Screen => {
                    let screen = ScreenCapture::default();
                    screen.start(description, arrived)?;
                    devices.push(CaptureImplement::Screen(screen));
                }
                _ => (),
            }
        }

        if let Some(SourceCaptureOptions {
            description,
            arrived,
        }) = audio
        {
            let audio = AudioCapture::default();
            audio.start(description, arrived)?;
            devices.push(CaptureImplement::Audio(audio));
        }

        Ok(Self(devices))
    }

    pub fn close(&self) -> Result<()> {
        log::info!("close capture");

        for item in self.0.iter() {
            match item {
                CaptureImplement::Screen(it) => it.stop(),
                // CaptureImplement::Camera(it) => it.stop(),
                CaptureImplement::Audio(it) => it.stop(),
            }?;
        }

        Ok(())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        log::info!("capture drop");

        drop(self.close());
    }
}
