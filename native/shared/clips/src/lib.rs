//! O replay instantâneo: a tela fica sempre gravando num buffer em disco, e só vira arquivo
//! quando a pessoa pede.
//!
//! `captura (GPU) → encoder de hardware → buffer em disco → MP4 ao salvar`. O quadro não
//! desce para a memória do processador em nenhum ponto: é o que deixa gravar sem tirar fps
//! do jogo.
//!
//! Só existe no Windows. Nos outros sistemas sobra o buffer em disco, que é Rust puro e roda
//! os testes em qualquer máquina.

#[cfg(target_os = "windows")]
pub mod aac;
#[cfg(target_os = "windows")]
pub mod audio;
#[cfg(target_os = "windows")]
pub mod capture;
#[cfg(target_os = "windows")]
pub mod clip;
#[cfg(target_os = "windows")]
pub mod clock;
#[cfg(target_os = "windows")]
pub mod encoder;
#[cfg(target_os = "windows")]
pub mod gallery;
#[cfg(target_os = "windows")]
pub mod hotkeys;
#[cfg(target_os = "windows")]
pub mod recorder;
pub mod replay;
