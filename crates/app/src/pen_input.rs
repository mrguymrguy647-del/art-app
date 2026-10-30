//! Reads pen and touchscreen input straight from Windows Ink.
//!
//! The window library under egui (winit) passes the pen on as a "touch"
//! with pressure, but drops tilt, rotation, the eraser end and the side
//! button. So on Windows we add a small *message hook*: a function that sees
//! every message Windows sends to our window just before winit handles it.
//! For pen and touch messages we ask Windows for the full details and store
//! them. Then winit carries on as normal.
//!
//! On other systems this module collects nothing, and the app uses the mouse.

use std::cell::RefCell;
use std::rc::Rc;

use art_engine::pen::PenSample;
use eframe::egui;

/// One pen or touch reading from Windows Ink.
#[derive(Debug, Clone, Default)]
pub struct InkEvent {
    /// The reading itself. `x` and `y` are in physical pixels inside the window.
    pub sample: PenSample,
    /// The pen tip or finger is touching the screen/tablet.
    pub in_contact: bool,
    /// The pen is near enough for the tablet to see it. False when it has
    /// just moved away or left the window.
    pub in_range: bool,
    /// The pen's side (barrel) button is held.
    pub barrel_button: bool,
    /// Which values the device actually measures.
    pub has_pressure: bool,
    pub has_tilt: bool,
    /// How many hardware readings this event stands for. When the app is
    /// busy, Windows bundles several readings into one message.
    pub reading_count: u32,
    /// The tablet's name, for example "Wacom One".
    pub device_name: String,
}

/// Don't keep more than this many unread events (e.g. while the window is
/// minimised and the app isn't reading them).
const MAX_QUEUED_EVENTS: usize = 10_000;

#[derive(Default)]
struct Shared {
    events: Vec<InkEvent>,
    egui_ctx: Option<egui::Context>,
}

/// Collects pen events from Windows Ink.
///
/// Cloning is cheap: all clones share the same queue. `Rc<RefCell<..>>` is
/// enough (no Mutex) because Windows messages and egui both run on the main
/// thread.
#[derive(Clone, Default)]
pub struct PenInput {
    shared: Rc<RefCell<Shared>>,
}

impl PenInput {
    /// Lets new pen events wake egui up, so the screen updates right away.
    pub fn set_egui_context(&self, ctx: &egui::Context) {
        self.shared.borrow_mut().egui_ctx = Some(ctx.clone());
    }

    /// Takes all pen events that arrived since the last call, oldest first.
    pub fn take_events(&self) -> Vec<InkEvent> {
        std::mem::take(&mut self.shared.borrow_mut().events)
    }

