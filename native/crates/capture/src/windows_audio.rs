//! Áudio do sistema no Windows.
//!
//! O Windows não tem um equivalente do ScreenCaptureKit: a captura de tela e a de som
//! são APIs separadas, e a de som exige escolher entre dois modos que parecem iguais e
//! não são.
//!
//! O laço clássico (`AUDCLNT_STREAMFLAGS_LOOPBACK` no dispositivo de saída) grava tudo
//! o que sai pela placa — **inclusive o que este app está tocando**, que é o som das
//! telas que estamos assistindo. Numa sala com duas pessoas compartilhando, isso é
//! realimentação garantida: cada um manda de volta o áudio do outro.
//!
//! O laço por processo resolve, e é o que está aqui: o Windows monta um dispositivo
//! virtual que grava tudo **menos** a árvore de processos indicada, e a árvore indicada
//! é a nossa. É a mesma promessa que o `excludesCurrentProcessAudio` cumpre no macOS.
//!
//! Só se pode excluir **uma** árvore por captura, então excluir o Discord *e* a nós
//! mesmos ao mesmo tempo não existe. A saída é virar a pergunta do avesso: ao
//! compartilhar uma janela, grava-se **só** a árvore daquele processo. O som do jogo
//! entra, e o do Discord, o do navegador e o nosso ficam de fora sem excluir ninguém —
//! que é exatamente o que quem compartilha um jogo quer.
//!
//! Na tela inteira não há um processo só, e aí a inclusão vira várias: um laço por
//! processo que toca som, menos a nossa árvore e a do Discord, somados aqui por um
//! relógio só. Excluir só a nossa árvore fica para quem pediu o Discord junto — e para
//! quando a mistura não abre: ela cai uma vez para esse laço, em vez de transmitir mudo.

use std::collections::{HashMap, HashSet, VecDeque};
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync, DEVICE_STATE_ACTIVE,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
    PROCESS_LOOPBACK_MODE, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    StructuredStorage::PROPVARIANT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
    TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcessId, GetProcessTimes, INFINITE, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{Interface, Ref, implement};

use crate::{AudioChunk, CaptureConfig, CaptureError, CaptureEvent};

/// De quem gravar o som.
#[derive(Debug, Clone, Copy)]
pub enum AudioScope {
    /// Tudo menos a nossa árvore. Evita devolver o som de quem se está assistindo.
    ExcludeSelf,

    /// Só a árvore deste processo. Deixa Discord, navegador e nós de fora de graça.
    OnlyProcess(u32),

    /// Cada processo que toca som, menos nós e `CaptureConfig::MUTED_EXECUTABLES`.
    ExceptMuted,
}

/// O que o resto do app espera receber, e o que o Opus quer na entrada.
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;

/// Tamanho do buffer do cliente, em unidades de 100 ns. Vinte milissegundos: um bloco
/// de Opus. Menor faz o laço acordar à toa; maior atrasa a voz.
const BUFFER_HNS: i64 = 200_000;

/// A mistura lê por relógio, não por evento: o buffer de cada processo tem de aguentar
/// uma volta inteira do laço, varredura de processos incluída, sem perder pacote.
const MIX_BUFFER_HNS: i64 = 2_000_000;

const MIX_TICK_MS: u32 = 10;

/// De quanto em quanto tempo procurar quem começou a tocar som. Um jogo aberto depois
/// de a transmissão começar leva até isso para entrar.
const RESCAN: Duration = Duration::from_secs(2);

/// Um bloco de Opus. Menos que isso não vale acordar o encoder.
const BLOCK_FRAMES: u64 = 960;

/// Quanto som de um processo precisa juntar antes de entrar na mistura: 40 ms.
const PRIME_SAMPLES: usize = 3_840;

/// O máximo de atraso que uma fila acumula antes de descartar o começo: 200 ms.
const BACKLOG_SAMPLES: usize = 19_200;

/// Uma volta travada por mais que isso não vira silêncio despejado de uma vez: 200 ms.
const STALL_FRAMES: u64 = 9_600;

/// Quantas varreduras seguidas sem enxergar os processos a mistura aguenta com laço
/// aberto antes de desistir: 10 s.
const BLIND_SCANS: u32 = 5;

