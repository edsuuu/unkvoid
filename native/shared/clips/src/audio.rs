//! Som do replay: o do sistema e o do microfone, cada um na sua thread, encaixados pelo
//! relógio e somados em blocos de 10 ms, que viram AAC.
//!
//! O do sistema é o laço por processo do unkvoid (`capture/src/windows_audio.rs`): tudo o
//! que toca na máquina menos a árvore deste app, para o player da galeria não entrar nos
//! clipes.
//!
//! Cada pacote chega com o tempo do QPC, o mesmo relógio dos quadros de vídeo, e a mistura
//! põe cada um no lugar pelo tempo e não pela ordem de chegada. O relógio de uma placa de som
//! escorrega alguns milissegundos por hora em relação ao do sistema; contando amostras, depois
//! de um dia gravando o som do clipe estaria fora da boca de quem fala.

use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, anyhow};
use nnnoiseless::DenoiseState;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX, eCapture, eConsole,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcessId, INFINITE, SetEvent, WaitForSingleObject,
};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{Interface, PCWSTR, Ref, implement};

use crate::aac::{AacEncoder, CHANNELS, SAMPLE_RATE};
use crate::clock;
use crate::replay::{Record, RecordSink, Track};

/// Um bloco da mistura: 10 ms, que é exatamente um quadro do RNNoise a 48 kHz.
const BLOCK_FRAMES: usize = nnnoiseless::FRAME_SIZE;
const BLOCK_NS: u64 = BLOCK_FRAMES as u64 * 1_000_000_000 / SAMPLE_RATE as u64;

/// Quanto a mistura anda atrás do relógio, esperando os pacotes chegarem. O Windows entrega
/// o som em pacotes de 10 a 20 ms; com menos folga que isso o fim de cada bloco sairia mudo.
const MIX_DELAY_NS: u64 = 100_000_000;

/// Diferença entre o tempo que o pacote diz e o que a fila esperava que se tolera sem mexer.
/// Acima disso é buraco (entra silêncio) ou placa adiantada (a fila é reancorada).
const DRIFT_TOLERANCE_NS: u64 = 20_000_000;

/// Teto da fila de cada origem. Mistura parada por mais que isso descarta o começo, em vez
/// de acumular memória.
const MAX_QUEUED_FRAMES: usize = SAMPLE_RATE as usize;

/// Buffer pedido ao Windows por cliente, em 100 ns: 20 ms.
const CLIENT_BUFFER_HNS: i64 = 200_000;

