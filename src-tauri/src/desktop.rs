//! Small Win32 helpers for living on the desktop.
//!
//! "Show desktop" (Win+D, or the button at the end of the taskbar) does not
//! minimise everything; it raises the desktop window (`Progman`, or `WorkerW`
//! on older builds) above all normal windows. A widget pinned to the bottom of
//! the z-order ends up underneath it and disappears. Always-on-top windows
//! stay visible, so while the desktop is raised we lift the widget and drop it
//! back afterwards.
//!
//! In wallpaper mode the widget instead becomes part of the desktop itself: it
//! is put inside the window that holds the wallpaper, behind the icons, the
//! way animated-wallpaper programs do it. There it needs no lifting, but it
//! cannot be clicked, because the icon layer in front takes the mouse.
//!
//! Whatever is inside the desktop is destroyed along with it when Explorer
//! exits. So that the widget's window is not lost that way, it is not put
//! there directly: an empty, see-through "carrier" window of ours, as large
//! as the widget, goes into the desktop and holds the widget. A window is told that it is being
//! destroyed before its contents are, and the carrier uses that moment to
//! move the widget back out.
//!
//! That covers a desktop that is closed in an orderly way. When Explorer is
//! killed or crashes, nobody is told anything: carrier and widget are simply
//! gone. The app notices that (`exists`) and starts itself again; a window
//! that vanished like this cannot be replaced, and the window toolkit fails
//! when it is asked about it.

/// A native window handle.
pub type Hwnd = *mut std::ffi::c_void;

#[cfg(windows)]
mod win {
    use super::Hwnd;
    use std::{
        ffi::c_void,
        ptr::{null, null_mut},
        sync::{
            atomic::{AtomicPtr, Ordering},
            Once,
        },
    };

    #[link(name = "user32")]
    extern "system" {
        fn GetTopWindow(hwnd: Hwnd) -> Hwnd;
        fn GetWindow(hwnd: Hwnd, cmd: u32) -> Hwnd;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn GetClassNameW(hwnd: Hwnd, buf: *mut u16, len: i32) -> i32;
        fn GetAsyncKeyState(key: i32) -> i16;
        fn GetSystemMetrics(index: i32) -> i32;
        fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
        fn FindWindowExW(parent: Hwnd, after: Hwnd, class: *const u16, title: *const u16) -> Hwnd;
        fn SendMessageTimeoutW(
            hwnd: Hwnd,
            msg: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            timeout: u32,
            result: *mut usize,
        ) -> isize;
        fn GetDesktopWindow() -> Hwnd;
        fn GetAncestor(hwnd: Hwnd, kind: u32) -> Hwnd;
        fn SetParent(child: Hwnd, parent: Hwnd) -> Hwnd;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
        fn SetLayeredWindowAttributes(hwnd: Hwnd, key: u32, alpha: u8, flags: u32) -> i32;
        fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32)
            -> i32;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn ScreenToClient(hwnd: Hwnd, point: *mut Point) -> i32;
        fn SetPropW(hwnd: Hwnd, name: *const u16, value: *mut c_void) -> i32;
        fn RemovePropW(hwnd: Hwnd, name: *const u16) -> *mut c_void;
        fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn IsWindow(hwnd: Hwnd) -> i32;
        fn RegisterClassW(class: *const WindowClass) -> u16;
        fn CreateWindowExW(
            extended: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: Hwnd,
            menu: *mut c_void,
            instance: *mut c_void,
            param: *mut c_void,
        ) -> Hwnd;
        fn DestroyWindow(hwnd: Hwnd) -> i32;
        fn DefWindowProcW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn OpenProcess(access: u32, inherit: i32, id: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmEnableBlurBehindWindow(hwnd: Hwnd, blur: *const BlurBehind) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateRectRgn(left: i32, top: i32, right: i32, bottom: i32) -> *mut c_void;
        fn DeleteObject(object: *mut c_void) -> i32;
    }

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct WindowClass {
        style: u32,
        procedure: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
        class_extra: i32,
        window_extra: i32,
        instance: *mut c_void,
        icon: *mut c_void,
        cursor: *mut c_void,
        background: *mut c_void,
        menu: *const u16,
        name: *const u16,
    }

    #[repr(C)]
    struct BlurBehind {
        flags: u32,
        enable: i32,
        region: *mut c_void,
        transition_on_maximized: i32,
    }

