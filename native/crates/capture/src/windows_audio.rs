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
//! mesmos ao mesmo tempo não existe. A saída é virar a pergunta do avesso quando dá: ao
//! compartilhar uma janela, grava-se **só** a árvore daquele processo. O som do jogo
//! entra, e o do Discord, o do navegador e o nosso ficam de fora sem excluir ninguém —
//! que é exatamente o que quem compartilha um jogo quer.
//!
//! Ao compartilhar a tela inteira não há um processo só, e aí volta a exclusão da nossa
//! árvore. Nesse caso o Discord entra no áudio, e a interface avisa antes.

use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, StructuredStorage::PROPVARIANT,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcessId, INFINITE, SetEvent, WaitForMultipleObjects,
    WaitForSingleObject,
};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{Interface, Ref, implement};

use crate::{AudioChunk, CaptureError, CaptureEvent};

/// De quem gravar o som.
#[derive(Debug, Clone, Copy)]
pub enum AudioScope {
    /// Tudo menos a nossa árvore. Evita devolver o som de quem se está assistindo.
    ExcludeSelf,

    /// Só a árvore deste processo. Deixa Discord, navegador e nós de fora de graça.
    OnlyProcess(u32),
}

/// O que o resto do app espera receber, e o que o Opus quer na entrada.
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;

/// Tamanho do buffer do cliente, em unidades de 100 ns. Vinte milissegundos: um bloco
/// de Opus. Menor faz o laço acordar à toa; maior atrasa a voz.
const BUFFER_HNS: i64 = 200_000;

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

        let outcome = pump(stop_event, sink, chunks, scope);

        CoUninitialize();

        outcome
    }
}

unsafe fn pump(
    stop_event: HANDLE,
    sink: &EventSink,
    chunks: &Arc<AtomicU64>,
    scope: AudioScope,
) -> Result<(), CaptureError> {
    unsafe {
        let client = activate(scope)?;

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
                BUFFER_HNS,
                0,
                &format,
                None,
            )
            .map_err(platform_error)?;

        let ready_event = CreateEventW(None, false, false, None).map_err(platform_error)?;

        client.SetEventHandle(ready_event).map_err(platform_error)?;

        let capture: IAudioCaptureClient = client.GetService().map_err(platform_error)?;

        client.Start().map_err(platform_error)?;

        let waits = [ready_event, stop_event];
        let per_frame = usize::from(CHANNELS);

        loop {
            // Índice 1 é o `parar`: sair aqui é a única saída limpa do laço.
            if WaitForMultipleObjects(&waits, false, INFINITE) != WAIT_OBJECT_0 {
                break;
            }

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

                chunks.fetch_add(1, Ordering::Relaxed);

                sink(CaptureEvent::Audio(AudioChunk {
                    sample_rate: SAMPLE_RATE,
                    channels: CHANNELS,
                    samples,
                }));
            }
        }

        let _ = client.Stop();
        let _ = CloseHandle(ready_event);

        Ok(())
    }
}

/// Pede ao Windows um cliente de áudio que grava tudo menos a nossa própria árvore.
unsafe fn activate(scope: AudioScope) -> Result<IAudioClient, CaptureError> {
    unsafe {
        let (process, mode) = match scope {
            AudioScope::ExcludeSelf => (
                GetCurrentProcessId(),
                PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            ),
            AudioScope::OnlyProcess(pid) => {
                (pid, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE)
            }
        };

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
        )
        .map_err(platform_error)?;

        WaitForSingleObject(done_event, INFINITE);

        let _ = CloseHandle(done_event);

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
}
