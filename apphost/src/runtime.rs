use std::{collections::HashMap, path::Path, error::Error, fs, io, env, ops::Deref};

use serde::Deserialize;
use netcorehost::{nethost, pdcstring::PdCString, hostfxr::Hostfxr, error::HostingError, bindings::char_t};

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct Sha512Hash(#[serde(with="hex::serde")] pub [u8; 64]);

#[derive(Deserialize, Debug, Clone)]
pub struct RuntimeDescriptor {
    #[serde(rename="version")]
    pub version: String,

    #[serde(rename="download")]
    pub download_url: String,

    #[serde(rename="download-sha512")]
    pub download_sha512: Option<Sha512Hash>,

    #[serde(rename="download-format")]
    pub download_format: RuntimeDownloadFormat
}

#[derive(Deserialize, Debug, Clone)]
pub enum RuntimeDownloadFormat {
    #[serde(rename="targz")] TarGz,
    #[serde(rename="zip")] Zip
}

#[derive(thiserror::Error, Debug)]
pub enum RuntimeError {
    #[error("Failed to parse the 'piton-runtime.yaml' runtime descriptor file: {0}")]
    RuntimeFileParse(Box<dyn Error>),

    #[error("Current runtime target '{0}' is not supported")]
    UnsupportedTarget(String)
}

pub fn read_runtime_descr(runtimes_file: &Path, target_id: &str) -> Result<RuntimeDescriptor, RuntimeError> {
    //Parse the runtimes file
    let runtimes = {
        let runtimes_file = match fs::File::open(runtimes_file) {
            Ok(f) => f,
            Err(e) => return Err(RuntimeError::RuntimeFileParse(Box::new(e)))
        };
    
        match serde_yaml::from_reader::<fs::File, HashMap<String, RuntimeDescriptor>>(runtimes_file) {
            Ok(r) => r,
            Err(e) => return Err(RuntimeError::RuntimeFileParse(Box::new(e)))
        }
    };

    //Get the runtime
    match runtimes.get(target_id) {
        Some(r) => Ok(r.clone()),
        None => Err(RuntimeError::UnsupportedTarget(String::from(target_id)))
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeCheckResult {
    NotARuntime,
    IDParseError,
    WrongTarget(String),
    WrongVersion(String),
    Compatible
}

pub fn check_runtime_install(runtime_dir: &Path, runtime_descr: &RuntimeDescriptor, target_id: &str) -> RuntimeCheckResult {
    //Check if the runtime directory contains a piton-runtime-id.txt file with the wanted runtime ID
    let dir_id_str = match fs::read_to_string(runtime_dir.join("piton-runtime-id.txt")) {
        Ok(id) => id,
        Err(_) => return RuntimeCheckResult::NotARuntime
    };

    //Parse the ID file contents
    let mut dir_id_split = dir_id_str.split_ascii_whitespace();
    let Some(dir_target_id) = dir_id_split.next() else { return RuntimeCheckResult::IDParseError; };
    let Some(dir_runtime_ver) = dir_id_split.next() else { return RuntimeCheckResult::IDParseError; };
    if dir_id_split.next().is_some() { return RuntimeCheckResult::IDParseError; }

    //Check for compatibility
    if dir_target_id != target_id {
        RuntimeCheckResult::WrongTarget(String::from(dir_target_id))
    } else if dir_runtime_ver != runtime_descr.version {
        RuntimeCheckResult::WrongVersion(String::from(dir_runtime_ver))
    } else {
        RuntimeCheckResult::Compatible
    }
}

pub fn write_runtime_id(runtime_dir: &Path, target_id: &str, runtime_descr: &RuntimeDescriptor) -> io::Result<()> {
    fs::write(runtime_dir.join("piton-runtime-id.txt"), format!("{target_id} {ver}", ver=runtime_descr.version))
}

pub struct AppInfo<'a> {
    pub app_path: &'a Path,
    pub bundle_offset: i64
}

impl AppInfo<'_> {
    pub const fn is_bundle(&self) -> bool { self.bundle_offset > 0 }
}

pub fn launch_app_binary(runtime_dir: Option<&Path>, app_info: &AppInfo) -> Result<i32, Box<dyn Error>> {
    //Load the hostfxr library
    let dotnet_root: PdCString;
    let hostfxr: Hostfxr;
    if let Some(runtime_dir) = runtime_dir {
        dotnet_root = PdCString::from_os_str(runtime_dir)?;
        hostfxr = nethost::load_hostfxr_with_dotnet_root(&dotnet_root)?;
    } else {
        //Use the system hostfxr
        //Note that we do not support self-contained apps, so we don't have to pass the application root to check for those
        hostfxr = nethost::load_hostfxr()?;

        //This is rather jank since it assumes a particular layout of the runtime root
        //However, hostfxr_resolver_t::dotnet_root() isn't exposed by the nethost library ._.
        let hostfxr_path = nethost::get_hostfxr_path()?;
        let hostfxr_path: &Path = hostfxr_path.as_ref();

        let hostfxr_dir = hostfxr_path.parent().ok_or("hostfxr library path has no parent directory")?;

        let fxr_dir = hostfxr_dir.parent().ok_or("hostfxr library directory has no parent directory")?;
        if !fxr_dir.file_name().map_or(false, |n| n.eq("fxr")) { return Err("'fxr' directory is not named 'fxr'".into()); }

        let host_dir = fxr_dir.parent().ok_or("'fxr' directory has no parent directory")?;
        if !host_dir.file_name().map_or(false, |n| n.eq("host")) { return Err("'host' directory is not named 'host'".into()); }

        let root_dir = host_dir.parent().ok_or("'host' directory has no parent directory")?;
        dotnet_root = PdCString::from_os_str(root_dir.as_os_str())?;
    }

    let hostfxr = hostfxr.lib.deref();

    //Discard printed error messages as they only clutter up our own log message
    //We handle errors based on the hostfxr return codes
    unsafe {
        extern "C" fn nop_writer(_err: *const char_t) {}
        hostfxr.hostfxr_set_error_writer(nop_writer);
    }

    //Run the app
    let host_path = PdCString::from_os_str(env::current_exe()?.as_os_str())?;
    let args: Vec<PdCString> = env::args_os().map(PdCString::from_os_str).collect::<Result<_, _>>()?;
    let app_path = PdCString::from_os_str(app_info.app_path.as_os_str())?;

    //Apply required setup or hacks
    prelaunch_setup();

    let res = unsafe {
        let args = args.iter().map(|s| s.as_ptr()).collect::<Vec<_>>();

        if app_info.is_bundle() {
            hostfxr.hostfxr_main_bundle_startupinfo(args.len() as i32, args.as_ptr(), host_path.as_ptr(), dotnet_root.as_ptr(), app_path.as_ptr(), app_info.bundle_offset)
        } else {
            hostfxr.hostfxr_main_startupinfo(args.len() as i32, args.as_ptr(), host_path.as_ptr(), dotnet_root.as_ptr(), app_path.as_ptr())
        }
    }.ok_or("failed to invoke hostfxr main routine")?;

    //Parse the result
    //Note that there's no mechanism to distinguish app return codes from hostfxr ones
    //So if an app returns such an error code, we will think that we failed to execute the app
    //Too bad!
    match HostingError::known_from_status_code(res as u32) {
        Ok(err) => Err(Box::new(err)),
        Err(_) => Ok(res)
    }
}


#[cfg(unix)]
use libc::{sigaction, SIGSEGV, SIG_DFL, sigaltstack, stack_t, SS_DISABLE};

#[cfg(unix)]
use std::{mem, ptr};

#[cfg(unix)]
fn prelaunch_setup() {
    //On Unix it is required for us to manually remove the registered signal handler altstack
    //in order to have a stable runtime, this is due to rust registering a tiny altstack
    //and then the runtime rolling with it, which causes it to overflow it when allocating a large
    //structure, see https://github.com/dotnet/runtime/issues/115438 for more details

    //Removing the SIGSEGV handler is not strictly required, but since we are messing with the
    //altstack we also reset it to the default, for safety

    let mut action: sigaction = unsafe { mem::zeroed() };
    action.sa_sigaction = SIG_DFL;
    unsafe { sigaction(SIGSEGV, &action, ptr::null_mut()) };

    let mut altstack: stack_t = unsafe { mem::zeroed() };
    altstack.ss_flags = SS_DISABLE;
    unsafe { sigaltstack(&altstack, ptr::null_mut()) };
}

#[cfg(not(unix))]
fn prelaunch_setup() {
    //There are not hackfixes needed yet for not(unix)
}