    const GW_HWNDNEXT: u32 = 2;
    const VK_LBUTTON: i32 = 0x01;
    const VK_RBUTTON: i32 = 0x02;
    const SM_SWAPBUTTON: i32 = 23;
    const GW_CHILD: u32 = 5;
    const GA_PARENT: u32 = 1;
    const WM_DESTROY: u32 = 0x0002;
    /// The right to wait for a process to end.
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
    const CARRIER_CLASS: &str = "TokenTracker.Carrier";
    const GWL_STYLE: i32 = -16;
    const GWL_EXSTYLE: i32 = -20;
    /// Title bar, system menu, sizing border and the minimise/maximise boxes.
    const WS_FRAME_PARTS: isize = 0x00C0_0000 | 0x0008_0000 | 0x0004_0000 | 0x0003_0000;
    /// Where an embedded window keeps the style it had on its own.
    const STYLE_PROP: &str = "TokenTracker.Style";
    const WS_EX_LAYERED: isize = 0x0008_0000;
    const LWA_ALPHA: u32 = 2;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    const DWM_BB_ENABLE: u32 = 1;
    const DWM_BB_BLURREGION: u32 = 2;
    /// Undocumented: asks the shell to create its wallpaper layer.
    const WM_SPAWN_WALLPAPER_LAYER: u32 = 0x052C;
    /// Guards against a z-order that changes under us and never terminates.
    const MAX_WINDOWS: usize = 10_000;

    fn is_desktop_class(hwnd: Hwnd) -> bool {
        let mut buf = [0u16; 32];
        let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        let class = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
        class == "Progman" || class == "WorkerW"
    }

