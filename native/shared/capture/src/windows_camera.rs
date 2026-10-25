use std::ffi::c_void;
use std::ptr::null_mut;
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread::{self, JoinHandle};

use ::windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Texture2D,
};
use ::windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use ::windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFMediaSource, IMFSourceReader, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources, MFShutdown, MFStartup, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFSTARTUP_FULL, MFMediaType_Video,
    MFVideoFormat_RGB32, MFVideoInterlace_Progressive,
};
use ::windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use ::windows::core::PWSTR;

use crate::{Camera, CaptureError, GpuSurface};

const VIDEO_STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// A camera frame copied out of Media Foundation's reusable sample buffer.
pub struct CameraFrame {
    pub width: u32,
    pub height: u32,
    pub timestamp_ns: u64,
    pub bgra: Vec<u8>,
}

pub struct CameraReader {
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl CameraReader {
    pub fn open<F>(
        index: u32,
        width: u32,
        height: u32,
        frame_rate: u32,
        on_frame: F,
    ) -> Result<Self, CaptureError>
    where
        F: FnMut(CameraFrame) + Send + 'static,
    {
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let (ready_sender, ready_receiver) = sync_channel(1);
        let worker = thread::Builder::new()
            .name("unkvoid-camera".into())
            .spawn(move || {
                let mut on_frame = on_frame;
                let mut session = match CameraSession::open(index, (width, height, frame_rate)) {
                    Ok(session) => session,
                    Err(error) => {
                        tracing::warn!(%error, "camera: não foi possível abrir a webcam");
                        let _ = ready_sender.send(Err(error.to_string()));

                        return;
                    }
                };

                if ready_sender.send(Ok(())).is_err() {
                    return;
                }

                while !worker_stopped.load(Ordering::Acquire) {
                    match session.read_frame() {
                        Ok(Some(frame)) => on_frame(frame),
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(%error, "camera: captura interrompida");

                            break;
                        }
                    }
                }
            })
            .map_err(|error| CaptureError::Platform(error.to_string()))?;

        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                stopped,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();

                Err(CaptureError::Platform(error))
            }
            Err(error) => {
                let _ = worker.join();

                Err(CaptureError::Platform(error.to_string()))
            }
        }
    }
}

impl Drop for CameraReader {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub struct SurfaceUploader {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    texture: Option<ID3D11Texture2D>,
    size: Option<(u32, u32)>,
}

impl SurfaceUploader {
    pub fn new() -> Result<Self, CaptureError> {
        let mut device = None;
        let mut context = None;

        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(platform_error)?;

        let (Some(device), Some(context)) = (device, context) else {
            return Err(CaptureError::Platform("Direct3D não devolveu o dispositivo da webcam".into()));
        };

        Ok(Self {
            device,
            context,
            texture: None,
            size: None,
        })
    }

