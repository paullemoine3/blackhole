//! Windows taskbar: the tasks of the app's jump list (right click on its button, or on its
//! pinned icon): "New window" and "New private window". Each one starts the app's own executable
//! with its arguments; every BlackHole window is its own process (`--private`: a private one).

use windows::core::{Interface, HSTRING};
use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
};

/// One task of the jump list: its title and the arguments given to the executable.
pub struct Task {
    pub title: String,
    pub args: String,
}

/// Replaces the tasks of the app's jump list. Runs on its own thread (COM, and the shell writes
/// the list to disk): never on the UI thread. Errors are ignored: the list is a convenience.
pub fn set_tasks(tasks: Vec<Task>) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("bh-jumplist".into())
        .spawn(move || {
            // SAFETY: COM is initialized for this thread only and released before it ends; every
            // object is created and used on this thread.
            unsafe {
                if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                    return;
                }
                let _ = commit(&exe, &tasks);
                CoUninitialize();
            }
        });
}

unsafe fn commit(exe: &std::path::Path, tasks: &[Task]) -> windows::core::Result<()> {
    let list: ICustomDestinationList = CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
    let mut slots = 0u32;
    let _removed: IObjectArray = list.BeginList(&mut slots)?;
    let items: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
    let path = HSTRING::from(exe.as_os_str());
    for task in tasks {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&path)?;
        link.SetArguments(&HSTRING::from(task.args.as_str()))?;
        link.SetIconLocation(&path, 0)?;
        link.SetDescription(&HSTRING::from(task.title.as_str()))?;
        // The title shown in the list is the link's `System.Title`.
        let store: IPropertyStore = link.cast()?;
        store.SetValue(&PKEY_Title, &PROPVARIANT::from(task.title.as_str()))?;
        store.Commit()?;
        items.AddObject(&link)?;
    }
    let array: IObjectArray = items.cast()?;
    list.AddUserTasks(&array)?;
    list.CommitList()
}