/// Teto de ancestrais a seguir. Número de processo reciclado pode fechar um ciclo.
const MAX_LINEAGE: usize = 64;

/// Quem toca o som dos outros. Mixer virtual (Sonar da SteelSeries, Voicemeeter) recebe
/// o Discord num dispositivo falso e o devolve misturado na saída de verdade, e o
/// `audiodg.exe` é o motor de áudio do próprio Windows. Incluir a árvore deles traria o
/// Discord de volta e dobraria o som de quem já entra pelo próprio processo.
const RELAYS: &[&str] = &[
    "audiodg.exe",
    "SteelSeriesSonar.exe",
    "voicemeeter.exe",
    "voicemeeter_x64.exe",
    "voicemeeterpro.exe",
    "voicemeeterpro_x64.exe",
    "voicemeeter8.exe",
    "voicemeeter8x64.exe",
];

/// `WAVE_FORMAT_IEEE_FLOAT`. Ponto flutuante porque é o que sai do mixer sem conversão
/// e é o que o encoder de áudio recebe — pedir inteiro aqui só criaria duas conversões.
const FORMATO_FLOAT: u16 = 3;

type EventSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

/// Espera a ativação assíncrona terminar.
///
/// `ActivateAudioInterfaceAsync` devolve na hora e avisa depois. Sem este objeto não há
/// para onde avisar, e a API não oferece versão síncrona.
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Done(HANDLE);

impl IActivateAudioInterfaceCompletionHandler_Impl for Done_Impl {
    fn ActivateCompleted(
        &self,
        _operacao: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        unsafe { SetEvent(self.0) }
    }
}

pub struct SystemAudio {
    stop_event: HANDLE,
    thread: Option<JoinHandle<()>>,
    chunks: Arc<AtomicU64>,
}

/// # Segurança
///
/// O único campo que o Rust recusa é o `HANDLE` do evento de parada, porque `HANDLE` é
/// ponteiro. Handle de evento não é objeto COM nem está preso a apartamento nenhum:
/// pertence ao processo inteiro, e `SetEvent` é chamado de outra thread por desenho. A
/// captura já atravessa threads assim lá dentro; aqui é a mesma travessia, com o dono
/// do handle junto.
unsafe impl Send for SystemAudio {}

impl SystemAudio {
    pub fn start(
        sink: EventSink,
        chunks: Arc<AtomicU64>,
        scope: AudioScope,
    ) -> Result<Self, CaptureError> {
        // Manual reset: quem espera pode ser mais de um ponto do laço, e um evento de
        // parada que se rearma sozinho seria perdido pela metade das esperas.
        let stop_event = unsafe { CreateEventW(None, true, false, None) }.map_err(platform_error)?;
        let counter = Arc::clone(&chunks);

        // `HANDLE` é ponteiro, e por isso não é `Send`. Um handle de evento pertence ao
        // processo inteiro e existe justamente para ser esperado de outra thread, então
        // ele atravessa como número — o compilador não precisa acreditar em nós.
        let stop_raw = stop_event.0 as isize;

        let thread = std::thread::Builder::new()
            .name("unkvoid-audio".into())
            .spawn(move || {
                let stop_event = HANDLE(stop_raw as *mut core::ffi::c_void);

                if let Err(failure) = unsafe { record(stop_event, &sink, &counter, scope) } {
                    // Sem isto o áudio simplesmente não existe e ninguém fica sabendo:
                    // a transmissão sai muda e todo contador marca saúde.
                    tracing::error!(failure = %failure, "a captura de áudio do sistema parou");
                }
            })
            .map_err(|failure| CaptureError::Platform(failure.to_string()))?;

        Ok(Self {
            stop_event,
            thread: Some(thread),
            chunks,
        })
    }

    pub fn chunks_captured(&self) -> u64 {
        self.chunks.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) {
        unsafe {
            let _ = SetEvent(self.stop_event);
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }

        unsafe {
            let _ = CloseHandle(self.stop_event);
        }
    }
}

impl Drop for SystemAudio {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.stop();
        }
    }
}

fn platform_error(failure: windows::core::Error) -> CaptureError {
    CaptureError::Platform(failure.to_string())
}

/// O som de um processo esperando a vez de entrar na mistura.
#[derive(Default)]
struct Lane {
    queue: VecDeque<f32>,
    primed: bool,
}

