//! O som da sala no Windows: o que chega toca pelo WASAPI, e o microfone sobe por ele.
//!
//! As duas pontas pedem ao Windows o formato do núcleo — 48 kHz, estéreo, `f32` — com o
//! `AUTOCONVERTPCM`, e o próprio Windows converte para o que a placa usa. Sem ele, uma saída
//! em 44,1 kHz recusaria o `Initialize` e a sala ficaria muda.
//!
//! Cada ponta é uma thread dona do seu `IAudioClient`: o COM do WASAPI não atravessa
//! thread, e é ela que abre, toca e fecha. A interface só conversa com o `Speaker` e o
//! `Microphone`, que são `Send`.

// No macOS o app só compila — para conferir o desenho —, e a mistura que a thread do som usa
// fica sem quem chame.
#![cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]

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
/// teto o atraso só cresce: passou disto, o mais velho vai saindo até sobrar a folga.
const LONGEST: usize = 200 * PER_MILLISECOND;

/// Quanto sai por bloco que chega enquanto o acumulado volta à folga. Cortar tudo de uma vez
/// comia 160 ms — uma palavra inteira; 5 ms por bloco somem no meio da fala e levam ~0,6 s.
const TRIM_STEP: usize = 5 * PER_MILLISECOND;

/// De quanto em quanto tempo as threads olham o WASAPI. O buffer tem 50 ms: dez de
/// intervalo deixam folga para a thread atrasar sem a placa ficar sem som.
const TICK: Duration = Duration::from_millis(10);

/// Em 100 ns, o tamanho do buffer que se pede ao Windows.
#[cfg(target_os = "windows")]
const BUFFER: i64 = 500_000;

/// A maior espera que um som segue: a do `Playout`, que nunca passa de meio segundo.
const MOST_HOLD: usize = 500 * PER_MILLISECOND;

/// O quanto a espera do som pode ficar longe da espera da imagem antes de segui-la. Abaixo
/// disto o descompasso não se percebe (a tolerância de lábio da ITU-R BT.1359 é de ~45 ms com
/// o som adiantado); seguir cada milissegundo que o `Playout` desce cortava o som a cada 100 ms.
const SYNC_SLACK: usize = 40 * PER_MILLISECOND;

/// O maior pedaço que sai por bloco quando a espera desce, em partes iguais: uma descida de
/// 250 ms vira uma dúzia de emendas, e não cinquenta cortes de 5 ms.
const HOLD_STEP: usize = 25 * PER_MILLISECOND;

/// A emenda de cada corte e de cada silêncio: o som se funde no que vem depois em vez de
/// pular de uma amostra para outra, que é o estalo.
const SPLICE: usize = 3 * PER_MILLISECOND;

/// O que cada pessoa mandou e ainda não tocou.
#[derive(Default)]
struct Lane {
    samples: VecDeque<f32>,
    /// Já juntou a folga desde a última vez que secou.
    primed: bool,
    volume: f32,
    /// Passou do teto e ainda está voltando à folga.
    trimming: bool,
    /// A espera desceu e o excesso ainda está saindo.
    shrinking: bool,
    /// A espera a mais que este som segue, em amostras: a da imagem que ele acompanha.
    hold: usize,
}

impl Lane {
    fn push(&mut self, samples: &[f32]) {
        let target = CUSHION + self.hold;

        self.samples.extend(samples);
        self.trimming |= self.samples.len() > LONGEST + self.hold;

        let excess = self.samples.len().saturating_sub(target);
        let amount = if self.trimming {
            excess.min(TRIM_STEP)
        } else if self.shrinking {
            excess / excess.div_ceil(HOLD_STEP).max(1)
        } else {
            return;
        };

        self.cut(amount);

        let over = self.samples.len() > target;

        self.trimming &= over;
        self.shrinking &= over;
    }

