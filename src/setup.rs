use std::{error::Error, net::TcpStream, path::Path, io::{Cursor, self}, fs::{self}};

use bytes::{BytesMut, BufMut, Bytes};
use bytesize::ByteSize;
use flate2::bufread::GzDecoder;
use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Sha512, Digest};
use thiserror::Error;
use tokio::runtime::Runtime;
use url::Url;

use crate::{runtime::{RuntimeDescriptor, RuntimeDownloadFormat, write_runtime_id}, ui::{run_progress_action, ProgressAction}};

type ErrorBox = Box<dyn Error>;
type CrossThreadErrorBox = Box<dyn Error + Send + Sync>;

#[derive(thiserror::Error, Debug)]
pub enum SetupError {
    #[error("Unable to connect to the runtime download server '{server}': {error}")]
    DownloadServerUnreachable{ server: String, error: ErrorBox},

    #[error("Failed to initialize the async runtime: {0}")]
    AsyncRuntimeError(ErrorBox),

    #[error("Failed to open the progress dialog: {0}")]
    ProgressActionError(ErrorBox),

    #[error("Failed to download the runtime: {0}")]
    DownloadError(CrossThreadErrorBox),

    #[error("Mismatching runtime hash - this might indicate that the download has been tampered with! (expected {expected}, got {actual})")]
    DownloadHashMismatch{ expected: String, actual: String },

    #[error("Failed to decompress the runtime: {0}")]
    DecompressError(CrossThreadErrorBox),

    #[error("Failed to finalize the runtime: {0}")]
    FinalizationError(CrossThreadErrorBox),

    #[error("The user cancelled the setup dialog")]
    Cancelled
}

pub enum AsyncSetupError {
    DownloadError(CrossThreadErrorBox),
    DownloadHashMismatch(String, String),
    DecompressError(CrossThreadErrorBox),
    FinalizationError(CrossThreadErrorBox),
}

impl From<AsyncSetupError> for SetupError {
    fn from(value: AsyncSetupError) -> Self {
        match value {
            AsyncSetupError::DownloadError(err) => Self::DownloadError(err),
            AsyncSetupError::DownloadHashMismatch(expected, actual) => Self::DownloadHashMismatch{ expected, actual },
            AsyncSetupError::DecompressError(err) => Self::DecompressError(err),
            AsyncSetupError::FinalizationError(err) => Self::FinalizationError(err)
        }
    }
}

pub fn setup_runtime(target_id: &str, runtime_descr: &RuntimeDescriptor, runtime_dir: &Path) -> Result<(), SetupError> {
    //Check that the download server is reachable
    let download_url = Url::parse(&runtime_descr.download_url).map_err(|e| SetupError::DownloadError(Box::new(e)))?;
    if let Some(download_host) = download_url.host() {
        if let Some(port) = download_url.port_or_known_default() {
            let download_host = download_host.to_string();
            if let Err(e) = TcpStream::connect((download_host.as_ref(), port)) {
                println!("Failed to connect to the download server host: {download_host}");
                return Err(SetupError::DownloadServerUnreachable { server: download_host, error: Box::new(e) });
            }
        }
    }

    //Setup the async runtime
    let async_runtime = Runtime::new().map_err(|e| SetupError::AsyncRuntimeError(Box::new(e)))?;

    //Open the progress dialog
    let diag_descr = format!("Setting up the .NET {} runtime, please wait...", runtime_descr.version);
    let Some(diag_res) = run_progress_action::<Result<(), AsyncSetupError>>(&diag_descr, move |act: &dyn ProgressAction| {
        //Download the runtime archive
        let runtime_data = download_runtime(act, &async_runtime, target_id, runtime_descr).map_err(AsyncSetupError::DownloadError)?;
        if act.is_cancelled() { return Ok(()); }

        //Validate the hash
        let runtime_hash: &[u8] = &Sha512::digest(&runtime_data);
        if !runtime_descr.download_sha512.eq(runtime_hash) {
            let expected_hash = hex::encode(runtime_descr.download_sha512);
            let actual_hash = hex::encode(runtime_hash);
            act.println(&format!("Unexpected download hash: {} != {}", expected_hash, actual_hash));
            return Err(AsyncSetupError::DownloadHashMismatch(expected_hash, actual_hash));
        }
        act.println("Downloaded runtime hash matches expected hash");

        //Decompress it
        match runtime_descr.download_format {
            RuntimeDownloadFormat::TarGz => decompress_targz_runtime(act, runtime_dir, &runtime_data).map_err(AsyncSetupError::DecompressError)?,
            RuntimeDownloadFormat::Zip =>  decompress_zip_runtime(act, runtime_dir, &runtime_data).map_err(AsyncSetupError::DecompressError)?
        }
        if act.is_cancelled() { return Ok(()); }

        act.set_progress("Finalizing", 1_f64);

        //Write the runtime ID file
        write_runtime_id(runtime_dir, target_id, runtime_descr).map_err(|e| AsyncSetupError::FinalizationError(Box::new(e)))?;

        act.println(&format!("Successfully set up runtime version {ver} for target '{target_id}' in '{dir}'", ver=runtime_descr.version, dir=runtime_dir.display()));
        Ok(())
    }).map_err(SetupError::ProgressActionError)? else {
        println!("The user cancelled the operation");
        return Err(SetupError::Cancelled);
    };

    if let Err(e) = diag_res { return Err(SetupError::from(e)); }

    Ok(())
}

