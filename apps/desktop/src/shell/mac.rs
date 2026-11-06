use std::cell::Cell;

use anyhow::{Context, Result, ensure};
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, extern_methods,
    msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, Sel},
    sel,
};
use objc2_app_kit::{NSApp, NSApplication, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{NSObjectProtocol, NSString};

use crate::exit::Exit;

#[derive(Default)]
pub struct Ivars {
    handling_send_event: Cell<Bool>,
}

define_class!(
    /// The application class Chromium requires: it tracks event dispatch and routes Quit
    /// through the window, so Chromium shuts down before the process ends.
    #[unsafe(super(NSApplication))]
    #[ivars = Ivars]
    struct ShellApplication;

    impl ShellApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let outer = !self.ivars().handling_send_event.get().as_bool();
            if outer {
                self.ivars().handling_send_event.set(Bool::YES);
            }
            let _: () = unsafe { msg_send![super(self), sendEvent: event] };
            if outer {
                self.ivars().handling_send_event.set(Bool::NO);
            }
        }

        #[unsafe(method(terminate:))]
        unsafe fn terminate(&self, _sender: Option<&AnyObject>) {
            super::quit(Exit::Quit);
        }
    }

    unsafe impl CrAppControlProtocol for ShellApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_handling_send_event(&self, handling: Bool) {
            self.ivars().handling_send_event.set(handling);
        }
    }

    unsafe impl CrAppProtocol for ShellApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CefAppProtocol for ShellApplication {}
);

impl ShellApplication {
    extern_methods! {
        #[unsafe(method(sharedApplication))]
        fn shared() -> Retained<Self>;
    }
}

pub fn install() -> Result<()> {
    let mtm = MainThreadMarker::new().context("SDRmm must start on the main thread")?;
    let _ = ShellApplication::shared();
    let app = NSApp(mtm);
    ensure!(
        app.isKindOfClass(ShellApplication::class()),
        "NSApp was created before the Chromium application class"
    );
    app.setMainMenu(Some(&menu_bar(mtm)));
    Ok(())
}

fn menu_bar(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let command = NSEventModifierFlags::Command;
    let bar = NSMenu::new(mtm);
    let app = submenu(&bar, "SDRmm", mtm);
    app.addItem(&item(mtm, "Hide SDRmm", sel!(hide:), "h", command));
    app.addItem(&item(
        mtm,
        "Hide Others",
        sel!(hideOtherApplications:),
        "h",
        command | NSEventModifierFlags::Option,
    ));
    app.addItem(&item(
        mtm,
        "Show All",
        sel!(unhideAllApplications:),
        "",
        command,
    ));
    app.addItem(&NSMenuItem::separatorItem(mtm));
    app.addItem(&item(mtm, "Quit SDRmm", sel!(terminate:), "q", command));
    let edit = submenu(&bar, "Edit", mtm);
    edit.addItem(&item(mtm, "Undo", sel!(undo:), "z", command));
    edit.addItem(&item(
        mtm,
        "Redo",
        sel!(redo:),
        "z",
        command | NSEventModifierFlags::Shift,
    ));
    edit.addItem(&NSMenuItem::separatorItem(mtm));
    edit.addItem(&item(mtm, "Cut", sel!(cut:), "x", command));
    edit.addItem(&item(mtm, "Copy", sel!(copy:), "c", command));
    edit.addItem(&item(mtm, "Paste", sel!(paste:), "v", command));
    edit.addItem(&item(mtm, "Select All", sel!(selectAll:), "a", command));
    let window = submenu(&bar, "Window", mtm);
    window.addItem(&item(
        mtm,
        "Minimize",
        sel!(performMiniaturize:),
        "m",
        command,
    ));
    window.addItem(&item(mtm, "Zoom", sel!(performZoom:), "", command));
    window.addItem(&item(mtm, "Close", sel!(performClose:), "w", command));
    NSApp(mtm).setWindowsMenu(Some(&window));
    bar
}

fn submenu(bar: &NSMenu, title: &str, mtm: MainThreadMarker) -> Retained<NSMenu> {
    let title = NSString::from_str(title);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&menu));
    bar.addItem(&holder);
    menu
}

fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    key: &str,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(modifiers);
    item
}