/// `WAVE_FORMAT_IEEE_FLOAT`: é o que sai do mixer do Windows sem conversão nenhuma.
const FORMAT_FLOAT: u16 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSettings {
    pub system: bool,
    pub microphone: Option<MicrophoneSettings>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MicrophoneSettings {
    /// O endpoint do WASAPI; `None` é o microfone padrão do Windows. O NVIDIA Broadcast
    /// aparece aqui como um microfone qualquer.
    pub device: Option<String>,
    pub noise_suppression: bool,
}

pub struct AudioCapture {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl AudioCapture {
    /// Liga as origens pedidas e a mistura. `None` quando não há som nenhum para gravar.
    pub fn start(settings: &AudioSettings, sink: RecordSink) -> anyhow::Result<Option<Self>> {
        if !settings.system && settings.microphone.is_none() {
            return Ok(None);
        }

        // Montado antes das threads: se o microfone recusar depois de o som do sistema já ter
        // aberto, o `Drop` daqui desliga o que subiu em vez de deixá-lo gravando sem dono.
        let mut capture = Self { stop: Arc::new(AtomicBool::new(false)), threads: Vec::new() };
        let system = settings.system.then(|| Arc::new(Mutex::new(Lane::default())));
        let microphone = settings.microphone.clone().map(|microphone| (microphone, Arc::new(Mutex::new(Lane::default()))));

        if let Some(lane) = system.clone() {
            capture.threads.push(spawn_source("unkvoid-clips-system-audio", capture.stop.clone(), lane, || unsafe {
                open_system_loopback()
            })?);
        }

        if let Some((settings, lane)) = &microphone {
            let device = settings.device.clone();

            capture.threads.push(spawn_source("unkvoid-clips-microphone", capture.stop.clone(), lane.clone(), move || unsafe {
                open_microphone(device.as_deref())
            })?);
        }

        let mixer_stop = capture.stop.clone();
        let mixer_microphone = microphone.map(|(settings, lane)| (lane, settings.noise_suppression));

        capture.threads.push(std::thread::Builder::new().name("unkvoid-clips-mixer".into()).spawn(move || {
            if let Err(error) = mix(system, mixer_microphone, &sink, &mixer_stop) {
                tracing::error!(error = %error, "áudio: a mistura parou, o replay segue sem som");
            }
        })?);

        Ok(Some(capture))
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// O som de uma origem esperando a vez na mistura, com o tempo da primeira amostra da fila.
#[derive(Default)]
struct Lane {
    samples: VecDeque<f32>,
    head_ns: u64,
}

impl Lane {
    fn frames(&self) -> usize {
        self.samples.len() / CHANNELS as usize
    }

    fn push(&mut self, timestamp_ns: u64, samples: &[f32]) {
        if self.samples.is_empty() {
            self.head_ns = timestamp_ns;
        } else {
            let expected_ns = self.head_ns + frames_to_ns(self.frames());

            if timestamp_ns > expected_ns + DRIFT_TOLERANCE_NS {
                let missing = ns_to_frames(timestamp_ns - expected_ns) * CHANNELS as usize;

                self.samples.extend(std::iter::repeat_n(0.0, missing));
            } else if timestamp_ns + DRIFT_TOLERANCE_NS < expected_ns {
                self.head_ns = timestamp_ns.saturating_sub(frames_to_ns(self.frames()));
            }
        }

        self.samples.extend(samples);

        let excess = self.frames().saturating_sub(MAX_QUEUED_FRAMES);

        if excess > 0 {
            self.samples.drain(..excess * CHANNELS as usize);
            self.head_ns += frames_to_ns(excess);
        }
    }

    /// Soma em `mixed` o trecho da fila que cai no bloco que começa em `start_ns`. O que
    /// ficou para trás do bloco é descartado; o que ainda está no futuro espera.
    fn add_into(&mut self, start_ns: u64, mixed: &mut [f32]) {
        if self.samples.is_empty() {
            return;
        }

        if self.head_ns < start_ns {
            let late = ns_to_frames(start_ns - self.head_ns).min(self.frames());

            self.samples.drain(..late * CHANNELS as usize);
            self.head_ns += frames_to_ns(late);
        }

        let block_frames = mixed.len() / CHANNELS as usize;
        let offset = if self.head_ns > start_ns { ns_to_frames(self.head_ns - start_ns) } else { 0 };

        if offset >= block_frames {
            return;
        }

        let available = self.frames().min(block_frames - offset);

        for (slot, sample) in mixed[offset * CHANNELS as usize..]
            .iter_mut()
            .zip(self.samples.drain(..available * CHANNELS as usize))
        {
            *slot += sample;
        }

        self.head_ns += frames_to_ns(available);
    }
}

fn frames_to_ns(frames: usize) -> u64 {
    frames as u64 * 1_000_000_000 / u64::from(SAMPLE_RATE)
}

fn ns_to_frames(nanoseconds: u64) -> usize {
    (nanoseconds * u64::from(SAMPLE_RATE) / 1_000_000_000) as usize
}

fn lock(lane: &Mutex<Lane>) -> MutexGuard<'_, Lane> {
    lane.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Uma thread por origem: abre o cliente do WASAPI nela (o COM é por thread) e bombeia os
/// pacotes para a fila até mandarem parar. Só devolve depois de o Windows aceitar a
/// abertura: origem que não abriu é erro que a pessoa precisa ver, não silêncio no clipe.
fn spawn_source(
    name: &str,
    stop: Arc<AtomicBool>,
    lane: Arc<Mutex<Lane>>,
    open: impl FnOnce() -> anyhow::Result<IAudioClient> + Send + 'static,
) -> anyhow::Result<JoinHandle<()>> {
    let (opened, answer) = sync_channel(1);
    let thread = std::thread::Builder::new().name(name.into()).spawn(move || {
        let _apartment = Apartment::enter();

        crate::recorder::join_multimedia_task("Audio");
        let started = open().and_then(|client| unsafe { Source::start(client) });

        match started {
            Ok(source) => {
                let _ = opened.send(Ok(()));

                source.pump(&lane, &stop);
            }
            Err(error) => {
                let _ = opened.send(Err(format!("{error:#}")));
            }
        }
    })?;

    match answer.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(thread),
        Ok(Err(error)) => Err(anyhow!(error).context(name.to_owned())),
        Err(_) => Err(anyhow!("o Windows não respondeu ao abrir o som").context(name.to_owned())),
    }
}

/// Um cliente do WASAPI já gravando, com o evento que o Windows sinaliza a cada pacote.
struct Source {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    ready: HANDLE,
}

impl Source {
    unsafe fn start(client: IAudioClient) -> anyhow::Result<Self> {
        unsafe {
            let ready = CreateEventW(None, false, false, None)?;
            let source = Self { capture: client.GetService()?, client, ready };

            source.client.SetEventHandle(ready)?;
            source.client.Start()?;

            Ok(source)
        }
    }

    fn pump(&self, lane: &Mutex<Lane>, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            unsafe {
                // Com prazo, para enxergar o pedido de parada mesmo quando nada toca: o laço
                // do sistema fica sem pacote nenhum enquanto a máquina está em silêncio.
                WaitForSingleObject(self.ready, 100);
                drain(&self.capture, lane);
            }
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.ready);
        }
    }
}

/// Tira do Windows tudo o que já está pronto, pacote a pacote, com o tempo de cada um.
unsafe fn drain(capture: &IAudioCaptureClient, lane: &Mutex<Lane>) {
    unsafe {
        loop {
            let mut data = std::ptr::null_mut();
            let mut frames = 0_u32;
            let mut flags = 0_u32;
            let mut position_hns = 0_u64;

            if capture.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut position_hns)).is_err()
                || frames == 0
            {
                break;
            }

            let count = frames as usize * CHANNELS as usize;
            let timestamp_ns = if flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 != 0 || position_hns == 0 {
                clock::now_ns().saturating_sub(frames_to_ns(frames as usize))
            } else {
                position_hns * 100
            };

            if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                lock(lane).push(timestamp_ns, &vec![0.0; count]);
            } else {
                lock(lane).push(timestamp_ns, std::slice::from_raw_parts(data.cast::<f32>(), count));
            }

            let _ = capture.ReleaseBuffer(frames);
        }
    }
}

