//! Only Session::prepare opens endpoints, and only the explicit Start action calls it.
use super::*;
use crate::{recorder::Control, windows_support::Apartment};
use std::{
    sync::{
        Arc,
        mpsc::{self, SyncSender},
    },
    thread::JoinHandle,
    time::Duration,
};
use windows::{
    Win32::{
        Media::{
            Audio::*,
            KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE},
            Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT},
        },
        System::Com::*,
    },
    core::GUID,
};

struct Endpoint {
    capture: IAudioCaptureClient,
    client: IAudioClient,
    format: Format,
    writer: Option<TrackWriter>,
    first: bool,
    running: bool,
}
impl Endpoint {
    fn open(enumerator: &IMMDeviceEnumerator, kind: Kind, path: &Path) -> Result<Self> {
        unsafe {
            let flow = match kind {
                Kind::Microphone => eCapture,
                Kind::System => eRender,
            };
            let device = enumerator
                .GetDefaultAudioEndpoint(flow, eConsole)
                .context("Default audio endpoint is unavailable")?;
            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .context("Cannot activate WASAPI endpoint")?;
            let raw = client.GetMixFormat()?;
            ensure!(!raw.is_null(), "WASAPI returned no mix format");
            let parsed = (|| -> Result<Format> {
                let wave = std::ptr::read_unaligned(raw);
                let (tag, mask) = if wave.wFormatTag as u32 == WAVE_FORMAT_EXTENSIBLE {
                    ensure!(wave.cbSize >= 22, "Truncated extensible audio format");
                    let extension = std::ptr::read_unaligned(raw.cast::<WAVEFORMATEXTENSIBLE>());
                    (extension.SubFormat, extension.dwChannelMask)
                } else {
                    (
                        GUID::from_u128(if wave.wFormatTag as u32 == WAVE_FORMAT_PCM {
                            KSDATAFORMAT_SUBTYPE_PCM.to_u128()
                        } else if wave.wFormatTag as u32 == WAVE_FORMAT_IEEE_FLOAT {
                            KSDATAFORMAT_SUBTYPE_IEEE_FLOAT.to_u128()
                        } else {
                            0
                        }),
                        0,
                    )
                };
                let encoding = match (tag, wave.wBitsPerSample) {
                    (t, 32) if t == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT => Encoding::Float32,
                    (t, 16) if t == KSDATAFORMAT_SUBTYPE_PCM => Encoding::Pcm16,
                    (t, 24) if t == KSDATAFORMAT_SUBTYPE_PCM => Encoding::Pcm24,
                    (t, 32) if t == KSDATAFORMAT_SUBTYPE_PCM => Encoding::Pcm32,
                    _ => anyhow::bail!(
                        "Unsupported endpoint mix format (expected float32 or PCM16/24/32)"
                    ),
                };
                let format = Format {
                    rate: wave.nSamplesPerSec,
                    channels: wave.nChannels,
                    encoding,
                    mask,
                };
                format.validate()?;
                ensure!(
                    wave.nBlockAlign as usize == format.bytes_per_frame(),
                    "Unsupported PCM block alignment"
                );
                Ok(format)
            })();
            let flags = if kind == Kind::System {
                AUDCLNT_STREAMFLAGS_LOOPBACK
            } else {
                0
            };
            let initialized = if parsed.is_ok() {
                client
                    .Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 10_000_000, 0, raw, None)
                    .map_err(anyhow::Error::from)
            } else {
                Ok(())
            };
            CoTaskMemFree(Some(raw.cast()));
            let format = parsed?;
            initialized.context("WASAPI shared-mode initialization failed")?;
            let capture = client.GetService()?;
            Ok(Self {
                capture,
                client,
                format,
                writer: Some(TrackWriter::create(&kind.path(path))?),
                first: true,
                running: false,
            })
        }
    }
    fn start(&mut self) -> Result<()> {
        unsafe {
            self.client.Reset()?;
            self.client.Start()?;
        }
        self.first = true;
        self.running = true;
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        if self.running {
            unsafe {
                self.client.Stop()?;
            }
            self.running = false;
        }
        Ok(())
    }
    fn pump(&mut self, control: &Control) -> Result<()> {
        unsafe {
            // A fixed endpoint buffer and bounded per-pass packet count prevent unbounded work.
            for _ in 0..512 {
                if self.capture.GetNextPacketSize()? == 0 {
                    return Ok(());
                }
                let (mut data, mut frames, mut flags, mut timestamp) =
                    (std::ptr::null_mut(), 0, 0, 0);
                self.capture.GetBuffer(
                    &mut data,
                    &mut frames,
                    &mut flags,
                    None,
                    Some(&mut timestamp),
                )?;
                let work = (|| -> Result<()> {
                    ensure!(
                        flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 == 0,
                        "WASAPI packet timestamp error; stopped to preserve synchronization"
                    );
                    ensure!(
                        self.first || flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 == 0,
                        "Audio data discontinuity (device changed or capture overrun); completed packets were retained"
                    );
                    self.first = false;
                    ensure!(
                        frames <= self.format.rate,
                        "WASAPI packet is larger than the one-second bound"
                    );
                    let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                    ensure!(silent || !data.is_null(), "WASAPI returned null audio data");
                    let bytes = if silent {
                        &[][..]
                    } else {
                        std::slice::from_raw_parts(
                            data,
                            frames as usize * self.format.bytes_per_frame(),
                        )
                    };
                    let end = timestamp
                        + frames as u64 * crate::timeline::TICKS_PER_SECOND
                            / self.format.rate as u64;
                    let packets = convert(
                        self.format,
                        bytes,
                        frames,
                        silent,
                        timestamp,
                        &control.spans(timestamp, end),
                    )?;
                    for packet in packets {
                        self.writer.as_mut().unwrap().append(&packet)?;
                    }
                    Ok(())
                })();
                let release = self
                    .capture
                    .ReleaseBuffer(frames)
                    .map_err(anyhow::Error::from);
                work?;
                release?;
            }
            anyhow::bail!("WASAPI could not drain its bounded capture buffer")
        }
    }
    fn finish(&mut self) -> Result<()> {
        let stop = self.stop();
        let flush = self.writer.take().map_or(Ok(()), |w| w.finish());
        stop.and(flush)
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
enum Command {
    Start,
    Finish,
}
pub struct Session {
    command: SyncSender<Command>,
    worker: Option<JoinHandle<Result<()>>>,
}
impl Session {
    pub fn prepare(path: &Path, options: Options, control: Arc<Control>) -> Result<Self> {
        let path = path.to_path_buf();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (command, rx) = mpsc::sync_channel(2);
        let worker = std::thread::spawn(move || -> Result<()> {
            let apartment = Apartment::new();
            let initialization = (|| -> Result<_> {
                let apartment = apartment?;
                let enumerator: IMMDeviceEnumerator =
                    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
                let mut endpoints = Vec::new();
                if options.microphone {
                    endpoints.push(
                        Endpoint::open(&enumerator, Kind::Microphone, &path)
                            .context("Microphone capture could not start")?,
                    );
                }
                if options.system {
                    endpoints.push(
                        Endpoint::open(&enumerator, Kind::System, &path)
                            .context("System audio capture could not start")?,
                    );
                }
                Ok((apartment, endpoints))
            })();
            let (_apartment, mut endpoints) = match initialization {
                Ok(value) => {
                    let _ = ready_tx.send(Ok(()));
                    value
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("{e:#}")));
                    return Err(e);
                }
            };
            let result = (|| -> Result<()> {
                if !matches!(rx.recv(), Ok(Command::Start)) {
                    return Ok(());
                }
                for endpoint in &mut endpoints {
                    endpoint.start()?;
                }
                loop {
                    if control.is_stopped() {
                        for e in &mut endpoints {
                            if e.running {
                                e.pump(&control)?;
                            }
                        }
                        break;
                    }
                    let paused = control.is_paused();
                    for endpoint in &mut endpoints {
                        if paused {
                            if endpoint.running {
                                endpoint.pump(&control)?;
                                endpoint.stop()?;
                            }
                        } else {
                            if !endpoint.running {
                                endpoint.start()?;
                            }
                            endpoint.pump(&control)?;
                        }
                    }
                    match rx.recv_timeout(Duration::from_millis(if paused { 50 } else { 10 })) {
                        Ok(Command::Finish) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        _ => {}
                    }
                }
                Ok(())
            })();
            if result.is_err() {
                control.stop();
            }
            let mut finalization = Ok(());
            for endpoint in &mut endpoints {
                if result.is_ok()
                    && endpoint.running
                    && let Err(e) = endpoint.pump(&control)
                {
                    finalization = Err(e);
                }
                if let Err(e) = endpoint.finish() {
                    finalization = Err(e);
                }
            }
            result.and(finalization)
        });
        match ready_rx
            .recv()
            .context("Audio worker initialization was interrupted")?
        {
            Ok(()) => Ok(Self {
                command,
                worker: Some(worker),
            }),
            Err(e) => {
                let _ = worker.join();
                anyhow::bail!(e)
            }
        }
    }
    pub fn start(&mut self) -> Result<()> {
        self.command
            .send(Command::Start)
            .context("Audio worker stopped before recording")
    }
    pub fn finish(mut self) -> Result<()> {
        let _ = self.command.try_send(Command::Finish);
        self.worker
            .take()
            .unwrap()
            .join()
            .map_err(|_| anyhow::anyhow!("Audio worker panicked"))?
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.command.try_send(Command::Finish);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
