//! O som da sala no Windows: o que chega toca pelo WASAPI, e o microfone sobe por ele.
//!
//! As duas pontas pedem ao Windows o formato do núcleo — 48 kHz, estéreo, `f32` — com o
//! `AUTOCONVERTPCM`, e o próprio Windows converte para o que a placa usa. Sem ele, uma saída
//! em 44,1 kHz recusaria o `Initialize` e a sala ficaria muda.
//!
//! Cada ponta é uma thread dona do seu `IAudioClient`: o COM do WASAPI não atravessa
//! thread, e é ela que abre, toca e fecha. A interface só conversa com o `Speaker` e o
//! `Microphone`, que são `Send`.

// Fora do Windows o app só compila — para o `clippy --workspace` do Linux —, e a mistura que
// a thread do WASAPI usa fica sem quem chame.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Result, anyhow};

/// O formato do núcleo: o que o `AudioUnpacker` devolve e o que o `AudioFeed` espera.
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 2;

/// Amostras (já contando os dois canais) em um milissegundo.
const PER_MILLISECOND: usize = SAMPLE_RATE as usize * CHANNELS / 1_000;

/// Quanto de cada pessoa se junta antes de tocar. Pacote de rede chega aos trancos; tocar
/// na hora faz cada tranco virar um estalo.
const CUSHION: usize = 40 * PER_MILLISECOND;

/// O máximo que se deixa acumular. Relógio de placa nunca bate com o de quem manda, e sem
/// teto o atraso só cresce: passou disto, o mais velho vai fora até sobrar a folga.
const LONGEST: usize = 200 * PER_MILLISECOND;

/// De quanto em quanto tempo as threads olham o WASAPI. O buffer tem 50 ms: dez de
/// intervalo deixam folga para a thread atrasar sem a placa ficar sem som.
const TICK: Duration = Duration::from_millis(10);

/// Em 100 ns, o tamanho do buffer que se pede ao Windows.
const BUFFER: i64 = 500_000;

/// O que cada pessoa mandou e ainda não tocou.
#[derive(Default)]
struct Lane {
    samples: VecDeque<f32>,
    /// Já juntou a folga desde a última vez que secou.
    primed: bool,
    volume: f32,
}

type Mix = Arc<Mutex<HashMap<String, Lane>>>;