    /// Normally the desktop is the last visible window in the z-order. It has
    /// been raised exactly when some other visible window sits below it.
    ///
    /// (Looking at what is *above* the desktop instead is fragile: tooltips and
    /// other small popups of unrelated apps can float there.)
    pub fn is_desktop_shown() -> bool {
        let mut below_desktop = false;
        unsafe {
            let mut hwnd = GetTopWindow(null_mut());
            for _ in 0..MAX_WINDOWS {
                if hwnd.is_null() {
                    break;
                }
                if IsWindowVisible(hwnd) != 0 {
                    if is_desktop_class(hwnd) {
                        below_desktop = true;
                    } else if below_desktop {
                        return true;
                    }
                }
                hwnd = GetWindow(hwnd, GW_HWNDNEXT);
            }
        }
        false
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// The next window of a class inside `parent` (null: among all windows),
    /// searching on from `after`.
    unsafe fn find(parent: Hwnd, after: Hwnd, class: &str) -> Hwnd {
        FindWindowExW(parent, after, wide(class).as_ptr(), null())
    }

    /// Where a window has to go to sit between the wallpaper and the icons.
    struct Host {
        parent: Hwnd,
        /// The sibling to sit right below.
        below: Hwnd,
    }

    /// Windows 11 24H2 and later: the icons and the wallpaper layer are both
    /// inside Progman, icons on top. Earlier versions arrange the desktop
    /// differently and are not supported.
    unsafe fn find_host() -> Option<Host> {
        let progman = FindWindowW(wide("Progman").as_ptr(), null());
        if progman.is_null() {
            return None;
        }
        let icons = find(progman, null_mut(), "SHELLDLL_DefView");
        let wallpaper = find(progman, null_mut(), "WorkerW");
        (!icons.is_null() && !wallpaper.is_null()).then_some(Host {
            parent: progman,
            below: icons,
        })
    }

    unsafe fn host() -> Option<Host> {
        if let Some(host) = find_host() {
            return Some(host);
        }
        let progman = FindWindowW(wide("Progman").as_ptr(), null());
        if progman.is_null() {
            return None;
        }
        SendMessageTimeoutW(progman, WM_SPAWN_WALLPAPER_LAYER, 0xD, 1, 0, 1000, null_mut());
        find_host()
    }

    /// The carrier window, once created. Only touched on the main thread.
    static CARRIER: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

    unsafe extern "system" fn carrier_procedure(
        hwnd: Hwnd,
        msg: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        if msg == WM_DESTROY {
            // The desktop is going away and takes the carrier along. Its
            // contents would be destroyed next; move them out first.
            loop {
                let inside = GetWindow(hwnd, GW_CHILD);
                if inside.is_null() {
                    break;
                }
                release(inside);
                if GetAncestor(inside, GA_PARENT) == hwnd {
                    break;
                }
            }
            let _ = CARRIER.compare_exchange(
                hwnd,
                null_mut(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// Let what is behind a window show through wherever it paints nothing.
    /// This only takes on a window that is on its own at the time.
    unsafe fn make_see_through(hwnd: Hwnd) {
        let region = CreateRectRgn(0, 0, -1, -1);
        let blur = BlurBehind {
            flags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
            enable: 1,
            region,
            transition_on_maximized: 0,
        };
        DwmEnableBlurBehindWindow(hwnd, &blur);
        DeleteObject(region);
    }

    /// Put a window that is inside `parent` at a place given in screen
    /// coordinates.
    unsafe fn place(hwnd: Hwnd, parent: Hwnd, rect: &Rect, below: Hwnd, flags: u32) {
        let mut corner = Point {
            x: rect.left,
            y: rect.top,
        };
        ScreenToClient(parent, &mut corner);
        SetWindowPos(
            hwnd,
            below,
            corner.x,
            corner.y,
            rect.right - rect.left,
            rect.bottom - rect.top,
            flags | SWP_NOACTIVATE,
        );
    }

    /// The carrier, inside `host` right below the icons, covering `rect` of
    /// the screen. It paints nothing and is see-through.
    unsafe fn carrier(host: &Host, rect: &Rect) -> Hwnd {
        static REGISTER: Once = Once::new();
        let class = wide(CARRIER_CLASS);
        let instance = GetModuleHandleW(null());
        REGISTER.call_once(|| {
            RegisterClassW(&WindowClass {
                style: 0,
                procedure: carrier_procedure,
                class_extra: 0,
                window_extra: 0,
                instance,
                icon: null_mut(),
                cursor: null_mut(),
                background: null_mut(),
                menu: null(),
                name: class.as_ptr(),
            });
        });

        let mut carrier = CARRIER.load(Ordering::Relaxed);
        if !carrier.is_null() && GetAncestor(carrier, GA_PARENT) != host.parent {
            // Left over from a desktop that is gone.
            if IsWindow(carrier) != 0 {
                DestroyWindow(carrier);
            }
            carrier = null_mut();
        }
        if carrier.is_null() {
            // Created on its own and moved in afterwards, like the widget:
            // being layered and see-through has to be set up beforehand.
            carrier = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED as u32,
                class.as_ptr(),
                null(),
                WS_POPUP,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            );
            if carrier.is_null() {
                return carrier;
            }
            make_see_through(carrier);
            SetLayeredWindowAttributes(carrier, 0, 255, LWA_ALPHA);
            SetParent(carrier, host.parent);
            CARRIER.store(carrier, Ordering::Relaxed);
        }
        place(carrier, host.parent, rect, host.below, SWP_SHOWWINDOW);
        carrier
    }

    /// The carrier a window is inside of, if it is.
    unsafe fn carrier_of(hwnd: Hwnd) -> Option<Hwnd> {
        let carrier = CARRIER.load(Ordering::Relaxed);
        (!carrier.is_null() && GetAncestor(hwnd, GA_PARENT) == carrier).then_some(carrier)
    }

    /// Wait until the process with this id has ended, but not for long.
    pub fn wait_for_exit(id: u32, milliseconds: u32) {
        unsafe {
            let process = OpenProcess(SYNCHRONIZE, 0, id);
            if !process.is_null() {
                WaitForSingleObject(process, milliseconds);
                CloseHandle(process);
            }
        }
    }

    /// False once the window has been destroyed.
    pub fn exists(hwnd: Hwnd) -> bool {
        unsafe { IsWindow(hwnd) != 0 }
    }

    /// True when the window is inside another window rather than on its own.
    pub fn is_embedded(hwnd: Hwnd) -> bool {
        unsafe {
            let parent = GetAncestor(hwnd, GA_PARENT);
            !parent.is_null() && parent != GetDesktopWindow()
        }
    }

    /// Make the window part of the desktop, behind the icons, without moving
    /// it on screen. False when this desktop offers no place for it. Call on
    /// the thread that owns the window.
    pub fn embed(hwnd: Hwnd) -> bool {
        unsafe {
            let Some(host) = host() else { return false };
            let mut rect = Rect::default();
            if GetWindowRect(hwnd, &mut rect) == 0 {
                return false;
            }
            let carrier = carrier(&host, &rect);
            if carrier.is_null() {
                return false;
            }
            if GetAncestor(hwnd, GA_PARENT) != carrier {
                // The desktop keeps no picture of its own for its contents to
                // draw into, so the window must be composed separately
                // ("layered"). That also keeps its transparent parts
                // transparent.
                let extended = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, extended | WS_EX_LAYERED);
                SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
                // A window on its own leaves its frame to the system, which
                // draws none for a borderless one. Inside another window it
                // would paint an old-style frame over itself, so it gives up
                // the frame styles.
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                SetPropW(hwnd, wide(STYLE_PROP).as_ptr(), style as *mut c_void);
                SetWindowLongPtrW(hwnd, GWL_STYLE, style & !WS_FRAME_PARTS);
                SetParent(hwnd, carrier);
            }
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            GetAncestor(hwnd, GA_PARENT) == carrier
        }
    }

    /// After an embedded window changed size, make its carrier match, or the
    /// window would be cut off at the carrier's edge.
    pub fn refit(hwnd: Hwnd) {
        unsafe {
            let Some(carrier) = carrier_of(hwnd) else { return };
            let (mut have, mut want) = (Rect::default(), Rect::default());
            GetClientRect(carrier, &mut have);
            GetWindowRect(hwnd, &mut want);
            let (width, height) = (want.right - want.left, want.bottom - want.top);
            if (have.right, have.bottom) != (width, height) {
                let flags = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE;
                SetWindowPos(carrier, null_mut(), 0, 0, width, height, flags);
            }
        }
    }

    /// Move an embedded window to a screen position. False when the window is
    /// not embedded and has to be moved the ordinary way.
    pub fn move_embedded(hwnd: Hwnd, x: i32, y: i32) -> bool {
        unsafe {
            let Some(carrier) = carrier_of(hwnd) else { return false };
            let rect = Rect {
                left: x,
                top: y,
                right: x,
                bottom: y,
            };
            let parent = GetAncestor(carrier, GA_PARENT);
            place(carrier, parent, &rect, null_mut(), SWP_NOSIZE | SWP_NOZORDER);
            true
        }
    }

    /// Make an embedded window an ordinary one again, in place.
    unsafe fn release(hwnd: Hwnd) {
        let mut rect = Rect::default();
        GetWindowRect(hwnd, &mut rect);
        SetParent(hwnd, null_mut());
        let extended = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, extended & !WS_EX_LAYERED);
        let style = RemovePropW(hwnd, wide(STYLE_PROP).as_ptr()) as isize;
        if style != 0 {
            SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        }
        SetWindowPos(
            hwnd,
            null_mut(),
            rect.left,
            rect.top,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        // Set up once at creation; again in case the round trip dropped it.
        make_see_through(hwnd);
    }

    /// Undo `embed`. Call on the thread that owns the window.
    pub fn lift(hwnd: Hwnd) {
        if !is_embedded(hwnd) {
            return;
        }
        unsafe {
            release(hwnd);
            let carrier = CARRIER.swap(null_mut(), Ordering::Relaxed);
            if !carrier.is_null() && IsWindow(carrier) != 0 {
                DestroyWindow(carrier);
            }
        }
    }

    /// True while the primary mouse button is held, e.g. during a window drag.
    pub fn is_dragging() -> bool {
        unsafe {
            let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
                VK_RBUTTON
            } else {
                VK_LBUTTON
            };
            GetAsyncKeyState(key) < 0
        }
    }
}

#[cfg(windows)]
pub use win::{
    embed, exists, is_desktop_shown, is_dragging, is_embedded, lift, move_embedded, refit,
    wait_for_exit,
};

#[cfg(not(windows))]
pub fn is_desktop_shown() -> bool {
    false
}

#[cfg(not(windows))]
pub fn is_dragging() -> bool {
    false
}

#[cfg(not(windows))]
pub fn exists(_: Hwnd) -> bool {
    true
}

#[cfg(not(windows))]
pub fn wait_for_exit(_: u32, _: u32) {}

#[cfg(not(windows))]
pub fn is_embedded(_: Hwnd) -> bool {
    false
}

#[cfg(not(windows))]
pub fn move_embedded(_: Hwnd, _: i32, _: i32) -> bool {
    false
}

#[cfg(not(windows))]
pub fn embed(_: Hwnd) -> bool {
    false
}

#[cfg(not(windows))]
pub fn lift(_: Hwnd) {}

#[cfg(not(windows))]
pub fn refit(_: Hwnd) {}