    /// ponytail: a webcam USB já entrega o quadro na memória da CPU; a cópia única para a GPU é inevitável, com teto de 1080p30.
    pub fn upload(&mut self, frame: &CameraFrame) -> Result<GpuSurface, CaptureError> {
        let expected_length = frame
            .width
            .checked_mul(frame.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|length| usize::try_from(length).ok())
            .ok_or_else(|| CaptureError::Platform("tamanho inválido do quadro da webcam".into()))?;

        if frame.bgra.len() != expected_length || frame.width == 0 || frame.height == 0 {
            return Err(CaptureError::Platform("quadro BGRA da webcam inválido".into()));
        }

        if self.size != Some((frame.width, frame.height)) {
            let description = D3D11_TEXTURE2D_DESC {
                Width: frame.width,
                Height: frame.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: 0,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut texture = None;

            unsafe { self.device.CreateTexture2D(&description, None, Some(&mut texture)) }
                .map_err(platform_error)?;

            self.texture = texture;
            self.size = Some((frame.width, frame.height));
        }

        let texture = self
            .texture
            .as_ref()
            .ok_or_else(|| CaptureError::Platform("Direct3D não criou a textura da webcam".into()))?;
        let row_pitch = frame.width.checked_mul(4).ok_or_else(|| CaptureError::Platform("linha BGRA muito larga".into()))?;

        unsafe {
            self.context.UpdateSubresource(
                texture,
                0,
                None,
                frame.bgra.as_ptr().cast::<c_void>(),
                row_pitch,
                0,
            );
        }

        Ok(GpuSurface {
            texture: texture.clone(),
            device: self.device.clone(),
            context: self.context.clone(),
        })
    }
}

fn closest_format(offered: &[(u32, u32, u32)], wanted: (u32, u32, u32)) -> Option<usize> {
    if wanted.0 == 0 || wanted.1 == 0 || wanted.2 == 0 {
        return None;
    }

    offered
        .iter()
        .enumerate()
        .filter(|(_, (width, height, frame_rate))| {
            *width > 0
                && *height > 0
                && *frame_rate > 0
                && *width <= wanted.0
                && *height <= wanted.1
                && *frame_rate <= wanted.2
        })
        .min_by_key(|(_, (width, height, frame_rate))| {
            (
                wanted.0 - *width + wanted.1 - *height,
                wanted.2 - *frame_rate,
            )
        })
        .or_else(|| {
            // Webcam que só tem tamanhos maiores que o pedido (uma 4K, uma que só faz 1080p):
            // vai a menor que ela oferece, em vez de não abrir.
            offered
                .iter()
                .enumerate()
                .filter(|(_, (width, height, frame_rate))| {
                    *width > 0 && *height > 0 && *frame_rate > 0
                })
                .min_by_key(|(_, (width, height, frame_rate))| {
                    (u64::from(*width) * u64::from(*height), frame_rate.abs_diff(wanted.2))
                })
        })
        .map(|(index, _)| index)
}

pub fn cameras() -> Vec<Camera> {
    let result = thread::Builder::new()
        .name("unkvoid-camera-list".into())
        .spawn(enumerate_cameras)
        .and_then(|worker| worker.join().map_err(|_| std::io::Error::other("camera enumeration thread panicked")));

    match result {
        Ok(Ok(cameras)) => cameras,
        Ok(Err(error)) => {
            tracing::warn!(%error, "camera: não foi possível enumerar webcams");

            Vec::new()
        }
        Err(error) => {
            tracing::warn!(%error, "camera: não foi possível enumerar webcams");

            Vec::new()
        }
    }
}

fn enumerate_cameras() -> Result<Vec<Camera>, CaptureError> {
    let _apartment = ComApartment::initialize()?;
    let _runtime = MediaFoundationRuntime::start()?;
    let activations = enumerate_devices()?;

    Ok(activations
        .iter()
        .enumerate()
        .map(|(index, activation)| Camera {
            index: index as u32,
            name: friendly_name(activation).unwrap_or_default(),
        })
        .collect())
}

fn enumerate_devices() -> Result<Vec<IMFActivate>, CaptureError> {
    let mut attributes = None;
    unsafe { MFCreateAttributes(&mut attributes, 1) }.map_err(platform_error)?;
    let attributes: IMFAttributes = attributes
        .ok_or_else(|| CaptureError::Platform("Media Foundation não criou os atributos".into()))?;

    unsafe {
        attributes.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )
    }
    .map_err(platform_error)?;

    let mut activation_array = null_mut();
    let mut activation_count = 0;

    if let Err(error) = unsafe {
        MFEnumDeviceSources(&attributes, &mut activation_array, &mut activation_count)
    } {
        if !activation_array.is_null() {
            unsafe { CoTaskMemFree(Some(activation_array.cast::<c_void>().cast_const())) };
        }

        return Err(platform_error(error));
    }

    if activation_array.is_null() || activation_count == 0 {
        return Ok(Vec::new());
    }

    let raw_activations = unsafe {
        slice::from_raw_parts_mut(activation_array, activation_count as usize)
    };
    let activations = raw_activations.iter_mut().filter_map(Option::take).collect();

    unsafe { CoTaskMemFree(Some(activation_array.cast::<c_void>().cast_const())) };

