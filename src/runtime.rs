use std::{collections::HashMap, path::{Path, PathBuf}, error::Error, fs, ffi::OsString, io};

use serde::Deserialize;
use netcorehost::{nethost, pdcstring::PdCString};

#[derive(Deserialize, Debug, Clone)]
pub struct RuntimeDescriptor {
    #[serde(rename="version")]
    pub version: String,

    #[serde(rename="download")]
    pub download_url: String,

    #[serde(rename="download-sha512", with="hex::serde")]
    pub download_sha512: [u8; 64],

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
    #[error("Failed to parse the 'everest-runtime.yaml' runtime descriptor file: {0}")]
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
    //Check if the runtime directory contains a everest-runtime-id.txt file with the wanted runtime ID
    let dir_id_str = match fs::read_to_string(runtime_dir.join("everest-runtime-id.txt")) {
        Ok(id) => id,
        Err(_) => return RuntimeCheckResult::NotARuntime
    };

    //Parse the ID file contents
    let mut dir_id_split = dir_id_str.split_ascii_whitespace();
    let Some(dir_target_id) = dir_id_split.next() else { return RuntimeCheckResult::IDParseError; };
    let Some(dir_runtime_ver) = dir_id_split.next() else { return RuntimeCheckResult::IDParseError; };
    if let Some(_) = dir_id_split.next() { return RuntimeCheckResult::IDParseError; }

    //Check for compatibility
    if dir_target_id != target_id {
        return RuntimeCheckResult::WrongTarget(String::from(dir_target_id));
    } else if dir_runtime_ver != runtime_descr.version {
        return RuntimeCheckResult::WrongVersion(String::from(dir_runtime_ver));
    } else {
        return RuntimeCheckResult::Compatible
    }
}

pub fn write_runtime_id(runtime_dir: &Path, target_id: &str, runtime_descr: &RuntimeDescriptor) -> io::Result<()> {
    fs::write(runtime_dir.join("everest-runtime-id.txt"), format!("{target_id} {ver}", ver=runtime_descr.version))
}

pub fn launch_app_binary(runtime_dir: &Path, app_bin_path: &Path, args: &[&str]) -> Result<i32, Box<dyn Error>> {
    //Load the hostfxr library
    let dotnet_root = PdCString::from_os_str(runtime_dir)?;
    let hostfxr = nethost::load_hostfxr_with_dotnet_root(&dotnet_root)?;

    //Initialize the hosting components
    let app_bin_path = PdCString::from_os_str(app_bin_path.as_os_str());
    let args: Vec<PdCString> = std::iter::once(app_bin_path).chain(args.iter().map(PdCString::from_os_str)).collect::<Result<_, _>>()?;
    let ctx = hostfxr.initialize_for_dotnet_command_line_with_args_and_dotnet_root(&args.iter().map(PdCString::as_ref).collect::<Vec<_>>(), dotnet_root)?;

    //Add the runtime root to the path (in case anything tries to run dotnet directly)
    let path_var = std::env::var_os("PATH").unwrap_or(OsString::default());
    let mut path = std::env::split_paths(&path_var).collect::<Vec<_>>();
    path.insert(0, PathBuf::from(runtime_dir));
    std::env::set_var("PATH", std::env::join_paths(path)?);

    //Run the application
    let res = ctx.run_app();
    if let Err(err) = res.as_hosting_exit_code().into_result() { return Err(Box::new(err)); };
    Ok(res.value())
}