impl Lane {
    /// Soma o próximo trecho desta fila em `mixed`.
    ///
    /// Fila que secou só volta com folga: entrar picada, um pacote por vez, põe silêncio
    /// no meio do som, e isso estala.
    fn add_into(&mut self, mixed: &mut [f32]) {
        if !self.primed {
            if self.queue.len() < PRIME_SAMPLES {
                return;
            }

            self.primed = true;
        }

        // O relógio da placa e o nosso nunca batem, e uma volta lenta empilha pacotes:
        // o excesso sai pelo começo em vez de virar atraso para sempre.
        let limit = mixed.len() + BACKLOG_SAMPLES;

        if self.queue.len() > limit {
            self.queue.drain(..self.queue.len() - limit);
        }

        let available = self.queue.len().min(mixed.len());

        for (slot, sample) in mixed.iter_mut().zip(self.queue.drain(..available)) {
            *slot += sample;
        }

        if available < mixed.len() {
            self.primed = false;
        }
    }
}

/// Um cliente de laço por processo, aberto e gravando.
struct Tap {
    process: u32,
    created: Option<u64>,
    client: IAudioClient,
    capture: IAudioCaptureClient,
    ready_event: HANDLE,
    lane: Lane,
}

impl Tap {
    unsafe fn open(
        process: u32,
        mode: PROCESS_LOOPBACK_MODE,
        buffer: i64,
    ) -> Result<Self, CaptureError> {
        unsafe {
            let client = activate(process, mode)?;

            let format = WAVEFORMATEX {
                wFormatTag: FORMATO_FLOAT,
                nChannels: CHANNELS,
                nSamplesPerSec: SAMPLE_RATE,
                nAvgBytesPerSec: SAMPLE_RATE * u32::from(CHANNELS) * 4,
                nBlockAlign: CHANNELS * 4,
                wBitsPerSample: 32,
                cbSize: 0,
            };

            // O dispositivo de laço por processo não tem formato de mixer para perguntar:
            // quem grava escolhe. `AUTOCONVERTPCM` é o que autoriza o Windows a reamostrar
            // quando a saída de verdade está em outra taxa — sem ele, uma placa em 44,1 kHz
            // faz o `Initialize` recusar o formato e a transmissão sai muda.
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK
                        | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    buffer,
                    0,
                    &format,
                    None,
                )
                .map_err(platform_error)?;

            let ready_event = CreateEventW(None, false, false, None).map_err(platform_error)?;

            let started = client
                .SetEventHandle(ready_event)
                .and_then(|()| client.GetService::<IAudioCaptureClient>())
                .and_then(|capture| client.Start().map(|()| capture));

            match started {
                Ok(capture) => Ok(Self {
                    process,
                    created: created(process),
                    client,
                    capture,
                    ready_event,
                    lane: Lane::default(),
                }),
                Err(failure) => {
                    // A mistura tenta de novo a cada varredura: sem fechar aqui, um
                    // processo que sempre recusa vazaria um handle por vez.
                    let _ = CloseHandle(ready_event);

                    Err(platform_error(failure))
                }
            }
        }
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.ready_event);
        }
    }
}

/// Abre o dispositivo virtual de laço por processo e bombeia até mandarem parar.
unsafe fn record(
    stop_event: HANDLE,
    sink: &EventSink,
    chunks: &Arc<AtomicU64>,
    scope: AudioScope,
) -> Result<(), CaptureError> {
    unsafe {
        // O COM é por thread, e toda a conversa com o WASAPI acontece nesta.
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(platform_error)?;

        let exclude_self = || {
            Tap::open(
                GetCurrentProcessId(),
                PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
                BUFFER_HNS,
            )
            .map(|tap| pump(stop_event, sink, chunks, &tap))
        };

        let outcome = match scope {
            AudioScope::ExcludeSelf => exclude_self(),
            AudioScope::OnlyProcess(process) => Tap::open(
                process,
                PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                BUFFER_HNS,
            )
            .map(|tap| pump(stop_event, sink, chunks, &tap)),
            // Uma queda só, e fica: o laço clássico não tenta voltar para a mistura, e se
            // ele também não abrir o erro sobe e a transmissão segue muda. Transmitir com o
            // Discord junto é ruim; transmitir o jogo sem som é pior.
            AudioScope::ExceptMuted => mix(stop_event, sink, chunks).or_else(|failure| {
                tracing::warn!(
                    failure = %failure,
                    "áudio: a mistura por processo falhou; passa a gravar tudo menos o app, e o Discord entra junto"
                );

                exclude_self()
            }),
        };

        CoUninitialize();

        outcome
    }
}

