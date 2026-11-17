// Copyright 2020-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! BlackHole: the view that holds a child web view (`build_as_child`). WebKit docks its inspector
//! inside the superview of the inspected view and lays both out as if the web view filled that
//! superview; with the window's content view as superview the page covered the whole window. The
//! host has the size of the web view, so the docked inspector stays inside the page's frame.

use objc2::{define_class, msg_send, rc::Retained, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_core_foundation::CGRect;
use objc2_foundation::MainThreadMarker;

define_class!(
  #[unsafe(super(NSView))]
  #[name = "WryWebViewHost"]
  pub struct WryWebViewHost;
);

impl WryWebViewHost {
  pub fn new(mtm: MainThreadMarker, frame: CGRect) -> Retained<Self> {
    let this = Self::alloc(mtm).set_ivars(());
    unsafe { msg_send![super(this), initWithFrame: frame] }
  }
}