    /// A espera cresceu: o que falta entra como silêncio na frente, e o som atrasa junto com a
    /// imagem — é o mesmo trecho que a imagem fica parada. Diminuiu: o excesso sai em poucas
    /// emendas. Dentro de `SYNC_SLACK` nada muda, porque a espera da imagem treme sem parar.
    fn hold(&mut self, wanted: usize) {
        let wanted = wanted.min(MOST_HOLD) & !(CHANNELS - 1);

        if wanted.abs_diff(self.hold) <= SYNC_SLACK {
            return;
        }

        if wanted > self.hold {
            self.delay(wanted - self.hold);
        } else {
            self.shrinking = self.samples.len() > CUSHION + wanted;
        }

        self.hold = wanted;
    }

    /// Tira `amount` amostras da frente: o que ia tocar agora se funde, em `SPLICE`, no que vem
    /// depois do trecho cortado.
    fn cut(&mut self, amount: usize) {
        let amount = amount & !(CHANNELS - 1);
        let blend = SPLICE.min(self.samples.len().saturating_sub(amount));

        for index in 0..blend {
            let rise = ramp(index, blend);

            self.samples[amount + index] = self.samples[index] * (1.0 - rise) + self.samples[amount + index] * rise;
        }

        self.samples.drain(..amount);
    }

    /// Põe `amount` amostras de silêncio na frente. O que ia tocar some aos poucos antes dele e
    /// volta aos poucos depois (os primeiros milissegundos tocam duas vezes, o que não se ouve).
    fn delay(&mut self, amount: usize) {
        let blend = SPLICE.min(self.samples.len()).min(amount) & !(CHANNELS - 1);
        let fading: Vec<f32> = (0..blend).map(|index| self.samples[index] * (1.0 - ramp(index, blend))).collect();

        for index in 0..blend {
            self.samples[index] *= ramp(index, blend);
        }

        for _ in 0..amount - blend {
            self.samples.push_front(0.0);
        }

        for &sample in fading.iter().rev() {
            self.samples.push_front(sample);
        }
    }
}

/// De 0 a 1 ao longo de uma emenda de `length` amostras, igual nos dois canais de cada par.
fn ramp(index: usize, length: usize) -> f32 {
    let frames = (length / CHANNELS).max(1);

    (index / CHANNELS + 1) as f32 / (frames + 1) as f32
}

type Mix = Arc<Mutex<HashMap<String, Lane>>>;

pub struct Speaker {
    mix: Mix,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// A saída escolhida, lida a cada abertura, e o pedido de reabrir nela.
    device: Arc<Mutex<Option<String>>>,
    switch: Arc<AtomicBool>,
}

impl Speaker {
    /// Abre a saída escolhida, ou a padrão do sistema. Não falha: sem saída, a sala segue
    /// sem som, o motivo vai para o log, e ela é tentada de novo de segundo em segundo.
    pub fn start(device: Option<String>) -> Self {
        let (mix, stop) = (Mix::default(), Arc::new(AtomicBool::new(false)));
        let (device, switch) = (Arc::new(Mutex::new(device)), Arc::new(AtomicBool::new(false)));
        let thread = std::thread::Builder::new()
            .name("unkvoid-som".into())
            .spawn({
                let (mix, stop, device, switch) = (mix.clone(), stop.clone(), device.clone(), switch.clone());

                move || platform::render(&device, &switch, &mix, &stop)
            })
            .ok();

        Self { mix, stop, thread, device, switch }
    }

    /// Passa a tocar em outra saída, sem perder o que esperava para tocar.
    pub fn use_device(&self, device: Option<String>) {
        *lock(&self.device) = device;
        self.switch.store(true, Ordering::Relaxed);
    }

    /// Um bloco de PCM de um producer, estéreo intercalado.
    pub fn play(&self, producer: &str, samples: &[f32]) {
        lock(&self.mix).entry(producer.to_owned()).or_insert_with(|| Lane { volume: 1.0, ..Lane::default() }).push(samples);
    }