unsafe fn pump(stop_event: HANDLE, sink: &EventSink, chunks: &Arc<AtomicU64>, tap: &Tap) {
    let waits = [tap.ready_event, stop_event];

    unsafe {
        // Índice 1 é o `parar`: sair aqui é a única saída limpa do laço.
        while WaitForMultipleObjects(&waits, false, INFINITE) == WAIT_OBJECT_0 {
            drain(&tap.capture, |samples| emit(sink, chunks, samples));
        }
    }
}

/// Grava cada processo que toca som num cliente próprio e soma tudo por um relógio só.
///
/// ponytail: um laço por processo e varredura a cada `RESCAN`; se o custo de muitos
/// processos aparecer, trocar a varredura por `IAudioSessionNotification`.
///
/// Devolve erro quando desiste (`gives_up`), com o motivo da última varredura: quem chama
/// cai para o laço clássico.
unsafe fn mix(
    stop_event: HANDLE,
    sink: &EventSink,
    chunks: &Arc<AtomicU64>,
) -> Result<(), CaptureError> {
    unsafe {
        let own = GetCurrentProcessId();
        let started = Instant::now();
        let mut taps: Vec<Tap> = Vec::new();
        let mut refused: Vec<(u32, Option<u64>)> = Vec::new();
        let mut scanned: Option<Instant> = None;
        let mut failed_scans = 0_u32;
        let mut emitted = 0_u64;

        while WaitForSingleObject(stop_event, MIX_TICK_MS) == WAIT_TIMEOUT {
            if scanned.is_none_or(|at| at.elapsed() >= RESCAN) {
                scanned = Some(Instant::now());

                match rescan(own, &mut taps, &mut refused) {
                    Ok(()) => failed_scans = 0,
                    Err(failure) => {
                        failed_scans += 1;

                        if gives_up(failed_scans, taps.len()) {
                            return Err(failure);
                        }

                        // Só a primeira da sequência: a varredura volta a cada 2 s.
                        if failed_scans == 1 {
                            tracing::warn!(failure = %failure, "áudio: não deu para ver quem toca som");
                        }
                    }
                }
            }

            for tap in &mut taps {
                drain(&tap.capture, |samples| tap.lane.queue.extend(samples));
            }

            let clock =
                (started.elapsed().as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000) as u64;

            emitted = emitted.max(clock.saturating_sub(STALL_FRAMES));

            if clock - emitted < BLOCK_FRAMES {
                continue;
            }

            let mut mixed = vec![0.0_f32; (clock - emitted) as usize * usize::from(CHANNELS)];

            for tap in &mut taps {
                tap.lane.add_into(&mut mixed);
            }

            for sample in &mut mixed {
                *sample = sample.clamp(-1.0, 1.0);
            }

            emitted = clock;
            emit(sink, chunks, mixed);
        }

        Ok(())
    }
}

/// A mistura desiste quando uma varredura falha sem laço nenhum aberto — está muda e sem
/// como deixar de estar — ou quando fica cega por `BLIND_SCANS` seguidas: o que já toca
/// continua, mas o jogo aberto depois não entraria nunca. Falha passageira com laço aberto
/// (fone desplugado no meio da varredura) não derruba nada.
fn gives_up(failed_scans: u32, open_taps: usize) -> bool {
    failed_scans > 0 && (open_taps == 0 || failed_scans >= BLIND_SCANS)
}