/// Soma as origens por um relógio só, limpa o microfone e codifica.
fn mix(
    system: Option<Arc<Mutex<Lane>>>,
    microphone: Option<(Arc<Mutex<Lane>>, bool)>,
    sink: &RecordSink,
    stop: &AtomicBool,
) -> anyhow::Result<()> {
    let _apartment = Apartment::enter();

    crate::recorder::join_multimedia_task("Audio");

    let mut encoder = AacEncoder::new()?;
    let mut denoiser = microphone.as_ref().is_some_and(|(_, suppress)| *suppress).then(DenoiseState::new);
    let mut mixed = vec![0.0_f32; BLOCK_FRAMES * CHANNELS as usize];
    let mut voice = mixed.clone();
    let mut mono = vec![0.0_f32; BLOCK_FRAMES];
    let mut cleaned = mono.clone();
    let mut pcm = vec![0_i16; mixed.len()];
    let mut block_ns = clock::now_ns().saturating_sub(MIX_DELAY_NS);

    while !stop.load(Ordering::Relaxed) {
        let now_ns = clock::now_ns();

        // Máquina que dormiu: o relógio pulou horas. Refazer cada bloco perdido seria
        // despejar horas de silêncio no buffer de uma vez; o som simplesmente recomeça.
        if now_ns > block_ns + MIX_DELAY_NS + 1_000_000_000 {
            block_ns = now_ns - MIX_DELAY_NS;
        }

        while block_ns + BLOCK_NS + MIX_DELAY_NS <= now_ns {
            mixed.fill(0.0);

            if let Some(lane) = &system {
                lock(lane).add_into(block_ns, &mut mixed);
            }

            if let Some((lane, _)) = &microphone {
                voice.fill(0.0);
                lock(lane).add_into(block_ns, &mut voice);

                match denoiser.as_mut() {
                    Some(denoiser) => {
                        // O RNNoise trabalha em mono e na escala de 16 bits.
                        for (target, [left, right]) in mono.iter_mut().zip(voice.as_chunks::<2>().0) {
                            *target = (left + right) * 0.5 * 32_767.0;
                        }

                        denoiser.process_frame(&mut cleaned, &mono);

                        for (pair, sample) in mixed.as_chunks_mut::<2>().0.iter_mut().zip(&cleaned) {
                            pair[0] += sample / 32_767.0;
                            pair[1] += sample / 32_767.0;
                        }
                    }
                    None => {
                        for (slot, sample) in mixed.iter_mut().zip(&voice) {
                            *slot += sample;
                        }
                    }
                }
            }

            for (target, sample) in pcm.iter_mut().zip(&mixed) {
                *target = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
            }

            for frame in encoder.encode(&pcm, block_ns)? {
                sink.push(Record { track: Track::Audio, keyframe: false, timestamp_ns: frame.timestamp_ns, data: frame.data });
            }

            block_ns += BLOCK_NS;
        }

        std::thread::sleep(Duration::from_millis(5));
    }

    Ok(())
}