    /// Adds an event and asks egui to redraw.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn push(&self, event: InkEvent) {
        // try_borrow_mut instead of borrow_mut: if the app happens to be
        // reading the queue right now, drop this event rather than crash.
        let Ok(mut shared) = self.shared.try_borrow_mut() else {
            return;
        };
        if shared.events.len() < MAX_QUEUED_EVENTS {
            shared.events.push(event);
        }
        if let Some(ctx) = &shared.egui_ctx {
            ctx.request_repaint();
        }
    }

    /// The hook to give eframe (`NativeOptions::event_loop_builder`).
    /// Returns `None` on systems other than Windows.
    pub fn event_loop_hook(&self) -> Option<eframe::EventLoopBuilderHook> {
        #[cfg(windows)]
        {
            Some(windows_ink::event_loop_hook(self.clone()))
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

#[cfg(windows)]
mod windows_ink {
    use std::collections::HashMap;

    use art_engine::pen::{InputKind, PenSample};
    use windows::Win32::Foundation::{HANDLE, POINT};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::UI::Controls::POINTER_DEVICE_INFO;
    use windows::Win32::UI::Input::Pointer::{
        GetPointerDevice, GetPointerInfo, GetPointerPenInfo, GetPointerType,
        POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE, POINTER_INFO, POINTER_PEN_INFO,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        MSG, PEN_FLAG_BARREL, PEN_FLAG_ERASER, PEN_FLAG_INVERTED, PEN_MASK_PRESSURE,
        PEN_MASK_ROTATION, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, POINTER_INPUT_TYPE, PT_PEN, PT_TOUCH,
        WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE,
    };
    use winit::platform::windows::EventLoopBuilderExtWindows;

    use super::{InkEvent, PenInput};

    /// Windows reports pen pressure from 0 to 1024.
    const MAX_PRESSURE: f32 = 1024.0;

    pub fn event_loop_hook(pen_input: PenInput) -> eframe::EventLoopBuilderHook {
        Box::new(move |builder| {
            // Tablet names, looked up once per device and then remembered.
            let mut device_names = HashMap::new();

            builder.with_msg_hook(move |msg| {
                // SAFETY: winit gives us a pointer to the MSG it just received
                // from Windows, valid for the duration of this call.
                let msg = unsafe { &*(msg as *const MSG) };
                if let Some(event) = read_message(msg, &mut device_names) {
                    pen_input.push(event);
                }
                // false = we only looked. winit still handles the message.
                false
            });
        })
    }

    /// Turns a pen or touch message into an `InkEvent`. Returns `None` for
    /// every other kind of message.
    fn read_message(msg: &MSG, device_names: &mut HashMap<usize, String>) -> Option<InkEvent> {
        if !matches!(
            msg.message,
            WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERLEAVE
        ) {
            return None;
        }

        // The pointer's ID is in the low 16 bits of wParam.
        let pointer_id = (msg.wParam.0 & 0xFFFF) as u32;

        let mut kind = POINTER_INPUT_TYPE::default();
        // SAFETY: `kind` is a valid place for Windows to write the result.
        unsafe { GetPointerType(pointer_id, &mut kind) }.ok()?;

        let (info, mut event) = if kind == PT_PEN {
            read_pen(pointer_id)?
        } else if kind == PT_TOUCH {
            read_touch(pointer_id)?
        } else {
            return None;
        };

        // Windows gives the position in screen pixels; we want it relative
        // to the inside of our window.
        // (The brush will later need sub-pixel precision; this is enough for now.)
        let mut point: POINT = info.ptPixelLocation;
        // SAFETY: `point` is a valid POINT for Windows to update.
        if !unsafe { ScreenToClient(msg.hwnd, &mut point) }.as_bool() {
            return None;
        }
        event.sample.x = point.x as f32;
        event.sample.y = point.y as f32;
        event.sample.time_ms = u64::from(info.dwTime);

        event.in_contact = info.pointerFlags.contains(POINTER_FLAG_INCONTACT);
        event.in_range =
            msg.message != WM_POINTERLEAVE && info.pointerFlags.contains(POINTER_FLAG_INRANGE);
        event.reading_count = info.historyCount.max(1);
        event.device_name = device_names
            .entry(info.sourceDevice.0 as usize)
            .or_insert_with(|| device_name(info.sourceDevice))
            .clone();

        Some(event)
    }

    fn read_pen(pointer_id: u32) -> Option<(POINTER_INFO, InkEvent)> {
        let mut pen = POINTER_PEN_INFO::default();
        // SAFETY: `pen` is a valid place for Windows to write the result.
        unsafe { GetPointerPenInfo(pointer_id, &mut pen) }.ok()?;

        // penMask says which values this pen really measures.
        let measures = |mask: u32| pen.penMask & mask != 0;
        let in_contact = pen
            .pointerInfo
            .pointerFlags
            .contains(POINTER_FLAG_INCONTACT);

        // INVERTED: the pen is turned round (eraser end down).
        // ERASER: the eraser end is pressed on the tablet.
        let eraser = pen.penFlags & (PEN_FLAG_INVERTED | PEN_FLAG_ERASER) != 0;

        let sample = PenSample {
            kind: if eraser {
                InputKind::PenEraser
            } else {
                InputKind::Pen
            },
            pressure: if measures(PEN_MASK_PRESSURE) {
                pen.pressure as f32 / MAX_PRESSURE
            } else if in_contact {
                1.0
            } else {
                0.0
            },
            tilt_x: if measures(PEN_MASK_TILT_X) {
                pen.tiltX as f32
            } else {
                0.0
            },
            tilt_y: if measures(PEN_MASK_TILT_Y) {
                pen.tiltY as f32
            } else {
                0.0
            },
            rotation: if measures(PEN_MASK_ROTATION) {
                pen.rotation as f32
            } else {
                0.0
            },
            ..PenSample::default()
        };

        let event = InkEvent {
            sample,
            barrel_button: pen.penFlags & PEN_FLAG_BARREL != 0,
            has_pressure: measures(PEN_MASK_PRESSURE),
            has_tilt: measures(PEN_MASK_TILT_X) || measures(PEN_MASK_TILT_Y),
            ..InkEvent::default()
        };
        Some((pen.pointerInfo, event))
    }

    fn read_touch(pointer_id: u32) -> Option<(POINTER_INFO, InkEvent)> {
        let mut info = POINTER_INFO::default();
        // SAFETY: `info` is a valid place for Windows to write the result.
        unsafe { GetPointerInfo(pointer_id, &mut info) }.ok()?;

        // Most touchscreens don't measure pressure: a finger is either on
        // the screen (1.0) or not (0.0).
        let in_contact = info.pointerFlags.contains(POINTER_FLAG_INCONTACT);
        let sample = PenSample {
            kind: InputKind::Touch,
            pressure: if in_contact { 1.0 } else { 0.0 },
            ..PenSample::default()
        };
        Some((
            info,
            InkEvent {
                sample,
                ..InkEvent::default()
            },
        ))
    }

    /// Asks Windows for a device's product name, e.g. "Wacom Intuos S".
    fn device_name(device: HANDLE) -> String {
        let mut info = POINTER_DEVICE_INFO::default();
        // SAFETY: `info` is a valid place for Windows to write the result.
        if unsafe { GetPointerDevice(device, &mut info) }.is_err() {
            return String::new();
        }
        // productString is UTF-16 text ending at the first 0.
        let text = &info.productString;
        let len = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        String::from_utf16_lossy(&text[..len])
    }
}