    /// Quanto o som de um producer espera a mais: a espera da imagem que ele acompanha (o som
    /// da tela segue a tela). Sem isto, numa rede com perda a imagem esperava o `Playout` e o
    /// som dela não, e saíam até meio segundo fora de sincronia.
    pub fn hold(&self, producer: &str, delay: Duration) {
        let wanted = usize::try_from(delay.as_millis()).unwrap_or(usize::MAX).saturating_mul(PER_MILLISECOND);

        lock(&self.mix).entry(producer.to_owned()).or_insert_with(|| Lane { volume: 1.0, ..Lane::default() }).hold(wanted);
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
                ..Lane::default()
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

    use std::time::Duration;

    use anyhow::{Context, Result, anyhow};
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, EDataFlow, IAudioCaptureClient, IAudioClient,
        IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
        eCapture, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
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

    /// Quanto esperar antes de reabrir o aparelho que caiu.
    const REOPEN_AFTER: Duration = Duration::from_secs(1);

    /// De quanto em quanto tique se confere se o padrão do Windows mudou: um segundo.
    const DEFAULT_CHECK_TICKS: u32 = 100;

    /// Toca até mandarem parar. O fone que desconecta, o Bluetooth que reconecta e o padrão do
    /// Windows que muda derrubam o cliente do WASAPI: a saída é reaberta em vez de a sala ficar
    /// muda até sair e entrar de novo. Sem aparelho nenhum, tenta de segundo em segundo.
    pub fn render(chosen: &Mutex<Option<String>>, switch: &AtomicBool, mix: &Arc<Mutex<HashMap<String, Lane>>>, stop: &AtomicBool) {
        let _apartment = Apartment::enter();
        let mut failing = false;

        clips_engine::recorder::join_multimedia_task("Audio");

        while !stop.load(Ordering::Relaxed) {
            let device = lock(chosen).clone();
            let device = device.as_deref();
            let played = unsafe { open(device, eRender) }.and_then(|(client, id)| {
                if std::mem::replace(&mut failing, false) {
                    tracing::info!("som: a saída voltou");
                }

                let played = unsafe { play(&client, id.as_deref(), (device, switch), mix, stop) };
                let _ = unsafe { client.Stop() };

                played
            });

            if let Err(failure) = played {
                if !std::mem::replace(&mut failing, true) {
                    tracing::warn!(failure = %format!("{failure:#}"), "som: a saída caiu, reabrindo");
                }

                std::thread::sleep(REOPEN_AFTER);
            }
        }
    }

    unsafe fn play(client: &IAudioClient, id: Option<&str>, (device, switch): (Option<&str>, &AtomicBool), mix: &Arc<Mutex<HashMap<String, Lane>>>, stop: &AtomicBool) -> Result<()> {
        unsafe {
            let frames = client.GetBufferSize().context("o Windows não disse o tamanho do buffer")?;
            let render: IAudioRenderClient = client.GetService().context("sem cliente de saída")?;
            let mut block = Vec::new();
            let mut ticks = 0_u32;

            client.Start().context("a saída não começou")?;
            tracing::info!(device = device.unwrap_or("padrão"), "som: tocando");

            while !stop.load(Ordering::Relaxed) {
                let free = frames.saturating_sub(client.GetCurrentPadding().context("o aparelho sumiu")?);

                if free > 0 {
                    block.resize(free as usize * CHANNELS, 0.0);
                    mix_into(&mut lock(mix), &mut block);

                    let target = render.GetBuffer(free).context("a placa não deu o buffer")?;

                    std::ptr::copy_nonoverlapping(block.as_ptr(), target.cast::<f32>(), block.len());
                    render.ReleaseBuffer(free, 0).context("a placa não aceitou o buffer")?;
                }

                ticks += 1;

                if device.is_none() && ticks.is_multiple_of(DEFAULT_CHECK_TICKS) && default_id(eRender).as_deref() != id {
                    return Err(anyhow!("o aparelho padrão do Windows mudou"));
                }

                if switch.swap(false, Ordering::Relaxed) {
                    return Err(anyhow!("a pessoa escolheu outra saída"));
                }

                std::thread::sleep(TICK);
            }
        }

        Ok(())
    }

    /// Grava até mandarem parar, reabrindo o microfone que caiu (ver `render`). Só a primeira
    /// abertura responde em `opened`: microfone que não abriu é erro que a pessoa precisa ver;
    /// o que cai depois volta sozinho, e enquanto isso a sala só não ouve.
    pub fn capture(
        device: Option<&str>,
        mut sink: impl FnMut(&[f32]),
        stop: &AtomicBool,
        opened: &SyncSender<std::result::Result<(), String>>,
    ) {
        let _apartment = Apartment::enter();
        let mut first = true;
        let mut failing = false;

        clips_engine::recorder::join_multimedia_task("Audio");

        while !stop.load(Ordering::Relaxed) {
            let recorded = match unsafe { open(device, eCapture) } {
                Ok((client, id)) => {
                    if std::mem::replace(&mut first, false) {
                        let _ = opened.send(Ok(()));
                    } else if std::mem::replace(&mut failing, false) {
                        tracing::info!("microfone: voltou");
                    }

                    let recorded = unsafe { record(&client, id.as_deref(), device, &mut sink, stop) };
                    let _ = unsafe { client.Stop() };

                    recorded
                }
                Err(failure) if first => {
                    let _ = opened.send(Err(format!("{failure:#}")));

                    return;
                }
                Err(failure) => Err(failure),
            };

            if let Err(failure) = recorded {
                if !std::mem::replace(&mut failing, true) {
                    tracing::warn!(failure = %format!("{failure:#}"), "microfone: caiu, reabrindo");
                }

                std::thread::sleep(REOPEN_AFTER);
            }
        }
    }

    unsafe fn record(client: &IAudioClient, id: Option<&str>, device: Option<&str>, sink: &mut impl FnMut(&[f32]), stop: &AtomicBool) -> Result<()> {
        unsafe {
            let capture: IAudioCaptureClient = client.GetService().context("sem cliente de captura")?;
            let mut block = Vec::new();
            let mut ticks = 0_u32;

            client.Start().context("o microfone não começou")?;
            tracing::info!(device = device.unwrap_or("padrão"), "microfone: aberto");

            while !stop.load(Ordering::Relaxed) {
                while capture.GetNextPacketSize().context("o microfone sumiu")? > 0 {
                    let mut data = std::ptr::null_mut();
                    let (mut frames, mut flags) = (0_u32, 0_u32);

                    capture
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                        .context("o microfone não deu o buffer")?;

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

                ticks += 1;

                if device.is_none() && ticks.is_multiple_of(DEFAULT_CHECK_TICKS) && default_id(eCapture).as_deref() != id {
                    return Err(anyhow!("o microfone padrão do Windows mudou"));
                }

                std::thread::sleep(TICK);
            }
        }

        Ok(())
    }

    /// O id do aparelho padrão de um sentido agora.
    fn default_id(flow: EDataFlow) -> Option<String> {
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;

            endpoint_id(&enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok()?)
        }
    }

    unsafe fn endpoint_id(endpoint: &IMMDevice) -> Option<String> {
        unsafe {
            let id = endpoint.GetId().ok()?;
            let text = id.to_string().ok();

            CoTaskMemFree(Some(id.0.cast_const().cast()));

            text
        }
    }

    /// O cliente aberto e o id do aparelho, para saber depois se o padrão mudou.
    unsafe fn open(device: Option<&str>, flow: EDataFlow) -> Result<(IAudioClient, Option<String>)> {
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

            Ok((client, endpoint_id(&endpoint)))
        }
    }
}

