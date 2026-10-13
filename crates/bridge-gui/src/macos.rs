//! The standard application-menu Quit action must reach the same draft and
//! cleanup decision as the tray. Dock/Apple-event termination reaches the same
//! decision through the existing application delegate, whose identity stays intact.
use anyhow::{Context, Result};
use objc2::{
    DefinedClass, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};
use std::{cell::RefCell, mem, ptr, sync::Arc};

struct TerminationCallback {
    delegate: *const AnyObject,
    callback: Arc<dyn Fn() + Send + Sync>,
    previous: Option<Imp>,
}
thread_local! {
    // AppKit invokes delegate methods on the main thread. The hook introduces
    // no ivars and leaves every unrelated delegate callback unchanged.
    static TERMINATION_CALLBACK: RefCell<Option<TerminationCallback>> = const { RefCell::new(None) };
}
type TerminationMethod =
    unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSApplication) -> NSApplicationTerminateReply;

unsafe extern "C-unwind" fn should_terminate(
    delegate: &AnyObject,
    selector: Sel,
    application: &NSApplication,
) -> NSApplicationTerminateReply {
    let (callback, previous) = TERMINATION_CALLBACK.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref().map_or((None, None), |hook| {
            (
                ptr::eq(delegate, hook.delegate).then(|| hook.callback.clone()),
                hook.previous,
            )
        })
    });
    if let Some(callback) = callback {
        callback();
        // Cancellation keeps the normal event loop and draft decision alive.
        // Successful GUI cleanup later exits through eframe's ordinary close.
        NSApplicationTerminateReply::TerminateCancel
    } else if let Some(previous) = previous {
        // SAFETY: Installation verified this exact method signature. Imp is
        // Objective-C's erased function pointer representation of that method.
        let previous = unsafe { mem::transmute::<Imp, TerminationMethod>(previous) };
        unsafe { previous(delegate, selector, application) }
    } else {
        NSApplicationTerminateReply::TerminateNow
    }
}
unsafe extern "C-unwind" fn default_should_terminate(
    _: &AnyObject,
    _: Sel,
    _: &NSApplication,
) -> NSApplicationTerminateReply {
    NSApplicationTerminateReply::TerminateNow
}
fn erased(method: TerminationMethod) -> Imp {
    // SAFETY: Both are Objective-C function pointer representations. The runtime
    // receives the matching explicit type encoding before calling this method.
    unsafe { mem::transmute::<TerminationMethod, Imp>(method) }
}

struct TerminationGuard {
    _delegate: Retained<AnyObject>,
    class: &'static AnyClass,
    previous: Option<Imp>,
}
impl TerminationGuard {
    fn install(application: &NSApplication, callback: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        anyhow::ensure!(
            TERMINATION_CALLBACK.with(|slot| slot.borrow().is_none()),
            "The native termination hook is already installed"
        );
        // SAFETY: NSApplication's existing delegate is an Objective-C object.
        // Erasing its static protocol type preserves ownership and identity.
        let delegate: Retained<AnyObject> = unsafe {
            Retained::cast_unchecked(
                application
                    .delegate()
                    .context("The application delegate is unavailable")?,
            )
        };
        let class = delegate.class();
        let selector = sel!(applicationShouldTerminate:);
        let previous = class
            .instance_method(selector)
            .map(|method| method.implementation());
        if previous.is_some() {
            class
                .verify_sel::<(&NSApplication,), NSApplicationTerminateReply>(selector)
                .context("The existing application termination signature is incompatible")?;
        }
        // SAFETY: The 64-bit macOS NSUInteger return and object arguments have
        // encoding Q@:@. No ivars/layout change is made. Add first so an inherited
        // method is never changed in its superclass; otherwise replace only this
        // class's implementation after verifying its ABI above.
        let added = unsafe {
            objc2::ffi::class_addMethod(
                ptr::from_ref(class).cast_mut(),
                selector,
                erased(should_terminate),
                c"Q@:@".as_ptr(),
            )
            .as_bool()
        };
        if !added {
            class
                .verify_sel::<(&NSApplication,), NSApplicationTerminateReply>(selector)
                .context("The application termination signature changed during installation")?;
            let method = class
                .instance_method(selector)
                .context("Could not install the application termination hook")?;
            // SAFETY: The exact selector ABI was verified and remains unchanged.
            unsafe {
                method.set_implementation(erased(should_terminate));
            }
        }
        TERMINATION_CALLBACK.with(|slot| {
            *slot.borrow_mut() = Some(TerminationCallback {
                delegate: &*delegate,
                callback,
                previous,
            });
        });
        Ok(Self {
            _delegate: delegate,
            class,
            previous,
        })
    }
}
impl Drop for TerminationGuard {
    fn drop(&mut self) {
        TERMINATION_CALLBACK.with(|slot| slot.borrow_mut().take());
        if let Some(method) = self
            .class
            .instance_method(sel!(applicationShouldTerminate:))
        {
            // SAFETY: Restore the original compatible implementation, or the
            // default AppKit answer when the optional method was absent. Added
            // Objective-C methods cannot be removed, so the latter restores behavior.
            unsafe {
                method.set_implementation(
                    self.previous
                        .unwrap_or_else(|| erased(default_should_terminate)),
                );
            }
        }
    }
}

