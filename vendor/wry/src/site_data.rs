// BlackHole: what wry does not offer about the permissions, data and downloads of sites.

use std::{path::PathBuf, rc::Rc};

use crate::PermissionKind;

/// A page asking for something. See [`crate::WebViewBuilder::with_site_permission_handler`].
#[derive(Debug, Clone)]
pub struct SitePermissionRequest {
  pub kinds: Vec<PermissionKind>,
  /// The origin of the frame that asks.
  pub origin: String,
  /// The address of the page shown.
  pub page: String,
}

/// Answers a [`SitePermissionRequest`] once. Dropping it unanswered denies the request.
pub struct SitePermissionResponder {
  answer: Option<Box<dyn FnOnce(bool)>>,
}

impl SitePermissionResponder {
  #[allow(dead_code)]
  pub(crate) fn new(answer: impl FnOnce(bool) + 'static) -> Self {
    Self {
      answer: Some(Box::new(answer)),
    }
  }

  pub fn respond(mut self, allow: bool) {
    if let Some(answer) = self.answer.take() {
      answer(allow);
    }
  }
}

impl Drop for SitePermissionResponder {
  fn drop(&mut self) {
    if let Some(answer) = self.answer.take() {
      answer(false);
    }
  }
}

pub type SitePermissionHandler = Box<dyn Fn(SitePermissionRequest, SitePermissionResponder)>;

/// What [`crate::WebView::clear_site_data`] erases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SiteData {
  /// Cookies and what sites store.
  pub cookies: bool,
  /// Images and files of the pages.
  pub cache: bool,
}

#[allow(dead_code)]
pub(crate) trait DownloadOps {
  fn pause(&self) -> bool;
  fn resume(&self) -> bool;
  fn cancel(&self);
  fn can_resume(&self) -> bool;
  fn received(&self) -> Option<u64>;
}

/// A download being made by the engine. See [`crate::WebViewBuilder::with_download_control_handler`].
#[derive(Clone)]
pub struct DownloadControl {
  #[allow(dead_code)]
  pub(crate) ops: Rc<dyn DownloadOps>,
}

impl DownloadControl {
  pub fn pause(&self) -> bool {
    self.ops.pause()
  }
  pub fn resume(&self) -> bool {
    self.ops.resume()
  }
  pub fn cancel(&self) {
    self.ops.cancel();
  }
  /// Whether a paused or interrupted download can go on from where it stopped.
  pub fn can_resume(&self) -> bool {
    self.ops.can_resume()
  }
  pub fn received(&self) -> Option<u64> {
    self.ops.received()
  }
}

/// Called with the destination of each download accepted by the started handler.
pub type DownloadControlHandler = Rc<dyn Fn(PathBuf, DownloadControl)>;