fn download_runtime(act: &dyn ProgressAction, async_runtime: &Runtime, target_id: &str, runtime_descr: &RuntimeDescriptor) -> Result<Bytes, CrossThreadErrorBox> {
    async_runtime.block_on(async move {
        //Create a new reqwest client and use it to fetch the runtime URL
        let client = Client::new();
        let resp = client.get(&runtime_descr.download_url).send().await?;

        //Obtain the length of the runtime archive
        let content_len = resp.content_length().ok_or("Download response has no Content-Length")?;

        act.println(&format!("Downloading runtime '{target_id}' from '{}' ({})...", runtime_descr.download_url, ByteSize::b(content_len)));

        let mut data = BytesMut::new();

        //Handle chunks from the response stream
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;

            //Bail if the dialog has been cancelled
            if act.is_cancelled() { return Ok(Bytes::default()); }

            //Append the chunk to the buffer
            data.put_slice(&chunk);

            //Update the progress bar
            act.set_progress(&format!("Downloading runtime '{target_id}': {}/{}", ByteSize::b(data.len() as u64), ByteSize::b(content_len)), (data.len() as f64) / (content_len as f64));
        }
        assert!(data.len() == content_len as usize);

        Ok::<_, CrossThreadErrorBox>(data.into())
    })
}

#[derive(Error, Debug)]
enum SecurityError {
    #[error("Detected attempted file traversal through archive path '{0}'")]
    AttemptedFileTraversal(String)
}

fn decompress_targz_runtime(act: &dyn ProgressAction, runtime_dir: &Path, data: &Bytes) -> Result<(), CrossThreadErrorBox> {
    fs::create_dir_all(runtime_dir)?;
    
    //Initialize the progress bar
    act.set_progress("Determining archive size", 0_f64);

    let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(data)));
    let num_entries = archive.entries()?.count();

    act.println(&format!("Unpacking TAR ({num_entries} entries)..."));
    act.set_progress(&format!("Unpacking archive: 0/{num_entries}"), 0_f64);
    
    //Unpack the TAR
    let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(data)));
    let mut num_unpacked = 0;
    for entry in archive.entries()? {
        let mut entry = entry?;

        //Bail if the dialog has been cancelled
        if act.is_cancelled() { return Ok(()); }

        //Unpack the entry
        if !entry.unpack_in(runtime_dir)? {
            return Err(Box::new(SecurityError::AttemptedFileTraversal(String::from(entry.path()?.to_str().unwrap()))));
        }
        num_unpacked += 1;

        //Update the progress bar
        act.set_progress(&format!("Unpacking archive: {num_unpacked}/{num_entries}"), (num_unpacked as f64) / (num_entries as f64));
    }

    Ok(())
}

fn decompress_zip_runtime(dialog: &dyn ProgressAction, runtime_dir: &Path, data: &Bytes) -> Result<(), CrossThreadErrorBox> {
    fs::create_dir_all(runtime_dir)?;

    //Unpack the ZIP
    let mut archive = zip::ZipArchive::new(Cursor::new(data))?;
    let num_entries = archive.len();

    println!("Unpacking ZIP ({num_entries} entries)...");
    dialog.set_progress(&format!("Unpacking archive: 0/{num_entries}"), 0_f64);

    for idx in 0..num_entries {
        let mut zip_file = archive.by_index(idx)?;
        assert!(zip_file.is_file());

        //Decompress the file
        let Some(zip_path) = zip_file.enclosed_name() else {
            return Err(Box::new(SecurityError::AttemptedFileTraversal(String::from(zip_file.name()))));
        };

        let out_path = runtime_dir.join(zip_path);
        if let Some(out_parent_dir) = out_path.parent() {
            fs::create_dir_all(out_parent_dir)?;
        }

        io::copy(&mut zip_file, &mut fs::File::create(out_path)?)?;

        //Update the progress bar
        dialog.set_progress(&format!("Unpacking archive: {}/{num_entries}", idx+1), (idx as f64) / (num_entries as f64));
    }

    Ok(())
}
