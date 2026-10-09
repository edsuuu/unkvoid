//! O relógio de tudo: o contador de performance do Windows (QPC), em nanossegundos.
//!
//! O Windows Graphics Capture marca cada quadro nele e o WASAPI marca cada pacote de som
//! nele, os dois em unidades de 100 ns. Medir tudo no mesmo relógio é o que mantém som e
//! imagem juntos num clipe tirado depois de horas gravando — dois relógios diferentes
//! escorregam alguns milissegundos por hora, e em trinta minutos de replay isso aparece.

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub fn now_ns() -> u64 {
    let mut counter = 0_i64;
    let mut frequency = 0_i64;

    unsafe {
        let _ = QueryPerformanceCounter(&mut counter);
        let _ = QueryPerformanceFrequency(&mut frequency);
    }

    (counter as u128 * 1_000_000_000 / frequency.max(1) as u128) as u64
}

/// O tempo que o Windows entrega em unidades de 100 ns, já no relógio do QPC.
pub fn from_hundred_nanoseconds(value: i64) -> u64 {
    value.max(0) as u64 * 100
}