/// Abre o laço de quem começou a tocar som e fecha o de quem saiu ou deixou de poder
/// entrar.
///
/// Falha quando não dá para ver os processos, ou quando quem tentou entrar recusou e não
/// sobrou laço nenhum aberto.
///
/// ponytail: recusa de um processo com outro laço aberto só vai para o log, e esse
/// processo fica mudo na transmissão. Se aparecer em hardware um jogo que recusa o laço,
/// a saída é cair para o `ExcludeSelf` em qualquer recusa.
unsafe fn rescan(
    own: u32,
    taps: &mut Vec<Tap>,
    refused: &mut Vec<(u32, Option<u64>)>,
) -> Result<(), CaptureError> {
    unsafe {
        let table = processes()?;
        let mut candidates = audible_processes()?;

        let excluded: Vec<u32> = table
            .iter()
            .filter(|(_, process)| {
                CaptureConfig::MUTED_EXECUTABLES
                    .iter()
                    .chain(RELAYS)
                    .any(|name| name.eq_ignore_ascii_case(&process.name))
            })
            .map(|(&pid, _)| pid)
            .chain([own])
            .collect();

        candidates.extend(taps.iter().map(|tap| tap.process));

        // Quem não está na lista nasceu depois dela: pode ser o Discord, e espera a volta
        // seguinte para ser conhecido.
        candidates.retain(|pid| table.contains_key(pid));

        let wanted = chosen(&candidates, &excluded, |pid| valid_parent(&table, pid));

        taps.retain(|tap| wanted.contains(&tap.process) && created(tap.process) == tap.created);

        let mut refusal = None;

        for process in wanted {
            let identity = (process, created(process));

            if taps.iter().any(|tap| tap.process == process) || refused.contains(&identity) {
                continue;
            }

            let name = table.get(&process).map_or("?", |entry| entry.name.as_str());

            match Tap::open(
                process,
                PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                MIX_BUFFER_HNS,
            ) {
                Ok(tap) => {
                    tracing::info!(process, name, "áudio: processo entra na mistura");
                    taps.push(tap);
                }
                Err(failure) => {
                    tracing::warn!(process, name, failure = %failure, "áudio: processo recusou o laço");
                    refused.push(identity);
                    refusal = Some(failure);
                }
            }
        }

        match refusal {
            Some(failure) if taps.is_empty() => Err(failure),
            _ => Ok(()),
        }
    }
}

/// Quais processos gravar, cada um pela sua árvore.
///
/// Fica de fora quem é silenciado, quem descende de um silenciado (o WebView toca num
/// filho nosso; o Discord, num filho dele) e quem tem um silenciado abaixo de si:
/// incluir a árvore do Explorer traria o Discord junto. Quem descende de outro escolhido
/// também fica, porque a árvore do ancestral já o traz e gravar de novo dobraria o som.
fn chosen(candidates: &[u32], muted: &[u32], parent: impl Fn(u32) -> Option<u32>) -> Vec<u32> {
    let parent = &parent;
    let lineage = |pid: u32| {
        std::iter::successors(Some(pid), move |&current| parent(current)).take(MAX_LINEAGE)
    };

    let above_muted: HashSet<u32> = muted.iter().flat_map(|&pid| lineage(pid)).collect();

    let allowed: Vec<u32> = candidates
        .iter()
        .copied()
        .filter(|&pid| {
            !above_muted.contains(&pid) && !lineage(pid).any(|ancestor| muted.contains(&ancestor))
        })
        .collect();

    let mut wanted: Vec<u32> = allowed
        .iter()
        .copied()
        .filter(|&pid| !lineage(pid).skip(1).any(|ancestor| allowed.contains(&ancestor)))
        .collect();

    wanted.sort_unstable();
    wanted.dedup();

    wanted
}

struct Process {
    parent: u32,
    name: String,
}

unsafe fn processes() -> Result<HashMap<u32, Process>, CaptureError> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(platform_error)?;
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut table = HashMap::new();
        let mut found = Process32FirstW(snapshot, &mut entry);

        while found.is_ok() {
            let length = entry
                .szExeFile
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(entry.szExeFile.len());

            table.insert(
                entry.th32ProcessID,
                Process {
                    parent: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..length]),
                },
            );

            found = Process32NextW(snapshot, &mut entry);
        }

        let _ = CloseHandle(snapshot);

        // Tabela vazia faria o Discord passar por desconhecido e entrar na mistura.
        if table.is_empty() {
            return Err(CaptureError::Platform("a lista de processos veio vazia".into()));
        }

        Ok(table)
    }
}

