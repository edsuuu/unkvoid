//! O gravador inteiro: buffer, captura da tela e som, religados quando a configuração muda
//! ou quando a captura morre sozinha — driver de vídeo atualizado, monitor desligado, placa
//! reiniciada. Quem usa o app nunca deveria descobrir que o replay parou só na hora de
//! salvar.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;

use crate::audio::{AudioCapture, AudioSettings};
use crate::capture::{ScreenCapture, VideoSettings};
use crate::clip::{ClipSummary, write_clip};
use crate::replay::{ReplayBuffer, Segment};

/// Folga do buffer além do replay pedido: o clipe começa no quadro-chave em ou antes do
/// corte, e o arquivo desse quadro precisa ainda existir.
const BUFFER_MARGIN: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq)]
pub struct RecorderSettings {
    /// O replay só grava depois que a pessoa liga, como no GeForce Experience. Ligado, fica
    /// ligado, inclusive quando o Windows inicia de novo.
    pub enabled: bool,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub replay: Duration,
}

pub struct Recorder {
    buffer: ReplayBuffer,
    capture: Option<ScreenCapture>,
    audio: Option<AudioCapture>,
    settings: RecorderSettings,
    problem: Option<String>,
}

/// O que um clipe precisa, tirado do gravador de uma vez. Escrever o MP4 demora (ler e
/// escrever gigabytes); com isto em mãos o gravador fica livre enquanto o arquivo é montado.
pub struct SaveJob {
    segments: Vec<Segment>,
    from_ns: u64,
    size: (u32, u32),
    frame_rate: u32,
}

impl Recorder {
    pub fn start(buffer_folder: PathBuf, settings: RecorderSettings) -> anyhow::Result<Self> {
        refuse_efficiency_mode();

        let buffer = ReplayBuffer::start(buffer_folder, settings.replay + BUFFER_MARGIN)?;
        let mut recorder = Self { buffer, capture: None, audio: None, settings, problem: None };

        recorder.restart();

        Ok(recorder)
    }

    pub fn apply(&mut self, settings: RecorderSettings) {
        if settings.replay != self.settings.replay {
            self.buffer.set_window(settings.replay + BUFFER_MARGIN);
        }

        let restart = settings.enabled != self.settings.enabled
            || settings.video != self.settings.video
            || settings.audio != self.settings.audio;

        self.settings = settings;

        if restart {
            self.restart();
        }
    }

    /// Religa a captura se ela parou. Devolve se o replay está gravando.
    pub fn keep_alive(&mut self) -> bool {
        if !self.settings.enabled {
            return false;
        }

        if self.capture.as_ref().is_some_and(ScreenCapture::is_running) {
            return true;
        }

        tracing::warn!("gravador: a captura parou, religando");
        self.restart();

        self.capture.as_ref().is_some_and(ScreenCapture::is_running)
    }

    /// O que a pessoa precisa saber sobre a gravação: `None` quando está tudo gravando.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn replay(&self) -> Duration {
        self.settings.replay
    }

    pub fn prepare_save(&self, length: Duration) -> anyhow::Result<SaveJob> {
        let capture = self.capture.as_ref().context("o replay está desligado")?;
        let size = capture.frame_size().context("nenhum quadro foi gravado ainda")?;

        Ok(SaveJob {
            segments: self.buffer.snapshot()?,
            from_ns: crate::clock::now_ns().saturating_sub(length.as_nanos() as u64),
            size,
            frame_rate: self.settings.video.frame_rate,
        })
    }

    fn restart(&mut self) {
        // O som antes da imagem, e os dois soltos antes de religar: dois encoders abertos na
        // mesma placa ao mesmo tempo é pedir para o segundo ser recusado.
        self.capture = None;
        self.audio = None;
        self.problem = None;

        // Desligado, o que estava no buffer vai embora: religar depois e salvar não pode
        // emendar a imagem de antes com a de agora num clipe só.
        if !self.settings.enabled {
            self.buffer.clear();

            return;
        }

        match AudioCapture::start(&self.settings.audio, self.buffer.sink()) {
            Ok(audio) => self.audio = audio,
            Err(error) if self.settings.audio.microphone.is_some() => {
                tracing::warn!(error = %format!("{error:#}"), "gravador: o microfone não abriu, gravando sem ele");

                self.problem = Some("O microfone não abriu. O replay está gravando sem ele.".into());
                self.audio = AudioCapture::start(&AudioSettings { microphone: None, ..self.settings.audio.clone() }, self.buffer.sink())
                    .unwrap_or_else(|error| {
                        tracing::error!(error = %format!("{error:#}"), "gravador: o som do sistema não abriu");

                        None
                    });
            }
            Err(error) => {
                tracing::error!(error = %format!("{error:#}"), "gravador: o som do sistema não abriu");

                self.problem = Some("O som do sistema não abriu. O replay está gravando sem som.".into());
            }
        }

        match ScreenCapture::start(self.settings.video, self.buffer.sink()) {
            Ok(capture) => self.capture = Some(capture),
            Err(error) => {
                tracing::error!(error = %format!("{error:#}"), "gravador: a captura da tela não abriu");

                self.problem = Some("A captura da tela não abriu. Tentando de novo em alguns segundos.".into());
            }
        }
    }
}

/// Pede ao Windows para não pôr o processo em modo de eficiência (EcoQoS).
///
/// O Windows 11 trata app sem janela visível como de fundo: joga as threads para os núcleos
/// de eficiência, em clock baixo. Para quem inicia com o Windows e vive na bandeja, isso é
/// sempre — e medido aqui, a captura caía de 57 para uns poucos quadros por segundo.
fn refuse_efficiency_mode() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling,
        SetProcessInformation,
    };

    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        // Zero nos bits de controle: "nunca", em vez de deixar o Windows decidir.
        StateMask: 0,
    };

    let result = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            std::ptr::from_ref(&state).cast(),
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };

    if let Err(error) = result {
        tracing::warn!(error = %error, "gravador: o Windows recusou tirar o modo de eficiência");
    }
}

/// Registra a thread que chama numa tarefa do MMCSS, o agendador multimídia do Windows
/// ("Capture", "Audio"): ela ganha prioridade sobre o que é só processamento comum sem
/// competir com o jogo, que tem a tarefa "Games" dele.
pub fn join_multimedia_task(task: &str) {
    use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
    use windows::core::HSTRING;

    let mut index = 0_u32;

    if let Err(error) = unsafe { AvSetMmThreadCharacteristicsW(&HSTRING::from(task), &mut index) } {
        tracing::warn!(error = %error, task, "gravador: o MMCSS recusou a thread");
    }
}

impl SaveJob {
    pub fn write(&self, output: &Path) -> anyhow::Result<ClipSummary> {
        write_clip(&self.segments, self.from_ns, self.size, self.frame_rate, output)
    }
}