    Ok(activations)
}

fn friendly_name(activation: &IMFActivate) -> Option<String> {
    let mut name = PWSTR::null();
    let mut length = 0;

    unsafe { activation.GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut name, &mut length) }.ok()?;

    let friendly_name = String::from_utf16_lossy(unsafe {
        slice::from_raw_parts(name.0, length as usize)
    });

    unsafe { CoTaskMemFree(Some(name.0.cast_const().cast())) };

    Some(friendly_name)
}

struct CameraSession {
    reader: Option<IMFSourceReader>,
    source: Option<IMFMediaSource>,
    activation: Option<IMFActivate>,
    runtime: Option<MediaFoundationRuntime>,
    apartment: Option<ComApartment>,
    width: u32,
    height: u32,
}

impl CameraSession {
    fn open(index: u32, wanted: (u32, u32, u32)) -> Result<Self, CaptureError> {
        let apartment = ComApartment::initialize()?;
        let runtime = MediaFoundationRuntime::start()?;
        let activations = enumerate_devices()?;
        let activation = activations
            .get(index as usize)
            .cloned()
            .ok_or_else(|| CaptureError::Platform("a webcam selecionada não está disponível".into()))?;
        let source: IMFMediaSource = unsafe { activation.ActivateObject() }.map_err(platform_error)?;
        let mut session = Self {
            reader: None,
            source: Some(source),
            activation: Some(activation),
            runtime: Some(runtime),
            apartment: Some(apartment),
            width: 0,
            height: 0,
        };
        let mut attributes = None;
        unsafe { MFCreateAttributes(&mut attributes, 1) }.map_err(platform_error)?;
        let attributes: IMFAttributes = attributes
            .ok_or_else(|| CaptureError::Platform("Media Foundation não criou os atributos".into()))?;
        unsafe { attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1) }
            .map_err(platform_error)?;
        let source = session
            .source
            .as_ref()
            .ok_or_else(|| CaptureError::Platform("fonte da webcam encerrada".into()))?;
        let reader = unsafe { MFCreateSourceReaderFromMediaSource(source, &attributes) }
            .map_err(platform_error)?;

        let mut offered = Vec::new();
        let mut native_index = 0;

        while let Ok(media_type) = unsafe { reader.GetNativeMediaType(VIDEO_STREAM, native_index) } {
            if let Some(format) = native_format(&media_type) {
                offered.push(format);
            }

            native_index += 1;
        }

        let selected_index = closest_format(&offered, wanted)
            .ok_or_else(|| CaptureError::Platform("a webcam não oferece um formato compatível".into()))?;
        let (width, height, frame_rate) = offered[selected_index];
        let output_type = unsafe { MFCreateMediaType() }.map_err(platform_error)?;

        let stride = width
            .checked_mul(4)
            .ok_or_else(|| CaptureError::Platform("linha da webcam excede o limite".into()))?;

        unsafe { output_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }.map_err(platform_error)?;
        unsafe { output_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32) }.map_err(platform_error)?;
        unsafe { output_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_pair(width, height)) }.map_err(platform_error)?;
        unsafe { output_type.SetUINT64(&MF_MT_FRAME_RATE, pack_pair(frame_rate, 1)) }.map_err(platform_error)?;
        unsafe {
            output_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        }
        .map_err(platform_error)?;
        unsafe { output_type.SetUINT32(&MF_MT_DEFAULT_STRIDE, stride) }.map_err(platform_error)?;
        unsafe { reader.SetCurrentMediaType(VIDEO_STREAM, None, &output_type) }.map_err(platform_error)?;

        session.reader = Some(reader);
        session.width = width;
        session.height = height;

        Ok(session)
    }

    fn read_frame(&mut self) -> Result<Option<CameraFrame>, CaptureError> {
        let reader = self
            .reader
            .as_ref()
            .ok_or_else(|| CaptureError::Platform("leitor da webcam encerrado".into()))?;
        let mut stream_flags = 0;
        let mut timestamp = 0;
        let mut sample = None;

        unsafe {
            reader.ReadSample(
                VIDEO_STREAM,
                0,
                None,
                Some(&mut stream_flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )
        }
        .map_err(platform_error)?;

        if stream_flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return Ok(None);
        }

        let Some(sample) = sample else {
            return Ok(None);
        };
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(platform_error)?;
        let mut data = null_mut();
        let mut current_length = 0;

        unsafe { buffer.Lock(&mut data, None, Some(&mut current_length)) }.map_err(platform_error)?;

        let byte_length = self
            .width
            .checked_mul(self.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| CaptureError::Platform("tamanho inválido do quadro da webcam".into()))?
            as usize;

        if (current_length as usize) < byte_length {
            let _ = unsafe { buffer.Unlock() };

            return Err(CaptureError::Platform("Media Foundation entregou um quadro incompleto".into()));
        }

        let mut bgra = unsafe { slice::from_raw_parts(data, byte_length) }.to_vec();
        unsafe { buffer.Unlock() }.map_err(platform_error)?;

        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel[3] = u8::MAX;
        }

        Ok(Some(CameraFrame {
            width: self.width,
            height: self.height,
            timestamp_ns: timestamp.max(0) as u64 * 100,
            bgra,
        }))
    }
}

