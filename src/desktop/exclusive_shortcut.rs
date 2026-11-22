//! Exclusive Carbon registration for the single everyday capture shortcut.
//! The session hotkey library requests shared registrations on macOS.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use std::{
    cell::RefCell,
    ffi::c_void,
    ptr::{self, NonNull},
};

type EventRef = *mut c_void;
const SIGNATURE: u32 = u32::from_be_bytes(*b"FrCp");
const HOT_KEY_PRESSED: EventType = EventType {
    class: u32::from_be_bytes(*b"keyb"),
    kind: 5,
};

#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}
#[repr(C)]
struct KeyId {
    signature: u32,
    id: u32,
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> EventRef;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: KeyId,
        target: EventRef,
        options: u32,
        handle: *mut EventRef,
    ) -> i32;
    fn UnregisterEventHotKey(handle: EventRef) -> i32;
    fn InstallEventHandler(
        target: EventRef,
        callback: unsafe extern "C" fn(EventRef, EventRef, EventRef) -> i32,
        count: usize,
        events: *const EventType,
        data: EventRef,
        handle: *mut EventRef,
    ) -> i32;
    fn RemoveEventHandler(handle: EventRef) -> i32;
    fn GetEventParameter(
        event: EventRef,
        name: u32,
        kind: u32,
        actual: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: EventRef,
    ) -> i32;
    fn CallNextEventHandler(next: EventRef, event: EventRef) -> i32;
}

struct Callback(Box<dyn Fn(u32)>);

// Carbon delivers this callback on the application event loop. The handler's
// owner keeps the boxed context alive until after RemoveEventHandler.
unsafe extern "C" fn pressed(next: EventRef, event: EventRef, context: EventRef) -> i32 {
    let mut key = KeyId {
        signature: 0,
        id: 0,
    };
    let status = unsafe {
        GetEventParameter(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"hkid"),
            ptr::null_mut(),
            std::mem::size_of::<KeyId>(),
            ptr::null_mut(),
            (&mut key as *mut KeyId).cast(),
        )
    };
    if status != 0 || key.signature != SIGNATURE {
        return unsafe { CallNextEventHandler(next, event) };
    }
    let callback = unsafe { &*context.cast::<Callback>() };
    (callback.0)(key.id);
    0
}

pub(super) struct ExclusiveShortcuts {
    handler: NonNull<c_void>,
    callback: Option<Box<Callback>>,
    registrations: RefCell<Vec<(HotKey, NonNull<c_void>)>>,
}

impl ExclusiveShortcuts {
    // The owner creates, uses and drops this non-Send value on the AppKit main thread.
    pub(super) fn new(deliver: impl Fn(u32) + 'static) -> Result<Self, String> {
        let mut callback = Box::new(Callback(Box::new(deliver)));
        let mut handler = ptr::null_mut();
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                pressed,
                1,
                &HOT_KEY_PRESSED,
                (&mut *callback as *mut Callback).cast(),
                &mut handler,
            )
        };
        if status != 0 {
            return Err(format!(
                "Could not initialise capture shortcuts (macOS {status})."
            ));
        }
        Ok(Self {
            handler: NonNull::new(handler).ok_or("macOS returned no shortcut handler.")?,
            callback: Some(callback),
            registrations: RefCell::new(Vec::new()),
        })
    }

    pub(super) fn register(&self, key: HotKey) -> Result<(), String> {
        let code = virtual_key(key.key)
            .ok_or("This key has no supported Mac virtual key code. Choose another key.")?;
        let mut modifiers = 0;
        for (modifier, mask) in [
            (Modifiers::SUPER, 256),
            (Modifiers::SHIFT, 512),
            (Modifiers::ALT, 2048),
            (Modifiers::CONTROL, 4096),
        ] {
            if key.mods.contains(modifier) {
                modifiers |= mask;
            }
        }
        let mut handle = ptr::null_mut();
        // kEventHotKeyExclusive is required. Zero permits a silent collision
        // between applications, even when RegisterEventHotKey reports success.
        let status = unsafe {
            RegisterEventHotKey(
                code,
                modifiers,
                KeyId {
                    signature: SIGNATURE,
                    id: key.id(),
                },
                GetApplicationEventTarget(),
                1,
                &mut handle,
            )
        };
        if status == -9878 {
            return Err("That shortcut is already in use on this Mac. Record another combination or choose Use menu bar only.".into());
        }
        if status != 0 {
            return Err(format!("macOS could not register that shortcut ({status}). Record another combination or choose Use menu bar only."));
        }
        let handle = NonNull::new(handle).ok_or("macOS returned no shortcut registration.")?;
        self.registrations.borrow_mut().push((key, handle));
        Ok(())
    }

    pub(super) fn release(&self, key: HotKey) -> Result<(), String> {
        let mut registrations = self.registrations.borrow_mut();
        let Some(index) = registrations
            .iter()
            .position(|(registered, _)| *registered == key)
        else {
            return Ok(());
        };
        let status = unsafe { UnregisterEventHotKey(registrations[index].1.as_ptr()) };
        if status != 0 {
            return Err(format!(
                "Could not release the previous shortcut (macOS {status})."
            ));
        }
        registrations.swap_remove(index);
        Ok(())
    }
}