/// Quem tem sessão de som em alguma saída ativa, tocando agora ou não.
unsafe fn audible_processes() -> Result<Vec<u32>, CaptureError> {
    unsafe {
        let devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(platform_error)?;
        let endpoints = devices
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(platform_error)?;
        let mut processes = Vec::new();

        for index in 0..endpoints.GetCount().map_err(platform_error)? {
            let Ok(manager) = endpoints
                .Item(index)
                .and_then(|device| device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None))
            else {
                continue;
            };

            let sessions = manager.GetSessionEnumerator().map_err(platform_error)?;

            for session_index in 0..sessions.GetCount().map_err(platform_error)? {
                let process = sessions
                    .GetSession(session_index)
                    .and_then(|session| session.cast::<IAudioSessionControl2>())
                    .and_then(|session| session.GetProcessId());

                // Zero é a sessão dos sons do sistema, que não é de processo nenhum.
                if let Ok(process) = process
                    && process != 0
                {
                    processes.push(process);
                }
            }
        }

        Ok(processes)
    }
}

/// O pai de `pid`, se ainda for ele.
///
/// O Windows recicla números de processo: o pai registrado de quem sobreviveu ao pai pode
/// ser hoje um jogo aberto depois, que passaria por ancestral do Discord e ficaria mudo.
unsafe fn valid_parent(table: &HashMap<u32, Process>, pid: u32) -> Option<u32> {
    let parent = table.get(&pid)?.parent;

    if parent == 0 || parent == pid || !table.contains_key(&parent) {
        return None;
    }

    unsafe { (created(parent)? <= created(pid)?).then_some(parent) }
}

/// Quando o processo nasceu: separa um processo de outro que herdou o número dele.
unsafe fn created(pid: u32) -> Option<u64> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);

        let _ = CloseHandle(handle);

        result.ok()?;

        Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }
}

/// Tira do Windows tudo o que já está pronto, pacote a pacote.
unsafe fn drain(capture: &IAudioCaptureClient, mut deliver: impl FnMut(Vec<f32>)) {
    let per_frame = usize::from(CHANNELS);

    unsafe {
        loop {
            let mut data = std::ptr::null_mut();
            let mut frames = 0_u32;
            let mut flags = 0_u32;

            if capture
                .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                .is_err()
                || frames == 0
            {
                break;
            }

            let total = frames as usize * per_frame;

            // Silêncio vem com o ponteiro sujo de propósito: o Windows avisa pela
            // bandeira em vez de zerar o buffer, e ler dali seria lixo audível.
            let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                vec![0.0_f32; total]
            } else {
                std::slice::from_raw_parts(data.cast::<f32>(), total).to_vec()
            };

            let _ = capture.ReleaseBuffer(frames);

            deliver(samples);
        }
    }
}

fn emit(sink: &EventSink, chunks: &AtomicU64, samples: Vec<f32>) {
    chunks.fetch_add(1, Ordering::Relaxed);

    sink(CaptureEvent::Audio(AudioChunk {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        samples,
    }));
}

/// Pede ao Windows um cliente de laço sobre a árvore de `process`.
unsafe fn activate(process: u32, mode: PROCESS_LOOPBACK_MODE) -> Result<IAudioClient, CaptureError> {
    unsafe {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: process,
                    ProcessLoopbackMode: mode,
                },
            },
        };

        let variant = blob_of(&mut params);

        let done_event = CreateEventW(None, false, false, None).map_err(platform_error)?;
        let handler: IActivateAudioInterfaceCompletionHandler = Done(done_event).into();

        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*variant),
            &handler,
        );

        if operation.is_ok() {
            WaitForSingleObject(done_event, INFINITE);
        }

        let _ = CloseHandle(done_event);

        let operation = operation.map_err(platform_error)?;
        let mut status = windows::core::HRESULT(0);
        let mut unknown = None;

        operation
            .GetActivateResult(&mut status, &mut unknown)
            .map_err(platform_error)?;

        status.ok().map_err(platform_error)?;

        unknown
            .ok_or_else(|| {
                CaptureError::Platform("o Windows não devolveu o cliente de áudio".into())
            })?
            .cast::<IAudioClient>()
            .map_err(platform_error)
    }
}