/// O COM desta thread, desligado na saída dela mesmo que a abertura falhe no meio. Só desliga
/// o que ligou: chamado da thread da interface, que já está num apartamento de outro tipo, o
/// `CoInitializeEx` recusa, e desligar ali derrubaria o COM do Slint.
struct Apartment {
    entered: bool,
}

impl Apartment {
    fn enter() -> Self {
        Self { entered: unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok() }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.entered {
            unsafe { CoUninitialize() };
        }
    }
}

fn float_format() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: FORMAT_FLOAT,
        nChannels: CHANNELS as u16,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * CHANNELS * 4,
        nBlockAlign: CHANNELS as u16 * 4,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}

/// O microfone pedido, ou o padrão do Windows, já inicializado em float 48 kHz estéreo.
/// `AUTOCONVERTPCM` deixa o Windows reamostrar microfone de 44,1 kHz ou mono.
unsafe fn open_microphone(device: Option<&str>) -> anyhow::Result<IAudioClient> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).context("o áudio do Windows não abriu")?;
        let endpoint: IMMDevice = match device {
            Some(id) => {
                let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();

                enumerator.GetDevice(PCWSTR(wide.as_ptr())).context("o microfone escolhido não existe mais")?
            }
            None => enumerator.GetDefaultAudioEndpoint(eCapture, eConsole).context("não há microfone padrão")?,
        };
        let client: IAudioClient = endpoint.Activate(CLSCTX_ALL, None)?;

        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            CLIENT_BUFFER_HNS,
            0,
            &float_format(),
            None,
        )?;

        Ok(client)
    }
}

/// Tudo o que toca na máquina menos a árvore deste processo.
unsafe fn open_system_loopback() -> anyhow::Result<IAudioClient> {
    unsafe {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: GetCurrentProcessId(),
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        let variant = blob_of(&mut params);
        let done = CreateEventW(None, false, false, None)?;
        let handler: IActivateAudioInterfaceCompletionHandler = Done(done).into();
        let operation =
            ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&*variant), &handler);

        if operation.is_ok() {
            WaitForSingleObject(done, INFINITE);
        }

        let _ = CloseHandle(done);

        let operation = operation?;
        let mut status = windows::core::HRESULT(0);
        let mut unknown = None;

        operation.GetActivateResult(&mut status, &mut unknown)?;
        status.ok()?;

        let client: IAudioClient = unknown.context("o Windows não devolveu o cliente de áudio")?.cast()?;

        // O dispositivo de laço por processo não tem formato de mixer para perguntar: quem
        // grava escolhe, e o `AUTOCONVERTPCM` autoriza o Windows a reamostrar quando a saída
        // de verdade está em outra taxa. Sem ele, uma placa em 44,1 kHz fazia o `Initialize`
        // recusar e o clipe sair mudo (medido no unkvoid).
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            CLIENT_BUFFER_HNS,
            0,
            &float_format(),
            None,
        )?;

        Ok(client)
    }
}

/// Espera a ativação assíncrona: `ActivateAudioInterfaceAsync` devolve na hora e avisa
/// depois, e a API não tem versão síncrona.
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Done(HANDLE);

impl IActivateAudioInterfaceCompletionHandler_Impl for Done_Impl {
    fn ActivateCompleted(&self, _operation: Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        unsafe { SetEvent(self.0) }
    }
}