impl Drop for ExclusiveShortcuts {
    fn drop(&mut self) {
        for (_, handle) in self.registrations.get_mut().drain(..) {
            unsafe {
                UnregisterEventHotKey(handle.as_ptr());
            }
        }
        if unsafe { RemoveEventHandler(self.handler.as_ptr()) } != 0 {
            // A handler Carbon could not remove might still reference this
            // allocation. Leave a no-op callback alive in that exceptional case.
            if let Some(mut callback) = self.callback.take() {
                callback.0 = Box::new(|_| {});
                Box::leak(callback);
            }
        }
    }
}

// Physical positions from HIToolbox/Events.h. The recorder limits choices to
// letters, numbers and function keys. Apple defines virtual codes through F20.
fn virtual_key(code: Code) -> Option<u32> {
    Some(match code {
        Code::KeyA => 0x00,
        Code::KeyS => 0x01,
        Code::KeyD => 0x02,
        Code::KeyF => 0x03,
        Code::KeyH => 0x04,
        Code::KeyG => 0x05,
        Code::KeyZ => 0x06,
        Code::KeyX => 0x07,
        Code::KeyC => 0x08,
        Code::KeyV => 0x09,
        Code::KeyB => 0x0B,
        Code::KeyQ => 0x0C,
        Code::KeyW => 0x0D,
        Code::KeyE => 0x0E,
        Code::KeyR => 0x0F,
        Code::KeyY => 0x10,
        Code::KeyT => 0x11,
        Code::Digit1 => 0x12,
        Code::Digit2 => 0x13,
        Code::Digit3 => 0x14,
        Code::Digit4 => 0x15,
        Code::Digit6 => 0x16,
        Code::Digit5 => 0x17,
        Code::Digit9 => 0x19,
        Code::Digit7 => 0x1A,
        Code::Digit8 => 0x1C,
        Code::Digit0 => 0x1D,
        Code::KeyO => 0x1F,
        Code::KeyU => 0x20,
        Code::KeyI => 0x22,
        Code::KeyP => 0x23,
        Code::KeyL => 0x25,
        Code::KeyJ => 0x26,
        Code::KeyK => 0x28,
        Code::KeyN => 0x2D,
        Code::KeyM => 0x2E,
        Code::F17 => 0x40,
        Code::F18 => 0x4F,
        Code::F19 => 0x50,
        Code::F20 => 0x5A,
        Code::F5 => 0x60,
        Code::F6 => 0x61,
        Code::F7 => 0x62,
        Code::F3 => 0x63,
        Code::F8 => 0x64,
        Code::F9 => 0x65,
        Code::F11 => 0x67,
        Code::F13 => 0x69,
        Code::F16 => 0x6A,
        Code::F14 => 0x6B,
        Code::F10 => 0x6D,
        Code::F12 => 0x6F,
        Code::F15 => 0x71,
        Code::F4 => 0x76,
        Code::F2 => 0x78,
        Code::F1 => 0x7A,
        _ => return None,
    })
}