/// PulseAudio, e o `pipewire-pulse`, que fala a mesma língua. O som sai por um `pacat` só,
/// alimentado pela mistura das `Lane` — a folga anti-estalo e o volume por pessoa são os
/// mesmos do Windows. O microfone entra pela captura do `shared/capture`: o `pulsesrc` de lá
/// já traz o `webrtcdsp` de ruído e ganho, e um segundo leitor do mesmo microfone seria
/// escrever a regra duas vezes.
#[cfg(target_os = "linux")]
mod platform {
    use std::collections::HashMap;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::SyncSender;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use anyhow::{Context, Result, anyhow};
    use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer};

    use super::{CHANNELS, Lane, PER_MILLISECOND, SAMPLE_RATE, TICK, lock, mix_into};

    /// Quanto o `pacat` guarda antes de tocar: a mesma régua do buffer de 50 ms do Windows.
    const LATENCY_MS: u32 = 40;

    /// O cano até o `pacat` fica com uma página só (4 KiB, uns 10 ms de som). É ele que dá o
    /// ritmo — o `write_all` só volta quando a placa consumiu — e o cano padrão de 64 KiB
    /// poria 170 ms a mais em cada fala.
    const PIPE_BYTES: libc::c_int = 4096;

    /// Quanto esperar antes de reabrir a saída que caiu.
    const REOPEN_AFTER: Duration = Duration::from_secs(1);

    /// Toca até mandarem parar. A saída que some (o `pacat` morre) é reaberta de segundo em
    /// segundo, e trocar de saída (`switch`) é fechar o `pacat` e abrir outro no aparelho novo,
    /// sem perder o que esperava para tocar — a mistura fica fora dele.
    pub fn render(chosen: &Mutex<Option<String>>, switch: &AtomicBool, mix: &Arc<Mutex<HashMap<String, Lane>>>, stop: &AtomicBool) {
        let mut failing = false;

        while !stop.load(Ordering::Relaxed) {
            let device = lock(chosen).clone();

            match play(device.as_deref(), switch, mix, stop) {
                Ok(()) => {
                    if std::mem::replace(&mut failing, false) {
                        tracing::info!("som: a saída voltou");
                    }
                }
                Err(failure) => {
                    if !std::mem::replace(&mut failing, true) {
                        tracing::warn!(failure = %format!("{failure:#}"), "som: a saída caiu, reabrindo");
                    }

                    std::thread::sleep(REOPEN_AFTER);
                }
            }
        }
    }

    /// Um `pacat` na saída escolhida, até mandarem parar ou trocar (`Ok`); `Err` quando ele
    /// não abriu ou fechou a entrada no meio.
    fn play(device: Option<&str>, switch: &AtomicBool, mix: &Arc<Mutex<HashMap<String, Lane>>>, stop: &AtomicBool) -> Result<()> {
        let mut command = Command::new("pacat");

        command.args([
            "--playback",
            "--raw",
            "--format=float32le",
            &format!("--rate={SAMPLE_RATE}"),
            &format!("--channels={CHANNELS}"),
            &format!("--latency-msec={LATENCY_MS}"),
            "--client-name=Unkvoid",
            "--stream-name=Voz",
        ]);

        if let Some(device) = device {
            command.arg(format!("--device={device}"));
        }

        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("o pacat não abriu; instale pulseaudio-utils")?;
        let mut stdin = child.stdin.take().context("o pacat abriu sem entrada")?;

        // SAFETY: `fcntl` num descritor que este processo acabou de abrir e ainda segura.
        unsafe { libc::fcntl(stdin.as_raw_fd(), libc::F_SETPIPE_SZ, PIPE_BYTES) };

        let mut out = vec![0.0_f32; TICK.as_millis() as usize * PER_MILLISECOND];
        let mut bytes = Vec::with_capacity(out.len() * 4);

        tracing::info!(device = device.unwrap_or("padrão"), "som: tocando pelo pacat");

        while !stop.load(Ordering::Relaxed) {
            mix_into(&mut lock(mix), &mut out);
            bytes.clear();
            bytes.extend(out.iter().flat_map(|sample| sample.to_le_bytes()));

            if stdin.write_all(&bytes).is_err() {
                let _ = child.wait();

                return Err(anyhow!("o pacat fechou a entrada: a saída de áudio sumiu?"));
            }

            if switch.swap(false, Ordering::Relaxed) {
                tracing::info!("som: a pessoa escolheu outra saída");

                break;
            }
        }

        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();

        Ok(())
    }

    pub fn capture(
        device: Option<&str>,
        sink: impl FnMut(&[f32]) + Send + 'static,
        stop: &AtomicBool,
        opened: &SyncSender<std::result::Result<(), String>>,
    ) {
        if let Some(device) = device
            && !crate::devices::use_microphone(device)
        {
            tracing::warn!(device, "microfone: o pactl não aceitou o aparelho; fica o padrão");
        }

        let sink = Mutex::new(sink);
        let config = CaptureConfig {
            source: CaptureSource::Microphone,
            capture_audio: true,
            ..CaptureConfig::default()
        };
        let started = PlatformCapturer::start(&config, move |event| {
            if let CaptureEvent::Audio(chunk) = event {
                let mut sink = lock(&sink);

                sink(&chunk.samples);
            }
        });

        let mut capturer = match started {
            Ok(capturer) => {
                let _ = opened.send(Ok(()));

                capturer
            }
            Err(failure) => {
                let _ = opened.send(Err(failure.to_string()));

                return;
            }
        };

        tracing::info!(device = device.unwrap_or("padrão"), "microfone: aberto pelo pulsesrc");

        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(TICK);
        }

        let _ = capturer.stop();
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod platform {
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::SyncSender;
    use std::sync::{Arc, Mutex};

    use super::Lane;

    pub fn render(_: &Mutex<Option<String>>, _: &AtomicBool, _: &Arc<Mutex<HashMap<String, Lane>>>, _: &AtomicBool) {
        tracing::warn!("som: sem WASAPI fora do Windows");
    }

    pub fn capture(
        _: Option<&str>,
        _: impl FnMut(&[f32]),
        _: &AtomicBool,
        opened: &SyncSender<std::result::Result<(), String>>,
    ) {
        let _ = opened.send(Err("sem microfone neste sistema".into()));
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
            ..Lane::default()
        }
    }

    /// A imagem passou a esperar 100 ms a mais: o som da tela atrasa os mesmos 100 ms, com
    /// silêncio na frente do que já esperava. Quando a espera desce, o excesso sai em emendas.
    #[test]
    fn the_sound_follows_the_wait_of_the_picture_it_goes_with() {
        let mut lane = lane(CUSHION, true);

        lane.hold(100 * PER_MILLISECOND);

        assert_eq!(lane.samples.len(), CUSHION + 100 * PER_MILLISECOND);
        assert!(lane.samples.range(SPLICE..100 * PER_MILLISECOND).all(|&sample| sample == 0.0), "o silêncio entra na frente");
        assert_eq!(lane.samples[100 * PER_MILLISECOND + SPLICE], 0.25, "o som que esperava vem depois do silêncio");

        lane.push(&[0.25; 4]);

        assert!(!lane.trimming && !lane.shrinking, "dentro da espera nova nada é cortado");

        lane.hold(20 * PER_MILLISECOND);
        lane.push(&[0.25; 4]);

        let cut = CUSHION + 100 * PER_MILLISECOND + 8 - lane.samples.len();

        assert!(lane.shrinking);
        assert!(cut > 0 && cut <= HOLD_STEP, "um passo por bloco, e não de uma vez: {cut}");

        while lane.shrinking {
            lane.push(&[]);
        }

        assert_eq!(lane.samples.len(), CUSHION + 20 * PER_MILLISECOND);
    }

    /// Um lá contínuo, estéreo, a partir da amostra `from` (contando os dois canais).
    fn tone(from: usize, length: usize) -> Vec<f32> {
        (from..from + length).map(|index| ((index / CHANNELS) as f32 * 2.0 * std::f32::consts::PI * 440.0 / SAMPLE_RATE as f32).sin() * 0.5).collect()
    }

    /// O maior salto entre duas amostras seguidas do mesmo canal.
    fn largest_step(samples: &[f32]) -> f32 {
        samples.windows(CHANNELS + 1).map(|pair| (pair[CHANNELS] - pair[0]).abs()).fold(0.0, f32::max)
    }

    /// O `Playout` desce 50 ms por segundo: 1 ms a cada bloco de 20 ms. Seguir cada degrau
    /// cortava 5 ms a cada ~100 ms — cinquenta cortes para ir de 300 a 50 ms.
    #[test]
    fn a_wait_that_falls_a_millisecond_per_block_is_not_cut_every_block() {
        let block = 20 * PER_MILLISECOND;
        let mut lane = Lane { samples: tone(0, CUSHION + 300 * PER_MILLISECOND).into(), primed: true, volume: 1.0, hold: 300 * PER_MILLISECOND, ..Lane::default() };
        let mut written = lane.samples.len();
        let mut cuts = 0;

        for index in 0..250 {
            lane.hold((300 - index.min(250)) * PER_MILLISECOND);

            let before = lane.samples.len();

            lane.push(&tone(written, block));
            written += block;
            cuts += usize::from(lane.samples.len() < before + block);
            lane.samples.drain(..block);
        }

        for _ in 0..20 {
            lane.push(&tone(written, block));
            written += block;
            lane.samples.drain(..block);
        }

        assert!(cuts <= 15, "{cuts} cortes para descer 250 ms");
        assert!(lane.hold <= 50 * PER_MILLISECOND + SYNC_SLACK, "a espera não desceu: {} ms", lane.hold / PER_MILLISECOND);
        assert_eq!(lane.samples.len(), CUSHION + lane.hold - block, "o excesso saiu todo");
    }

    /// A espera da imagem treme alguns milissegundos a cada quadro: o som não se mexe.
    #[test]
    fn a_wait_that_trembles_inside_the_slack_leaves_the_sound_alone() {
        let mut lane = Lane { samples: tone(0, CUSHION + 100 * PER_MILLISECOND).into(), primed: true, volume: 1.0, hold: 100 * PER_MILLISECOND, ..Lane::default() };
        let length = lane.samples.len();

        for wanted in [70, 130, 95, 135, 65, 100, 120, 80] {
            lane.hold(wanted * PER_MILLISECOND);
            lane.push(&[]);
        }

        assert_eq!(lane.samples.len(), length, "nem silêncio nem corte");
        assert_eq!(lane.hold, 100 * PER_MILLISECOND);
    }

    /// Cortar e pôr silêncio emendam: nenhum salto maior que o do próprio som, que é o estalo.
    #[test]
    fn a_cut_and_a_silence_are_spliced_without_a_click() {
        let natural = largest_step(&tone(0, SAMPLE_RATE as usize * CHANNELS / 100));
        let mut lane = Lane { samples: tone(0, CUSHION + 200 * PER_MILLISECOND).into(), primed: true, volume: 1.0, hold: 200 * PER_MILLISECOND, ..Lane::default() };
        let mut heard = tone(0, 1_000).split_off(1_000 - CHANNELS);

        lane.samples.drain(..1_000 - CHANNELS);
        lane.cut(HOLD_STEP + 6);
        heard.extend(lane.samples.range(..SPLICE * 4));

        assert!(largest_step(&heard) <= natural * 1.5, "a emenda do corte saltou {} (o som salta {natural})", largest_step(&heard));

        let mut heard: Vec<f32> = heard.split_off(heard.len() - CHANNELS);

        lane.samples.drain(..SPLICE * 4 - CHANNELS);
        lane.delay(60 * PER_MILLISECOND);
        heard.extend(lane.samples.range(..70 * PER_MILLISECOND));

        assert!(largest_step(&heard) <= natural * 1.5, "a emenda do silêncio saltou {} (o som salta {natural})", largest_step(&heard));
    }

    #[test]
    fn the_wait_never_passes_half_a_second_nor_splits_a_stereo_pair() {
        let mut lane = lane(0, false);

        lane.hold(10 * MOST_HOLD + 1);

        assert_eq!(lane.hold, MOST_HOLD);
        assert_eq!(lane.samples.len() % CHANNELS, 0);
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

        let loud = Lane { samples: std::iter::repeat_n(0.9, CUSHION).collect(), primed: true, volume: 1.0, ..Lane::default() };
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

    /// Passado o teto, o acumulado volta à folga aos poucos — um passo por bloco —, e não de uma vez.
    #[test]
    fn a_backlog_longer_than_the_ceiling_is_cut_back_to_the_cushion() {
        let speaker = Speaker {
            mix: Mix::default(),
            stop: Arc::new(AtomicBool::new(true)),
            thread: None,
            device: Arc::default(),
            switch: Arc::default(),
        };

        speaker.play("ada", &vec![0.1; LONGEST + 10]);

        assert_eq!(lock(&speaker.mix)["ada"].samples.len(), LONGEST + 10 - TRIM_STEP, "só um passo por vez");

        while lock(&speaker.mix)["ada"].trimming {
            speaker.play("ada", &[]);
        }

        assert_eq!(lock(&speaker.mix)["ada"].samples.len(), CUSHION);
    }
}