pub struct Speaker {
    mix: Mix,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Speaker {
    /// Abre a saída escolhida, ou a padrão do sistema. Não falha: sem saída, a sala segue
    /// sem som e o motivo vai para o log.
    pub fn start(device: Option<String>) -> Self {
        let (mix, stop) = (Mix::default(), Arc::new(AtomicBool::new(false)));
        let thread = std::thread::Builder::new()
            .name("unkvoid-som".into())
            .spawn({
                let (mix, stop) = (mix.clone(), stop.clone());

                move || {
                    if let Err(failure) = platform::render(device.as_deref(), &mix, &stop) {
                        tracing::warn!(%failure, "som: a saída de áudio não abriu");
                    }
                }
            })
            .ok();

        Self { mix, stop, thread }
    }

    /// Um bloco de PCM de um producer, estéreo intercalado.
    pub fn play(&self, producer: &str, samples: &[f32]) {
        let mut mix = lock(&self.mix);
        let lane = mix.entry(producer.to_owned()).or_insert_with(|| Lane {
            volume: 1.0,
            ..Lane::default()
        });

        lane.samples.extend(samples);

        if lane.samples.len() > LONGEST {
            let late = lane.samples.len() - CUSHION;

            lane.samples.drain(..late);
        }
    }

    pub fn set_volume(&self, producer: &str, volume: f32) {
        lock(&self.mix)
            .entry(producer.to_owned())
            .or_default()
            .volume = volume.clamp(0.0, 1.0);
    }
}

/// Um toque do app pela saída escolhida, numa saída só dele, aberta pelo tempo do toque: a da
/// sala pode já ter fechado — é o caso do "saiu da voz". Sem o teto de atraso da voz, que
/// cortaria um toque inteiro de uma vez.
pub fn chime(device: Option<String>, samples: Vec<f32>) {
    let spawned = std::thread::Builder::new().name("unkvoid-toque".into()).spawn(move || {
        let speaker = Speaker::start(device);
        let length = Duration::from_millis((samples.len() / PER_MILLISECOND) as u64);

        lock(&speaker.mix).insert(
            "toque".to_owned(),
            Lane {
                samples: samples.into(),
                primed: true,
                volume: 1.0,
            },
        );

        std::thread::sleep(length + Duration::from_millis(250));
    });

    if let Err(failure) = spawned {
        tracing::warn!(%failure, "som: o toque não tocou");
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Soma quem tem som para tocar em `out`. Quem ainda não juntou a folga espera; quem secou
/// no meio volta a esperar — é melhor um instante de silêncio que um estalo por pacote.
fn mix_into(mix: &mut HashMap<String, Lane>, out: &mut [f32]) {
    out.fill(0.0);

    for lane in mix.values_mut() {
        if !lane.primed {
            if lane.samples.len() < CUSHION {
                continue;
            }

            lane.primed = true;
        }

        let taken = out.len().min(lane.samples.len());

        for (slot, sample) in out.iter_mut().zip(lane.samples.drain(..taken)) {
            *slot += sample * lane.volume;
        }

        if lane.samples.is_empty() {
            lane.primed = false;
        }
    }

    for slot in out.iter_mut() {
        *slot = slot.clamp(-1.0, 1.0);
    }
}

pub struct Microphone {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Microphone {
    /// Abre o microfone escolhido, ou o padrão, e entrega cada bloco a `sink` na thread
    /// dele. Só volta depois de o Windows aceitar: microfone que não abriu é erro que a
    /// pessoa precisa ver, e não um silêncio que ela descobre falando.
    pub fn start(device: Option<String>, sink: impl FnMut(&[f32]) + Send + 'static) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (opened, answer) = sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("unkvoid-microfone".into())
            .spawn({
                let stop = stop.clone();

                move || platform::capture(device.as_deref(), sink, &stop, &opened)
            })?;

        match answer.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self { stop, thread: Some(thread) }),
            Ok(Err(failure)) => Err(anyhow!(failure)),
            Err(_) => Err(anyhow!("o microfone não respondeu")),
        }
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock<T>(cell: &Mutex<T>) -> MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(target_os = "windows")]
mod platform {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::SyncSender;
    use std::sync::{Arc, Mutex};

    use anyhow::{Context, Result};
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, EDataFlow, IAudioCaptureClient, IAudioClient,
        IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
        eCapture, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
    use windows::core::PCWSTR;

    use super::{BUFFER, CHANNELS, Lane, SAMPLE_RATE, TICK, lock, mix_into};

    /// `WAVE_FORMAT_IEEE_FLOAT`.
    const FLOAT: u16 = 3;

    /// O COM desta thread, desligado na saída dela mesmo que a abertura falhe no meio.
    struct Apartment;

    impl Apartment {
        fn enter() -> Self {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

            Self
        }
    }

    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    pub fn render(device: Option<&str>, mix: &Arc<Mutex<HashMap<String, Lane>>>, stop: &AtomicBool) -> Result<()> {
        let _apartment = Apartment::enter();

        unsafe {
            let client = open(device, eRender)?;
            let frames = client.GetBufferSize().context("o Windows não disse o tamanho do buffer")?;
            let render: IAudioRenderClient = client.GetService().context("sem cliente de saída")?;
            let mut block = Vec::new();

            client.Start().context("a saída não começou")?;
            tracing::info!(device = device.unwrap_or("padrão"), "som: tocando");

            while !stop.load(Ordering::Relaxed) {
                let free = frames.saturating_sub(client.GetCurrentPadding().unwrap_or(frames));

                if free > 0 {
                    block.resize(free as usize * CHANNELS, 0.0);
                    mix_into(&mut lock(mix), &mut block);

                    let target = render.GetBuffer(free).context("a placa não deu o buffer")?;

                    std::ptr::copy_nonoverlapping(block.as_ptr(), target.cast::<f32>(), block.len());
                    render.ReleaseBuffer(free, 0).context("a placa não aceitou o buffer")?;
                }

                std::thread::sleep(TICK);
            }

            let _ = client.Stop();
        }

        Ok(())
    }

    pub fn capture(
        device: Option<&str>,
        mut sink: impl FnMut(&[f32]),
        stop: &AtomicBool,
        opened: &SyncSender<std::result::Result<(), String>>,
    ) {
        let _apartment = Apartment::enter();

        let started = unsafe {
            open(device, eCapture).and_then(|client| {
                let capture: IAudioCaptureClient = client.GetService().context("sem cliente de captura")?;

                client.Start().context("o microfone não começou")?;

                Ok((client, capture))
            })
        };

        let (client, capture) = match started {
            Ok(started) => {
                let _ = opened.send(Ok(()));

                started
            }
            Err(failure) => {
                let _ = opened.send(Err(format!("{failure:#}")));

                return;
            }
        };

        tracing::info!(device = device.unwrap_or("padrão"), "microfone: aberto");

        let mut block = Vec::new();

        while !stop.load(Ordering::Relaxed) {
            unsafe {
                while capture.GetNextPacketSize().unwrap_or(0) > 0 {
                    let mut data = std::ptr::null_mut();
                    let (mut frames, mut flags) = (0_u32, 0_u32);

                    if capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).is_err() {
                        break;
                    }

                    let length = frames as usize * CHANNELS;

                    block.clear();

                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                        block.resize(length, 0.0);
                    } else {
                        block.extend_from_slice(std::slice::from_raw_parts(data.cast::<f32>(), length));
                    }

                    let _ = capture.ReleaseBuffer(frames);

                    sink(&block);
                }
            }

            std::thread::sleep(TICK);
        }

        let _ = unsafe { client.Stop() };
    }

    unsafe fn open(device: Option<&str>, flow: EDataFlow) -> Result<IAudioClient> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).context("o áudio do Windows não abriu")?;
            let endpoint: IMMDevice = match device {
                Some(id) => {
                    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();

                    enumerator.GetDevice(PCWSTR(wide.as_ptr())).context("o aparelho escolhido sumiu")?
                }
                None => enumerator
                    .GetDefaultAudioEndpoint(flow, eConsole)
                    .context("não há aparelho padrão")?,
            };
            let client: IAudioClient = endpoint.Activate(CLSCTX_ALL, None).context("o aparelho não ativou")?;
            #[allow(clippy::cast_possible_truncation)]
            let format = WAVEFORMATEX {
                wFormatTag: FLOAT,
                nChannels: CHANNELS as u16,
                nSamplesPerSec: SAMPLE_RATE,
                nAvgBytesPerSec: SAMPLE_RATE * CHANNELS as u32 * 4,
                nBlockAlign: CHANNELS as u16 * 4,
                wBitsPerSample: 32,
                cbSize: 0,
            };

            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    BUFFER,
                    0,
                    &format,
                    None,
                )
                .context("o aparelho recusou 48 kHz estéreo")?;

            Ok(client)
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::SyncSender;
    use std::sync::{Arc, Mutex};

    use anyhow::{Result, anyhow};

    use super::Lane;

    pub fn render(_: Option<&str>, _: &Arc<Mutex<HashMap<String, Lane>>>, _: &AtomicBool) -> Result<()> {
        Err(anyhow!("sem WASAPI fora do Windows"))
    }

    pub fn capture(
        _: Option<&str>,
        _: impl FnMut(&[f32]),
        _: &AtomicBool,
        opened: &SyncSender<std::result::Result<(), String>>,
    ) {
        let _ = opened.send(Err("sem WASAPI fora do Windows".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(samples: usize, primed: bool) -> Lane {
        Lane {
            samples: std::iter::repeat_n(0.25, samples).collect(),
            primed,
            volume: 1.0,
        }
    }

    #[test]
    fn nobody_plays_before_the_cushion_is_full() {
        let mut mix = HashMap::from([("ada".to_owned(), lane(CUSHION - 2, false))]);
        let mut out = vec![1.0; 8];

        mix_into(&mut mix, &mut out);

        assert!(out.iter().all(|&sample| sample == 0.0));
        assert_eq!(mix["ada"].samples.len(), CUSHION - 2);
    }

    #[test]
    fn two_people_are_summed_and_the_sum_never_clips_past_one() {
        let mut mix = HashMap::from([
            ("ada".to_owned(), lane(CUSHION, false)),
            ("bia".to_owned(), lane(CUSHION, false)),
        ]);
        let mut out = vec![0.0; 4];

        mix_into(&mut mix, &mut out);

        assert!(out.iter().all(|&sample| (sample - 0.5).abs() < 1e-6), "{out:?}");

        let loud = Lane { samples: std::iter::repeat_n(0.9, CUSHION).collect(), primed: true, volume: 1.0 };
        let mut mix = HashMap::from([("ada".to_owned(), loud), ("bia".to_owned(), lane(CUSHION, true))]);

        mix_into(&mut mix, &mut out);

        assert!(out.iter().all(|&sample| (sample - 1.0).abs() < 1e-6), "{out:?}");
    }

    #[test]
    fn whoever_runs_dry_waits_for_the_cushion_again() {
        let mut mix = HashMap::from([("ada".to_owned(), lane(4, true))]);
        let mut out = vec![0.0; 8];

        mix_into(&mut mix, &mut out);

        assert!(!mix["ada"].primed);
    }

    #[test]
    fn the_volume_scales_only_that_person() {
        let mut mix = HashMap::from([("ada".to_owned(), Lane { volume: 0.5, ..lane(CUSHION, true) })]);
        let mut out = vec![0.0; 4];

        mix_into(&mut mix, &mut out);

        assert!(out.iter().all(|&sample| (sample - 0.125).abs() < 1e-6), "{out:?}");
    }

    #[test]
    fn a_backlog_longer_than_the_ceiling_is_cut_back_to_the_cushion() {
        let speaker = Speaker { mix: Mix::default(), stop: Arc::new(AtomicBool::new(true)), thread: None };

        speaker.play("ada", &vec![0.1; LONGEST + 10]);

        assert_eq!(lock(&speaker.mix)["ada"].samples.len(), CUSHION);
    }
}
