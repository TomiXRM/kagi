//! Shell-only macOS display accessibility bridge (#1417/#1418, ADR-0173).
#[cfg(target_os = "macos")]
mod macos {
    use block2::RcBlock;
    use gpui::App;
    use objc2::{class, msg_send, runtime::AnyObject};
    use std::{
        cell::{Cell, RefCell},
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    thread_local! {
        static CHANGED: Cell<bool> = const { Cell::new(false) };
        static WAITER: RefCell<Option<Waker>> = const { RefCell::new(None) };
    }

    pub fn init() {
        // SAFETY: sharedWorkspace and these readonly BOOL properties are public
        // AppKit APIs. Startup and notification delivery both run on the main thread.
        let (motion, transparency) = unsafe {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let motion: bool = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
            let transparency: bool =
                msg_send![workspace, accessibilityDisplayShouldReduceTransparency];
            (motion, transparency)
        };
        kagi_ui_core::accessibility::set_system_display(motion, transparency);
    }

    struct DisplayChanged;
    impl Future for DisplayChanged {
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if CHANGED.with(|changed| changed.replace(false)) {
                Poll::Ready(())
            } else {
                WAITER.with(|waiter| *waiter.borrow_mut() = Some(cx.waker().clone()));
                Poll::Pending
            }
        }
    }

    struct Observer {
        center: *mut AnyObject,
        token: *mut AnyObject,
    }
    impl Drop for Observer {
        fn drop(&mut self) {
            // SAFETY: center and token are retained below. Dropping the foreground
            // task removes delivery before releasing either Objective-C object.
            unsafe {
                let _: () = msg_send![self.center, removeObserver: self.token];
                let _: () = msg_send![self.token, release];
                let _: () = msg_send![self.center, release];
            }
            WAITER.with(|waiter| waiter.borrow_mut().take());
        }
    }

    pub fn install(cx: &mut App) {
        let observer = unsafe {
            // SAFETY: NSString init copies the static UTF-8 bytes. The notification
            // center copies the block; mainQueue confines the callback and waker
            // to GPUI's foreground thread. Retain the returned observer until drop.
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let center: *mut AnyObject = msg_send![workspace, notificationCenter];
            let center: *mut AnyObject = msg_send![center, retain];
            let name: *mut AnyObject = msg_send![class!(NSString), alloc];
            let name: *mut AnyObject = msg_send![name, initWithUTF8String: c"NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification".as_ptr()];
            let queue: *mut AnyObject = msg_send![class!(NSOperationQueue), mainQueue];
            let callback = RcBlock::new(|_: *mut AnyObject| {
                init();
                CHANGED.with(|changed| changed.set(true));
                let waiter = WAITER.with(|waiter| waiter.borrow_mut().take());
                if let Some(waiter) = waiter {
                    waiter.wake();
                }
            });
            let token: *mut AnyObject = msg_send![center, addObserverForName: name, object: std::ptr::null_mut::<AnyObject>(), queue: queue, usingBlock: &*callback];
            let token: *mut AnyObject = msg_send![token, retain];
            let _: () = msg_send![name, release];
            Observer { center, token }
        };
        // Close the startup/registration interval with another complete read.
        init();
        cx.spawn(async move |cx| {
            let _observer = observer;
            loop {
                DisplayChanged.await;
                cx.update(|cx| cx.refresh_windows());
            }
        })
        .detach();
    }
}

pub fn init() {
    #[cfg(target_os = "macos")]
    macos::init();
}

pub fn install(cx: &mut gpui::App) {
    #[cfg(target_os = "macos")]
    macos::install(cx);
    #[cfg(not(target_os = "macos"))]
    let _ = cx;
}