impl Drop for CameraSession {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = unsafe { reader.Flush(VIDEO_STREAM) };
            drop(reader);
        }

        if let Some(source) = self.source.take() {
            let _ = unsafe { source.Shutdown() };
        }

        if let Some(activation) = self.activation.take() {
            let _ = unsafe { activation.ShutdownObject() };
        }

        self.runtime.take();
        self.apartment.take();
    }
}

fn native_format(media_type: &::windows::Win32::Media::MediaFoundation::IMFMediaType) -> Option<(u32, u32, u32)> {
    let packed_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }.ok()?;
    let packed_rate = unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) }.ok()?;
    let width = (packed_size >> 32) as u32;
    let height = packed_size as u32;
    let numerator = (packed_rate >> 32) as u32;
    let denominator = packed_rate as u32;

    if denominator == 0 {
        return None;
    }

    let frame_rate = numerator.saturating_add(denominator / 2) / denominator;

    Some((width, height, frame_rate))
}

fn pack_pair(high: u32, low: u32) -> u64 {
    u64::from(high) << 32 | u64::from(low)
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, CaptureError> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(platform_error)?;

        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct MediaFoundationRuntime;

impl MediaFoundationRuntime {
    fn start() -> Result<Self, CaptureError> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(platform_error)?;

        Ok(Self)
    }
}

impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        if let Err(error) = unsafe { MFShutdown() } {
            tracing::warn!(%error, "camera: Media Foundation não encerrou corretamente");
        }
    }
}

fn platform_error(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::Platform(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn closest_format_prefers_the_requested_size_and_rate_without_upscaling() {
        let offered = [
            (1920, 1080, 60),
            (1280, 720, 15),
            (1280, 720, 30),
            (640, 480, 30),
        ];

        assert_eq!(closest_format(&offered, (1280, 720, 30)), Some(2));
        assert_eq!(closest_format(&offered, (1280, 720, 24)), Some(1));
        assert_eq!(
            closest_format(&offered, (640, 480, 15)),
            Some(3),
            "nada cabe no pedido: vai o menor que a webcam tem"
        );
        assert_eq!(closest_format(&[(1920, 1080, 30)], (1280, 720, 30)), Some(0));
        assert_eq!(closest_format(&offered, (0, 0, 0)), None);
    }

    #[test]
    #[ignore = "requires a Windows webcam"]
    fn webcam_reader_delivers_tightly_packed_bgra_frames() {
        let (frame_sender, frame_receiver) = sync_channel(4);
        let reader = CameraReader::open(0, 1280, 720, 30, move |frame| {
            let _ = frame_sender.try_send(frame);
        })
        .expect("a webcam should open");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut received = 0;

        while Instant::now() < deadline {
            if let Ok(frame) = frame_receiver.recv_timeout(Duration::from_millis(100)) {
                assert_eq!(frame.bgra.len(), frame.width as usize * frame.height as usize * 4);
                received += 1;
            }
        }

        drop(reader);
        assert!(received > 0, "the webcam should produce frames for two seconds");
    }
}
