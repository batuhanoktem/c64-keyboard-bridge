//! The standard application-menu Quit action must reach the same draft and
//! cleanup decision as the tray. AppKit's default terminate: skips that path.
use anyhow::{Context, Result};
use objc2::{
    DefinedClass, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};
use std::sync::Arc;

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
        #[unsafe(method(requestC64BridgeQuit:))]
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
}
impl QuitMenu {
    pub fn install(callback: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        let mtm =
            MainThreadMarker::new().context("The application menu requires the main thread")?;
        let menu = NSApplication::sharedApplication(mtm)
            .mainMenu()
            .context("The native application menu is unavailable")?;
        let (menu, index, item) =
            find_quit(&menu).context("The native application Quit action is unavailable")?;
        let original_action = item.action();
        let original_target = item.target();
        let target = QuitTarget::new(mtm, callback);
        // SAFETY: The retained target implements this selector with AppKit's
        // one-object action signature. It remains alive until the action is restored.
        unsafe {
            item.setTarget(Some(&target));
            item.setAction(Some(sel!(requestC64BridgeQuit:)));
        }
        Ok(Self {
            item,
            menu,
            index,
            _target: target,
            original_action,
            original_target,
        })
    }
    /// Exercise the actual installed menu item in software-only acceptance;
    /// this does not synthesize or capture any keyboard input.
    pub fn perform_quit(&self) {
        self.menu.performActionForItemAtIndex(self.index);
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