/// A configuração no `PROPVARIANT` de blob que a API recebe, montada campo a campo.
///
/// `ManuallyDrop` porque o `PROPVARIANT` se julga dono do que carrega: ao sair de escopo ele
/// devolveria `pBlobData` — a pilha de quem chamou — ao alocador do COM. No unkvoid isso
/// corrompia o heap e matava o processo sem HRESULT nem pânico.
///
/// # Segurança
///
/// O `PROPVARIANT` devolvido aponta para `params` e só vale enquanto ele existir.
unsafe fn blob_of(params: &mut AUDIOCLIENT_ACTIVATION_PARAMS) -> ManuallyDrop<PROPVARIANT> {
    let mut variant = ManuallyDrop::new(PROPVARIANT::default());

    unsafe {
        let fields = &mut variant.Anonymous.Anonymous;

        fields.vt = VT_BLOB;
        fields.Anonymous.blob.cbSize = size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;
        fields.Anonymous.blob.pBlobData = std::ptr::from_mut(params).cast::<u8>();
    }

    variant
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microphone {
    /// O identificador do endpoint. Nunca aparece na tela.
    pub id: String,
    /// O nome que a pessoa reconhece ("Microfone (NVIDIA Broadcast)").
    pub label: String,
}

/// Os microfones ligados agora, como o Windows os lista. Copiado de
/// `unkvoid/native/apps/windows/src/devices.rs`. Sem WASAPI a lista sai vazia: é uma máquina
/// onde a escolha não existe, não uma falha.
pub fn microphones() -> Vec<Microphone> {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE;
    use windows::Win32::System::Com::STGM_READ;
    use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
    use windows::Win32::System::Variant::VT_LPWSTR;

    let _apartment = Apartment::enter();

    unsafe {
        let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            return Vec::new();
        };
        let Ok(endpoints) = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) else {
            return Vec::new();
        };
        let mut found = Vec::new();

        for index in 0..endpoints.GetCount().unwrap_or(0) {
            let Ok(endpoint) = endpoints.Item(index) else { continue };
            let Some(id) = endpoint.GetId().ok().and_then(|id| id.to_string().ok()) else { continue };
            let Ok(store) = endpoint.OpenPropertyStore(STGM_READ) else { continue };
            let Ok(mut value) = store.GetValue(&PKEY_Device_FriendlyName) else { continue };
            let fields = &value.Anonymous.Anonymous;
            let label = if fields.vt == VT_LPWSTR { fields.Anonymous.pwszVal.to_string().ok() } else { None };

            // O `PROPVARIANT` volta alocado pelo COM: sem liberar, cada abertura da lista
            // vazaria uma string por aparelho.
            let _ = PropVariantClear(&mut value);

            if let Some(label) = label {
                found.push(Microphone { id, label });
            }
        }

        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn block(start_ns: u64, lane: &mut Lane) -> Vec<f32> {
        let mut mixed = vec![0.0; BLOCK_FRAMES * 2];

        lane.add_into(start_ns, &mut mixed);

        mixed
    }

    #[test]
    fn a_packet_lands_where_its_clock_says_not_where_the_queue_ends() {
        let mut lane = Lane::default();

        // 10 ms de som que começa 5 ms depois do bloco: a primeira metade do bloco é silêncio.
        lane.push(1_005 * MS, &[1.0; BLOCK_FRAMES * 2]);

        let mixed = block(1_000 * MS, &mut lane);

        assert_eq!(mixed[0], 0.0);
        assert_eq!(mixed[BLOCK_FRAMES / 2 * 2 - 2], 0.0);
        assert_eq!(mixed[BLOCK_FRAMES / 2 * 2 + 2], 1.0);
        assert_eq!(lane.frames(), BLOCK_FRAMES / 2);
    }

    #[test]
    fn a_gap_becomes_silence_and_a_late_queue_is_trimmed() {
        let mut lane = Lane::default();

        lane.push(0, &[1.0; BLOCK_FRAMES * 2]);
        // O próximo pacote chega 50 ms depois do fim do primeiro: buraco de 50 ms.
        lane.push(60 * MS, &[2.0; BLOCK_FRAMES * 2]);

        assert_eq!(lane.frames(), BLOCK_FRAMES * 2 + ns_to_frames(50 * MS));

        // A mistura já está em 60 ms: tudo antes foi descartado sem tocar.
        let mixed = block(60 * MS, &mut lane);

        assert!(mixed.iter().all(|&sample| sample == 2.0));
    }
}