/// Empacota a configuração no `PROPVARIANT` de blob que a API recebe. Não há construtor
/// para isso na crate, então o registro é montado campo a campo.
///
/// Sai como `ManuallyDrop` porque o `PROPVARIANT` se julga dono do que carrega: ao sair
/// de escopo ele chama `PropVariantClear`, e para `VT_BLOB` isso devolve `pBlobData` ao
/// alocador do COM. O blob aqui é a pilha de quem chamou, que nunca veio desse alocador
/// — liberá-la corrompia o heap e matava o processo (0xC0000374) milissegundos depois de
/// a transmissão dizer que estava no ar, sem HRESULT e sem pânico. Dono destes bytes é
/// `params`, e ele vive na pilha de quem chamou até depois da ativação.
///
/// # Segurança
///
/// O `PROPVARIANT` devolvido guarda um ponteiro cru para `params`: ele só vale enquanto
/// `params` não sair de escopo.
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

#[cfg(test)]
mod testes {
    use super::*;

    /// Deixa o `PROPVARIANT` do blob sair de escopo de propósito. Enquanto ele for
    /// `ManuallyDrop` isso não faz nada; no dia em que voltar a ser solto normalmente, o
    /// `PropVariantClear` devolve pilha ao alocador do COM e este teste morre com o
    /// processo — que é a falha que se quer impedir de voltar.
    #[test]
    fn o_blob_nao_libera_a_pilha() {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: 1,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };

        let esperado = std::ptr::from_mut(&mut params).cast::<u8>();
        let variant = unsafe { blob_of(&mut params) };

        unsafe {
            assert_eq!(variant.Anonymous.Anonymous.vt, VT_BLOB);
            assert_eq!(variant.Anonymous.Anonymous.Anonymous.blob.pBlobData, esperado);
        }
    }

    #[test]
    fn mix_leaves_discord_and_ourselves_out() {
        // (filho, pai). Explorer 10 abriu o Discord 20, nós 30 e o jogo 40; o Discord
        // toca no filho 21 e o nosso WebView no neto 32. A Steam 50 toca som e abriu o
        // jogo 51. O Chrome 60 toca no filho 61.
        let links = [(20, 10), (21, 20), (30, 10), (31, 30), (32, 31), (40, 10), (51, 50), (61, 60)];
        let parent = |pid| links.iter().find(|(child, _)| *child == pid).map(|(_, parent)| *parent);

        let wanted = chosen(&[10, 21, 32, 40, 50, 51, 61, 40], &[20, 21, 30], parent);

        // O Explorer traria o Discord na árvore, 21 e 32 descendem de silenciados, e o 51
        // já vem na árvore da Steam.
        assert_eq!(wanted, vec![40, 50, 61]);
    }

    #[test]
    fn the_mix_gives_up_once_it_is_mute_or_blind_for_too_long() {
        // Varredura que deu certo nunca derruba, com ou sem laço aberto.
        assert!(!gives_up(0, 0));
        assert!(!gives_up(0, 3));

        // Falhou sem nada aberto: muda, e a primeira já basta.
        assert!(gives_up(1, 0));

        // Falhou com laço aberto: aguenta a passageira, não a que não passa.
        assert!(!gives_up(1, 2));
        assert!(!gives_up(BLIND_SCANS - 1, 2));
        assert!(gives_up(BLIND_SCANS, 2));
    }

    #[test]
    fn a_lane_waits_for_slack_and_trims_backlog() {
        let mut lane = Lane::default();
        let mut mixed = vec![0.0_f32; 4];

        // Pouco som ainda não entra: entraria picado.
        lane.queue.extend([1.0_f32; 2]);
        lane.add_into(&mut mixed);
        assert_eq!(mixed, vec![0.0; 4]);

        lane.queue.extend(vec![0.5_f32; PRIME_SAMPLES]);
        lane.add_into(&mut mixed);
        assert_eq!(mixed, vec![1.0, 1.0, 0.5, 0.5]);

        // Atraso acima do teto sai pelo começo.
        lane.queue.extend(vec![0.25_f32; BACKLOG_SAMPLES * 2]);
        lane.add_into(&mut mixed);
        assert_eq!(lane.queue.len(), BACKLOG_SAMPLES);

        // Fila que seca volta a esperar folga.
        lane.queue.truncate(2);
        lane.add_into(&mut mixed);
        assert!(!lane.primed);
    }
}