struct QuitTargetIvars {
    callback: Arc<dyn Fn() + Send + Sync>,
}
define_class!(
    // SAFETY: NSObject has no subclassing requirements. Actions run on the
    // AppKit main thread; the target and its callback outlive the menu hook.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = QuitTargetIvars]
    struct QuitTarget;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for QuitTarget {}

    impl QuitTarget {
        #[unsafe(method(requestC64KeyboardBridgeQuit:))]
        fn request_quit(&self, _sender: Option<&AnyObject>) { (self.ivars().callback)(); }
    }
);
impl QuitTarget {
    fn new(mtm: MainThreadMarker, callback: Arc<dyn Fn() + Send + Sync>) -> Retained<Self> {
        let allocated = Self::alloc(mtm).set_ivars(QuitTargetIvars { callback });
        // SAFETY: NSObject's initializer is valid for this subclass.
        unsafe { msg_send![super(allocated), init] }
    }
}

pub struct QuitMenu {
    item: Retained<NSMenuItem>,
    menu: Retained<NSMenu>,
    index: isize,
    _target: Retained<QuitTarget>,
    original_action: Option<Sel>,
    original_target: Option<Retained<AnyObject>>,
    _termination: TerminationGuard,
}
impl QuitMenu {
    pub fn install(
        callback: Arc<dyn Fn() + Send + Sync>,
        terminate_callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let mtm =
            MainThreadMarker::new().context("The application menu requires the main thread")?;
        let application = NSApplication::sharedApplication(mtm);
        let menu = application
            .mainMenu()
            .context("The native application menu is unavailable")?;
        let (menu, index, item) =
            find_quit(&menu).context("The native application Quit action is unavailable")?;
        let original_action = item.action();
        let original_target = item.target();
        let target = QuitTarget::new(mtm, callback);
        let termination = TerminationGuard::install(&application, terminate_callback)?;
        // SAFETY: The retained target implements this selector with AppKit's
        // one-object action signature. It remains alive until the action is restored.
        unsafe {
            item.setTarget(Some(&target));
            item.setAction(Some(sel!(requestC64KeyboardBridgeQuit:)));
        }
        Ok(Self {
            item,
            menu,
            index,
            _target: target,
            original_action,
            original_target,
            _termination: termination,
        })
    }
    /// Exercise the actual installed menu item in software-only acceptance;
    /// this does not synthesize or capture any keyboard input.
    pub fn perform_quit(&self) {
        self.menu.performActionForItemAtIndex(self.index);
    }
    /// The same application termination entry point used by Dock/Apple events.
    pub fn perform_application_quit(&self) {
        let application = NSApplication::sharedApplication(self._target.mtm());
        application.terminate(None);
    }
}

/// Confirm guard removal restores the delegate's default answer without
/// terminating the process or synthesizing an operating-system event.
pub fn termination_hook_restored() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let application = NSApplication::sharedApplication(mtm);
    // SAFETY: This method was installed with this exact signature and no ivars.
    // The captured delegate remains the same object throughout the event loop.
    unsafe {
        application.delegate().is_some_and(|delegate| {
            let reply: NSApplicationTerminateReply =
                msg_send![&*delegate, applicationShouldTerminate: &*application];
            reply == NSApplicationTerminateReply::TerminateNow
        })
    }
}
impl Drop for QuitMenu {
    fn drop(&mut self) {
        // SAFETY: Restore the captured action/target while both retained menu
        // objects and this hook target are still alive, on the AppKit main thread.
        unsafe {
            self.item.setAction(self.original_action);
            self.item.setTarget(self.original_target.as_deref());
        }
    }
}
fn find_quit(menu: &NSMenu) -> Option<(Retained<NSMenu>, isize, Retained<NSMenuItem>)> {
    for index in 0..menu.numberOfItems() {
        let item = menu.itemAtIndex(index)?;
        if item.action() == Some(sel!(terminate:)) {
            return Some((menu.retain(), index, item));
        }
        if let Some(submenu) = item.submenu()
            && let Some(found) = find_quit(&submenu)
        {
            return Some(found);
        }
    }
    None
}
